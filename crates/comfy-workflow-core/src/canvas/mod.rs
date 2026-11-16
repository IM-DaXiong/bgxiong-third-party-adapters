//! Pure ComfyUI canvas compiler: format detection, scoped IR, links and subgraphs, widget
//! binding (L03 `widgets`), compile and provenance. Host code calls `compile::compile` only.

pub mod bridge_evidence;
pub mod compile;
pub mod format;
pub mod ir;
pub mod links;
pub mod provenance;
pub mod resolve;
pub mod subgraph;
pub mod widgets;

pub use compile::{
    bridge_evidence_digest, compile, source_digest, BridgeEvidenceV1, CapabilityPlanV1, CompileContextV1,
    OutputSelection, ResolvedCapability,
};
pub use format::{detect_format, SourceFormat};
pub use ir::{
    parse_to_ir, GraphIrV1, InputValue, IrNode, LinkRef, NodeAttrsV1, NodeKey, NodeMode, OverrideSource, WidgetOverride,
};
pub use provenance::{
    collect_annotation_units, ArtifactDigests, CompiledArtifactV1, InputProvenance, NoteUnitRaw, SourceMapV1,
    ValueOrigin,
};
