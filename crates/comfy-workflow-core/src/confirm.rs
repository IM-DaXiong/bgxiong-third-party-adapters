#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};
use alloc::collections::{BTreeMap, BTreeSet};

use crate::error::{MappingError, MappingErrorCode};
use crate::types::*;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingResolutionV1 {
    pub diagnostic_code: String,
    pub selected_candidate_binding_id: String,
}

fn role_for_diagnostic(code: &str) -> Option<(String, SemanticRole, BindingValueType)> {
    match code {
        "POSITIVE_PROMPT_AMBIGUOUS" => Some((
            "prompt.positive".into(),
            SemanticRole::PositivePrompt,
            BindingValueType::String,
        )),
        "NEGATIVE_PROMPT_AMBIGUOUS" => Some((
            "prompt.negative".into(),
            SemanticRole::NegativePrompt,
            BindingValueType::String,
        )),
        "FIRSTFRAME_AMBIGUOUS" => Some((
            "media.firstFrame".into(),
            SemanticRole::FirstFrame,
            BindingValueType::MediaRef,
        )),
        "LASTFRAME_AMBIGUOUS" => Some((
            "media.lastFrame".into(),
            SemanticRole::LastFrame,
            BindingValueType::MediaRef,
        )),
        "MASK_AMBIGUOUS" => Some((
            "media.mask".into(),
            SemanticRole::Mask,
            BindingValueType::MediaRef,
        )),
        _ => None,
    }
}

fn parse_candidate(candidate: &str) -> Result<(String, String), MappingError> {
    let raw = candidate.strip_prefix("node:").ok_or_else(|| {
        MappingError::new(
            MappingErrorCode::BindingAmbiguous,
            "mapping candidate has an invalid prefix",
        )
    })?;
    let (node_id, input_name) = raw.rsplit_once('.').ok_or_else(|| {
        MappingError::new(
            MappingErrorCode::BindingAmbiguous,
            "mapping candidate has no input field",
        )
    })?;
    if node_id.is_empty() || input_name.is_empty() {
        return Err(MappingError::new(
            MappingErrorCode::BindingAmbiguous,
            "mapping candidate is empty",
        ));
    }
    Ok((node_id.to_string(), input_name.to_string()))
}

/// Converts explicit user choices into bindings. Every blocking diagnostic must be resolved once,
/// and the selected target must come from that diagnostic's closed candidate set.
pub fn confirm_mapping_contract(
    mut contract: MappingContractV1,
    resolutions: Vec<MappingResolutionV1>,
) -> Result<MappingContractV1, MappingError> {
    if contract.status != ContractStatus::NeedsConfirmation {
        return Err(MappingError::new(
            MappingErrorCode::BindingAmbiguous,
            "mapping contract does not require confirmation",
        ));
    }
    let supplied: BTreeMap<_, _> = resolutions
        .into_iter()
        .map(|item| (item.diagnostic_code, item.selected_candidate_binding_id))
        .collect();
    let blocking_codes: BTreeSet<_> = contract
        .diagnostics
        .iter()
        .filter(|item| item.severity == MappingDiagnosticSeverity::Blocking)
        .map(|item| item.code.clone())
        .collect();
    if supplied.len() != blocking_codes.len()
        || supplied.keys().any(|code| !blocking_codes.contains(code))
    {
        return Err(MappingError::new(
            MappingErrorCode::BindingAmbiguous,
            "every blocking mapping diagnostic must be resolved exactly once",
        ));
    }

    for diagnostic in contract
        .diagnostics
        .iter()
        .filter(|item| item.severity == MappingDiagnosticSeverity::Blocking)
    {
        let selected = supplied.get(&diagnostic.code).ok_or_else(|| {
            MappingError::new(
                MappingErrorCode::BindingAmbiguous,
                format!("missing resolution for {}", diagnostic.code),
            )
        })?;
        if !diagnostic.candidate_binding_ids.contains(selected) {
            return Err(MappingError::new(
                MappingErrorCode::BindingAmbiguous,
                format!("selected target is not a candidate for {}", diagnostic.code),
            ));
        }
        let (binding_id, semantic_role, value_type) = role_for_diagnostic(&diagnostic.code)
            .ok_or_else(|| {
                MappingError::new(
                    MappingErrorCode::BindingAmbiguous,
                    format!("unsupported blocking diagnostic {}", diagnostic.code),
                )
            })?;
        let (node_id, input_name) = parse_candidate(selected)?;
        contract
            .bindings
            .retain(|item| item.binding_id != binding_id);
        contract.bindings.push(ParameterBindingV1 {
            binding_id,
            semantic_role,
            value_type,
            targets: vec![BindingTargetV1 {
                node_id,
                input_name,
                transform: TransformKind::Identity,
                dependency_binding_ids: vec![],
            }],
            default_value: None,
            range: None,
            enum_values: vec![],
            confirmation: ConfirmationKind::UserConfirmed,
            asset_slots: vec![],
        });
    }
    contract
        .diagnostics
        .retain(|item| item.severity != MappingDiagnosticSeverity::Blocking);
    contract.status = ContractStatus::Ready;
    Ok(contract)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_target_outside_the_closed_candidate_set() {
        let contract: MappingContractV1 = serde_json::from_value(serde_json::json!({
            "schemaVersion":1,"contractVersion":1,
            "snapshot":{"identity":{"schemaVersion":1,"instanceId":"local","userScope":"default","sourceKind":"clientLibrary","sourcePath":"workflows/x.json","displayName":"X"},"sourceDigest":"a","graphDigest":"b","definitionDigest":"c","ruleRegistryVersion":1,"capturedAtMs":1},
            "outputSelection":{"nodeId":"9","mediaKind":"image"},"bindings":[],
            "diagnostics":[{"severity":"blocking","code":"POSITIVE_PROMPT_AMBIGUOUS","candidateBindingIds":["node:1.text","node:2.text"]}],"status":"needsConfirmation"
        })).unwrap();
        let err = confirm_mapping_contract(
            contract,
            vec![MappingResolutionV1 {
                diagnostic_code: "POSITIVE_PROMPT_AMBIGUOUS".into(),
                selected_candidate_binding_id: "node:3.text".into(),
            }],
        )
        .unwrap_err();
        assert_eq!(err.code, MappingErrorCode::BindingAmbiguous);
    }
}


/// Error of [`confirm_selections`] (CW55[L01]). Opaque ids are never parsed by display text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmError {
    /// A diagnostic id appears more than once in the selection list.
    DuplicateDiagnostic(DiagnosticId),
    /// Candidate id is not a structural `{MappingTarget:?}|{execId}|{inputName}` triple.
    InvalidCandidateId(CandidateId),
    /// Mapping target of the candidate has no standard binding role (outputs / Advanced).
    UnsupportedTarget { candidate_id: CandidateId, target: MappingTarget },
    /// The contract is not in a confirmable status.
    NotConfirmable { status: ContractStatus },
}

impl core::fmt::Display for ConfirmError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DuplicateDiagnostic(id) => write!(f, "duplicate diagnostic {}", id.0),
            Self::InvalidCandidateId(id) => write!(f, "invalid candidate id {}", id.0),
            Self::UnsupportedTarget { candidate_id, target } => {
                write!(f, "candidate {} has unsupported target {target:?}", candidate_id.0)
            }
            Self::NotConfirmable { status } => write!(f, "contract status {status:?} is not confirmable"),
        }
    }
}

impl core::error::Error for ConfirmError {}

/// Parse a structural candidate id produced by the host (`{MappingTarget:?}|{execId}|{inputName}`).
/// The triple is split on the first and last `|` so `execId` may contain `:` (subgraph) but not `|`.
pub fn parse_candidate_id(id: &CandidateId) -> Result<(MappingTarget, String, String), ConfirmError> {
    let raw = &id.0;
    let (target_s, rest) = raw
        .split_once('|')
        .ok_or_else(|| ConfirmError::InvalidCandidateId(id.clone()))?;
    let (exec_id, input_name) = rest
        .rsplit_once('|')
        .ok_or_else(|| ConfirmError::InvalidCandidateId(id.clone()))?;
    if exec_id.is_empty() || input_name.is_empty() {
        return Err(ConfirmError::InvalidCandidateId(id.clone()));
    }
    let target = match target_s {
        "PositivePrompt" => MappingTarget::PositivePrompt,
        "NegativePrompt" => MappingTarget::NegativePrompt,
        "Seed" => MappingTarget::Seed,
        "Steps" => MappingTarget::Steps,
        "Cfg" => MappingTarget::Cfg,
        "Denoise" => MappingTarget::Denoise,
        "Width" => MappingTarget::Width,
        "Height" => MappingTarget::Height,
        "AspectRatio" => MappingTarget::AspectRatio,
        "Resolution" => MappingTarget::Resolution,
        "DurationSeconds" => MappingTarget::DurationSeconds,
        "Fps" => MappingTarget::Fps,
        "FirstFrame" => MappingTarget::FirstFrame,
        "LastFrame" => MappingTarget::LastFrame,
        "ReferenceImage" => MappingTarget::ReferenceImage,
        "ReferenceVideo" => MappingTarget::ReferenceVideo,
        "ReferenceAudio" => MappingTarget::ReferenceAudio,
        "Mask" => MappingTarget::Mask,
        "OutputImage" => MappingTarget::OutputImage,
        "OutputVideo" => MappingTarget::OutputVideo,
        "OutputAudio" => MappingTarget::OutputAudio,
        "Advanced" => MappingTarget::Advanced,
        _ => return Err(ConfirmError::InvalidCandidateId(id.clone())),
    };
    Ok((target, exec_id.to_string(), input_name.to_string()))
}

fn role_and_value_type(
    target: MappingTarget,
    candidate_id: &CandidateId,
) -> Result<(SemanticRole, BindingValueType, String), ConfirmError> {
    let (role, vt, binding_id) = match target {
        MappingTarget::PositivePrompt => (SemanticRole::PositivePrompt, BindingValueType::String, "prompt.positive"),
        MappingTarget::NegativePrompt => (SemanticRole::NegativePrompt, BindingValueType::String, "prompt.negative"),
        MappingTarget::Seed => (SemanticRole::Seed, BindingValueType::Integer, "sampler.seed"),
        MappingTarget::Steps => (SemanticRole::Steps, BindingValueType::Integer, "sampler.steps"),
        MappingTarget::Cfg => (SemanticRole::Cfg, BindingValueType::Number, "sampler.cfg"),
        MappingTarget::Denoise => (SemanticRole::Denoise, BindingValueType::Number, "sampler.denoise"),
        MappingTarget::Width => (SemanticRole::Width, BindingValueType::Integer, "size.width"),
        MappingTarget::Height => (SemanticRole::Height, BindingValueType::Integer, "size.height"),
        MappingTarget::AspectRatio => (SemanticRole::Aspect, BindingValueType::String, "size.aspect"),
        MappingTarget::Resolution => (SemanticRole::Resolution, BindingValueType::String, "size.resolution"),
        MappingTarget::DurationSeconds => (SemanticRole::Duration, BindingValueType::Number, "timing.duration"),
        MappingTarget::Fps => (SemanticRole::Fps, BindingValueType::Number, "timing.fps"),
        MappingTarget::FirstFrame => (SemanticRole::FirstFrame, BindingValueType::MediaRef, "media.firstFrame"),
        MappingTarget::LastFrame => (SemanticRole::LastFrame, BindingValueType::MediaRef, "media.lastFrame"),
        MappingTarget::ReferenceImage => (SemanticRole::ReferenceImage, BindingValueType::MediaRef, "media.referenceImage"),
        MappingTarget::ReferenceVideo => (SemanticRole::ReferenceVideo, BindingValueType::MediaRef, "media.referenceVideo"),
        MappingTarget::ReferenceAudio => (SemanticRole::ReferenceAudio, BindingValueType::MediaRef, "media.referenceAudio"),
        MappingTarget::Mask => (SemanticRole::Mask, BindingValueType::MediaRef, "media.mask"),
        MappingTarget::OutputImage
        | MappingTarget::OutputVideo
        | MappingTarget::OutputAudio
        | MappingTarget::Advanced => {
            return Err(ConfirmError::UnsupportedTarget {
                candidate_id: candidate_id.clone(),
                target,
            })
        }
    };
    Ok((role, vt, binding_id.into()))
}

/// Apply structured `(diagnosticId, candidateId)` selections to a mapping contract (CW55).
///
/// Pure: no IO. The manifest revision CAS is enforced by the host repository before this call
/// (`mapping_repository::confirm_mapping` reads the current revision and refuses a stale one);
/// `_expected_revision` is part of the registered signature (`## 接口登记`) and is not re-checked
/// here because the contract carries no revision field. The host also checks that every pair was
/// offered by the analysis. This function refuses duplicate diagnostic ids, unparseable candidate
/// ids and targets without a standard binding role, then writes one user-confirmed binding per
/// selection, and drops the blocking role ambiguities those bindings answer. The status becomes
/// `ready` only when no blocking diagnostic remains.
pub fn confirm_selections(
    contract: &MappingContract,
    _expected_revision: u64,
    selections: &[(DiagnosticId, CandidateId)],
) -> Result<MappingContract, ConfirmError> {
    if !matches!(
        contract.status,
        ContractStatus::NeedsConfirmation | ContractStatus::Ready
    ) {
        return Err(ConfirmError::NotConfirmable {
            status: contract.status,
        });
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut next = contract.clone();
    for (diagnostic_id, candidate_id) in selections {
        if !seen.insert(diagnostic_id.0.as_str()) {
            return Err(ConfirmError::DuplicateDiagnostic(diagnostic_id.clone()));
        }
        let (target, exec_id, input_name) = parse_candidate_id(candidate_id)?;
        let (semantic_role, value_type, binding_id) = role_and_value_type(target, candidate_id)?;
        next.bindings.retain(|b| b.binding_id != binding_id);
        next.bindings.push(ParameterBindingV1 {
            binding_id,
            semantic_role,
            value_type,
            targets: vec![BindingTargetV1 {
                node_id: exec_id,
                input_name,
                transform: TransformKind::Identity,
                dependency_binding_ids: vec![],
            }],
            default_value: None,
            range: None,
            enum_values: vec![],
            confirmation: ConfirmationKind::UserConfirmed,
            asset_slots: vec![],
        });
    }
    // A blocking role ambiguity is answered once its role carries a user-confirmed binding;
    // any other blocking diagnostic stays and keeps the contract out of `ready`.
    let confirmed: BTreeSet<String> = next
        .bindings
        .iter()
        .filter(|b| b.confirmation == ConfirmationKind::UserConfirmed)
        .map(|b| b.binding_id.clone())
        .collect();
    next.diagnostics.retain(|d| {
        d.severity != MappingDiagnosticSeverity::Blocking
            || !role_for_diagnostic(&d.code).is_some_and(|(binding_id, _, _)| confirmed.contains(&binding_id))
    });
    if next
        .diagnostics
        .iter()
        .all(|d| d.severity != MappingDiagnosticSeverity::Blocking)
    {
        next.status = ContractStatus::Ready;
    }
    Ok(next)
}

#[cfg(test)]
mod confirm_selections_tests {
    use super::*;
    use serde_json::json;

    fn base_contract() -> MappingContractV1 {
        serde_json::from_value(json!({
            "schemaVersion":1,"contractVersion":2,
            "snapshot":{"identity":{"schemaVersion":1,"instanceId":"local","userScope":"default","sourceKind":"clientLibrary","sourcePath":"workflows/x.json","displayName":"X"},"sourceDigest":"a","graphDigest":"b","definitionDigest":"c","ruleRegistryVersion":1,"capturedAtMs":1},
            "outputSelection":{"nodeId":"9","mediaKind":"image"},"bindings":[],
            "diagnostics":[{"severity":"blocking","code":"POSITIVE_PROMPT_AMBIGUOUS","candidateBindingIds":[]}],
            "status":"needsConfirmation"
        }))
        .unwrap()
    }

    #[test]
    fn same_code_three_diagnostics_confirm_independently() {
        let contract = base_contract();
        let a = DiagnosticId("d1".into());
        let b = DiagnosticId("d2".into());
        let c = DiagnosticId("d3".into());
        let ca = CandidateId("PositivePrompt|1|text".into());
        let cb = CandidateId("PositivePrompt|2|text".into());
        let cc = CandidateId("NegativePrompt|3|text".into());
        let out = confirm_selections(&contract, 7, &[(a, ca), (b, cb.clone()), (c, cc)]).unwrap();
        assert_eq!(out.status, ContractStatus::Ready);
        assert!(out.bindings.iter().any(|x| x.binding_id == "prompt.positive"
            && x.targets[0].node_id == "2"
            && x.confirmation == ConfirmationKind::UserConfirmed));
        assert!(out.bindings.iter().any(|x| x.binding_id == "prompt.negative"
            && x.targets[0].node_id == "3"));
        // Last PositivePrompt selection wins the single role binding.
        assert_eq!(
            out.bindings
                .iter()
                .find(|x| x.binding_id == "prompt.positive")
                .unwrap()
                .targets[0]
                .node_id,
            "2"
        );
        let _ = cb;
    }

    #[test]
    fn dotted_node_id_and_input_name_are_not_reparsed() {
        let contract = base_contract();
        let out = confirm_selections(
            &contract,
            1,
            &[(
                DiagnosticId("d".into()),
                CandidateId("Seed|232:242.7|noise.seed".into()),
            )],
        )
        .unwrap();
        let b = out.bindings.iter().find(|x| x.binding_id == "sampler.seed").unwrap();
        assert_eq!(b.targets[0].node_id, "232:242.7");
        assert_eq!(b.targets[0].input_name, "noise.seed");
    }

    #[test]
    fn duplicate_diagnostic_is_refused() {
        let contract = base_contract();
        let d = DiagnosticId("same".into());
        let err = confirm_selections(
            &contract,
            1,
            &[
                (d.clone(), CandidateId("Seed|1|seed".into())),
                (d, CandidateId("Seed|2|seed".into())),
            ],
        )
        .unwrap_err();
        assert!(matches!(err, ConfirmError::DuplicateDiagnostic(_)));
    }
}