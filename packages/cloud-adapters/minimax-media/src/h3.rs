use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use bgx_adapter_v2_sdk::Host;
use serde_json::{json, Value};

use super::provider::{self, Failure};

const MODEL: &str = "MiniMax-H3";
const OPERATION: &str = "local.bgx.minimax-media/h3-text";
const TASK_SCHEMA: &str = r#"{"type":"object","additionalProperties":false,"required":["taskId"],"properties":{"taskId":{"type":"string","minLength":1}}}"#;
const INPUT_SCHEMA: &str = r#"{"type":"object","additionalProperties":false,"required":["inputMode","prompt","durationSeconds","resolution","aspectRatio"],"properties":{
  "inputMode":{"type":"string","const":"text_to_video","enum":["text_to_video"],"default":"text_to_video"},
  "prompt":{"type":"string","minLength":1,"maxLength":7000},
  "durationSeconds":{"type":"integer","minimum":4,"maximum":15,"default":4},
  "resolution":{"type":"string","enum":["768P","2K"],"default":"768P"},
  "aspectRatio":{"type":"string","enum":["21:9","16:9","4:3","1:1","3:4","9:16"],"default":"16:9"},
  "aigcWatermark":{"type":"boolean","default":false}
}}"#;

pub(crate) fn capability() -> Value {
    let configuration = super::schema("configuration", super::CONFIG);
    let input = super::schema("input", INPUT_SCHEMA);
    let output = super::schema("output", TASK_SCHEMA);
    let resume = super::schema("resume", TASK_SCHEMA);
    let controls:Vec<_> = [
        ("/inputMode","select","生成方式",Some("inputMode")),
        ("/prompt","multiline","提示词",Some("prompt")),
        ("/durationSeconds","number","时长",Some("duration")),
        ("/resolution","select","清晰度",Some("resolution")),
        ("/aspectRatio","select","画幅",Some("aspectRatio")),
        ("/aigcWatermark","toggle","水印",None),
    ].into_iter().map(|(pointer,widget,label,role)| {
        let mut value = json!({"fieldPointer":pointer,"widget":widget,"label":label,"group":"","description":""});
        if let Some(role) = role { value["displayRole"] = json!(role); }
        value
    }).collect();
    json!({"schemaVersion":2,"localId":"h3-text","capabilityKey":"video.generate","transportSlot":"video","modelIds":[MODEL],
        "configurationSchema":{"id":"configuration","digest":configuration["digest"]},"schemas":[configuration,input,output,resume],
        "operations":[{"id":OPERATION,"version":1,"inputProfile":"operation-input-v1","label":"MiniMax H3 文生视频",
            "inputSchema":{"id":"input","digest":input["digest"]},"outputSchema":{"id":"output","digest":output["digest"]},
            "resumeSchema":{"id":"resume","digest":resume["digest"]},"uiSchema":{"controls":controls},"mediaSlots":[],
            "outputSlots":[{"role":"video","kind":"video","minCount":1,"maxCount":1},
                {"role":"provider-response","kind":"file","minCount":1,"maxCount":1}],
            "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported","resumable":true,"remoteIdempotency":false}}]
    })
}

fn is_operation(selection: &Value) -> bool {
    selection["modelId"] == MODEL
        && selection["capabilityKey"] == "video.generate"
        && selection["operationId"] == OPERATION
        && selection["operationVersion"] == 1
}

pub(crate) fn material(
    selection: &Value,
    input: &Value,
) -> Result<Value, (&'static str, &'static str)> {
    if !is_operation(selection) {
        return Err(("", "MiniMax H3 操作身份无效"));
    }
    if input["inputMode"] != "text_to_video" {
        return Err(("/inputMode", "所选操作需要文生视频模式"));
    }
    let prompt = input["prompt"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .ok_or(("/prompt", "请输入提示词"))?;
    Ok(
        json!({"model":MODEL,"content":[{"type":"text","text":prompt}],"resolution":input["resolution"],
        "duration":input["durationSeconds"],"ratio":input["aspectRatio"],"aigc_watermark":input["aigcWatermark"].as_bool().unwrap_or(false)}),
    )
}

fn api_call(
    host: &mut dyn Host,
    method: &str,
    path: &str,
    payload: &Value,
    body: Option<&Value>,
    budget: u64,
) -> Result<(Value, String), Failure> {
    let configuration = &payload["configuration"];
    let origin = configuration["endpointBaseUrl"]
        .as_str()
        .map(|url| url.trim_end_matches('/'))
        .filter(|url| !url.is_empty())
        .ok_or_else(|| Failure::new("INVALID_INPUT", "MiniMax H3 API 地址缺失"))?;
    let credential = configuration["credentialRef"]
        .as_str()
        .ok_or_else(|| Failure::new("INVALID_INPUT", "MiniMax H3 账号引用缺失"))?;
    let body = if let Some(body) = body {
        let bytes = serde_json::to_vec(body)
            .map_err(|_| Failure::new("INVALID_INPUT", "MiniMax H3 请求编码失败"))?;
        let handle = provider::put_media(host, "file", "application/octet-stream", &bytes)?;
        json!({"kind":"media","handle":handle})
    } else {
        json!({"kind":"none"})
    };
    let response = provider::request(
        host,
        method,
        &format!("{origin}{path}"),
        body,
        Some(credential),
        budget,
    )
    .map_err(|error| {
        if method == "POST" && error.code == "UPSTREAM_ERROR" {
            Failure::new("ACCEPTANCE_UNKNOWN", "MiniMax H3 的原提交结果未确认")
        } else {
            error
        }
    })?;
    let status = response["status"]
        .as_u64()
        .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "MiniMax H3 HTTP 状态缺失"))?;
    let handle = response["payload"]["handle"]
        .as_str()
        .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "MiniMax H3 响应租约缺失"))?;
    let raw = provider::media_bytes(host, handle, 1024 * 1024)?;
    let json = serde_json::from_slice::<Value>(&raw);
    if !(200..300).contains(&status) {
        let kind = match status {
            401 | 403 => "UNAUTHORIZED",
            402 => "RESOURCE_UNAVAILABLE",
            429 => "RATE_LIMITED",
            400 | 422 => "INVALID_INPUT",
            _ => "UPSTREAM_ERROR",
        };
        let mut error = Failure::new(kind, "MiniMax H3 拒绝请求");
        error.vendor = json.as_ref().ok().and_then(|value| {
            value["error"]["code"]
                .as_str()
                .or_else(|| value["error"]["type"].as_str())
                .map(String::from)
        });
        error.vendor_request = json
            .as_ref()
            .ok()
            .and_then(|value| value["request_id"].as_str().map(String::from));
        if method == "POST" && status >= 500 {
            error.code = "ACCEPTANCE_UNKNOWN";
        }
        return Err(error);
    }
    let data = json.map_err(|_| {
        Failure::new(
            if method == "POST" {
                "ACCEPTANCE_UNKNOWN"
            } else {
                "UPSTREAM_ERROR"
            },
            "MiniMax H3 响应格式无效",
        )
    })?;
    Ok((data, handle.into()))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn task_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(String::from)
        .or_else(|| value.as_u64().map(|id| id.to_string()))
        .filter(|id| valid_id(id))
}

fn reported_usage(task: &Value) -> Value {
    let measurements: Vec<_> = [
        ("total_seconds", "duration", "seconds"),
        (
            "input_seconds",
            "local.bgx.minimax-media/input_duration",
            "seconds",
        ),
        (
            "output_seconds",
            "local.bgx.minimax-media/output_duration",
            "seconds",
        ),
        (
            "input_image_count",
            "local.bgx.minimax-media/input_image_count",
            "local.bgx.minimax-media/images",
        ),
    ]
    .into_iter()
    .filter_map(|(field, metric, unit)| {
        task["usage"][field].as_u64().map(|amount| {
            json!({"metric":metric,"unit":unit,"amount":amount.to_string(),"source":"reported"})
        })
    })
    .collect();
    if measurements.is_empty() {
        Value::Null
    } else {
        json!({"measurements":measurements})
    }
}

pub(crate) fn submit(host: &mut dyn Host, payload: &Value, budget: u64) -> Result<Value, Failure> {
    let body = material(payload, &payload["input"])
        .map_err(|(_, message)| Failure::new("INVALID_INPUT", message))?;
    if payload["preparation"]["material"] != body {
        return Err(Failure::new(
            "INVALID_INPUT",
            "MiniMax H3 提交与冻结预检不一致",
        ));
    }
    let (response, _) = api_call(
        host,
        "POST",
        "/v2/video_generation",
        payload,
        Some(&body),
        budget,
    )?;
    let id = task_id(&response["task_id"])
        .ok_or_else(|| Failure::new("ACCEPTANCE_UNKNOWN", "MiniMax H3 回执缺少原任务号"))?;
    Ok(
        json!({"kind":"accepted","vendorTaskId":id,"resumeToken":{"taskId":id},"appliedParams":null,"usage":null}),
    )
}

pub(crate) fn query(host: &mut dyn Host, payload: &Value, budget: u64) -> Result<Value, Failure> {
    if !is_operation(payload) {
        return Err(Failure::new("INVALID_INPUT", "MiniMax H3 续办操作身份无效"));
    }
    let id = payload["vendorTaskId"]
        .as_str()
        .filter(|id| valid_id(id) && payload["resumeToken"]["taskId"] == *id)
        .ok_or_else(|| Failure::new("INVALID_INPUT", "MiniMax H3 续办任务号与原回执不一致"))?;
    let (response, raw) = api_call(
        host,
        "GET",
        &format!("/v2/query/video_generation/{id}"),
        payload,
        None,
        budget,
    )?;
    let task = &response["task"];
    if task_id(&task["id"]).as_deref() != Some(id) {
        return Err(Failure::new(
            "UPSTREAM_ERROR",
            "MiniMax H3 返回了另一条任务",
        ));
    }
    match task["status"].as_str() {
        Some(status @ ("queued" | "running")) => {
            Ok(json!({"status":status,"vendorTaskId":id,"progress":null}))
        }
        Some(status @ ("failed" | "cancelled" | "expired")) => {
            Ok(json!({"status":status,"vendorTaskId":id,
            "terminalFailure":{"category":"execution","message":"MiniMax H3 原任务结束","vendorCode":task["error"]["code"].as_str(),"retryable":false}}))
        }
        Some("succeeded") => {
            let url = task["content"]["url"].as_str().ok_or_else(|| {
                Failure::new("UPSTREAM_ERROR", "MiniMax H3 成功回执缺少视频下载地址")
            })?;
            let downloaded =
                provider::request(host, "GET", url, json!({"kind":"none"}), None, budget)?;
            let handle = downloaded["payload"]["handle"]
                .as_str()
                .filter(|_| downloaded["status"] == 200)
                .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "MiniMax H3 原生成视频下载失败"))?;
            let outputs = vec![
                provider::output(host, handle, "video")?,
                provider::output(host, &raw, "provider-response")?,
            ];
            Ok(
                json!({"status":"succeeded","vendorTaskId":id,"outputs":outputs,"data":{"taskId":id},"usage":reported_usage(task)}),
            )
        }
        _ => Err(Failure::new(
            "UPSTREAM_ERROR",
            "MiniMax H3 返回未知任务状态",
        )),
    }
}
