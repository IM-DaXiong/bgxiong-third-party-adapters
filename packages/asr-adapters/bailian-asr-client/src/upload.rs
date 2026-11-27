use super::*;

const FILES: &str = "/compatible-mode/v1/files";
const FILE_ORIGIN: &str = "dashscope-file-mgr.oss-cn-beijing.aliyuncs.com";

pub(super) fn file_id(value: &Value) -> Option<&str> {
    value.as_str().filter(|id| {
        id.starts_with("file-fe-")
            && id.len() <= 64
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    })
}

pub(super) fn result_url(url: &str) -> bool {
    let Some((_, path)) = url
        .strip_prefix("https://dashscope-result-bj.oss-cn-beijing.aliyuncs.com")
        .and_then(|rest| rest.split_once('/').filter(|(before, _)| before.is_empty()))
    else {
        return false;
    };
    !path.is_empty()
        && !url
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ' || byte == b'\\')
}

pub(super) fn create(
    host: &mut dyn Host,
    origin: &str,
    key: &str,
    audio: &Audio<'_>,
    budget: u64,
) -> Result<String, Failure> {
    let reply = http::exchange(
        host,
        "POST",
        &format!("{origin}{FILES}"),
        Some(key),
        json!([]),
        json!({"kind":"multipart","parts":[
            {"kind":"field","name":"purpose","value":"file-extract"},
            {"kind":"file","name":"file","fileName":"audio.wav","mime":audio.mime,"handle":audio.handle}
        ]}),
        budget,
        false,
    )?;
    file_id(&reply["id"])
        .map(String::from)
        .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "百炼文件上传缺少文件编号"))
}

pub(super) fn signed_source_url(
    host: &mut dyn Host,
    origin: &str,
    key: &str,
    id: &str,
    budget: u64,
) -> Result<String, Failure> {
    let reply = http::exchange(
        host,
        "GET",
        &format!("{origin}/api/v1/files/{id}"),
        Some(key),
        json!([]),
        json!({"kind":"none"}),
        budget,
        false,
    )?;
    let rejected = || Failure::new("UPSTREAM_ERROR", "百炼文件缺少有效签名下载地址");
    let url = reply["data"]["url"].as_str().ok_or_else(rejected)?;
    let tail = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(rejected)?;
    let (hostname, path) = tail.split_once('/').ok_or_else(rejected)?;
    if hostname != FILE_ORIGIN
        || !path.contains('?')
        || path
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b' ' | b'\\' | b'#'))
    {
        return Err(rejected());
    }
    // The file metadata API can spell the signed link with HTTP. OSS signs the
    // object and query parameters, and the same address is available over HTTPS.
    Ok(format!("https://{hostname}/{path}"))
}

pub(super) fn delete(
    host: &mut dyn Host,
    origin: &str,
    key: &str,
    id: &str,
    budget: u64,
) -> Result<(), Failure> {
    let reply = http::exchange(
        host,
        "DELETE",
        &format!("{origin}{FILES}/{id}"),
        Some(key),
        json!([]),
        json!({"kind":"none"}),
        budget,
        false,
    )?;
    if reply.is_null() || (reply["deleted"] == true && reply["id"] == id) {
        return Ok(());
    }
    Err(Failure::new("UPSTREAM_ERROR", "百炼已上传音频删除未确认"))
}
