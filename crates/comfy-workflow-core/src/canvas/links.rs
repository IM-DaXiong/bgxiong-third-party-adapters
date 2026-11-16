//! Node/link id normalization (CW19) and link resolution across reroutes, primitives,
//! mute/bypass (CW23, CW24) and subgraph boundaries (CW21).
//!
//! Ids are normalized by the source format: integer ids become their decimal string, string ids
//! are kept verbatim; a missing or malformed id/slot is an error, never `0`.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};

use alloc::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::types::CompileDiagnostic;

use super::ir::{codes, Diag, LinkRef, NodeKey, NodeMode, OverrideSource};

/// Litegraph subgraph boundary pseudo-node ids (official subgraph format).
pub(crate) const SUBGRAPH_INPUT_NODE: &str = "-10";
pub(crate) const SUBGRAPH_OUTPUT_NODE: &str = "-20";

/// Frontend-only node classes of the official ComfyUI frontend (virtual, never executed).
pub(crate) const REROUTE_CLASS: &str = "Reroute";
pub(crate) const PRIMITIVE_CLASS: &str = "PrimitiveNode";

pub fn is_annotation_class(class_type: &str) -> bool {
    matches!(class_type, "Note" | "MarkdownNote")
}

pub(crate) fn is_virtual_class(class_type: &str) -> bool {
    is_annotation_class(class_type) || class_type == REROUTE_CLASS || class_type == PRIMITIVE_CLASS
}

/// Source id → canonical string. Integers (any sign) keep their decimal form; non-empty strings
/// are kept verbatim; floats, empty strings and other JSON kinds are rejected.
pub(crate) fn parse_node_id(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i.to_string())
            } else {
                n.as_u64().map(|u| u.to_string())
            }
        }
        _ => None,
    }
}
pub(crate) fn parse_slot(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|s| u32::try_from(s).ok())
}

/// Litegraph modes: 0 ALWAYS, 1 ON_EVENT, 3 ON_TRIGGER execute; 2 NEVER mutes; 4 bypasses.
pub(crate) fn parse_mode(v: &Value) -> Option<NodeMode> {
    match v.as_u64()? {
        0 | 1 | 3 => Some(NodeMode::Normal),
        2 => Some(NodeMode::Muted),
        4 => Some(NodeMode::Bypassed),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RawLink {
    pub id: String,
    pub origin: String,
    pub origin_slot: u32,
    pub target: String,
    pub target_slot: u32,
}

/// Canvas 0.4 array `[id, origin_id, origin_slot, target_id, target_slot, type]` or the object
/// form used by Canvas 1.0 and subgraph definitions. Returns the offending field on error.
pub(crate) fn parse_link(v: &Value) -> Result<RawLink, &'static str> {
    let (id, origin, origin_slot, target, target_slot, ty) = match v {
        Value::Array(a) => {
            if a.len() != 6 {
                return Err("arity");
            }
            (&a[0], &a[1], &a[2], &a[3], &a[4], &a[5])
        }
        Value::Object(o) => (
            o.get("id").ok_or("id")?,
            o.get("origin_id").ok_or("origin_id")?,
            o.get("origin_slot").ok_or("origin_slot")?,
            o.get("target_id").ok_or("target_id")?,
            o.get("target_slot").ok_or("target_slot")?,
            o.get("type").ok_or("type")?,
        ),
        _ => return Err("shape"),
    };
    // The link type is validated for shape only: the edge type is the origin socket's.
    if !ty.is_string() {
        return Err("type");
    }
    Ok(RawLink {
        id: parse_node_id(id).ok_or("id")?,
        origin: parse_node_id(origin).ok_or("origin_id")?,
        origin_slot: parse_slot(origin_slot).ok_or("origin_slot")?,
        target: parse_node_id(target).ok_or("target_id")?,
        target_slot: parse_slot(target_slot).ok_or("target_slot")?,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct Socket {
    pub name: String,
    pub ty: Option<String>,
    pub link: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct OutSocket {
    pub name: String,
    pub ty: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RawNode {
    pub local_id: String,
    pub class_type: String,
    pub mode: NodeMode,
    pub inputs: Vec<Socket>,
    pub outputs: Vec<OutSocket>,
    pub widgets_values: Option<Value>,
    pub title: Option<String>,
    pub properties: Option<Value>,
}

#[derive(Debug, Clone)]
pub(crate) struct PortDef {
    pub name: String,
    /// Every interior consumer of this port is a widget socket (`widget: {..}`): the port is a
    /// promoted widget and owns one position in the instance's promoted `widgets_values`.
    pub widget: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SubgraphDef {
    pub inputs: Vec<PortDef>,
    pub outputs: Vec<PortDef>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Scope {
    pub nodes: BTreeMap<String, RawNode>,
    pub links: BTreeMap<String, RawLink>,
    /// Instance node local id → subgraph definition id.
    pub instances: BTreeMap<String, String>,
}

/// Phase-1 result: every scope, definition and instance registered before any link is resolved.
#[derive(Debug, Clone, Default)]
pub(crate) struct RegisteredGraph {
    pub scopes: BTreeMap<Vec<String>, Scope>,
    pub defs: BTreeMap<String, SubgraphDef>,
}

/// The instance stores promoted widgets by proxy: no `widgets_values` copy and an explicit
/// `properties.proxyWidgets` list (ComfyUI frontend >= 1.2x subgraph widget promotion).
pub(crate) fn promoted_by_proxy(inst: &RawNode) -> bool {
    let empty = match &inst.widgets_values {
        None | Some(Value::Null) => true,
        Some(Value::Array(a)) => a.is_empty(),
        Some(_) => false,
    };
    empty
        && inst
            .properties
            .as_ref()
            .and_then(|p| p.get("proxyWidgets"))
            .is_some_and(Value::is_array)
}

pub(crate) enum Resolved {
    Link(LinkRef),
    Override(Value, OverrideSource),
    /// No executable source (muted, unconnected boundary, bypass without matching input).
    /// The consumer input is omitted, as the official serializer does; reported non-blocking.
    Dropped {
        code: &'static str,
        reason: &'static str,
    },
    /// Proxy-promoted widget (instance `widgets_values` empty, `properties.proxyWidgets` set):
    /// the instance stores no copy, the interior consumer keeps its own widget value, exactly as
    /// the official frontend (1.53.10 oracle) serializes it. The consumer input stays unlinked.
    InteriorWidget,
    /// A blocking diagnostic has already been recorded.
    Failed,
}

pub(crate) struct Resolver<'g> {
    pub graph: &'g RegisteredGraph,
    pub diagnostics: Vec<CompileDiagnostic>,
}

fn key_of(scope: &[String], local: &str) -> NodeKey {
    NodeKey {
        scope: scope.to_vec(),
        local_id: local.to_string(),
    }
}

impl<'g> Resolver<'g> {
    pub(crate) fn new(graph: &'g RegisteredGraph) -> Self {
        Self {
            graph,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn node(&self, scope: &[String], local: &str) -> Option<&'g RawNode> {
        self.graph
            .scopes
            .get(scope)
            .and_then(|s| s.nodes.get(local))
    }

    /// Own mode combined with every enclosing instance: muted anywhere mutes; otherwise
    /// bypassed anywhere bypasses.
    pub(crate) fn effective_mode(&self, scope: &[String], local: &str) -> Option<NodeMode> {
        let mut modes = vec![self.node(scope, local)?.mode];
        for depth in 0..scope.len() {
            modes.push(self.node(&scope[..depth], &scope[depth])?.mode);
        }
        if modes.contains(&NodeMode::Muted) {
            Some(NodeMode::Muted)
        } else if modes.contains(&NodeMode::Bypassed) {
            Some(NodeMode::Bypassed)
        } else {
            Some(NodeMode::Normal)
        }
    }

    fn fail(&mut self, d: Diag) -> Resolved {
        self.diagnostics.push(d.build());
        Resolved::Failed
    }

    pub(crate) fn resolve_link(
        &mut self,
        scope: &[String],
        link_id: &str,
        consumer: &NodeKey,
        input: &str,
        visited: &mut BTreeSet<(Vec<String>, String, u32)>,
    ) -> Resolved {
        let graph = self.graph;
        let Some(link) = graph.scopes.get(scope).and_then(|s| s.links.get(link_id)) else {
            return self.fail(
                Diag::blocking(codes::LINK_MISSING)
                    .at(consumer)
                    .input(input)
                    .param("linkId", link_id),
            );
        };
        self.resolve_origin(
            scope,
            &link.origin,
            link.origin_slot,
            consumer,
            input,
            visited,
        )
    }

    pub(crate) fn resolve_origin(
        &mut self,
        scope: &[String],
        origin: &str,
        slot: u32,
        consumer: &NodeKey,
        input: &str,
        visited: &mut BTreeSet<(Vec<String>, String, u32)>,
    ) -> Resolved {
        if !visited.insert((scope.to_vec(), origin.to_string(), slot)) {
            return self.fail(
                Diag::blocking(codes::LINK_RESOLUTION_CYCLE)
                    .at(consumer)
                    .input(input)
                    .param("via", key_of(scope, origin).exec_id()),
            );
        }
        let graph = self.graph;
        if origin == SUBGRAPH_INPUT_NODE {
            return self.resolve_boundary_input(scope, slot, consumer, input, visited);
        }
        let Some(node) = self.node(scope, origin) else {
            return self.fail(
                Diag::blocking(codes::LINK_ORIGIN_MISSING)
                    .at(consumer)
                    .input(input)
                    .param("origin", key_of(scope, origin).exec_id()),
            );
        };
        let Some(mode) = self.effective_mode(scope, origin) else {
            return self.fail(
                Diag::blocking(codes::LINK_ORIGIN_MISSING)
                    .at(consumer)
                    .input(input)
                    .param("origin", key_of(scope, origin).exec_id()),
            );
        };
        match mode {
            NodeMode::Muted => {
                return Resolved::Dropped {
                    code: codes::LINK_SOURCE_DROPPED,
                    reason: "sourceMuted",
                }
            }
            NodeMode::Bypassed => {
                return self.resolve_bypass(scope, node, slot, consumer, input, visited)
            }
            NodeMode::Normal => {}
        }
        let instances = &graph.scopes[scope].instances;
        if instances.contains_key(origin) {
            return self.resolve_instance_output(scope, node, slot, consumer, input, visited);
        }
        if node.class_type == REROUTE_CLASS {
            return match node.inputs.first().and_then(|s| s.link.clone()) {
                Some(l) => self.resolve_link(scope, &l, consumer, input, visited),
                None => Resolved::Dropped {
                    code: codes::LINK_SOURCE_DROPPED,
                    reason: "rerouteUnconnected",
                },
            };
        }
        if node.class_type == PRIMITIVE_CLASS {
            return match node
                .widgets_values
                .as_ref()
                .and_then(|w| w.as_array())
                .and_then(|a| a.first())
            {
                Some(v) => Resolved::Override(
                    v.clone(),
                    OverrideSource::Primitive {
                        node: key_of(scope, origin),
                    },
                ),
                None => self.fail(
                    Diag::blocking(codes::PRIMITIVE_VALUE_MISSING)
                        .at(consumer)
                        .input(input)
                        .param("primitive", key_of(scope, origin).exec_id()),
                ),
            };
        }
        if is_annotation_class(&node.class_type) {
            return self.fail(
                Diag::blocking(codes::ANNOTATION_AS_SOURCE)
                    .at(consumer)
                    .input(input)
                    .param("origin", key_of(scope, origin).exec_id()),
            );
        }
        let Some(out) = node.outputs.get(slot as usize) else {
            return self.fail(
                Diag::blocking(codes::LINK_SLOT_OUT_OF_RANGE)
                    .at(consumer)
                    .input(input)
                    .param("origin", key_of(scope, origin).exec_id())
                    .param("slot", slot),
            );
        };
        Resolved::Link(LinkRef {
            from: key_of(scope, origin),
            output_slot: slot,
            output_name: Some(out.name.clone()),
            output_type: out.ty.clone(),
        })
    }

    fn resolve_boundary_input(
        &mut self,
        scope: &[String],
        slot: u32,
        consumer: &NodeKey,
        input: &str,
        visited: &mut BTreeSet<(Vec<String>, String, u32)>,
    ) -> Resolved {
        let graph = self.graph;
        let Some((inst_id, parent)) = scope.split_last() else {
            return self.fail(
                Diag::blocking(codes::LINK_ORIGIN_MISSING)
                    .at(consumer)
                    .input(input)
                    .param("origin", SUBGRAPH_INPUT_NODE),
            );
        };
        let Some(inst) = self.node(parent, inst_id) else {
            return self.fail(
                Diag::blocking(codes::BOUNDARY_PORT_MISSING)
                    .at(consumer)
                    .input(input),
            );
        };
        let def = graph.scopes[parent]
            .instances
            .get(inst_id)
            .and_then(|d| graph.defs.get(d));
        let Some(port) = def.and_then(|d| d.inputs.get(slot as usize)) else {
            return self.fail(
                Diag::blocking(codes::BOUNDARY_PORT_MISSING)
                    .at(consumer)
                    .input(input)
                    .param("port", slot),
            );
        };
        let matches: Vec<usize> = inst
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.name == port.name)
            .map(|(i, _)| i)
            .collect();
        let socket = match matches.as_slice() {
            [i] => Some(&inst.inputs[*i]),
            [] => None,
            _ => {
                return self.fail(
                    Diag::blocking(codes::BOUNDARY_PORT_AMBIGUOUS)
                        .at(consumer)
                        .input(input)
                        .param("instance", key_of(parent, inst_id).exec_id())
                        .param("port", &port.name),
                )
            }
        };
        if let Some(l) = socket.and_then(|s| s.link.clone()) {
            return self.resolve_link(parent, &l, consumer, input, visited);
        }
        if port.widget {
            if promoted_by_proxy(inst) {
                return Resolved::InteriorWidget;
            }
            // Promoted values follow the definition's widget ports in port order; ports the
            // instance does not list as sockets still own their position.
            let ports = def.map(|d| d.inputs.as_slice()).unwrap_or_default();
            let widget_index = ports[..slot as usize].iter().filter(|p| p.widget).count();
            let value = inst
                .widgets_values
                .as_ref()
                .and_then(Value::as_array)
                .and_then(|a| a.get(widget_index));
            return match value {
                Some(v) => Resolved::Override(
                    v.clone(),
                    OverrideSource::Promoted {
                        instance: key_of(parent, inst_id),
                    },
                ),
                None => self.fail(
                    Diag::blocking(codes::PROMOTED_WIDGET_COUNT_MISMATCH)
                        .at(consumer)
                        .input(input)
                        .param("instance", key_of(parent, inst_id).exec_id())
                        .param("widgetIndex", widget_index),
                ),
            };
        }
        if socket.is_none() {
            return self.fail(
                Diag::blocking(codes::BOUNDARY_PORT_MISSING)
                    .at(consumer)
                    .input(input)
                    .param("instance", key_of(parent, inst_id).exec_id())
                    .param("port", &port.name),
            );
        }
        Resolved::Dropped {
            code: codes::BOUNDARY_INPUT_UNCONNECTED,
            reason: "boundaryInputUnconnected",
        }
    }

    fn resolve_instance_output(
        &mut self,
        scope: &[String],
        inst: &RawNode,
        slot: u32,
        consumer: &NodeKey,
        input: &str,
        visited: &mut BTreeSet<(Vec<String>, String, u32)>,
    ) -> Resolved {
        let graph = self.graph;
        let inst_key = key_of(scope, &inst.local_id);
        let def = graph.scopes[scope]
            .instances
            .get(&inst.local_id)
            .and_then(|d| graph.defs.get(d));
        let Some(def) = def else {
            return self.fail(
                Diag::blocking(codes::SUBGRAPH_DEFINITION_INVALID)
                    .at(consumer)
                    .input(input),
            );
        };
        let Some(port) = def.outputs.get(slot as usize) else {
            return self.fail(
                Diag::blocking(codes::BOUNDARY_PORT_MISSING)
                    .at(consumer)
                    .input(input)
                    .param("instance", inst_key.exec_id())
                    .param("port", slot),
            );
        };
        if inst.outputs.get(slot as usize).map(|o| o.name.as_str()) != Some(port.name.as_str()) {
            return self.fail(
                Diag::blocking(codes::BOUNDARY_PORT_MISMATCH)
                    .at(consumer)
                    .input(input)
                    .param("instance", inst_key.exec_id())
                    .param("port", &port.name),
            );
        }
        let mut child: Vec<String> = scope.to_vec();
        child.push(inst.local_id.clone());
        let Some(child_scope) = graph.scopes.get(&child) else {
            return self.fail(
                Diag::blocking(codes::SUBGRAPH_DEFINITION_INVALID)
                    .at(consumer)
                    .input(input),
            );
        };
        let inner: Vec<&RawLink> = child_scope
            .links
            .values()
            .filter(|l| l.target == SUBGRAPH_OUTPUT_NODE && l.target_slot == slot)
            .collect();
        match inner.as_slice() {
            [l] => {
                let (origin, origin_slot) = (l.origin.clone(), l.origin_slot);
                self.resolve_origin(&child, &origin, origin_slot, consumer, input, visited)
            }
            [] => Resolved::Dropped {
                code: codes::BOUNDARY_OUTPUT_UNCONNECTED,
                reason: "boundaryOutputUnconnected",
            },
            _ => self.fail(
                Diag::blocking(codes::BOUNDARY_PORT_AMBIGUOUS)
                    .at(consumer)
                    .input(input)
                    .param("instance", inst_key.exec_id())
                    .param("port", &port.name),
            ),
        }
    }

    /// Bypass passes an input through to the output of the same type. Official order: the input
    /// at the same slot index when its type matches, else the unique input of that type. Several
    /// candidates without that proof are ambiguous and block; none drops the value.
    fn resolve_bypass(
        &mut self,
        scope: &[String],
        node: &RawNode,
        slot: u32,
        consumer: &NodeKey,
        input: &str,
        visited: &mut BTreeSet<(Vec<String>, String, u32)>,
    ) -> Resolved {
        let key = key_of(scope, &node.local_id);
        let Some(out_ty) = node.outputs.get(slot as usize).and_then(|o| o.ty.clone()) else {
            return self.fail(
                Diag::blocking(codes::BYPASS_AMBIGUOUS)
                    .at(consumer)
                    .input(input)
                    .param("bypassed", key.exec_id())
                    .param("reason", "outputTypeUnknown"),
            );
        };
        let same_index = node
            .inputs
            .get(slot as usize)
            .filter(|s| s.ty.as_deref() == Some(out_ty.as_str()));
        let chosen = match same_index {
            Some(s) => s,
            None => {
                let candidates: Vec<&Socket> = node
                    .inputs
                    .iter()
                    .filter(|s| s.ty.as_deref() == Some(out_ty.as_str()))
                    .collect();
                match candidates.as_slice() {
                    [s] => *s,
                    [] => {
                        return Resolved::Dropped {
                            code: codes::LINK_SOURCE_DROPPED,
                            reason: "bypassNoMatchingInput",
                        }
                    }
                    _ => {
                        return self.fail(
                            Diag::blocking(codes::BYPASS_AMBIGUOUS)
                                .at(consumer)
                                .input(input)
                                .param("bypassed", key.exec_id())
                                .param("type", &out_ty)
                                .param("candidates", candidates.len()),
                        )
                    }
                }
            }
        };
        match chosen.link.clone() {
            Some(l) => self.resolve_link(scope, &l, consumer, input, visited),
            None => Resolved::Dropped {
                code: codes::LINK_SOURCE_DROPPED,
                reason: "bypassInputUnconnected",
            },
        }
    }
}
