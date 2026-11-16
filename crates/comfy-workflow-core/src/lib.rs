#![cfg_attr(target_arch = "wasm32", no_std)]
//! Pure ComfyUI workflow mapping core.
//!
//! Platform-specific by design, but isolated from the generic adapter SDK and desktop host.
//! Inputs and outputs are serializable values; IO and production lifecycle stay in the host.
//! `canvas::compile` is the only workflow compiler; hosts keep thin facades over it.

#[cfg_attr(target_arch = "wasm32", macro_use)]
extern crate alloc;

pub mod analysis;
pub mod apply;
pub mod canvas;
pub mod confirm;
pub mod digest;
pub mod error;
pub mod graph;
pub mod identity;
pub mod media_contracts;
pub mod node_registry;
pub mod prompt_projection;
pub mod reference_image_slots;
pub mod types;

pub mod contract {
    include!("generated/comfy_workflow_mapping_contract.rs");
}

pub use analysis::{analyze_mapping_contract, discover_output_candidates};
pub use apply::apply_mapping_contract;
pub use canvas::{collect_annotation_units, compile as compile_workflow, parse_to_ir};
pub use confirm::{confirm_mapping_contract, MappingResolutionV1};
pub use digest::{digest_json, digest_text, relevant_definition_digest, DefinitionDigest};
pub use error::{MappingError, MappingErrorCode};
pub use graph::{linked_source, WorkflowGraph, WorkflowNode};
pub use identity::{normalize_source_path, stable_workflow_key, validate_identity};
pub use types::*;

/// Generated from the mapping contract; absent on old revisions means provenance is unknown.
pub mod prompt_origin {
    #[cfg(target_arch = "wasm32")]
    use alloc::string::String;
    include!("generated/comfy_prompt_origin.rs");
}
