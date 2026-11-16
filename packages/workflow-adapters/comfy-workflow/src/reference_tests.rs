use std::{env, fs};

use bgx_comfy_workflow::canvas::{parse_to_ir, source_digest, CompileContextV1};
use bgx_comfy_workflow::node_registry::{
    definition::{normalize_object_info, parse_object_info_text},
    serializer_profile::standard_profile_for,
    RULE_REGISTRY_VERSION,
};
use serde_json::json;

#[test]
#[ignore = "requires the certified source and object_info paths in BGX_COMFY_REFERENCE_SOURCE and BGX_COMFY_REFERENCE_DEFINITIONS"]
fn certified_qwen_edit_preserves_declared_reference_order_and_original_links() {
    let raw = fs::read_to_string(env::var_os("BGX_COMFY_REFERENCE_DEFINITIONS").unwrap()).unwrap();
    let source = fs::read_to_string(env::var_os("BGX_COMFY_REFERENCE_SOURCE").unwrap()).unwrap();
    let graph = parse_to_ir(source.as_bytes()).unwrap();
    let raw = parse_object_info_text(&raw).unwrap();
    let classes = graph
        .nodes
        .iter()
        .map(|node| node.class_type.clone())
        .collect();
    let mut definitions = normalize_object_info(&raw, &classes).unwrap();
    definitions.frontend_version = Some("1.53.10".into());
    definitions.server_version = Some("0.39.2".into());
    let profile = standard_profile_for(definitions.frontend_version.as_deref()).unwrap();
    let context = CompileContextV1 {
        source_format_version: "canvas-0.4".into(),
        source_digest: source_digest(source.as_bytes()),
        definitions,
        serializer_profile_version: profile.profile_version,
        rule_set_version: RULE_REGISTRY_VERSION,
        bridge_evidence: None,
        capability_plan: None,
    };
    let mut input = json!({"source":source,"context":context,
        "selection":{"outputKeys":[{"scope":[],"localId":"461"}]}});
    let baseline = crate::compile::compile(&source, &input).unwrap();
    assert_eq!(baseline["status"], "ready", "{}", baseline["diagnostics"]);
    let baseline_graph = &baseline["value"]["apiPrompt"];
    let mut cases = Vec::new();
    let expected_keys: Vec<_> = (1..=16)
        .map(|index| format!("images.image_{index}"))
        .collect();
    for count in [0, 3, 16, 17] {
        input["referenceCount"] = json!(count);
        let result = super::project(&source, &input).unwrap();
        cases.push(json!({"input":input,"result":result}));
        if !(1..=16).contains(&count) {
            assert_eq!(result["status"], "blocked");
            assert_eq!(
                result["diagnostics"][0]["code"],
                "comfyReferenceImageCountOutOfRange"
            );
            continue;
        }
        assert_eq!(result["status"], "ready", "{}", result["diagnostics"]);
        let response = &result["value"];
        let reference = &response["referenceImage"];
        assert_eq!(response["outputNodeId"], "461");
        assert_eq!(response["sourceDigest"], context.source_digest);
        assert_eq!(
            response["compiledGraphDigest"],
            baseline["value"]["digests"]["graph"]
        );
        assert_eq!(response["selectedInputs"], json!(&expected_keys[..count]));
        assert_eq!(reference["minCount"], 1);
        assert_eq!(reference["maxCount"], 16);
        assert_eq!(reference["parentKey"], "images");
        assert_eq!(reference["slots"].as_array().unwrap().len(), 16);
        let consumer = reference["consumerNodeId"].as_str().unwrap();
        let mut original_wires = 0;
        for (slot, key) in reference["slots"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&expected_keys)
        {
            assert_eq!(slot["inputName"], *key);
            if let Some(link) = baseline_graph[consumer]["inputs"][key].as_array() {
                original_wires += 1;
                assert_eq!(
                    slot["existingLink"],
                    json!({"nodeId":link[0],"outputIndex":link[1]})
                );
            } else {
                assert!(slot["existingLink"].is_null());
            }
        }
        assert!(original_wires > 0);
    }
    if let Some(path) = env::var_os("BGX_COMFY_REFERENCE_EVIDENCE") {
        fs::write(path, json!(cases).to_string()).unwrap();
    }
}
