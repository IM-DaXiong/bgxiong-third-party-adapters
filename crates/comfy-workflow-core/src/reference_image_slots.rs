//! Project explicit IMAGE Autogrow slots from the selected ComfyUI output.

use alloc::collections::BTreeSet;
#[cfg(target_arch = "wasm32")]
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::node_registry::definition::{DefinitionSnapshotV1, InputDef};
use crate::{linked_source, MappingError, MappingErrorCode, WorkflowGraph};

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceImageSlots {
    pub consumer_node_id: String,
    pub parent_key: String,
    pub min_count: usize,
    pub max_count: usize,
    pub slots: Vec<ReferenceImageSlot>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceImageSlot {
    pub input_name: String,
    pub existing_link: Option<ReferenceImageLink>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceImageLink {
    pub node_id: String,
    pub output_index: usize,
}

fn invalid(reason: &'static str) -> MappingError {
    MappingError::new(MappingErrorCode::NodeDefinitionIncomplete, reason)
}

fn selected_branch(
    graph: &Map<String, Value>,
    inputs: &Map<String, Value>,
) -> Option<&'static str> {
    if !(inputs.contains_key("on_true") || inputs.contains_key("on_false")) {
        return None;
    }
    let switch = inputs.get("switch")?;
    let value = switch.as_bool().or_else(|| {
        let (id, _) = linked_source(switch)?;
        graph.get(&id)?["inputs"]["value"].as_bool()
    })?;
    Some(if value { "on_true" } else { "on_false" })
}

fn effective_ancestry(graph: &Map<String, Value>, output: &str) -> BTreeSet<String> {
    let mut stack = vec![output.to_string()];
    let mut visited = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(inputs) = graph.get(&id).and_then(|node| node["inputs"].as_object()) else {
            continue;
        };
        let branch = selected_branch(graph, inputs);
        stack.extend(inputs.iter().filter_map(|(key, value)| {
            if matches!(key.as_str(), "on_true" | "on_false")
                && branch.is_some_and(|selected| selected != key)
            {
                return None;
            }
            linked_source(value)
                .map(|(id, _)| id)
                .filter(|id| !visited.contains(id))
        }));
    }
    visited
}

fn image_template(input: &InputDef) -> Option<&Value> {
    if input.hidden || !input.type_name.contains("COMFY_AUTOGROW") {
        return None;
    }
    let template = input.options.get("template")?;
    let children = template.get("input").or_else(|| template.get("inputs"))?;
    let has_image = ["required", "optional"]
        .iter()
        .filter_map(|section| children[*section].as_object())
        .flat_map(|group| group.values())
        .any(|child| {
            child
                .as_str()
                .or_else(|| child[0].as_str())
                .or_else(|| child["type"].as_str())
                == Some("IMAGE")
        });
    has_image.then_some(template)
}

fn declared_limit(template: &Value, name: &str) -> Result<Option<usize>, MappingError> {
    template
        .get(name)
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| invalid("invalid_reference_count_bound"))
        })
        .transpose()
}

fn freeze_group(
    graph: &Map<String, Value>,
    definitions: &DefinitionSnapshotV1,
    consumer: &str,
    parent: &str,
    template: &Value,
) -> Result<ReferenceImageSlots, MappingError> {
    let names = template["names"]
        .as_array()
        .ok_or_else(|| invalid("reference_names_not_array"))?;
    let inputs = graph[consumer]["inputs"]
        .as_object()
        .ok_or_else(|| invalid("reference_consumer_inputs_missing"))?;
    let prefix = format!("{parent}.");
    let mut keys = BTreeSet::new();
    let mut slots = Vec::with_capacity(names.len());
    for name in names {
        let name = name
            .as_str()
            .filter(|name| !name.is_empty() && name.trim() == *name)
            .ok_or_else(|| invalid("reference_name_invalid"))?;
        let input_name = format!("{prefix}{name}");
        if !keys.insert(input_name.clone()) {
            return Err(invalid("duplicate_reference_name"));
        }
        let existing_link = inputs
            .get(&input_name)
            .map(|value| {
                let (node_id, output_index) =
                    linked_source(value).ok_or_else(|| invalid("reference_child_not_linked"))?;
                if !graph.get(&node_id).is_some_and(|node| {
                    node["class_type"] == "LoadImage" && node["inputs"].get("image").is_some()
                }) || definitions
                    .classes
                    .get("LoadImage")
                    .and_then(|definition| definition.outputs.get(output_index))
                    .is_none_or(|kind| kind != "IMAGE")
                {
                    return Err(invalid("reference_child_not_load_image_output"));
                }
                Ok(ReferenceImageLink {
                    node_id,
                    output_index,
                })
            })
            .transpose()?;
        slots.push(ReferenceImageSlot {
            input_name,
            existing_link,
        });
    }
    if inputs
        .keys()
        .any(|key| key.starts_with(&prefix) && !keys.contains(key))
    {
        return Err(invalid("undeclared_reference_child"));
    }
    let wired = slots.iter().any(|slot| slot.existing_link.is_some());
    let min_count = declared_limit(template, "min")?
        .unwrap_or(0)
        .max(usize::from(wired));
    let max_count = declared_limit(template, "max")?.unwrap_or(slots.len());
    if max_count == 0 || max_count > slots.len() || min_count > max_count {
        return Err(invalid("invalid_reference_count_range"));
    }
    Ok(ReferenceImageSlots {
        consumer_node_id: consumer.into(),
        parent_key: parent.into(),
        min_count,
        max_count,
        slots,
    })
}

/// Reuse the host reference-name-group rule over normalized definitions. The returned slots
/// preserve `template.names` declaration order and the original graph's existing links.
pub fn project_reference_image_slots(
    workflow: &Value,
    definitions: &DefinitionSnapshotV1,
    output: &str,
) -> Result<Option<ReferenceImageSlots>, MappingError> {
    WorkflowGraph::from_api(workflow)?.reverse_ancestry(output)?;
    let graph = workflow.as_object().expect("validated API object");
    let ancestry = effective_ancestry(graph, output);
    let groups: Vec<_> = ancestry
        .iter()
        .filter_map(|id| {
            let class = graph[id]["class_type"].as_str()?;
            definitions
                .classes
                .get(class)
                .map(|definition| (id, definition))
        })
        .flat_map(|(id, definition)| {
            definition
                .inputs
                .iter()
                .filter_map(move |input| image_template(input).map(|t| (id, input, t)))
        })
        .collect();

    if groups.iter().any(|(_, _, template)| {
        let has_prefix = template["prefix"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty());
        template.get("names").is_some() == has_prefix
    }) {
        return Err(invalid("invalid_reference_key_declaration"));
    }
    let Some((consumer, input, template)) = groups
        .iter()
        .find(|(_, _, template)| template.get("names").is_some())
    else {
        return Ok(None);
    };
    if groups.len() != 1 {
        return Err(MappingError::new(
            MappingErrorCode::BindingAmbiguous,
            "multiple_reference_image_groups",
        ));
    }
    freeze_group(graph, definitions, consumer, &input.name, template).map(Some)
}
