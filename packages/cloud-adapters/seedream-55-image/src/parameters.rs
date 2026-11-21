use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use serde_json::{json, Value};

use crate::{EDIT, GENERATE, MODEL, OP_VERSION, PLUGIN};

#[derive(Debug)]
pub(crate) struct Invalid(pub &'static str, pub &'static str);

pub(crate) struct Parameters<'a> {
    pub prompt: &'a str,
    pub size: String,
    pub response_format: &'a str,
    pub output_format: &'a str,
    pub background: &'a str,
    pub watermark: bool,
    pub references: Vec<&'a str>,
}

fn choice<'a>(
    input: &'a Value,
    key: &str,
    allowed: &[&'static str],
    diagnostic: Invalid,
) -> Result<&'a str, Invalid> {
    match input.get(key) {
        None => Ok(allowed[0]),
        Some(value) => value
            .as_str()
            .filter(|s| allowed.contains(s))
            .ok_or(diagnostic),
    }
}

pub(crate) fn dimensions_valid(size: &str) -> bool {
    if matches!(size, "1K" | "1.5K" | "2K") {
        return true;
    }
    let parse = |value: &str| {
        if value.starts_with('0') || value.len() > 8 || !value.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        value.parse::<u64>().ok()
    };
    let Some((width, height)) = size.split_once('x') else {
        return false;
    };
    let (Some(w), Some(h)) = (parse(width), parse(height)) else {
        return false;
    };
    (921_600..=4_624_220).contains(&(w * h)) && w <= 16 * h && h <= 16 * w
}

impl<'a> Parameters<'a> {
    pub fn parse(selection: &Value, input: &'a Value) -> Result<Self, Invalid> {
        let edit = selection["operationId"] == EDIT;
        if selection["modelId"] != MODEL
            || selection["operationVersion"] != OP_VERSION
            || selection["capabilityKey"] != "image.generate"
            || (!edit && selection["operationId"] != GENERATE)
        {
            return Err(Invalid("", "操作或模型已变化"));
        }
        let fields = input.as_object().ok_or(Invalid("", "需要操作输入对象"))?;
        if fields.keys().any(|key| match key.as_str() {
            "prompt" | "size" | "resolution" | "aspectRatio" | "responseFormat"
            | "outputFormat" | "background" | "watermark" => false,
            "referenceImages" => !edit,
            _ => true,
        }) {
            return Err(Invalid("", "包含未声明的输入字段"));
        }
        let prompt = input["prompt"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or(Invalid("/prompt", "提示词不能为空"))?;
        let tier = choice(
            input,
            "resolution",
            &["2K", "1K", "1.5K", "custom"],
            Invalid("/resolution", "清晰度无效"),
        )?;
        let size = if tier == "custom" {
            input["size"]
                .as_str()
                .filter(|s| s.contains('x') && dimensions_valid(s))
                .ok_or(Invalid("/size", "请填写有效宽x高，例如 1920x1080"))?
        } else {
            tier
        };
        let ratio = match input.get("aspectRatio") {
            None => "auto",
            Some(value) => value.as_str().ok_or(Invalid("/aspectRatio", "画幅无效"))?,
        };
        let size = if ratio == "auto" {
            size.to_string()
        } else {
            let sizes: Value = serde_json::from_str(include_str!("../image-sizes.json"))
                .map_err(|_| Invalid("/size", "图片尺寸表无效"))?;
            if sizes.get(size).is_none() {
                return Err(Invalid(
                    "/aspectRatio",
                    "自定义宽高已指定画幅，请选择自动画幅",
                ));
            }
            sizes[size][ratio]
                .as_str()
                .ok_or(Invalid("/aspectRatio", "不支持此画幅"))?
                .to_string()
        };
        let response_format = choice(
            input,
            "responseFormat",
            &["url", "b64_json"],
            Invalid("/responseFormat", "响应格式无效"),
        )?;
        let output_format = choice(
            input,
            "outputFormat",
            &["png", "jpeg"],
            Invalid("/outputFormat", "输出格式无效"),
        )?;
        let background = choice(
            input,
            "background",
            &["auto", "transparent", "opaque"],
            Invalid("/background", "背景模式无效"),
        )?;
        if output_format == "jpeg" && background == "transparent" {
            return Err(Invalid("/background", "透明背景需要 PNG"));
        }
        let watermark = match input.get("watermark") {
            None => true,
            Some(value) => value
                .as_bool()
                .ok_or(Invalid("/watermark", "水印需要布尔值"))?,
        };
        let mut references = Vec::new();
        if edit {
            let items = input["referenceImages"]
                .as_array()
                .filter(|v| (1..=10).contains(&v.len()))
                .ok_or(Invalid("/referenceImages", "需要 1–10 张参考图片"))?;
            for item in items {
                let id = item["mediaId"]
                    .as_str()
                    .filter(|id| !id.trim().is_empty())
                    .filter(|id| {
                        item.as_object().is_some_and(|fields| fields.len() == 1)
                            && !references.contains(id)
                    })
                    .ok_or(Invalid("/referenceImages", "参考图标识无效或重复"))?;
                references.push(id);
            }
        }
        Ok(Self {
            prompt,
            size,
            response_format,
            output_format,
            background,
            watermark,
            references,
        })
    }

    pub fn material(&self) -> Value {
        json!({
            "size": self.size,
            "responseFormat": self.response_format,
            "outputFormat": self.output_format,
            "background": self.background,
            "watermark": self.watermark,
            "referenceCount": self.references.len(),
        })
    }
}

pub(crate) fn preflight(request: &Value) -> Value {
    let result = if request["environment"]["ready"] == true {
        Parameters::parse(&request["selection"], &request["input"])
    } else {
        Err(Invalid("", "适配器账号或连接尚未就绪"))
    };
    let digest = &request["requestDigest"];
    match result {
        Ok(parameters) => json!({
            "kind": "operation_input_preflight", "status": "ready",
            "requestDigest": digest, "material": parameters.material(),
        }),
        Err(Invalid(pointer, message)) => json!({
            "kind": "operation_input_preflight", "status": "rejected", "requestDigest": digest,
            "diagnostics": [{"code": format!("{PLUGIN}/invalid_input"), "fieldPointer": pointer, "message": message}],
        }),
    }
}
