use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use bgx_adapter_v2_sdk::Host;
use serde_json::{json, Value};

use crate::{media, parameters::Operation, provider, Failure};

pub(crate) fn task_id(raw: &Value) -> Option<String> {
    [
        "/data/task_id",
        "/data/data/task_id",
        "/Result/task_id",
        "/Result/data/task_id",
        "/Result/data/data/task_id",
        "/task_id",
    ]
    .into_iter()
    .find_map(|path| {
        let id = raw.pointer(path)?;
        if let Some(text) = id.as_str() {
            (!text.is_empty() && text.trim() == text).then(|| text.into())
        } else if id.as_u64().is_some() {
            Some(id.to_string())
        } else {
            None
        }
    })
}

pub(crate) fn provider_status(raw: &Value) -> &str {
    [
        "/data/status",
        "/Result/data/status",
        "/data/data/status",
        "/Result/data/data/status",
        "/status",
    ]
    .into_iter()
    .find_map(|path| raw.pointer(path).and_then(Value::as_str))
    .unwrap_or("")
}

fn outputs_of<'a>(raw: &'a Value, field: &str) -> Vec<&'a str> {
    let mut outputs = Vec::new();
    for parent in [
        "/data",
        "/data/data",
        "/Result/data",
        "/Result/data/data",
        "/Result",
        "",
    ] {
        let Some(value) = raw.pointer(parent).and_then(|value| value.get(field)) else {
            continue;
        };
        let items = value.as_str().into_iter().chain(
            value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        );
        for item in items.map(str::trim).filter(|value| !value.is_empty()) {
            if !outputs.contains(&item) {
                outputs.push(item);
            }
        }
    }
    outputs
}

pub(crate) fn usage(raw: &Value) -> Value {
    let usage = raw
        .get("usage")
        .or_else(|| raw.pointer("/data/usage"))
        .or_else(|| raw.pointer("/Result/data/usage"))
        .unwrap_or(&Value::Null);
    let measurements = [
        ("prompt_tokens", "input_tokens", "tokens"),
        ("completion_tokens", "output_tokens", "tokens"),
        ("total_tokens", "total_tokens", "tokens"),
        ("duration_seconds", "duration", "seconds"),
    ]
    .into_iter()
    .filter_map(|(field, metric, unit)| {
        let value = usage.get(field)?;
        let amount = value.as_u64().map(|n| n.to_string()).or_else(|| {
            value
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(|n| {
                    if n == 0.0 {
                        String::from("0")
                    } else {
                        n.to_string()
                    }
                })
        })?;
        Some(json!({"metric":metric,"unit":unit,"amount":amount,"source":"reported"}))
    })
    .collect::<Vec<_>>();
    if measurements.is_empty() {
        Value::Null
    } else {
        json!({"measurements":measurements})
    }
}

pub(crate) fn query(
    host: &mut dyn Host,
    operation: &Operation,
    payload: &Value,
    budget: u64,
) -> Result<Value, Failure> {
    let id = payload["vendorTaskId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Failure::new("INVALID_INPUT", "火山智能视觉续办缺少原任务号"))?;
    let token = &payload["resumeToken"];
    if token["taskId"] != id || token["requestKey"] != operation.request_key {
        return Err(Failure::new(
            "INVALID_INPUT",
            "火山智能视觉续办身份与受理回执不一致",
        ));
    }
    let kind = operation.mode.output_kind();
    let mut body = json!({"req_key":operation.request_key,"task_id":id});
    if kind == "image" {
        body["req_json"] = json!(json!({"return_url":true}).to_string());
    }
    let (raw, handle) = provider::exchange(
        host,
        &payload["configuration"],
        operation.mode.query_action(),
        &body,
        false,
        budget,
    )?;
    if task_id(&raw).is_some_and(|known| known != id) {
        return Err(Failure::new("UPSTREAM_ERROR", "火山智能视觉返回另一项任务"));
    }
    let source_status = provider_status(&raw).to_ascii_lowercase();
    let provider_error = raw
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|n| n != 10000)
        || raw
            .pointer("/ResponseMetadata/Error")
            .is_some_and(|value| !value.is_null());
    let urls = outputs_of(
        &raw,
        if kind == "image" {
            "image_urls"
        } else {
            "video_url"
        },
    );
    let inline = if kind == "image" {
        outputs_of(&raw, "binary_data_base64")
    } else {
        Vec::new()
    };
    let status = match source_status.as_str() {
        "in_queue" | "queued" => "queued",
        "expired" | "not_found" => "expired",
        "cancelled" | "canceled" => "cancelled",
        "failed" | "fail" | "error" => "failed",
        "done" | "success" | "succeed" | "finished" | "complete" | "completed" => {
            if provider_error {
                "failed"
            } else {
                "succeeded"
            }
        }
        "" if !provider_error && (!urls.is_empty() || !inline.is_empty()) => "succeeded",
        _ => "running",
    };
    let mut outputs = Vec::new();
    let mut data = Value::Null;
    if status == "succeeded" {
        if urls.is_empty() {
            for image in inline {
                outputs.push(media::inline_image(host, image)?);
            }
        } else {
            for url in urls {
                outputs.push(media::download(host, url, kind, budget)?);
            }
        }
        if outputs.is_empty() {
            return Err(Failure::new(
                "UPSTREAM_ERROR",
                "火山智能视觉成功回执缺少原件",
            ));
        }
        outputs.push(media::output(host, &handle, "provider-response", "file")?);
        data = json!({"taskId":id});
    }
    let failure = match status {
        "failed" => Some(("vendorTaskFailed", "火山智能视觉原任务失败")),
        "expired" => Some(("vendorTaskExpired", "火山智能视觉原任务已过期")),
        "cancelled" => Some(("vendorTaskCancelled", "火山智能视觉原任务已取消")),
        _ => None,
    }
    .map_or(Value::Null, |(category, message)| {
        let vendor_code = raw
            .pointer("/ResponseMetadata/Error/Code")
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| {
                raw.get("code")
                    .filter(|value| value.is_number())
                    .map(Value::to_string)
            })
            .unwrap_or(source_status);
        json!({"category":category,"message":message,"vendorCode":vendor_code})
    });
    Ok(
        json!({"status":status,"progress":null,"outputs":outputs,"data":data,"usage":usage(&raw),
        "terminalFailure":failure,"resumeToken":token}),
    )
}
