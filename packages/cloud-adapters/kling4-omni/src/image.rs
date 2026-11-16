use alloc::{format, string::ToString, vec::Vec};
use bgx_adapter_v2_sdk::{Host, Invocation};
use serde_json::{json, Value};

use crate::{
    api_failure, authorization, failure, http, invalid, malformed, port, reject_before_submit,
    Result, API, PLUGIN,
};

mod modes;
mod references;

pub(crate) use modes::capabilities;
use modes::{selected_mode, ImageMode};

fn material(
    selection: &Value,
    input: &Value,
) -> core::result::Result<Value, (&'static str, &'static str)> {
    let mode = selected_mode(selection, true).ok_or(("", "可灵图像操作身份已改变"))?;
    let prompt = input["prompt"]
        .as_str()
        .filter(|p| !p.trim().is_empty() && p.chars().count() <= 2500)
        .ok_or(("/prompt", "请输入 1–2500 个字符的可灵提示词"))?;
    let prompt = if mode.is_omni() {
        prompt.trim()
    } else {
        prompt
    };
    let count = input["referenceImages"].as_array().map_or(0, Vec::len);
    let limit = mode.reference_limit();
    if (limit == 0 && count != 0) || (limit > 0 && !(1..=limit).contains(&count)) {
        return Err(("/referenceImages", "参考图数量不符合所选可灵操作"));
    }
    let mut body = json!({"model_name":selection["modelId"],"prompt":prompt,"n":1});
    if let Some(negative) = input["negativePrompt"].as_str() {
        if mode != ImageMode::ClassicText {
            return Err(("/negativePrompt", "所选操作通过提示词表达具体要求"));
        }
        if negative.chars().count() > 2500 {
            return Err(("/negativePrompt", "可灵反向提示词最多 2500 字符"));
        }
        if !negative.trim().is_empty() {
            body["negative_prompt"] = json!(negative.trim());
        }
    }
    if let Some(size) = input["size"].as_str() {
        if !matches!(size, "1k" | "2k") {
            return Err(("/size", "可灵图像清晰度须为 1K 或 2K"));
        }
        body["resolution"] = json!(size);
    }
    if let Some(ratio) = input["aspectRatio"].as_str() {
        if !(mode.is_omni() && ratio == "auto")
            && !matches!(
                ratio,
                "16:9" | "9:16" | "1:1" | "4:3" | "3:4" | "3:2" | "2:3" | "21:9"
            )
        {
            return Err(("/aspectRatio", "可灵图像不支持该画幅"));
        }
        body["aspect_ratio"] = json!(ratio);
    }
    Ok(body)
}

pub(crate) fn preflight(payload: &Value) -> Value {
    let value = if payload["environment"]["ready"] != true {
        Err(("", "可灵图像账号尚未就绪"))
    } else {
        material(&payload["selection"], &payload["input"])
    };
    match value {
        Ok(material) => {
            json!({"kind":"operation_input_preflight","status":"ready","requestDigest":payload["requestDigest"],"material":material})
        }
        Err((field, message)) => {
            json!({"kind":"operation_input_preflight","status":"rejected","requestDigest":payload["requestDigest"],
            "diagnostics":[{"code":format!("{PLUGIN}/invalid_image_input"),"fieldPointer":field,"message":message}]})
        }
    }
}

fn original_handle(host: &mut dyn Host, bytes: &[u8]) -> Result<alloc::string::String> {
    let result = port(
        host,
        "host-media.create",
        json!({"kind":"file","mimeHint":"application/json"}),
    )?;
    let handle = result.as_str().ok_or_else(malformed)?;
    for block in bytes.chunks(16 * 1024) {
        port(
            host,
            "host-media.write",
            json!({"handle":handle,"bytes":block}),
        )?;
    }
    port(host, "host-media.finish", json!({"handle":handle}))?;
    Ok(handle.to_string())
}

fn output(host: &mut dyn Host, handle: &str, kind: &str, role: &str) -> Result<Value> {
    let info = port(host, "host-media.info", json!({"handle":handle}))?;
    if info["kind"] != kind
        || info["byteLength"]
            .as_str()
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
            == 0
    {
        return Err(malformed());
    }
    Ok(json!({"kind":"media","role":role,"media":{
        "id":info["id"],"kind":info["kind"],"mime":info["mime"],"handle":handle,
        "fileName":info["fileName"],"byteLength":info["byteLength"]
    }}))
}

fn result_urls(value: &Value) -> Vec<&str> {
    let rows = [
        value.get("data"),
        value.pointer("/data/images"),
        value.pointer("/data/task_result/images"),
        value.pointer("/data/task_result/image_list"),
        value.pointer("/result/images"),
        value.get("images"),
    ];
    let mut urls = Vec::new();
    for rows in rows.into_iter().flatten().filter_map(Value::as_array) {
        for item in rows {
            let url = item["url"]
                .as_str()
                .or_else(|| item["image_url"].as_str())
                .or_else(|| item["image"].as_str())
                .or_else(|| item.as_str());
            if let Some(url) = url.map(str::trim).filter(|url| !url.is_empty()) {
                if !urls.contains(&url) {
                    urls.push(url);
                }
            }
        }
    }
    urls
}

fn result_media(
    host: &mut dyn Host,
    request: &Invocation,
    value: &Value,
    original: &[u8],
) -> Result<(Vec<Value>, usize)> {
    let urls = result_urls(value);
    if urls.is_empty() {
        return Err(malformed());
    }
    let count = urls.len();
    let mut outputs = Vec::with_capacity(count + 1);
    for url in urls {
        let response = http(
            host,
            request,
            "GET",
            url.into(),
            json!({"kind":"none"}),
            None,
        )?;
        let handle = response["payload"]["handle"]
            .as_str()
            .filter(|_| response["status"] == 200)
            .ok_or_else(|| failure("UPSTREAM_ERROR", "可灵原图下载失败"))?;
        outputs.push(output(host, handle, "image", "images")?);
    }
    let receipt = original_handle(host, original)?;
    outputs.push(output(host, &receipt, "file", "provider-response")?);
    Ok((outputs, count))
}

fn usage(value: &Value) -> Value {
    let amount = value
        .pointer("/data/final_unit_deduction")
        .and_then(Value::as_str)
        .filter(|amount| {
            !amount.is_empty()
                && amount.split('.').count() <= 2
                && amount
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        });
    amount.map_or(Value::Null, |amount| json!({"measurements":[{"metric":"credits","unit":"credits","amount":amount,"source":"reported"}]}))
}

fn reply(response: &Value) -> Result<(u64, Vec<u8>, Value)> {
    let status = response["status"].as_u64().ok_or_else(malformed)?;
    let bytes: Vec<u8> =
        serde_json::from_value(response["payload"]["bytes"].clone()).map_err(|_| malformed())?;
    let value = serde_json::from_slice(&bytes).map_err(|_| malformed())?;
    Ok((status, bytes, value))
}

fn business_code(value: &Value) -> Option<i64> {
    if let Some(code) = value.get("code") {
        if code == "success" || code == "0" {
            return Some(0);
        }
        return code
            .as_i64()
            .or_else(|| code.as_str().and_then(|code| code.parse().ok()));
    }
    None
}

fn task_id(value: &Value) -> Option<&str> {
    value
        .pointer("/data/task_id")
        .or_else(|| value.get("task_id"))
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

fn unknown() -> crate::Failure {
    failure("ACCEPTANCE_UNKNOWN", "可灵图像提交受理状态未确认")
}

pub(crate) fn submit(host: &mut dyn Host, request: &Invocation, payload: &Value) -> Result<Value> {
    let mode = selected_mode(payload, true).ok_or_else(|| invalid("可灵图像操作身份已改变"))?;
    let body = material(payload, &payload["input"]).map_err(|(_, message)| invalid(message))?;
    if payload["preparation"]["material"] != body {
        return Err(invalid("可灵图像请求与冻结预检不一致"));
    }
    let credential = match authorization(host, payload) {
        Ok(credential) => credential,
        Err(error) => {
            reject_before_submit(host, request, &error)?;
            return Err(error);
        }
    };
    let body = references::request_body(host, mode, payload, &body)?;
    let response = http(
        host,
        request,
        "POST",
        format!("{API}{}", mode.path()),
        body.clone(),
        Some(credential),
    );
    if let Some(handle) = body["handle"].as_str() {
        let _ = port(host, "host-media.release", json!({"handle":handle}));
    }
    let response = response.map_err(|_| unknown())?;
    let (status, bytes, value) = reply(&response).map_err(|_| unknown())?;
    let code = business_code(&value);
    if !(200..300).contains(&status) || code.is_some_and(|code| code != 0) {
        return Err(if status >= 500 || task_id(&value).is_some() {
            unknown()
        } else {
            api_failure(status, code)
        });
    }
    if let Some(id) = task_id(&value) {
        return Ok(
            json!({"kind":"accepted","vendorTaskId":id,"appliedParams":null,
            "resumeToken":mode.resume_token(&payload["modelId"],id),"usage":usage(&value)}),
        );
    }
    if result_urls(&value).is_empty() {
        return Err(unknown());
    }
    let (outputs, count) = result_media(host, request, &value, &bytes)?;
    Ok(
        json!({"kind":"completed","vendorTaskId":null,"appliedParams":null,"outputs":outputs,
        "data":{"remoteTaskId":null,"imageCount":count},"usage":usage(&value)}),
    )
}

pub(crate) fn query(host: &mut dyn Host, request: &Invocation, payload: &Value) -> Result<Value> {
    let id = payload["vendorTaskId"]
        .as_str()
        .ok_or_else(|| invalid("可灵原图任务号缺失"))?;
    let mode = selected_mode(payload, false).ok_or_else(|| invalid("可灵图像续查身份不匹配"))?;
    if payload["resumeToken"] != mode.resume_token(&payload["modelId"], id)
        || task_id(&json!({"task_id":id})) != Some(id)
    {
        return Err(invalid("可灵图像续查身份不匹配"));
    }
    let credential = authorization(host, payload)?;
    let response = http(
        host,
        request,
        "GET",
        format!("{API}{}/{id}", mode.path()),
        json!({"kind":"none"}),
        Some(credential),
    )?;
    let (status, bytes, value) = reply(&response)?;
    let code = business_code(&value);
    if !(200..300).contains(&status) || code.is_some_and(|code| code != 0) {
        return Err(api_failure(status, code));
    }
    if task_id(&value) != Some(id) {
        return Err(malformed());
    }
    let status = value
        .pointer("/data/task_status")
        .or_else(|| value.get("task_status"))
        .or_else(|| value.pointer("/data/status"))
        .and_then(Value::as_str);
    let mut result = json!({"status":null,"outputs":[],"data":null,"usage":usage(&value),
        "progress":null,"terminalFailure":null,"resumeToken":payload["resumeToken"]});
    match status {
        Some("submitted") => result["status"] = json!("queued"),
        Some("processing") => result["status"] = json!("running"),
        Some("succeed") => {
            let (outputs, count) =
                result_media(host, request, &value, &bytes).map_err(|mut error| {
                    error.retryable = true;
                    error
                })?;
            result["status"] = json!("succeeded");
            result["outputs"] = json!(outputs);
            result["data"] = json!({"remoteTaskId":id,"imageCount":count});
        }
        Some("failed") => {
            result["status"] = json!("failed");
            result["terminalFailure"] = json!({"category":"vendorTaskFailed",
                "message":"可灵图像任务失败，请核对原任务记录","vendorCode":null,"details":{"taskId":id}});
        }
        _ => return Err(malformed()),
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
