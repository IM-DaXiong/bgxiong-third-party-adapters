use alloc::{format, string::String};
use serde_json::Value;

// Platform inputs use the SDK's measured media identity. Container recognition,
// decoding and lease authorization belong to the SDK, not a MIME spelling list.
pub(crate) fn validate(media: &Value, kind: &str) -> Result<(), String> {
    let id = media["id"].as_str().unwrap_or("unknown");
    let invalid = |reason: &str| format!("素材 {id}：{reason}。视频生成尚未提交。");
    if media["kind"].as_str() != Some(kind) {
        return Err(invalid("媒体分类与引用用途不一致"));
    }
    let mime = media["mime"].as_str().unwrap_or("");
    let accepted = match kind {
        "image" => ["image/png", "image/jpeg", "image/webp"].contains(&mime),
        "audio" => mime.strip_prefix("audio/").is_some_and(|subtype| {
            !subtype.is_empty() && subtype.bytes().all(|c| c.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&c))
        }),
        _ => false,
    };
    if !accepted {
        return Err(invalid(&format!("无法用于此操作的媒体类型 {mime}")));
    }
    for (field, reason) in [("handle", "媒体句柄缺失"), ("fileName", "媒体文件名缺失")] {
        if media[field].as_str().is_none_or(|s| s.trim().is_empty()) {
            return Err(invalid(reason));
        }
    }
    if media["byteLength"].as_str().and_then(|s| s.parse::<u64>().ok()).is_none_or(|n| n == 0) {
        return Err(invalid("媒体文件为空或字节长度无效"));
    }
    Ok(())
}
