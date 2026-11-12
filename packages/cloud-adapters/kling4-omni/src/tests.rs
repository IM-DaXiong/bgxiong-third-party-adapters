use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::{host::HostError, HostPortCallV2};

fn invocation(operation: &str, payload: &Value) -> Invocation {
    serde_json::from_value(
        json!({"protocolVersion":2,"operation":operation,"profileId":"test","bindingId":"test",
        "capabilitySlot":"video","requestId":"test-request","budgetMs":10000,"negotiatedVersion":2,
        "enabledFeatures":[],"payloadJson":payload.to_string()}),
    )
    .unwrap()
}

fn payload(mode: Mode) -> Value {
    let mut input = json!({"prompt":"第一行\n第二行 @image_1","inputMode":mode.input_mode(),
        "durationSeconds":5,"fps":24,"resolution":"1080p","aspectRatio":"16:9"});
    let media = if mode == Mode::ReferenceImages {
        input["referenceImages"] = json!([{"mediaId":"photo"}]);
        json!([{"id":"photo","kind":"image","mime":"image/png","fileName":"source.png","handle":"image-handle","byteLength":"1100001"}])
    } else {
        json!([])
    };
    json!({"schemaVersion":2,"modelId":mode.model(),"capabilityKey":"video.generate","operationId":mode.operation(),
        "operationVersion":1,"inputSchemaDigest":mode.input_digest(),"input":input,"media":media,"uploads":[],
        "configuration":{"endpointBaseUrl":API,"credentialRef":"managed-test-key"}})
}

#[derive(Default)]
struct Fixture {
    calls: Vec<Value>,
    staged: Vec<u8>,
    posted: Option<Value>,
    checkpoint: Option<Value>,
    unknown_submit: bool,
    deny_download: bool,
}

fn buffered(body: Value) -> Value {
    json!({"status":200,"headers":[],"payload":{"kind":"buffered","bytes":body.to_string().into_bytes()}})
}

impl Host for Fixture {
    fn call(&mut self, request: HostPortCallV2) -> core::result::Result<Value, HostError> {
        let request = serde_json::to_value(request).unwrap();
        let args = &request["args"];
        self.calls.push(request.clone());
        Ok(match request["method"].as_str().unwrap() {
            "host-checkpoint.read" => json!({"revision":"0","valueJson":json!({
                "revision":"0","requestId":"test-request","requestDigest":"fixture",
                "state":"acceptance_unknown","vendorTaskId":null,"resumeToken":null,"result":null
            }).to_string()}),
            "host-checkpoint.write" => {
                assert_eq!(args["expectedRevision"], "0");
                self.checkpoint =
                    Some(serde_json::from_str(args["valueJson"].as_str().unwrap()).unwrap());
                json!({"revision":"1"})
            }
            "host-media.info" if args["handle"] == "image-handle" => {
                json!({"id":"photo","kind":"image",
                "mime":"image/png","fileName":"source.png","byteLength":"1100001","width":1024,"height":768})
            }
            "host-media.info" => {
                json!({"id":"result","kind":"video","mime":"video/mp4","fileName":"result.mp4",
                "byteLength":"100","durationMs":"5000","width":1280,"height":720})
            }
            "host-media.create" => json!("staged-json"),
            "host-media.read" => {
                let count = args["maxBytes"].as_u64().unwrap();
                assert!(count <= 48 * 1024);
                json!(alloc::vec![19u8; count as usize])
            }
            "host-media.write" => {
                let bytes: Vec<u8> = serde_json::from_value(args["bytes"].clone()).unwrap();
                assert!(bytes.len() <= 64 * 1024);
                self.staged.extend(bytes);
                Value::Null
            }
            "host-media.finish" | "host-media.release" => Value::Null,
            "host-http.execute" => {
                let plan = &args["plan"];
                if plan["method"] == "POST" {
                    assert_eq!(plan["url"], format!("{API}{CREATE}"));
                    assert_eq!(args["credential"], "managed-test-key");
                    assert_eq!(plan["generatedHeaders"][0]["kind"], "bearer");
                    let bytes = if plan["body"]["kind"] == "media" {
                        assert_eq!(plan["body"]["handle"], "staged-json");
                        self.staged.clone()
                    } else {
                        serde_json::from_value(plan["body"]["bytes"].clone()).unwrap()
                    };
                    self.posted = Some(serde_json::from_slice(&bytes).unwrap());
                    if self.unknown_submit {
                        buffered(json!({"code":0,"data":{}}))
                    } else {
                        buffered(json!({"code":0,"data":{"id":"task-71","status":"submitted"}}))
                    }
                } else if plan["url"] == format!("{API}/tasks?task_ids=task-71") {
                    buffered(
                        json!({"code":0,"data":[{"id":"task-71","status":"succeeded",
                        "outputs":[{"type":"video","url":"https://p2-kling.klingai.com/result.mp4?sig=synthetic"}],
                        "usage":{"output_duration":"5"}}]}),
                    )
                } else {
                    assert!(args["credential"].is_null());
                    assert_eq!(plan["generatedHeaders"], json!([]));
                    json!({"status":if self.deny_download {403} else {200},"headers":[],
                        "payload":{"kind":"media","handle":"video-handle"}})
                }
            }
            other => panic!("unexpected port: {other}"),
        })
    }
}

#[test]
fn submit_and_query_use_frozen_task_and_never_send_key_to_media_origin() {
    let mut host = Fixture::default();
    let mut p = payload(Mode::Text);
    let accepted = submit(&mut host, &invocation("submit", &p), &p).unwrap();
    assert_eq!(accepted["vendorTaskId"], "task-71");
    assert_eq!(host.checkpoint.as_ref().unwrap()["state"], "accepted");
    assert_eq!(
        host.posted.as_ref().unwrap()["contents"][0]["text"],
        p["input"]["prompt"]
    );
    p["vendorTaskId"] = accepted["vendorTaskId"].clone();
    p["resumeToken"] = accepted["resumeToken"].clone();
    host.deny_download = true;
    assert!(
        query(&mut host, &invocation("query", &p), &p)
            .unwrap_err()
            .retryable
    );
    host.deny_download = false;
    let result = query(&mut host, &invocation("query", &p), &p).unwrap();
    assert_eq!(result["status"], "succeeded");
    assert_eq!(result["outputs"][0]["media"]["kind"], "video");
    assert_eq!(result["usage"]["measurements"][0]["amount"], "5");
    assert_eq!(
        host.calls
            .iter()
            .filter(|c| c["args"]["plan"]["method"] == "POST")
            .count(),
        1
    );
}

#[test]
fn large_reference_body_is_chunked_and_preserves_original_bytes() {
    let mut host = Fixture::default();
    let p = payload(Mode::ReferenceImages);
    submit(&mut host, &invocation("submit", &p), &p).unwrap();
    let sent = host.posted.unwrap();
    assert_eq!(sent["contents"][1]["id"], "image_1");
    assert_eq!(
        STANDARD
            .decode(sent["contents"][1]["url"].as_str().unwrap())
            .unwrap(),
        alloc::vec![19u8;1100001]
    );
    assert!(sent.get("model_name").is_none());
}

#[test]
fn missing_task_id_preserves_unknown_and_local_rejection_never_posts() {
    let mut host = Fixture {
        unknown_submit: true,
        ..Fixture::default()
    };
    let mut p = payload(Mode::Text);
    assert_eq!(
        submit(&mut host, &invocation("submit", &p), &p)
            .unwrap_err()
            .code,
        "ACCEPTANCE_UNKNOWN"
    );
    assert!(host.checkpoint.is_none());
    host.calls.clear();
    p["input"]["durationSeconds"] = json!(31);
    assert_eq!(
        submit(&mut host, &invocation("submit", &p), &p)
            .unwrap_err()
            .code,
        "INVALID_INPUT"
    );
    assert_eq!(host.checkpoint.unwrap()["state"], "rejected");
    assert!(host
        .calls
        .iter()
        .all(|c| c["method"] != "host-http.execute"));
}
