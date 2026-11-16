use alloc::{string::ToString, vec::Vec};
use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::Host;
use serde_json::{json, Value};

use crate::{invalid, malformed, port, Result};

use super::ImageMode;

const IMAGE_MAX_BYTES: usize = 10_000_000;
// Call limits allow 48 KiB and only the final block may need Base64 padding.
const BLOCK: usize = 48 * 1024;

fn measured_reference<'a>(
    host: &mut dyn Host,
    payload: &'a Value,
    id: &str,
) -> Result<(&'a str, usize)> {
    let lease = payload["media"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|lease| lease["id"] == id)
        .ok_or_else(|| invalid("可灵参考图租约缺失"))?;
    let handle = lease["handle"]
        .as_str()
        .filter(|handle| !handle.is_empty())
        .ok_or_else(|| invalid("可灵参考图租约缺失"))?;
    let facts = port(host, "host-media.info", json!({"handle":handle}))?;
    if facts["id"] != id
        || facts["byteLength"] != lease["byteLength"]
        || facts["mime"] != lease["mime"]
    {
        return Err(invalid("可灵参考图租约与冻结素材不一致"));
    }
    let unsupported = || {
        invalid(
            "可灵参考图须为 JPEG 或 PNG，宽高至少 300 像素，画幅为 2:5 到 5:2，文件不超过 10 MB",
        )
    };
    let width = facts["width"]
        .as_u64()
        .filter(|n| (300..=u32::MAX as u64).contains(n))
        .ok_or_else(unsupported)?;
    let height = facts["height"]
        .as_u64()
        .filter(|n| (300..=u32::MAX as u64).contains(n))
        .ok_or_else(unsupported)?;
    if facts["kind"] != "image"
        || !matches!(facts["mime"].as_str(), Some("image/jpeg" | "image/png"))
        || width * 5 < height * 2
        || height * 5 < width * 2
    {
        return Err(unsupported());
    }
    let length = facts["byteLength"]
        .as_str()
        .and_then(|length| length.parse::<usize>().ok())
        .filter(|length| (1..=IMAGE_MAX_BYTES).contains(length))
        .ok_or_else(unsupported)?;
    Ok((handle, length))
}

fn write(host: &mut dyn Host, handle: &str, bytes: &[u8]) -> Result<()> {
    port(
        host,
        "host-media.write",
        json!({"handle":handle,"bytes":bytes}),
    )?;
    Ok(())
}

fn append_base64(host: &mut dyn Host, target: &str, source: &str, length: usize) -> Result<()> {
    let mut offset = 0;
    while offset < length {
        let count = BLOCK.min(length - offset);
        let bytes: Vec<u8> = serde_json::from_value(port(
            host,
            "host-media.read",
            json!({
                "handle":source,"offset":offset.to_string(),"maxBytes":count
            }),
        )?)
        .map_err(|_| malformed())?;
        if bytes.len() != count {
            return Err(invalid("可灵参考图实际长度与冻结租约不一致"));
        }
        write(host, target, STANDARD.encode(&bytes).as_bytes())?;
        offset += count;
    }
    Ok(())
}

pub(super) fn request_body(
    host: &mut dyn Host,
    mode: ImageMode,
    payload: &Value,
    body: &Value,
) -> Result<Value> {
    if mode.reference_limit() == 0 {
        return Ok(json!({"kind":"bytes","bytes":body.to_string().into_bytes()}));
    }
    let slots = payload["input"]["referenceImages"]
        .as_array()
        .ok_or_else(|| invalid("可灵参考图槽缺失"))?;
    let mut references = Vec::with_capacity(slots.len());
    for reference in slots {
        let id = reference
            .as_str()
            .ok_or_else(|| invalid("可灵参考图身份无效"))?;
        references.push(measured_reference(host, payload, id)?);
    }
    let raw = port(
        host,
        "host-media.create",
        json!({"kind":"file","mimeHint":"application/octet-stream"}),
    )?;
    let handle = raw.as_str().ok_or_else(malformed)?;
    let result = (|| {
        let json = body.to_string();
        let prefix = json.strip_suffix('}').ok_or_else(malformed)?;
        write(host, handle, prefix.as_bytes())?;
        if mode.is_omni() {
            write(host, handle, b",\"image_list\":[")?;
            for (index, (source, length)) in references.into_iter().enumerate() {
                if index > 0 {
                    write(host, handle, b",")?;
                }
                write(host, handle, b"{\"image\":\"")?;
                append_base64(host, handle, source, length)?;
                write(host, handle, b"\"}")?;
            }
            write(host, handle, b"]}")?;
        } else {
            let (source, length) = references.first().ok_or_else(malformed)?;
            write(host, handle, b",\"image\":\"")?;
            append_base64(host, handle, source, *length)?;
            write(host, handle, b"\"}")?;
        }
        port(host, "host-media.finish", json!({"handle":handle}))?;
        Ok(json!({"kind":"media","handle":handle}))
    })();
    if result.is_err() {
        let _ = port(host, "host-media.release", json!({"handle":handle}));
    }
    result
}
