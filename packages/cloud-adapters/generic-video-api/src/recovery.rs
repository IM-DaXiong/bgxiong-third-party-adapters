use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use bgx_adapter_v2_sdk::Host;
use serde_json::{json, Value};

use super::provider::{download, exchange, result_media, Error};

pub(crate) fn task_id(value: &Value) -> Option<&str> {
    ["/task_id", "/data/task_id", "/id"]
        .into_iter()
        .filter_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .map(str::trim)
        .find(|id| !id.is_empty())
}

fn task_path(id: &str) -> String {
    use core::fmt::Write;
    let mut path = String::from("/videos/");
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'~') {
            path.push(char::from(byte));
        } else {
            write!(&mut path, "%{byte:02X}").expect("String formatting");
        }
    }
    path
}

fn status(value: &Value) -> (String, &'static str) {
    let provider = [
        "/status",
        "/data/status",
        "/data/task_status",
        "/task_status",
    ]
    .into_iter()
    .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
    .unwrap_or_default()
    .trim()
    .to_ascii_lowercase();
    let normalized = match provider.as_str() {
        "pending" | "queued" | "waiting" => "queued",
        "succeeded" | "succeed" | "success" | "completed" | "complete" | "finished" | "done" => {
            "succeeded"
        }
        "failed" | "fail" | "failure" | "error" => "failed",
        "cancelled" | "canceled" => "cancelled",
        "expired" => "expired",
        _ => "running",
    };
    (provider, normalized)
}

fn urls(value: &Value) -> Vec<&str> {
    let videos = value
        .pointer("/data/task_result/videos")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|video| video["url"].as_str());
    let alternate = value
        .pointer("/data/video_url")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    let mut urls = Vec::new();
    for url in videos
        .chain(alternate)
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    if urls.is_empty() {
        if let Some(url) = ["/data/video_url", "/video_url"]
            .into_iter()
            .filter_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
            .map(str::trim)
            .find(|url| !url.is_empty())
        {
            urls.push(url);
        }
    }
    urls
}

fn usage(value: &Value) -> Value {
    let measurements: Vec<_> = [
        ("prompt_tokens","input_tokens"),
        ("completion_tokens","output_tokens"),
        ("total_tokens","total_tokens"),
    ].into_iter().filter_map(|(field,metric)| value["usage"][field].as_u64()
        .map(|amount| json!({"metric":metric,"unit":"tokens","amount":amount.to_string(),"source":"reported"}))
    ).collect();
    if measurements.is_empty() {
        Value::Null
    } else {
        json!({"measurements":measurements})
    }
}

fn failure(value: &Value, status: &str, provider_status: &str) -> Value {
    let (category, message) = match status {
        "failed" => ("vendorTaskFailed", "通用视频原任务失败"),
        "cancelled" => ("vendorTaskCancelled", "通用视频原任务已取消"),
        "expired" => ("vendorTaskExpired", "通用视频原任务已过期"),
        _ => return Value::Null,
    };
    let vendor_code = ["/data/error/code", "/error/code", "/data/code", "/code"]
        .into_iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .unwrap_or(provider_status);
    let vendor_message = [
        "/data/task_status_msg",
        "/task_status_msg",
        "/data/error/message",
        "/data/message",
        "/output/message",
        "/message",
        "/msg",
    ]
    .into_iter()
    .filter_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
    .map(str::trim)
    .find(|message| !message.is_empty())
    .unwrap_or(provider_status)
    .chars()
    .take(640)
    .collect::<String>();
    json!({"category":category,"message":message,"vendorCode":vendor_code,"details":{"message":vendor_message}})
}

pub(crate) fn query(host: &mut dyn Host, payload: &Value, budget: u64) -> Result<Value, Error> {
    let id = payload["vendorTaskId"]
        .as_str()
        .filter(|id| !id.trim().is_empty() && payload["resumeToken"]["taskId"] == *id)
        .ok_or_else(|| Error::new("INVALID_INPUT", "通用视频续办任务号与原回执不一致"))?;
    let (raw, source) = exchange(
        host,
        &payload["configuration"],
        &task_path(id),
        None,
        budget,
    )?;
    if task_id(&raw).is_some_and(|known| known != id) {
        return Err(Error::new("UPSTREAM_ERROR", "通用视频查询返回另一项任务"));
    }
    let (provider_status, status) = status(&raw);
    let mut outputs = Vec::new();
    let mut data = Value::Null;
    if status == "succeeded" {
        let urls = urls(&raw);
        if urls.is_empty() {
            return Err(Error::new(
                "UPSTREAM_ERROR",
                "通用视频原任务成功回执缺少成果地址",
            ));
        }
        for url in urls {
            outputs.push(download(host, url, budget)?);
        }
        outputs.push(result_media(host, &source, "provider-response", "file")?);
        data = json!({"taskId":id});
    }
    Ok(
        json!({"status":status,"progress":null,"outputs":outputs,"data":data,"usage":usage(&raw),
        "terminalFailure":failure(&raw,status,&provider_status),"resumeToken":payload["resumeToken"]}),
    )
}
