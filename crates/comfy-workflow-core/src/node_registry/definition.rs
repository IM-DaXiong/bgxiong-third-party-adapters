//! Normalized ComfyUI node definitions (`/object_info`) for the compile context (PLAN CW30, D04).
//!
//! Only the classes a graph uses are captured. An empty or partial snapshot is never
//! authoritative: `normalize_object_info` fails instead of returning a snapshot that could
//! later be mistaken for proof that a node or an input does not exist.
//!
//! `input_order` is kept exactly as the server sent it. It proves the server-side field order
//! only; widget positions are decided by a serializer profile (`serializer_profile.rs`) or a
//! reviewed special rule (`special.rs`), never by this module. JSON object key order is never
//! used: when `input_order` is absent, `NodeDefinitionV1::input_order` stays `None` and `inputs`
//! is listed by category in canonical name order, which carries no positional meaning.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec, boxed::Box};

use alloc::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::digest::{digest_json, digest_text};
use crate::types::{CompileDiagnostic, DiagnosticId, DiagnosticSeverity, NodeLocator};

/// Diagnostic codes raised by definition normalization and widget binding (L03).
/// The coordinator folds them into the closed code set of the import contract (L01, CW09/CW15).
pub mod codes {
    /// No usable `object_info` (empty, not an object, or no class requested) -> needsDefinitions.
    pub const DEFINITIONS_UNAVAILABLE: &str = "workflowDefinitionsUnavailable";
    /// Some used classes are absent from `object_info` -> needsDefinitions.
    pub const DEFINITIONS_INCOMPLETE: &str = "workflowDefinitionsIncomplete";
    /// A class definition does not follow the `object_info` shape.
    pub const DEFINITION_MALFORMED: &str = "workflowDefinitionMalformed";
    /// Positional widgets need an explicit `input_order` that the definition lacks -> needsDefinitions.
    pub const INPUT_ORDER_MISSING: &str = "workflowDefinitionInputOrderMissing";
    /// No verified serializer profile/special rule proves this node's widget layout.
    pub const SERIALIZER_EVIDENCE_REQUIRED: &str = "workflowSerializerEvidenceRequired";
    /// `widgets_values` holds fewer values than the proven layout consumes.
    pub const WIDGET_COUNT_MISMATCH: &str = "workflowWidgetCountMismatch";
    /// A `widgets_values` position/key is neither consumed nor proven UI-only.
    pub const WIDGET_VALUE_UNCONSUMED: &str = "workflowWidgetValueUnconsumed";
    /// A widget value does not fit its input definition (layout evidence contradicted).
    pub const WIDGET_VALUE_INVALID: &str = "workflowWidgetValueInvalid";
    /// A DynamicCombo selector names an option the definition does not declare.
    pub const DYNAMIC_COMBO_OPTION_UNKNOWN: &str = "workflowDynamicComboOptionUnknown";
    /// The special rule / definition handed to the binder does not belong to this node.
    pub const SPECIAL_RULE_MISMATCH: &str = "workflowSpecialRuleMismatch";
    /// A promoted (subgraph instance) override cannot be applied.
    pub const PROMOTED_OVERRIDE_INVALID: &str = "workflowPromotedOverrideInvalid";
    /// A literal value cannot be represented for its input (`__value__`/CURVE/scalar type).
    pub const LITERAL_INVALID: &str = "workflowLiteralInvalid";
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputDef {
    pub name: String,
    /// Server type name. A legacy list-form combo (`[["a","b"], {...}]`) is `COMBO`.
    pub type_name: String,
    pub required: bool,
    pub hidden: bool,
    pub force_input: bool,
    /// The input's option object as sent by the server (unknown keys kept), or `null` when the
    /// spec has no option object. A legacy list-form combo stores its choices under `options`.
    pub options: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeDefinitionV1 {
    pub class_type: String,
    /// Explicit server `input_order` flattened as required, optional, hidden. `None` = not sent.
    pub input_order: Option<Vec<String>>,
    /// Required, optional, hidden; each category in `input_order` order when present.
    pub inputs: Vec<InputDef>,
    pub outputs: Vec<String>,
    pub python_module: Option<String>,
    /// Every other definition key, verbatim (display name, category, output names, ...). Input
    /// sections other than required/optional/hidden are kept under `extra.input`.
    pub extra: Value,
}

impl NodeDefinitionV1 {
    pub fn input(&self, name: &str) -> Option<&InputDef> {
        self.inputs.iter().find(|input| input.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionSnapshotV1 {
    pub classes: BTreeMap<String, NodeDefinitionV1>,
    /// `sha256:` over exactly the raw definitions of `classes` (same scheme as
    /// `relevant_definition_digest`), so unrelated packs do not change it.
    pub digest: String,
    /// Unknown unless the host proves it (e.g. `/system_stats`); never inferred here.
    pub server_version: Option<String>,
    /// Unknown unless the host proves it from the connected frontend; never inferred here.
    pub frontend_version: Option<String>,
    /// `true` only when every requested class was normalized.
    pub complete: bool,
}

const CATEGORIES: [&str; 3] = ["required", "optional", "hidden"];

/// Reviewed model-file inventories shared with the host's model-visibility probe.
/// These are file selections, not sampler/dtype or arbitrary custom-node enums.
pub const MODEL_FILE_INPUTS: &[(&str, &str)] = &[
    ("UNETLoader", "unet_name"),
    ("CheckpointLoaderSimple", "ckpt_name"),
    ("CLIPLoader", "clip_name"),
    ("VAELoader", "vae_name"),
];

/// Submission compatibility of the executable graph, distinct from the compiler's full raw
/// snapshot digest. Upload/model-file inventories are runtime assets, not closed parameter
/// choices. The graph identity freezes chosen values; model-file selections must still be
/// present in the live inventory. Types, order, bounds, other enums and metadata stay signed.
pub fn mapping_definition_digest(
    workflow: &Value,
    snapshot: &DefinitionSnapshotV1,
) -> Result<String, crate::error::MappingError> {
    use crate::error::{MappingError, MappingErrorCode};
    let used: BTreeSet<&str> = workflow.as_object().into_iter().flat_map(|m| m.values())
        .filter_map(|n| n.get("class_type").and_then(Value::as_str)).collect();
    let mut relevant = BTreeMap::new();
    for class in used {
        let mut definition = snapshot.classes.get(class).cloned().ok_or_else(||
            MappingError::new(MappingErrorCode::GraphInvalid, format!("definition missing: {class}")))?;
        for input in &mut definition.inputs {
            let upload = ["image_upload", "video_upload", "audio_upload"].iter()
                .any(|flag| input.options.get(*flag).and_then(Value::as_bool) == Some(true));
            let model_file = MODEL_FILE_INPUTS.contains(&(class, input.name.as_str()));
            if model_file && input.type_name == "COMBO" {
                let choices = input.options.get("options").and_then(Value::as_array).ok_or_else(||
                    MappingError::new(MappingErrorCode::NodeDefinitionIncomplete,
                        format!("model inventory unavailable: {class}.{}", input.name)))?;
                for (node_id, node) in workflow.as_object().into_iter().flat_map(|nodes| nodes.iter()) {
                    if node.get("class_type").and_then(Value::as_str) != Some(class) { continue; }
                    let Some(selected) = node.get("inputs").and_then(|inputs| inputs.get(&input.name)) else { continue; };
                    if crate::linked_source(selected).is_some() { continue; }
                    if !selected.is_string() || !choices.contains(selected) {
                        return Err(MappingError::new(MappingErrorCode::ValueOutOfRange,
                            format!("selected model file is not available: {class}.{} at node {node_id}: {selected}", input.name)));
                    }
                }
            }
            if (upload || model_file) && input.type_name == "COMBO" {
                if let Some(options) = input.options.as_object_mut() {
                    options.remove("options");
                }
            }
        }
        relevant.insert(class, definition);
    }
    let value = serde_json::to_value(relevant).map_err(|e|
        MappingError::new(MappingErrorCode::GraphInvalid, e.to_string()))?;
    digest_json(&canonical_json(&value))
}

/// Capture the definitions of `classes` from a raw `/object_info` response.
pub fn normalize_object_info(
    raw: &Value,
    classes: &BTreeSet<String>,
) -> Result<DefinitionSnapshotV1, Box<CompileDiagnostic>> {
    if classes.is_empty() {
        return Err(Box::new(definition_diagnostic(
            codes::DEFINITIONS_UNAVAILABLE,
            None,
            params([("reason", "noClassesRequested".to_string())]),
        )));
    }
    let all = match raw.as_object() {
        Some(all) if !all.is_empty() => all,
        Some(_) => {
            return Err(Box::new(definition_diagnostic(
                codes::DEFINITIONS_UNAVAILABLE,
                None,
                params([("reason", "emptyObjectInfo".to_string())]),
            )))
        }
        None => {
            return Err(Box::new(definition_diagnostic(
                codes::DEFINITIONS_UNAVAILABLE,
                None,
                params([("reason", "objectInfoNotObject".to_string())]),
            )))
        }
    };
    let missing: Vec<&str> = classes
        .iter()
        .filter(|class_type| !all.contains_key(class_type.as_str()))
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        return Err(Box::new(definition_diagnostic(
            codes::DEFINITIONS_INCOMPLETE,
            None,
            params([
                ("missingCount", missing.len().to_string()),
                ("missingClasses", Value::from(missing.clone()).to_string()),
            ]),
        )));
    }
    let mut normalized = BTreeMap::new();
    let mut relevant = Map::new();
    for class_type in classes {
        let raw_def = &all[class_type.as_str()];
        normalized.insert(class_type.clone(), normalize_node(class_type, raw_def)?);
        relevant.insert(class_type.clone(), raw_def.clone());
    }
    let digest = digest_json(&canonical_json(&Value::Object(relevant))).map_err(|err| {
        definition_diagnostic(
            codes::DEFINITION_MALFORMED,
            None,
            params([("reason", "digest".to_string()), ("detail", err.detail)]),
        )
    })?;
    Ok(DefinitionSnapshotV1 {
        classes: normalized,
        digest,
        server_version: None,
        frontend_version: None,
        complete: true,
    })
}

fn normalize_node(class_type: &str, raw: &Value) -> Result<NodeDefinitionV1, Box<CompileDiagnostic>> {
    let malformed = |reason: &str, extra: Option<(&'static str, String)>| {
        let mut p = params([("reason", reason.to_string())]);
        if let Some((key, value)) = extra {
            p.insert(key.to_string(), value);
        }
        definition_diagnostic(codes::DEFINITION_MALFORMED, Some(class_type), p)
    };
    let obj = raw
        .as_object()
        .ok_or_else(|| malformed("definitionNotObject", None))?;
    let input = obj
        .get("input")
        .ok_or_else(|| malformed("inputMissing", None))?
        .as_object()
        .ok_or_else(|| malformed("inputNotObject", None))?;

    let mut sections: BTreeMap<&str, &Map<String, Value>> = BTreeMap::new();
    let mut unknown_sections = Map::new();
    for (key, value) in input {
        if CATEGORIES.contains(&key.as_str()) {
            let section = value
                .as_object()
                .ok_or_else(|| malformed("inputSectionNotObject", Some(("section", key.clone()))))?;
            sections.insert(key.as_str(), section);
        } else {
            unknown_sections.insert(key.clone(), value.clone());
        }
    }

    let explicit_order = match obj.get("input_order") {
        None => None,
        Some(Value::Object(order)) => Some(parse_input_order(order).map_err(|(reason, section)| {
            malformed(reason, Some(("section", section)))
        })?),
        Some(_) => return Err(Box::new(malformed("inputOrderNotObject", None))),
    };

    let mut inputs = Vec::new();
    let mut flat_order = Vec::new();
    for category in CATEGORIES {
        let empty = Map::new();
        let section = sections.get(category).copied().unwrap_or(&empty);
        let category_order = explicit_order.as_ref().and_then(|o| o.get(category));
        let names: Vec<String> = match category_order {
            Some(order) => {
                let ordered: BTreeSet<&str> = order.iter().map(String::as_str).collect();
                let declared: BTreeSet<&str> = section.keys().map(String::as_str).collect();
                if ordered.len() != order.len() || ordered != declared {
                    return Err(Box::new(malformed(
                        "inputOrderMismatch",
                        Some(("section", category.to_string())),
                    )));
                }
                order.clone()
            }
            None => {
                // `hidden` never carries widgets, so its order is irrelevant; for required and
                // optional a missing list leaves `input_order: None` (not positional evidence).
                if category != "hidden" && explicit_order.is_some() && !section.is_empty() {
                    return Err(Box::new(malformed(
                        "inputOrderMismatch",
                        Some(("section", category.to_string())),
                    )));
                }
                section.keys().cloned().collect()
            }
        };
        for name in names {
            let spec = &section[name.as_str()];
            let def = parse_input_spec(&name, spec, category == "required", category == "hidden")
                .map_err(|reason| malformed(reason, Some(("input", name.clone()))))?;
            if category_order.is_some() {
                flat_order.push(name.clone());
            }
            inputs.push(def);
        }
    }

    let outputs = match obj.get("output") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(type_name) => Ok(type_name.clone()),
                // object_info encodes a combo output as its choice list.
                Value::Array(_) => Ok("COMBO".to_string()),
                _ => Err(Box::new(malformed("outputTypeInvalid", None))),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(Box::new(malformed("outputNotArray", None))),
        None => return Err(Box::new(malformed("outputMissing", None))),
    };
    let python_module = match obj.get("python_module") {
        None => None,
        Some(Value::String(module)) => Some(module.clone()),
        Some(_) => return Err(Box::new(malformed("pythonModuleNotString", None))),
    };
    let mut extra = Map::new();
    for (key, value) in obj {
        if !matches!(key.as_str(), "input" | "input_order" | "output" | "python_module") {
            extra.insert(key.clone(), value.clone());
        }
    }
    if !unknown_sections.is_empty() {
        extra.insert("input".to_string(), Value::Object(unknown_sections));
    }
    Ok(NodeDefinitionV1 {
        class_type: class_type.to_string(),
        input_order: explicit_order.map(|_| flat_order),
        inputs,
        outputs,
        python_module,
        extra: Value::Object(extra),
    })
}

/// Lowercase hex sha256 of the canonical JSON (object keys sorted at every level) of one
/// normalized class definition. This is the `definition_digest` special rules are pinned to.
pub fn class_definition_digest(def: &NodeDefinitionV1) -> Result<String, Box<CompileDiagnostic>> {
    let value = serde_json::to_value(def).map_err(|err| {
        definition_diagnostic(
            codes::DEFINITION_MALFORMED,
            Some(def.class_type.as_str()),
            params([("reason", "digest".to_string()), ("detail", err.to_string())]),
        )
    })?;
    let bytes = canonical_json(&value).to_string();
    Ok(hex::encode(Sha256::digest(bytes.as_bytes())))
}

/// Same value with object keys inserted in sorted order at every level, so the serialized bytes
/// do not depend on the map implementation (`preserve_order` or not).
fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonical_json(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

/// Parse `input_order`; errors carry `(reason, section)`.
pub(crate) fn parse_input_order(
    order: &Map<String, Value>,
) -> Result<BTreeMap<&'static str, Vec<String>>, (&'static str, String)> {
    let mut out = BTreeMap::new();
    for (key, value) in order {
        let category = CATEGORIES
            .iter()
            .copied()
            .find(|c| *c == key.as_str())
            .ok_or_else(|| ("inputOrderUnknownSection", key.clone()))?;
        let names = value
            .as_array()
            .ok_or_else(|| ("inputOrderNotArray", key.clone()))?
            .iter()
            .map(|name| name.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| ("inputOrderNameNotString", key.clone()))?;
        out.insert(category, names);
    }
    Ok(out)
}

/// Parse one `[type, options?]` input spec. Errors are stable reason strings.
pub(crate) fn parse_input_spec(
    name: &str,
    spec: &Value,
    required: bool,
    hidden: bool,
) -> Result<InputDef, &'static str> {
    let items = match spec {
        Value::Array(items) if (1..=2).contains(&items.len()) => items.as_slice(),
        // Hidden inputs are sent as a bare type string (e.g. `"PROMPT"`).
        Value::String(_) => core::slice::from_ref(spec),
        _ => return Err("inputSpecShape"),
    };
    let mut options = match items.get(1) {
        None => None,
        Some(Value::Object(options)) => Some(options.clone()),
        Some(_) => return Err("inputOptionsNotObject"),
    };
    let type_name = match &items[0] {
        Value::String(type_name) => type_name.clone(),
        Value::Array(choices) => {
            let opts = options.get_or_insert_with(Map::new);
            if opts.contains_key("options") {
                return Err("comboOptionsConflict");
            }
            opts.insert("options".to_string(), Value::Array(choices.clone()));
            "COMBO".to_string()
        }
        _ => return Err("inputTypeInvalid"),
    };
    let force_input = match options.as_ref().and_then(|o| o.get("forceInput")) {
        None => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return Err("forceInputNotBool"),
    };
    Ok(InputDef {
        name: name.to_string(),
        type_name,
        required,
        hidden,
        force_input,
        options: options.map(Value::Object).unwrap_or(Value::Null),
    })
}

pub(crate) fn params<const N: usize>(entries: [(&str, String); N]) -> BTreeMap<String, String> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn definition_diagnostic(
    code: &str,
    class_type: Option<&str>,
    params: BTreeMap<String, String>,
) -> CompileDiagnostic {
    blocking_diagnostic(code, None, class_type, None, params)
}

/// Blocking diagnostic whose id is a digest of its structural identity (code, locator, class,
/// input, params); ids are compared, never parsed.
pub(crate) fn blocking_diagnostic(
    code: &str,
    locator: Option<NodeLocator>,
    class_type: Option<&str>,
    input_name: Option<&str>,
    params: BTreeMap<String, String>,
) -> CompileDiagnostic {
    let locator_identity = locator
        .as_ref()
        .map(|l| json!({ "scopePath": l.scope_path, "nodeId": l.node_id }));
    let identity = json!([code, locator_identity, class_type, input_name, params]);
    CompileDiagnostic {
        diagnostic_id: DiagnosticId(digest_text(&identity.to_string())),
        code: code.to_string(),
        severity: DiagnosticSeverity::Blocking,
        locator,
        class_type: class_type.map(str::to_string),
        input_name: input_name.map(str::to_string),
        params,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    /// Non-lexicographic order on purpose: `seed` < `model` < `cfg` would sort differently.
    fn sampler_info() -> Value {
        json!({
            "Sampler": {
                "input": {
                    "required": {
                        "model": ["MODEL"],
                        "seed": ["INT", {"default": 0, "min": 0}],
                        "cfg": ["FLOAT", {"default": 8.0}],
                        "sampler_name": [["euler", "dpmpp_2m"], {"tooltip": "t"}]
                    },
                    "optional": { "note": ["STRING", {"multiline": true}] },
                    "hidden": { "prompt": "PROMPT", "unique_id": "UNIQUE_ID" },
                    "x_vendor": { "k": 1 }
                },
                "input_order": {
                    "required": ["seed", "model", "sampler_name", "cfg"],
                    "optional": ["note"],
                    "hidden": ["unique_id", "prompt"]
                },
                "output": ["LATENT", ["a", "b"]],
                "output_name": ["LATENT", "choice"],
                "python_module": "custom_nodes.pack",
                "display_name": "Sampler",
                "x_unknown_meta": {"keep": true}
            },
            "Unrelated": { "input": {}, "output": [] }
        })
    }

    #[test]
    fn mapping_digest_keeps_existing_selection_valid_when_model_inventory_grows() {
        let workflow = json!({"model": {"class_type": "UNETLoader", "inputs": {"unet_name": "selected.safetensors"}}});
        let initial = json!({"UNETLoader": {"input": {"required": {
            "unet_name": [["selected.safetensors"], {}], "weight_dtype": [["default", "fp8"], {}]
        }}, "output": ["MODEL"]}});
        let used = classes(&["UNETLoader"]);
        let initial_digest = mapping_definition_digest(&workflow, &normalize_object_info(&initial, &used).unwrap()).unwrap();
        let mut current = initial.clone();
        current["UNETLoader"]["input"]["required"]["unet_name"][0] = json!(["new.safetensors", "selected.safetensors"]);
        assert_eq!(initial_digest, mapping_definition_digest(&workflow, &normalize_object_info(&current, &used).unwrap()).unwrap());
        current["UNETLoader"]["input"]["required"]["unet_name"][0] = json!(["new.safetensors"]);
        let error = mapping_definition_digest(&workflow, &normalize_object_info(&current, &used).unwrap()).unwrap_err();
        assert_eq!(error.code, crate::MappingErrorCode::ValueOutOfRange);
    }

    #[test]
    fn mapping_digest_still_invalidates_changed_types_and_non_file_enums() {
        let workflow = json!({"model": {"class_type": "UNETLoader", "inputs": {"unet_name": "selected.safetensors"}}});
        let raw = json!({"UNETLoader": {"input": {"required": {
            "unet_name": [["selected.safetensors"], {}], "weight_dtype": [["default", "fp8"], {}]
        }}, "output": ["MODEL"]}});
        let used = classes(&["UNETLoader"]);
        let original = mapping_definition_digest(&workflow, &normalize_object_info(&raw, &used).unwrap()).unwrap();
        let mut other_enum = raw.clone();
        other_enum["UNETLoader"]["input"]["required"]["weight_dtype"][0] = json!(["default", "fp8", "fp16"]);
        assert_ne!(original, mapping_definition_digest(&workflow, &normalize_object_info(&other_enum, &used).unwrap()).unwrap());
        let mut changed_type = raw;
        changed_type["UNETLoader"]["input"]["required"]["unet_name"] = json!(["STRING", {}]);
        assert_ne!(original, mapping_definition_digest(&workflow, &normalize_object_info(&changed_type, &used).unwrap()).unwrap());
    }

    #[test]
    fn empty_or_non_object_info_is_never_authoritative() {
        for raw in [json!({}), json!(null), json!([1])] {
            let err = normalize_object_info(&raw, &classes(&["Sampler"])).unwrap_err();
            assert_eq!(err.code, codes::DEFINITIONS_UNAVAILABLE);
        }
        let err = normalize_object_info(&sampler_info(), &BTreeSet::new()).unwrap_err();
        assert_eq!(err.code, codes::DEFINITIONS_UNAVAILABLE);
    }

    #[test]
    fn partial_snapshot_is_rejected_with_missing_classes() {
        let err = normalize_object_info(&sampler_info(), &classes(&["Sampler", "Missing"])).unwrap_err();
        assert_eq!(err.code, codes::DEFINITIONS_INCOMPLETE);
        assert_eq!(err.params["missingCount"], "1");
        assert_eq!(err.params["missingClasses"], "[\"Missing\"]");
    }

    #[test]
    fn explicit_input_order_is_kept_not_key_order() {
        let snap = normalize_object_info(&sampler_info(), &classes(&["Sampler"])).unwrap();
        assert!(snap.complete);
        assert_eq!(snap.server_version, None);
        assert_eq!(snap.frontend_version, None);
        let def = &snap.classes["Sampler"];
        let names: Vec<&str> = def.inputs.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["seed", "model", "sampler_name", "cfg", "note", "unique_id", "prompt"]);
        assert_eq!(
            def.input_order.as_deref().unwrap(),
            ["seed", "model", "sampler_name", "cfg", "note", "unique_id", "prompt"]
        );
        let combo = def.input("sampler_name").unwrap();
        assert_eq!(combo.type_name, "COMBO");
        assert_eq!(combo.options["options"], json!(["euler", "dpmpp_2m"]));
        assert_eq!(combo.options["tooltip"], json!("t"));
        let note = def.input("note").unwrap();
        assert!(!note.required && !note.hidden);
        let hidden = def.input("prompt").unwrap();
        assert!(hidden.hidden && !hidden.required);
        assert_eq!(hidden.options, Value::Null);
        assert_eq!(def.outputs, ["LATENT", "COMBO"]);
        assert_eq!(def.python_module.as_deref(), Some("custom_nodes.pack"));
        assert_eq!(def.extra["x_unknown_meta"], json!({"keep": true}));
        assert_eq!(def.extra["input"], json!({"x_vendor": {"k": 1}}));
        assert_eq!(def.extra["output_name"], json!(["LATENT", "choice"]));
    }

    #[test]
    fn digest_covers_only_requested_classes() {
        let a = normalize_object_info(&sampler_info(), &classes(&["Sampler"])).unwrap();
        let mut other = sampler_info();
        other["Unrelated"]["display_name"] = json!("changed");
        let b = normalize_object_info(&other, &classes(&["Sampler"])).unwrap();
        assert_eq!(a.digest, b.digest);
        other["Sampler"]["display_name"] = json!("changed");
        let c = normalize_object_info(&other, &classes(&["Sampler"])).unwrap();
        assert_ne!(a.digest, c.digest);
    }

    #[test]
    fn class_digest_is_lowercase_hex_and_tracks_definition_changes() {
        let snap = normalize_object_info(&sampler_info(), &classes(&["Sampler"])).unwrap();
        let def = &snap.classes["Sampler"];
        let digest = class_definition_digest(def).unwrap();
        assert_eq!(digest.len(), 64);
        assert!(digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_eq!(class_definition_digest(&def.clone()).unwrap(), digest);
        let mut changed = def.clone();
        changed.inputs[0].force_input = true;
        assert_ne!(class_definition_digest(&changed).unwrap(), digest);
        assert!(snap.digest.starts_with("sha256:"));
    }

    #[test]
    fn missing_input_order_stays_none() {
        let mut raw = sampler_info();
        raw["Sampler"].as_object_mut().unwrap().remove("input_order");
        let snap = normalize_object_info(&raw, &classes(&["Sampler"])).unwrap();
        assert_eq!(snap.classes["Sampler"].input_order, None);
    }

    #[test]
    fn input_order_disagreeing_with_inputs_is_malformed() {
        let mut raw = sampler_info();
        raw["Sampler"]["input_order"]["required"] = json!(["seed", "model", "cfg"]);
        let err = normalize_object_info(&raw, &classes(&["Sampler"])).unwrap_err();
        assert_eq!(err.code, codes::DEFINITION_MALFORMED);
        assert_eq!(err.params["reason"], "inputOrderMismatch");
        assert_eq!(err.class_type.as_deref(), Some("Sampler"));

        let mut dup = sampler_info();
        dup["Sampler"]["input_order"]["required"] =
            json!(["seed", "model", "sampler_name", "cfg", "cfg"]);
        assert!(normalize_object_info(&dup, &classes(&["Sampler"])).is_err());
    }

    #[test]
    fn force_input_is_read_strictly() {
        let mut raw = sampler_info();
        raw["Sampler"]["input"]["required"]["cfg"] = json!(["FLOAT", {"forceInput": true}]);
        let snap = normalize_object_info(&raw, &classes(&["Sampler"])).unwrap();
        assert!(snap.classes["Sampler"].input("cfg").unwrap().force_input);

        raw["Sampler"]["input"]["required"]["cfg"] = json!(["FLOAT", {"forceInput": "yes"}]);
        let err = normalize_object_info(&raw, &classes(&["Sampler"])).unwrap_err();
        assert_eq!(err.params["reason"], "forceInputNotBool");
        assert_eq!(err.params["input"], "cfg");
    }

    #[test]
    fn diagnostic_ids_are_structural_and_stable() {
        let a = normalize_object_info(&json!({}), &classes(&["X"])).unwrap_err();
        let b = normalize_object_info(&json!({}), &classes(&["X"])).unwrap_err();
        let c = normalize_object_info(&json!(null), &classes(&["X"])).unwrap_err();
        assert_eq!(a.diagnostic_id, b.diagnostic_id);
        assert_ne!(a.diagnostic_id, c.diagnostic_id);
    }
}

// ---------------------------------------------------------------------------------------------
// Transmitted key order (frontend 1.53.10 DynamicCombo child order)
// ---------------------------------------------------------------------------------------------

/// Parse a raw `/object_info` response and record, for every `COMFY_DYNAMICCOMBO_V3` option that
/// has no explicit `inputs.input_order`, the transmitted key order of its `required` / `optional`
/// sections as `input_order`. The official frontend builds DynamicCombo children in exactly this
/// order (JavaScript object insertion order of the parsed response); `serde_json::Value` would
/// lose it, so it is captured here, at the single parse point, as explicit evidence.
pub fn parse_object_info_text(text: &str) -> Result<Value, serde_json::Error> {
    let mut value: Value = serde_json::from_str(text)?;
    let ordered: OrderedJson = serde_json::from_str(text)?;
    annotate(&ordered, &mut value);
    Ok(value)
}

enum OrderedJson {
    Object(Vec<(String, OrderedJson)>),
    Array(Vec<OrderedJson>),
    Leaf,
}

impl<'de> serde::Deserialize<'de> for OrderedJson {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = OrderedJson;
            fn expecting(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut m: A) -> Result<OrderedJson, A::Error> {
                let mut entries = Vec::new();
                while let Some((k, v)) = m.next_entry::<String, OrderedJson>()? {
                    entries.push((k, v));
                }
                Ok(OrderedJson::Object(entries))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut s: A) -> Result<OrderedJson, A::Error> {
                let mut items = Vec::new();
                while let Some(v) = s.next_element::<OrderedJson>()? {
                    items.push(v);
                }
                Ok(OrderedJson::Array(items))
            }
            fn visit_bool<E>(self, _: bool) -> Result<OrderedJson, E> { Ok(OrderedJson::Leaf) }
            fn visit_i64<E>(self, _: i64) -> Result<OrderedJson, E> { Ok(OrderedJson::Leaf) }
            fn visit_u64<E>(self, _: u64) -> Result<OrderedJson, E> { Ok(OrderedJson::Leaf) }
            fn visit_f64<E>(self, _: f64) -> Result<OrderedJson, E> { Ok(OrderedJson::Leaf) }
            fn visit_str<E>(self, _: &str) -> Result<OrderedJson, E> { Ok(OrderedJson::Leaf) }
            fn visit_unit<E>(self) -> Result<OrderedJson, E> { Ok(OrderedJson::Leaf) }
            fn visit_none<E>(self) -> Result<OrderedJson, E> { Ok(OrderedJson::Leaf) }
        }
        d.deserialize_any(V)
    }
}

impl OrderedJson {
    fn get(&self, key: &str) -> Option<&OrderedJson> {
        match self {
            // Last duplicate wins, as in serde_json::Value.
            OrderedJson::Object(entries) => entries.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn keys(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let OrderedJson::Object(entries) = self {
            for (k, _) in entries {
                if !out.contains(k) {
                    out.push(k.clone());
                }
            }
        }
        out
    }
}

fn annotate(ordered: &OrderedJson, value: &mut Value) {
    match (ordered, value) {
        (OrderedJson::Array(items), Value::Array(values)) => {
            let dynamic = values.first().and_then(Value::as_str) == Some("COMFY_DYNAMICCOMBO_V3");
            if dynamic {
                if let (Some(OrderedJson::Array(o_opts)), Some(v_opts)) = (
                    items.get(1).and_then(|o| o.get("options")),
                    values.get_mut(1).and_then(|v| v.get_mut("options")).and_then(Value::as_array_mut),
                ) {
                    for (o_opt, v_opt) in o_opts.iter().zip(v_opts.iter_mut()) {
                        record_option_order(o_opt, v_opt);
                    }
                }
            }
            for (o, v) in items.iter().zip(values.iter_mut()) {
                annotate(o, v);
            }
        }
        (OrderedJson::Object(_), Value::Object(map)) => {
            for (k, v) in map.iter_mut() {
                if let Some(o) = ordered.get(k) {
                    annotate(o, v);
                }
            }
        }
        _ => {}
    }
}

fn record_option_order(o_opt: &OrderedJson, v_opt: &mut Value) {
    let Some(o_inputs) = o_opt.get("inputs") else { return };
    let Some(v_inputs) = v_opt.get_mut("inputs").and_then(Value::as_object_mut) else { return };
    if v_inputs.contains_key("input_order") {
        return;
    }
    let mut order = serde_json::Map::new();
    for category in ["required", "optional"] {
        if let Some(section) = o_inputs.get(category) {
            if matches!(section, OrderedJson::Object(_)) {
                order.insert(category.to_string(), Value::from(section.keys()));
            }
        }
    }
    v_inputs.insert("input_order".to_string(), Value::Object(order));
}
