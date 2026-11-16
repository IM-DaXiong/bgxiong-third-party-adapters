use alloc::{
    format,
    string::{String, ToString},
};
use bgx_adapter_v2_sdk::{host, Host};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::{media, parameters::Operation, recovery, sha256, Failure};

const API_VERSION: &str = "2022-08-31";
const REGION: &str = "cn-north-1";
const SERVICE: &str = "cv";
const SIGNED_HEADERS: &str = "content-type;host;x-content-sha256;x-date";
const RESPONSE_LIMIT: usize = 16 * 1024 * 1024;

pub(crate) fn port(host: &mut dyn Host, method: &str, args: Value) -> Result<Value, Failure> {
    host::call(host, method, &args).map_err(|error| {
        let raw = serde_json::to_value(error).unwrap_or(Value::Null);
        let code = match raw["code"].as_str() {
            Some("PERMISSION_DENIED") => "PERMISSION_DENIED",
            Some("INVALID_INPUT") => "INVALID_INPUT",
            Some("RESOURCE_UNAVAILABLE") => "RESOURCE_UNAVAILABLE",
            Some("BUDGET_EXCEEDED") => "BUDGET_EXCEEDED",
            Some("TIMEOUT") => "TIMEOUT",
            _ => "UPSTREAM_ERROR",
        };
        Failure::new(code, "火山智能视觉宿主请求未完成")
    })
}

pub(crate) fn endpoint(config: &Value) -> Option<(&str, &str)> {
    let base = config["endpointBaseUrl"]
        .as_str()?
        .trim()
        .trim_end_matches('/');
    let (scheme, authority) = base.split_once("://")?;
    if !matches!(scheme, "https" | "http")
        || authority.is_empty()
        || authority.contains(['/', '@', '?', '#', '\\'])
        || authority.chars().any(char::is_whitespace)
    {
        return None;
    }
    let (name, port) = authority
        .split_once(':')
        .map_or((authority, None), |(name, port)| (name, Some(port)));
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.'))
        || port.is_some_and(|port| port.parse::<u16>().ok().is_none_or(|n| n == 0))
        || (scheme == "http" && !matches!(name, "localhost" | "127.0.0.1"))
    {
        return None;
    }
    let canonical_host = if matches!(
        (scheme, port),
        ("http", Some("80")) | ("https", Some("443"))
    ) {
        name
    } else {
        authority
    };
    Some((base, canonical_host))
}

fn sign(
    host: &mut dyn Host,
    credential: &str,
    query: &str,
    authority: &str,
    hash: &str,
) -> Result<(String, String), Failure> {
    let error = || Failure::new("UNAUTHORIZED", "火山智能视觉的签名凭据未就绪");
    let ask = |host: &mut dyn Host, value: Value| {
        port(
            host,
            "host-credential.sign",
            json!({"handle":credential,"specJson":value.to_string()}),
        )
    };
    let seconds = ask(host, json!({"version":1,"kind":"credential-utc-seconds"}))?;
    let timestamp = seconds
        .as_str()
        .and_then(|value| value.parse::<i64>().ok())
        .and_then(|value| DateTime::<Utc>::from_timestamp(value, 0))
        .ok_or_else(error)?;
    let date = timestamp.format("%Y%m%dT%H%M%SZ").to_string();
    let day = timestamp.format("%Y%m%d").to_string();
    let scope = format!("{day}/{REGION}/{SERVICE}/request");
    let canonical_request = format!(
        "POST\n/\n{query}\ncontent-type:application/json\nhost:{authority}\nx-content-sha256:{hash}\nx-date:{date}\n\n{SIGNED_HEADERS}\n{hash}"
    );
    let canonical = format!(
        "HMAC-SHA256\n{date}\n{scope}\n{}",
        sha256(canonical_request.as_bytes())
    );
    let template = format!(
        "HMAC-SHA256 Credential={{publicId}}/{scope}, SignedHeaders={SIGNED_HEADERS}, Signature={{hexHmacOfCanonical}}"
    );
    let value = ask(
        host,
        json!({"version":1,"kind":"hmac-sha256-credential","canonical":canonical,
        "date":date,"derivation":{"messages":[day,REGION,SERVICE,"request"],"authorizationTemplate":template}}),
    )?;
    let authorization = value
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or_else(error)?;
    Ok((date, authorization.into()))
}

fn vendor_info(raw: &Value, mut failure: Failure) -> Failure {
    failure.vendor_code = raw
        .pointer("/ResponseMetadata/Error/Code")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| {
            raw.get("code")
                .filter(|value| value.is_number())
                .map(Value::to_string)
        });
    failure.request_id = raw
        .pointer("/ResponseMetadata/RequestId")
        .or_else(|| raw.get("request_id"))
        .and_then(Value::as_str)
        .map(String::from);
    failure
}

pub(crate) fn exchange(
    host: &mut dyn Host,
    config: &Value,
    action: &str,
    body: &Value,
    submitting: bool,
    budget: u64,
) -> Result<(Value, String), Failure> {
    let (base, authority) = endpoint(config)
        .ok_or_else(|| Failure::new("INVALID_INPUT", "火山智能视觉的 API 源站无效"))?;
    let credential = config["credentialRef"]
        .as_str()
        .filter(|reference| !reference.is_empty())
        .ok_or_else(|| Failure::new("UNAUTHORIZED", "火山智能视觉需要 AK/SK 凭据引用"))?;
    let query = format!("Action={action}&Version={API_VERSION}");
    let bytes = serde_json::to_vec(body)
        .map_err(|_| Failure::new("INVALID_INPUT", "火山智能视觉请求无法序列化"))?;
    let hash = sha256(&bytes);
    let (date, authorization) = sign(host, credential, &query, authority, &hash)?;
    let handle = media::write(host, &bytes, "file")?;
    let ambiguous = || Failure::new("ACCEPTANCE_UNKNOWN", "未收到火山智能视觉的可确认受理回执");
    let malformed = || {
        if submitting {
            ambiguous()
        } else {
            Failure::new("UPSTREAM_ERROR", "火山智能视觉查询回执无效")
        }
    };
    let response = port(host, "host-http.execute", json!({"plan":{
        "method":"POST","url":format!("{base}/?{query}"),"headers":[
            {"name":"Content-Type","value":"application/json"},{"name":"X-Date","value":date},
            {"name":"X-Content-Sha256","value":hash},{"name":"Authorization","value":authorization}],
        "generatedHeaders":[],"body":{"kind":"media","handle":handle},
        "sink":{"kind":"media"},"timeoutMs":budget},"credential":credential}))
        .map_err(|error| if submitting && matches!(
            error.code, "UPSTREAM_ERROR" | "TIMEOUT" | "RESOURCE_UNAVAILABLE"
        ) {
            ambiguous()
        } else { error })?;
    let status = response["status"].as_u64().ok_or_else(malformed)?;
    let response_handle = response["payload"]["handle"].as_str();
    let decode = |host: &mut dyn Host, handle: &str| {
        media::read(host, handle, RESPONSE_LIMIT)
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).map_err(|_| malformed()))
    };
    if !(200..=299).contains(&status) {
        let raw = response_handle
            .and_then(|handle| decode(host, handle).ok())
            .unwrap_or(Value::Null);
        let code = match status {
            400 | 404 | 422 => "INVALID_INPUT",
            401 | 403 => "UNAUTHORIZED",
            402 => "RESOURCE_UNAVAILABLE",
            429 => "RATE_LIMITED",
            408 | 500..=599 if submitting => "ACCEPTANCE_UNKNOWN",
            _ => "UPSTREAM_ERROR",
        };
        return Err(vendor_info(
            &raw,
            Failure::new(code, "火山智能视觉返回 HTTP 错误"),
        ));
    }
    let handle = response_handle.ok_or_else(malformed)?;
    let raw = decode(host, handle).map_err(|_| malformed())?;
    if raw
        .pointer("/ResponseMetadata/Error")
        .is_some_and(|error| !error.is_null())
        || raw
            .get("code")
            .and_then(Value::as_i64)
            .is_some_and(|code| code != 10000)
    {
        let mapped = match raw.get("code").and_then(Value::as_i64) {
            Some(50429 | 50500) => "RATE_LIMITED",
            Some(50413) => "INVALID_INPUT",
            _ if submitting && recovery::task_id(&raw).is_some() => "ACCEPTANCE_UNKNOWN",
            _ => "UPSTREAM_ERROR",
        };
        // A completed query can include a business failure with a durable task status.
        if submitting || recovery::provider_status(&raw).is_empty() {
            return Err(vendor_info(
                &raw,
                Failure::new(mapped, "火山智能视觉拒绝请求"),
            ));
        }
    }
    Ok((raw, handle.into()))
}

pub(crate) fn submit(
    host: &mut dyn Host,
    operation: &Operation,
    payload: &Value,
    budget: u64,
) -> Result<Value, Failure> {
    let frozen = operation
        .material(&payload["input"])
        .map_err(|(_, message)| Failure::new("INVALID_INPUT", message))?;
    if payload["preparation"]["material"] != frozen {
        return Err(Failure::new(
            "INVALID_INPUT",
            "火山智能视觉提交与冻结预检不一致",
        ));
    }
    let mut body = frozen["body"].clone();
    let images = media::provider_images(host, operation, payload)?;
    if !images.is_empty() {
        body["binary_data_base64"] = json!(images);
    }
    let (raw, _) = exchange(
        host,
        &payload["configuration"],
        operation.mode.submit_action(),
        &body,
        true,
        budget,
    )?;
    let id = recovery::task_id(&raw)
        .ok_or_else(|| Failure::new("ACCEPTANCE_UNKNOWN", "火山智能视觉受理回执缺少任务号"))?;
    Ok(json!({"kind":"accepted","vendorTaskId":id,
        "resumeToken":{"taskId":id,"requestKey":operation.request_key},
        "appliedParams":null,"usage":recovery::usage(&raw)}))
}
