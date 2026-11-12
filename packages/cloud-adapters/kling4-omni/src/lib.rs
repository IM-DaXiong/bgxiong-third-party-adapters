#![cfg_attr(target_arch = "wasm32", no_std)]
extern crate alloc;

mod contracts;
mod media;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use bgx_adapter_v2_sdk::{guest, host, submission_rejection, Adapter, Host, Invocation, Response};
use contracts::Mode;
use serde_json::{json, Value};

pub const PLUGIN: &str = "local.thirdparty.kling4-omni";
const API: &str = "https://api-beijing.klingai.com";
const CREATE: &str = "/omni-video/kling-4.0";

#[derive(Debug)]
struct Failure {
    code: &'static str,
    message: &'static str,
    retryable: bool,
    vendor_code: Option<String>,
}

type Result<T> = core::result::Result<T, Failure>;

fn failure(code: &'static str, message: &'static str) -> Failure {
    Failure {
        code,
        message,
        retryable: false,
        vendor_code: None,
    }
}

fn invalid(message: &'static str) -> Failure {
    failure("INVALID_INPUT", message)
}

fn malformed() -> Failure {
    failure("UPSTREAM_ERROR", "可灵返回了不完整或未知的任务数据")
}

fn port(h: &mut dyn Host, method: &str, args: Value) -> Result<Value> {
    host::call(h, method, &args).map_err(|error| {
        let code = serde_json::to_value(&error).unwrap_or(Value::Null);
        failure(
            match code["code"].as_str() {
                Some("PERMISSION_DENIED") => "PERMISSION_DENIED",
                Some("BUDGET_EXCEEDED") => "BUDGET_EXCEEDED",
                Some("TIMEOUT") => "TIMEOUT",
                _ => "RESOURCE_UNAVAILABLE",
            },
            "SDK 受管端口未完成请求",
        )
    })
}

fn configuration(payload: &Value) -> Result<&str> {
    let config = &payload["configuration"];
    if config.as_object().is_none_or(|obj| obj.len() != 2) || config["endpointBaseUrl"] != API {
        return Err(invalid("可灵仅支持已声明的北京 API"));
    }
    config["credentialRef"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("请配置可灵 API Key"))
}

fn http(
    h: &mut dyn Host,
    r: &Invocation,
    method: &str,
    url: String,
    body: Value,
    credential: Option<&str>,
) -> Result<Value> {
    let download = credential.is_none();
    port(
        h,
        "host-http.execute",
        json!({
            "plan":{
                "method":method,"url":url,
                "headers":if method=="POST" {json!([{"name":"Content-Type","value":"application/json"}])} else {json!([])},
                "generatedHeaders":if download {json!([])} else {json!([{"name":"Authorization","kind":"bearer","targetField":"Authorization"}])},
                "body":body,"sink":if download {json!({"kind":"media"})} else {json!({"kind":"buffer","maxBytes":1048576})},
                "timeoutMs":r.r#budget_ms
            },
            "credential":credential
        }),
    )
}

fn response_json(response: &Value) -> Result<Value> {
    let bytes: Vec<u8> =
        serde_json::from_value(response["payload"]["bytes"].clone()).map_err(|_| malformed())?;
    serde_json::from_slice(&bytes).map_err(|_| malformed())
}

fn api_failure(status: u64, code: Option<i64>) -> Failure {
    let mut result = match (status, code) {
        (401 | 403, _) | (_, Some(1000..=1103)) => {
            failure("UNAUTHORIZED", "可灵鉴权或账号权限校验未通过")
        }
        (429, _) | (_, Some(1302 | 1303)) => failure("RATE_LIMITED", "可灵请求频率或并发达到限制"),
        (_, Some(1200..=1203)) => invalid("可灵拒绝了生成参数"),
        (_, Some(1301)) => failure("UPSTREAM_ERROR", "可灵内容安全校验未通过"),
        _ => failure("UPSTREAM_ERROR", "可灵 API 请求未完成"),
    };
    result.vendor_code = code.map(|n| n.to_string());
    result
}

fn checkpoint(h: &mut dyn Host, r: &Invocation) -> Result<Value> {
    let row = port(
        h,
        "host-checkpoint.read",
        json!({"requestId":r.r#request_id}),
    )?;
    let raw = row["valueJson"].as_str().ok_or_else(malformed)?;
    let value: Value = serde_json::from_str(raw).map_err(|_| malformed())?;
    if row["revision"] != value["revision"] || value["requestId"] != r.r#request_id {
        return Err(malformed());
    }
    Ok(value)
}

fn write_checkpoint(h: &mut dyn Host, r: &Invocation, old: &Value, next: &Value) -> Result<()> {
    let saved = port(
        h,
        "host-checkpoint.write",
        json!({
            "requestId":r.r#request_id,"expectedRevision":old["revision"],"valueJson":next.to_string()
        }),
    )?;
    if saved["revision"] != next["revision"] {
        return Err(malformed());
    }
    Ok(())
}

fn reject_before_submit(h: &mut dyn Host, r: &Invocation, error: &Failure) -> Result<()> {
    let old = checkpoint(h, r)?;
    let next = submission_rejection::checkpoint_candidate(
        &old,
        "before_submission",
        error.message,
        &json!({"code":error.code}),
    )
    .map_err(|_| malformed())?;
    write_checkpoint(h, r, &old, &next)
}

fn validate_settings(mode: Mode, input: &Value) -> Result<()> {
    let prompt = input["prompt"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("请输入提示词"))?;
    let ascii = prompt.chars().filter(|c| c.is_ascii()).count();
    let other = prompt.chars().count() - ascii;
    if ascii > 26000 || other > 11000 || ascii * 11000 + other * 26000 > 26000 * 11000 {
        return Err(invalid("提示词超出可灵长度限制"));
    }
    if input["inputMode"] != mode.input_mode()
        || input["fps"] != 24
        || input["durationSeconds"]
            .as_u64()
            .is_none_or(|n| !(3..=30).contains(&n))
        || !matches!(input["resolution"].as_str(), Some("720p" | "1080p"))
        || !matches!(
            input["aspectRatio"].as_str(),
            Some("21:9" | "16:9" | "9:16" | "1:1" | "adaptive")
        )
    {
        return Err(invalid("可灵画幅、清晰度、时长或输入模式超出声明范围"));
    }
    Ok(())
}

fn submit(h: &mut dyn Host, r: &Invocation, p: &Value) -> Result<Value> {
    let prepare = (|| {
        let mode = contracts::mode(p, true).ok_or_else(|| invalid("模型、操作或参数合同已改变"))?;
        let credential = configuration(p)?;
        validate_settings(mode, &p["input"])?;
        let external_id = contracts::digest(r.r#request_id.as_bytes());
        let body = media::body(h, mode, p, &external_id[..32])?;
        Ok((mode, credential, body))
    })();
    let (mode, credential, body) = match prepare {
        Ok(prepared) => prepared,
        Err(error) => {
            reject_before_submit(h, r, &error)?;
            return Err(error);
        }
    };
    let response = http(
        h,
        r,
        "POST",
        format!("{API}{CREATE}"),
        body.clone(),
        Some(credential),
    );
    if let Some(handle) = body["handle"].as_str() {
        let _ = port(h, "host-media.release", json!({"handle":handle}));
    }
    let unknown = || {
        failure(
            "ACCEPTANCE_UNKNOWN",
            "可灵受理状态不确定，请核对原任务，勿重复提交",
        )
    };
    let response = response.map_err(|_| unknown())?;
    let status = response["status"].as_u64().ok_or_else(unknown)?;
    let value = response_json(&response).map_err(|_| unknown())?;
    let code = value["code"].as_i64().ok_or_else(unknown)?;
    if code != 0 || !(200..300).contains(&status) {
        // Only documented explicit rejections with no remote ID can close an unknown checkpoint.
        let explicitly_rejected = matches!((status, code),
            (401, 1000..=1004) | (403, 1103) | (429, 1100..=1102 | 1302..=1304)
                | (400, 1200..=1201 | 1300..=1301) | (404, 1202..=1203));
        if explicitly_rejected && value["data"]["id"].is_null() {
            let error = api_failure(status, Some(code));
            let old = checkpoint(h, r).map_err(|_| unknown())?;
            let next = submission_rejection::checkpoint_candidate(
                &old,
                "service_rejected",
                error.message,
                &json!({"httpStatus":status,"vendorCode":code}),
            )
            .map_err(|_| unknown())?;
            write_checkpoint(h, r, &old, &next).map_err(|_| unknown())?;
            return Err(error);
        }
        return Err(unknown());
    }
    let id = value["data"]["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(unknown)?;
    let result = json!({"kind":"accepted","vendorTaskId":id,"appliedParams":null,
        "resumeToken":{"taskId":id,"modelId":mode.model()},"usage":null});
    let old = checkpoint(h, r).map_err(|_| unknown())?;
    let rev = old["revision"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .and_then(|n| n.checked_add(1))
        .ok_or_else(unknown)?;
    let mut next = old.clone();
    next["revision"] = json!(rev.to_string());
    next["state"] = json!("accepted");
    next["vendorTaskId"] = json!(id);
    next["resumeToken"] = result["resumeToken"].clone();
    next["result"] = result.clone();
    write_checkpoint(h, r, &old, &next).map_err(|_| unknown())?;
    Ok(result)
}

fn query(h: &mut dyn Host, r: &Invocation, p: &Value) -> Result<Value> {
    let mode = contracts::mode(p, false).ok_or_else(|| invalid("查询操作身份不匹配"))?;
    let id = p["vendorTaskId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(malformed)?;
    if p["resumeToken"] != json!({"taskId":id,"modelId":mode.model()}) {
        return Err(invalid("续查任务身份不匹配"));
    }
    let encoded: String = id
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let response = http(
        h,
        r,
        "GET",
        format!("{API}/tasks?task_ids={encoded}"),
        json!({"kind":"none"}),
        Some(configuration(p)?),
    )?;
    let value = response_json(&response)?;
    let status = response["status"].as_u64().ok_or_else(malformed)?;
    let code = value["code"].as_i64().ok_or_else(malformed)?;
    if code != 0 || !(200..300).contains(&status) {
        return Err(api_failure(status, Some(code)));
    }
    let rows = value["data"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or_else(malformed)?;
    let row = &rows[0];
    if row["id"] != id {
        return Err(malformed());
    }
    let state = match row["status"].as_str() {
        Some("submitted") => "queued",
        Some("processing") => "running",
        Some("succeeded") => "succeeded",
        Some("failed") => "failed",
        _ => return Err(malformed()),
    };
    let mut outputs = Vec::new();
    if state == "succeeded" {
        outputs.push(media::download(h, r, row).map_err(|mut error| {
            error.retryable = true;
            error
        })?);
    }
    let measurements: Vec<_> = [
        ("input_duration", "input_duration"),
        ("output_duration", "output_duration"),
    ]
    .into_iter()
    .filter_map(|(key, metric)| {
        let amount = row["usage"][key].as_str()?;
        let mut parts = amount.split('.');
        let whole = parts.next()?;
        if whole.is_empty()
            || (whole.len() > 1 && whole.starts_with('0'))
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || parts
                .next()
                .is_some_and(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
            || parts.next().is_some()
        {
            return None;
        }
        Some(
            json!({"metric":format!("{PLUGIN}/{metric}"),"unit":format!("{PLUGIN}/seconds"),
                "amount":amount,"source":"reported"}),
        )
    })
    .collect();
    Ok(
        json!({"status":state,"outputs":outputs,"data":{"remoteTaskId":id,"downloadedCount":outputs.len()},
        "usage":if measurements.is_empty(){Value::Null}else{json!({"measurements":measurements})},
        "progress":null,"terminalFailure":if state=="failed" {
            json!({"category":"vendorTaskFailed","message":"可灵任务失败，请在可灵任务记录核对原因","vendorCode":null,"details":{"taskId":id}})
        } else {Value::Null},
        "resumeToken":p["resumeToken"]}),
    )
}

pub struct KlingOmni;

impl Adapter for KlingOmni {
    fn invoke(r: Invocation, h: &mut dyn Host) -> Response {
        let result = (|| {
            let p: Value =
                serde_json::from_str(&r.r#payload_json).map_err(|_| invalid("请求 JSON 无效"))?;
            let operation = serde_json::to_value(&r.r#operation).expect("operation");
            match operation.as_str() {
                Some("capabilities") => Ok(contracts::capabilities()),
                Some("probe") if p["kind"] == "package_validation" => Ok(
                    json!({"packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]}),
                ),
                Some("submit") => submit(h, &r, &p),
                Some("query") => query(h, &r, &p),
                Some("cancel") => Ok(json!({"outcome":"unsupported"})),
                _ => Err(failure("UNSUPPORTED_FEATURE", "可灵接口未开放此操作")),
            }
        })();
        match result {
            Ok(value) => guest::completed_response(&r, &value).expect("public result"),
            Err(error) => {
                serde_json::from_value(json!({"protocolVersion":2,"operation":r.r#operation,
                "error":{"code":error.code,"message":error.message,"retryable":error.retryable,
                    "vendorCode":error.vendor_code,"vendorRequestId":null}}))
                .expect("public error")
            }
        }
    }
}

bgx_adapter_v2_sdk::export_adapter!(KlingOmni);

#[cfg(test)]
mod tests;
