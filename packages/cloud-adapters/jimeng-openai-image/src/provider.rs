use alloc::{string::ToString, vec::Vec};
use bgx_adapter_v2_sdk::{Host, Invocation};
use serde_json::{json, Value};
use url::Url;

use super::{media, parameters, port, Failure};

const MAX_RESPONSE_BYTES: usize = 50 * 1024 * 1024;

fn request(
    host: &mut dyn Host,
    method: &str,
    url: &str,
    body: Value,
    credential: Option<&str>,
    budget: u64,
) -> Result<Value, Failure> {
    let headers = if method == "POST" {
        json!([{"name":"Content-Type","value":"application/json"}])
    } else {
        json!([])
    };
    let generated = if credential.is_some() {
        json!([{"name":"Authorization","kind":"bearer","targetField":"Authorization"}])
    } else {
        json!([])
    };
    port(
        host,
        "host-http.execute",
        json!({"plan":{
        "method":method,"url":url,"headers":headers,"generatedHeaders":generated,
        "body":body,"sink":{"kind":"media"},"timeoutMs":budget
    },"credential":credential}),
    )
}

fn output_url(host: &mut dyn Host, url: &str, budget: u64) -> Result<Value, Failure> {
    let valid = Url::parse(url).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    });
    if !valid {
        return Err(Failure::upstream("图像网关返回的成果地址无效"));
    }
    let response = request(host, "GET", url, json!({"kind":"none"}), None, budget)?;
    let handle = response["payload"]["handle"]
        .as_str()
        .filter(|_| response["status"] == 200)
        .ok_or_else(|| Failure::upstream("图像网关原始成果下载失败"))?;
    media::descriptor(host, handle, "images", "image")
}

fn emit(
    host: &mut dyn Host,
    invocation: &Invocation,
    sequence: &mut u64,
    kind: &str,
    value: Value,
) -> Result<(), Failure> {
    *sequence += 1;
    port(
        host,
        "host-stream.emit-event",
        json!({"event":{"requestId":invocation.r#request_id,
        "sequence":*sequence,"type":kind,"payload":value}}),
    )?;
    Ok(())
}

fn emit_media(
    host: &mut dyn Host,
    invocation: &Invocation,
    sequence: &mut u64,
    value: &Value,
) -> Result<(), Failure> {
    emit(
        host,
        invocation,
        sequence,
        "media_chunk",
        json!({"role":value["role"],"media":value["media"]}),
    )
}

fn usage(response: &Value) -> Value {
    let measurements: Vec<_> = [
        ("input_tokens", "input_tokens", "tokens"),
        ("output_tokens", "output_tokens", "tokens"),
        ("total_tokens", "total_tokens", "tokens"),
        (
            "image_count",
            "local.bgx.jimeng-openai-image/image_count",
            "local.bgx.jimeng-openai-image/images",
        ),
    ]
    .into_iter()
    .filter_map(|(key, metric, unit)| {
        response["usage"][key].as_u64().map(|amount|
            json!({"metric":metric,"unit":unit,"amount":amount.to_string(),"source":"reported"}))
    })
    .collect();
    if measurements.is_empty() {
        Value::Null
    } else {
        json!({"measurements":measurements})
    }
}

fn error_from_response(status: u64, response: &Value) -> Option<Failure> {
    let error = response.get("error").filter(|value| !value.is_null());
    if (200..300).contains(&status) && error.is_none() {
        return None;
    }
    let vendor = error.and_then(|value| value["code"].as_str());
    let balance = vendor.is_some_and(|code| {
        let code = code.to_ascii_lowercase();
        ["quota", "balance", "billing"]
            .iter()
            .any(|part| code.contains(part))
    });
    let code = match status {
        402 => "RESOURCE_UNAVAILABLE",
        _ if balance => "RESOURCE_UNAVAILABLE",
        401 | 403 => "UNAUTHORIZED",
        429 => "RATE_LIMITED",
        400 | 404 | 422 => "INVALID_INPUT",
        500..=599 => "ACCEPTANCE_UNKNOWN",
        _ => "UPSTREAM_ERROR",
    };
    let mut failure = Failure::new(code, "图像网关返回错误");
    failure.vendor = vendor.map(ToString::to_string);
    Some(failure)
}

fn references(response: &Value) -> Result<(Vec<&str>, Vec<&str>), Failure> {
    let rows = response["data"]
        .as_array()
        .ok_or_else(|| Failure::upstream("图像网关响应缺少 data 数组"))?;
    let mut urls = Vec::new();
    let mut inline = Vec::new();
    for item in rows {
        if let Some(url) = item["url"].as_str().or_else(|| item["image_url"].as_str()) {
            urls.push(url);
        } else if let Some(values) = item["urls"].as_array() {
            urls.extend(values.iter().filter_map(Value::as_str));
        }
        if let Some(base64) = item["b64_json"]
            .as_str()
            .or_else(|| item["b64"].as_str())
            .or_else(|| item["image_base64"].as_str())
        {
            inline.push(base64);
        }
    }
    urls = urls
        .into_iter()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect();
    inline = inline
        .into_iter()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect();
    if urls.is_empty() && inline.is_empty() {
        return Err(Failure::upstream("图像网关响应无可读取的图像"));
    }
    Ok((urls, inline))
}

pub(crate) fn submit(
    host: &mut dyn Host,
    invocation: &Invocation,
    payload: &Value,
) -> Result<Value, Failure> {
    let config = &payload["configuration"];
    let mut body = parameters::material(payload, &payload["input"], config)
        .map_err(|(_, message)| Failure::invalid(message))?;
    if payload["preparation"]["material"] != body {
        return Err(Failure::invalid("图像网关请求与冻结预检不一致"));
    }
    let endpoint = config["endpointBaseUrl"]
        .as_str()
        .and_then(parameters::endpoint)
        .ok_or_else(|| Failure::invalid("图像网关地址无效"))?;
    media::attach_references(host, payload, &mut body)?;
    let request_bytes =
        serde_json::to_vec(&body).map_err(|_| Failure::invalid("图像网关请求编码失败"))?;
    let model = body["model"].take();
    drop(body);
    let request_handle = media::put(host, "file", "application/octet-stream", &request_bytes)?;
    drop(request_bytes);
    let budget = invocation.r#budget_ms;
    let reply = request(
        host,
        "POST",
        &endpoint,
        json!({"kind":"media","handle":request_handle}),
        config["credentialRef"].as_str(),
        budget,
    )
    .map_err(|error| {
        if matches!(
            error.code,
            "UPSTREAM_ERROR" | "TIMEOUT" | "RESOURCE_UNAVAILABLE"
        ) {
            Failure::new("ACCEPTANCE_UNKNOWN", "图像网关的原提交结果未确认")
        } else {
            error
        }
    })?;
    let status = reply["status"]
        .as_u64()
        .ok_or_else(|| Failure::new("ACCEPTANCE_UNKNOWN", "图像网关原始提交状态缺失"))?;
    let missing_body = || {
        error_from_response(status, &Value::Null)
            .unwrap_or_else(|| Failure::new("ACCEPTANCE_UNKNOWN", "图像网关原始提交响应缺失"))
    };
    let handle = reply["payload"]["handle"]
        .as_str()
        .ok_or_else(missing_body)?;
    let mut sequence = 0;
    let raw =
        media::descriptor(host, handle, "provider-response", "file").map_err(|_| missing_body())?;
    emit_media(host, invocation, &mut sequence, &raw)?;
    let bytes = media::read(host, handle, MAX_RESPONSE_BYTES)?;
    let response = serde_json::from_slice::<Value>(&bytes);
    drop(bytes);
    if let Some(failure) = error_from_response(status, response.as_ref().unwrap_or(&Value::Null)) {
        return Err(with_request_id(failure, response.as_ref().ok()));
    }
    let response =
        response.map_err(|_| Failure::new("ACCEPTANCE_UNKNOWN", "图像网关原始提交响应无法识别"))?;
    let (urls, inline) =
        references(&response).map_err(|error| with_request_id(error, Some(&response)))?;
    let usage = usage(&response);
    if !usage.is_null() {
        emit(host, invocation, &mut sequence, "usage", usage.clone())?;
    }
    let mut outputs = Vec::new();
    let mut first_failure = None;
    for (source, encoded) in urls
        .into_iter()
        .map(|value| (value, false))
        .chain(inline.into_iter().map(|value| (value, true)))
    {
        let result = if encoded {
            media::inline_image(host, source)
        } else {
            output_url(host, source, budget)
        };
        match result {
            Ok(media) => {
                emit_media(host, invocation, &mut sequence, &media)?;
                outputs.push(media);
            }
            Err(error) => {
                first_failure.get_or_insert(error);
            }
        }
    }
    if let Some(error) = first_failure {
        return Err(with_request_id(error, Some(&response)));
    }
    let image_count = outputs.len();
    outputs.push(raw);
    Ok(
        json!({"kind":"completed","vendorTaskId":null,"appliedParams":null,"outputs":outputs,
        "data":{"model":model,"providerRequestId":request_id(&response),"imageCount":image_count},"usage":usage}),
    )
}

fn request_id(response: &Value) -> Option<&str> {
    response["request_id"]
        .as_str()
        .or_else(|| response["requestId"].as_str())
}

fn with_request_id(mut error: Failure, response: Option<&Value>) -> Failure {
    error.request = response.and_then(request_id).map(ToString::to_string);
    error
}

#[cfg(test)]
mod tests;
