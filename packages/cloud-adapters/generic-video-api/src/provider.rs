use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::{host, Host};
use serde_json::{json, Value};

use crate::parameters::{self, Operation};

const CHUNK: usize = 16 * 1024;
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct Error {
    pub code: &'static str,
    pub message: &'static str,
    pub vendor_code: Option<String>,
    pub vendor_request_id: Option<String>,
}

impl Error {
    pub(crate) fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            vendor_code: None,
            vendor_request_id: None,
        }
    }
}

fn call(host: &mut dyn Host, method: &str, args: Value) -> Result<Value, Error> {
    host::call(host, method, &args).map_err(|error| {
        let detail = serde_json::to_value(error).unwrap_or(Value::Null);
        let code = match detail["code"].as_str() {
            Some("PERMISSION_DENIED") => "PERMISSION_DENIED",
            Some("INVALID_INPUT") => "INVALID_INPUT",
            Some("TIMEOUT") => "TIMEOUT",
            Some("RESOURCE_UNAVAILABLE") => "RESOURCE_UNAVAILABLE",
            _ => "UPSTREAM_ERROR",
        };
        Error::new(code, "通用视频的宿主请求未完成")
    })
}

fn read_media(
    host: &mut dyn Host,
    lease: &str,
    info: &Value,
    limit: usize,
) -> Result<Vec<u8>, Error> {
    let size = info["byteLength"]
        .as_str()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|size| (1..=limit).contains(size))
        .ok_or_else(|| Error::new("RESOURCE_UNAVAILABLE", "通用视频媒体为空或超过读取限额"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| Error::new("RESOURCE_UNAVAILABLE", "通用视频内存不足"))?;
    while bytes.len() < size {
        let count = (size - bytes.len()).min(CHUNK);
        let data = call(
            host,
            "host-media.read",
            json!({"handle":lease,"offset":bytes.len().to_string(),"maxBytes":count}),
        )?;
        let block: Vec<u8> = serde_json::from_value(data)
            .map_err(|_| Error::new("UPSTREAM_ERROR", "通用视频媒体读取无效"))?;
        if block.is_empty() || block.len() > count {
            return Err(Error::new("UPSTREAM_ERROR", "通用视频媒体长度与租约不一致"));
        }
        bytes.extend(block);
    }
    Ok(bytes)
}

fn put_json(host: &mut dyn Host, body: &Value) -> Result<String, Error> {
    let bytes = serde_json::to_vec(body)
        .map_err(|_| Error::new("INVALID_INPUT", "通用视频参数不能序列化"))?;
    let lease = call(
        host,
        "host-media.create",
        json!({"kind":"file","mimeHint":"application/octet-stream"}),
    )?;
    let lease = lease
        .as_str()
        .ok_or_else(|| Error::new("UPSTREAM_ERROR", "通用视频请求媒体未创建"))?;
    for block in bytes.chunks(CHUNK) {
        call(
            host,
            "host-media.write",
            json!({"handle":lease,"bytes":block}),
        )?;
    }
    call(host, "host-media.finish", json!({"handle":lease}))?;
    Ok(lease.into())
}

pub(crate) fn base_url(config: &Value) -> Option<String> {
    let base = config["endpointBaseUrl"]
        .as_str()?
        .trim()
        .trim_end_matches('/');
    let location = base
        .strip_prefix("https://")
        .or_else(|| base.strip_prefix("http://"))?;
    let authority = location.split('/').next()?;
    if authority.is_empty()
        || authority.contains('@')
        || base.contains(['?', '#', '\\'])
        || base.chars().any(char::is_whitespace)
    {
        return None;
    }
    if base.ends_with("/v1") || base.contains("/v1/") {
        Some(base.into())
    } else {
        Some(format!("{base}/v1"))
    }
}

pub(crate) fn exchange(
    host: &mut dyn Host,
    config: &Value,
    path: &str,
    body: Option<&Value>,
    budget: u64,
) -> Result<(Value, String), Error> {
    let base = base_url(config)
        .ok_or_else(|| Error::new("INVALID_INPUT", "通用视频账号缺少有效的 API 地址"))?;
    let credential = config["credentialRef"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::new("UNAUTHORIZED", "通用视频账号缺少凭据"))?;
    let submitting = body.is_some();
    let (method, headers, request) = if let Some(body) = body {
        (
            "POST",
            json!([{"name":"Content-Type","value":"application/json"}]),
            json!({"kind":"media","handle":put_json(host,body)?}),
        )
    } else {
        ("GET", json!([]), json!({"kind":"none"}))
    };
    let ambiguous = if submitting {
        "ACCEPTANCE_UNKNOWN"
    } else {
        "UPSTREAM_ERROR"
    };
    let response = call(host,"host-http.execute",json!({"plan":{
        "method":method,"url":format!("{base}{path}"),"headers":headers,
        "generatedHeaders":[{"name":"Authorization","kind":"bearer","targetField":"Authorization"}],
        "body":request,"sink":{"kind":"media"},"timeoutMs":budget},"credential":credential
    })).map_err(|error| {
        if submitting && matches!(error.code,"UPSTREAM_ERROR"|"TIMEOUT") {
            Error::new(ambiguous,"通用视频提交后未收到可确认的回执")
        } else { error }
    })?;
    let status = response["status"]
        .as_u64()
        .ok_or_else(|| Error::new(ambiguous, "通用视频回执缺少 HTTP 状态"))?;
    let lease = response["payload"]["handle"].as_str();
    let read_response = || Error::new(ambiguous, "通用视频回执不是可读取的 JSON");
    let decode = |host: &mut dyn Host, lease: &str| -> Result<Value, Error> {
        let info = call(host, "host-media.info", json!({"handle":lease}))?;
        let bytes = read_media(host, lease, &info, RESPONSE_LIMIT)?;
        serde_json::from_slice(&bytes).map_err(|_| read_response())
    };
    if !(200..300).contains(&status) {
        let raw = lease
            .and_then(|lease| decode(host, lease).ok())
            .unwrap_or(Value::Null);
        let code = match status {
            401 | 403 => "UNAUTHORIZED",
            402 => "RESOURCE_UNAVAILABLE",
            400 | 404 | 422 => "INVALID_INPUT",
            429 => "RATE_LIMITED",
            408 | 500..=599 if submitting => "ACCEPTANCE_UNKNOWN",
            _ => "UPSTREAM_ERROR",
        };
        let mut error = Error::new(code, "通用视频 API 返回 HTTP 错误");
        error.vendor_code = raw
            .pointer("/error/code")
            .or_else(|| raw.get("code"))
            .and_then(Value::as_str)
            .map(String::from);
        error.vendor_request_id = raw
            .get("request_id")
            .and_then(Value::as_str)
            .map(String::from);
        return Err(error);
    }
    let lease = lease.ok_or_else(read_response)?;
    Ok((
        decode(host, lease).map_err(|_| read_response())?,
        lease.into(),
    ))
}

fn original_image(host: &mut dyn Host, media: &Value, id: &str) -> Result<String, Error> {
    let handle = media
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["id"] == id && item["kind"] == "image")
        .and_then(|item| item["handle"].as_str())
        .ok_or_else(|| Error::new("INVALID_INPUT", "通用视频图片素材租约缺失"))?;
    let info = call(host, "host-media.info", json!({"handle":handle}))?;
    if info["kind"] != "image" {
        return Err(Error::new(
            "INVALID_INPUT",
            "通用视频的素材类型与图片槽不匹配",
        ));
    }
    Ok(STANDARD.encode(read_media(host, handle, &info, usize::MAX)?))
}

pub(crate) fn submit(
    host: &mut dyn Host,
    operation: &Operation,
    payload: &Value,
    budget: u64,
) -> Result<Value, Error> {
    let config = &payload["configuration"];
    let frozen = parameters::material(operation, &payload["input"], config)
        .map_err(|(_, message)| Error::new("INVALID_INPUT", message))?;
    if payload["preparation"]["material"] != frozen {
        return Err(Error::new("INVALID_INPUT", "通用视频提交与冻结预检不一致"));
    }
    let mut body = frozen["body"].clone();
    if let Some(image) = operation.image {
        let ids = frozen["imageIds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str);
        let images = ids
            .map(|id| original_image(host, &payload["media"], id))
            .collect::<Result<Vec<_>, _>>()?;
        let value = if image.role == "first_frame" {
            json!(images.first())
        } else {
            json!(images)
        };
        body[image.wire_field] = value;
    }
    let path = format!("/videos/{}", operation.path);
    let (raw, _) = exchange(host, config, &path, Some(&body), budget)?;
    let id = crate::recovery::task_id(&raw)
        .ok_or_else(|| Error::new("ACCEPTANCE_UNKNOWN", "通用视频回执缺少原任务号"))?;
    Ok(
        json!({"kind":"accepted","vendorTaskId":id,"resumeToken":{"taskId":id},"appliedParams":null,"usage":null}),
    )
}

pub(crate) fn result_media(
    host: &mut dyn Host,
    handle: &str,
    role: &str,
    kind: &str,
) -> Result<Value, Error> {
    let info = call(host, "host-media.info", json!({"handle":handle}))?;
    if info["kind"] != kind {
        return Err(Error::new(
            "UPSTREAM_ERROR",
            "通用视频结果字节与声明类型不匹配",
        ));
    }
    Ok(json!({"kind":"media","role":role,"media":{
        "id":info["id"],"handle":handle,"kind":kind,"mime":info["mime"],
        "fileName":info["fileName"],"byteLength":info["byteLength"]
    }}))
}

pub(crate) fn download(host: &mut dyn Host, url: &str, budget: u64) -> Result<Value, Error> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(Error::new("UPSTREAM_ERROR", "通用视频结果缺少有效下载地址"));
    }
    let response = call(
        host,
        "host-http.execute",
        json!({"plan":{
        "method":"GET","url":url,"body":{"kind":"none"},"headers":[],"generatedHeaders":[],
        "sink":{"kind":"media"},"timeoutMs":budget},"credential":null}),
    )?;
    let handle = response["payload"]["handle"]
        .as_str()
        .filter(|_| response["status"] == 200)
        .ok_or_else(|| Error::new("UPSTREAM_ERROR", "通用视频的原始媒体尚未收取"))?;
    result_media(host, handle, "video", "video")
}
