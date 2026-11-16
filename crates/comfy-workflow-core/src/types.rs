#[cfg(target_arch = "wasm32")]
use alloc::{string::String, vec::Vec};
use alloc::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::media_contracts::{DurationContract, ResolutionContract};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowIdentityV1 {
    pub schema_version: u32,
    pub instance_id: String,
    pub user_scope: String,
    pub source_kind: SourceKind,
    pub source_path: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    ComfyUserData,
    LocalFile,
    ClientLibrary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSnapshotV1 {
    pub identity: WorkflowIdentityV1,
    pub source_digest: String,
    pub graph_digest: String,
    pub definition_digest: String,
    pub rule_registry_version: u32,
    pub captured_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputSelectionV1 {
    pub node_id: String,
    pub media_kind: MediaKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaKind {
    Image,
    Video,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContractStatus {
    Ready,
    NeedsConfirmation,
    Unsupported,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfirmationKind {
    Automatic,
    UserConfirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SemanticRole {
    PositivePrompt,
    NegativePrompt,
    Seed,
    Width,
    Height,
    Aspect,
    Resolution,
    Duration,
    Fps,
    FirstFrame,
    LastFrame,
    ReferenceImage,
    ReferenceAudio,
    ReferenceVideo,
    Mask,
    Steps,
    Cfg,
    Denoise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BindingValueType {
    String,
    Integer,
    Number,
    Boolean,
    MediaRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransformKind {
    Identity,
    SecondsToFrames,
    FramesToSeconds,
    AspectSelector,
    ResolutionSelector,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingTargetV1 {
    pub node_id: String,
    pub input_name: String,
    pub transform: TransformKind,
    #[serde(default)]
    pub dependency_binding_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberRangeV1 {
    pub min: f64,
    pub max: f64,
    pub step: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterBindingV1 {
    pub binding_id: String,
    pub semantic_role: SemanticRole,
    pub value_type: BindingValueType,
    pub targets: Vec<BindingTargetV1>,
    pub default_value: Option<Value>,
    pub range: Option<NumberRangeV1>,
    #[serde(default)]
    pub enum_values: Vec<Value>,
    pub confirmation: ConfirmationKind,
    /// Asset slot structure from the platform asset adapter (named / indexed autogrow / flat).
    /// Non-empty marks a structural media binding: applied by the asset adapter, never by a
    /// plain value write.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asset_slots: Vec<AssetSlotV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssetSlotFamily {
    /// Named optional consumer input (e.g. first_frame / end_image).
    Named,
    /// Indexed dynamic / Autogrow group (`parent.prefix{n}`), capacity-bounded.
    Indexed,
    /// Declared ordered name list of a dynamic group (Autogrow `template.names`, e.g.
    /// `parent.image_1 … parent.image_16`); capacity is bounded by the closed name set.
    ExplicitNames,
    /// Classic LoadImage natural order (no named/indexed consumer).
    Flat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetSlotV1 {
    pub family: AssetSlotFamily,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_prefix: Option<String>,
    /// `explicitNames` only: the declared child names in declaration order (frozen so that the
    /// executor never re-derives keys); the full input key is `{parent_key}.{name}`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_names: Vec<String>,
    pub min_count: u32,
    pub max_count: u32,
    pub required: bool,
}

/// Constant graph write frozen at analysis (e.g. pinning an on-graph prompt-enhance switch off).
/// Applied on every submit; never driven by a user value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixedWriteV1 {
    pub node_id: String,
    pub input_name: String,
    pub value: Value,
}

/// Platform rule output handed to the core for validation and freezing.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleProjectionV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_projection: Option<crate::prompt_projection::PromptProjectionDescriptor>,
    /// Prompt / seed / image-size value bindings from the platform generation plan (D2).
    #[serde(default)]
    pub value_bindings: Vec<ParameterBindingV1>,
    #[serde(default)]
    pub asset_bindings: Vec<ParameterBindingV1>,
    #[serde(default)]
    pub fixed_writes: Vec<FixedWriteV1>,
    #[serde(default)]
    pub timing: Option<DurationContract>,
    #[serde(default)]
    pub resolution: Option<ResolutionContract>,
}

/// Severity of a legacy mapping-contract diagnostic (`MappingDiagnosticV1`). Wire values stay
/// `info | warning | blocking` (closed set `diagnosticSeverities` in comfy-workflow-mapping.json).
/// Renamed from `DiagnosticSeverity` (CW11[L01]): that name now belongs to the compile/import
/// diagnostic severity registered in `## 接口登记`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MappingDiagnosticSeverity {
    Info,
    Warning,
    Blocking,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingDiagnosticV1 {
    pub severity: MappingDiagnosticSeverity,
    pub code: String,
    #[serde(default)]
    pub candidate_binding_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingContractV1 {
    /// Explicit format declaration, frozen with this workflow version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_projection: Option<crate::prompt_projection::PromptProjectionDescriptor>,
    pub schema_version: u32,
    pub contract_version: u32,
    pub snapshot: WorkflowSnapshotV1,
    pub output_selection: OutputSelectionV1,
    pub bindings: Vec<ParameterBindingV1>,
    #[serde(default)]
    pub diagnostics: Vec<MappingDiagnosticV1>,
    pub status: ContractStatus,
    /// Constant writes applied on every submit (frozen platform plan, D2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixed_writes: Vec<FixedWriteV1>,
    /// Frozen timing sub-contract (duration + fps; INV-COMFY-TIMING) for video outputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<DurationContract>,
    /// Frozen resolution sub-contract (INV-COMFY-RES) for video outputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<ResolutionContract>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingValueV1 {
    pub binding_id: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequestV1 {
    pub contract: MappingContractV1,
    pub workflow: Value,
    pub values: Vec<BindingValueV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedTargetV1 {
    pub node_id: String,
    pub input_name: String,
    pub actual_value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedBindingV1 {
    pub binding_id: String,
    pub semantic_role: SemanticRole,
    pub targets: Vec<AppliedTargetV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResultV1 {
    pub workflow: Value,
    pub applied_bindings: Vec<AppliedBindingV1>,
}

// ---------------------------------------------------------------------------------------------
// Import / compile contract types (PLAN 20261014 CW10[L01], CW11, CW53[L01], CW54[L01]).
// Registered in `## 接口登记` · Pure crate types. Closed sets are mirrored 1:1 by
// packages/app-contracts/v1/comfy-workflow-mapping.json (checked by the tests below).
// ---------------------------------------------------------------------------------------------

/// Mapping contract version (CW11). Single source: `contractVersion` of comfy-workflow-mapping.json.
pub const MAPPING_CONTRACT_VERSION: u32 = crate::contract::CONTRACT_VERSION;

/// Registered name of the frozen mapping contract (`## 接口登记` uses `MappingContract`).
pub type MappingContract = MappingContractV1;

/// Stable structural diagnostic identity. Opaque: never parsed or split by consumers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiagnosticId(pub String);

/// Stable structural candidate identity. Opaque: never split on '.' or any other separator.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CandidateId(pub String);

/// Source location of a node: subgraph instance path (outermost first) + local node id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeLocator {
    pub scope_path: Vec<String>,
    pub node_id: String,
}

/// Severity of a compile / import diagnostic (closed set `compileDiagnosticSeverities`).
/// `NonBlocking` is reported but never prevents the selected output from compiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Blocking,
    NonBlocking,
}

/// One structured compile / import diagnostic. `code` is from the closed import diagnostic set
/// (`$defs.diagnosticCode` of comfy-workflow-import-v1.schema.json, generated as
/// `contract::IMPORT_DIAGNOSTIC_CODES`); `params` are the template parameters of the SAME
/// diagnostic (CW46), never derived from display text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompileDiagnostic {
    pub diagnostic_id: DiagnosticId,
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub locator: Option<NodeLocator>,
    pub class_type: Option<String>,
    pub input_name: Option<String>,
    pub params: BTreeMap<String, String>,
}

/// Typed mapping target (closed set `mappingTargets`). `Advanced` = a constrained descriptor
/// (`AdvancedParamDescriptor`), never a free-form write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MappingTarget {
    PositivePrompt,
    NegativePrompt,
    Seed,
    Steps,
    Cfg,
    Denoise,
    Width,
    Height,
    AspectRatio,
    Resolution,
    DurationSeconds,
    Fps,
    FirstFrame,
    LastFrame,
    ReferenceImage,
    ReferenceVideo,
    ReferenceAudio,
    Mask,
    OutputImage,
    OutputVideo,
    OutputAudio,
    Advanced,
}

/// Why a candidate was proposed (closed set `evidenceKinds`). `NameHint` / `NoteHint` alone are
/// never structural proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceKind {
    OutputAncestry,
    ConsumerPortType,
    ReviewedSemanticRule,
    NameHint,
    NoteHint,
}

/// Value type of an advanced parameter (closed set `paramValueTypes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParamValueType {
    Int,
    Float,
    Boolean,
    String,
    Combo,
}

/// Constrained descriptor of a verifiable parameter that has no standard role (CW53). Ranges and
/// options come from the node definition; `value` is the unmodified current graph value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdvancedParamDescriptor {
    pub binding_id: String,
    pub label: String,
    pub value_type: ParamValueType,
    pub value: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub options: Option<Vec<Value>>,
    pub unit: Option<String>,
    pub source: NodeLocator,
    pub input_name: String,
}

#[cfg(test)]
mod import_contract_set_tests {
    use super::*;

    fn wire<T: Serialize>(v: &T) -> String {
        serde_json::to_value(v)
            .expect("closed enum serializes")
            .as_str()
            .expect("closed enum is a string")
            .to_string()
    }

    fn assert_same(rust: Vec<String>, contract: &[&str], name: &str) {
        let contract: Vec<String> = contract.iter().map(|s| s.to_string()).collect();
        assert_eq!(rust, contract, "{name}: Rust enum and comfy-workflow-mapping.json differ");
    }

    #[test]
    fn closed_sets_match_the_mapping_contract() {
        use MappingTarget as T;
        let targets = [
            T::PositivePrompt, T::NegativePrompt, T::Seed, T::Steps, T::Cfg, T::Denoise, T::Width,
            T::Height, T::AspectRatio, T::Resolution, T::DurationSeconds, T::Fps, T::FirstFrame,
            T::LastFrame, T::ReferenceImage, T::ReferenceVideo, T::ReferenceAudio, T::Mask,
            T::OutputImage, T::OutputVideo, T::OutputAudio, T::Advanced,
        ];
        assert_same(targets.iter().map(wire).collect(), crate::contract::MAPPING_TARGETS, "mappingTargets");
        use EvidenceKind as E;
        let evidence = [E::OutputAncestry, E::ConsumerPortType, E::ReviewedSemanticRule, E::NameHint, E::NoteHint];
        assert_same(evidence.iter().map(wire).collect(), crate::contract::EVIDENCE_KINDS, "evidenceKinds");
        use ParamValueType as P;
        let types = [P::Int, P::Float, P::Boolean, P::String, P::Combo];
        assert_same(types.iter().map(wire).collect(), crate::contract::PARAM_VALUE_TYPES, "paramValueTypes");
        let sev = [DiagnosticSeverity::Blocking, DiagnosticSeverity::NonBlocking];
        assert_same(
            sev.iter().map(wire).collect(),
            crate::contract::COMPILE_DIAGNOSTIC_SEVERITIES,
            "compileDiagnosticSeverities",
        );
        let legacy = [MappingDiagnosticSeverity::Info, MappingDiagnosticSeverity::Warning, MappingDiagnosticSeverity::Blocking];
        assert_same(legacy.iter().map(wire).collect(), crate::contract::DIAGNOSTIC_SEVERITIES, "diagnosticSeverities");
    }

    #[test]
    fn ids_are_opaque_strings_on_the_wire() {
        let id = CandidateId("PositivePrompt|a.b:c|text.x".into());
        assert_eq!(serde_json::to_value(&id).unwrap(), serde_json::json!("PositivePrompt|a.b:c|text.x"));
        let back: DiagnosticId = serde_json::from_value(serde_json::json!("d.1.2")).unwrap();
        assert_eq!(back, DiagnosticId("d.1.2".into()));
    }

    #[test]
    fn compile_diagnostic_is_camel_case_and_closed() {
        let raw = serde_json::json!({
            "diagnosticId": "x", "code": "workflowDefinitionMissing", "severity": "nonBlocking",
            "locator": {"scopePath": ["232", "242"], "nodeId": "7"},
            "classType": null, "inputName": null, "params": {"classType": "Foo"}
        });
        let d: CompileDiagnostic = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(d.severity, DiagnosticSeverity::NonBlocking);
        assert_eq!(serde_json::to_value(&d).unwrap(), raw);
        let mut snake = raw.clone();
        snake["diagnostic_id"] = serde_json::json!("x");
        assert!(serde_json::from_value::<CompileDiagnostic>(snake).is_err());
    }

    #[test]
    fn advanced_descriptor_round_trips_every_value_type() {
        for (vt, value) in [
            ("int", serde_json::json!(20)),
            ("float", serde_json::json!(7.5)),
            ("boolean", serde_json::json!(true)),
            ("string", serde_json::json!("euler")),
            ("combo", serde_json::json!("model_a.safetensors")),
        ] {
            let raw = serde_json::json!({
                "bindingId": "advanced|3|x", "label": "x", "valueType": vt, "value": value,
                "min": null, "max": null, "step": null, "options": null, "unit": null,
                "source": {"scopePath": [], "nodeId": "3"}, "inputName": "x"
            });
            let d: AdvancedParamDescriptor = serde_json::from_value(raw.clone()).unwrap();
            assert_eq!(serde_json::to_value(&d).unwrap(), raw, "value kept unmodified for {vt}");
        }
    }
}
