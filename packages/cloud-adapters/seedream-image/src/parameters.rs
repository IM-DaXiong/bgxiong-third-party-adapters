use alloc::{format, string::String};
use serde_json::{json, Value};

use super::{MODEL, OP_EDIT, OP_GENERATE, OP_VERSION};

pub(crate) fn operation(input: &Value) -> Option<usize> {
    if input["modelId"] != MODEL || input["operationVersion"] != OP_VERSION || input["capabilityKey"] != "image.generate" {
        return None;
    }
    match input["operationId"].as_str()? {
        OP_GENERATE => Some(0),
        OP_EDIT => Some(1),
        _ => None,
    }
}

pub(crate) fn material(input: &Value, minimum: usize) -> Result<Value, (&'static str, &'static str)> {
    let count = input["referenceImages"].as_array().map_or(0, |references| references.len());
    let limit = if minimum == 0 { 0 } else { 10 };
    if count < minimum || count > limit {
        return Err(("/referenceImages", "参考图片数量不符合操作声明"));
    }
    if input["prompt"].as_str().is_none_or(|prompt| prompt.trim().is_empty()) {
        return Err(("/prompt", "提示词不能为空"));
    }
    Ok(json!({
        "size":input.get("size").cloned().unwrap_or(json!("2K")),
        "watermark":input.get("watermark").cloned().unwrap_or(json!(false)),
        "responseFormat":input.get("responseFormat").cloned().unwrap_or(json!("url")),
        "referenceCount":count,
    }))
}

pub(crate) fn preflight(request: &Value) -> Value {
    let projected = if request["environment"]["ready"] != true {
        Err(("", "适配器账号或连接尚未就绪"))
    } else {
        operation(&request["selection"]).ok_or(("", "操作或模型已变化"))
            .and_then(|minimum| material(&request["input"], minimum))
    };
    match projected {
        Ok(material) => json!({"kind":"operation_input_preflight","status":"ready","requestDigest":request["requestDigest"],"material":material}),
        Err((pointer,message)) => json!({"kind":"operation_input_preflight","status":"rejected","requestDigest":request["requestDigest"],
            "diagnostics":[{"code":"local.bgx.seedream-image/invalid_input","fieldPointer":pointer,"message":message}]}),
    }
}

pub(crate) fn endpoint_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with("/images/generations") {
        return String::from(base);
    }
    let base = base.strip_suffix("/chat/completions").unwrap_or(base);
    format!("{base}/images/generations")
}
