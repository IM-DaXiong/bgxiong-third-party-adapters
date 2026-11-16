#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Map, Value};

use crate::contract::{MAX_EDGES, MAX_NODES, MAX_TRAVERSAL_DEPTH, MAX_WORKFLOW_JSON_BYTES};
use crate::error::{MappingError, MappingErrorCode};

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowNode {
    pub id: String,
    pub class_type: String,
    pub inputs: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowGraph {
    nodes: BTreeMap<String, WorkflowNode>,
    upstream: BTreeMap<String, BTreeSet<String>>,
    edge_count: usize,
}

pub fn linked_source(value: &Value) -> Option<(String, usize)> {
    let link = value.as_array()?;
    if link.len() != 2 || !link.get(1).is_some_and(Value::is_number) {
        return None;
    }
    let id = match link.first()? {
        Value::String(id) if !id.is_empty() => id.clone(),
        Value::Number(id) => id.to_string(),
        _ => return None,
    };
    let slot = link.get(1)?.as_u64()? as usize;
    Some((id, slot))
}

impl WorkflowGraph {
    pub fn from_api(workflow: &Value) -> Result<Self, MappingError> {
        let encoded = serde_json::to_vec(workflow).map_err(|e| {
            MappingError::new(
                MappingErrorCode::GraphInvalid,
                format!("serialize graph: {e}"),
            )
        })?;
        if encoded.len() > MAX_WORKFLOW_JSON_BYTES {
            return Err(MappingError::new(
                MappingErrorCode::ResourceLimitExceeded,
                "workflow JSON exceeds maxWorkflowJsonBytes",
            ));
        }
        let raw_nodes = workflow.as_object().ok_or_else(|| {
            MappingError::new(
                MappingErrorCode::GraphInvalid,
                "API workflow must be an object",
            )
        })?;
        if raw_nodes.len() > MAX_NODES {
            return Err(MappingError::new(
                MappingErrorCode::ResourceLimitExceeded,
                "workflow exceeds maxNodes",
            ));
        }

        let mut nodes = BTreeMap::new();
        for (id, raw_node) in raw_nodes {
            if id.is_empty() || id.chars().any(char::is_control) {
                return Err(MappingError::new(
                    MappingErrorCode::GraphInvalid,
                    "node id is invalid",
                ));
            }
            let node = raw_node.as_object().ok_or_else(|| {
                MappingError::new(
                    MappingErrorCode::GraphInvalid,
                    format!("node {id} must be an object"),
                )
            })?;
            let class_type = node
                .get("class_type")
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    MappingError::new(
                        MappingErrorCode::GraphInvalid,
                        format!("node {id} has no class_type"),
                    )
                })?;
            let inputs = node
                .get("inputs")
                .and_then(Value::as_object)
                .cloned()
                .ok_or_else(|| {
                    MappingError::new(
                        MappingErrorCode::GraphInvalid,
                        format!("node {id} has no inputs object"),
                    )
                })?;
            nodes.insert(
                id.clone(),
                WorkflowNode {
                    id: id.clone(),
                    class_type: class_type.to_string(),
                    inputs,
                },
            );
        }

        let mut upstream: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut edge_count = 0usize;
        for (id, node) in &nodes {
            let parents = upstream.entry(id.clone()).or_default();
            for value in node.inputs.values() {
                let Some((source, _)) = linked_source(value) else {
                    continue;
                };
                if !nodes.contains_key(&source) {
                    return Err(MappingError::new(
                        MappingErrorCode::TargetMissing,
                        format!("node {id} links missing source {source}"),
                    ));
                }
                if parents.insert(source) {
                    edge_count += 1;
                    if edge_count > MAX_EDGES {
                        return Err(MappingError::new(
                            MappingErrorCode::ResourceLimitExceeded,
                            "workflow exceeds maxEdges",
                        ));
                    }
                }
            }
        }
        Ok(Self {
            nodes,
            upstream,
            edge_count,
        })
    }

    pub fn node(&self, id: &str) -> Option<&WorkflowNode> {
        self.nodes.get(id)
    }
    pub fn nodes(&self) -> impl Iterator<Item = &WorkflowNode> {
        self.nodes.values()
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn edge_count(&self) -> usize {
        self.edge_count
    }

    pub fn reverse_ancestry(&self, terminal_id: &str) -> Result<BTreeSet<String>, MappingError> {
        if !self.nodes.contains_key(terminal_id) {
            return Err(MappingError::new(
                MappingErrorCode::TargetMissing,
                format!("terminal {terminal_id} is absent"),
            ));
        }
        let mut visited = BTreeSet::new();
        let mut queued = BTreeSet::new();
        let mut queue = VecDeque::from([(terminal_id.to_string(), 0usize)]);
        queued.insert(terminal_id.to_string());
        while let Some((id, depth)) = queue.pop_front() {
            if depth > MAX_TRAVERSAL_DEPTH {
                return Err(MappingError::new(
                    MappingErrorCode::ResourceLimitExceeded,
                    "graph traversal exceeds maxTraversalDepth",
                ));
            }
            visited.insert(id.clone());
            for source in self.upstream.get(&id).into_iter().flatten() {
                if queued.insert(source.clone()) {
                    queue.push_back((source.clone(), depth + 1));
                }
            }
        }
        Ok(visited)
    }

    pub fn unique_node_by_class<'a>(
        &'a self,
        class_types: &[&str],
    ) -> Result<&'a WorkflowNode, MappingError> {
        let found: Vec<&WorkflowNode> = self
            .nodes
            .values()
            .filter(|node| class_types.contains(&node.class_type.as_str()))
            .collect();
        match found.as_slice() {
            [only] => Ok(*only),
            [] => Err(MappingError::new(
                MappingErrorCode::TargetMissing,
                "no matching node",
            )),
            _ => Err(MappingError::new(
                MappingErrorCode::OutputAmbiguous,
                "multiple matching nodes",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ancestry_is_stable_when_node_ids_and_object_order_change() {
        let a = WorkflowGraph::from_api(&json!({
            "1":{"class_type":"A","inputs":{}},
            "2":{"class_type":"B","inputs":{"x":["1",0]}},
            "9":{"class_type":"SaveImage","inputs":{"images":["2",0]}}
        }))
        .unwrap();
        let b = WorkflowGraph::from_api(&json!({
            "z":{"class_type":"SaveImage","inputs":{"images":["y",0]}},
            "x":{"class_type":"A","inputs":{}},
            "y":{"class_type":"B","inputs":{"x":["x",0]}}
        }))
        .unwrap();
        let classes = |g: &WorkflowGraph, terminal: &str| {
            g.reverse_ancestry(terminal)
                .unwrap()
                .into_iter()
                .map(|id| g.node(&id).unwrap().class_type.clone())
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(classes(&a, "9"), classes(&b, "z"));
    }

    #[test]
    fn dangling_link_fails_instead_of_becoming_a_literal() {
        let err =
            WorkflowGraph::from_api(&json!({"2":{"class_type":"B","inputs":{"x":["missing",0]}}}))
                .unwrap_err();
        assert_eq!(err.code, MappingErrorCode::TargetMissing);
    }

    #[test]
    fn cyclic_graph_traversal_is_bounded_by_visited_set() {
        let graph = WorkflowGraph::from_api(&json!({
            "1":{"class_type":"A","inputs":{"x":["2",0]}},
            "2":{"class_type":"B","inputs":{"x":["1",0]}}
        }))
        .unwrap();
        assert_eq!(
            graph.reverse_ancestry("1").unwrap(),
            BTreeSet::from(["1".into(), "2".into()])
        );
    }
}
