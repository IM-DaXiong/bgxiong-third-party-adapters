// @generated from JSON Schema. Do not edit.

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ComfyConfirmationKind {
    #[serde(rename = "automatic")]
    Automatic,
    #[serde(rename = "userConfirmed")]
    UserConfirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ComfyEvidenceKind {
    #[serde(rename = "outputAncestry")]
    OutputAncestry,
    #[serde(rename = "consumerPortType")]
    ConsumerPortType,
    #[serde(rename = "reviewedSemanticRule")]
    ReviewedSemanticRule,
    #[serde(rename = "nameHint")]
    NameHint,
    #[serde(rename = "noteHint")]
    NoteHint,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComfyPromptProjectionOrigin {
    #[serde(rename = "confirmation")]
    pub confirmation: ComfyConfirmationKind,
    #[serde(rename = "evidence")]
    pub evidence: ComfyEvidenceKind,
    #[serde(rename = "sourceDigest")]
    pub source_digest: String,
}
