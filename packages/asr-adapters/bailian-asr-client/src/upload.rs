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
    wav: &[u8],
    budget: u64,
) -> Result<String, Failure> {
    let form = multipart(wav, &[("purpose", "file-extract")])?;
    let boundary = boundary(wav);
    let handle = http::put_media(host, "file", "application/octet-stream", &form)?;
    let reply = http::exchange(
        host,
        "POST",
        &format!("{origin}{FILES}"),
        Some(key),
        json!([{"name":"Content-Type","value":format!("multipart/form-data; boundary={boundary}")}]),
        json!({"kind":"media","handle":handle}),
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

fn boundary(wav: &[u8]) -> String {
    format!("bgx-{:x}", Sha256::digest(wav))
}

pub(super) fn multipart(wav: &[u8], fields: &[(&str, &str)]) -> Result<Vec<u8>, Failure> {
    let boundary = boundary(wav);
    if wav
        .windows(boundary.len())
        .any(|part| part == boundary.as_bytes())
        || fields.iter().any(|(_, value)| {
            value.len() > 16384 || value.contains(['\r', '\n']) || value.contains(&boundary)
        })
    {
        return Err(Failure::new("INVALID_INPUT", "百炼上传表单无效"));
    }
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n").as_bytes());
    body.extend_from_slice(wav);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Ok(body)
}
