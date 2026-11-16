//! Model-neutral prompt projection descriptors.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, vec::Vec};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Neutral input modes declared by the application prompt projection schema.
pub const INPUT_MODE_IDS: &[&str] = &[
    "text_to_video",
    "image_to_video",
    "multi_image_to_video",
    "first_last_frame",
    "last_frame_only",
    "reference_to_video",
    "editor_to_video",
];

/// Application media slots that may carry a prompt projection descriptor.
pub const PROMPT_PROJECTION_SLOTS: &[&str] = &["video", "keyframe"];

/// Maximum length of `profileId` and `defaultMethodName`.
pub const PROMPT_PROJECTION_ID_MAX_CHARS: usize = 128;

/// Largest descriptor `version` (schema `maximum`).
pub const PROMPT_PROJECTION_VERSION_MAX: u32 = 2_147_483_647;

/// JSON key of the descriptor on a runtime-caps slot, a manifest model and the host video
/// extensions projection.
pub const PROMPT_PROJECTION_KEY: &str = "promptProjection";

pub fn is_valid_input_mode_id(id: &str) -> bool {
    INPUT_MODE_IDS.contains(&id)
}

pub fn slot_allows_prompt_projection(slot: &str) -> bool {
    PROMPT_PROJECTION_SLOTS.contains(&slot)
}

/// Declared per-model prompt projection for the application workbench.
/// Resolve it through the registered profile directory using the declared identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromptProjectionDescriptor {
    pub profile_id: String,
    pub version: u32,
    pub default_method_name: String,
    pub input_modes: Vec<String>,
}

impl PromptProjectionDescriptor {
    /// Field rules of the schema (closed mode set, id shapes, version range).
    pub fn validate(&self) -> Result<(), String> {
        validate_profile_id(&self.profile_id)?;
        if self.version == 0 || self.version > PROMPT_PROJECTION_VERSION_MAX {
            return Err(format!(
                "promptProjection.version must be 1..={PROMPT_PROJECTION_VERSION_MAX}"
            ));
        }
        validate_method_name(&self.default_method_name)?;
        if self.input_modes.is_empty() {
            return Err("promptProjection.inputModes must not be empty".into());
        }
        for (i, mode) in self.input_modes.iter().enumerate() {
            if !is_valid_input_mode_id(mode) {
                return Err(format!(
                    "promptProjection.inputModes has unknown mode {mode}"
                ));
            }
            if self.input_modes[..i].iter().any(|m| m == mode) {
                return Err(format!("promptProjection.inputModes repeats {mode}"));
            }
        }
        Ok(())
    }

    /// Slot rules: only [`PROMPT_PROJECTION_SLOTS`] carry a descriptor; when the slot declares
    /// `implementedModeIds`, every input mode must be one of them.
    pub fn validate_for_slot(
        &self,
        slot: &str,
        implemented_mode_ids: Option<&[String]>,
    ) -> Result<(), String> {
        self.validate()?;
        if !slot_allows_prompt_projection(slot) {
            return Err(format!("promptProjection is not allowed on slot {slot}"));
        }
        if let Some(implemented) = implemented_mode_ids {
            for mode in &self.input_modes {
                if !implemented.iter().any(|m| m == mode) {
                    return Err(format!(
                        "promptProjection.inputModes {mode} is not in implementedModeIds"
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "profileId": self.profile_id,
            "version": self.version,
            "defaultMethodName": self.default_method_name,
            "inputModes": self.input_modes,
        })
    }
}

fn validate_profile_id(id: &str) -> Result<(), String> {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return Err("promptProjection.profileId must not be empty".into());
    };
    if id.chars().count() > PROMPT_PROJECTION_ID_MAX_CHARS {
        return Err(format!(
            "promptProjection.profileId exceeds {PROMPT_PROJECTION_ID_MAX_CHARS} chars"
        ));
    }
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err("promptProjection.profileId must start with a-z or 0-9".into());
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
    {
        return Err("promptProjection.profileId allows only a-z 0-9 . _ -".into());
    }
    Ok(())
}

fn validate_method_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." {
        return Err("promptProjection.defaultMethodName must be a package name".into());
    }
    if name.chars().count() > PROMPT_PROJECTION_ID_MAX_CHARS {
        return Err(format!(
            "promptProjection.defaultMethodName exceeds {PROMPT_PROJECTION_ID_MAX_CHARS} chars"
        ));
    }
    if name
        .chars()
        .any(|c| c == '/' || c == '\\' || (c as u32) < 0x20 || c == '\u{7f}')
    {
        return Err(
            "promptProjection.defaultMethodName must not contain path separators or control characters"
                .into(),
        );
    }
    Ok(())
}

/// Strict parse of a declared descriptor value (unknown fields, wrong types and rule
/// violations are errors; nothing is coerced).
pub fn parse_prompt_projection(value: &Value) -> Result<PromptProjectionDescriptor, String> {
    let d: PromptProjectionDescriptor =
        serde_json::from_value(value.clone()).map_err(|e| format!("promptProjection: {e}"))?;
    d.validate()?;
    Ok(d)
}

/// Why a model has no usable projection target. Every case is explicit; none falls back to
/// a guessed target or to a generic method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptProjectionUnavailable {
    /// The model's capabilities carry no descriptor (legacy or non-projecting adapter).
    Undeclared,
    /// A descriptor is present but violates the schema.
    Invalid(String),
    /// `profileId@version` is not registered by the host projection directory.
    UnknownProfile { profile_id: String, version: u32 },
}

impl PromptProjectionUnavailable {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Undeclared => "PROMPT_PROJECTION_UNDECLARED",
            Self::Invalid(_) => "PROMPT_PROJECTION_INVALID",
            Self::UnknownProfile { .. } => "PROMPT_PROJECTION_UNKNOWN_PROFILE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptProjectionAvailability {
    Available(PromptProjectionDescriptor),
    Unavailable(PromptProjectionUnavailable),
}

impl PromptProjectionAvailability {
    pub fn descriptor(&self) -> Option<&PromptProjectionDescriptor> {
        match self {
            Self::Available(d) => Some(d),
            Self::Unavailable(_) => None,
        }
    }
}

/// Resolve the declared descriptor of one model against the host projection registry.
/// `declared` is the raw `promptProjection` value (absent or JSON null = undeclared);
/// `is_registered_profile(profile_id, version)` is the host registry lookup and the only
/// source of compatibility. No plugin id, label or node name is consulted.
pub fn resolve_prompt_projection<F>(
    declared: Option<&Value>,
    is_registered_profile: F,
) -> PromptProjectionAvailability
where
    F: Fn(&str, u32) -> bool,
{
    let Some(raw) = declared.filter(|v| !v.is_null()) else {
        return PromptProjectionAvailability::Unavailable(PromptProjectionUnavailable::Undeclared);
    };
    let descriptor = match parse_prompt_projection(raw) {
        Ok(d) => d,
        Err(e) => {
            return PromptProjectionAvailability::Unavailable(PromptProjectionUnavailable::Invalid(
                e,
            ))
        }
    };
    if !is_registered_profile(&descriptor.profile_id, descriptor.version) {
        return PromptProjectionAvailability::Unavailable(
            PromptProjectionUnavailable::UnknownProfile {
                profile_id: descriptor.profile_id,
                version: descriptor.version,
            },
        );
    }
    PromptProjectionAvailability::Available(descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DESCRIPTOR_SCHEMA_JSON: &str = include_str!(
        "../contracts/prompt-projection-capability.schema.json"
    );

    fn platform_a() -> Value {
        json!({
            "profileId": "cloud-async.video-a",
            "version": 1,
            "defaultMethodName": "video-a-prompt-writing",
            "inputModes": ["text_to_video", "image_to_video", "first_last_frame"]
        })
    }

    fn platform_b() -> Value {
        json!({
            "profileId": "lan-workflow.video-b",
            "version": 3,
            "defaultMethodName": "video-b-prompt-writing",
            "inputModes": ["multi_image_to_video", "reference_to_video"]
        })
    }

    #[test]
    fn descriptor_matches_application_schema() {
        let schema: Value = serde_json::from_str(DESCRIPTOR_SCHEMA_JSON).unwrap();
        let props = &schema["properties"];
        assert_eq!(props["inputModes"]["items"]["enum"], json!(INPUT_MODE_IDS));
        assert_eq!(
            props["profileId"]["maxLength"],
            json!(PROMPT_PROJECTION_ID_MAX_CHARS)
        );
        assert_eq!(
            props["version"]["maximum"],
            json!(PROMPT_PROJECTION_VERSION_MAX)
        );
        for example in schema["examples"].as_array().unwrap() {
            parse_prompt_projection(example).unwrap();
        }
    }

    #[test]
    fn two_unrelated_platforms_parse_to_the_same_shape() {
        let a = parse_prompt_projection(&platform_a()).unwrap();
        let b = parse_prompt_projection(&platform_b()).unwrap();
        assert_eq!(a.to_value(), platform_a());
        assert_eq!(b.version, 3);
        let modes_a: Vec<String> = ["text_to_video", "image_to_video", "first_last_frame"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        a.validate_for_slot("video", Some(modes_a.as_slice()))
            .unwrap();
        b.validate_for_slot("keyframe", None).unwrap();
    }

    #[test]
    fn invalid_descriptors_are_rejected_not_coerced() {
        let mut extra = platform_a();
        extra["nodeHint"] = json!("x");
        assert!(parse_prompt_projection(&extra).is_err());
        let mut float_version = platform_a();
        float_version["version"] = json!(1.0);
        assert!(parse_prompt_projection(&float_version).is_err());
        let mut bad_mode = platform_a();
        bad_mode["inputModes"] = json!(["video_to_video"]);
        assert!(parse_prompt_projection(&bad_mode).is_err());
        let mut dup = platform_a();
        dup["inputModes"] = json!(["text_to_video", "text_to_video"]);
        assert!(parse_prompt_projection(&dup).is_err());
        let mut upper = platform_a();
        upper["profileId"] = json!("Cloud/Video");
        assert!(parse_prompt_projection(&upper).is_err());
        let mut dots = platform_a();
        dots["defaultMethodName"] = json!("..");
        assert!(parse_prompt_projection(&dots).is_err());
    }

    #[test]
    fn slot_rules() {
        let a = parse_prompt_projection(&platform_a()).unwrap();
        assert!(a.validate_for_slot("image", None).is_err());
        let only_t2v = vec!["text_to_video".to_string()];
        assert!(a
            .validate_for_slot("video", Some(only_t2v.as_slice()))
            .is_err());
    }

    #[test]
    fn availability_is_explicit_for_missing_invalid_and_unknown() {
        let registry = |id: &str, version: u32| id == "cloud-async.video-a" && version == 1;
        assert_eq!(
            resolve_prompt_projection(None, registry),
            PromptProjectionAvailability::Unavailable(PromptProjectionUnavailable::Undeclared)
        );
        assert_eq!(
            resolve_prompt_projection(Some(&Value::Null), registry),
            PromptProjectionAvailability::Unavailable(PromptProjectionUnavailable::Undeclared)
        );
        let invalid = resolve_prompt_projection(Some(&json!({"profileId": "x"})), registry);
        assert!(matches!(
            invalid,
            PromptProjectionAvailability::Unavailable(PromptProjectionUnavailable::Invalid(_))
        ));
        let unknown = resolve_prompt_projection(Some(&platform_b()), registry);
        assert_eq!(
            unknown,
            PromptProjectionAvailability::Unavailable(
                PromptProjectionUnavailable::UnknownProfile {
                    profile_id: "lan-workflow.video-b".into(),
                    version: 3
                }
            )
        );
        let mut newer = platform_a();
        newer["version"] = json!(2);
        assert!(resolve_prompt_projection(Some(&newer), registry)
            .descriptor()
            .is_none());
        let ok = resolve_prompt_projection(Some(&platform_a()), registry);
        assert_eq!(ok.descriptor().unwrap().profile_id, "cloud-async.video-a");
    }
}
