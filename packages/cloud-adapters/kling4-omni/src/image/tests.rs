use alloc::vec;
use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::{host::HostError, HostPortCallV2};

use super::*;

const IMAGE: &str = "https://p2-kling.klingai.com/fixture.png";
const TASK: &str = "task-72";
const MODEL: &str = "kling-v3";
const PATH: &str = "/v1/images/generations";
const FIRST: &[u8] = &[0x5a; 50_001];
const SECOND: &[u8] = b"test-second-jpeg";

#[derive(Default)]
struct Fixture {
    calls: Vec<Value>,
    raw: Vec<u8>,
    staged: Vec<u8>,
    download: bool,
    post_disconnect: bool,
    sign_denied: bool,
    signatures: usize,
    checkpoint: Option<Value>,
}

impl Host for Fixture {
    fn call(&mut self, request: HostPortCallV2) -> core::result::Result<Value, HostError> {
        let call = serde_json::to_value(request).unwrap();
        let args = &call["args"];
        let plan = &args["plan"];
        let buffered = |value: Value| json!({"status":200,"headers":[],"payload":{"kind":"buffered","bytes":value.to_string().into_bytes()}});
        self.calls.push(call.clone());
        Ok(match call["method"].as_str().unwrap() {
            "host-credential.sign" if self.sign_denied => {
                return Err(serde_json::from_value(json!({
                    "code":"PERMISSION_DENIED","message":"synthetic signing grant missing"
                }))
                .unwrap());
            }
            "host-credential.sign" => {
                self.signatures += 1;
                json!(format!("synthetic.jwt.{}", self.signatures))
            }
            "host-checkpoint.read" => json!({"revision":"0","valueJson":json!({
                "revision":"0","requestId":"fixture-image","requestDigest":"fixture",
                "state":"acceptance_unknown","vendorTaskId":null,"resumeToken":null,"result":null
            }).to_string()}),
            "host-checkpoint.write" => {
                assert_eq!(args["expectedRevision"], "0");
                self.checkpoint =
                    Some(serde_json::from_str(args["valueJson"].as_str().unwrap()).unwrap());
                json!({"revision":"1"})
            }
            "host-http.execute" if plan["method"] == "POST" => {
                if self.post_disconnect {
                    return Err(serde_json::from_value(
                        json!({"code":"RESOURCE_UNAVAILABLE","message":"mock disconnect"}),
                    )
                    .unwrap());
                }
                buffered(json!({"code":0,"data":{"task_id":TASK,"task_status":"submitted"}}))
            }
            "host-http.execute" if plan["url"] == IMAGE => {
                self.download = true;
                json!({"status":200,"headers":[],"payload":{"kind":"media","handle":"image-artifact"}})
            }
            "host-http.execute" => buffered(
                json!({"code":0,"data":{"task_id":TASK,"task_status":"succeed",
                "final_unit_deduction":"0.05","task_result":{"images":[{"url":IMAGE}]}}}),
            ),
            "host-media.create" if args["mimeHint"] == "application/octet-stream" => {
                json!("staged-request")
            }
            "host-media.create" => json!("provider-receipt"),
            "host-media.write" => {
                let destination = if args["handle"] == "staged-request" {
                    &mut self.staged
                } else {
                    &mut self.raw
                };
                destination
                    .extend(serde_json::from_value::<Vec<u8>>(args["bytes"].clone()).unwrap());
                Value::Null
            }
            "host-media.finish" | "host-media.release" => Value::Null,
            "host-media.info" if args["handle"] == "first" || args["handle"] == "second" => {
                let (bytes, mime) = if args["handle"] == "first" {
                    (FIRST, "image/png")
                } else {
                    (SECOND, "image/jpeg")
                };
                json!({"id":args["handle"],"kind":"image","mime":mime,
                    "byteLength":bytes.len().to_string(),"width":512,"height":512})
            }
            "host-media.read" => {
                let bytes = if args["handle"] == "first" {
                    FIRST
                } else {
                    SECOND
                };
                let offset = args["offset"].as_str().unwrap().parse::<usize>().unwrap();
                let limit = args["maxBytes"].as_u64().unwrap() as usize;
                json!(&bytes[offset..offset + limit])
            }
            "host-media.info" if args["handle"] == "image-artifact" => json!({
                "id":"image","kind":"image","mime":"image/png","fileName":"fixture.png","byteLength":"240"
            }),
            "host-media.info" => json!({
                "id":"response","kind":"file","mime":"application/json","fileName":"response.json",
                "byteLength":self.raw.len().to_string()
            }),
            method => panic!("unexpected host operation {method}"),
        })
    }
}

fn fixture() -> (Invocation, Value) {
    fixture_for(
        ImageMode::ClassicText,
        MODEL,
        json!({"prompt":"一杯清茶","negativePrompt":" 低清晰度 ","size":"2k","aspectRatio":"3:4"}),
    )
}

fn fixture_for(mode: ImageMode, model: &str, input: Value) -> (Invocation, Value) {
    let mut payload = json!({"schemaVersion":2,"modelId":model,"operationId":mode.operation(),"operationVersion":1,
        "inputSchemaDigest":mode.input_schema()["digest"],"capabilityKey":"image.generate",
        "input":input,
        "configuration":{"endpointBaseUrl":API,"credentialRef":"kling-test-bearer"}});
    payload["preparation"]["material"] = material(&payload, &payload["input"]).unwrap();
    let invocation = serde_json::from_value(json!({
        "protocolVersion":2,"operation":"submit","profileId":"fixture","bindingId":"fixture",
        "capabilitySlot":"image","requestId":"fixture-image","budgetMs":10000,"negotiatedVersion":2,
        "enabledFeatures":[],"payloadJson":"{}"
    }))
    .unwrap();
    (invocation, payload)
}

#[test]
fn exact_v3_body_and_original_query_download_preserve_result_and_account() {
    let (invocation, mut payload) = fixture();
    let mut host = Fixture::default();
    let accepted = submit(&mut host, &invocation, &payload).unwrap();
    assert_eq!(accepted["vendorTaskId"], TASK);
    assert_eq!(host.calls[0]["args"]["plan"]["url"], format!("{API}{PATH}"));
    let posted: Vec<u8> =
        serde_json::from_value(host.calls[0]["args"]["plan"]["body"]["bytes"].clone()).unwrap();
    assert_eq!(
        posted,
        serde_json::to_vec(&json!({
            "model_name":"kling-v3","prompt":"一杯清茶","negative_prompt":"低清晰度",
            "resolution":"2k","aspect_ratio":"3:4","n":1
        }))
        .unwrap()
    );
    payload["vendorTaskId"] = accepted["vendorTaskId"].clone();
    payload["resumeToken"] = accepted["resumeToken"].clone();
    let result = query(&mut host, &invocation, &payload).unwrap();
    assert_eq!(result["status"], "succeeded");
    assert_eq!(result["usage"]["measurements"][0]["amount"], "0.05");
    assert_eq!(result["outputs"][0]["media"]["id"], "image");
    assert_eq!(result["outputs"][1]["media"]["id"], "response");
    assert_eq!(
        serde_json::from_slice::<Value>(&host.raw).unwrap()["data"]["task_id"],
        TASK
    );
    assert!(host.download);
    let authenticated = vec![&host.calls[0], &host.calls[1]];
    assert!(authenticated
        .iter()
        .all(|call| call["args"]["credential"] == "kling-test-bearer"
            && call["args"]["plan"]["generatedHeaders"][0]["kind"] == "bearer"));
    assert_eq!(
        host.calls[1]["args"]["plan"]["url"],
        format!("{API}{PATH}/{TASK}")
    );
    assert!(host.calls[2]["args"]["credential"].is_null());
    payload["resumeToken"]["taskId"] = json!("other-task");
    let before = host.calls.len();
    assert_eq!(
        query(&mut host, &invocation, &payload).unwrap_err().code,
        "INVALID_INPUT"
    );
    assert_eq!(host.calls.len(), before);
}

#[test]
fn jwt_image_signs_for_submit_and_query_and_keeps_the_cdn_unauthenticated() {
    let (invocation, mut payload) = fixture();
    payload["configuration"]["credentialMode"] = json!("jwt-hs256");
    payload["configuration"]["credentialRef"] = json!("synthetic-image-jwt");
    let mut host = Fixture::default();
    let accepted = submit(&mut host, &invocation, &payload).unwrap();
    assert_eq!(accepted["vendorTaskId"], TASK);
    assert!(!accepted.to_string().contains("synthetic.jwt."));
    payload["vendorTaskId"] = accepted["vendorTaskId"].clone();
    payload["resumeToken"] = accepted["resumeToken"].clone();
    assert_eq!(
        query(&mut host, &invocation, &payload).unwrap()["status"],
        "succeeded"
    );
    for (index, url) in [format!("{API}{PATH}"), format!("{API}{PATH}/{TASK}")]
        .iter()
        .enumerate()
    {
        let sign = &host.calls[index * 2];
        assert_eq!(sign["method"], "host-credential.sign");
        assert_eq!(sign["args"]["handle"], "synthetic-image-jwt");
        assert_eq!(
            serde_json::from_str::<Value>(sign["args"]["specJson"].as_str().unwrap()).unwrap(),
            json!({"version":1,"kind":"jwt-hs256","claimsJson":"{}",
                "hostTime":{"lifetimeSeconds":1800,"notBeforeLeewaySeconds":5}})
        );
        let request = &host.calls[index * 2 + 1]["args"];
        assert_eq!(request["credential"], "synthetic-image-jwt");
        assert_eq!(request["plan"]["url"], *url);
        assert_eq!(request["plan"]["generatedHeaders"], json!([]));
        assert_eq!(
            request["plan"]["headers"]
                .as_array()
                .unwrap()
                .last()
                .unwrap(),
            &json!({"name":"Authorization","value":format!("Bearer synthetic.jwt.{}", index + 1)})
        );
    }
    let download = &host.calls[4]["args"];
    assert_eq!(download["plan"]["url"], IMAGE);
    assert_eq!(download["plan"]["headers"], json!([]));
    assert_eq!(download["plan"]["generatedHeaders"], json!([]));
    assert!(download["credential"].is_null());
}

#[test]
fn jwt_image_signing_denial_records_no_submission_and_never_sends_a_request() {
    let (invocation, mut payload) = fixture();
    payload["configuration"]["credentialMode"] = json!("jwt-hs256");
    let mut host = Fixture {
        sign_denied: true,
        ..Fixture::default()
    };
    assert_eq!(
        submit(&mut host, &invocation, &payload).unwrap_err().code,
        "PERMISSION_DENIED"
    );
    assert_eq!(host.calls[0]["method"], "host-credential.sign");
    assert_eq!(host.checkpoint.as_ref().unwrap()["state"], "rejected");
    assert!(host
        .calls
        .iter()
        .all(|call| call["method"] != "host-http.execute"));
}

#[test]
fn disconnect_remains_unknown_without_resubmission() {
    let (invocation, payload) = fixture();
    let mut host = Fixture {
        post_disconnect: true,
        ..Default::default()
    };
    assert_eq!(
        submit(&mut host, &invocation, &payload).unwrap_err().code,
        "ACCEPTANCE_UNKNOWN"
    );
    assert_eq!(host.calls.len(), 1);
}

fn references_payload() -> Value {
    json!([
        {"id":"second","handle":"second","kind":"image","mime":"image/jpeg","byteLength":SECOND.len().to_string()},
        {"id":"first","handle":"first","kind":"image","mime":"image/png","byteLength":FIRST.len().to_string()}
    ])
}

#[test]
fn scoped_reference_operations_send_original_bytes_to_each_provider_route() {
    for (mode, model, ids) in [
        (ImageMode::ClassicReference, MODEL, vec!["first"]),
        (
            ImageMode::OmniReference,
            "kling-image-o1",
            vec!["first", "second"],
        ),
        (
            ImageMode::OmniReference,
            "kling-v3-omni",
            vec!["first", "second"],
        ),
    ] {
        let ratio = if mode.is_omni() { "auto" } else { "1:1" };
        let (invocation, mut payload) = fixture_for(
            mode,
            model,
            json!({
                "prompt":"一杯清茶","referenceImages":ids,"size":"2k","aspectRatio":ratio
            }),
        );
        payload["media"] = references_payload();
        let mut host = Fixture::default();
        let accepted = submit(&mut host, &invocation, &payload).unwrap();
        let post = host
            .calls
            .iter()
            .find(|call| call["args"]["plan"]["method"] == "POST")
            .unwrap();
        assert_eq!(post["args"]["plan"]["url"], format!("{API}{}", mode.path()));
        assert_eq!(post["args"]["plan"]["body"]["handle"], "staged-request");
        let posted: Value = serde_json::from_slice(&host.staged).unwrap();
        let mut expected = json!({"model_name":model,"prompt":"一杯清茶","n":1,"resolution":"2k","aspect_ratio":ratio});
        if mode.is_omni() {
            expected["image_list"] =
                json!([{"image":STANDARD.encode(FIRST)},{"image":STANDARD.encode(SECOND)}]);
        } else {
            expected["image"] = json!(STANDARD.encode(FIRST));
        }
        assert_eq!(posted, expected);
        assert_eq!(accepted["resumeToken"]["modelId"], model);
        assert_eq!(accepted["resumeToken"]["operationId"], mode.operation());
        payload["vendorTaskId"] = accepted["vendorTaskId"].clone();
        payload["resumeToken"] = accepted["resumeToken"].clone();
        let complete = query(&mut host, &invocation, &payload).unwrap();
        assert_eq!(complete["status"], "succeeded");
        assert!(host
            .calls
            .iter()
            .any(|call| call["args"]["plan"]["url"] == format!("{API}{}/{TASK}", mode.path())));
    }
}

#[test]
fn omni_text_has_no_media_and_recovery_rejects_mode_and_model_changes() {
    for model in ["kling-image-o1", "kling-v3-omni"] {
        let (invocation, mut payload) = fixture_for(
            ImageMode::OmniText,
            model,
            json!({
                "prompt":" 一杯清茶 ","size":"1k","aspectRatio":"auto"
            }),
        );
        let mut host = Fixture::default();
        let accepted = submit(&mut host, &invocation, &payload).unwrap();
        let post = &host.calls[0]["args"]["plan"];
        assert_eq!(post["url"], format!("{API}/v1/images/omni-image"));
        let bytes: Vec<u8> = serde_json::from_value(post["body"]["bytes"].clone()).unwrap();
        assert_eq!(
            bytes,
            serde_json::to_vec(&json!({
                "model_name":model,"prompt":"一杯清茶","n":1,"resolution":"1k","aspect_ratio":"auto"
            }))
            .unwrap()
        );
        payload["vendorTaskId"] = accepted["vendorTaskId"].clone();
        payload["resumeToken"] = accepted["resumeToken"].clone();
        payload["operationId"] = json!(ImageMode::OmniReference.operation());
        assert_eq!(
            query(&mut host, &invocation, &payload).unwrap_err().code,
            "INVALID_INPUT"
        );
        payload["operationId"] = json!(ImageMode::OmniText.operation());
        payload["modelId"] = json!(if model == "kling-v3-omni" {
            "kling-image-o1"
        } else {
            "kling-v3-omni"
        });
        assert_eq!(
            query(&mut host, &invocation, &payload).unwrap_err().code,
            "INVALID_INPUT"
        );
        assert_eq!(host.calls.len(), 1);
    }
}

#[test]
fn reference_counts_match_the_existing_routes() {
    let (_, mut classic) = fixture_for(
        ImageMode::ClassicReference,
        MODEL,
        json!({"prompt":"一杯清茶","referenceImages":["one"]}),
    );
    classic["input"]["referenceImages"] = json!(["one", "two"]);
    assert_eq!(
        material(&classic, &classic["input"]).unwrap_err().0,
        "/referenceImages"
    );
    let (_, mut omni) = fixture_for(
        ImageMode::OmniReference,
        "kling-v3-omni",
        json!({
            "prompt":"一杯清茶","referenceImages":["one","two","three","four","five","six","seven","eight","nine"]
        }),
    );
    omni["input"]["referenceImages"]
        .as_array_mut()
        .unwrap()
        .push(json!("ten"));
    assert_eq!(
        material(&omni, &omni["input"]).unwrap_err().0,
        "/referenceImages"
    );
}
