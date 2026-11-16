use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::Host;
use serde_json::{json, Value};

use crate::{
    parameters::{Mode, Operation},
    provider::port,
    seededit_image, Failure,
};

const CHUNK: usize = 16 * 1024;

pub(crate) fn read(host: &mut dyn Host, handle: &str, limit: usize) -> Result<Vec<u8>, Failure> {
    let info = port(host, "host-media.info", json!({"handle":handle}))?;
    read_known(host, handle, &info, limit)
}

fn read_known(
    host: &mut dyn Host,
    handle: &str,
    info: &Value,
    limit: usize,
) -> Result<Vec<u8>, Failure> {
    let length = info["byteLength"]
        .as_str()
        .and_then(|size| size.parse::<usize>().ok())
        .filter(|size| (1..=limit).contains(size))
        .ok_or_else(|| {
            Failure::new("RESOURCE_UNAVAILABLE", "火山视觉媒体长度超过请求限额或为空")
        })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| Failure::new("RESOURCE_UNAVAILABLE", "火山视觉媒体缓冲区无法分配"))?;
    while bytes.len() < length {
        let count = (length - bytes.len()).min(CHUNK);
        let raw = port(
            host,
            "host-media.read",
            json!({"handle":handle,"offset":bytes.len().to_string(),"maxBytes":count}),
        )?;
        let block: Vec<u8> = serde_json::from_value(raw)
            .map_err(|_| Failure::new("UPSTREAM_ERROR", "火山视觉媒体读取未返回字节"))?;
        if block.is_empty() || block.len() > count {
            return Err(Failure::new(
                "UPSTREAM_ERROR",
                "火山视觉媒体与租约长度不一致",
            ));
        }
        bytes.extend(block);
    }
    Ok(bytes)
}

pub(crate) fn provider_images(
    host: &mut dyn Host,
    operation: &Operation,
    payload: &Value,
) -> Result<Vec<String>, Failure> {
    let ids = operation
        .image_ids(&payload["input"])
        .map_err(|(_, message)| Failure::new("INVALID_INPUT", message))?;
    let mut measured = Vec::new();
    let mut sources = Vec::new();
    for id in ids {
        let handle = payload["media"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|media| media["id"] == id && media["kind"] == "image")
            .and_then(|media| media["handle"].as_str())
            .ok_or_else(|| Failure::new("INVALID_INPUT", "火山视觉图片角色缺少对应租约"))?;
        let mut info = port(host, "host-media.info", json!({"handle":handle}))?;
        info["mediaId"] = json!(id);
        sources.push(handle);
        measured.push(info);
    }
    operation
        .validate_metadata(&payload["input"], &json!(measured))
        .map_err(|(_, message)| Failure::new("INVALID_INPUT", message))?;
    let limit = match operation.mode {
        Mode::SeedEdit => 24 * 1024 * 1024,
        Mode::Portrait => 5_000_000,
        _ => 4_700_000,
    };
    sources
        .into_iter()
        .zip(measured)
        .map(|(handle, info)| {
            let source = read_known(host, handle, &info, limit)?;
            if operation.mode == Mode::SeedEdit {
                Ok(STANDARD.encode(seededit_image::normalized_jpeg(&source)?))
            } else {
                Ok(STANDARD.encode(source))
            }
        })
        .collect()
}

pub(crate) fn inline_image(host: &mut dyn Host, encoded: &str) -> Result<Value, Failure> {
    let text = encoded
        .split_once("base64,")
        .map_or(encoded, |(_, payload)| payload);
    let bytes = STANDARD
        .decode(text)
        .map_err(|_| Failure::new("UPSTREAM_ERROR", "火山视觉输出图片的 Base64 无效"))?;
    let handle = write(host, &bytes, "image")?;
    output(host, &handle, "image", "image")
}

pub(crate) fn write(host: &mut dyn Host, bytes: &[u8], kind: &str) -> Result<String, Failure> {
    let raw = port(
        host,
        "host-media.create",
        json!({"kind":kind,"mimeHint":"application/octet-stream"}),
    )?;
    let handle = raw
        .as_str()
        .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "火山视觉无法创建媒体租约"))?;
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

pub(crate) fn output(
    host: &mut dyn Host,
    handle: &str,
    role: &str,
    kind: &str,
) -> Result<Value, Failure> {
    let info = port(host, "host-media.info", json!({"handle":handle}))?;
    if info["kind"] != kind {
        return Err(Failure::new(
            "UPSTREAM_ERROR",
            "火山视觉原件类型与包声明不一致",
        ));
    }
    Ok(
        json!({"kind":"media","role":role,"media":{"id":info["id"],"handle":handle,"kind":kind,
        "mime":info["mime"],"fileName":info["fileName"],"byteLength":info["byteLength"]}}),
    )
}

pub(crate) fn download(
    host: &mut dyn Host,
    url: &str,
    kind: &str,
    budget: u64,
) -> Result<Value, Failure> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(Failure::new(
            "UPSTREAM_ERROR",
            "火山视觉没有可读取的原件地址",
        ));
    }
    let response = port(
        host,
        "host-http.execute",
        json!({"plan":{
        "method":"GET","url":url,"body":{"kind":"none"},"headers":[],"generatedHeaders":[],
        "sink":{"kind":"media"},"timeoutMs":budget},"credential":null}),
    )?;
    let handle = response["payload"]["handle"]
        .as_str()
        .filter(|_| response["status"] == 200)
        .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "火山视觉原件下载未完成"))?;
    output(host, handle, kind, kind)
}
