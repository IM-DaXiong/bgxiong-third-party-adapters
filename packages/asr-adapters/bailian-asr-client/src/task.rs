use super::*;

const JSON: &str = "application/json";
fn vendor_id(value: &Value) -> Option<&str> {
    value.as_str().filter(|id| {
        !id.is_empty()
            && id.len() <= 64
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    })
}

pub(super) fn submit(
    host: &mut dyn Host,
    model: Model,
    payload: &Value,
    wav: &[u8],
    duration: u64,
    budget: u64,
) -> Result<Value, Failure> {
    let (origin, key) = http::origin(&payload["configuration"])?;
    let url = upload::temporary(host, origin, key, model.id(), wav, budget)?;
    let body = http::json_body(
        host,
        &submission(model, &url, payload["input"]["language"].as_str()),
        4096,
    )?;
    let reply = http::exchange(
        host,
        "POST",
        &format!("{origin}/api/v1/services/audio/asr/transcription"),
        Some(key),
        json!([{"name":"Content-Type","value":JSON},{"name":"X-DashScope-Async","value":"enable"},
        {"name":"X-DashScope-OssResourceResolve","value":"enable"}]),
        body,
        budget,
        true,
    )?;
    let id = vendor_id(&reply["output"]["task_id"])
        .ok_or_else(|| Failure::new("ACCEPTANCE_UNKNOWN", "百炼识别已提交但缺少可验证任务号"))?;
    // Carry the measured source length inside the opaque resume identity. The separate
    // vendorTaskId remains the exact upstream ID and is checked on every query.
    Ok(
        json!({"kind":"accepted","vendorTaskId":id,"resumeToken":{"schemaVersion":1,
        "jobId":format!("{id}:{duration}")},"appliedParams":null,"usage":null}),
    )
}

pub(super) fn submission(model: Model, url: &str, language: Option<&str>) -> Value {
    let mut body = json!({"model":model.id(),"input":{"file_urls":[url]}});
    if model == Model::Paraformer {
        body["parameters"] = json!({"channel_id":[0]});
    }
    if let Some(language @ ("zh" | "en")) = language {
        body["parameters"]["language_hints"] = json!([language]);
    }
    body
}

pub(super) fn query(host: &mut dyn Host, payload: &Value, budget: u64) -> Result<Value, Failure> {
    let (origin, key) = http::origin(&payload["configuration"])?;
    let invalid = || Failure::new("INVALID_INPUT", "百炼续办任务身份不符");
    let id = vendor_id(&payload["vendorTaskId"]).ok_or_else(invalid)?;
    let resume = &payload["resumeToken"];
    let (frozen, duration) = resume["jobId"]
        .as_str()
        .and_then(|value| value.split_once(':'))
        .ok_or_else(invalid)?;
    let duration: u64 = duration
        .parse()
        .ok()
        .filter(|d| (1..=300000).contains(d))
        .ok_or_else(invalid)?;
    if frozen != id || resume["schemaVersion"] != 1 {
        return Err(invalid());
    }
    let reply = http::exchange(
        host,
        "GET",
        &format!("{origin}/api/v1/tasks/{id}"),
        Some(key),
        json!([]),
        json!({"kind":"none"}),
        budget,
        false,
    )?;
    let out = &reply["output"];
    if vendor_id(&out["task_id"]) != Some(id) {
        return Err(Failure::new("UPSTREAM_ERROR", "百炼查询返回另一条任务"));
    }
    let mut data = Value::Null;
    let mut failure = Value::Null;
    let status = match out["task_status"].as_str() {
        Some("PENDING") => "queued",
        Some("RUNNING") => "running",
        Some("SUCCEEDED") => {
            if out["results"]
                .as_array()
                .is_none_or(|r| r.len() != 1 || r[0]["subtask_status"] != "SUCCEEDED")
            {
                failure = json!({"category":"vendorTaskFailed","message":"百炼单文件转写失败","vendorCode":null,"details":null});
                "failed"
            } else {
                let url = out["results"][0]["transcription_url"]
                    .as_str()
                    .filter(|url| upload::result_url(url))
                    .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "百炼转写结果地址无效"))?;
                let artifact = http::exchange(
                    host,
                    "GET",
                    url,
                    None,
                    json!([]),
                    json!({"kind":"none"}),
                    budget,
                    false,
                )?;
                data = result(&text(&artifact)?, duration);
                "succeeded"
            }
        }
        Some("FAILED") => {
            failure = json!({"category":"vendorTaskFailed","message":"百炼识别任务失败","vendorCode":null,"details":null});
            "failed"
        }
        Some("CANCELED") => {
            failure = json!({"category":"vendorTaskCancelled","message":"百炼识别任务已取消","vendorCode":null,"details":null});
            "cancelled"
        }
        _ => return Err(Failure::new("UPSTREAM_ERROR", "百炼任务状态无效")),
    };
    let usage = reply["usage"]["duration"].as_number().map(|seconds| {
        json!({"measurements":[{"metric":"duration","unit":"seconds","amount":seconds.to_string(),"source":"reported"}]})
    });
    Ok(
        json!({"status":status,"progress":null,"outputs":[],"data":data,
        "usage":usage,"terminalFailure":failure,"resumeToken":resume}),
    )
}

fn text(value: &Value) -> Result<String, Failure> {
    let bad = || Failure::new("UPSTREAM_ERROR", "百炼转写文件格式无效");
    let transcripts = value["transcripts"].as_array().ok_or_else(bad)?;
    let mut text = String::new();
    for item in transcripts {
        let part = item["text"].as_str().ok_or_else(bad)?.trim();
        if !part.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(part);
        }
    }
    Ok(text)
}
