use alloc::{collections::BTreeMap, format, string::String, vec};
use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::{host::HostError, HostPortCallV2};

use super::*;
use crate::PLUGIN;

const FIRST: &[u8] = b"\x89PNG\r\n\x1a\nfirst";
const SECOND: &[u8] = b"\xff\xd8\xffsecond";
const IMAGE_URL: &str = "https://cdn.example/result.png";

struct Fixture {
    bytes: BTreeMap<String, Vec<u8>>,
    calls: Vec<Value>,
    events: Vec<Value>,
    transport_failure: Option<&'static str>,
}

impl Fixture {
    fn new() -> Self {
        let response = json!({"request_id":"mock-request","data":[
            {"image_url":IMAGE_URL},
            {"image_base64":STANDARD.encode(SECOND)}
        ],"usage":{"total_tokens":6}});
        Self {
            bytes: BTreeMap::from([
                ("ref-first".into(), FIRST.into()),
                ("ref-second".into(), SECOND.into()),
                (
                    "original-response".into(),
                    serde_json::to_vec(&response).unwrap(),
                ),
                ("original-cdn".into(), FIRST.into()),
            ]),
            calls: Vec::new(),
            events: Vec::new(),
            transport_failure: None,
        }
    }
}

impl Host for Fixture {
    fn call(&mut self, call: HostPortCallV2) -> Result<Value, HostError> {
        let call = serde_json::to_value(call).unwrap();
        let args = &call["args"];
        let handle = args["handle"].as_str().unwrap_or_default();
        match call["method"].as_str().unwrap() {
            "host-media.create" => {
                let name = match args["kind"].as_str() {
                    Some("file") => "request",
                    Some("image") => "base64-image",
                    other => panic!("unexpected media kind {other:?}"),
                };
                assert!(self.bytes.insert(name.into(), Vec::new()).is_none());
                Ok(json!(name))
            }
            "host-media.write" => {
                let bytes: Vec<u8> = serde_json::from_value(args["bytes"].clone()).unwrap();
                self.bytes.get_mut(handle).unwrap().extend(bytes);
                Ok(Value::Null)
            }
            "host-media.finish" => Ok(Value::Null),
            "host-media.info" => {
                let bytes = &self.bytes[handle];
                let (kind, mime) = match handle {
                    "original-response" => ("file", "application/json"),
                    "ref-first" | "original-cdn" => ("image", "image/png"),
                    "ref-second" | "base64-image" => ("image", "image/jpeg"),
                    _ => panic!("unexpected handle {handle}"),
                };
                Ok(
                    json!({"id":handle,"fileName":handle,"kind":kind,"mime":mime,"byteLength":bytes.len().to_string()}),
                )
            }
            "host-media.read" => {
                let bytes = &self.bytes[handle];
                let start = args["offset"].as_str().unwrap().parse::<usize>().unwrap();
                let end = start + args["maxBytes"].as_u64().unwrap() as usize;
                Ok(json!(&bytes[start..end.min(bytes.len())]))
            }
            "host-http.execute" => {
                self.calls.push(args.clone());
                if let Some(code) = self.transport_failure {
                    return Err(serde_json::from_value(
                        json!({"code":code,"message":"connection dropped after send"}),
                    )
                    .unwrap());
                }
                let handle = if args["plan"]["method"] == "POST" {
                    "original-response"
                } else {
                    "original-cdn"
                };
                Ok(json!({"status":200,"payload":{"handle":handle}}))
            }
            "host-stream.emit-event" => {
                self.events.push(args["event"].clone());
                Ok(Value::Null)
            }
            method => panic!("unexpected port {method}"),
        }
    }
}

fn submit_fixture(operation: &str, model: &str, references: &[&str]) -> (Invocation, Value) {
    let config = if model == "configured-gateway-model" {
        json!({"endpointBaseUrl":"https://gateway.example/tenant/v1/chat/completions/","credentialRef":"account",
            "gatewayModelId":" e p - configured "})
    } else {
        json!({"endpointBaseUrl":"https://gateway.example/tenant/v1/chat/completions/","credentialRef":"account"})
    };
    let mut payload = json!({
        "capabilityKey":"image.generate","operationId":format!("{PLUGIN}/{operation}"),"operationVersion":1,"modelId":model,
        "configuration":config,"input":{"prompt":"Keep the first composition","size":" 1024x1024 ","responseFormat":"b64_json"},
        "media":[{"id":"second","kind":"image","handle":"ref-second"},{"id":"first","kind":"image","handle":"ref-first"}]
    });
    if !references.is_empty() {
        payload["input"]["referenceImages"] = json!(references);
    }
    payload["preparation"]["material"] =
        parameters::material(&payload, &payload["input"], &config).unwrap();
    let invocation = serde_json::from_value(json!({"protocolVersion":2,"operation":"submit","profileId":"fixture","bindingId":"fixture",
        "capabilitySlot":"image","requestId":"fixture-request","budgetMs":1000,"negotiatedVersion":2,"enabledFeatures":["stream-events-v1"],"payloadJson":"{}"})).unwrap();
    (invocation, payload)
}

#[test]
fn maps_endpoint_without_replacing_the_selected_gateway() {
    for (input, expected) in [
        (
            "https://gateway.example",
            "https://gateway.example/v1/images/generations",
        ),
        (
            "https://gateway.example/custom/v1/",
            "https://gateway.example/custom/v1/images/generations",
        ),
        (
            "https://gateway.example/custom/v1/chat/completions",
            "https://gateway.example/custom/v1/images/generations",
        ),
        (
            "https://gateway.example/api/v3/images/generations?version=2",
            "https://gateway.example/api/v3/images/generations?version=2",
        ),
        (
            "https://gateway.example/api/v3",
            "https://gateway.example/api/v3/v1/images/generations",
        ),
    ] {
        assert_eq!(parameters::endpoint(input).as_deref(), Some(expected));
    }
    assert!(parameters::endpoint("https://someone:pass@gateway.example/v1").is_none());
    assert!(parameters::endpoint("file:///tmp/secret").is_none());
}

#[test]
fn keeps_gateway_body_and_preserves_both_original_result_forms() {
    for refs in [vec!["first"], vec!["first", "second"]] {
        let (invocation, payload) =
            submit_fixture("gateway-reference", "configured-gateway-model", &refs);
        let mut host = Fixture::new();
        let result = submit(&mut host, &invocation, &payload).unwrap();
        let first = format!("data:image/png;base64,{}", STANDARD.encode(FIRST));
        let second = format!("data:image/jpeg;base64,{}", STANDARD.encode(SECOND));
        let images = if refs.len() == 1 {
            json!(first)
        } else {
            json!([first, second])
        };
        let expected = json!({"model":"ep-configured","prompt":"Keep the first composition","response_format":"b64_json",
            "size":"1024x1024","image":images});
        assert_eq!(
            host.bytes["request"],
            serde_json::to_vec(&expected).unwrap()
        );
        assert_eq!(host.calls.len(), 2);
        assert_eq!(
            host.calls[0]["plan"]["url"],
            "https://gateway.example/tenant/v1/images/generations"
        );
        assert_eq!(host.calls[0]["credential"], "account");
        assert_eq!(
            host.calls[0]["plan"]["generatedHeaders"][0]["kind"],
            "bearer"
        );
        assert_eq!(host.calls[1]["plan"]["url"], IMAGE_URL);
        assert!(host.calls[1]["credential"].is_null());
        assert_eq!(host.bytes["base64-image"], SECOND);
        assert_eq!(result["outputs"][0]["media"]["id"], "original-cdn");
        assert_eq!(result["outputs"][2]["media"]["id"], "original-response");
        assert_eq!(result["data"]["providerRequestId"], "mock-request");
        assert_eq!(result["data"]["imageCount"], 2);
        assert_eq!(result["usage"]["measurements"][0]["amount"], "6");
    }
}

#[test]
fn unknown_acceptance_is_reported_after_one_post() {
    for code in ["TIMEOUT", "RESOURCE_UNAVAILABLE"] {
        let (invocation, mut payload) = submit_fixture("dall-e-3-text", "dall-e-3", &[]);
        let mut host = Fixture::new();
        host.transport_failure = Some(code);
        let error = submit(&mut host, &invocation, &payload).unwrap_err();
        assert_eq!(error.code, "ACCEPTANCE_UNKNOWN");
        assert_eq!(host.calls.len(), 1);
        payload["configuration"]["credentialRef"] = json!("");
        assert_eq!(
            submit(&mut host, &invocation, &payload).unwrap_err().code,
            "INVALID_INPUT"
        );
        assert_eq!(host.calls.len(), 1);
    }
    assert_eq!(
        error_from_response(402, &Value::Null).unwrap().code,
        "RESOURCE_UNAVAILABLE"
    );
}

#[test]
fn keeps_the_current_gateway_reference_limit() {
    let references = [
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    ];
    let (_, mut payload) =
        submit_fixture("gateway-reference", "configured-gateway-model", &references);
    assert!(parameters::material(&payload, &payload["input"], &payload["configuration"]).is_ok());
    payload["input"]["referenceImages"]
        .as_array_mut()
        .unwrap()
        .push(json!("ten"));
    assert_eq!(
        parameters::material(&payload, &payload["input"], &payload["configuration"])
            .unwrap_err()
            .0,
        "/referenceImages"
    );
}
