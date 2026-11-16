use std::collections::BTreeMap;

use bgx_adapter_v2_sdk::{host::HostError, Adapter, Host, HostPortCallV2, Invocation};
use bgx_generic_video_api::GenericVideoApi;
use serde_json::{json, Value};

#[derive(Default)]
struct Fixture {
    media: BTreeMap<String, (String, Vec<u8>)>,
    requests: Vec<Value>,
    result_url: Option<String>,
    submit_status: Option<u16>,
}

impl Fixture {
    fn store(&mut self, id: &str, kind: &str, bytes: &[u8]) {
        self.media.insert(id.into(), (kind.into(), bytes.into()));
    }
}

impl Host for Fixture {
    fn call(&mut self, call: HostPortCallV2) -> Result<Value, HostError> {
        let call = serde_json::to_value(call).unwrap();
        let args = &call["args"];
        let handle = || args["handle"].as_str().unwrap();
        let result = match call["method"].as_str().unwrap() {
            "host-media.create" => {
                self.store("request", "file", &[]);
                json!("request")
            }
            "host-media.write" => {
                self.media
                    .get_mut(handle())
                    .unwrap()
                    .1
                    .extend(serde_json::from_value::<Vec<u8>>(args["bytes"].clone()).unwrap());
                Value::Null
            }
            "host-media.finish" => Value::Null,
            "host-media.info" => {
                let (kind, bytes) = &self.media[handle()];
                json!({"kind":kind,"id":handle(),"byteLength":bytes.len().to_string(),
                    "mime":if kind=="video" {"video/mp4"} else {"application/octet-stream"},"fileName":handle()})
            }
            "host-media.read" => {
                let offset = args["offset"].as_str().unwrap().parse::<usize>().unwrap();
                let bytes = &self.media[handle()].1;
                let end = (offset + args["maxBytes"].as_u64().unwrap() as usize).min(bytes.len());
                json!(&bytes[offset..end])
            }
            "host-http.execute" => {
                self.requests.push(args.clone());
                let plan = &args["plan"];
                if plan["method"] == "POST" {
                    if let Some(status) = self.submit_status {
                        self.store(
                            "rejected",
                            "file",
                            br#"{"code":"FixtureSubmitResponse","request_id":"fixture-request"}"#,
                        );
                        return Ok(json!({"status":status,"payload":{"handle":"rejected"}}));
                    }
                    self.store("accepted", "file", br#"{"data":{"task_id":"video-123"}}"#);
                    json!({"status":200,"payload":{"handle":"accepted"}})
                } else if self.result_url.as_deref() == plan["url"].as_str() {
                    assert!(args["credential"].is_null());
                    self.store("video", "video", b"original video bytes");
                    json!({"status":200,"payload":{"handle":"video"}})
                } else {
                    let raw = json!({"data":{"task_id":"video-123","task_status":"Complete","task_result":{
                        "videos":[{"url":self.result_url}]},"video_url":[self.result_url]},
                        "usage":{"prompt_tokens":2,"completion_tokens":3}});
                    self.store("query", "file", &serde_json::to_vec(&raw).unwrap());
                    json!({"status":200,"payload":{"handle":"query"}})
                }
            }
            other => panic!("unexpected host port: {other}"),
        };
        Ok(result)
    }
}

fn invoke(fixture: &mut Fixture, operation: &str, payload: Value) -> Value {
    let request: Invocation = serde_json::from_value(json!({"protocolVersion":2,"operation":operation,
        "bindingId":"fixture","profileId":"fixture","capabilitySlot":"video","requestId":"fixture","budgetMs":5000,
        "negotiatedVersion":2,"enabledFeatures":[],"payloadJson":payload.to_string()})).unwrap();
    let response = serde_json::to_value(GenericVideoApi::invoke(request, fixture)).unwrap();
    response["dataJson"]
        .as_str()
        .map_or(response.clone(), |value| {
            serde_json::from_str(value).unwrap()
        })
}

fn selection(key: &str) -> Value {
    json!({"modelId":"generic-video-api","capabilityKey":"video.generate",
        "operationId":format!("local.bgx.generic-video-api/{key}"),"operationVersion":1})
}

fn config() -> Value {
    json!({"endpointBaseUrl":"https://api.example.invalid/prefix/v1/adapter/","credentialRef":"mock-key","upstreamModelId":"fixture-upstream"})
}

#[test]
fn submits_the_explicit_routes_and_retains_original_reference_order() {
    let mut fixture = Fixture::default();
    fixture.store("first", "image", b"first original");
    fixture.store("second", "image", b"second original");
    let media = json!([
        {"id":"first","kind":"image","handle":"first"},
        {"id":"second","kind":"image","handle":"second"}
    ]);
    for (key, mode, path, field, expected) in [
        ("text", "text_to_video", "text2video", None, Value::Null),
        (
            "image",
            "image_to_video",
            "image2video",
            Some("first_frame_b64"),
            json!("Zmlyc3Qgb3JpZ2luYWw="),
        ),
        (
            "camera",
            "camera_control",
            "image2video",
            Some("first_frame_b64"),
            json!("Zmlyc3Qgb3JpZ2luYWw="),
        ),
        (
            "multi-image",
            "multi_image_to_video",
            "multi_image2video",
            Some("reference_images_b64"),
            json!(["c2Vjb25kIG9yaWdpbmFs", "Zmlyc3Qgb3JpZ2luYWw="]),
        ),
    ] {
        let mut input = json!({"inputMode":mode,"prompt":"first prompt line\nsecond prompt line","providerParameters":{"seconds":"4","camera":{"x":1}}});
        match key {
            "image" | "camera" => input["firstFrame"] = json!(["first"]),
            "multi-image" => input["referenceImages"] = json!(["second", "first"]),
            _ => {}
        }
        let selection = selection(key);
        let ready = invoke(
            &mut fixture,
            "probe",
            json!({"kind":"operation_input_preflight","environment":{"ready":true},
            "selection":selection,"configuration":config(),"input":input}),
        );
        assert_eq!(ready["status"], "ready");
        let mut submit = selection;
        submit["configuration"] = config();
        submit["input"] = input;
        submit["media"] = media.clone();
        submit["preparation"] = json!({"material":ready["material"]});
        let accepted = invoke(&mut fixture, "submit", submit);
        assert_eq!(accepted["vendorTaskId"], "video-123");
        let call = fixture.requests.last().unwrap();
        assert_eq!(
            call["plan"]["url"],
            format!("https://api.example.invalid/prefix/v1/adapter/videos/{path}")
        );
        assert_eq!(call["credential"], "mock-key");
        let body: Value = serde_json::from_slice(&fixture.media["request"].1).unwrap();
        assert_eq!(body["model"], "fixture-upstream");
        assert_eq!(body["prompt"], "first prompt line\nsecond prompt line");
        assert_eq!(body["seconds"], "4");
        assert_eq!(body["camera"]["x"], 1);
        if let Some(field) = field {
            assert_eq!(body[field], expected);
        } else {
            assert!(
                body.get("first_frame_b64").is_none() && body.get("reference_images_b64").is_none()
            );
        }
    }
}

#[test]
fn binds_original_task_and_downloads_only_one_copy_without_the_api_key() {
    let mut fixture = Fixture {
        result_url: Some("https://media.example.invalid/video.mp4".into()),
        ..Default::default()
    };
    let mut payload = selection("text");
    payload["configuration"] = config();
    payload["vendorTaskId"] = json!("video-123");
    payload["resumeToken"] = json!({"taskId":"some-other-task"});
    assert_eq!(
        invoke(&mut fixture, "query", payload.clone())["error"]["code"],
        "INVALID_INPUT"
    );
    assert!(fixture.requests.is_empty());
    payload["resumeToken"] = json!({"taskId":"video-123"});
    let query = invoke(&mut fixture, "query", payload);
    assert_eq!(fixture.requests.len(), 2);
    assert_eq!(
        fixture.requests[0]["plan"]["url"],
        "https://api.example.invalid/prefix/v1/adapter/videos/video-123"
    );
    assert_eq!(fixture.requests[0]["credential"], "mock-key");
    assert_eq!(query["status"], "succeeded");
    assert_eq!(query["outputs"][0]["role"], "video");
    assert_eq!(query["outputs"][1]["role"], "provider-response");
    assert_eq!(
        query["usage"]["measurements"],
        json!([
            {"metric":"input_tokens","unit":"tokens","amount":"2","source":"reported"},
            {"metric":"output_tokens","unit":"tokens","amount":"3","source":"reported"}
        ])
    );
}

#[test]
fn distinguishes_explicit_rejection_from_unknown_acceptance_and_never_retries_submit() {
    let mut fixture = Fixture::default();
    let selection = selection("text");
    let input = json!({"inputMode":"text_to_video","prompt":"A blue ball moves."});
    let prep = |selection: &Value, input: &Value| {
        json!({"kind":"operation_input_preflight","environment":{"ready":true},
        "selection":selection,"configuration":config(),"input":input})
    };
    let mut wrong = selection.clone();
    wrong["modelId"] = json!("unregistered-provider");
    let rejected = invoke(&mut fixture, "probe", prep(&wrong, &input));
    assert_eq!(rejected["status"], "rejected");
    assert!(fixture.requests.is_empty());

    let prepared = invoke(&mut fixture, "probe", prep(&selection, &input));
    let mut submit = selection;
    submit["configuration"] = config();
    submit["input"] = input;
    submit["preparation"] = json!({"material":prepared["material"]});
    for (status, code) in [
        (400, "INVALID_INPUT"),
        (402, "RESOURCE_UNAVAILABLE"),
        (408, "ACCEPTANCE_UNKNOWN"),
        (503, "ACCEPTANCE_UNKNOWN"),
    ] {
        fixture.submit_status = Some(status);
        let previous = fixture.requests.len();
        let reply = invoke(&mut fixture, "submit", submit.clone());
        assert_eq!(reply["error"]["code"], code);
        assert_eq!(reply["error"]["vendorCode"], "FixtureSubmitResponse");
        assert_eq!(reply["error"]["vendorRequestId"], "fixture-request");
        assert_eq!(fixture.requests.len(), previous + 1);
    }
}
