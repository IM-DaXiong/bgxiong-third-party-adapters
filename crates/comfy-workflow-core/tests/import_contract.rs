//! Offline contract tests for the pure canvas compiler (CW16[L02], CW18–CW27, CW29[L02]).
//! No network, no ComfyUI, no model calls; fixtures are synthetic and inline.

use std::collections::BTreeMap;

use bgx_comfy_workflow::canvas::ir::codes;
use bgx_comfy_workflow::canvas::{
    collect_annotation_units, compile, detect_format, parse_to_ir, source_digest, CapabilityPlanV1,
    CompileContextV1, GraphIrV1, InputValue, NodeKey, NodeMode, OutputSelection, OverrideSource,
    ResolvedCapability, SourceFormat, ValueOrigin,
};
use bgx_comfy_workflow::node_registry::definition::{DefinitionSnapshotV1, NodeDefinitionV1};
use bgx_comfy_workflow::{CompileDiagnostic, DiagnosticSeverity};
use serde_json::{json, Value};

#[test]
fn authored_output_selection_is_metadata_in_an_api_prompt() {
    let source = json!({
        "1": {"class_type":"EmptyImage","inputs":{"width":64,"height":64}},
        "2": {"class_type":"SaveImage","inputs":{"images":["1",0]}},
        "bgxiongOutputSelection":{"version":1,"nodeId":"2"},
    });
    let raw = serde_json::to_vec(&source).unwrap();
    let graph = parse_to_ir(&raw).unwrap();
    assert_eq!(graph.nodes.len(), 2);
    assert!(graph.diagnostics.is_empty());
}

// ---------- helpers ----------

fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).expect("fixture serializes")
}

fn ir_of(v: &Value) -> GraphIrV1 {
    match parse_to_ir(&bytes(v)) {
        Ok(ir) => ir,
        Err(d) => panic!("expected IR, got {d:?}"),
    }
}

fn err_codes(v: &Value) -> Vec<String> {
    match parse_to_ir(&bytes(v)) {
        Ok(ir) => panic!("expected parse failure, got {ir:?}"),
        Err(d) => d.into_iter().map(|d| d.code).collect(),
    }
}

fn key(scope: &[&str], id: &str) -> NodeKey {
    NodeKey { scope: scope.iter().map(|s| s.to_string()).collect(), local_id: id.to_string() }
}

fn has(diags: &[CompileDiagnostic], code: &str) -> bool {
    diags.iter().any(|d| d.code == code)
}

fn node(id: Value, ty: &str, mode: u64, inputs: Value, outputs: Value, widgets: Value) -> Value {
    json!({"id": id, "type": ty, "mode": mode, "pos": [0, 0], "inputs": inputs, "outputs": outputs, "widgets_values": widgets})
}

fn img_out() -> Value {
    json!([{"name": "IMAGE", "type": "IMAGE", "links": []}])
}

fn img_in(name: &str, link: Value) -> Value {
    json!({"name": name, "type": "IMAGE", "link": link})
}

fn canvas(nodes: Value, links: Value) -> Value {
    json!({"version": 0.4, "nodes": nodes, "links": links})
}

fn link_of(ir: &GraphIrV1, k: &NodeKey, input: &str) -> NodeKey {
    match ir.node(k).and_then(|n| n.inputs.get(input)) {
        Some(InputValue::Link(l)) => l.from.clone(),
        other => panic!("{k:?}.{input} is not a link: {other:?}"),
    }
}

// ---------- CW18: format detection ----------

#[test]
fn detects_each_format_structurally() {
    assert_eq!(detect_format(&json!({"1": {"class_type": "A", "inputs": {}}})).unwrap(), SourceFormat::ApiPrompt);
    assert_eq!(
        detect_format(&json!({"1": {"class_type": "A", "inputs": {}, "pos": [0, 0]}})).unwrap(),
        SourceFormat::FlatUi
    );
    assert_eq!(detect_format(&json!({"version": 0.4, "nodes": [], "links": []})).unwrap(), SourceFormat::Canvas04);
    assert_eq!(detect_format(&json!({"version": 1, "nodes": [], "links": []})).unwrap(), SourceFormat::Canvas10);
}

#[test]
fn rejects_mixed_empty_truncated_and_unversioned_sources() {
    let mixed = json!({"1": {"class_type": "A", "inputs": {}, "pos": [0, 0]}, "2": {"class_type": "B", "inputs": {}}});
    assert_eq!(detect_format(&mixed).unwrap_err().code, codes::FORMAT_AMBIGUOUS);
    assert_eq!(detect_format(&json!({})).unwrap_err().code, codes::GRAPH_EMPTY);
    assert_eq!(detect_format(&json!([1, 2])).unwrap_err().code, codes::TOP_LEVEL_NOT_OBJECT);
    assert_eq!(
        detect_format(&json!({"nodes": [], "links": []})).unwrap_err().code,
        codes::FORMAT_VERSION_UNSUPPORTED
    );
    assert_eq!(detect_format(&json!({"version": 0.4, "nodes": {}, "links": []})).unwrap_err().code, codes::FORMAT_UNRECOGNIZED);

    let empty = parse_to_ir(b"  \n").unwrap_err();
    assert_eq!(empty[0].code, codes::JSON_EMPTY);
    let truncated = parse_to_ir(br#"{"1": {"class_type": "A", "inputs": {"#).unwrap_err();
    assert_eq!(truncated[0].code, codes::JSON_PARSE_FAILED);
    assert_eq!(truncated[0].params.get("category").map(String::as_str), Some("eof"));
}

#[test]
fn bom_is_skipped_for_reading_only() {
    let mut raw = vec![0xEF, 0xBB, 0xBF];
    raw.extend(bytes(&json!({"1": {"class_type": "A", "inputs": {}}})));
    let ir = parse_to_ir(&raw).unwrap();
    assert_eq!(ir.format, SourceFormat::ApiPrompt);
    assert_eq!(raw[0], 0xEF, "source bytes are not rewritten");
}

// ---------- CW19: id normalization ----------

#[test]
fn exec_ids_are_reversible_including_separators() {
    for k in [key(&[], "12"), key(&["232"], "10"), key(&["5", "7"], "3"), key(&["a:b"], "50%:x"), key(&[], "uuid-1-2")] {
        let id = k.exec_id();
        assert_eq!(NodeKey::from_exec_id(&id), Some(k));
    }
    assert_eq!(key(&["5", "7"], "3").exec_id(), "5:7:3");
    assert_eq!(NodeKey::from_exec_id("a::b"), None);
    assert_eq!(NodeKey::from_exec_id("a%zz"), None);
    assert_eq!(NodeKey::from_exec_id(""), None);
}

#[test]
fn string_and_integer_ids_follow_the_format_without_default_zero() {
    let big = json!(18446744073709551615u64);
    let g = canvas(
        json!([
            node(json!("c0ffee00-1111-2222-3333-444455556666"), "LoadImage", 0, json!([]), img_out(), json!(["a.png", "image"])),
            node(big.clone(), "SaveImage", 0, json!([img_in("images", json!(1))]), json!([]), json!(["p"]))
        ]),
        json!([[1, "c0ffee00-1111-2222-3333-444455556666", 0, big, 0, "IMAGE"]]),
    );
    let ir = ir_of(&g);
    let save = key(&[], "18446744073709551615");
    assert_eq!(link_of(&ir, &save, "images"), key(&[], "c0ffee00-1111-2222-3333-444455556666"));
}

#[test]
fn integer_and_string_forms_of_one_id_collide() {
    let g = canvas(
        json!([
            node(json!(5), "LoadImage", 0, json!([]), img_out(), json!([])),
            node(json!("5"), "LoadImage", 0, json!([]), img_out(), json!([]))
        ]),
        json!([]),
    );
    assert!(err_codes(&g).contains(&codes::NODE_ID_COLLISION.to_string()));
}

#[test]
fn malformed_ids_and_slots_are_errors_not_zero() {
    let float_id = canvas(json!([node(json!(1.5), "LoadImage", 0, json!([]), img_out(), json!([]))]), json!([]));
    assert!(err_codes(&float_id).contains(&codes::NODE_ID_INVALID.to_string()));

    let missing_slot = json!({"version": 1, "nodes": [
        node(json!(1), "LoadImage", 0, json!([]), img_out(), json!([])),
        node(json!(2), "SaveImage", 0, json!([img_in("images", json!(9))]), json!([]), json!([]))
    ], "links": [{"id": 9, "origin_id": 1, "target_id": 2, "target_slot": 0, "type": "IMAGE"}]});
    assert!(err_codes(&missing_slot).contains(&codes::LINK_INVALID.to_string()));

    let bad_slot = canvas(
        json!([
            node(json!(1), "LoadImage", 0, json!([]), img_out(), json!([])),
            node(json!(2), "SaveImage", 0, json!([img_in("images", json!(3))]), json!([]), json!([]))
        ]),
        json!([[3, 1, 4, 2, 0, "IMAGE"]]),
    );
    let ir = ir_of(&bad_slot);
    assert!(has(&ir.diagnostics, codes::LINK_SLOT_OUT_OF_RANGE));
    assert!(ir.node(&key(&[], "2")).unwrap().inputs.is_empty());
}

#[test]
fn missing_mode_is_not_defaulted() {
    let g = canvas(json!([{"id": 1, "type": "LoadImage", "inputs": [], "outputs": []}]), json!([]));
    let ir = ir_of(&g);
    assert!(has(&ir.diagnostics, codes::NODE_MODE_INVALID));
    assert!(ir.nodes.is_empty());
}

// ---------- CW20 / CW21 / CW22: subgraphs ----------

fn def(id: &str, inputs: Value, outputs: Value, nodes: Value, links: Value) -> Value {
    json!({"id": id, "name": id, "inputs": inputs, "outputs": outputs, "nodes": nodes, "links": links})
}

fn sublink(id: u64, o: i64, os: u64, t: i64, ts: u64, ty: &str) -> Value {
    json!({"id": id, "origin_id": o, "origin_slot": os, "target_id": t, "target_slot": ts, "type": ty})
}

/// WAN Animate shape: instance 232 → instance 242 → SaveImage 278.
fn wan_fixture() -> Value {
    let producer = def(
        "def-producer",
        json!([]),
        json!([{"id": "o1", "name": "IMAGE", "type": "IMAGE"}]),
        json!([node(json!(10), "LoadImage", 0, json!([]), img_out(), json!(["x.png", "image"]))]),
        json!([sublink(1, 10, 0, -20, 0, "IMAGE")]),
    );
    let scaler = def(
        "def-scaler",
        json!([{"id": "i1", "name": "image", "type": "IMAGE"}]),
        json!([{"id": "o1", "name": "IMAGE", "type": "IMAGE"}]),
        json!([node(json!(20), "ImageScale", 0, json!([img_in("image", json!(1))]), img_out(), json!(["nearest-exact", 512, 512, "disabled"]))]),
        json!([sublink(1, -10, 0, 20, 0, "IMAGE"), sublink(2, 20, 0, -20, 0, "IMAGE")]),
    );
    json!({
        "version": 0.4,
        "nodes": [
            node(json!(232), "def-producer", 0, json!([]), img_out(), json!([])),
            node(json!(242), "def-scaler", 0, json!([img_in("image", json!(500))]), img_out(), json!([])),
            node(json!(278), "SaveImage", 0, json!([img_in("images", json!(501))]), json!([]), json!(["wan"]))
        ],
        "links": [[500, 232, 0, 242, 0, "IMAGE"], [501, 242, 0, 278, 0, "IMAGE"]],
        "definitions": {"subgraphs": [producer, scaler]}
    })
}

#[test]
fn wan_instance_to_instance_link_232_242_278_resolves() {
    let ir = ir_of(&wan_fixture());
    assert_eq!(link_of(&ir, &key(&[], "278"), "images"), key(&["242"], "20"));
    assert_eq!(link_of(&ir, &key(&["242"], "20"), "image"), key(&["232"], "10"));
    assert!(ir.diagnostics.iter().all(|d| d.severity != DiagnosticSeverity::Blocking), "{:?}", ir.diagnostics);
}

#[test]
fn two_level_nesting_resolves_child_to_parent_to_root() {
    let inner = def(
        "def-inner",
        json!([]),
        json!([{"id": "o", "name": "IMAGE", "type": "IMAGE"}]),
        json!([node(json!(3), "LoadImage", 0, json!([]), img_out(), json!(["a.png", "image"]))]),
        json!([sublink(1, 3, 0, -20, 0, "IMAGE")]),
    );
    let outer = def(
        "def-outer",
        json!([]),
        json!([{"id": "o", "name": "IMAGE", "type": "IMAGE"}]),
        json!([node(json!(7), "def-inner", 0, json!([]), img_out(), json!([]))]),
        json!([sublink(1, 7, 0, -20, 0, "IMAGE")]),
    );
    let g = json!({
        "version": 0.4,
        "nodes": [
            node(json!(5), "def-outer", 0, json!([]), img_out(), json!([])),
            node(json!(9), "SaveImage", 0, json!([img_in("images", json!(1))]), json!([]), json!(["n"]))
        ],
        "links": [[1, 5, 0, 9, 0, "IMAGE"]],
        "definitions": {"subgraphs": [inner, outer]}
    });
    let ir = ir_of(&g);
    let src = link_of(&ir, &key(&[], "9"), "images");
    assert_eq!(src, key(&["5", "7"], "3"));
    assert_eq!(src.exec_id(), "5:7:3");
}

fn seeded_def() -> Value {
    def(
        "def-seed",
        json!([{"id": "i-seed", "name": "seed", "type": "INT"}]),
        json!([]),
        json!([node(json!(1), "KSampler", 0, json!([{"name": "seed", "type": "INT", "link": 1, "widget": {"name": "seed"}}]), json!([]), json!([0, "fixed", 20, 7.0, "euler", "normal", 1.0]))]),
        json!([sublink(1, -10, 0, 1, 0, "INT")]),
    )
}

fn seeded_instance(id: u64, seed: u64) -> Value {
    node(json!(id), "def-seed", 0, json!([{"name": "seed", "type": "INT", "link": null, "widget": {"name": "seed"}}]), json!([]), json!([seed]))
}

#[test]
fn same_definition_two_instances_keep_independent_overrides() {
    let g = json!({"version": 0.4, "nodes": [seeded_instance(30, 111), seeded_instance(31, 222)], "links": [],
        "definitions": {"subgraphs": [seeded_def()]}});
    let ir = ir_of(&g);
    let a = ir.attrs(&key(&["30"], "1")).expect("instance 30 inner node");
    let a_node = ir.node(&key(&["30"], "1")).unwrap();
    let b = ir.attrs(&key(&["31"], "1")).expect("instance 31 inner node");
    let b_node = ir.node(&key(&["31"], "1")).unwrap();
    assert_eq!(a.overrides.len(), 1);
    assert_eq!(a.overrides[0].value, json!(111));
    assert_eq!(a.overrides[0].source, OverrideSource::Promoted { instance: key(&[], "30") });
    assert_eq!(b.overrides[0].value, json!(222));
    // shared definition values are untouched on the local copies
    assert_eq!(a_node.widgets_values, b_node.widgets_values);
    assert!(!a_node.inputs.contains_key("seed"));
}

#[test]
fn promoted_widget_count_mismatch_blocks() {
    let mut inst = seeded_instance(30, 1);
    inst["widgets_values"] = json!([1, 2]);
    let g = json!({"version": 0.4, "nodes": [inst], "links": [], "definitions": {"subgraphs": [seeded_def()]}});
    assert!(err_codes(&g).contains(&codes::PROMOTED_WIDGET_COUNT_MISMATCH.to_string()));
}

#[test]
fn recursive_definition_is_reported_and_unused_definitions_are_not_expanded() {
    let rec = def("def-rec", json!([]), json!([]), json!([node(json!(1), "def-rec", 0, json!([]), json!([]), json!([]))]), json!([]));
    let g = json!({"version": 0.4, "nodes": [node(json!(1), "def-rec", 0, json!([]), json!([]), json!([]))], "links": [],
        "definitions": {"subgraphs": [rec]}});
    assert!(err_codes(&g).contains(&codes::SUBGRAPH_DEFINITION_CYCLE.to_string()));

    let unused = json!({"version": 0.4, "nodes": [node(json!(1), "LoadImage", 0, json!([]), img_out(), json!([]))], "links": [],
        "definitions": {"subgraphs": [seeded_def()]}});
    let ir = ir_of(&unused);
    assert_eq!(ir.nodes.len(), 1);
}

// ---------- CW23: reroute and primitive ----------

#[test]
fn multi_level_reroute_and_primitive_values() {
    let g = canvas(
        json!([
            node(json!(1), "LoadImage", 0, json!([]), img_out(), json!(["a.png", "image"])),
            node(json!(2), "Reroute", 0, json!([{"name": "", "type": "*", "link": 1}]), json!([{"name": "", "type": "IMAGE"}]), json!([])),
            node(json!(3), "Reroute", 0, json!([{"name": "", "type": "*", "link": 2}]), json!([{"name": "", "type": "IMAGE"}]), json!([])),
            node(json!(4), "SaveImage", 0, json!([img_in("images", json!(3)), {"name": "filename_prefix", "type": "STRING", "link": 4, "widget": {"name": "filename_prefix"}}]), json!([]), json!(["old"])),
            node(json!(5), "PrimitiveNode", 0, json!([]), json!([{"name": "STRING", "type": "STRING"}]), json!(["from-primitive"]))
        ]),
        json!([[1, 1, 0, 2, 0, "IMAGE"], [2, 2, 0, 3, 0, "IMAGE"], [3, 3, 0, 4, 0, "IMAGE"], [4, 5, 0, 4, 1, "STRING"]]),
    );
    let ir = ir_of(&g);
    let save = ir.attrs(&key(&[], "4")).unwrap();
    assert!(!ir.node(&key(&[], "4")).unwrap().inputs.contains_key("filename_prefix"));
    assert_eq!(link_of(&ir, &key(&[], "4"), "images"), key(&[], "1"));
    assert_eq!(save.overrides.len(), 1);
    assert_eq!(save.overrides[0].input_name, "filename_prefix");
    assert_eq!(save.overrides[0].value, json!("from-primitive"));
    assert_eq!(save.overrides[0].source, OverrideSource::Primitive { node: key(&[], "5") });
    assert!(ir.node(&key(&[], "2")).is_none() && ir.node(&key(&[], "5")).is_none(), "virtual nodes are not executable");
}

// ---------- CW24: mute and bypass ----------

#[test]
fn muted_source_drops_input_visibly() {
    let g = canvas(
        json!([
            node(json!(1), "LoadImage", 2, json!([]), img_out(), json!([])),
            node(json!(2), "SaveImage", 0, json!([img_in("images", json!(1))]), json!([]), json!([]))
        ]),
        json!([[1, 1, 0, 2, 0, "IMAGE"]]),
    );
    let ir = ir_of(&g);
    assert!(ir.node(&key(&[], "2")).unwrap().inputs.is_empty());
    let d = ir.diagnostics.iter().find(|d| d.code == codes::LINK_SOURCE_DROPPED).expect("dropped diagnostic");
    assert_eq!(d.severity, DiagnosticSeverity::NonBlocking);
    assert_eq!(ir.node(&key(&[], "1")).unwrap().mode, NodeMode::Muted);
}

#[test]
fn bypass_uses_same_index_type_match() {
    let g = canvas(
        json!([
            node(json!(1), "LoadImage", 0, json!([]), img_out(), json!([])),
            node(json!(2), "ImageInvert", 4, json!([img_in("image", json!(1))]), img_out(), json!([])),
            node(json!(3), "SaveImage", 0, json!([img_in("images", json!(2))]), json!([]), json!([]))
        ]),
        json!([[1, 1, 0, 2, 0, "IMAGE"], [2, 2, 0, 3, 0, "IMAGE"]]),
    );
    let ir = ir_of(&g);
    assert_eq!(link_of(&ir, &key(&[], "3"), "images"), key(&[], "1"));
}

#[test]
fn bypass_with_several_same_type_inputs_is_not_guessed() {
    let g = canvas(
        json!([
            node(json!(1), "LoadImage", 0, json!([]), img_out(), json!([])),
            node(json!(2), "Blend", 4,
                json!([{"name": "mask", "type": "MASK", "link": null}, img_in("a", json!(1)), img_in("b", json!(2))]),
                img_out(), json!([])),
            node(json!(3), "SaveImage", 0, json!([img_in("images", json!(3))]), json!([]), json!([]))
        ]),
        json!([[1, 1, 0, 2, 1, "IMAGE"], [2, 1, 0, 2, 2, "IMAGE"], [3, 2, 0, 3, 0, "IMAGE"]]),
    );
    let ir = ir_of(&g);
    let d = ir.diagnostics.iter().find(|d| d.code == codes::BYPASS_AMBIGUOUS).expect("ambiguous bypass");
    assert_eq!(d.severity, DiagnosticSeverity::Blocking);
    assert!(ir.node(&key(&[], "3")).unwrap().inputs.is_empty());
}

// ---------- CW26: API literal preservation ----------

#[test]
fn api_literals_are_preserved_and_arrays_are_not_links() {
    let g = json!({
        "1": {"class_type": "Loader", "inputs": {"name": "m.safetensors"}},
        "2": {"class_type": "Consumer", "inputs": {
            "model": ["1", 0],
            "pair": ["a", "b"],
            "triple": ["1", 0, 1],
            "curve": {"__value__": [[0, 0], [1, 1]]},
            "dangling": ["9", 0]
        }}
    });
    let ir = ir_of(&g);
    let n = ir.node(&key(&[], "2")).unwrap();
    assert_eq!(link_of(&ir, &key(&[], "2"), "model"), key(&[], "1"));
    assert_eq!(n.inputs.get("pair"), Some(&InputValue::Literal(json!(["a", "b"]))));
    assert_eq!(n.inputs.get("triple"), Some(&InputValue::Literal(json!(["1", 0, 1]))));
    assert_eq!(n.inputs.get("curve"), Some(&InputValue::Literal(json!({"__value__": [[0, 0], [1, 1]]}))));
    assert!(!n.inputs.contains_key("dangling"));
    assert!(has(&ir.diagnostics, codes::API_LINK_TARGET_MISSING));
}

// ---------- CW25 / CW27 / CW37: compile ----------

fn defn(class: &str, output_node: bool) -> NodeDefinitionV1 {
    NodeDefinitionV1 {
        class_type: class.to_string(),
        input_order: None,
        inputs: Vec::new(),
        outputs: Vec::new(),
        python_module: Some("nodes".to_string()),
        extra: json!({"output_node": output_node}),
    }
}

fn ctx_for(raw: &[u8], classes: &[(&str, bool)], plan: &[(&str, ResolvedCapability)]) -> CompileContextV1 {
    let classes: BTreeMap<String, NodeDefinitionV1> =
        classes.iter().map(|(c, o)| (c.to_string(), defn(c, *o))).collect();
    CompileContextV1 {
        source_format_version: "api".to_string(),
        source_digest: source_digest(raw),
        definitions: DefinitionSnapshotV1 {
            classes,
            digest: "sha256:defs".to_string(),
            server_version: Some("0.3.test".to_string()),
            frontend_version: Some("1.0.test".to_string()),
            complete: true,
        },
        serializer_profile_version: "profile-test".to_string(),
        rule_set_version: 1,
        bridge_evidence: None,
        capability_plan: Some(CapabilityPlanV1 {
            per_node: plan.iter().map(|(id, c)| (id.to_string(), c.clone())).collect(),
        }),
    }
}

fn api_graph() -> Value {
    json!({
        "1": {"class_type": "Loader", "inputs": {"name": "m.safetensors"}, "_meta": {"title": "Load"}},
        "2": {"class_type": "Save", "inputs": {"model": ["1", 0], "prefix": "out"}},
        "3": {"class_type": "Loader", "inputs": {"name": "unused.safetensors"}}
    })
}

fn sel(ids: &[&str]) -> OutputSelection {
    OutputSelection { output_keys: ids.iter().map(|i| key(&[], i)).collect() }
}

#[test]
fn compile_named_api_closure_with_full_provenance() {
    let raw = bytes(&api_graph());
    let ctx = ctx_for(&raw, &[("Loader", false), ("Save", true)], &[
        ("1", ResolvedCapability::NamedApi),
        ("2", ResolvedCapability::NamedApi),
        ("3", ResolvedCapability::NamedApi),
    ]);
    let art = compile(&raw, &ctx, &sel(&["2"])).expect("compiles");
    assert_eq!(
        art.api_prompt,
        json!({
            "1": {"class_type": "Loader", "inputs": {"name": "m.safetensors"}, "_meta": {"title": "Load"}},
            "2": {"class_type": "Save", "inputs": {"model": ["1", 0], "prefix": "out"}}
        })
    );
    assert_eq!(art.retained, vec!["1".to_string(), "2".to_string()]);
    assert_eq!(art.pruned, vec![("3".to_string(), "notInOutputClosure".to_string())]);
    let origin = |exec: &str, input: &str| {
        art.inputs.iter().find(|p| p.exec_id == exec && p.input_name == input).map(|p| p.origin.clone())
    };
    assert_eq!(origin("2", "model"), Some(ValueOrigin::Link));
    assert_eq!(origin("2", "prefix"), Some(ValueOrigin::NamedApi));
    assert_eq!(origin("1", "name"), Some(ValueOrigin::NamedApi));
    // every API input has exactly one provenance record, every exec id maps back to its source
    let inputs_total: usize = art.api_prompt.as_object().unwrap().values().map(|n| n["inputs"].as_object().unwrap().len()).sum();
    assert_eq!(art.inputs.len(), inputs_total);
    for (exec, _) in art.api_prompt.as_object().unwrap() {
        let loc = art.source_of(exec).expect("source map entry");
        assert_eq!(NodeKey::from_exec_id(exec), Some(NodeKey { scope: loc.scope_path.clone(), local_id: loc.node_id.clone() }));
    }
    assert!(art.digests.graph.starts_with("sha256:"));

    let mut independent = ctx;
    independent.definitions.frontend_version = None;
    independent.serializer_profile_version = bgx_comfy_workflow::node_registry::serializer_profile::standard_profile_for(None)
        .expect("default serializer profile").profile_version;
    independent.capability_plan = None;
    let resolved = bgx_comfy_workflow::canvas::resolve::compile_with_evidence(&raw, independent, &sel(&["2"]))
        .expect("independent core resolves named API inputs");
    assert_eq!(resolved.api_prompt, art.api_prompt);
    assert_eq!(resolved.source_map, art.source_map);
    assert_eq!(resolved.digests.graph, art.digests.graph);
}

#[test]
fn compile_fails_explicitly_without_fallback() {
    let raw = bytes(&api_graph());
    let mut ctx = ctx_for(&raw, &[("Loader", false), ("Save", true)], &[("2", ResolvedCapability::NamedApi)]);
    let diags = compile(&raw, &ctx, &sel(&["2"])).unwrap_err();
    assert!(has(&diags, codes::CAPABILITY_MISSING), "node 1 has no capability: {diags:?}");

    ctx.capability_plan = None;
    assert!(has(&compile(&raw, &ctx, &sel(&["2"])).unwrap_err(), codes::CAPABILITY_PLAN_MISSING));

    let ok_ctx = ctx_for(&raw, &[("Loader", false), ("Save", true)], &[("1", ResolvedCapability::NamedApi), ("2", ResolvedCapability::NamedApi)]);
    let mut tampered = raw.clone();
    tampered.push(b' ');
    assert!(has(&compile(&tampered, &ok_ctx, &sel(&["2"])).unwrap_err(), codes::SOURCE_DIGEST_MISMATCH));
    assert!(has(&compile(&raw, &ok_ctx, &sel(&["1"])).unwrap_err(), codes::OUTPUT_NOT_OUTPUT_NODE));
    assert!(has(&compile(&raw, &ok_ctx, &sel(&[])).unwrap_err(), codes::OUTPUT_SELECTION_EMPTY));
}

#[test]
fn execution_cycle_is_diagnosed() {
    let g = json!({
        "1": {"class_type": "Loader", "inputs": {"x": ["2", 0]}},
        "2": {"class_type": "Save", "inputs": {"model": ["1", 0]}}
    });
    let raw = bytes(&g);
    let ctx = ctx_for(&raw, &[("Loader", false), ("Save", true)], &[("1", ResolvedCapability::NamedApi), ("2", ResolvedCapability::NamedApi)]);
    assert!(has(&compile(&raw, &ctx, &sel(&["2"])).unwrap_err(), codes::EXECUTION_CYCLE));
}

#[test]
fn broken_unselected_branch_is_reported_but_blocks_when_selected() {
    let g = json!({
        "1": {"class_type": "Loader", "inputs": {"name": "m"}},
        "2": {"class_type": "Save", "inputs": {"model": ["1", 0]}},
        "3": {"class_type": "Save", "inputs": {"model": ["404", 0]}}
    });
    let raw = bytes(&g);
    let ctx = ctx_for(&raw, &[("Loader", false), ("Save", true)], &[
        ("1", ResolvedCapability::NamedApi),
        ("2", ResolvedCapability::NamedApi),
        ("3", ResolvedCapability::NamedApi),
    ]);
    let art = compile(&raw, &ctx, &sel(&["2"])).expect("selected output compiles");
    let d = art.diagnostics.iter().find(|d| d.code == codes::API_LINK_TARGET_MISSING).expect("still reported");
    assert_eq!(d.severity, DiagnosticSeverity::NonBlocking);
    assert_eq!(d.params.get(codes::OUTSIDE_SELECTED_CLOSURE).map(String::as_str), Some("true"));
    assert!(has(&compile(&raw, &ctx, &sel(&["3"])).unwrap_err(), codes::API_LINK_TARGET_MISSING));
}

#[test]
fn unaccounted_widget_fails_compile() {
    let g = json!({
        "1": {"class_type": "Save", "inputs": {}, "pos": [0, 0], "widgets_values": ["forgotten"]}
    });
    let raw = bytes(&g);
    let ctx = ctx_for(&raw, &[("Save", true)], &[("1", ResolvedCapability::NamedApi)]);
    let diags = compile(&raw, &ctx, &sel(&["1"])).unwrap_err();
    assert!(has(&diags, codes::WIDGET_UNACCOUNTED), "{diags:?}");
}

#[test]
fn muted_canvas_nodes_are_pruned_with_reason() {
    let g = canvas(
        json!([
            node(json!(1), "Loader", 2, json!([]), img_out(), json!([])),
            node(json!(2), "Save", 0, json!([]), json!([]), json!([]))
        ]),
        json!([]),
    );
    let raw = bytes(&g);
    let ctx = ctx_for(&raw, &[("Loader", false), ("Save", true)], &[("2", ResolvedCapability::NamedApi)]);
    let art = compile(&raw, &ctx, &sel(&["2"])).expect("compiles");
    assert_eq!(art.pruned, vec![("1".to_string(), "muted".to_string())]);
}

// ---------- CW62[L02]: annotations before stripping ----------

#[test]
fn annotations_are_collected_with_scope_before_stripping() {
    let sub = def(
        "def-doc",
        json!([]),
        json!([]),
        json!([{"id": 5, "type": "MarkdownNote", "mode": 0, "widgets_values": ["## 子图说明"]}]),
        json!([]),
    );
    let g = json!({
        "version": 0.4,
        "nodes": [
            {"id": 5, "type": "Note", "mode": 0, "widgets_values": ["Use 1024px. 使用 1024 像素。"]},
            {"id": 6, "type": "Note", "mode": 0, "widgets_values": [""]},
            node(json!(7), "def-doc", 0, json!([]), json!([]), json!([]))
        ],
        "links": [],
        "groups": [{"title": "Upscale stage"}],
        "definitions": {"subgraphs": [sub]},
        "extra": {"info": {"author": "someone"}}
    });
    let units = collect_annotation_units(&g);
    let find = |scope: &[&str], id: Option<&str>, kind: &str| {
        units.iter().find(|u| {
            u.scope_path == scope.iter().map(|s| s.to_string()).collect::<Vec<_>>()
                && u.node_id.as_deref() == id
                && u.kind == kind
        })
    };
    assert_eq!(find(&[], Some("5"), "note").unwrap().text, "Use 1024px. 使用 1024 像素。");
    assert_eq!(find(&[], Some("6"), "note").unwrap().text, "");
    assert_eq!(find(&["#definition", "def-doc"], Some("5"), "markdownNote").unwrap().text, "## 子图说明");
    assert_eq!(find(&[], None, "group").unwrap().text, "Upscale stage");
    assert_eq!(find(&[], None, "workflowInfo.author").unwrap().text, "someone");

    // the compiler strips the same notes from the executable IR
    let ir = ir_of(&g);
    assert!(ir.nodes.iter().all(|n| n.class_type != "Note" && n.class_type != "MarkdownNote"));
}
