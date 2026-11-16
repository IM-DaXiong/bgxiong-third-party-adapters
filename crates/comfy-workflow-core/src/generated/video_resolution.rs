// @generated from JSON Schema. Do not edit.

fn deserialize_optional_deny_null<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where D: serde::Deserializer<'de>, T: serde::Deserialize<'de> {
    match <Option<T> as serde::Deserialize>::deserialize(d)? {
        Some(value) => Ok(Some(value)),
        None => Err(serde::de::Error::custom("null is refused; omit the key")),
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum VideoResolutionValue {
    #[serde(rename = "megapixels")]
    Megapixels {
    #[serde(rename = "value")]
    value: f64,
    },
}

fn deserialize_video_resolution_value_caps_kind<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let value = <String as serde::Deserialize>::deserialize(d)?;
    if value != "megapixels" { return Err(serde::de::Error::custom("schema const mismatch")); }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoResolutionValueCaps {
    #[serde(rename = "kind")]
    #[serde(deserialize_with = "deserialize_video_resolution_value_caps_kind")]
    pub kind: String,
    #[serde(rename = "max")]
    pub max: f64,
    #[serde(rename = "min")]
    pub min: f64,
    #[serde(rename = "multipleOf")]
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_optional_deny_null")]
    pub multiple_of: Option<f64>,
    #[serde(rename = "step")]
    pub step: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoResolutionSelection {
    #[serde(rename = "resolution")]
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_optional_deny_null")]
    pub resolution: Option<String>,
    #[serde(rename = "resolutionValue")]
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_optional_deny_null")]
    pub resolution_value: Option<VideoResolutionValue>,
}
