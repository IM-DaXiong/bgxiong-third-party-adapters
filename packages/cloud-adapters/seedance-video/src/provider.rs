use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use bgx_adapter_v2_sdk::{host, Host};
use serde_json::{json, Value};

use super::{
    media,
    parameters::{self, Operation, MODEL},
};

const TASKS: &str = "/contents/generations/tasks";

pub(crate) struct Failure {
    pub code: &'static str,
    pub message: &'static str,
    pub vendor_code: Option<String>,
    pub vendor_task_id: Option<String>,
}
pub(crate) type Result<T> = core::result::Result<T, Failure>;

impl Failure {
    pub(crate) fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            vendor_code: None,
            vendor_task_id: None,
        }
    }
    pub(crate) fn invalid(message: &'static str) -> Self {
        Self::new("INVALID_INPUT", message)
    }
}

pub(crate) fn port(host: &mut dyn Host, method: &str, args: Value) -> Result<Value> {
    host::call(host, method, &args).map_err(|error| {
        if serde_json::to_value(error)
            .ok()
            .is_some_and(|value| value["code"] == "PERMISSION_DENIED")
        {
            return Failure::new("PERMISSION_DENIED", "SDK 拒绝了网络或媒体授权");
        }
        Failure::new("UPSTREAM_ERROR", "SDK 网络或资源操作失败")
    })
}

struct Endpoint<'a> {
    url: String,
    credential: &'a str,
    budget: u64,
}

impl<'a> Endpoint<'a> {
    fn new(payload: &'a Value, budget: u64) -> Result<Self> {
        let config = &payload["configuration"];
        let base = config["endpointBaseUrl"]
            .as_str()
            .map(str::trim)
            .map(|url| url.trim_end_matches('/'))
            .filter(|url| !url.is_empty())
            .ok_or(Failure::invalid("方舟 API 地址缺失"))?;
        let credential = config["credentialRef"]
            .as_str()
            .ok_or(Failure::invalid("方舟账号引用缺失"))?;
        let base = base.strip_suffix("/chat/completions").unwrap_or(base);
        let url = if base.ends_with(TASKS) {
            String::from(base)
        } else {
            format!("{base}{TASKS}")
        };
        Ok(Self {
            url,
            credential,
            budget,
        })
    }

    fn request(
        &self,
        host: &mut dyn Host,
        method: &str,
        task: Option<&str>,
        body: Option<&Value>,
    ) -> Result<Value> {
        let url = task.map_or_else(|| self.url.clone(), |id| format!("{}/{id}", self.url));
        let (headers, body) = match body {
            Some(value) => {
                let handle = media::request_body(host, value)?;
                (
                    json!([{"name":"Content-Type","value":"application/json"}]),
                    json!({"kind":"media","handle":handle}),
                )
            }
            None => (json!([]), json!({"kind":"none"})),
        };
        let response = port(host, "host-http.execute", json!({"plan":{
            "method":method,"url":url,"headers":headers,
            "generatedHeaders":[{"name":"Authorization","kind":"bearer","targetField":"Authorization"}],
            "body":body,"sink":{"kind":"buffer","maxBytes":65536},"timeoutMs":self.budget,
        },"credential":self.credential})).map_err(|error| {
            if method == "POST" && error.code == "UPSTREAM_ERROR" {
                return Failure::new("ACCEPTANCE_UNKNOWN", "方舟提交结果未确认");
            }
            error
        })?;
        let status = response["status"]
            .as_u64()
            .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "方舟 HTTP 状态缺失"))?;
        let bytes = serde_json::from_value::<Vec<u8>>(response["payload"]["bytes"].clone())
            .unwrap_or_default();
        let parsed = serde_json::from_slice::<Value>(&bytes);
        if !(200..300).contains(&status) {
            let code = match status {
                401 | 403 => "UNAUTHORIZED",
                429 => "RATE_LIMITED",
                400 | 422 => "INVALID_INPUT",
                _ => "UPSTREAM_ERROR",
            };
            let mut failure = Failure::new(code, "方舟拒绝请求");
            failure.vendor_code = parsed.as_ref().ok().and_then(|value| {
                value
                    .pointer("/error/code")
                    .or_else(|| value.get("code"))
                    .and_then(Value::as_str)
                    .map(String::from)
            });
            failure.vendor_task_id = task.map(String::from);
            return Err(failure);
        }
        if method == "DELETE" && bytes.is_empty() {
            return Ok(Value::Null);
        }
        parsed.map_err(|_| {
            Failure::new(
                if method == "POST" {
                    "ACCEPTANCE_UNKNOWN"
                } else {
                    "UPSTREAM_ERROR"
                },
                "方舟返回了无法识别的响应",
            )
        })
    }
}

fn task_id(payload: &Value) -> Result<&str> {
    let id = payload["vendorTaskId"]
        .as_str()
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 120
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .ok_or(Failure::invalid("原任务编号无效"))?;
    if payload["resumeToken"]["taskId"] != id {
        return Err(Failure::invalid("原任务编号和恢复凭证不一致"));
    }
    Ok(id)
}

fn usage(value: &Value) -> Value {
    let mut measurements: Vec<_> = [
        ("prompt_tokens", "input_tokens"),
        ("completion_tokens", "output_tokens"),
        ("total_tokens", "total_tokens"),
    ]
    .into_iter()
    .filter_map(|(field, metric)| {
        value["usage"][field].as_u64().map(|amount| {
            json!({"metric":metric,"unit":"tokens","amount":amount.to_string(),"source":"reported"})
        })
    })
    .collect();
    if let Some(amount) = value
        .pointer("/usage/tool_usage/web_search")
        .and_then(Value::as_u64)
    {
        measurements.push(json!({"metric":"local.bgx.seedance-video/web_search","unit":"local.bgx.seedance-video/searches",
            "amount":amount.to_string(),"source":"reported"}));
    }
    if measurements.is_empty() {
        Value::Null
    } else {
        json!({"measurements":measurements})
    }
}

pub(crate) fn probe(host: &mut dyn Host, budget: u64, payload: &Value) -> Result<Value> {
    Endpoint::new(payload, budget)?.request(host, "GET", None, None)?;
    Ok(
        json!({"packageValid":true,"connection":"ready","authentication":"ready","resources":"not_checked","issues":[]}),
    )
}

pub(crate) fn submit(
    host: &mut dyn Host,
    budget: u64,
    operation: &Operation,
    payload: &Value,
) -> Result<Value> {
    let input = &payload["input"];
    let mut fields =
        parameters::material(operation, input).map_err(|(_, reason)| Failure::invalid(reason))?;
    if payload["preparation"]["material"] != Value::Object(fields.clone()) {
        return Err(Failure::invalid("冻结参数与操作预检不一致"));
    }
    let mut content = Vec::new();
    if let Some(prompt) = input["prompt"]
        .as_str()
        .filter(|prompt| !prompt.trim().is_empty())
    {
        content.push(json!({"type":"text","text":prompt}));
    }
    let sources = payload["media"]
        .as_array()
        .ok_or(Failure::invalid("素材租约列表缺失"))?;
    for slot in operation.slots {
        let link_type = format!("{}_url", slot.kind);
        for reference in input[slot.field].as_array().into_iter().flatten() {
            let medium = sources
                .iter()
                .find(|medium| medium["id"] == *reference)
                .ok_or(Failure::invalid("操作素材与 SDK 租约不一致"))?;
            let url = media::data_url(host, medium, slot.kind)?;
            content
                .push(json!({"type":link_type,(link_type.clone()):{"url":url},"role":slot.role}));
        }
    }
    for url in parameters::take_video_sources(&mut fields) {
        content.push(json!({"type":"video_url","video_url":{"url":url},"role":"reference_video"}));
    }
    let mut body = Value::Object(fields);
    body["model"] = json!(MODEL);
    body["content"] = json!(content);
    let response = Endpoint::new(payload, budget)?.request(host, "POST", None, Some(&body))?;
    let Some(id) = response["id"].as_str().filter(|id| !id.trim().is_empty()) else {
        let mut failure = Failure::new("ACCEPTANCE_UNKNOWN", "方舟提交回执缺少任务编号");
        failure.vendor_code = response["error"]["code"].as_str().map(String::from);
        return Err(failure);
    };
    Ok(
        json!({"kind":"accepted","vendorTaskId":id,"appliedParams":null,
        "resumeToken":{"taskId":id},"usage":usage(&response)}),
    )
}

pub(crate) fn query(host: &mut dyn Host, budget: u64, payload: &Value) -> Result<Value> {
    let id = task_id(payload)?;
    let response = Endpoint::new(payload, budget)?.request(host, "GET", Some(id), None)?;
    if response.get("id").is_some_and(|actual| actual != id) {
        return Err(Failure::new("UPSTREAM_ERROR", "方舟任务回执身份不一致"));
    }
    let mut outputs = Vec::new();
    let mut data = Value::Null;
    let status = match response["status"].as_str() {
        Some("queued") => "queued",
        Some("running") => "running",
        Some("succeeded") => {
            let video = response
                .pointer("/content/video_url")
                .and_then(Value::as_str)
                .ok_or(Failure::new("UPSTREAM_ERROR", "方舟成功回执缺少视频地址"))?;
            outputs.push(media::download(host, video, "video", "video", budget)?);
            if let Some(frame) = response
                .pointer("/content/last_frame_url")
                .and_then(Value::as_str)
            {
                outputs.push(media::download(host, frame, "image", "last-frame", budget)?);
            }
            data = json!({"taskId":id});
            "succeeded"
        }
        Some("failed") => "failed",
        Some("cancelled" | "canceled") => "cancelled",
        Some("expired") => "expired",
        _ => return Err(Failure::new("UPSTREAM_ERROR", "无法识别方舟视频任务状态")),
    };
    let terminal_failure = match status {
        "failed" => Some(("vendorTaskFailed", "方舟视频任务失败")),
        "cancelled" => Some(("vendorTaskCancelled", "方舟视频任务已取消")),
        "expired" => Some(("vendorTaskExpired", "方舟视频任务已过期")),
        _ => None,
    }
    .map_or(Value::Null, |(category, message)| {
        json!({"category":category,"message":message,
        "vendorCode":response.pointer("/error/code").and_then(Value::as_str),"details":null})
    });
    Ok(
        json!({"status":status,"progress":null,"outputs":outputs,"data":data,
        "usage":usage(&response),"terminalFailure":terminal_failure,"resumeToken":payload["resumeToken"]}),
    )
}

pub(crate) fn cancel(host: &mut dyn Host, budget: u64, payload: &Value) -> Result<Value> {
    let id = task_id(payload)?;
    Endpoint::new(payload, budget)?.request(host, "DELETE", Some(id), None)?;
    Ok(json!({"outcome":"remote_requested"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bgx_adapter_v2_sdk::{host::HostError, HostPortCallV2};

    #[derive(Default)]
    struct ArkFixture {
        request_bytes: Vec<u8>,
        http_calls: Vec<Value>,
    }

    impl Host for ArkFixture {
        fn call(&mut self, call: HostPortCallV2) -> core::result::Result<Value, HostError> {
            let call = serde_json::to_value(call).unwrap();
            let args = &call["args"];
            let value = match call["method"].as_str().unwrap() {
                "host-media.info" if args["handle"] == "ref" => json!({
                    "kind":"image","mime":"image/jpeg","byteLength":"3","id":"source","fileName":"ref.jpg"
                }),
                "host-media.info" => json!({
                    "kind":"video","mime":"video/mp4","byteLength":"3","id":"video","fileName":"source.mp4"
                }),
                "host-media.read" => json!([255, 216, 255]),
                "host-media.create" => json!("body"),
                "host-media.write" => {
                    let bytes: Vec<u8> = serde_json::from_value(args["bytes"].clone()).unwrap();
                    self.request_bytes.extend(bytes);
                    Value::Null
                }
                "host-media.finish" => Value::Null,
                "host-http.execute" => {
                    self.http_calls.push(args.clone());
                    let plan = &args["plan"];
                    let json_response = |data: Value| {
                        json!({
                            "status":200,"payload":{"bytes":serde_json::to_vec(&data).unwrap()}
                        })
                    };
                    match plan["method"].as_str().unwrap() {
                        "POST" => json_response(json!({"id":"cgt-fixture"})),
                        "GET" if plan["sink"]["kind"] == "media" => json!({
                            "status":200,"payload":{"handle":"result"}
                        }),
                        "GET" => json_response(json!({"id":"cgt-fixture","status":"succeeded",
                            "content":{"video_url":"https://media.example.test/original.mp4"},
                            "usage":{"completion_tokens":7,"total_tokens":7}})),
                        "DELETE" => json!({"status":204,"payload":{"bytes":[]}}),
                        _ => panic!("unexpected method"),
                    }
                }
                _ => panic!("unexpected port"),
            };
            Ok(value)
        }
    }

    #[test]
    fn edit_uses_documented_video_sources_and_rejects_embedded_video_before_sending() {
        let operation = parameters::OPERATIONS.iter().find(|op| op.id == "edit").unwrap();
        let configuration = json!({"endpointBaseUrl":"https://ark.example.test/api/v3","credentialRef":"key"});
        let mut input = json!({"inputMode":"video_edit","durationSeconds":-1,"aspectRatio":"adaptive",
            "referenceVideoSources":"data:video/mp4;base64,AAAA"});
        let mut host = ArkFixture::default();
        let payload = |input: &Value, material: Value| json!({"input":input,"configuration":configuration,
            "preparation":{"material":material},"media":[]});
        assert!(submit(&mut host, 5000, operation, &payload(&input, Value::Null)).is_err());
        assert!(host.http_calls.is_empty());
        input["referenceVideoSources"] = json!("https://media.example.test/original.mp4\r\nasset://asset-20261116-demo");
        let material = parameters::material(operation, &input).unwrap();
        let result = submit(&mut host, 5000, operation, &payload(&input, Value::Object(material)))
            .unwrap_or_else(|error| panic!("{}", error.message));
        let sent: Value = serde_json::from_slice(&host.request_bytes).unwrap();
        assert_eq!(sent["omni_reference_task_type"], "edit");
        assert_eq!(sent["duration"], -1);
        assert!(sent.get("referenceVideoSources").is_none());
        assert_eq!(sent["content"], json!([
            {"type":"video_url","video_url":{"url":"https://media.example.test/original.mp4"},"role":"reference_video"},
            {"type":"video_url","video_url":{"url":"asset://asset-20261116-demo"},"role":"reference_video"}
        ]));
        assert_eq!(result["vendorTaskId"], "cgt-fixture");
        assert_eq!(host.http_calls.len(), 1);
    }

    #[test]
    fn keyframe_submission_and_resume_keep_one_original_and_confine_credentials() {
        let operation = parameters::OPERATIONS
            .iter()
            .find(|op| op.id == "first-last-frame")
            .unwrap();
        let input = json!({"inputMode":"first_last_frame","firstFrame":["source"],"lastFrame":["source"],
            "durationSeconds":4,"generateAudio":false,"prompt":"original text"});
        let material = parameters::material(operation, &input).unwrap();
        let configuration =
            json!({"endpointBaseUrl":"https://ark.example.test/api/v3","credentialRef":"key"});
        let mut host = ArkFixture::default();
        let result = submit(&mut host, 5000, operation, &json!({"input":input,"configuration":configuration,
            "preparation":{"material":material},"media":[{"id":"source","handle":"ref","kind":"image"}]}))
            .unwrap_or_else(|error| panic!("{}", error.message));
        assert_eq!(result["vendorTaskId"], "cgt-fixture");
        let sent: Value = serde_json::from_slice(&host.request_bytes).unwrap();
        assert_eq!(
            sent["content"],
            json!([
                {"type":"text","text":"original text"},
                {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,/9j/"},"role":"first_frame"},
                {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,/9j/"},"role":"last_frame"}
            ])
        );
        assert_eq!(sent["ratio"], "adaptive");
        assert_eq!(sent["generate_audio"], false);

        let mut original = json!({"configuration":configuration,"vendorTaskId":"cgt-fixture","resumeToken":result["resumeToken"]});
        assert_eq!(
            query(&mut host, 5000, &original).unwrap_or_else(|error| panic!("{}", error.message))
                ["status"],
            "succeeded"
        );
        assert_eq!(
            host.http_calls[1]["plan"]["url"],
            "https://ark.example.test/api/v3/contents/generations/tasks/cgt-fixture"
        );
        assert!(host.http_calls[2]["credential"].is_null());
        assert_eq!(host.http_calls[2]["plan"]["generatedHeaders"], json!([]));
        original["vendorTaskId"] = json!("other");
        assert!(cancel(&mut host, 5000, &original).is_err());
        assert_eq!(host.http_calls.len(), 3);
        original["vendorTaskId"] = json!("cgt-fixture");
        assert_eq!(
            cancel(&mut host, 5000, &original).unwrap_or_else(|error| panic!("{}", error.message))
                ["outcome"],
            "remote_requested"
        );
        assert_eq!(host.http_calls[3]["plan"]["method"], "DELETE");
    }
}
