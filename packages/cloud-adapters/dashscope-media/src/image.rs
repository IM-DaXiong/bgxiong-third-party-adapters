use alloc::{format, string::ToString, vec, vec::Vec};
use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::Host;
use serde_json::{json, Value};

use super::{
    download, exchange, media_bytes, media_output, put_media, schema, Failure, CONFIG, PLUGIN,
};

const GENERATE_PATH: &str = "/api/v1/services/aigc/multimodal-generation/generation";
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const QWEN_MODELS: &[&str] = &["qwen-image-3.0", "qwen-image-3.0-pro"];
const OUTPUT_SCHEMA: &str = r#"{"type":"object","additionalProperties":false,"required":["model","providerRequestId","imageCount"],"properties":{"model":{"type":"string","minLength":1},"providerRequestId":{"type":["string","null"]},"imageCount":{"type":"integer","const":1}}}"#;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    QwenText,
    QwenEdit,
    QwenEditPlus,
    ZImageText,
}

const OPERATIONS: [Operation; 4] = [
    Operation::QwenText,
    Operation::QwenEdit,
    Operation::QwenEditPlus,
    Operation::ZImageText,
];

impl Operation {
    fn key(self) -> &'static str {
        match self {
            Self::QwenText => "image-text",
            Self::QwenEdit => "image-edit",
            Self::QwenEditPlus => "image-edit-plus",
            Self::ZImageText => "image-z-text",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::QwenText => "千问图像 3.0 文生图",
            Self::QwenEdit => "千问图像 3.0 图像编辑",
            Self::QwenEditPlus => "千问图像编辑 Plus",
            Self::ZImageText => "Z-Image Turbo 文生图",
        }
    }

    fn models(self) -> &'static [&'static str] {
        match self {
            Self::QwenText | Self::QwenEdit => QWEN_MODELS,
            Self::QwenEditPlus => &["qwen-image-edit-plus"],
            Self::ZImageText => &["z-image-turbo"],
        }
    }

    fn accepts_references(self) -> bool {
        matches!(self, Self::QwenEdit | Self::QwenEditPlus)
    }

    fn uses_extend_modes(self) -> bool {
        matches!(self, Self::QwenText | Self::QwenEdit)
    }

    fn extend_by_default(self) -> bool {
        self != Self::ZImageText
    }

    fn input_schema(self) -> Value {
        let mut input = json!({"type": "object", "additionalProperties": false, "required": ["prompt"], "properties": {
            "prompt": {"type": "string", "minLength": 1},
            "size": {"type": "string", "pattern": "^(auto|[1-9][0-9]{0,4}[*xX][1-9][0-9]{0,4})$"},
            "seed": {"type": "integer", "minimum": 0, "maximum": 2147483647},
            "promptExtend": {"type": "boolean", "default": self.extend_by_default()}
        }});
        let properties = &mut input["properties"];
        if self == Self::ZImageText {
            properties["prompt"]["maxLength"] = json!(800);
            properties["size"]["default"] = json!("1024*1536");
        } else {
            properties["negativePrompt"] = json!({"type": "string", "maxLength": 500});
            properties["watermark"] = json!({"type": "boolean", "default": false});
        }
        if self == Self::QwenEditPlus {
            properties["size"]["default"] = json!("2048*2048");
        }
        if self.uses_extend_modes() {
            let modes: &[&str] = if self == Self::QwenText {
                &["direct", "agent"]
            } else {
                &["direct"]
            };
            properties["promptExtendMode"] =
                json!({"type": "string", "enum": modes, "default": "direct"});
        }
        if self.accepts_references() {
            properties["referenceImages"] = json!({"type": "array", "minItems": 1, "maxItems": 3,
                "items": {"type": "string", "minLength": 1}});
            input["required"] = json!(["prompt", "referenceImages"]);
        }
        input
    }

    fn controls(self) -> Vec<Value> {
        let mut controls = vec![
            control("/prompt", "multiline", "提示词", Some("prompt")),
            control("/size", "text", "尺寸", None),
            control("/seed", "number", "随机种子", None),
            control("/promptExtend", "toggle", "提示词改写", None),
        ];
        if self != Self::ZImageText {
            controls.push(control("/negativePrompt", "multiline", "反向提示词", None));
            controls.push(control("/watermark", "toggle", "水印", None));
        }
        if self.uses_extend_modes() {
            controls.push(control("/promptExtendMode", "select", "改写方式", None));
        }
        if self.accepts_references() {
            controls.push(control("/referenceImages", "media", "参考图", None));
        }
        controls
    }

    fn capability(self) -> Value {
        let configuration = schema("configuration", CONFIG);
        let input = schema("input", &self.input_schema().to_string());
        let output = schema("output", OUTPUT_SCHEMA);
        let slots = if self.accepts_references() {
            vec![
                json!({"inputPointer": "/referenceImages", "role": "reference-image",
                "mediaKind": "image", "minCount": 1, "maxCount": 3}),
            ]
        } else {
            Vec::new()
        };
        json!({"schemaVersion": 2, "localId": self.key(), "capabilityKey": "image.generate",
            "transportSlot": "image", "modelIds": self.models(),
            "configurationSchema": {"id": "configuration", "digest": configuration["digest"]},
            "schemas": [configuration, input, output],
            "operations": [{"id": format!("{PLUGIN}/{}", self.key()), "version": 1,
                "inputProfile": "operation-input-v1", "label": self.label(),
                "inputSchema": {"id": "input", "digest": input["digest"]},
                "outputSchema": {"id": "output", "digest": output["digest"]}, "resumeSchema": null,
                "uiSchema": {"controls": self.controls()}, "mediaSlots": slots,
                "outputSlots": [{"role": "images", "kind": "image", "minCount": 1, "maxCount": 1},
                    {"role": "provider-response", "kind": "file", "minCount": 1, "maxCount": 1}],
                "execution": {"mode": "synchronous", "streaming": false, "cancellation": "unsupported",
                    "resumable": false, "remoteIdempotency": false}}]})
    }
}

fn control(pointer: &str, widget: &str, label: &str, role: Option<&str>) -> Value {
    let mut value = json!({"fieldPointer": pointer, "widget": widget, "label": label, "group": "", "description": ""});
    if let Some(role) = role {
        value["displayRole"] = json!(role);
    }
    value
}

pub(crate) fn capabilities() -> Vec<Value> {
    OPERATIONS.into_iter().map(Operation::capability).collect()
}

fn operation(selection: &Value) -> Option<Operation> {
    if selection["capabilityKey"] != "image.generate" || selection["operationVersion"] != 1 {
        return None;
    }
    let id = selection["operationId"]
        .as_str()?
        .strip_prefix(PLUGIN)?
        .strip_prefix('/')?;
    let model = selection["modelId"].as_str()?;
    OPERATIONS
        .into_iter()
        .find(|op| op.key() == id && op.models().contains(&model))
}

pub(crate) fn material(
    selection: &Value,
    input: &Value,
) -> Result<Value, (&'static str, &'static str)> {
    let op = operation(selection).ok_or(("", "百炼图像操作身份无效"))?;
    let prompt = input["prompt"]
        .as_str()
        .filter(|prompt| !prompt.trim().is_empty())
        .ok_or(("/prompt", "请输入提示词"))?;
    let references = input["referenceImages"].as_array().map_or(0, Vec::len);
    if (op.accepts_references() && !(1..=3).contains(&references))
        || (!op.accepts_references() && references != 0)
    {
        return Err(("/referenceImages", "参考图片数量不符合操作声明"));
    }
    let mut parameters = json!({"prompt_extend": input["promptExtend"].as_bool().unwrap_or_else(|| op.extend_by_default())});
    match input["size"].as_str().unwrap_or("auto") {
        "auto" if op == Operation::ZImageText => parameters["size"] = json!("1024*1536"),
        "auto" if op == Operation::QwenEditPlus => parameters["size"] = json!("2048*2048"),
        "auto" => (),
        size => {
            let (width, height) = size
                .split_once(['*', 'x', 'X'])
                .and_then(|(width, height)| {
                    Some((width.parse::<u64>().ok()?, height.parse::<u64>().ok()?))
                })
                .filter(|(width, height)| {
                    width
                        .checked_mul(*height)
                        .is_some_and(|area| (512 * 512..=2048 * 2048).contains(&area))
                })
                .ok_or(("/size", "尺寸须在百炼支持的像素面积范围内"))?;
            if op != Operation::ZImageText && width.max(height) > 8 * width.min(height) {
                return Err(("/size", "千问图像尺寸的长短边比不能超过 8:1"));
            }
            parameters["size"] = json!(format!("{width}*{height}"));
        }
    }
    if let Some(seed) = input.get("seed") {
        parameters["seed"] = seed.clone();
    }
    if op != Operation::ZImageText {
        parameters["n"] = json!(1);
        parameters["watermark"] = json!(input["watermark"].as_bool().unwrap_or(false));
        if let Some(prompt) = input["negativePrompt"]
            .as_str()
            .filter(|prompt| !prompt.trim().is_empty())
        {
            parameters["negative_prompt"] = json!(prompt);
        }
    }
    if let Some(mode) = input["promptExtendMode"].as_str() {
        if !op.uses_extend_modes()
            || (mode != "direct" && (mode != "agent" || op != Operation::QwenText))
        {
            return Err(("/promptExtendMode", "所选操作不支持该提示词改写方式"));
        }
        parameters["prompt_extend_mode"] = json!(mode);
    }
    Ok(
        json!({"model": selection["modelId"], "input": {"messages": [{"role": "user", "content": [{"text": prompt}]}]},
        "parameters": parameters}),
    )
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if bytes.starts_with(b"II\x2a\x00") || bytes.starts_with(b"MM\x00\x2a") {
        return Some("image/tiff");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    None
}

fn usage(value: &Value) -> Value {
    let reported = &value["usage"];
    let measurements: Vec<_> = [
        ("input_tokens", "input_tokens", "tokens"),
        ("output_tokens", "output_tokens", "tokens"),
        ("total_tokens", "total_tokens", "tokens"),
        (
            "input_image_count",
            "local.bgx.dashscope-media/input_image_count",
            "local.bgx.dashscope-media/images",
        ),
        (
            if reported.get("output_image_count").is_some() {
                "output_image_count"
            } else {
                "image_count"
            },
            "local.bgx.dashscope-media/output_image_count",
            "local.bgx.dashscope-media/images",
        ),
    ]
    .into_iter()
    .filter_map(|(field, metric, unit)| {
        reported[field].as_u64().map(|value| {
            json!({"metric": metric, "unit": unit,
            "amount": value.to_string(), "source": "reported"})
        })
    })
    .collect();
    if measurements.is_empty() {
        Value::Null
    } else {
        json!({"measurements": measurements})
    }
}

pub(crate) fn submit(host: &mut dyn Host, payload: &Value, budget: u64) -> Result<Value, Failure> {
    let input = &payload["input"];
    let mut body =
        material(payload, input).map_err(|(_, message)| Failure::new("INVALID_INPUT", message))?;
    if payload["preparation"]["material"] != body {
        return Err(Failure::new(
            "INVALID_INPUT",
            "百炼图像请求与冻结预检不一致",
        ));
    }
    if let Some(references) = input["referenceImages"].as_array() {
        let mut content = Vec::with_capacity(references.len() + 1);
        for reference in references {
            let handle = payload["media"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|item| item["id"] == *reference && item["kind"] == "image")
                .and_then(|item| item["handle"].as_str())
                .ok_or_else(|| Failure::new("INVALID_INPUT", "缺少百炼参考图片资源"))?;
            let bytes = media_bytes(host, handle, MAX_IMAGE_BYTES)?;
            let mime = image_mime(&bytes)
                .ok_or_else(|| Failure::new("INVALID_INPUT", "百炼不支持该参考图格式"))?;
            content
                .push(json!({"image": format!("data:{mime};base64,{}", STANDARD.encode(bytes))}));
        }
        let original = &mut body["input"]["messages"][0]["content"];
        content.push(original[0].take());
        *original = json!(content);
    }
    let (response, raw) = exchange(
        host,
        payload,
        GENERATE_PATH,
        "POST",
        Some(&body),
        json!([]),
        budget,
    )?;
    let request_id = response["request_id"].as_str();
    let result = (|| {
        let mut images = response["output"]["choices"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|choice| {
                choice["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .filter(|item| item.get("image").is_some() || item.get("image_base64").is_some());
        let image = images
            .next()
            .filter(|_| images.next().is_none())
            .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "百炼返回的图像数量与请求不符"))?;
        let artifact = if let Some(url) = image["image"].as_str() {
            download(host, url, "images", budget)?
        } else if let Some(encoded) = image["image_base64"].as_str() {
            if encoded.len() > 40 * 1024 * 1024 {
                return Err(Failure::new(
                    "RESOURCE_UNAVAILABLE",
                    "百炼返回的内联图片超出大小限制",
                ));
            }
            let bytes = STANDARD
                .decode(encoded)
                .map_err(|_| Failure::new("UPSTREAM_ERROR", "百炼内联图片编码无效"))?;
            let mime = image_mime(&bytes)
                .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "百炼内联图片格式无效"))?;
            let handle = put_media(host, "image", mime, &bytes)?;
            media_output(host, &handle, "images")?
        } else {
            return Err(Failure::new("UPSTREAM_ERROR", "百炼图片响应无可下载内容"));
        };
        Ok(
            json!({"kind": "completed", "vendorTaskId": null, "appliedParams": null,
            "outputs": [artifact, media_output(host, &raw, "provider-response")?],
            "data": {"model": body["model"], "providerRequestId": request_id, "imageCount": 1}, "usage": usage(&response)}),
        )
    })();
    result.map_err(|mut error: Failure| {
        if error.vendor_request.is_none() {
            error.vendor_request = request_id.map(ToString::to_string);
        }
        error
    })
}

#[cfg(test)]
mod provider_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(op: Operation, model: &str) -> Value {
        json!({"capabilityKey": "image.generate", "operationVersion": 1,
            "operationId": format!("{PLUGIN}/{}", op.key()), "modelId": model})
    }

    #[test]
    fn retired_models_are_rejected_and_text_defaults_match_each_family() {
        let input = json!({"prompt": "a small red mug"});
        for retired in [
            "wan2.7-image",
            "wan2.7-image-pro",
            "qwen-image-2.0",
            "qwen-image-2.0-pro",
        ] {
            for op in OPERATIONS {
                assert!(operation(&selection(op, retired)).is_none());
            }
        }
        let z = material(&selection(Operation::ZImageText, "z-image-turbo"), &input).unwrap();
        assert_eq!(
            z["parameters"],
            json!({"size": "1024*1536", "prompt_extend": false})
        );
        let qwen = material(
            &selection(Operation::QwenText, "qwen-image-3.0-pro"),
            &input,
        )
        .unwrap();
        assert_eq!(
            qwen["parameters"],
            json!({"n": 1, "prompt_extend": true, "watermark": false})
        );
    }

    #[test]
    fn reference_edit_keeps_count_range_and_rejects_agent_or_invalid_resolution() {
        let identity = selection(Operation::QwenEdit, "qwen-image-3.0");
        let mut input = json!({"prompt": "change the mug to blue", "referenceImages": ["mug"],
            "size": "512x512", "promptExtend": false});
        let body = material(&identity, &input).unwrap();
        assert_eq!(body["parameters"]["size"], "512*512");
        assert_eq!(body["parameters"]["prompt_extend"], false);
        input["promptExtendMode"] = json!("agent");
        assert_eq!(
            material(&identity, &input).unwrap_err().0,
            "/promptExtendMode"
        );
        input["size"] = json!("16384*32");
        assert_eq!(material(&identity, &input).unwrap_err().0, "/size");
        input["referenceImages"] = json!([]);
        assert_eq!(
            material(&identity, &input).unwrap_err().0,
            "/referenceImages"
        );
    }
}
