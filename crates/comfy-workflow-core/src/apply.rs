#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};
use alloc::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::contract::{
    CONTRACT_VERSION, MAX_BINDINGS, MAX_DIAGNOSTICS, MAX_NODES, MAX_TARGETS_PER_BINDING,
    MAX_WORKFLOW_JSON_BYTES,
};
use crate::digest::digest_json;
use crate::error::{MappingError, MappingErrorCode};
use crate::identity::validate_identity;
use crate::types::*;

fn validate_value(binding: &ParameterBindingV1, value: &Value) -> Result<(), MappingError> {
    let type_ok = match binding.value_type {
        BindingValueType::String | BindingValueType::MediaRef => value.is_string(),
        BindingValueType::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
        BindingValueType::Number => value.is_number(),
        BindingValueType::Boolean => value.is_boolean(),
    };
    if !type_ok {
        return Err(MappingError::new(
            MappingErrorCode::TypeMismatch,
            format!("binding {} value type mismatch", binding.binding_id),
        ));
    }
    if let Some(range) = &binding.range {
        let number = value.as_f64().ok_or_else(|| {
            MappingError::new(MappingErrorCode::TypeMismatch, "range requires a number")
        })?;
        if number < range.min || number > range.max {
            return Err(MappingError::new(
                MappingErrorCode::ValueOutOfRange,
                format!(
                    "binding {} is outside {}..={}",
                    binding.binding_id, range.min, range.max
                ),
            ));
        }
    }
    if !binding.enum_values.is_empty() && !binding.enum_values.contains(value) {
        return Err(MappingError::new(
            MappingErrorCode::ValueOutOfRange,
            format!("binding {} is outside its enum", binding.binding_id),
        ));
    }
    Ok(())
}

fn validate_contract(contract: &MappingContractV1, workflow: &Value) -> Result<(), MappingError> {
    validate_identity(&contract.snapshot.identity)?;
    if contract.schema_version != 1 || contract.contract_version != CONTRACT_VERSION {
        return Err(MappingError::new(
            MappingErrorCode::ContractStale,
            "mapping contract version is not current",
        ));
    }
    if contract.status != ContractStatus::Ready {
        return Err(MappingError::new(
            MappingErrorCode::ContractStale,
            "mapping contract is not ready",
        ));
    }
    if contract.bindings.len() > MAX_BINDINGS || contract.diagnostics.len() > MAX_DIAGNOSTICS {
        return Err(MappingError::new(
            MappingErrorCode::ResourceLimitExceeded,
            "mapping contract exceeds its budget",
        ));
    }
    let bytes = serde_json::to_vec(workflow)
        .map_err(|e| MappingError::new(MappingErrorCode::GraphInvalid, e.to_string()))?;
    let graph = workflow.as_object().ok_or_else(|| {
        MappingError::new(
            MappingErrorCode::GraphInvalid,
            "workflow must be an API object",
        )
    })?;
    if bytes.len() > MAX_WORKFLOW_JSON_BYTES || graph.len() > MAX_NODES {
        return Err(MappingError::new(
            MappingErrorCode::ResourceLimitExceeded,
            "workflow exceeds its budget",
        ));
    }
    if !graph.contains_key(&contract.output_selection.node_id) {
        return Err(MappingError::new(
            MappingErrorCode::TargetMissing,
            "selected output node is absent",
        ));
    }
    if digest_json(workflow)? != contract.snapshot.graph_digest {
        return Err(MappingError::new(
            MappingErrorCode::ContractStale,
            "workflow graph digest changed",
        ));
    }
    Ok(())
}

pub fn apply_mapping_contract(request: ApplyRequestV1) -> Result<ApplyResultV1, MappingError> {
    validate_contract(&request.contract, &request.workflow)?;
    let bindings: BTreeMap<&str, &ParameterBindingV1> = request
        .contract
        .bindings
        .iter()
        .map(|b| (b.binding_id.as_str(), b))
        .collect();
    if bindings.len() != request.contract.bindings.len() {
        return Err(MappingError::new(
            MappingErrorCode::BindingAmbiguous,
            "duplicate bindingId",
        ));
    }
    let mut supplied = BTreeSet::new();
    let mut writes: BTreeMap<(String, String), (String, Value)> = BTreeMap::new();
    for item in &request.values {
        if !supplied.insert(item.binding_id.as_str()) {
            return Err(MappingError::new(
                MappingErrorCode::BindingAmbiguous,
                format!("duplicate value for {}", item.binding_id),
            ));
        }
        let binding = bindings.get(item.binding_id.as_str()).ok_or_else(|| {
            MappingError::new(
                MappingErrorCode::BindingAmbiguous,
                format!("unknown bindingId {}", item.binding_id),
            )
        })?;
        if !binding.asset_slots.is_empty() {
            return Err(MappingError::new(
                MappingErrorCode::TypeMismatch,
                format!(
                    "asset binding {} is structural; it is applied by the platform asset adapter",
                    binding.binding_id
                ),
            ));
        }
        validate_value(binding, &item.value)?;
        if binding.targets.is_empty() || binding.targets.len() > MAX_TARGETS_PER_BINDING {
            return Err(MappingError::new(
                MappingErrorCode::ResourceLimitExceeded,
                format!("binding {} has invalid target count", binding.binding_id),
            ));
        }
        for target in &binding.targets {
            if target.transform != TransformKind::Identity {
                return Err(MappingError::new(
                    MappingErrorCode::NodeDefinitionIncomplete,
                    format!("transform {:?} is not implemented", target.transform),
                ));
            }
            let key = (target.node_id.clone(), target.input_name.clone());
            if let Some((other_binding, other_value)) = writes.get(&key) {
                if other_value != &item.value {
                    return Err(MappingError::new(
                        MappingErrorCode::WriteConflict,
                        format!(
                            "{} and {} write different values to {}.{}",
                            other_binding, binding.binding_id, target.node_id, target.input_name
                        ),
                    ));
                }
            } else {
                writes.insert(key, (binding.binding_id.clone(), item.value.clone()));
            }
        }
    }

    for fixed in &request.contract.fixed_writes {
        let key = (fixed.node_id.clone(), fixed.input_name.clone());
        if let Some((other_binding, other_value)) = writes.get(&key) {
            if other_value != &fixed.value {
                return Err(MappingError::new(
                    MappingErrorCode::WriteConflict,
                    format!(
                        "{} conflicts with fixed write on {}.{}",
                        other_binding, fixed.node_id, fixed.input_name
                    ),
                ));
            }
        } else {
            writes.insert(key, ("fixedWrite".into(), fixed.value.clone()));
        }
    }

    let mut workflow = request.workflow;
    let graph = workflow.as_object_mut().expect("validated object");
    for ((node_id, input_name), (_, value)) in &writes {
        let node = graph
            .get_mut(node_id)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                MappingError::new(
                    MappingErrorCode::TargetMissing,
                    format!("node {node_id} is absent"),
                )
            })?;
        let inputs = node
            .get_mut("inputs")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                MappingError::new(
                    MappingErrorCode::TargetMissing,
                    format!("node {node_id} has no inputs object"),
                )
            })?;
        if !inputs.contains_key(input_name) {
            return Err(MappingError::new(
                MappingErrorCode::TargetMissing,
                format!("target {node_id}.{input_name} is absent"),
            ));
        }
        inputs.insert(input_name.clone(), value.clone());
    }
    for fixed in &request.contract.fixed_writes {
        if workflow[&fixed.node_id]["inputs"][&fixed.input_name] != fixed.value {
            return Err(MappingError::new(
                MappingErrorCode::WriteConflict,
                format!("fixed write {}.{} did not read back", fixed.node_id, fixed.input_name),
            ));
        }
    }

    let mut applied_bindings = Vec::new();
    for item in request.values {
        let binding = bindings[item.binding_id.as_str()];
        let mut targets = Vec::new();
        for target in &binding.targets {
            let actual_value = workflow[&target.node_id]["inputs"][&target.input_name].clone();
            targets.push(AppliedTargetV1 {
                node_id: target.node_id.clone(),
                input_name: target.input_name.clone(),
                actual_value,
            });
        }
        applied_bindings.push(AppliedBindingV1 {
            binding_id: item.binding_id,
            semantic_role: binding.semantic_role,
            targets,
        });
    }
    Ok(ApplyResultV1 {
        workflow,
        applied_bindings,
    })
}

/// One frozen advanced-parameter write target (descriptor + compiled exec node).
#[derive(Debug, Clone)]
pub struct AdvancedWriteTargetV1<'a> {
    pub descriptor: &'a AdvancedParamDescriptor,
    pub exec_id: &'a str,
}

fn validate_advanced_value(d: &AdvancedParamDescriptor, value: &Value) -> Result<(), MappingError> {
    let type_ok = match d.value_type {
        ParamValueType::Int => value.as_i64().is_some() || value.as_u64().is_some(),
        ParamValueType::Float => value.is_number(),
        ParamValueType::Boolean => value.is_boolean(),
        ParamValueType::String => value.is_string(),
        ParamValueType::Combo => true,
    };
    if !type_ok {
        return Err(MappingError::new(
            MappingErrorCode::TypeMismatch,
            format!("advanced {} value type mismatch", d.binding_id),
        ));
    }
    if let Some(n) = value.as_f64() {
        if d.min.is_some_and(|min| n < min) || d.max.is_some_and(|max| n > max) {
            return Err(MappingError::new(
                MappingErrorCode::ValueOutOfRange,
                format!("advanced {} is outside its range", d.binding_id),
            ));
        }
    }
    if let Some(options) = &d.options {
        if !options.contains(value) {
            return Err(MappingError::new(
                MappingErrorCode::ValueOutOfRange,
                format!("advanced {} is outside its options", d.binding_id),
            ));
        }
    }
    Ok(())
}

/// Apply user-changed advanced values onto an already contract-applied workflow (CW53/CW59).
///
/// Every value must name a frozen descriptor; targets already written by the contract
/// (`occupied`) are a write conflict, never overwritten. Validation happens for all values before
/// any write (zero write on error); each write is read back.
pub fn apply_advanced_values(
    workflow: &mut Value,
    targets: &[AdvancedWriteTargetV1<'_>],
    values: &[BindingValueV1],
    occupied: &BTreeSet<(String, String)>,
) -> Result<Vec<AppliedTargetV1>, MappingError> {
    let by_id: BTreeMap<&str, &AdvancedWriteTargetV1<'_>> = targets
        .iter()
        .map(|t| (t.descriptor.binding_id.as_str(), t))
        .collect();
    let mut planned = Vec::new();
    let mut seen = BTreeSet::new();
    for item in values {
        if !seen.insert(item.binding_id.as_str()) {
            return Err(MappingError::new(
                MappingErrorCode::BindingAmbiguous,
                format!("duplicate value for {}", item.binding_id),
            ));
        }
        let target = by_id.get(item.binding_id.as_str()).ok_or_else(|| {
            MappingError::new(
                MappingErrorCode::BindingAmbiguous,
                format!("unknown advanced bindingId {}", item.binding_id),
            )
        })?;
        validate_advanced_value(target.descriptor, &item.value)?;
        let key = (
            target.exec_id.to_string(),
            target.descriptor.input_name.clone(),
        );
        if occupied.contains(&key) {
            return Err(MappingError::new(
                MappingErrorCode::WriteConflict,
                format!("advanced {} targets a contract-owned input", item.binding_id),
            ));
        }
        let present = workflow
            .get(target.exec_id)
            .and_then(|n| n.get("inputs"))
            .and_then(|i| i.get(&target.descriptor.input_name))
            .is_some();
        if !present {
            return Err(MappingError::new(
                MappingErrorCode::TargetMissing,
                format!("target {}.{} is absent", key.0, key.1),
            ));
        }
        planned.push((key, item.value.clone()));
    }
    let mut applied = Vec::new();
    for ((node_id, input_name), value) in planned {
        workflow[&node_id]["inputs"][&input_name] = value.clone();
        let actual_value = workflow[&node_id]["inputs"][&input_name].clone();
        if actual_value != value {
            return Err(MappingError::new(
                MappingErrorCode::WriteConflict,
                format!("advanced write {node_id}.{input_name} did not read back"),
            ));
        }
        applied.push(AppliedTargetV1 {
            node_id,
            input_name,
            actual_value,
        });
    }
    Ok(applied)
}

/// Read-back proof over the final graph (CW59): every input outside the declared write set and
/// outside structurally rewritten nodes must be byte-identical to the frozen graph, and no node may
/// disappear or appear outside `structural_nodes`.
pub fn verify_write_set(
    frozen: &Value,
    prepared: &Value,
    allowed: &BTreeSet<(String, String)>,
    structural_nodes: &BTreeSet<String>,
) -> Result<(), MappingError> {
    let before = frozen.as_object().ok_or_else(|| {
        MappingError::new(MappingErrorCode::GraphInvalid, "frozen workflow must be an object")
    })?;
    let after = prepared.as_object().ok_or_else(|| {
        MappingError::new(MappingErrorCode::GraphInvalid, "prepared workflow must be an object")
    })?;
    for (id, node) in before {
        if structural_nodes.contains(id) {
            continue;
        }
        let Some(next) = after.get(id) else {
            return Err(MappingError::new(
                MappingErrorCode::WriteConflict,
                format!("node {id} vanished outside the write set"),
            ));
        };
        if node.get("class_type") != next.get("class_type") {
            return Err(MappingError::new(
                MappingErrorCode::WriteConflict,
                format!("node {id} class changed"),
            ));
        }
        let empty = serde_json::Map::new();
        let old_inputs = node.get("inputs").and_then(Value::as_object).unwrap_or(&empty);
        let new_inputs = next.get("inputs").and_then(Value::as_object).unwrap_or(&empty);
        let names: BTreeSet<&String> = old_inputs.keys().chain(new_inputs.keys()).collect();
        for name in names {
            if allowed.contains(&(id.clone(), name.clone())) {
                continue;
            }
            if old_inputs.get(name) != new_inputs.get(name) {
                return Err(MappingError::new(
                    MappingErrorCode::WriteConflict,
                    format!("{id}.{name} changed outside the write set"),
                ));
            }
        }
    }
    for id in after.keys() {
        if !before.contains_key(id) && !structural_nodes.contains(id) {
            return Err(MappingError::new(
                MappingErrorCode::WriteConflict,
                format!("node {id} appeared outside the write set"),
            ));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn advanced(id: &str, input: &str, vt: ParamValueType) -> AdvancedParamDescriptor {
        AdvancedParamDescriptor {
            binding_id: id.into(),
            label: input.into(),
            value_type: vt,
            value: json!(20),
            min: Some(1.0),
            max: Some(100.0),
            step: Some(1.0),
            options: None,
            unit: None,
            source: NodeLocator {
                scope_path: vec![],
                node_id: "3".into(),
            },
            input_name: input.into(),
        }
    }

    #[test]
    fn advanced_values_write_validate_and_read_back() {
        let mut wf = json!({"3":{"class_type":"KSampler","inputs":{"steps":20,"seed":1}}});
        let d = advanced("adv-steps", "steps", ParamValueType::Int);
        let targets = [AdvancedWriteTargetV1 { descriptor: &d, exec_id: "3" }];
        let occupied = BTreeSet::from([("3".to_string(), "seed".to_string())]);
        let out = apply_advanced_values(
            &mut wf,
            &targets,
            &[BindingValueV1 { binding_id: "adv-steps".into(), value: json!(30) }],
            &occupied,
        )
        .unwrap();
        assert_eq!(out[0].actual_value, json!(30));
        assert_eq!(wf["3"]["inputs"]["steps"], json!(30));
        assert_eq!(wf["3"]["inputs"]["seed"], json!(1));
        let err = apply_advanced_values(
            &mut wf,
            &targets,
            &[BindingValueV1 { binding_id: "adv-steps".into(), value: json!(500) }],
            &occupied,
        )
        .unwrap_err();
        assert_eq!(err.code, MappingErrorCode::ValueOutOfRange);
        assert_eq!(wf["3"]["inputs"]["steps"], json!(30), "zero write on rejection");
    }

    #[test]
    fn advanced_value_on_contract_owned_input_conflicts() {
        let mut wf = json!({"3":{"class_type":"KSampler","inputs":{"seed":1}}});
        let d = advanced("adv-seed", "seed", ParamValueType::Int);
        let targets = [AdvancedWriteTargetV1 { descriptor: &d, exec_id: "3" }];
        let occupied = BTreeSet::from([("3".to_string(), "seed".to_string())]);
        let err = apply_advanced_values(
            &mut wf,
            &targets,
            &[BindingValueV1 { binding_id: "adv-seed".into(), value: json!(2) }],
            &occupied,
        )
        .unwrap_err();
        assert_eq!(err.code, MappingErrorCode::WriteConflict);
    }

    #[test]
    fn write_set_proof_rejects_untracked_changes() {
        let frozen = json!({"1":{"class_type":"A","inputs":{"x":1,"y":2}},"2":{"class_type":"B","inputs":{}}});
        let mut prepared = frozen.clone();
        prepared["1"]["inputs"]["x"] = json!(5);
        let allowed = BTreeSet::from([("1".to_string(), "x".to_string())]);
        verify_write_set(&frozen, &prepared, &allowed, &BTreeSet::new()).unwrap();
        prepared["1"]["inputs"]["y"] = json!(3);
        assert_eq!(
            verify_write_set(&frozen, &prepared, &allowed, &BTreeSet::new()).unwrap_err().code,
            MappingErrorCode::WriteConflict
        );
        let mut grown = frozen.clone();
        grown["9"] = json!({"class_type":"C","inputs":{}});
        assert!(verify_write_set(&frozen, &grown, &allowed, &BTreeSet::new()).is_err());
        assert!(verify_write_set(&frozen, &grown, &allowed, &BTreeSet::from(["9".to_string()])).is_ok());
    }
    fn contract(bindings: Vec<ParameterBindingV1>, workflow: &Value) -> MappingContractV1 {
        MappingContractV1 {
            schema_version: 1,
            contract_version: CONTRACT_VERSION,
            snapshot: WorkflowSnapshotV1 {
                identity: WorkflowIdentityV1 {
                    schema_version: 1,
                    instance_id: "local-1".into(),
                    user_scope: "default".into(),
                    source_kind: SourceKind::ClientLibrary,
                    source_path: "workflows/demo.json".into(),
                    display_name: "Demo".into(),
                },
                source_digest: "sha256:a".into(),
                graph_digest: digest_json(workflow).unwrap(),
                definition_digest: "sha256:c".into(),
                rule_registry_version: 1,
                captured_at_ms: 1,
            },
            output_selection: OutputSelectionV1 {
                node_id: "9".into(),
                media_kind: MediaKind::Image,
            },
            bindings,
            diagnostics: vec![],
            status: ContractStatus::Ready,
            fixed_writes: vec![],
            timing: None,
            resolution: None,
        prompt_projection: None,
        }
    }

    fn binding(id: &str, node: &str, input: &str, role: SemanticRole) -> ParameterBindingV1 {
        ParameterBindingV1 {
            binding_id: id.into(),
            semantic_role: role,
            value_type: BindingValueType::String,
            targets: vec![BindingTargetV1 {
                node_id: node.into(),
                input_name: input.into(),
                transform: TransformKind::Identity,
                dependency_binding_ids: vec![],
            }],
            default_value: None,
            range: None,
            enum_values: vec![],
            confirmation: ConfirmationKind::Automatic,
            asset_slots: vec![],
        }
    }

    #[test]
    fn fixed_writes_apply_without_values_and_conflicts_fail() {
        let workflow = json!({"4":{"class_type":"PrimitiveBoolean","inputs":{"value":true}},"5":{"class_type":"CLIPTextEncode","inputs":{"text":"a"}},"9":{"class_type":"SaveImage","inputs":{"images":["5",0]}}});
        let mut c = contract(vec![binding("prompt.positive", "5", "text", SemanticRole::PositivePrompt)], &workflow);
        c.fixed_writes = vec![FixedWriteV1 {
            node_id: "4".into(),
            input_name: "value".into(),
            value: json!(false),
        }];
        let result = apply_mapping_contract(ApplyRequestV1 {
            contract: c.clone(),
            workflow: workflow.clone(),
            values: vec![],
        })
        .unwrap();
        assert_eq!(result.workflow["4"]["inputs"]["value"], json!(false));

        c.fixed_writes[0].node_id = "5".into();
        c.fixed_writes[0].input_name = "text".into();
        let err = apply_mapping_contract(ApplyRequestV1 {
            contract: c,
            workflow,
            values: vec![BindingValueV1 {
                binding_id: "prompt.positive".into(),
                value: json!("user"),
            }],
        })
        .unwrap_err();
        assert_eq!(err.code, MappingErrorCode::WriteConflict);
    }

    #[test]
    fn structural_asset_bindings_are_not_value_written() {
        let workflow = json!({"4":{"class_type":"LoadImage","inputs":{"image":"a.png"}},"9":{"class_type":"SaveImage","inputs":{"images":["4",0]}}});
        let mut asset = binding("media.referenceImage", "4", "image", SemanticRole::ReferenceImage);
        asset.value_type = BindingValueType::MediaRef;
        asset.asset_slots = vec![AssetSlotV1 {
            family: AssetSlotFamily::Flat,
            consumer_node_id: None,
            input_key: None,
            parent_key: None,
            key_prefix: None,
            key_names: vec![],
            min_count: 0,
            max_count: 1,
            required: false,
        }];
        let request = ApplyRequestV1 {
            contract: contract(vec![asset], &workflow),
            workflow,
            values: vec![BindingValueV1 {
                binding_id: "media.referenceImage".into(),
                value: json!("b.png"),
            }],
        };
        assert_eq!(
            apply_mapping_contract(request).unwrap_err().code,
            MappingErrorCode::TypeMismatch
        );
    }

    #[test]
    fn apply_uses_declared_target_and_returns_readback_proof() {
        let workflow = json!({"4":{"class_type":"CLIPTextEncode","inputs":{"text":"old"}},"9":{"class_type":"SaveImage","inputs":{"images":["4",0]}}});
        let request = ApplyRequestV1 {
            contract: contract(
                vec![binding(
                    "prompt.main",
                    "4",
                    "text",
                    SemanticRole::PositivePrompt,
                )],
                &workflow,
            ),
            workflow,
            values: vec![BindingValueV1 {
                binding_id: "prompt.main".into(),
                value: json!("new"),
            }],
        };
        let out = apply_mapping_contract(request).unwrap();
        assert_eq!(out.workflow["4"]["inputs"]["text"], "new");
        assert_eq!(out.applied_bindings[0].targets[0].actual_value, "new");
    }

    #[test]
    fn conflicting_bindings_fail_before_any_result() {
        let workflow = json!({"4":{"class_type":"CLIPTextEncode","inputs":{"text":"old"}},"9":{"class_type":"SaveImage","inputs":{}}});
        let request = ApplyRequestV1 {
            contract: contract(
                vec![
                    binding("a", "4", "text", SemanticRole::PositivePrompt),
                    binding("b", "4", "text", SemanticRole::NegativePrompt),
                ],
                &workflow,
            ),
            workflow,
            values: vec![
                BindingValueV1 {
                    binding_id: "a".into(),
                    value: json!("one"),
                },
                BindingValueV1 {
                    binding_id: "b".into(),
                    value: json!("two"),
                },
            ],
        };
        assert_eq!(
            apply_mapping_contract(request).unwrap_err().code,
            MappingErrorCode::WriteConflict
        );
    }

    #[test]
    fn missing_target_does_not_get_created() {
        let workflow = json!({"4":{"class_type":"CLIPTextEncode","inputs":{"text":"old"}},"9":{"class_type":"SaveImage","inputs":{}}});
        let request = ApplyRequestV1 {
            contract: contract(
                vec![binding("a", "4", "missing", SemanticRole::PositivePrompt)],
                &workflow,
            ),
            workflow,
            values: vec![BindingValueV1 {
                binding_id: "a".into(),
                value: json!("one"),
            }],
        };
        assert_eq!(
            apply_mapping_contract(request).unwrap_err().code,
            MappingErrorCode::TargetMissing
        );
    }
}
