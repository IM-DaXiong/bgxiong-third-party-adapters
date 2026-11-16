use std::{env, fs, path::PathBuf};

use bgx_comfy_workflow::{
    canvas::{parse_to_ir, source_digest, CompileContextV1, SourceFormat},
    contract::CONTRACT_VERSION,
    digest_json,
    node_registry::{
        definition::{mapping_definition_digest, normalize_object_info, parse_object_info_text},
        serializer_profile::standard_profile_for,
        RULE_REGISTRY_VERSION,
    },
};
use serde_json::json;

use crate::{compile, execute};

// The certified originals and captured object_info remain in their primary repository.
#[test]
#[ignore = "requires BGX_COMFY_TIMING_FIXTURES pointing to the certified LTX 2.5 fixture directory"]
fn certified_ltx25_preparation_preserves_the_graph_and_applies_two_primitives() {
    let root = PathBuf::from(env::var_os("BGX_COMFY_TIMING_FIXTURES").expect("fixture path"));
    let definitions = fs::read_to_string(root.join("object_info.json")).unwrap();
    let definitions = parse_object_info_text(&definitions).unwrap();
    for (source_name, duration, fps, length, latent) in [
        ("video_ltx2_5_t2v.json", "450", "449", "444", "434"),
        ("video_ltx2_5_i2v.json", "413", "425", "441", "437"),
    ] {
        let source = fs::read_to_string(root.join(source_name)).unwrap();
        let ir = parse_to_ir(source.as_bytes()).unwrap();
        let classes = ir
            .nodes
            .iter()
            .map(|node| node.class_type.clone())
            .collect();
        let definitions = normalize_object_info(&definitions, &classes).unwrap();
        let profile = standard_profile_for(definitions.frontend_version.as_deref())
            .map(|profile| profile.profile_version)
            .unwrap_or_else(|| "none".into());
        let source_format = match ir.format {
            SourceFormat::Canvas04 => "canvas-0.4",
            SourceFormat::Canvas10 => "canvas-1.0",
            _ => panic!("expected certified canvas"),
        };
        let context = CompileContextV1 {
            source_format_version: source_format.into(),
            source_digest: source_digest(source.as_bytes()),
            definitions,
            serializer_profile_version: profile,
            rule_set_version: RULE_REGISTRY_VERSION,
            bridge_evidence: None,
            capability_plan: None,
        };
        let mut input = json!({
            "source":source, "context":context,
            "selection":{"outputKeys":[{"scope":[],"localId":"75"}]},
            "outputBindings":[{"nodeId":"75","kind":"video","collection":"videos"}]
        });
        let base = compile::compile(&source, &input).unwrap();
        assert_eq!(base["status"], "ready", "{}", base["diagnostics"]);
        let mut expected = base["value"]["apiPrompt"].clone();
        assert_eq!(expected[latent]["inputs"]["length"], json!([length, 1]));
        expected[duration]["inputs"]["value"] = json!(7);
        expected[fps]["inputs"]["value"] = json!(32);

        input["timing"] = json!({"seconds":7,"fps":32});
        let (applied, material) = execute::preparation(&input)
            .unwrap_or_else(|error| panic!("{source_name}: {}", error.message));
        assert_eq!(applied, expected, "{source_name}");
        assert_eq!(
            material["timing"],
            json!({"seconds":7,"fps":32,"frames":225})
        );
        if let Some(out) = env::var_os("BGX_COMFY_TIMING_EVIDENCE") {
            let out = PathBuf::from(out);
            fs::write(out.join(source_name), input.to_string()).unwrap();
            fs::write(
                out.join(format!("{source_name}.material.json")),
                material.to_string(),
            )
            .unwrap();
        }

        for (seconds, fps) in [(7, 25), (40, 32), (0, 32), (u32::MAX, 32)] {
            input["timing"] = json!({"seconds":seconds,"fps":fps});
            let error = execute::preparation(&input).unwrap_err();
            assert_eq!(
                error.code,
                super::INVALID_TIMING,
                "{source_name}: {}",
                error.message
            );
        }

        let original = &base["value"]["apiPrompt"];
        input["timing"] = json!({"seconds":7,"fps":32});
        input["scalarMapping"] = json!({"values":[],"contract":{
            "schemaVersion":1,"contractVersion":CONTRACT_VERSION,"status":"ready",
            "bindings":[],"outputSelection":{"nodeId":"75","mediaKind":"video"},
            "snapshot":{"identity":{"schemaVersion":1,"instanceId":"regression","userScope":"fixture",
                "sourceKind":"localFile","sourcePath":source_name,"displayName":"LTX 2.5"},
                "sourceDigest":context.source_digest,"graphDigest":digest_json(original).unwrap(),
                "definitionDigest":mapping_definition_digest(original, &context.definitions).unwrap(),
                "ruleRegistryVersion":RULE_REGISTRY_VERSION,"capturedAtMs":0},
            "fixedWrites":[{"nodeId":length,"inputName":"expression","value":"a"}]
        }});
        let error = execute::preparation(&input).unwrap_err();
        assert_eq!(error.code, super::INVALID_TIMING, "{}", error.message);
    }
}
