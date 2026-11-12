use crate::{failure, http, invalid, malformed, port, Mode, Result};
use alloc::{format, string::ToString, vec::Vec};
use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::{Host, Invocation};
use serde_json::{json, Value};

const CHUNK: usize = 48 * 1024; // Multiple of three, so only the final chunk is padded.

fn write(h: &mut dyn Host, handle: &str, bytes: &[u8]) -> Result<()> {
    port(
        h,
        "host-media.write",
        json!({"handle":handle,"bytes":bytes}),
    )?;
    Ok(())
}

fn image_length(stat: &Value) -> Result<usize> {
    let invalid_image =
        || invalid("参考图须为 JPEG 或 PNG，宽高至少 300 像素，比例 2:5 到 5:2，且不超过 30 MB");
    let width = stat["width"]
        .as_u64()
        .filter(|n| (300..=u32::MAX as u64).contains(n))
        .ok_or_else(invalid_image)?;
    let height = stat["height"]
        .as_u64()
        .filter(|n| (300..=u32::MAX as u64).contains(n))
        .ok_or_else(invalid_image)?;
    if stat["kind"] != "image"
        || !matches!(stat["mime"].as_str(), Some("image/jpeg" | "image/png"))
        || width * 5 < height * 2
        || height * 5 < width * 2
    {
        return Err(invalid_image());
    }
    stat["byteLength"]
        .as_str()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| (1..=30_000_000).contains(n))
        .ok_or_else(invalid_image)
}

pub fn body(h: &mut dyn Host, mode: Mode, payload: &Value, external_id: &str) -> Result<Value> {
    let input = &payload["input"];
    let prompt = json!({"type":"prompt","text":input["prompt"]});
    let settings = json!({"duration":input["durationSeconds"],"resolution":input["resolution"],
        "aspect_ratio":input["aspectRatio"],"audio":"native"});
    let options = json!({"external_task_id":external_id,"watermark_info":{"enabled":false}});
    if mode == Mode::Text {
        return Ok(json!({"kind":"bytes","bytes":json!({
            "contents":[prompt],"settings":settings,"options":options
        }).to_string().into_bytes()}));
    }
    let references = input["referenceImages"]
        .as_array()
        .filter(|r| (1..=10).contains(&r.len()))
        .ok_or_else(|| invalid("请选择 1 至 10 张参考图"))?;
    let leases = payload["media"]
        .as_array()
        .ok_or_else(|| invalid("参考图租约缺失"))?;
    let mut images = Vec::new();
    for reference in references {
        let id = reference["mediaId"]
            .as_str()
            .ok_or_else(|| invalid("参考图身份无效"))?;
        let lease = leases
            .iter()
            .find(|media| media["mediaId"] == id || media["id"] == id)
            .ok_or_else(|| invalid("参考图租约缺失"))?;
        let handle = lease["handle"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid("参考图句柄无效"))?;
        let stat = port(h, "host-media.info", json!({"handle":handle}))?;
        if stat["id"] != id
            || stat["byteLength"] != lease["byteLength"]
            || stat["mime"] != lease["mime"]
        {
            return Err(invalid("参考图与冻结媒体不一致"));
        }
        images.push((handle, image_length(&stat)?));
    }
    let handle = port(
        h,
        "host-media.create",
        json!({"kind":"file","mimeHint":"application/octet-stream"}),
    )?
    .as_str()
    .ok_or_else(malformed)?
    .to_string();
    let result = (|| {
        write(h, &handle, format!("{{\"contents\":[{prompt}").as_bytes())?;
        for (index, (image, size)) in images.into_iter().enumerate() {
            write(
                h,
                &handle,
                format!(
                    ",{{\"type\":\"refer_image\",\"id\":\"image_{}\",\"url\":\"",
                    index + 1
                )
                .as_bytes(),
            )?;
            let mut offset = 0;
            while offset < size {
                let count = CHUNK.min(size - offset);
                let bytes: Vec<u8> = serde_json::from_value(port(
                    h,
                    "host-media.read",
                    json!({"handle":image,"offset":offset.to_string(),"maxBytes":count}),
                )?)
                .map_err(|_| malformed())?;
                if bytes.len() != count {
                    return Err(invalid("读取参考图时字节长度发生变化"));
                }
                write(h, &handle, STANDARD.encode(bytes).as_bytes())?;
                offset += count;
            }
            write(h, &handle, b"\"}")?;
        }
        write(
            h,
            &handle,
            format!("],\"settings\":{settings},\"options\":{options}}}").as_bytes(),
        )?;
        port(h, "host-media.finish", json!({"handle":handle}))?;
        Ok(json!({"kind":"media","handle":handle}))
    })();
    if result.is_err() {
        let _ = port(h, "host-media.release", json!({"handle":handle}));
    }
    result
}

pub fn download(h: &mut dyn Host, r: &Invocation, task: &Value) -> Result<Value> {
    let mut videos = task["outputs"]
        .as_array()
        .ok_or_else(malformed)?
        .iter()
        .filter(|row| row["type"] == "video");
    let video = videos
        .next()
        .filter(|_| videos.next().is_none())
        .ok_or_else(malformed)?;
    let url = video["url"].as_str().ok_or_else(malformed)?;
    let origin = url
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next());
    if !matches!(
        origin,
        Some("api-beijing.klingai.com" | "p2-kling.klingai.com")
    ) {
        return Err(failure(
            "PERMISSION_DENIED",
            "云端任务已完成，原片地址超出已核定的下载域名",
        ));
    }
    let response = http(h, r, "GET", url.into(), json!({"kind":"none"}), None)?;
    if response["status"] != 200 {
        return Err(failure(
            "UPSTREAM_ERROR",
            "云端任务已完成，原片下载未完成，可按原任务续查",
        ));
    }
    let handle = response["payload"]["handle"]
        .as_str()
        .ok_or_else(malformed)?;
    let stat = port(h, "host-media.info", json!({"handle":handle}))?;
    let positive = |value: &Value| {
        value
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|n| n > 0)
    };
    if stat["kind"] != "video"
        || !stat["mime"]
            .as_str()
            .is_some_and(|mime| mime.starts_with("video/"))
        || !positive(&stat["byteLength"])
        || !positive(&stat["durationMs"])
        || !stat["width"].as_u64().is_some_and(|n| n > 0)
        || !stat["height"].as_u64().is_some_and(|n| n > 0)
    {
        return Err(failure("UPSTREAM_ERROR", "原片未通过 SDK 视频探测"));
    }
    Ok(json!({"kind":"media","role":"video","media":{
        "id":stat["id"],"kind":"video","handle":handle,"mime":stat["mime"],
        "fileName":stat["fileName"],"byteLength":stat["byteLength"]
    }}))
}
