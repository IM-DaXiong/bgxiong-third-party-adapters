use super::*;
use bgx_adapter_v2_sdk::{host::HostError, HostPortCallV2};

const FIRST: &[u8] = b"\x89PNG\r\n\x1a\nfirst";
const SECOND: &[u8] = b"\xff\xd8\xffsecond";
const RESPONSE: &[u8] = br#"{"request_id":"paid-frame-1","output":{"choices":[{"message":{"content":[{"image":"https://cdn.example/result.png"}]}}]},"usage":{"input_image_count":2,"output_image_count":1}}"#;

#[derive(Default)]
struct RecordingHost {
    body: Vec<u8>,
    calls: Vec<Value>,
}

impl Host for RecordingHost {
    fn call(&mut self, call: HostPortCallV2) -> Result<Value, HostError> {
        let call = serde_json::to_value(call).unwrap();
        let args = &call["args"];
        let handle = args["handle"].as_str().unwrap_or_default();
        match call["method"].as_str().unwrap() {
            "host-media.create" => Ok(json!("request")),
            "host-media.write" => {
                let bytes: Vec<u8> = serde_json::from_value(args["bytes"].clone()).unwrap();
                self.body.extend(bytes);
                Ok(Value::Null)
            }
            "host-media.finish" => Ok(Value::Null),
            "host-media.info" => {
                let (bytes, kind, mime) = match handle {
                    "original-first" => (FIRST.len(), "image", "image/png"),
                    "original-second" => (SECOND.len(), "image", "image/jpeg"),
                    "upstream" => (RESPONSE.len(), "file", "application/json"),
                    "artifact" => (74, "image", "image/png"),
                    _ => panic!("unexpected handle {handle}"),
                };
                Ok(
                    json!({"id": handle, "fileName": handle, "byteLength": bytes.to_string(), "kind": kind, "mime": mime}),
                )
            }
            "host-media.read" => Ok(json!(match handle {
                "original-first" => FIRST,
                "original-second" => SECOND,
                "upstream" => RESPONSE,
                _ => panic!("unexpected handle {handle}"),
            })),
            "host-http.execute" => {
                self.calls.push(args.clone());
                let handle = if args["plan"]["method"] == "POST" {
                    "upstream"
                } else {
                    "artifact"
                };
                Ok(json!({"status":200,"payload":{"handle":handle}}))
            }
            other => panic!("unexpected host operation {other}"),
        }
    }
}

#[test]
fn edit_submits_frozen_reference_order_and_downloads_without_account_credential() {
    let mut payload = json!({"capabilityKey":"image.generate","operationVersion":1,
        "operationId":format!("{PLUGIN}/image-edit"),"modelId":"qwen-image-3.0",
        "input":{"prompt":"use the first background with the second mug","referenceImages":["first","second"],"size":"512*512"},
        "media":[{"id":"second","kind":"image","handle":"original-second"},
                 {"id":"first","kind":"image","handle":"original-first"}],
        "configuration":{"endpointBaseUrl":"https://api.example","credentialRef":"account"}});
    payload["preparation"]["material"] = material(&payload, &payload["input"]).unwrap();
    let mut host = RecordingHost::default();
    let result = submit(&mut host, &payload, 10_000).unwrap();
    let request: Value = serde_json::from_slice(&host.body).unwrap();
    assert_eq!(
        request["input"]["messages"][0]["content"],
        json!([
            {"image": format!("data:image/png;base64,{}", STANDARD.encode(FIRST))},
            {"image": format!("data:image/jpeg;base64,{}", STANDARD.encode(SECOND))},
            {"text":"use the first background with the second mug"},
        ])
    );
    assert_eq!(host.calls.len(), 2);
    assert_eq!(
        host.calls[0]["plan"]["url"],
        format!("https://api.example{GENERATE_PATH}")
    );
    assert_eq!(host.calls[0]["credential"], "account");
    assert_eq!(
        host.calls[1]["plan"]["url"],
        "https://cdn.example/result.png"
    );
    assert!(host.calls[1]["credential"].is_null());
    assert_eq!(result["outputs"][0]["media"]["id"], "artifact");
    assert_eq!(result["outputs"][1]["media"]["id"], "upstream");
    assert_eq!(result["data"]["providerRequestId"], "paid-frame-1");
    assert_eq!(result["usage"]["measurements"][0]["amount"], "2");
}
