use alloc::{format, string::String, vec, vec::Vec};
use serde_json::{json, Value};

pub(crate) const PLUGIN: &str = "local.bgx.minimax-media";
pub(crate) const MODELS: &[&str] = &["image-01", "image-01-live"];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageOperation {
    Text,
    Character,
}

impl ImageOperation {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Text => "image-text",
            Self::Character => "image-character",
        }
    }

    pub(crate) fn id(self) -> String {
        format!("{PLUGIN}/{}", self.key())
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Text => "MiniMax 文生图",
            Self::Character => "MiniMax 人像参考生图",
        }
    }

    pub(crate) fn reference_count(self) -> usize {
        usize::from(self == Self::Character)
    }

    pub(crate) fn input_schema(self) -> Value {
        let count = self.reference_count();
        json!({"type":"object","additionalProperties":false,"required":["prompt","subjectImages"],"properties":{
            "prompt":{"type":"string","minLength":1,"maxLength":1500},
            "subjectImages":{"type":"array","items":{"type":"string","minLength":1},"minItems":count,"maxItems":count},
            "aspectRatio":{"type":"string","enum":["1:1","16:9","4:3","3:2","2:3","3:4","9:16","21:9"]},
            "seed":{"type":"integer"},"responseFormat":{"type":"string","enum":["base64","url"],"default":"base64"}
        }})
    }

    pub(crate) fn controls(self) -> Vec<Value> {
        let mut controls = vec![
            control("/prompt", "multiline", "提示词", Some("prompt")),
            control("/aspectRatio", "select", "画幅", Some("aspectRatio")),
            control("/seed", "number", "随机种子", None),
            control("/responseFormat", "select", "响应格式", None),
        ];
        if self == Self::Character {
            controls.push(control("/subjectImages", "media", "人物主体参考图", None));
        }
        controls
    }
}

fn control(pointer: &str, widget: &str, label: &str, role: Option<&str>) -> Value {
    let mut control =
        json!({"fieldPointer":pointer,"widget":widget,"label":label,"group":"","description":""});
    if let Some(role) = role {
        control["displayRole"] = json!(role);
    }
    control
}

pub(crate) fn operation(selection: &Value) -> Option<ImageOperation> {
    if selection["capabilityKey"] != "image.generate"
        || selection["operationVersion"] != 1
        || !selection["modelId"]
            .as_str()
            .is_some_and(|model| MODELS.contains(&model))
    {
        return None;
    }
    [ImageOperation::Text, ImageOperation::Character]
        .into_iter()
        .find(|operation| selection["operationId"] == operation.id())
}

pub(crate) fn material(
    selection: &Value,
    input: &Value,
) -> Result<Value, (&'static str, &'static str)> {
    let op = operation(selection).ok_or(("", "MiniMax 图片操作身份无效"))?;
    let prompt = input["prompt"]
        .as_str()
        .filter(|prompt| !prompt.trim().is_empty())
        .ok_or(("/prompt", "请输入提示词"))?;
    if input["subjectImages"].as_array().map(Vec::len) != Some(op.reference_count()) {
        return Err(("/subjectImages", "人物参考图数量与所选操作不符"));
    }
    if selection["modelId"] == "image-01-live" && input["aspectRatio"] == "21:9" {
        return Err(("/aspectRatio", "image-01-live 不支持 21:9"));
    }
    let mut body = json!({"model":selection["modelId"],"prompt":prompt,"n":1,"prompt_optimizer":false,
        "response_format":input["responseFormat"].as_str().unwrap_or("base64")});
    if let Some(value) = input.get("aspectRatio") {
        body["aspect_ratio"] = value.clone();
    }
    if let Some(value) = input.get("seed") {
        body["seed"] = value.clone();
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_uses_the_selected_model_and_exact_character_count() {
        let mut selection = json!({"operationId":ImageOperation::Character.id(),"operationVersion":1,
            "capabilityKey":"image.generate","modelId":"image-01-live"});
        let mut input = json!({"prompt":"portrait","subjectImages":[],"aspectRatio":"21:9"});
        assert_eq!(
            material(&selection, &input).unwrap_err().0,
            "/subjectImages"
        );
        input["subjectImages"] = json!(["portrait"]);
        assert_eq!(material(&selection, &input).unwrap_err().0, "/aspectRatio");
        selection["modelId"] = json!("image-01");
        let prepared = material(&selection, &input).unwrap();
        assert_eq!(prepared["aspect_ratio"], "21:9");
        assert_eq!(prepared["prompt_optimizer"], false);
    }
}
