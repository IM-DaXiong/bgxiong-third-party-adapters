// @generated from JSON Schema. Do not edit.

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ComfyResolutionPathStepTransform {
    #[serde(rename = "source")]
    Source,
    #[serde(rename = "identity")]
    Identity,
    #[serde(rename = "half")]
    Half,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComfyResolutionPathStep {
    #[serde(rename = "inputName")]
    pub input_name: String,
    #[serde(rename = "nodeId")]
    pub node_id: String,
    #[serde(rename = "outputIndex")]
    pub output_index: u64,
    #[serde(rename = "transform")]
    pub transform: ComfyResolutionPathStepTransform,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComfyResolutionSelectorProof {
    #[serde(rename = "definitionDigest")]
    pub definition_digest: String,
    #[serde(rename = "heightPath")]
    pub height_path: Vec<ComfyResolutionPathStep>,
    #[serde(rename = "selectorNodeId")]
    pub selector_node_id: String,
    #[serde(rename = "sourceGraphDigest")]
    pub source_graph_digest: String,
    #[serde(rename = "terminalNodeId")]
    pub terminal_node_id: String,
    #[serde(rename = "widthPath")]
    pub width_path: Vec<ComfyResolutionPathStep>,
}
