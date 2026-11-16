use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::Host;
use serde_json::{json, Value};

use super::provider::{port, Failure, Result};

const CHUNK: usize = 16 * 1024;

pub(crate) fn data_url(host: &mut dyn Host, medium: &Value, kind: &str) -> Result<String> {
    let handle = medium["handle"]
        .as_str()
        .ok_or(Failure::invalid("素材租约缺失"))?;
    let stat = port(host, "host-media.info", json!({"handle":handle}))?;
    let mime = stat["mime"]
        .as_str()
        .ok_or(Failure::invalid("素材类型缺失"))?;
    if stat["kind"] != kind || mime.split_once('/').map(|(category, _)| category) != Some(kind) {
        return Err(Failure::invalid("素材类型与操作槽不匹配"));
    }
    let length = stat["byteLength"]
        .as_str()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|count| *count > 0)
        .ok_or(Failure::invalid("素材为空或长度无效"))?;
    let mut content = Vec::new();
    content
        .try_reserve_exact(length)
        .map_err(|_| Failure::new("RESOURCE_UNAVAILABLE", "素材超过适配器可用内存"))?;
    while content.len() < length {
        let count = (length - content.len()).min(CHUNK);
        let block: Vec<u8> = serde_json::from_value(port(
            host,
            "host-media.read",
            json!({
                "handle":handle,"offset":content.len().to_string(),"maxBytes":count,
            }),
        )?)
        .map_err(|_| Failure::invalid("无法读取素材"))?;
        if block.is_empty() || block.len() > count {
            return Err(Failure::invalid("素材读入长度与租约不一致"));
        }
        content.extend_from_slice(&block);
    }
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(content)))
}

pub(crate) fn request_body(host: &mut dyn Host, value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| Failure::invalid("无法编码请求"))?;
    let handle = port(
        host,
        "host-media.create",
        json!({"kind":"file","mimeHint":"application/octet-stream"}),
    )?
    .as_str()
    .map(String::from)
    .ok_or(Failure::invalid("请求资源租约缺失"))?;
    for block in bytes.chunks(CHUNK) {
        port(
            host,
            "host-media.write",
            json!({"handle":handle,"bytes":block}),
        )?;
    }
    port(host, "host-media.finish", json!({"handle":handle}))?;
    Ok(handle)
}

pub(crate) fn download(
    host: &mut dyn Host,
    url: &str,
    kind: &str,
    role: &str,
    budget: u64,
) -> Result<Value> {
    let response = port(
        host,
        "host-http.execute",
        json!({"plan":{
        "method":"GET","url":url,"headers":[],"generatedHeaders":[],"body":{"kind":"none"},
        "sink":{"kind":"media"},"timeoutMs":budget,
    },"credential":null}),
    )?;
    if response["status"] != 200 {
        return Err(Failure::new("UPSTREAM_ERROR", "方舟媒体下载失败"));
    }
    let handle = response["payload"]["handle"]
        .as_str()
        .ok_or(Failure::new("UPSTREAM_ERROR", "下载结果缺少媒体租约"))?;
    let stat = port(host, "host-media.info", json!({"handle":handle}))?;
    if stat["kind"] != kind {
        return Err(Failure::new("UPSTREAM_ERROR", "下载字节与成果类型不匹配"));
    }
    Ok(
        json!({"kind":"media","role":role,"media":{"id":stat["id"],"handle":handle,
        "kind":kind,"mime":stat["mime"],"fileName":stat["fileName"],"byteLength":stat["byteLength"]}}),
    )
}
