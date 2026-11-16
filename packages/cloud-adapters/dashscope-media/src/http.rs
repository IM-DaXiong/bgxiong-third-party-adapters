use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use bgx_adapter_v2_sdk::{host, Host};
use serde_json::{json, Value};

const CHUNK: usize = 16 * 1024;

#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
    pub(crate) vendor: Option<String>,
    pub(crate) vendor_request: Option<String>,
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
}

fn port(host: &mut dyn Host, operation: &str, payload: Value) -> Result<Value, Failure> {
    host::call(host, operation, &payload).map_err(|error| {
        let detail = serde_json::to_value(error).unwrap_or(Value::Null);
        let code = match detail["code"].as_str() {
            Some("PERMISSION_DENIED") => "PERMISSION_DENIED",
            Some("INVALID_INPUT") => "INVALID_INPUT",
            Some("TIMEOUT") => "TIMEOUT",
            Some("RESOURCE_UNAVAILABLE") => "RESOURCE_UNAVAILABLE",
            _ => "UPSTREAM_ERROR",
        };
        Failure::new(code, "百炼请求或媒体读取未完成")
    })
}

pub(crate) fn media_bytes(
    host: &mut dyn Host,
    handle: &str,
    limit: usize,
) -> Result<Vec<u8>, Failure> {
    let info = port(host, "host-media.info", json!({"handle":handle}))?;
    let size = info["byteLength"]
        .as_str()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|size| *size > 0 && *size <= limit)
        .ok_or_else(|| Failure::new("INVALID_INPUT", "百炼媒体长度超出操作限制"))?;
    let mut data = Vec::new();
    data.try_reserve_exact(size)
        .map_err(|_| Failure::new("RESOURCE_UNAVAILABLE", "适配包媒体内存不足"))?;
    while data.len() < size {
        let max = (size - data.len()).min(CHUNK);
        let block: Vec<u8> = serde_json::from_value(port(
            host,
            "host-media.read",
            json!({
                "handle":handle,"offset":data.len().to_string(),"maxBytes":max
            }),
        )?)
        .map_err(|_| Failure::new("UPSTREAM_ERROR", "百炼媒体读取响应无效"))?;
        if block.is_empty() || block.len() > max {
            return Err(Failure::new("UPSTREAM_ERROR", "百炼媒体实际长度不符"));
        }
        data.extend(block);
    }
    Ok(data)
}

pub(crate) fn put_media(
    host: &mut dyn Host,
    kind: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<String, Failure> {
    let lease = port(
        host,
        "host-media.create",
        json!({"kind":kind,"mimeHint":mime}),
    )?;
    let handle = lease
        .as_str()
        .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "百炼请求媒体租约缺失"))?;
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

fn rejection_code(status: u64, vendor: Option<&str>, submit: bool) -> &'static str {
    let vendor = vendor.unwrap_or_default().to_ascii_lowercase();
    if status == 402
        || ["quota", "balance", "arrearage", "overdue"]
            .iter()
            .any(|part| vendor.contains(part))
    {
        return "RESOURCE_UNAVAILABLE";
    }
    match status {
        401 | 403 => "UNAUTHORIZED",
        429 => "RATE_LIMITED",
        400 | 404 | 422 => "INVALID_INPUT",
        500..=599 if submit => "ACCEPTANCE_UNKNOWN",
        _ if vendor.contains("invalid") || vendor.contains("unsupported") => "INVALID_INPUT",
        _ => "UPSTREAM_ERROR",
    }
}

pub(crate) fn exchange(
    host: &mut dyn Host,
    payload: &Value,
    path: &str,
    method: &str,
    body: Option<&Value>,
    extra_headers: Value,
    budget: u64,
) -> Result<(Value, String), Failure> {
    let config = &payload["configuration"];
    let endpoint = config["endpointBaseUrl"]
        .as_str()
        .map(|base| base.trim_end_matches('/'))
        .filter(|base| base.starts_with("https://") || base.starts_with("http://"))
        .ok_or_else(|| Failure::new("INVALID_INPUT", "百炼工作空间 API 地址无效"))?;
    let credential = config["credentialRef"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Failure::new("INVALID_INPUT", "百炼工作空间账号未配置"))?;
    let mut headers = extra_headers.as_array().cloned().unwrap_or_default();
    let request_body = if let Some(body) = body {
        headers.push(json!({"name":"Content-Type","value":"application/json"}));
        let bytes = serde_json::to_vec(body)
            .map_err(|_| Failure::new("INVALID_INPUT", "百炼请求不能序列化"))?;
        let handle = put_media(host, "file", "application/octet-stream", &bytes)?;
        json!({"kind":"media","handle":handle})
    } else {
        json!({"kind":"none"})
    };
    let submit = method == "POST";
    let intent = if submit {
        "ACCEPTANCE_UNKNOWN"
    } else {
        "UPSTREAM_ERROR"
    };
    let response = port(host, "host-http.execute", json!({"plan":{
        "method":method,"url":format!("{endpoint}{path}"),"headers":headers,
        "generatedHeaders":[{"name":"Authorization","kind":"bearer","targetField":"Authorization"}],
        "body":request_body,"sink":{"kind":"media"},"timeoutMs":budget},"credential":credential
    })).map_err(|error| {
        if submit && matches!(error.code, "TIMEOUT" | "UPSTREAM_ERROR") {
            Failure::new(intent, "百炼原提交受理状态未知")
        } else { error }
    })?;
    let status = response["status"]
        .as_u64()
        .ok_or_else(|| Failure::new(intent, "百炼响应缺少 HTTP 状态"))?;
    let handle = response["payload"]["handle"]
        .as_str()
        .ok_or_else(|| Failure::new(intent, "百炼原始响应媒体租约缺失"))?;
    let bytes = media_bytes(host, handle, 4 * 1024 * 1024)
        .map_err(|_| Failure::new(intent, "百炼响应读取失败"))?;
    let value = serde_json::from_slice::<Value>(&bytes);
    let upstream = value.as_ref().ok();
    let vendor = upstream
        .and_then(|data| data["code"].as_str())
        .filter(|code| !code.is_empty());
    if !(200..300).contains(&status) || vendor.is_some() {
        let mut failure = Failure::new(rejection_code(status, vendor, submit), "百炼返回错误");
        failure.vendor = vendor.map(String::from);
        failure.vendor_request = upstream
            .and_then(|data| data["request_id"].as_str())
            .map(String::from);
        return Err(failure);
    }
    Ok((
        value.map_err(|_| Failure::new(intent, "百炼响应格式无效"))?,
        handle.into(),
    ))
}

pub(crate) fn download(
    host: &mut dyn Host,
    url: &str,
    role: &str,
    budget: u64,
) -> Result<Value, Failure> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(Failure::new("UPSTREAM_ERROR", "百炼成果地址格式无效"));
    }
    let reply = port(
        host,
        "host-http.execute",
        json!({"plan":{
            "method":"GET","url":url,"headers":[],"generatedHeaders":[],"body":{"kind":"none"},
            "sink":{"kind":"media"},"timeoutMs":budget},"credential":null
        }),
    )?;
    let handle = reply["payload"]["handle"]
        .as_str()
        .filter(|_| reply["status"] == 200)
        .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "百炼原成果下载未完成"))?;
    media_output(host, handle, role)
}

pub(crate) fn media_output(
    host: &mut dyn Host,
    handle: &str,
    role: &str,
) -> Result<Value, Failure> {
    let info = port(host, "host-media.info", json!({"handle":handle}))?;
    Ok(json!({"kind":"media","role":role,"media":{
        "id":info["id"],"handle":handle,"kind":info["kind"],"fileName":info["fileName"],
        "mime":info["mime"],"byteLength":info["byteLength"]
    }}))
}
