#[cfg(target_arch = "wasm32")]
use alloc::{string::String, vec::Vec};
use alloc::collections::BTreeSet;

use serde_json::Value;

use crate::contract::CONTRACT_VERSION;
use crate::digest::digest_json;
use crate::error::{MappingError, MappingErrorCode};
use crate::graph::WorkflowGraph;
use crate::node_registry::{semantics_for, NodeRole, RULE_REGISTRY_VERSION};
use crate::types::*;

pub fn discover_output_candidates(
    workflow: &Value,
) -> Result<Vec<OutputSelectionV1>, MappingError> {
    let graph = WorkflowGraph::from_api(workflow)?;
    let mut out = Vec::new();
    for node in graph.nodes() {
        let media_kind = match semantics_for(&node.class_type).role {
            NodeRole::ImageOutput => Some(MediaKind::Image),
            NodeRole::VideoOutput => Some(MediaKind::Video),
            _ => None,
        };
        if let Some(media_kind) = media_kind {
            out.push(OutputSelectionV1 {
                node_id: node.id.clone(),
                media_kind,
            });
        }
    }
    Ok(out)
}

/// Build the mapping contract for one output.
///
/// Prompt / seed / image-size value bindings, fixed graph writes, asset slots, timing and
/// resolution come from the platform rule modules (injector generation plan / asset_adapt /
/// duration_slots / resolution_slots) as a [`RuleProjectionV1`] (PLAN decisions D1/D2); this
/// core validates them against the frozen graph and freezes them into the contract. It never
/// re-derives them with its own heuristics. Only the unambiguous sampling scalars
/// (steps / cfg / denoise) are derived here from the selected output's ancestry.
pub fn analyze_mapping_contract(
    workflow: &Value,
    mut snapshot: WorkflowSnapshotV1,
    output_selection: OutputSelectionV1,
    projection: RuleProjectionV1,
) -> Result<MappingContractV1, MappingError> {
    crate::identity::validate_identity(&snapshot.identity)?;
    let graph = WorkflowGraph::from_api(workflow)?;
    let ancestry = graph.reverse_ancestry(&output_selection.node_id)?;
    let mut bindings = Vec::new();
    let mut diagnostics = Vec::new();
    analyze_scalar_bindings(&graph, &ancestry, &mut bindings, &mut diagnostics);
    validate_rule_projection(&graph, &bindings, &projection)?;
    let RuleProjectionV1 {
        value_bindings,
        asset_bindings,
        fixed_writes,
        timing,
        resolution,
        prompt_projection,
    } = projection;
    bindings.splice(0..0, value_bindings);
    bindings.extend(asset_bindings);
    snapshot.graph_digest = digest_json(workflow)?;
    snapshot.rule_registry_version = RULE_REGISTRY_VERSION;
    let status = if diagnostics
        .iter()
        .any(|d| d.severity == MappingDiagnosticSeverity::Blocking)
    {
        ContractStatus::NeedsConfirmation
    } else if bindings.is_empty()
        && fixed_writes.is_empty()
        && timing.is_none()
        && resolution.is_none()
    {
        ContractStatus::Unsupported
    } else {
        ContractStatus::Ready
    };
    if bindings.len() > crate::contract::MAX_BINDINGS
        || diagnostics.len() > crate::contract::MAX_DIAGNOSTICS
    {
        return Err(MappingError::new(
            MappingErrorCode::ResourceLimitExceeded,
            "analysis exceeds mapping budget",
        ));
    }
    Ok(MappingContractV1 {
        schema_version: 1,
        contract_version: CONTRACT_VERSION,
        snapshot,
        output_selection,
        bindings,
        diagnostics,
        status,
        fixed_writes,
        timing,
        resolution,
        prompt_projection,
    })
}

const ASSET_ROLES: &[SemanticRole] = &[
    SemanticRole::FirstFrame,
    SemanticRole::LastFrame,
    SemanticRole::ReferenceImage,
    SemanticRole::ReferenceAudio,
    SemanticRole::ReferenceVideo,
    SemanticRole::Mask,
];

fn projection_error(detail: String) -> MappingError {
    MappingError::new(MappingErrorCode::TargetMissing, detail)
}

fn require_node_input(
    graph: &WorkflowGraph,
    node_id: &str,
    input: Option<&str>,
    what: &str,
) -> Result<(), MappingError> {
    let node = graph
        .node(node_id)
        .ok_or_else(|| projection_error(format!("{what}: node {node_id} is absent")))?;
    if let Some(input) = input {
        if !node.inputs.contains_key(input) {
            return Err(projection_error(format!(
                "{what}: input {node_id}.{input} is absent"
            )));
        }
    }
    Ok(())
}

/// Structural validation of platform rule output against the frozen graph (no re-derivation).
fn validate_rule_projection(
    graph: &WorkflowGraph,
    core_bindings: &[ParameterBindingV1],
    projection: &RuleProjectionV1,
) -> Result<(), MappingError> {
    if let Some(projection) = &projection.prompt_projection {
        projection.validate().map_err(projection_error)?;
    }
    let mut ids: BTreeSet<&str> = core_bindings.iter().map(|b| b.binding_id.as_str()).collect();
    for binding in &projection.value_bindings {
        if !ids.insert(binding.binding_id.as_str()) {
            return Err(MappingError::new(
                MappingErrorCode::BindingAmbiguous,
                format!("duplicate bindingId {}", binding.binding_id),
            ));
        }
        if ASSET_ROLES.contains(&binding.semantic_role)
            || binding.value_type == BindingValueType::MediaRef
            || !binding.asset_slots.is_empty()
        {
            return Err(MappingError::new(
                MappingErrorCode::TypeMismatch,
                format!("value binding {} carries media semantics", binding.binding_id),
            ));
        }
        if binding.targets.is_empty()
            || binding.targets.len() > crate::contract::MAX_TARGETS_PER_BINDING
        {
            return Err(MappingError::new(
                MappingErrorCode::ResourceLimitExceeded,
                format!("value binding {} has an invalid target count", binding.binding_id),
            ));
        }
        for target in &binding.targets {
            require_node_input(
                graph,
                &target.node_id,
                Some(&target.input_name),
                &binding.binding_id,
            )?;
        }
    }
    let mut fixed_seen = BTreeSet::new();
    for write in &projection.fixed_writes {
        require_node_input(graph, &write.node_id, Some(&write.input_name), "fixedWrite")?;
        if !fixed_seen.insert((write.node_id.as_str(), write.input_name.as_str())) {
            return Err(MappingError::new(
                MappingErrorCode::WriteConflict,
                format!("duplicate fixed write {}.{}", write.node_id, write.input_name),
            ));
        }
    }
    for binding in &projection.asset_bindings {
        if !ids.insert(binding.binding_id.as_str()) {
            return Err(MappingError::new(
                MappingErrorCode::BindingAmbiguous,
                format!("duplicate bindingId {}", binding.binding_id),
            ));
        }
        if !ASSET_ROLES.contains(&binding.semantic_role)
            || binding.value_type != BindingValueType::MediaRef
            || binding.asset_slots.is_empty()
        {
            return Err(MappingError::new(
                MappingErrorCode::TypeMismatch,
                format!("asset binding {} is malformed", binding.binding_id),
            ));
        }
        if binding.targets.len() > crate::contract::MAX_TARGETS_PER_BINDING {
            return Err(MappingError::new(
                MappingErrorCode::ResourceLimitExceeded,
                format!("asset binding {} exceeds maxTargetsPerBinding", binding.binding_id),
            ));
        }
        for target in &binding.targets {
            require_node_input(
                graph,
                &target.node_id,
                Some(&target.input_name),
                &binding.binding_id,
            )?;
        }
        for slot in &binding.asset_slots {
            if slot.min_count > slot.max_count || slot.max_count == 0 {
                return Err(MappingError::new(
                    MappingErrorCode::ValueOutOfRange,
                    format!("asset binding {} has an invalid capacity", binding.binding_id),
                ));
            }
            let shape_ok = match slot.family {
                AssetSlotFamily::Named => {
                    slot.consumer_node_id.is_some()
                        && slot.input_key.is_some()
                        && slot.key_names.is_empty()
                }
                AssetSlotFamily::Indexed => {
                    slot.consumer_node_id.is_some()
                        && slot.parent_key.is_some()
                        && slot.key_prefix.is_some()
                        && slot.key_names.is_empty()
                }
                AssetSlotFamily::ExplicitNames => explicit_names_shape_ok(slot),
                AssetSlotFamily::Flat => {
                    slot.consumer_node_id.is_none() && slot.key_names.is_empty()
                }
            };
            if !shape_ok {
                return Err(MappingError::new(
                    MappingErrorCode::TypeMismatch,
                    format!("asset binding {} slot shape mismatch", binding.binding_id),
                ));
            }
            if let Some(consumer) = slot.consumer_node_id.as_deref() {
                require_node_input(graph, consumer, None, &binding.binding_id)?;
            }
        }
    }
    if let Some(timing) = &projection.timing {
        for target in &timing.targets {
            require_node_input(graph, &target.node_id, Some(&target.field), "timing")?;
        }
        if let Some(fps) = &timing.fps {
            if let Some(node_id) = fps.node_id.as_deref() {
                require_node_input(graph, node_id, fps.field.as_deref(), "timing.fps")?;
            }
        }
    }
    if let Some(resolution) = &projection.resolution {
        if let Some(target) = &resolution.target {
            let width_node = target.width_node_id.as_deref().unwrap_or(&target.node_id);
            let height_node = target.height_node_id.as_deref().unwrap_or(&target.node_id);
            require_node_input(graph, width_node, None, "resolution.width")?;
            require_node_input(graph, height_node, None, "resolution.height")?;
        }
        if let Some(selector) = &resolution.selector_target {
            require_node_input(
                graph,
                &selector.node_id,
                Some(&selector.aspect_field),
                "resolution.selector",
            )?;
        }
    }
    Ok(())
}

fn role_slug(role: SemanticRole) -> &'static str {
    match role {
        SemanticRole::Steps => "steps",
        SemanticRole::Cfg => "cfg",
        SemanticRole::Denoise => "denoise",
        _ => "parameter",
    }
}

fn scalar_role(field: &str) -> Option<SemanticRole> {
    match field {
        "steps" => Some(SemanticRole::Steps),
        "cfg" => Some(SemanticRole::Cfg),
        "denoise" => Some(SemanticRole::Denoise),
        _ => None,
    }
}

fn analyze_scalar_bindings(
    graph: &WorkflowGraph,
    ancestry: &BTreeSet<String>,
    bindings: &mut Vec<ParameterBindingV1>,
    diagnostics: &mut Vec<MappingDiagnosticV1>,
) {
    for role in [
        SemanticRole::Steps,
        SemanticRole::Cfg,
        SemanticRole::Denoise,
    ] {
        let mut candidates = Vec::new();
        for id in ancestry {
            let Some(node) = graph.node(id) else { continue };
            for (field, value) in &node.inputs {
                if scalar_role(field) == Some(role) && !value.is_array() && value.is_number() {
                    candidates.push((id.clone(), field.clone(), value.clone()));
                }
            }
        }
        if candidates.len() > 1 {
            diagnostics.push(MappingDiagnosticV1 {
                severity: MappingDiagnosticSeverity::Warning,
                code: format!(
                    "{}_MULTIPLE_NOT_EXPOSED",
                    role_slug(role).to_ascii_uppercase()
                ),
                candidate_binding_ids: candidates
                    .iter()
                    .map(|(id, field, _)| format!("node:{id}.{field}"))
                    .collect(),
            });
        } else if let Some((node_id, input_name, default)) = candidates.into_iter().next() {
            let value_type = if default.as_i64().is_some() || default.as_u64().is_some() {
                BindingValueType::Integer
            } else {
                BindingValueType::Number
            };
            bindings.push(ParameterBindingV1 {
                binding_id: format!("parameter.{}", role_slug(role)),
                semantic_role: role,
                value_type,
                targets: vec![BindingTargetV1 {
                    node_id,
                    input_name,
                    transform: TransformKind::Identity,
                    dependency_binding_ids: vec![],
                }],
                default_value: Some(default),
                range: None,
                enum_values: vec![],
                confirmation: ConfirmationKind::Automatic,
                asset_slots: vec![],
            });
        }
    }
}


/// `explicitNames`: consumer + parent, no prefix, a non-empty closed set of unique, non-empty
/// names, and a capacity that the name set can actually hold.
fn explicit_names_shape_ok(slot: &crate::types::AssetSlotV1) -> bool {
    let mut seen = BTreeSet::new();
    let names_ok = !slot.key_names.is_empty()
        && slot
            .key_names
            .iter()
            .all(|n| !n.trim().is_empty() && n.trim() == n && seen.insert(n.as_str()));
    slot.consumer_node_id.is_some()
        && slot.parent_key.is_some()
        && slot.key_prefix.is_none()
        && slot.input_key.is_none()
        && names_ok
        && usize::try_from(slot.max_count).is_ok_and(|max| max <= slot.key_names.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot() -> WorkflowSnapshotV1 {
        WorkflowSnapshotV1 {
            identity: WorkflowIdentityV1 {
                schema_version: 1,
                instance_id: "local".into(),
                user_scope: "default".into(),
                source_kind: SourceKind::ClientLibrary,
                source_path: "workflows/x.json".into(),
                display_name: "X".into(),
            },
            source_digest: "sha256:a".into(),
            graph_digest: String::new(),
            definition_digest: "sha256:b".into(),
            rule_registry_version: 0,
            captured_at_ms: 1,
        }
    }

    fn named_slot(consumer: &str, key: &str) -> AssetSlotV1 {
        AssetSlotV1 {
            family: AssetSlotFamily::Named,
            consumer_node_id: Some(consumer.into()),
            input_key: Some(key.into()),
            parent_key: None,
            key_prefix: None,
            key_names: vec![],
            min_count: 0,
            max_count: 1,
            required: false,
        }
    }

    fn asset_binding(id: &str, role: SemanticRole, load: &str, slot: AssetSlotV1) -> ParameterBindingV1 {
        ParameterBindingV1 {
            binding_id: id.into(),
            semantic_role: role,
            value_type: BindingValueType::MediaRef,
            targets: vec![BindingTargetV1 {
                node_id: load.into(),
                input_name: "image".into(),
                transform: TransformKind::Identity,
                dependency_binding_ids: vec![],
            }],
            default_value: None,
            range: None,
            enum_values: vec![],
            confirmation: ConfirmationKind::Automatic,
            asset_slots: vec![slot],
        }
    }

    fn flf_workflow() -> Value {
        json!({
            "first":{"class_type":"LoadImage","inputs":{"image":"first.png"}},
            "last":{"class_type":"LoadImage","inputs":{"image":"last.png"}},
            "p":{"class_type":"CLIPTextEncode","inputs":{"text":"move"}},
            "sampler":{"class_type":"KSampler","inputs":{"positive":["p",0],"seed":11,"steps":20,"cfg":5.5,"denoise":0.9,"start_image":["first",0],"end_image":["last",0]}},
            "save":{"class_type":"SaveVideo","inputs":{"video":["sampler",0]}}
        })
    }

    #[test]
    fn core_does_not_guess_asset_roles_from_input_names() {
        let workflow = flf_workflow();
        assert_eq!(
            discover_output_candidates(&workflow).unwrap(),
            vec![OutputSelectionV1 {
                node_id: "save".into(),
                media_kind: MediaKind::Video
            }]
        );
        let contract = analyze_mapping_contract(
            &workflow,
            snapshot(),
            OutputSelectionV1 {
                node_id: "save".into(),
                media_kind: MediaKind::Video,
            },
            RuleProjectionV1::default(),
        )
        .unwrap();
        assert!(!contract
            .bindings
            .iter()
            .any(|item| item.value_type == BindingValueType::MediaRef));
        assert!(contract
            .bindings
            .iter()
            .all(|item| item.semantic_role != SemanticRole::Seed
                && item.semantic_role != SemanticRole::PositivePrompt));
        assert!(contract
            .bindings
            .iter()
            .any(|item| item.semantic_role == SemanticRole::Steps
                && item.targets[0].input_name == "steps"));
    }

    fn value_binding(id: &str, role: SemanticRole, node: &str, input: &str) -> ParameterBindingV1 {
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

    fn analyze_video(projection: RuleProjectionV1) -> Result<MappingContractV1, MappingError> {
        analyze_mapping_contract(
            &flf_workflow(),
            snapshot(),
            OutputSelectionV1 {
                node_id: "save".into(),
                media_kind: MediaKind::Video,
            },
            projection,
        )
    }

    #[test]
    fn platform_value_bindings_and_fixed_writes_are_frozen() {
        let contract = analyze_video(RuleProjectionV1 {
            value_bindings: vec![value_binding(
                "prompt.positive",
                SemanticRole::PositivePrompt,
                "p",
                "text",
            )],
            fixed_writes: vec![FixedWriteV1 {
                node_id: "sampler".into(),
                input_name: "denoise".into(),
                value: json!(1.0),
            }],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(contract.bindings[0].binding_id, "prompt.positive");
        assert_eq!(contract.fixed_writes.len(), 1);
        assert_eq!(contract.status, ContractStatus::Ready);
    }

    #[test]
    fn value_binding_with_missing_input_is_rejected() {
        let err = analyze_video(RuleProjectionV1 {
            value_bindings: vec![value_binding(
                "prompt.positive",
                SemanticRole::PositivePrompt,
                "p",
                "ghost_input",
            )],
            ..Default::default()
        })
        .unwrap_err();
        assert_eq!(err.code, MappingErrorCode::TargetMissing);
    }

    #[test]
    fn value_binding_with_media_semantics_is_rejected() {
        let mut binding = value_binding("media.x", SemanticRole::FirstFrame, "first", "image");
        binding.value_type = BindingValueType::MediaRef;
        let err = analyze_video(RuleProjectionV1 {
            value_bindings: vec![binding],
            ..Default::default()
        })
        .unwrap_err();
        assert_eq!(err.code, MappingErrorCode::TypeMismatch);
    }

    #[test]
    fn platform_asset_projection_is_frozen_after_structural_validation() {
        let workflow = flf_workflow();
        let projection = RuleProjectionV1 {
            asset_bindings: vec![
                asset_binding(
                    "media.firstFrame",
                    SemanticRole::FirstFrame,
                    "first",
                    named_slot("sampler", "start_image"),
                ),
                asset_binding(
                    "media.lastFrame",
                    SemanticRole::LastFrame,
                    "last",
                    named_slot("sampler", "end_image"),
                ),
            ],
            ..Default::default()
        };
        let contract = analyze_mapping_contract(
            &workflow,
            snapshot(),
            OutputSelectionV1 {
                node_id: "save".into(),
                media_kind: MediaKind::Video,
            },
            projection,
        )
        .unwrap();
        let first = contract
            .bindings
            .iter()
            .find(|b| b.semantic_role == SemanticRole::FirstFrame)
            .unwrap();
        assert_eq!(first.targets[0].node_id, "first");
        assert_eq!(first.asset_slots[0].input_key.as_deref(), Some("start_image"));
    }

    #[test]
    fn projection_targeting_missing_nodes_is_rejected() {
        let workflow = flf_workflow();
        let projection = RuleProjectionV1 {
            asset_bindings: vec![asset_binding(
                "media.firstFrame",
                SemanticRole::FirstFrame,
                "ghost",
                named_slot("sampler", "start_image"),
            )],
            ..Default::default()
        };
        let err = analyze_mapping_contract(
            &workflow,
            snapshot(),
            OutputSelectionV1 {
                node_id: "save".into(),
                media_kind: MediaKind::Video,
            },
            projection,
        )
        .unwrap_err();
        assert_eq!(err.code, MappingErrorCode::TargetMissing);
    }

    #[test]
    fn invalid_capacity_is_rejected() {
        let workflow = flf_workflow();
        let mut slot = named_slot("sampler", "start_image");
        slot.min_count = 2;
        let projection = RuleProjectionV1 {
            asset_bindings: vec![asset_binding(
                "media.firstFrame",
                SemanticRole::FirstFrame,
                "first",
                slot,
            )],
            ..Default::default()
        };
        let err = analyze_mapping_contract(
            &workflow,
            snapshot(),
            OutputSelectionV1 {
                node_id: "save".into(),
                media_kind: MediaKind::Video,
            },
            projection,
        )
        .unwrap_err();
        assert_eq!(err.code, MappingErrorCode::ValueOutOfRange);
    }

    #[test]
    fn multiple_scalar_targets_are_not_guessed() {
        let workflow = json!({
            "p":{"class_type":"CLIPTextEncode","inputs":{"text":"move"}},
            "a":{"class_type":"KSampler","inputs":{"positive":["p",0],"steps":10}},
            "b":{"class_type":"KSampler","inputs":{"positive":["p",0],"steps":30}},
            "save":{"class_type":"SaveImage","inputs":{"a":["a",0],"b":["b",0]}}
        });
        let contract = analyze_mapping_contract(
            &workflow,
            snapshot(),
            OutputSelectionV1 {
                node_id: "save".into(),
                media_kind: MediaKind::Image,
            },
            RuleProjectionV1::default(),
        )
        .unwrap();
        assert!(!contract
            .bindings
            .iter()
            .any(|item| item.semantic_role == SemanticRole::Steps));
        assert!(contract
            .diagnostics
            .iter()
            .any(|item| item.code == "STEPS_MULTIPLE_NOT_EXPOSED"));
    }

    fn explicit_slot(names: &[&str], max: u32) -> AssetSlotV1 {
        AssetSlotV1 {
            family: AssetSlotFamily::ExplicitNames,
            consumer_node_id: Some("sampler".into()),
            input_key: None,
            parent_key: Some("images".into()),
            key_prefix: None,
            key_names: names.iter().map(|n| (*n).to_owned()).collect(),
            min_count: 0,
            max_count: max,
            required: false,
        }
    }

    fn analyze_explicit(slot: AssetSlotV1) -> Result<MappingContractV1, MappingError> {
        analyze_video(RuleProjectionV1 {
            asset_bindings: vec![asset_binding("media.referenceImage", SemanticRole::ReferenceImage, "first", slot)],
            ..Default::default()
        })
    }

    #[test]
    fn explicit_names_slot_is_frozen_with_its_declared_order() {
        let contract = analyze_explicit(explicit_slot(&["image_1", "image_2", "image_3"], 3)).unwrap();
        let slot = &contract.bindings.iter().find(|b| b.binding_id == "media.referenceImage").unwrap().asset_slots[0];
        assert_eq!(slot.family, AssetSlotFamily::ExplicitNames);
        assert_eq!(slot.key_names, vec!["image_1", "image_2", "image_3"]);
        let wire = serde_json::to_value(slot).unwrap();
        assert_eq!(wire["family"], "explicitNames");
        assert_eq!(wire["keyNames"], json!(["image_1", "image_2", "image_3"]));
    }

    #[test]
    fn explicit_names_slot_shape_violations_are_rejected() {
        let mut with_prefix = explicit_slot(&["image_1"], 1);
        with_prefix.key_prefix = Some("image".into());
        let mut with_input_key = explicit_slot(&["image_1"], 1);
        with_input_key.input_key = Some("images.image_1".into());
        let mut no_parent = explicit_slot(&["image_1"], 1);
        no_parent.parent_key = None;
        for (label, slot) in [
            ("empty", explicit_slot(&[], 1)),
            ("duplicate", explicit_slot(&["image_1", "image_1"], 2)),
            ("blank", explicit_slot(&["image_1", " "], 2)),
            ("over capacity", explicit_slot(&["image_1", "image_2"], 3)),
            ("prefix", with_prefix),
            ("input key", with_input_key),
            ("no parent", no_parent),
        ] {
            let err = analyze_explicit(slot).unwrap_err();
            assert_eq!(err.code, MappingErrorCode::TypeMismatch, "{label}");
        }
    }

    #[test]
    fn non_explicit_families_reject_key_names() {
        let mut named = named_slot("sampler", "start_image");
        named.key_names = vec!["image_1".into()];
        let err = analyze_explicit(named).unwrap_err();
        assert_eq!(err.code, MappingErrorCode::TypeMismatch);
    }

    #[test]
    fn legacy_slot_without_key_names_still_deserializes() {
        let slot: AssetSlotV1 = serde_json::from_value(json!({
            "family": "flat", "minCount": 0, "maxCount": 1, "required": false
        })).unwrap();
        assert!(slot.key_names.is_empty());
        assert!(serde_json::to_value(&slot).unwrap().get("keyNames").is_none());
    }
}
