#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{MappingError, MappingErrorCode};

pub fn digest_text(text: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(text.as_bytes())))
}

pub fn digest_json(value: &Value) -> Result<String, MappingError> {
    // Cargo unifies serde_json features across consumers. A host enabling
    // `preserve_order` must not change graph/definition identities: JSON object
    // order has no meaning, including objects nested inside arrays.
    let mut canonical = value.clone();
    canonical.sort_all_objects();
    let bytes = serde_json::to_vec(&canonical).map_err(|e| {
        MappingError::new(
            MappingErrorCode::GraphInvalid,
            format!("serialize graph: {e}"),
        )
    })?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

/// Outcome of digesting the node definitions a graph uses (PLAN S17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionDigest {
    /// Every used `class_type` has a definition: `sha256:` over exactly those definitions, so
    /// unrelated node packs added or removed on the server do not change it.
    Authoritative(String),
    /// No usable `object_info` snapshot (unreachable, empty or not an object).
    Unavailable,
    /// The snapshot lacks some used class types. An incomplete snapshot is never treated as proof
    /// that a node is missing; it just cannot sign the contract.
    Incomplete(Vec<String>),
}

/// Digest of the definitions for the class types used by API-format `workflow`.
pub fn relevant_definition_digest(
    workflow: &Value,
    object_info: Option<&Value>,
) -> Result<DefinitionDigest, MappingError> {
    let Some(defs) = object_info
        .and_then(Value::as_object)
        .filter(|defs| !defs.is_empty())
    else {
        return Ok(DefinitionDigest::Unavailable);
    };
    let used: alloc::collections::BTreeSet<&str> = workflow
        .as_object()
        .into_iter()
        .flat_map(|nodes| nodes.values())
        .filter_map(|node| node.get("class_type").and_then(Value::as_str))
        .collect();
    let mut relevant = serde_json::Map::new();
    let mut missing = Vec::new();
    for class_type in used {
        match defs.get(class_type) {
            Some(def) => {
                relevant.insert(class_type.to_string(), def.clone());
            }
            None => missing.push(class_type.to_string()),
        }
    }
    if !missing.is_empty() {
        return Ok(DefinitionDigest::Incomplete(missing));
    }
    digest_json(&Value::Object(relevant)).map(DefinitionDigest::Authoritative)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wf() -> Value {
        json!({"1":{"class_type":"KSampler","inputs":{}},"2":{"class_type":"SaveImage","inputs":{}}})
    }

    #[test]
    fn digest_covers_only_used_definitions() {
        let ordered: Value =
            serde_json::from_str(r#"{"z":[{"b":2,"a":1}],"a":{"y":true,"x":null}}"#).unwrap();
        let reordered: Value =
            serde_json::from_str(r#"{"a":{"x":null,"y":true},"z":[{"a":1,"b":2}]}"#).unwrap();
        let presentation = serde_json::to_string(&ordered).unwrap();
        assert_eq!(
            digest_json(&ordered).unwrap(),
            digest_json(&reordered).unwrap()
        );
        assert_eq!(serde_json::to_string(&ordered).unwrap(), presentation);
        let mut changed = reordered.clone();
        changed["z"][0]["b"] = json!(3);
        assert_ne!(
            digest_json(&ordered).unwrap(),
            digest_json(&changed).unwrap()
        );
        let a = json!({"KSampler":{"input":{"required":{}}},"SaveImage":{"input":{}}});
        let mut b = a.clone();
        b["ThirdPartyPack"] = json!({"input":{}});
        let da = relevant_definition_digest(&wf(), Some(&a)).unwrap();
        assert!(matches!(da, DefinitionDigest::Authoritative(_)));
        assert_eq!(da, relevant_definition_digest(&wf(), Some(&b)).unwrap());
        let mut c = a.clone();
        c["KSampler"]["input"]["required"]["steps"] = json!(["INT", {}]);
        assert_ne!(da, relevant_definition_digest(&wf(), Some(&c)).unwrap());
    }

    #[test]
    fn empty_or_partial_object_info_is_not_authoritative() {
        assert_eq!(
            relevant_definition_digest(&wf(), None).unwrap(),
            DefinitionDigest::Unavailable
        );
        assert_eq!(
            relevant_definition_digest(&wf(), Some(&json!({}))).unwrap(),
            DefinitionDigest::Unavailable
        );
        assert_eq!(
            relevant_definition_digest(&wf(), Some(&json!({"KSampler":{}}))).unwrap(),
            DefinitionDigest::Incomplete(vec!["SaveImage".into()])
        );
    }
}
