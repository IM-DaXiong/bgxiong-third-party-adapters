use super::*;

fn storage_origin(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let host = &rest[..end];
    let bucket = host.strip_suffix(".oss-cn-beijing.aliyuncs.com")?;
    (!bucket.is_empty()
        && bucket.starts_with("dashscope-")
        && bucket
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
    .then_some(&url[..8 + end])
}

pub(super) fn result_url(url: &str) -> bool {
    storage_origin(url).is_some_and(|origin| url.as_bytes().get(origin.len()) == Some(&b'/'))
}

pub(super) fn temporary(
    host: &mut dyn Host,
    origin: &str,
    key: &str,
    model: &str,
    wav: &[u8],
    budget: u64,
) -> Result<String, Failure> {
    let reply = http::exchange(
        host,
        "GET",
        &format!("{origin}/api/v1/uploads?action=getPolicy&model={model}"),
        Some(key),
        json!([]),
        json!({"kind":"none"}),
        budget,
        false,
    )?;
    let policy = &reply["data"];
    let invalid = || Failure::new("UPSTREAM_ERROR", "百炼上传策略无效");
    let upload = policy["upload_host"]
        .as_str()
        .filter(|url| storage_origin(url) == Some(*url))
        .ok_or_else(invalid)?;
    let directory = policy["upload_dir"]
        .as_str()
        .filter(|value| {
            !value.starts_with('/')
                && value
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'/' | b'.'))
        })
        .ok_or_else(invalid)?;
    let max = policy["max_file_size_mb"]
        .as_u64()
        .and_then(|mb| mb.checked_mul(1_000_000))
        .ok_or_else(invalid)?;
    if wav.len() as u64 > max {
        return Err(Failure::new("INVALID_INPUT", "百炼上传策略音频超限"));
    }
    // The policy's directory is per upload. Use the file digest as a safe file name.
    let object = format!("{directory}/{:x}.wav", Sha256::digest(wav));
    let fields = [
        ("key", object.as_str()),
        ("policy", policy["policy"].as_str().ok_or_else(invalid)?),
        (
            "Signature",
            policy["signature"].as_str().ok_or_else(invalid)?,
        ),
        (
            "OSSAccessKeyId",
            policy["oss_access_key_id"].as_str().ok_or_else(invalid)?,
        ),
        (
            "x-oss-object-acl",
            policy["x_oss_object_acl"].as_str().ok_or_else(invalid)?,
        ),
        (
            "x-oss-forbid-overwrite",
            policy["x_oss_forbid_overwrite"]
                .as_str()
                .ok_or_else(invalid)?,
        ),
        ("success_action_status", "200"),
    ];
    let form = multipart(wav, &fields)?;
    let boundary = boundary(wav);
    let handle = http::put_media(host, "file", "application/octet-stream", &form)?;
    // Host must authorize the exact OSS origin independently of the API credential.
    let response = http::port(
        host,
        "host-http.execute",
        json!({"plan":{
        "method":"POST","url":upload,"headers":[{"name":"Content-Type","value":format!("multipart/form-data; boundary={boundary}")}],
        "generatedHeaders":[],"body":{"kind":"media","handle":handle},"sink":{"kind":"media"},"timeoutMs":budget
    },"credential":null}),
    )?;
    if !matches!(response["status"].as_u64(), Some(200 | 204)) {
        return Err(Failure::new("UPSTREAM_ERROR", "百炼临时音频上传失败"));
    }
    Ok(format!("oss://{object}"))
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
