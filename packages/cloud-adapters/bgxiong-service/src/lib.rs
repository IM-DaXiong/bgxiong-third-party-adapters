#![cfg_attr(target_arch = "wasm32", no_std)]
extern crate alloc;

use alloc::{format, vec::Vec};
use bgx_adapter_v2_sdk::{guest, host, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};

const BASE: &str = "/client/ai-service/v1";
const MODEL: &str = "bgxiong-asr-standard";

fn valid_task(id: &str) -> bool {
    id.len() == 36 && id.bytes().all(|x| x.is_ascii_hexdigit() || x == b'-')
}

fn request(host: &mut dyn Host, config: &Value, method: &str, suffix: &str, input: Option<&Value>)
    -> Result<Value, &'static str> {
    let origin = config["endpointBaseUrl"].as_str().ok_or("INVALID_CONFIGURATION")?;
    if origin != "https://www.bgxiong.com" && !origin.strip_prefix("http://127.0.0.1:")
        .is_some_and(|port| port.parse::<u16>().is_ok_and(|p| p != 0)) {
        return Err("PERMISSION_DENIED");
    }
    if config["credentialRef"] != "bgx-asr-task-grant" {
        return Err("INVALID_CONFIGURATION");
    }
    let (body, headers) = if let Some(input) = input {
        let bytes = serde_json::to_vec(input).map_err(|_| "INVALID_INPUT")?;
        (json!({"kind":"bytes","bytes":bytes}), json!([{"name":"Content-Type","value":"application/json"}]))
    } else {
        (json!({"kind":"none"}), json!([]))
    };
    let response: Value = host::call(host, "host-http.execute", &json!({
        "plan":{"method":method,"url":format!("{origin}{BASE}{suffix}"),"headers":headers,
            "generatedHeaders":[{"name":"Authorization","kind":"bearer","targetField":"Authorization"}],
            "body":body,"sink":{"kind":"buffer","maxBytes":393216},"timeoutMs":12000},
        "credential":"bgx-asr-task-grant"
    })).map_err(|_| "ACCEPTANCE_UNKNOWN")?;
    let status = response["status"].as_u64().ok_or("INVALID_RESPONSE")?;
    let bytes: Vec<u8> = serde_json::from_value(response["payload"]["bytes"].clone())
        .map_err(|_| "INVALID_RESPONSE")?;
    if !(200..300).contains(&status) {
        return match status {
            401 | 403 => Err("AUTH_REQUIRED"),
            402 => Err("INSUFFICIENT_CREDIT"),
            404 => Err("NOT_FOUND"),
            409 => Err("REQUEST_CONFLICT"),
            _ => Err("SERVICE_UNAVAILABLE"),
        };
    }
    serde_json::from_slice(&bytes).map_err(|_| "INVALID_RESPONSE")
}

fn invoke(request_value: &Invocation, host: &mut dyn Host) -> Result<Value, &'static str> {
    let op = serde_json::to_value(&request_value.r#operation).map_err(|_| "INVALID_INPUT")?;
    if op == "capabilities" {
        return serde_json::from_str(include_str!("capabilities.json")).map_err(|_| "INVALID_PACKAGE");
    }
    if op == "probe" {
        return Ok(json!({"packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"ready","issues":[]}));
    }
    let payload: Value = serde_json::from_str(&request_value.r#payload_json).map_err(|_| "INVALID_INPUT")?;
    let (model, operation) = match payload["operationId"].as_str() {
        Some("std/speech.transcribe") => (MODEL, "speech.transcribe"),
        Some("std/speech.subtitle") => ("bgxiong-asr-subtitle", "speech.subtitle"),
        _ => return Err("INVALID_INPUT"),
    };
    if payload["modelId"] != model || payload["operationVersion"] != 1 {
        return Err("INVALID_INPUT");
    }
    let config = &payload["configuration"];
    if op == "submit" {
        let submitted = &config["submission"];
        if submitted["modelId"] != model || submitted["operation"] != operation
            || submitted["clientRequestId"] != request_value.r#request_id {
            return Err("INVALID_INPUT");
        }
        let task = request(host, config, "POST", "/tasks", Some(submitted))?;
        let id = task["serviceTaskId"].as_str().filter(|id| valid_task(id)).ok_or("INVALID_RESPONSE")?;
        if submitted["quoteId"] != id {
            return Err("INVALID_RESPONSE");
        }
        return Ok(json!({"kind":"accepted","vendorTaskId":id,"appliedParams":null,
            "resumeToken":{"schemaVersion":1,"jobId":id},"usage":null}));
    }
    let id = payload["vendorTaskId"].as_str().filter(|id| valid_task(id)).ok_or("INVALID_INPUT")?;
    if payload["resumeToken"]["jobId"] != id {
        return Err("INVALID_INPUT");
    }
    if op == "cancel" {
        let result = request(host, config, "POST", &format!("/tasks/{id}/cancel"), None)?;
        let cancelled = result["cancellationState"] == "confirmed";
        return Ok(json!({"outcome":if cancelled {"remote_confirmed"} else {"unsupported"}}));
    }
    if op == "query" {
        let task = request(host, config, "GET", &format!("/tasks/{id}"), None)?;
        let state = task["executionState"].as_str().ok_or("INVALID_RESPONSE")?;
        let data = if state == "succeeded" {
            let content = request(host, config, "GET", &format!("/tasks/{id}/content"), None)?;
            if operation == "speech.subtitle" {
                json!({"schemaVersion":1,"speechState":content["speechState"],"sampleRate":content["sampleRate"],
                    "originalFrames":content["originalFrames"],"cues":content["cues"]})
            } else {
                json!({"schemaVersion":1,"text":content["text"],"speechState":content["speechState"],
                    "durationMs":content["durationMs"],"language":content["language"]})
            }
        } else { Value::Null };
        let status = match state {
            "queued" | "running" | "succeeded" | "failed" | "cancelled" => state,
            _ => return Err("INVALID_RESPONSE"),
        };
        let terminal_failure = match status {
            "failed" => json!({"category":"vendorTaskFailed","message":"识别未完成","vendorCode":null,"details":null}),
            "cancelled" => json!({"category":"vendorTaskCancelled","message":"识别已取消","vendorCode":null,"details":null}),
            _ => Value::Null,
        };
        return Ok(json!({"status":status,"progress":null,"outputs":[],"data":data,
            "terminalFailure":terminal_failure,"resumeToken":payload["resumeToken"],"usage":null}));
    }
    Err("INVALID_INPUT")
}

struct BgXiongAsr;

impl Adapter for BgXiongAsr {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        match invoke(&request, host) {
            Ok(data) => guest::completed_response(&request, &data).expect("bounded ASR response"),
            Err(code) => serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,
                "error":{"code":code,"message":"比格熊语音识别请求未完成","retryable":false,
                         "vendorCode":null,"vendorRequestId":null}})).expect("ASR error response"),
        }
    }
}

bgx_adapter_v2_sdk::export_adapter!(BgXiongAsr);
