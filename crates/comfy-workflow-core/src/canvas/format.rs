//! Source format detection (CW18[L02]) and the dict-shaped parsers: API prompt literal
//! preservation (CW26[L02]) and flat UI dictionaries.
//!
//! Detection is purely structural (never by file name or extension); multiple matches are
//! reported as ambiguous instead of picking one.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec, boxed::Box};

use alloc::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::types::CompileDiagnostic;

use super::ir::{codes, structural_sinks, Diag, GraphIrV1, InputValue, IrNode, LinkRef, NodeAttrsV1, NodeKey, NodeMode};
use super::links::{is_annotation_class, parse_mode};

pub const AUTHORED_OUTPUT_KEY: &str = "bgxiongOutputSelection";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceFormat {
    ApiPrompt,
    FlatUi,
    Canvas04,
    Canvas10,
}

pub fn detect_format(raw: &Value) -> Result<SourceFormat, Box<CompileDiagnostic>> {
    let Some(obj) = raw.as_object() else {
        return Err(Box::new(Diag::blocking(codes::TOP_LEVEL_NOT_OBJECT).build()));
    };
    if obj.is_empty() {
        return Err(Box::new(Diag::blocking(codes::GRAPH_EMPTY).build()));
    }
    if let Some(nodes) = obj.get("nodes") {
        if !nodes.is_array() {
            return Err(Box::new(Diag::blocking(codes::FORMAT_UNRECOGNIZED).param("key", "nodes").build()));
        }
        if !obj.get("links").is_some_and(Value::is_array) {
            return Err(Box::new(Diag::blocking(codes::FORMAT_UNRECOGNIZED).param("key", "links").build()));
        }
        return match obj.get("version") {
            Some(v) if v.as_u64() == Some(1) => Ok(SourceFormat::Canvas10),
            Some(v) if v.as_f64() == Some(0.4) => Ok(SourceFormat::Canvas04),
            Some(v) => Err(Box::new(Diag::blocking(codes::FORMAT_VERSION_UNSUPPORTED)
                .param("version", v.to_string())
                .build())),
            None => Err(Box::new(Diag::blocking(codes::FORMAT_VERSION_UNSUPPORTED)
                .param("version", "missing")
                .build())),
        };
    }
    let mut with_pos = 0usize;
    for (id, node) in obj {
        let Some(node) = node.as_object() else {
            return Err(Box::new(Diag::blocking(codes::FORMAT_UNRECOGNIZED).param("key", id).build()));
        };
        if node.contains_key("pos") {
            with_pos += 1;
        }
    }
    if with_pos == 0 {
        Ok(SourceFormat::ApiPrompt)
    } else if with_pos == obj.len() {
        Ok(SourceFormat::FlatUi)
    } else {
        Err(Box::new(Diag::blocking(codes::FORMAT_AMBIGUOUS)
            .param("nodesWithLayout", with_pos)
            .param("nodes", obj.len())
            .build()))
    }
}

/// Parse raw bytes (a leading UTF-8 BOM is skipped for reading; the stored bytes are untouched)
/// and detect the format.
pub(crate) fn detect_format_bytes(raw: &[u8]) -> Result<(Value, SourceFormat), Box<CompileDiagnostic>> {
    let body = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(raw);
    if body.iter().all(u8::is_ascii_whitespace) {
        return Err(Box::new(Diag::blocking(codes::JSON_EMPTY).build()));
    }
    let value: Value = serde_json::from_slice(body).map_err(|e| {
        let category = match e.classify() {
            serde_json::error::Category::Io => "io",
            serde_json::error::Category::Syntax => "syntax",
            serde_json::error::Category::Data => "data",
            serde_json::error::Category::Eof => "eof",
        };
        Diag::blocking(codes::JSON_PARSE_FAILED)
            .param("line", e.line())
            .param("column", e.column())
            .param("category", category)
            .build()
    })?;
    let format = detect_format(&value)?;
    Ok((value, format))
}

/// Official API link shape (`is_link`): exactly `[node_id: string, output_slot: non-negative int]`.
/// Any other array (including other two-element arrays) is a literal.
fn api_link_parts(v: &Value) -> Option<(&str, u64)> {
    let arr = v.as_array()?;
    if arr.len() != 2 {
        return None;
    }
    let id = arr[0].as_str()?;
    let slot = arr[1].as_u64()?;
    Some((id, slot))
}

fn title_of(format: SourceFormat, node: &Map<String, Value>) -> Option<String> {
    match format {
        SourceFormat::ApiPrompt => node
            .get("_meta")
            .and_then(|m| m.get("title"))
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => node.get("title").and_then(Value::as_str).map(str::to_string),
    }
}

/// API prompt / flat UI dictionaries. API literal values are preserved verbatim (CW26).
pub(crate) fn parse_dict_graph(
    value: &Value,
    format: SourceFormat,
) -> Result<GraphIrV1, Vec<CompileDiagnostic>> {
    let Some(obj) = value.as_object() else {
        return Err(vec![Diag::blocking(codes::TOP_LEVEL_NOT_OBJECT).build()]);
    };
    let mut diagnostics = Vec::new();
    let mut nodes = Vec::new();
    let mut node_attrs = BTreeMap::new();
    for (id, node) in obj {
        if id == AUTHORED_OUTPUT_KEY {
            continue;
        }
        if id.is_empty() {
            diagnostics.push(Diag::blocking(codes::NODE_ID_INVALID).param("nodeId", "").build());
            continue;
        }
        let key = NodeKey::root(id.clone());
        let Some(node) = node.as_object() else {
            diagnostics.push(Diag::blocking(codes::NODE_INVALID).at(&key).build());
            continue;
        };
        let Some(class_type) = node.get("class_type").and_then(Value::as_str).filter(|c| !c.is_empty())
        else {
            diagnostics.push(Diag::blocking(codes::MISSING_CLASS_TYPE).at(&key).build());
            continue;
        };
        if is_annotation_class(class_type) {
            continue;
        }
        let mode = match format {
            SourceFormat::ApiPrompt => NodeMode::Normal,
            _ => match node.get("mode") {
                // Flat UI dictionaries carry `mode` only when the exporter wrote it; absence means
                // the node was never muted/bypassed in that export.
                None => NodeMode::Normal,
                Some(m) => match parse_mode(m) {
                    Some(mode) => mode,
                    None => {
                        diagnostics.push(
                            Diag::blocking(codes::NODE_MODE_INVALID)
                                .at(&key)
                                .class(class_type)
                                .param("mode", m.to_string())
                                .build(),
                        );
                        continue;
                    }
                },
            },
        };
        let raw_inputs = match (format, node.get("inputs")) {
            (_, Some(Value::Object(map))) => Some(map),
            (SourceFormat::FlatUi, None) => None,
            _ => {
                diagnostics.push(
                    Diag::blocking(codes::NODE_INVALID)
                        .at(&key)
                        .class(class_type)
                        .param("field", "inputs")
                        .build(),
                );
                continue;
            }
        };
        let mut inputs = BTreeMap::new();
        for (name, v) in raw_inputs.into_iter().flatten() {
            match api_link_parts(v) {
                Some((src, slot)) => {
                    let target_exists = obj
                        .get(src)
                        .and_then(|n| n.get("class_type"))
                        .and_then(Value::as_str)
                        .is_some_and(|c| !is_annotation_class(c));
                    let Ok(slot) = u32::try_from(slot) else {
                        diagnostics.push(
                            Diag::blocking(codes::LINK_SLOT_OUT_OF_RANGE)
                                .at(&key)
                                .class(class_type)
                                .input(name)
                                .param("slot", slot)
                                .build(),
                        );
                        continue;
                    };
                    if !target_exists {
                        diagnostics.push(
                            Diag::blocking(codes::API_LINK_TARGET_MISSING)
                                .at(&key)
                                .class(class_type)
                                .input(name)
                                .param("sourceId", src)
                                .build(),
                        );
                        continue;
                    }
                    inputs.insert(
                        name.clone(),
                        InputValue::Link(LinkRef {
                            from: NodeKey::root(src),
                            output_slot: slot,
                            output_name: None,
                            output_type: None,
                        }),
                    );
                }
                None => {
                    inputs.insert(name.clone(), InputValue::Literal(v.clone()));
                }
            }
        }
        let widgets_values = match format {
            SourceFormat::ApiPrompt => {
                if node.contains_key("widgets_values") {
                    // API prompts are executed from named inputs only; a stored widgets_values
                    // sidecar is never sent to ComfyUI. Reported explicitly, not silently.
                    diagnostics.push(
                        Diag::non_blocking(codes::API_WIDGETS_SIDECAR_NOT_EXECUTED)
                            .at(&key)
                            .class(class_type)
                            .build(),
                    );
                }
                None
            }
            _ => node.get("widgets_values").cloned(),
        };
        nodes.push(IrNode {
            key: key.clone(),
            class_type: class_type.to_string(),
            mode,
            inputs,
            widgets_values,
            source_locator: key.locator(),
        });
        node_attrs.insert(
            key.exec_id(),
            NodeAttrsV1 { title: title_of(format, node), properties: node.get("properties").cloned(), overrides: Vec::new() },
        );
    }
    let outputs = structural_sinks(&nodes);
    Ok(GraphIrV1 { format, nodes, outputs, diagnostics, node_attrs })
}
