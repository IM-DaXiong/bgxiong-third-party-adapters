use alloc::{format, string::ToString, vec, vec::Vec};
use bgx_adapter_v2_sdk::Host;
use serde_json::{json, Value};

use super::{download, exchange, schema, Failure, CONFIG};
use media_inputs::{control, Operation, OPERATIONS};

mod media_inputs;

const MODELS: [&str; 2] = ["wan3.0-video", "wan3.0-video-prime"];
const QUERY: &str = "/api/v1/tasks";
const SUBMIT: &str = "/api/v1/services/aigc/video-generation/video-synthesis";
const TASK_SCHEMA: &str = r#"{"type":"object","additionalProperties":false,"properties":{"taskId":{"type":"string","minLength":1}},"required":["taskId"]}"#;

pub(crate) fn capabilities() -> Vec<Value> {
    OPERATIONS.iter().copied().map(capability).collect()
}

fn capability(operation: Operation) -> Value {
    let configuration = schema("configuration", CONFIG);
    let required = if operation.requires_prompt() {
        json!(["inputMode", "prompt"])
    } else {
        json!(["inputMode"])
    };
    let mut input_schema = json!({
        "type":"object","additionalProperties":false,"required":required,
        "properties":{
            "inputMode":{"type":"string","const":operation.mode,"enum":[operation.mode],"default":operation.mode},
            "prompt":{"type":"string","minLength":usize::from(operation.requires_prompt()),"maxLength":20000},
            "durationSeconds":{"type":"integer","anyOf":[{"const":-1},{"minimum":2,"maximum":30}],"default":5},
            "resolution":{"type":"string","enum":["480P","720P","1080P"],"default":"1080P"},
            "aspectRatio":{"type":"string","enum":["adaptive","21:9","16:9","4:3","1:1","3:4","9:16"],"default":"adaptive"},
            "generateAudio":{"type":"boolean","default":true},
            "seed":{"type":"integer","minimum":-1,"maximum":2147483647},
            "promptExtend":{"type":"boolean","default":true},
            "watermark":{"type":"boolean","default":false}
        }
    });
    if operation.adaptive_only() {
        input_schema["properties"]["aspectRatio"]["enum"] = json!(["adaptive"]);
    }
    let mut controls: Vec<_> = [
        ("/inputMode", "select", "生成方式", Some("inputMode")),
        ("/prompt", "multiline", "提示词", Some("prompt")),
        ("/durationSeconds", "number", "时长（秒）", Some("duration")),
        ("/resolution", "select", "分辨率", Some("resolution")),
        ("/aspectRatio", "select", "画幅", Some("aspectRatio")),
        ("/generateAudio", "toggle", "生成声音", None),
        ("/seed", "integer", "随机种子", None),
        ("/promptExtend", "toggle", "改写提示词", None),
        ("/watermark", "toggle", "水印", None),
    ]
    .into_iter()
    .map(|(pointer, widget, label, role)| control(pointer, widget, label, role))
    .collect();
    let slots = operation.description(&mut input_schema, &mut controls);
    let input = schema("input", &input_schema.to_string());
    let output = schema("output", TASK_SCHEMA);
    let resume = schema("resume", TASK_SCHEMA);
    json!({"schemaVersion":2,"localId":operation.local_id,"capabilityKey":"video.generate","transportSlot":"video","modelIds":MODELS,
            "configurationSchema":{"id":"configuration","digest":configuration["digest"]},"schemas":[configuration,input,output,resume],
            "operations":[{"id":operation.id,"version":1,"inputProfile":"operation-input-v1","label":operation.label,
                "inputSchema":{"id":"input","digest":input["digest"]},"outputSchema":{"id":"output","digest":output["digest"]},
                "resumeSchema":{"id":"resume","digest":resume["digest"]},"uiSchema":{"controls":controls},
                "mediaSlots":slots,"outputSlots":[{"role":"video","kind":"video","minCount":1,"maxCount":1}],
                "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported","resumable":true,"remoteIdempotency":false}}]
    })
}

fn selected_operation(selection: &Value) -> Option<(&str, Operation)> {
    let model = selection["modelId"].as_str()?;
    if !MODELS.contains(&model)
        || selection["capabilityKey"] != "video.generate"
        || selection["operationVersion"] != 1
    {
        return None;
    }
    let operation = OPERATIONS
        .iter()
        .copied()
        .find(|operation| selection["operationId"] == operation.id)?;
    Some((model, operation))
}

pub(crate) fn material(
    selection: &Value,
    input: &Value,
) -> Result<Value, (&'static str, &'static str)> {
    let (model, operation) =
        selected_operation(selection).ok_or(("/inputMode", "万相视频操作身份无效"))?;
    if input["inputMode"] != operation.mode {
        return Err(("/inputMode", "所选万相操作与生成方式不一致"));
    }
    let prompt = input["prompt"]
        .as_str()
        .filter(|text| !text.trim().is_empty());
    if operation.requires_prompt() && prompt.is_none() {
        return Err(("/prompt", "请输入提示词"));
    }
    if prompt.is_some_and(|text| text.chars().count() > 20000) {
        return Err(("/prompt", "万相 3.0 提示词最多 20000 字符"));
    }
    let ratio = input["aspectRatio"].as_str().unwrap_or("adaptive");
    if operation.adaptive_only() && ratio != "adaptive" {
        return Err(("/aspectRatio", "万相视频延长需要自适应画幅"));
    }
    let media = operation.material(input)?;
    let duration = input["durationSeconds"].as_i64().unwrap_or(5);
    if duration != -1 && !(2..=30).contains(&duration) {
        return Err((
            "/durationSeconds",
            "万相 3.0 时长应在 2–30 秒，或使用智能时长",
        ));
    }
    let mut operation_input = json!({});
    if let Some(prompt) = prompt {
        operation_input["prompt"] = json!(prompt);
    }
    if !media.is_empty() {
        operation_input["media"] = json!(media);
    }
    let mut material = json!({"model":model,"input":operation_input,"parameters":{
        "duration":duration,"resolution":input["resolution"].as_str().unwrap_or("1080P"),
        "ratio":ratio,
        "audio":input["generateAudio"].as_bool().unwrap_or(true),"prompt_extend":input["promptExtend"].as_bool().unwrap_or(true),
        "watermark":input["watermark"].as_bool().unwrap_or(false)
    }});
    if let Some(seed) = input["seed"].as_i64() {
        material["parameters"]["seed"] = json!(seed);
    }
    Ok(material)
}

fn task_id(value: &Value) -> Option<&str> {
    value.as_str().filter(|id| {
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    })
}

pub(crate) fn submit(host: &mut dyn Host, payload: &Value, budget: u64) -> Result<Value, Failure> {
    let mut request = material(payload, &payload["input"])
        .map_err(|(_, message)| Failure::new("INVALID_INPUT", message))?;
    if payload["preparation"]["material"] != request {
        return Err(Failure::new("INVALID_INPUT", "万相提交与原预检不一致"));
    }
    let needs_oss = media_inputs::convert(host, &mut request, &payload["media"])?;
    let mut headers = vec![json!({"name":"X-DashScope-Async","value":"enable"})];
    if needs_oss {
        headers.push(json!({"name":"X-DashScope-OssResourceResolve","value":"enable"}));
    }
    let (response, _) = exchange(
        host,
        payload,
        SUBMIT,
        "POST",
        Some(&request),
        json!(headers),
        budget,
    )?;
    let id = task_id(&response["output"]["task_id"])
        .ok_or_else(|| Failure::new("ACCEPTANCE_UNKNOWN", "万相未返回可核实的原任务号"))?;
    Ok(
        json!({"kind":"accepted","vendorTaskId":id,"resumeToken":{"taskId":id},"appliedParams":null,"usage":null}),
    )
}

fn usage(response: &Value) -> Value {
    let measurements: Vec<_> = [
        ("duration", "duration", "seconds"),
        (
            "input_video_duration",
            "local.bgx.dashscope-media/input_duration",
            "seconds",
        ),
        (
            "output_video_duration",
            "local.bgx.dashscope-media/output_duration",
            "seconds",
        ),
        (
            "video_count",
            "local.bgx.dashscope-media/video_count",
            "local.bgx.dashscope-media/videos",
        ),
    ]
    .into_iter()
    .filter_map(|(field, metric, unit)| {
        response["usage"][field].as_number().map(|amount| {
            json!({
                "metric":metric,"unit":unit,"amount":amount.to_string(),"source":"reported"
            })
        })
    })
    .collect();
    if measurements.is_empty() {
        Value::Null
    } else {
        json!({"measurements":measurements})
    }
}

pub(crate) fn query(host: &mut dyn Host, payload: &Value, budget: u64) -> Result<Value, Failure> {
    if selected_operation(payload).is_none() {
        return Err(Failure::new("INVALID_INPUT", "万相续办操作身份无效"));
    }
    let id = task_id(&payload["vendorTaskId"])
        .filter(|id| payload["resumeToken"]["taskId"] == *id)
        .ok_or_else(|| Failure::new("INVALID_INPUT", "万相续办与原任务号不一致"))?;
    let (response, _) = exchange(
        host,
        payload,
        &format!("{QUERY}/{id}"),
        "GET",
        None,
        json!([]),
        budget,
    )?;
    let output = &response["output"];
    if task_id(&output["task_id"]) != Some(id) {
        return Err(Failure::new(
            "UPSTREAM_ERROR",
            "万相查询返回另一条任务或缺少任务号",
        ));
    }
    let mut outputs = Vec::new();
    let mut data = Value::Null;
    let mut failure = Value::Null;
    let status = match output["task_status"].as_str() {
        Some("PENDING") => "queued",
        Some("RUNNING") => "running",
        Some("SUCCEEDED") => {
            let video = output["video_url"]
                .as_str()
                .filter(|url| url.starts_with("https://"))
                .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "万相成功回执缺少视频下载地址"))?;
            outputs.push(download(host, video, "video", budget)?);
            data = json!({"taskId":id});
            "succeeded"
        }
        Some(state @ ("FAILED" | "CANCELED")) => {
            let (status, category, message) = if state == "FAILED" {
                ("failed", "vendorTaskFailed", "万相视频任务失败")
            } else {
                ("cancelled", "vendorTaskCancelled", "万相视频任务已取消")
            };
            failure = json!({"category":category,"message":message,"vendorCode":output["code"].as_str(),"details":null});
            status
        }
        _ => return Err(Failure::new("UPSTREAM_ERROR", "万相原任务状态未知")),
    };
    Ok(
        json!({"status":status,"progress":null,"outputs":outputs,"data":data,
        "usage":usage(&response),"terminalFailure":failure,"resumeToken":payload["resumeToken"]}),
    )
}
