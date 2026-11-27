use alloc::{
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

pub(crate) fn port(host: &mut dyn Host, operation: &str, payload: Value) -> Result<Value, Failure> {
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

pub(crate) fn origin(config: &Value) -> Result<(&str, &str), Failure> {
    let url = config["endpointBaseUrl"]
        .as_str()
        .unwrap_or_default()
        .trim_end_matches('/');
    let workspace = url
        .strip_prefix("https://")
        .and_then(|host| host.strip_suffix(".cn-beijing.maas.aliyuncs.com"));
    if !workspace.is_some_and(|name| {
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }) {
        return Err(Failure::new("INVALID_INPUT", "请选择百炼北京业务空间"));
    }
    let key = config["credentialRef"]
        .as_str()
        .filter(|key| !key.is_empty())
        .ok_or_else(|| Failure::new("UNAUTHORIZED", "请选择百炼账号"))?;
    Ok((url, key))
}

pub(crate) fn exchange(
    host: &mut dyn Host,
    method: &str,
    url: &str,
    key: Option<&str>,
    headers: Value,
    body: Value,
    budget: u64,
    submission: bool,
) -> Result<Value, Failure> {
    let generated = if key.is_some() {
        json!([{"name":"Authorization","kind":"bearer","targetField":"Authorization"}])
    } else {
        json!([])
    };
    let result = port(
        host,
        "host-http.execute",
        json!({"plan":{
            "method":method,"url":url,"headers":headers,"generatedHeaders":generated,
            "body":body,"sink":{"kind":"media"},"timeoutMs":budget},"credential":key
        }),
    );
    let unknown = || Failure::new("ACCEPTANCE_UNKNOWN", "百炼识别提交的受理状态未知");
    let result = result.map_err(|err| {
        if submission && matches!(err.code, "TIMEOUT" | "UPSTREAM_ERROR") {
            unknown()
        } else {
            err
        }
    })?;
    let status = result["status"].as_u64().ok_or_else(|| {
        if submission {
            unknown()
        } else {
            Failure::new("UPSTREAM_ERROR", "百炼响应缺少 HTTP 状态")
        }
    })?;
    if !((200..300).contains(&status)) {
        return Err(Failure::new(
            rejection_code(status, None, submission),
            "百炼请求失败",
        ));
    }
    if status == 204 {
        return Ok(Value::Null);
    }
    let handle = result["payload"]["handle"].as_str().ok_or_else(|| {
        if submission {
            unknown()
        } else {
            Failure::new("UPSTREAM_ERROR", "百炼响应文件缺失")
        }
    })?;
    let parsed = media_bytes(host, handle, 4 * 1024 * 1024).and_then(|bytes| {
        serde_json::from_slice(&bytes)
            .map_err(|_| Failure::new("UPSTREAM_ERROR", "百炼响应格式无效"))
    });
    let _ = port(host, "host-media.release", json!({"handle":handle}));
    let value: Value = parsed.map_err(|err| if submission { unknown() } else { err })?;
    if value.get("error").is_some() || value["code"].as_str().is_some_and(|s| !s.is_empty()) {
        return Err(Failure::new(
            rejection_code(status, value["code"].as_str(), submission),
            "百炼返回识别错误",
        ));
    }
    Ok(value)
}

pub(crate) fn json_body(host: &mut dyn Host, value: &Value, max: usize) -> Result<Value, Failure> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| Failure::new("INVALID_INPUT", "百炼请求不能序列化"))?;
    if bytes.len() > max {
        return Err(Failure::new("INVALID_INPUT", "百炼请求体超限"));
    }
    let handle = put_media(host, "file", "application/octet-stream", &bytes)?;
    Ok(json!({"kind":"media","handle":handle}))
}
