//! Canvas 0.4 / 1.0 parsing with two-phase subgraph handling (CW20, CW21, CW22[L02]).
//!
//! Phase 1 registers every definition, scope and instance path (recursively, only for instances
//! that exist — unused definitions are never expanded). Phase 2 resolves every link over the whole
//! registered graph, so instance→instance, parent→child, child→parent, nested and fan-out links
//! all go through the same resolver. Promoted instance values become per-instance overrides on
//! local node copies; the shared definition is never touched.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};

use alloc::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::types::CompileDiagnostic;

use super::format::SourceFormat;
use super::ir::{codes, structural_sinks, Diag, GraphIrV1, InputValue, IrNode, NodeAttrsV1, NodeKey, NodeMode, WidgetOverride};
use super::links::{
    is_virtual_class, parse_link, promoted_by_proxy, parse_mode, parse_node_id, OutSocket, PortDef, RawNode, RegisteredGraph,
    Resolved, Resolver, Scope, Socket, SubgraphDef, SUBGRAPH_INPUT_NODE, SUBGRAPH_OUTPUT_NODE,
};

/// Nesting budget for subgraph instances. Exceeding it is an explicit, recoverable diagnostic.
pub const MAX_SUBGRAPH_DEPTH: usize = crate::contract::MAX_SUBGRAPH_DEPTH;

fn array_field<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a Vec<Value>> {
    obj.get(key).and_then(Value::as_array)
}

/// Definitions from `definitions.subgraphs` (and the older `extra.subgraphs` location), including
/// definitions nested inside definitions.
fn collect_definition_values<'a>(obj: &'a Map<String, Value>, out: &mut Vec<&'a Value>) {
    for holder in ["definitions", "extra"] {
        if let Some(list) = obj.get(holder).and_then(|h| h.get("subgraphs")).and_then(Value::as_array) {
            for def in list {
                out.push(def);
                if let Some(def_obj) = def.as_object() {
                    collect_definition_values(def_obj, out);
                }
            }
        }
    }
}

struct DefSource<'a> {
    def: SubgraphDef,
    nodes: &'a Vec<Value>,
    links: &'a Vec<Value>,
}

fn parse_ports(list: Option<&Vec<Value>>) -> Option<Vec<PortDef>> {
    let mut ports = Vec::new();
    for p in list? {
        let name = p.get("name").and_then(Value::as_str)?;
        ports.push(PortDef { name: name.to_string(), widget: false });
    }
    Some(ports)
}

/// Mark input ports whose every interior consumer socket is a widget socket. A port without
/// interior consumers is not a promoted widget.
fn mark_widget_ports(inputs: &mut [PortDef], nodes: &[Value], links: &[Value]) {
    let socket_is_widget = |target: &str, slot: u32| -> bool {
        nodes
            .iter()
            .find(|n| n.get("id").and_then(parse_node_id).as_deref() == Some(target))
            .and_then(|n| n.get("inputs")?.as_array()?.get(slot as usize))
            .is_some_and(|s| s.get("widget").is_some_and(Value::is_object))
    };
    let parsed: Vec<_> = links.iter().filter_map(|l| parse_link(l).ok()).collect();
    for (slot, port) in inputs.iter_mut().enumerate() {
        let mut consumers = parsed
            .iter()
            .filter(|l| l.origin == SUBGRAPH_INPUT_NODE && l.origin_slot as usize == slot)
            .peekable();
        // The frontend promotes a port as a widget when any consumer is a widget input (Qwen 2.1
        // edit `prompt` feeds ComfySwitchNode.on_false and TextGenerate.prompt; 1.53.10 oracle).
        port.widget = consumers.any(|l| socket_is_widget(&l.target, l.target_slot));
    }
}

fn register_definitions<'a>(
    root: &'a Map<String, Value>,
    diagnostics: &mut Vec<CompileDiagnostic>,
) -> BTreeMap<String, DefSource<'a>> {
    let mut values = Vec::new();
    collect_definition_values(root, &mut values);
    let mut raw_by_id: BTreeMap<String, &Value> = BTreeMap::new();
    let mut defs = BTreeMap::new();
    for value in values {
        let Some(obj) = value.as_object() else {
            diagnostics.push(Diag::blocking(codes::SUBGRAPH_DEFINITION_INVALID).param("field", "definition").build());
            continue;
        };
        let Some(id) = obj.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
            diagnostics.push(Diag::blocking(codes::SUBGRAPH_DEFINITION_INVALID).param("field", "id").build());
            continue;
        };
        if let Some(prev) = raw_by_id.get(id) {
            if *prev != value {
                diagnostics.push(Diag::blocking(codes::SUBGRAPH_DEFINITION_DUPLICATE).param("defId", id).build());
            }
            continue;
        }
        raw_by_id.insert(id.to_string(), value);
        let inputs = parse_ports(array_field(obj, "inputs"));
        let outputs = parse_ports(array_field(obj, "outputs"));
        let (Some(inputs), Some(outputs), Some(nodes), Some(links)) =
            (inputs, outputs, array_field(obj, "nodes"), array_field(obj, "links"))
        else {
            diagnostics.push(Diag::blocking(codes::SUBGRAPH_DEFINITION_INVALID).param("defId", id).build());
            continue;
        };
        let mut inputs = inputs;
        mark_widget_ports(&mut inputs, nodes, links);
        defs.insert(id.to_string(), DefSource { def: SubgraphDef { inputs, outputs }, nodes, links });
    }
    defs
}

fn parse_sockets(node: &Map<String, Value>, key: &NodeKey, diagnostics: &mut Vec<CompileDiagnostic>) -> Option<Vec<Socket>> {
    let mut sockets = Vec::new();
    let Some(list) = node.get("inputs") else { return Some(sockets) };
    let Some(list) = list.as_array() else {
        diagnostics.push(Diag::blocking(codes::NODE_INVALID).at(key).param("field", "inputs").build());
        return None;
    };
    let mut names = BTreeSet::new();
    for s in list {
        let Some(name) = s.get("name").and_then(Value::as_str) else {
            diagnostics.push(Diag::blocking(codes::NODE_INVALID).at(key).param("field", "inputs.name").build());
            return None;
        };
        if !names.insert(name.to_string()) {
            diagnostics.push(Diag::blocking(codes::NODE_INVALID).at(key).input(name).param("field", "inputs.duplicateName").build());
            return None;
        }
        let link = match s.get("link") {
            None | Some(Value::Null) => None,
            Some(v) => match parse_node_id(v) {
                Some(id) => Some(id),
                None => {
                    diagnostics.push(Diag::blocking(codes::LINK_INVALID).at(key).input(name).param("field", "link").build());
                    return None;
                }
            },
        };
        sockets.push(Socket {
            name: name.to_string(),
            ty: s.get("type").and_then(Value::as_str).map(str::to_string),
            link,
        });
    }
    Some(sockets)
}

fn parse_out_sockets(node: &Map<String, Value>, key: &NodeKey, diagnostics: &mut Vec<CompileDiagnostic>) -> Option<Vec<OutSocket>> {
    let mut sockets = Vec::new();
    let Some(list) = node.get("outputs") else { return Some(sockets) };
    let Some(list) = list.as_array() else {
        diagnostics.push(Diag::blocking(codes::NODE_INVALID).at(key).param("field", "outputs").build());
        return None;
    };
    for s in list {
        let Some(name) = s.get("name").and_then(Value::as_str) else {
            diagnostics.push(Diag::blocking(codes::NODE_INVALID).at(key).param("field", "outputs.name").build());
            return None;
        };
        sockets.push(OutSocket { name: name.to_string(), ty: s.get("type").and_then(Value::as_str).map(str::to_string) });
    }
    Some(sockets)
}

struct Registrar<'a> {
    defs: &'a BTreeMap<String, DefSource<'a>>,
    group_nodes: BTreeSet<String>,
    graph: RegisteredGraph,
    diagnostics: Vec<CompileDiagnostic>,
}

impl<'a> Registrar<'a> {
    fn register_scope(&mut self, scope: Vec<String>, nodes: &[Value], links: &[Value], stack: &mut Vec<String>) {
        let mut s = Scope::default();
        for node in nodes {
            let Some(obj) = node.as_object() else {
                self.diagnostics.push(Diag::blocking(codes::NODE_INVALID).param("scope", scope.join("/")).build());
                continue;
            };
            let Some(local) = obj.get("id").and_then(parse_node_id) else {
                self.diagnostics.push(
                    Diag::blocking(codes::NODE_ID_INVALID)
                        .param("scope", scope.join("/"))
                        .param("id", obj.get("id").map(Value::to_string).unwrap_or_else(|| "missing".into()))
                        .build(),
                );
                continue;
            };
            if s.nodes.contains_key(&local) || local == SUBGRAPH_INPUT_NODE || local == SUBGRAPH_OUTPUT_NODE {
                self.diagnostics.push(
                    Diag::blocking(codes::NODE_ID_COLLISION).param("scope", scope.join("/")).param("id", &local).build(),
                );
                continue;
            }
            let key = NodeKey { scope: scope.clone(), local_id: local.clone() };
            let Some(class_type) = obj.get("type").and_then(Value::as_str).filter(|c| !c.is_empty()) else {
                self.diagnostics.push(Diag::blocking(codes::MISSING_CLASS_TYPE).at(&key).build());
                continue;
            };
            if self.group_nodes.contains(class_type) {
                self.diagnostics.push(Diag::blocking(codes::LEGACY_GROUP_NODE_UNSUPPORTED).at(&key).class(class_type).build());
                continue;
            }
            let Some(mode) = obj.get("mode").and_then(parse_mode) else {
                self.diagnostics.push(
                    Diag::blocking(codes::NODE_MODE_INVALID)
                        .at(&key)
                        .class(class_type)
                        .param("mode", obj.get("mode").map(Value::to_string).unwrap_or_else(|| "missing".into()))
                        .build(),
                );
                continue;
            };
            let Some(inputs) = parse_sockets(obj, &key, &mut self.diagnostics) else { continue };
            let Some(outputs) = parse_out_sockets(obj, &key, &mut self.diagnostics) else { continue };
            let raw = RawNode {
                local_id: local.clone(),
                class_type: class_type.to_string(),
                mode,
                inputs,
                outputs,
                widgets_values: obj.get("widgets_values").cloned(),
                title: obj.get("title").and_then(Value::as_str).map(str::to_string),
                properties: obj.get("properties").cloned(),
            };
            if let Some(def) = self.defs.get(class_type) {
                let widget_sockets = def.def.inputs.iter().filter(|p| p.widget).count();
                let values = match &raw.widgets_values {
                    None => 0,
                    Some(Value::Array(a)) => a.len(),
                    Some(_) => usize::MAX,
                };
                if widget_sockets != values && !promoted_by_proxy(&raw) {
                    self.diagnostics.push(
                        Diag::blocking(codes::PROMOTED_WIDGET_COUNT_MISMATCH)
                            .param("instance", key.exec_id())
                            .param("widgetSockets", widget_sockets)
                            .param("widgetsValues", if values == usize::MAX { "notArray".to_string() } else { values.to_string() })
                            .build(),
                    );
                }
                s.instances.insert(local.clone(), class_type.to_string());
            }
            s.nodes.insert(local, raw);
        }
        for link in links {
            match parse_link(link) {
                Ok(l) => {
                    if s.links.contains_key(&l.id) {
                        self.diagnostics.push(
                            Diag::blocking(codes::LINK_ID_COLLISION).param("scope", scope.join("/")).param("linkId", &l.id).build(),
                        );
                        continue;
                    }
                    s.links.insert(l.id.clone(), l);
                }
                Err(field) => self.diagnostics.push(
                    Diag::blocking(codes::LINK_INVALID).param("scope", scope.join("/")).param("field", field).build(),
                ),
            }
        }
        let instances: Vec<(String, String)> = s.instances.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        self.graph.scopes.insert(scope.clone(), s);
        for (local, def_id) in instances {
            let defs = self.defs;
            let def = &defs[&def_id];
            let mut child = scope.clone();
            child.push(local);
            if stack.contains(&def_id) {
                self.diagnostics.push(
                    Diag::blocking(codes::SUBGRAPH_DEFINITION_CYCLE)
                        .param("defId", &def_id)
                        .param("instance", NodeKey { scope: child[..child.len() - 1].to_vec(), local_id: child[child.len() - 1].clone() }.exec_id())
                        .build(),
                );
                continue;
            }
            if child.len() > MAX_SUBGRAPH_DEPTH {
                self.diagnostics.push(Diag::blocking(codes::SUBGRAPH_DEPTH_EXCEEDED).param("depth", child.len()).build());
                continue;
            }
            stack.push(def_id.clone());
            self.register_scope(child, def.nodes, def.links, stack);
            stack.pop();
        }
    }
}

fn types_compatible(output: &str, input: &str) -> bool {
    output == "*"
        || input == "*"
        || output == input
        || input.split(',').any(|t| t.trim() == output)
        || output.split(',').any(|t| t.trim() == input)
}

pub(crate) fn parse_canvas(value: &Value, format: SourceFormat) -> Result<GraphIrV1, Vec<CompileDiagnostic>> {
    let Some(root) = value.as_object() else {
        return Err(vec![Diag::blocking(codes::TOP_LEVEL_NOT_OBJECT).build()]);
    };
    let (Some(nodes), Some(links)) = (array_field(root, "nodes"), array_field(root, "links")) else {
        return Err(vec![Diag::blocking(codes::FORMAT_UNRECOGNIZED).build()]);
    };
    let mut diagnostics = Vec::new();
    let defs = register_definitions(root, &mut diagnostics);
    let mut group_nodes = BTreeSet::new();
    if let Some(groups) = root.get("extra").and_then(|e| e.get("groupNodes")).and_then(Value::as_object) {
        for name in groups.keys() {
            group_nodes.insert(format!("workflow>{name}"));
            group_nodes.insert(format!("workflow/{name}"));
        }
    }
    let mut registrar = Registrar { defs: &defs, group_nodes, graph: RegisteredGraph::default(), diagnostics };
    for (id, d) in &defs {
        registrar.graph.defs.insert(id.clone(), d.def.clone());
    }
    registrar.register_scope(Vec::new(), nodes, links, &mut Vec::new());
    let Registrar { graph, mut diagnostics, .. } = registrar;

    let mut resolver = Resolver::new(&graph);
    let mut ir_nodes = Vec::new();
    let mut node_attrs = BTreeMap::new();
    for (scope_path, scope) in &graph.scopes {
        for (local, node) in &scope.nodes {
            if is_virtual_class(&node.class_type) || scope.instances.contains_key(local) {
                continue;
            }
            let key = NodeKey { scope: scope_path.clone(), local_id: local.clone() };
            let Some(mode) = resolver.effective_mode(scope_path, local) else { continue };
            let mut inputs = BTreeMap::new();
            let mut overrides = Vec::new();
            if mode == NodeMode::Normal {
                for (idx, socket) in node.inputs.iter().enumerate() {
                    let Some(link_id) = &socket.link else { continue };
                    let link = scope.links.get(link_id);
                    // The socket's link id is authoritative (frontend graphToPrompt follows
                    // `inputs[i].link`); a stale `target_slot` from an older socket layout of a
                    // dynamic node is repaired by the frontend on load, so it only warns.
                    if link.is_some_and(|l| &l.target == local && l.target_slot as usize != idx) {
                        resolver.diagnostics.push(
                            Diag::non_blocking(codes::LINK_INCONSISTENT)
                                .at(&key)
                                .class(&node.class_type)
                                .input(&socket.name)
                                .param("linkId", link_id)
                                .param("reason", "targetSlotStale")
                                .build(),
                        );
                    }
                    let consistent = link.is_some_and(|l| &l.target == local);
                    if !consistent {
                        resolver.diagnostics.push(
                            Diag::blocking(codes::LINK_INCONSISTENT)
                                .at(&key)
                                .class(&node.class_type)
                                .input(&socket.name)
                                .param("linkId", link_id)
                                .build(),
                        );
                        continue;
                    }
                    let mut visited = BTreeSet::new();
                    match resolver.resolve_link(scope_path, link_id, &key, &socket.name, &mut visited) {
                        Resolved::Link(r) => {
                            if let (Some(out), Some(inp)) = (&r.output_type, &socket.ty) {
                                if !types_compatible(out, inp) {
                                    resolver.diagnostics.push(
                                        Diag::blocking(codes::LINK_TYPE_MISMATCH)
                                            .at(&key)
                                            .class(&node.class_type)
                                            .input(&socket.name)
                                            .param("outputType", out)
                                            .param("inputType", inp)
                                            .build(),
                                    );
                                    continue;
                                }
                            }
                            inputs.insert(socket.name.clone(), InputValue::Link(r));
                        }
                        Resolved::Override(value, source) => {
                            overrides.push(WidgetOverride { input_name: socket.name.clone(), value, source })
                        }
                        Resolved::Dropped { code, reason } => resolver.diagnostics.push(
                            Diag::non_blocking(code)
                                .at(&key)
                                .class(&node.class_type)
                                .input(&socket.name)
                                .param("reason", reason)
                                .build(),
                        ),
                        Resolved::InteriorWidget | Resolved::Failed => {}
                    }
                }
            }
            ir_nodes.push(IrNode {
                key: key.clone(),
                class_type: node.class_type.clone(),
                mode,
                inputs,
                widgets_values: node.widgets_values.clone(),
                source_locator: key.locator(),
            });
            node_attrs.insert(
                key.exec_id(),
                NodeAttrsV1 { title: node.title.clone(), properties: node.properties.clone(), overrides },
            );
        }
    }
    diagnostics.append(&mut resolver.diagnostics);
    let outputs = structural_sinks(&ir_nodes);
    Ok(GraphIrV1 { format, nodes: ir_nodes, outputs, diagnostics, node_attrs })
}
