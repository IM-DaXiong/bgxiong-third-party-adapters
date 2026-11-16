//! Verify a captured frontend serialization against the same source and definitions.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};
use serde_json::Value;
use crate::CompileDiagnostic;
use super::{bridge_evidence_digest, resolve::diagnostic, BridgeEvidenceV1, CompileContextV1};

mod codes {
    pub const BRIDGE_REPLY_INVALID: &str = "workflowBridgeReplyInvalid";
    pub const BRIDGE_SOURCE_DIGEST_MISMATCH: &str = "workflowBridgeSourceDigestMismatch";
    pub const BRIDGE_DEFINITION_MISMATCH: &str = "workflowBridgeDefinitionMismatch";
    pub const BRIDGE_FRONTEND_VERSION_MISMATCH: &str = "workflowBridgeFrontendVersionMismatch";
    pub const BRIDGE_FRONTEND_VERSION_UNKNOWN: &str = "workflowBridgeFrontendVersionUnknown";
    pub const BRIDGE_PLUGIN_DIGEST_INVALID: &str = "workflowBridgePluginDigestInvalid";
    pub const BRIDGE_API_INVALID: &str = "workflowBridgeApiInvalid";
    pub const BRIDGE_SOURCE_MAP_MISMATCH: &str = "workflowBridgeSourceMapMismatch";
    pub const BRIDGE_EVIDENCE_REPLACED: &str = "workflowBridgeEvidenceReplaced";
}

fn digest_prefix(digest: &str) -> String {
    digest.chars().take(23).collect()
}

fn is_sha256_digest(s: &str) -> bool {
    s.strip_prefix("sha256:")
        .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn api_diag(params: [(&str, String); 2]) -> CompileDiagnostic {
    diagnostic(codes::BRIDGE_API_INVALID, None, None, params)
}

/// Structural check of an API prompt: object of nodes with string `class_type`, object `inputs`,
/// and links `[existing node id, non-negative integer slot]`.
fn verify_api_structure(api: &Value, out: &mut Vec<CompileDiagnostic>) {
    let Some(nodes) = api.as_object().filter(|m| !m.is_empty()) else {
        out.push(api_diag([("execId", String::new()), ("reason", "notNonEmptyObject".into())]));
        return;
    };
    for (id, node) in nodes {
        let class_ok = node
            .get("class_type")
            .and_then(Value::as_str)
            .is_some_and(|c| !c.is_empty());
        if !class_ok {
            out.push(api_diag([("execId", id.clone()), ("reason", "classType".into())]));
        }
        let Some(inputs) = node.get("inputs").and_then(Value::as_object) else {
            out.push(api_diag([("execId", id.clone()), ("reason", "inputs".into())]));
            continue;
        };
        for (name, value) in inputs {
            let Some(arr) = value.as_array() else { continue };
            // Array literals are only links when shaped [string id, integer slot].
            if let [Value::String(src), slot] = arr.as_slice() {
                let slot_ok = slot.as_u64().is_some();
                if !slot_ok || !nodes.contains_key(src) {
                    out.push(api_diag([
                        ("execId", id.clone()),
                        ("reason", format!("link:{name}")),
                    ]));
                }
            }
        }
    }
}

/// Re-verify bridge evidence against the context it must belong to. Rejects a tampered source,
/// other definitions, an old/unknown frontend version, a malformed plugin digest, an evidence
/// swap and a structurally invalid API graph / SourceMap.
pub fn verify_bridge_evidence(
    ev: &BridgeEvidenceV1,
    expected: &CompileContextV1,
) -> Result<(), Vec<CompileDiagnostic>> {
    let mut out = Vec::new();
    if ev.source_digest != expected.source_digest {
        out.push(diagnostic(
            codes::BRIDGE_SOURCE_DIGEST_MISMATCH,
            None,
            None,
            [
                ("expected", expected.source_digest.clone()),
                ("actual", ev.source_digest.clone()),
            ],
        ));
    }
    if ev.definition_digest != expected.definitions.digest {
        out.push(diagnostic(
            codes::BRIDGE_DEFINITION_MISMATCH,
            None,
            None,
            [
                ("expected", expected.definitions.digest.clone()),
                ("actual", ev.definition_digest.clone()),
            ],
        ));
    }
    match &expected.definitions.frontend_version {
        Some(v) if *v == ev.frontend_version => {}
        Some(v) => out.push(diagnostic(
            codes::BRIDGE_FRONTEND_VERSION_MISMATCH,
            None,
            None,
            [("expected", v.clone()), ("actual", ev.frontend_version.clone())],
        )),
        None => out.push(diagnostic(
            codes::BRIDGE_FRONTEND_VERSION_UNKNOWN,
            None,
            None,
            [("actual", ev.frontend_version.clone())],
        )),
    }
    if !is_sha256_digest(&ev.plugin_digest) {
        out.push(diagnostic(
            codes::BRIDGE_PLUGIN_DIGEST_INVALID,
            None,
            None,
            [("actual", digest_prefix(&ev.plugin_digest))],
        ));
    }
    if ev.consumed_widget_owners.len() != ev.consumed_widgets.len() {
        out.push(diagnostic(
            codes::BRIDGE_REPLY_INVALID,
            None,
            None,
            [
                ("consumedWidgets", ev.consumed_widgets.len().to_string()),
                ("consumedWidgetOwners", ev.consumed_widget_owners.len().to_string()),
            ],
        ));
    }
    for owner in &ev.consumed_widget_owners {
        if !ev.source_map.exec_to_source.contains_key(owner) {
            out.push(diagnostic(
                codes::BRIDGE_SOURCE_MAP_MISMATCH,
                None,
                None,
                [("execId", owner.clone()), ("reason", "widgetOwnerUnmapped".into())],
            ));
        }
    }
    if let Some(pinned) = &expected.bridge_evidence {
        let (a, b) = match (bridge_evidence_digest(pinned), bridge_evidence_digest(ev)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(d), _) | (_, Err(d)) => {
                out.push(*d);
                return Err(out);
            }
        };
        if a != b {
            out.push(diagnostic(
                codes::BRIDGE_EVIDENCE_REPLACED,
                None,
                None,
                [("expected", a), ("actual", b)],
            ));
        }
    }
    verify_api_structure(&ev.api_prompt, &mut out);
    if let Some(nodes) = ev.api_prompt.as_object() {
        for id in nodes.keys() {
            if !ev.source_map.exec_to_source.contains_key(id) {
                out.push(diagnostic(
                    codes::BRIDGE_SOURCE_MAP_MISMATCH,
                    None,
                    None,
                    [("execId", id.clone()), ("reason", "unmapped".into())],
                ));
            }
        }
        for id in ev.source_map.exec_to_source.keys() {
            if !nodes.contains_key(id) {
                out.push(diagnostic(
                    codes::BRIDGE_SOURCE_MAP_MISMATCH,
                    None,
                    None,
                    [("execId", id.clone()), ("reason", "dangling".into())],
                ));
            }
        }
    }
    if out.is_empty() {
        Ok(())
    } else {
        Err(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{source_digest, SourceMapV1};
    use crate::node_registry::definition::DefinitionSnapshotV1;
    use crate::NodeLocator;
    use serde_json::json;
    use alloc::collections::BTreeMap;

    const SRC: &[u8] = br#"{"nodes":[],"links":[]}"#;

    fn ctx() -> CompileContextV1 {
        CompileContextV1 {
            source_format_version: "canvas-0.4".into(),
            source_digest: source_digest(SRC),
            definitions: DefinitionSnapshotV1 {
                classes: BTreeMap::new(),
                digest: "sha256:defs-v1".into(),
                server_version: Some("0.3.60".into()),
                frontend_version: Some("1.28.0".into()),
                complete: true,
            },
            serializer_profile_version: "std-1".into(),
            rule_set_version: 1,
            bridge_evidence: None,
            capability_plan: None,
        }
    }

    fn evidence() -> BridgeEvidenceV1 {
        let mut exec_to_source = BTreeMap::new();
        for id in ["1", "2"] {
            exec_to_source.insert(
                id.to_string(),
                NodeLocator {
                    scope_path: vec![],
                    node_id: id.to_string(),
                },
            );
        }
        BridgeEvidenceV1 {
            source_digest: source_digest(SRC),
            definition_digest: "sha256:defs-v1".into(),
            frontend_version: "1.28.0".into(),
            plugin_digest: format!("sha256:{}", "a".repeat(64)),
            api_prompt: json!({
                "1": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": "m.safetensors"}},
                "2": {"class_type": "KSampler", "inputs": {"model": ["1", 0], "seed": 5, "list": [1, 2]}}
            }),
            source_map: SourceMapV1 { exec_to_source },
            consumed_widgets: vec![],
            consumed_widget_owners: vec![],
        }
    }

    fn codes_of(r: Result<(), Vec<CompileDiagnostic>>) -> Vec<String> {
        r.expect_err("rejected").into_iter().map(|d| d.code).collect()
    }

    #[test]
    fn valid_evidence_is_accepted() {
        verify_bridge_evidence(&evidence(), &ctx()).expect("valid");
    }

    #[test]
    fn tampered_source_is_rejected() {
        let mut ev = evidence();
        ev.source_digest = source_digest(b"tampered");
        assert_eq!(codes_of(verify_bridge_evidence(&ev, &ctx())), vec![codes::BRIDGE_SOURCE_DIGEST_MISMATCH]);
    }

    #[test]
    fn old_frontend_version_is_rejected() {
        let mut ev = evidence();
        ev.frontend_version = "1.20.0".into();
        assert_eq!(codes_of(verify_bridge_evidence(&ev, &ctx())), vec![codes::BRIDGE_FRONTEND_VERSION_MISMATCH]);
    }

    #[test]
    fn unknown_expected_frontend_version_is_rejected() {
        let mut c = ctx();
        c.definitions.frontend_version = None;
        assert_eq!(codes_of(verify_bridge_evidence(&evidence(), &c)), vec![codes::BRIDGE_FRONTEND_VERSION_UNKNOWN]);
    }

    #[test]
    fn other_definitions_are_rejected() {
        let mut ev = evidence();
        ev.definition_digest = "sha256:defs-v0".into();
        assert_eq!(codes_of(verify_bridge_evidence(&ev, &ctx())), vec![codes::BRIDGE_DEFINITION_MISMATCH]);
    }

    #[test]
    fn malformed_plugin_digest_is_rejected() {
        let mut ev = evidence();
        ev.plugin_digest = "dev".into();
        assert_eq!(codes_of(verify_bridge_evidence(&ev, &ctx())), vec![codes::BRIDGE_PLUGIN_DIGEST_INVALID]);
    }

    #[test]
    fn dangling_link_and_unmapped_node_are_rejected() {
        let mut ev = evidence();
        ev.api_prompt["2"]["inputs"]["model"] = json!(["9", 0]);
        ev.api_prompt["3"] = json!({"class_type": "SaveImage", "inputs": {}});
        let codes = codes_of(verify_bridge_evidence(&ev, &ctx()));
        assert_eq!(codes, vec![codes::BRIDGE_API_INVALID, codes::BRIDGE_SOURCE_MAP_MISMATCH]);
    }

    #[test]
    fn swapped_evidence_is_rejected() {
        let mut c = ctx();
        let mut pinned = evidence();
        pinned.consumed_widgets.clear();
        pinned.api_prompt["1"]["inputs"]["ckpt_name"] = json!("other.safetensors");
        c.bridge_evidence = Some(pinned);
        assert_eq!(codes_of(verify_bridge_evidence(&evidence(), &c)), vec![codes::BRIDGE_EVIDENCE_REPLACED]);
    }

}
