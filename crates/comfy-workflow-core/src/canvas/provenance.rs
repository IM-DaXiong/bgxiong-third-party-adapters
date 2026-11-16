//! Compile provenance (CW10[L02], CW27) and annotation collection before stripping (CW62[L02]).
//!
//! Every API input carries its origin, every source widget position is consumed, UI-only or
//! blocked, and every execution id maps back to its source locator.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};

use alloc::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{CompileDiagnostic, DiagnosticSeverity, NodeLocator};

use super::ir::{codes, Diag, NodeKey};
use super::links::{is_annotation_class, parse_node_id};
use super::widgets::WidgetConsumption;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceMapV1 {
    pub exec_to_source: BTreeMap<String, NodeLocator>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ValueOrigin {
    SourceWidget { index: u32 },
    NamedApi,
    PromotedOverride { instance: NodeKey },
    DefinitionDefault,
    SourceDefault,
    Link,
    /// Value of a frontend `PrimitiveNode` applied to this input (the primitive is virtual).
    PrimitiveWidget { node: NodeKey },
    /// Value serialized by the official frontend through verified bridge evidence.
    BridgeSerialized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputProvenance {
    pub exec_id: String,
    pub input_name: String,
    pub origin: ValueOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactDigests {
    pub graph: String,
    pub definition: String,
    pub serializer: String,
    pub rules: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledArtifactV1 {
    pub api_prompt: Value,
    pub source_map: SourceMapV1,
    pub inputs: Vec<InputProvenance>,
    pub retained: Vec<String>,
    pub pruned: Vec<(String, String)>,
    pub diagnostics: Vec<CompileDiagnostic>,
    pub digests: ArtifactDigests,
}

impl CompiledArtifactV1 {
    /// Reverse lookup used by mapping/UI: execution id → source locator.
    pub fn source_of(&self, exec_id: &str) -> Option<&NodeLocator> {
        self.source_map.exec_to_source.get(exec_id)
    }
}

/// CW27: every widget position `0..count` must be accounted exactly once. `Blocked` entries
/// surface their own diagnostic.
pub(crate) fn account_widgets(
    key: &NodeKey,
    class_type: &str,
    widgets_values: Option<&Value>,
    consumptions: &[WidgetConsumption],
) -> Vec<CompileDiagnostic> {
    let count = match widgets_values {
        None | Some(Value::Null) => 0,
        Some(Value::Array(a)) => a.len(),
        Some(Value::Object(o)) => o.len(),
        Some(_) => 1,
    };
    let mut out = Vec::new();
    let mut seen = vec![0u32; count];
    for c in consumptions {
        let index = match c {
            WidgetConsumption::NamedInput { index, .. }
            | WidgetConsumption::UiOnly { index, .. }
            | WidgetConsumption::Linked { index, .. } => *index,
            WidgetConsumption::Blocked { index, diagnostic } => {
                let mut d = diagnostic.clone();
                d.severity = DiagnosticSeverity::Blocking;
                out.push(d);
                *index
            }
        };
        match seen.get_mut(index as usize) {
            Some(n) => *n += 1,
            None => out.push(
                Diag::blocking(codes::WIDGET_INDEX_OUT_OF_RANGE)
                    .at(key)
                    .class(class_type)
                    .param("index", index)
                    .param("count", count)
                    .build(),
            ),
        }
    }
    for (index, n) in seen.iter().enumerate() {
        match n {
            1 => {}
            0 => out.push(
                Diag::blocking(codes::WIDGET_UNACCOUNTED).at(key).class(class_type).param("index", index).build(),
            ),
            _ => out.push(
                Diag::blocking(codes::WIDGET_DOUBLE_ACCOUNTED)
                    .at(key)
                    .class(class_type)
                    .param("index", index)
                    .param("times", n)
                    .build(),
            ),
        }
    }
    out
}

/// Raw author annotation, captured from the original document before any annotation stripping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoteUnitRaw {
    /// Root graph: `[]`. Inside a subgraph definition: `["#definition", <definition id>]`
    /// (a definition note is shared by all its instances).
    pub scope_path: Vec<String>,
    pub node_id: Option<String>,
    /// `note` | `markdownNote` | `group` | `workflowInfo.<field>`
    pub kind: String,
    pub text: String,
}

fn note_text(node: &serde_json::Map<String, Value>) -> String {
    let from_widgets = node.get("widgets_values").and_then(|w| match w {
        Value::Array(a) => a.first(),
        other => Some(other),
    });
    let from_inputs = node.get("inputs").and_then(|i| i.get("text"));
    match from_widgets.or(from_inputs) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn note_kind(class_type: &str) -> &'static str {
    if class_type == "MarkdownNote" {
        "markdownNote"
    } else {
        "note"
    }
}

fn collect_canvas_scope(scope: &[String], obj: &serde_json::Map<String, Value>, out: &mut Vec<NoteUnitRaw>) {
    for node in obj.get("nodes").and_then(Value::as_array).into_iter().flatten() {
        let Some(n) = node.as_object() else { continue };
        let Some(class_type) = n.get("type").and_then(Value::as_str) else { continue };
        if is_annotation_class(class_type) {
            out.push(NoteUnitRaw {
                scope_path: scope.to_vec(),
                node_id: n.get("id").and_then(parse_node_id),
                kind: note_kind(class_type).to_string(),
                text: note_text(n),
            });
        }
    }
    for group in obj.get("groups").and_then(Value::as_array).into_iter().flatten() {
        if let Some(title) = group.get("title").and_then(Value::as_str) {
            out.push(NoteUnitRaw {
                scope_path: scope.to_vec(),
                node_id: None,
                kind: "group".to_string(),
                text: title.to_string(),
            });
        }
    }
}

fn collect_definitions(obj: &serde_json::Map<String, Value>, out: &mut Vec<NoteUnitRaw>, seen: &mut alloc::collections::BTreeSet<String>) {
    for holder in ["definitions", "extra"] {
        let list = obj.get(holder).and_then(|h| h.get("subgraphs")).and_then(Value::as_array);
        for def in list.into_iter().flatten() {
            let Some(d) = def.as_object() else { continue };
            let Some(id) = d.get("id").and_then(Value::as_str) else { continue };
            if !seen.insert(id.to_string()) {
                continue;
            }
            collect_canvas_scope(&["#definition".to_string(), id.to_string()], d, out);
            collect_definitions(d, out, seen);
        }
    }
}

/// CW62[L02]: collect every Note / MarkdownNote, group title and workflow info string from the
/// original document. Must run on the raw source, before compile strips annotation nodes.
pub fn collect_annotation_units(raw: &Value) -> Vec<NoteUnitRaw> {
    let mut out = Vec::new();
    let Some(obj) = raw.as_object() else { return out };
    if obj.get("nodes").is_some_and(Value::is_array) {
        collect_canvas_scope(&[], obj, &mut out);
        collect_definitions(obj, &mut out, &mut alloc::collections::BTreeSet::new());
        if let Some(info) = obj.get("extra").and_then(|e| e.get("info")).and_then(Value::as_object) {
            for (field, value) in info {
                if let Some(text) = value.as_str() {
                    out.push(NoteUnitRaw {
                        scope_path: Vec::new(),
                        node_id: None,
                        kind: format!("workflowInfo.{field}"),
                        text: text.to_string(),
                    });
                }
            }
        }
    } else {
        for (id, node) in obj {
            let Some(n) = node.as_object() else { continue };
            let Some(class_type) = n.get("class_type").and_then(Value::as_str) else { continue };
            if is_annotation_class(class_type) {
                out.push(NoteUnitRaw {
                    scope_path: Vec::new(),
                    node_id: Some(id.clone()),
                    kind: note_kind(class_type).to_string(),
                    text: note_text(n),
                });
            }
        }
    }
    out
}
