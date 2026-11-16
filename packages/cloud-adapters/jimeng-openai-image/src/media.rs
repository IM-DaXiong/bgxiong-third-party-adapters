use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use bgx_adapter_v2_sdk::Host;
use serde_json::{json, Value};

use super::{port, Failure};

const CHUNK: usize = 16 * 1024;
const MAX_IMAGE_BYTES: usize = 50 * 1024 * 1024;

pub(crate) fn read(host: &mut dyn Host, handle: &str, limit: usize) -> Result<Vec<u8>, Failure> {
    let info = port(host, "host-media.info", json!({"handle":handle}))?;
    let size = info["byteLength"]
        .as_str()
        .and_then(|size| size.parse::<usize>().ok())
        .filter(|size| *size > 0 && *size <= limit)
        .ok_or_else(|| Failure::new("RESOURCE_UNAVAILABLE", "图像网关媒体大小超出可用范围"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| Failure::new("RESOURCE_UNAVAILABLE", "图像网关媒体内存不足"))?;
    while bytes.len() < size {
        let max = (size - bytes.len()).min(CHUNK);
        let part: Vec<u8> = serde_json::from_value(port(
            host,
            "host-media.read",
            json!({
                "handle":handle,"offset":bytes.len().to_string(),"maxBytes":max
            }),
        )?)
        .map_err(|_| Failure::upstream("图像网关媒体读取失败"))?;
        if part.is_empty() || part.len() > max {
            return Err(Failure::upstream("图像网关媒体读取长度无效"));
        }
        bytes.extend(part);
    }
    Ok(bytes)
}

pub(crate) fn put(
    host: &mut dyn Host,
    kind: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<String, Failure> {
    let value = port(
        host,
        "host-media.create",
        json!({"kind":kind,"mimeHint":mime}),
    )?;
    let handle = value
        .as_str()
        .ok_or_else(|| Failure::upstream("图像网关媒体租约缺失"))?;
    for chunk in bytes.chunks(CHUNK) {
        port(
            host,
            "host-media.write",
            json!({"handle":handle,"bytes":chunk}),
        )?;
    }
    port(host, "host-media.finish", json!({"handle":handle}))?;
    Ok(handle.to_string())
}

pub(crate) fn descriptor(
    host: &mut dyn Host,
    handle: &str,
    role: &str,
    kind: &str,
) -> Result<Value, Failure> {
    let value = port(host, "host-media.info", json!({"handle":handle}))?;
    if value["kind"] != kind
        || value["byteLength"]
            .as_str()
            .and_then(|size| size.parse::<u64>().ok())
            .unwrap_or(0)
            == 0
    {
        return Err(Failure::upstream("图像网关返回的实际媒体类型或大小无效"));
    }
    Ok(json!({"kind":"media","role":role,"media":{
        "id":value["id"],"handle":handle,"kind":value["kind"],"mime":value["mime"],
        "fileName":value["fileName"],"byteLength":value["byteLength"]
    }}))
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if bytes.starts_with(b"II\x2a\x00") || bytes.starts_with(b"MM\x00\x2a") {
        return Some("image/tiff");
    }
    None
}

pub(crate) fn attach_references(
    host: &mut dyn Host,
    payload: &Value,
    body: &mut Value,
) -> Result<(), Failure> {
    let Some(references) = payload["input"]["referenceImages"].as_array() else {
        return Ok(());
    };
    let mut images = Vec::with_capacity(references.len());
    let mut remaining = MAX_IMAGE_BYTES;
    for reference in references {
        let handle = payload["media"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|media| media["id"] == *reference && media["kind"] == "image")
            .and_then(|media| media["handle"].as_str())
            .ok_or_else(|| Failure::invalid("图像网关参考图租约缺失"))?;
        let bytes = read(host, handle, remaining)?;
        remaining -= bytes.len();
        let mime = image_mime(&bytes).ok_or_else(|| Failure::invalid("图像网关参考图格式无效"))?;
        images.push(format!("data:{mime};base64,{}", STANDARD.encode(bytes)));
    }
    if images.len() == 1 {
        body["image"] = json!(images.pop().unwrap());
    } else if !images.is_empty() {
        body["image"] = json!(images);
    }
    Ok(())
}

pub(crate) fn inline_image(host: &mut dyn Host, source: &str) -> Result<Value, Failure> {
    let encoded = source
        .strip_prefix("data:")
        .and_then(|source| source.split_once(";base64,"))
        .map_or(source, |(_, encoded)| encoded);
    let encoded: String = encoded
        .chars()
        .filter(|value| !value.is_ascii_whitespace())
        .collect();
    let bytes = if encoded.ends_with('=') {
        STANDARD.decode(&encoded)
    } else {
        STANDARD_NO_PAD.decode(&encoded)
    }
    .map_err(|_| Failure::upstream("图像网关返回的 Base64 无效"))?;
    let mime = image_mime(&bytes).ok_or_else(|| Failure::upstream("图像网关返回的图像格式无效"))?;
    let handle = put(host, "image", mime, &bytes)?;
    descriptor(host, &handle, "images", "image")
}
