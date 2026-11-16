//! Versioned Comfy node knowledge used by mapping analysis and canvas compile.
//!
//! - `definition`: normalized `/object_info` snapshots (`DefinitionSnapshotV1`).
//! - `serializer_profile`: the verified standard frontend widget serialization profile.
//! - `special`: package/version-scoped special serialization rules; the only source of
//!   non-standard widget layouts is `special::special_rule_for`, and its `RULE_REGISTRY_VERSION`
//!   is the rule-set version stamped into contracts.
//! - `widgets`: the pre-CW28 legacy audited layout table, used only by the host's legacy
//!   `compiler.rs`/`normalizer.rs` until CW28 switches them to `canvas::compile`. New node
//!   layouts go to `special.rs`, never to that table.
//!
//! Application orchestration and generic workbenches must not grow class-type branches.

pub mod definition;
pub mod serializer_profile;
pub mod special;
pub mod widgets;

pub use special::RULE_REGISTRY_VERSION;
pub use widgets::{
    merge_widgets_values_into_inputs, overlay_widget_field, widget_layout, ComboOption,
    DynamicComboLayout, WidgetLayout, WidgetSlot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    TextEncode,
    Sampler,
    ConditioningPassthrough,
    ConditioningSplit,
    ImageOutput,
    VideoOutput,
    Scalar,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeSemantics {
    pub role: NodeRole,
    pub seed_field: Option<&'static str>,
}

pub fn semantics_for(class_type: &str) -> NodeSemantics {
    let role = match class_type {
        "CLIPTextEncode" => NodeRole::TextEncode,
        "KSampler" | "KSamplerAdvanced" => NodeRole::Sampler,
        "ReferenceLatent" | "FluxGuidance" => NodeRole::ConditioningPassthrough,
        "LTXVConditioning" | "LTXVCropGuides" | "LTXVAddGuide" => NodeRole::ConditioningSplit,
        "SaveImage" | "SaveImageAdvanced" | "PreviewImage" => NodeRole::ImageOutput,
        "VHS_VideoCombine" | "SaveVideo" | "VHS_SaveVideo" | "SaveMP4" | "VideoSave"
        | "ExportVideo" => NodeRole::VideoOutput,
        "PrimitiveInt"
        | "PrimitiveFloat"
        | "PrimitiveString"
        | "PrimitiveStringMultiline"
        | "PrimitiveBoolean" => NodeRole::Scalar,
        _ => NodeRole::Unknown,
    };
    let seed_field = match class_type {
        "KSampler" => Some("seed"),
        "KSamplerAdvanced" => Some("noise_seed"),
        "RandomNoise" => Some("noise_seed"),
        _ => None,
    };
    NodeSemantics { role, seed_field }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_nodes_do_not_inherit_guessed_semantics() {
        let semantics = semantics_for("ThirdPartyMagicSampler");
        assert_eq!(semantics.role, NodeRole::Unknown);
        assert_eq!(semantics.seed_field, None);
        assert!(widget_layout("ThirdPartyMagicSampler").is_none());
    }

    #[test]
    fn versioned_special_rules_are_not_duplicated_in_the_legacy_table() {
        for rule in special::all_rules() {
            assert!(
                widget_layout(rule.class_type).is_none(),
                "{} has a versioned special rule; it must not also live in the legacy table",
                rule.class_type
            );
        }
    }
}
