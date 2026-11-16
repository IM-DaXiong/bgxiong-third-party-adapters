use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::{host, Host};
use serde_json::{json, Value};

use super::parameters;

const CHUNK: usize = 16 * 1024;
const MAX_REFERENCE_BYTES: usize = 10 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 24 * 1024 * 1024;

pub(crate) struct Failure {
    pub code: &'static str,
    pub message: &'static str,
    pub vendor: Option<String>,
    pub vendor_request: Option<String>,
}

impl Failure {
    pub(crate) fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            vendor: None,
            vendor_request: None,
        }
    }
    fn invalid(message: &'static str) -> Self {
        Self::new("INVALID_INPUT", message)
    }
    fn upstream(message: &'static str) -> Self {
        Self::new("UPSTREAM_ERROR", message)
    }
}

type Result<T> = core::result::Result<T, Failure>;

fn port(host: &mut dyn Host, method: &str, args: Value) -> Result<Value> {
    host::call(host, method, &args).map_err(|error| {
        if serde_json::to_value(error).is_ok_and(|value| value["code"] == "PERMISSION_DENIED") {
            Failure::new("PERMISSION_DENIED", "SDK 拒绝了资源或网络授权")
        } else {
            Failure::upstream("SDK 网络或资源操作失败")
        }
    })
}

pub(crate) fn media_bytes(host: &mut dyn Host, handle: &str, limit: usize) -> Result<Vec<u8>> {
    let identity = port(host, "host-media.info", json!({"handle":handle}))?;
    let size = identity["byteLength"]
        .as_str()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|size| *size > 0 && *size <= limit)
        .ok_or(Failure::invalid("媒体大小不符合声明"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| Failure::new("RESOURCE_UNAVAILABLE", "媒体超过适配器内存限额"))?;
    while bytes.len() < size {
        let max = (size - bytes.len()).min(CHUNK);
        let block: Vec<u8> = serde_json::from_value(port(
            host,
            "host-media.read",
            json!({
                "handle":handle,"offset":bytes.len().to_string(),"maxBytes":max
            }),
        )?)
        .map_err(|_| Failure::upstream("媒体读取结果无效"))?;
        if block.is_empty() || block.len() > max {
            return Err(Failure::upstream("媒体实际长度与租约不符"));
        }
        bytes.extend(block);
    }
    Ok(bytes)
}

pub(crate) fn put_media(
    host: &mut dyn Host,
    kind: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<String> {
    let resource = port(
        host,
        "host-media.create",
        json!({"kind":kind,"mimeHint":mime}),
    )?;
    let handle = resource
        .as_str()
        .ok_or(Failure::upstream("无法创建媒体租约"))?;
    for block in bytes.chunks(CHUNK) {
        port(
            host,
            "host-media.write",
            json!({"handle":handle,"bytes":block}),
        )?;
    }
    port(host, "host-media.finish", json!({"handle":handle}))?;
    Ok(handle.into())
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Some("image/jpeg");
    }
    None
}

pub(crate) fn request(
    host: &mut dyn Host,
    method: &str,
    url: &str,
    body: Value,
    credential: Option<&str>,
    budget: u64,
) -> Result<Value> {
    let headers = if method == "POST" {
        json!([{"name":"Content-Type","value":"application/json"}])
    } else {
        json!([])
    };
    let authorization = if credential.is_some() {
        json!([{"name":"Authorization","kind":"bearer","targetField":"Authorization"}])
    } else {
        json!([])
    };
    port(
        host,
        "host-http.execute",
        json!({"plan":{"method":method,"url":url,"headers":headers,"generatedHeaders":authorization,
        "body":body,"sink":{"kind":"media"},"timeoutMs":budget},"credential":credential}),
    )
}

pub(crate) fn output(host: &mut dyn Host, handle: &str, role: &str) -> Result<Value> {
    let identity = port(host, "host-media.info", json!({"handle":handle}))?;
    Ok(json!({"kind":"media","role":role,"media":{
        "id":identity["id"],"handle":handle,"kind":identity["kind"],"fileName":identity["fileName"],"mime":identity["mime"],"byteLength":identity["byteLength"]
    }}))
}

pub(crate) fn submit(host: &mut dyn Host, payload: &Value, budget: u64) -> Result<Value> {
    let input = &payload["input"];
    let mut body =
        parameters::material(payload, input).map_err(|(_, message)| Failure::invalid(message))?;
    if payload["preparation"]["material"] != body {
        return Err(Failure::invalid("MiniMax 提交值与已冻结预检不一致"));
    }
    if let Some(subject) = input["subjectImages"][0].as_str() {
        let handle = payload["media"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|value| value["id"] == subject)
            .and_then(|value| value["handle"].as_str())
            .ok_or(Failure::invalid("人物参考图租约缺失"))?;
        let bytes = media_bytes(host, handle, MAX_REFERENCE_BYTES)?;
        let mime = image_mime(&bytes).ok_or(Failure::invalid("人物参考图需要 JPEG 或 PNG"))?;
        body["subject_reference"] = json!([{"type":"character","image_file":format!("data:{mime};base64,{}",STANDARD.encode(bytes))}]);
    }
    let config = &payload["configuration"];
    let endpoint = config["endpointBaseUrl"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or(Failure::invalid("MiniMax API 地址缺失"))?
        .trim_end_matches('/');
    let secret = config["credentialRef"]
        .as_str()
        .ok_or(Failure::invalid("MiniMax 账号引用缺失"))?;
    let url = if endpoint.ends_with("/v1/image_generation") {
        endpoint.into()
    } else if endpoint.ends_with("/v1") {
        format!("{endpoint}/image_generation")
    } else {
        format!("{endpoint}/v1/image_generation")
    };
    let encoded =
        serde_json::to_vec(&body).map_err(|_| Failure::invalid("MiniMax 请求编码失败"))?;
    let request_body = put_media(host, "file", "application/octet-stream", &encoded)?;
    let response = request(
        host,
        "POST",
        &url,
        json!({"kind":"media","handle":request_body}),
        Some(secret),
        budget,
    )
    .map_err(|failure| {
        if failure.code == "UPSTREAM_ERROR" {
            Failure::new("ACCEPTANCE_UNKNOWN", "MiniMax 响应未确认")
        } else {
            failure
        }
    })?;
    let status = response["status"]
        .as_u64()
        .ok_or(Failure::upstream("MiniMax HTTP 状态缺失"))?;
    let response_handle = response["payload"]["handle"]
        .as_str()
        .ok_or(Failure::upstream("MiniMax 响应文件缺失"))?;
    let raw = media_bytes(host, response_handle, MAX_RESPONSE_BYTES)?;
    let body = serde_json::from_slice::<Value>(&raw);
    let vendor = body
        .as_ref()
        .ok()
        .and_then(|value| value.pointer("/base_resp/status_code"))
        .and_then(Value::as_i64);
    if !(200..300).contains(&status) || vendor.is_some_and(|code| code != 0) {
        let code = match (status, vendor) {
            (402, _) | (_, Some(1008)) => "RESOURCE_UNAVAILABLE",
            (401 | 403, _) | (_, Some(1004)) => "UNAUTHORIZED",
            (429, _) | (_, Some(1002)) => "RATE_LIMITED",
            (400 | 422, _) | (_, Some(2013)) => "INVALID_INPUT",
            _ => "UPSTREAM_ERROR",
        };
        let message = if status == 402 || vendor == Some(1008) {
            "MiniMax 账号余额不足"
        } else {
            "MiniMax 拒绝图像请求"
        };
        let mut details = Failure::new(code, message);
        details.vendor = vendor.map(|code| code.to_string());
        details.vendor_request = body
            .as_ref()
            .ok()
            .and_then(|value| value["id"].as_str().map(String::from));
        return Err(details);
    }
    let body = body.map_err(|_| {
        Failure::new(
            "ACCEPTANCE_UNKNOWN",
            "MiniMax 返回了无效结果，原请求状态未确认",
        )
    })?;
    let urls = body["data"]["image_urls"]
        .as_array()
        .filter(|values| !values.is_empty());
    let base64 = body["data"]["image_base64"]
        .as_array()
        .filter(|values| !values.is_empty());
    let (kind, image) = if let Some(values) = base64 {
        ("base64", values)
    } else if let Some(values) = urls {
        ("url", values)
    } else {
        return Err(Failure::upstream("MiniMax 返回结果内没有图片"));
    };
    let [image] = image.as_slice() else {
        return Err(Failure::upstream("图片数量与请求不符"));
    };
    let image = image
        .as_str()
        .ok_or(Failure::upstream("图片回执格式无效"))?;
    let handle = if kind == "base64" {
        let bytes = STANDARD
            .decode(image)
            .map_err(|_| Failure::upstream("MiniMax 图片编码无效"))?;
        let mime = image_mime(&bytes).ok_or(Failure::upstream("MiniMax 图片容器无效"))?;
        put_media(host, "image", mime, &bytes)?
    } else {
        let result = request(host, "GET", image, json!({"kind":"none"}), None, budget)?;
        if !result["status"]
            .as_u64()
            .is_some_and(|status| (200..300).contains(&status))
        {
            return Err(Failure::upstream("MiniMax 原图下载失败"));
        }
        result["payload"]["handle"]
            .as_str()
            .ok_or(Failure::upstream("MiniMax 原图租约缺失"))?
            .into()
    };
    let image = output(host, &handle, "images")?;
    let reply = output(host, response_handle, "provider-response")?;
    Ok(
        json!({"kind":"completed","vendorTaskId":null,"appliedParams":null,"outputs":[image,reply],
        "data":{"providerRequestId":body["id"],"imageCount":1},"usage":null}),
    )
}
