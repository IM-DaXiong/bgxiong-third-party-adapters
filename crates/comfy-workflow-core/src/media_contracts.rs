//! Typed ComfyUI media sub-contracts embedded in `MappingContractV1`.
//!
//! Timing (duration + fps, INV-COMFY-TIMING) and resolution (INV-COMFY-RES) contracts are proposed,
//! confirmed and applied by the platform rule modules in `src/comfyui/workflow/{duration_slots,
//! resolution_slots}.rs`; this crate owns only their data shape so the mapping contract can freeze
//! them. Serialized shape is unchanged from the former host definitions (IPC / caps.json stable).

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, vec::Vec};

use serde::{Deserialize, Serialize};
mod video_resolution {
    #[cfg(target_arch = "wasm32")]
    use alloc::string::String;
    include!("generated/video_resolution.rs");
}
pub use video_resolution::{VideoResolutionSelection, VideoResolutionValue, VideoResolutionValueCaps};
mod selector_proof_types {
    #[cfg(target_arch = "wasm32")]
    use alloc::{string::String, vec::Vec};
    include!("generated/comfy_resolution_proof.rs");
}
pub use selector_proof_types::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DurationMode {
    UiInjectable,
    WorkflowFixed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DurationTargetUnit {
    Frames,
    Seconds,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DurationTarget {
    pub node_id: String,
    pub field: String,
    pub unit: DurationTargetUnit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FpsSource {
    WorkflowSlot,
    Explicit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FpsMode {
    UiInjectable,
    WorkflowFixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FpsRangeContract {
    pub min_fps: u32,
    pub max_fps: u32,
    pub step_fps: u32,
    pub default_fps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IntegerSlotRangeContract {
    pub min: u32,
    pub max: u32,
    pub step: u32,
    pub default_value: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ContractConfirmation {
    #[default]
    Unconfirmed,
    Confirmed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DurationFpsContract {
    pub source: FpsSource,
    /// v2：fps 控件模式；缺省按 WorkflowFixed（兼容 v1 caps）。
    #[serde(default = "default_fps_mode_workflow_fixed")]
    pub mode: FpsMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub value: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<FpsRangeContract>,
}

fn default_fps_mode_workflow_fixed() -> FpsMode {
    FpsMode::WorkflowFixed
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DurationRangeContract {
    pub min_seconds: u32,
    pub max_seconds: u32,
    pub step_seconds: u32,
    pub default_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DurationContract {
    pub version: u32,
    pub mode: DurationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<DurationFpsContract>,
    #[serde(default)]
    pub targets: Vec<DurationTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<DurationRangeContract>,
    /// frame-based timing：object_info 帧数约束（供 fps 联动）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_range: Option<IntegerSlotRangeContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_info_fingerprint: Option<String>,
    #[serde(default)]
    pub confirmation: ContractConfirmation,
    /// Optional per-workflow frame budget (hand-declared in caps; never proposed).
    /// `max_frames` is the only hard frame cap; the others are UI soft-warning thresholds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_budget: Option<FrameBudgetContract>,
}

/// INV-COMFY-TIMING frame budget: soft thresholds + optional hard `maxFrames` (caps may only narrow).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FrameBudgetContract {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_max_fps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warn_frames: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_frames: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ResolutionMode {
    Selectable,
    WorkflowNative,
    /// 写 ResolutionSelector.aspect_ratio (+ megapixels)；不写 consumer WxH。
    SelectorAspect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionTarget {
    pub node_id: String,
    pub width_field: String,
    pub height_field: String,
    /// LTX PrimitiveInt 链：宽/高分属不同节点时写入（apply 分别 patch）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_node_id: Option<String>,
}

/// ResolutionSelector 写入目标（mode=SelectorAspect）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionSelectorTarget {
    pub node_id: String,
    pub aspect_field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub megapixels_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiple_field: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionOptionContract {
    pub id: String,
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub aspect_ratios: Vec<String>,
    #[serde(default)]
    pub is_default: bool,
}

/// SelectorAspect 画幅选项（与 Selectable WxH options 分列，降低 FE 回归面）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectorAspectOption {
    pub id: String,
    pub label: String,
    /// 完整 combo，如 "16:9 (Widescreen)"
    pub aspect_combo: String,
    /// UI 画幅 id，如 "16:9"
    pub aspect_ratio_ui: String,
    #[serde(default)]
    pub is_default: bool,
}

/// SelectorAspect megapixels 产品档。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectorMegapixelsOption {
    pub id: String,
    pub label: String,
    pub megapixels: f64,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionContract {
    pub version: u32,
    pub mode: ResolutionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ResolutionTarget>,
    /// Selectable：INT WxH 选项。
    #[serde(default)]
    pub options: Vec<ResolutionOptionContract>,
    /// SelectorAspect：写入目标。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector_target: Option<ResolutionSelectorTarget>,
    /// SelectorAspect：画幅选项（每档一条；非 aspect×mp 笛卡尔）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selector_aspect_options: Vec<SelectorAspectOption>,
    /// SelectorAspect：megapixels 产品档。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selector_megapixels_options: Vec<SelectorMegapixelsOption>,
    /// 已证明的数值合法域；旧记录缺项不意味着支持自定义。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_value_caps: Option<VideoResolutionValueCaps>,
    /// 两轴来自同一Selector及当前节点定义的来源证明。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector_proof: Option<ComfyResolutionSelectorProof>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_info_fingerprint: Option<String>,
    #[serde(default)]
    pub confirmation: ContractConfirmation,
}
