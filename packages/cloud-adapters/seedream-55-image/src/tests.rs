use std::{collections::BTreeMap, io::Cursor};

use bgx_adapter_v2_sdk::{HostPortCallV2, HostPortCallV2HostError};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};

use super::*;

const URL: &str = "https://ark-content-generation-v2-cn-beijing.tos-cn-beijing.volces.com/test.png?example=fixture";

struct Object {
    id: String,
    kind: String,
    mime: String,
    bytes: Vec<u8>,
}

struct Fixture {
    objects: BTreeMap<String, Object>,
    plans: Vec<Value>,
    events: Vec<Value>,
    body: Value,
    status: u16,
    response: Value,
    png: Vec<u8>,
    transport_error: bool,
    download_status: u16,
}

impl Fixture {
    fn new() -> Self {
        Self {
            objects: BTreeMap::new(),
            plans: vec![],
            events: vec![],
            body: Value::Null,
            status: 200,
            response: json!({
                "request_id":"fixture-request","data":[{"url":URL}],
                "usage":{"generated_images":1,"input_tokens":0},
            }),
            png: image(80, ImageFormat::Png),
            transport_error: false,
            download_status: 200,
        }
    }

    fn put(&mut self, kind: &str, mime: &str, bytes: Vec<u8>) -> String {
        let id = format!("media-{}", self.objects.len());
        self.objects.insert(
            id.clone(),
            Object {
                id: id.clone(),
                kind: kind.to_owned(),
                mime: mime.to_owned(),
                bytes,
            },
        );
        id
    }

    fn invoke(&mut self, choice: &Value, input: Value, media: Value, stale: bool) -> Value {
        let preflight = parameters::preflight(&json!({
            "selection":choice,"input":input,"environment":{"ready":true},"requestDigest":"fixture",
        }));
        let mut payload = choice.clone();
        for (key, value) in json!({
            "configuration":{"endpointBaseUrl":API_ORIGIN,"credentialRef":CREDENTIAL},
            "input":input,"media":media,
            "preparation":{"material":if stale {Value::Null} else {preflight["material"].clone()}},
        })
        .as_object()
        .unwrap()
        {
            payload[key] = value.clone();
        }
        let request: Invocation = serde_json::from_value(json!({
            "protocolVersion":2,"requestId":"fixture","operation":"submit",
            "enabledFeatures":["stream-events-v1"],"budgetMs":180000,"payloadJson":payload.to_string(),"bindingId":"fixture","profileId":"fixture","capabilitySlot":"image","negotiatedVersion":2,
        })).unwrap();
        let output = serde_json::to_value(Seedream::invoke(request, self)).unwrap();
        output["dataJson"]
            .as_str()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(output)
    }
}

impl Host for Fixture {
    fn call(&mut self, call: HostPortCallV2) -> Result<Value, HostPortCallV2HostError> {
        let call = serde_json::to_value(call).unwrap();
        let args = &call["args"];
        let handle = args["handle"].as_str().unwrap_or_default();
        Ok(match call["method"].as_str().unwrap() {
            "host-media.create" => json!(self.put(
                args["kind"].as_str().unwrap(),
                args["mimeHint"].as_str().unwrap(),
                vec![]
            )),
            "host-media.write" => {
                let chunk: Vec<u8> = serde_json::from_value(args["bytes"].clone()).unwrap();
                self.objects.get_mut(handle).unwrap().bytes.extend(chunk);
                Value::Null
            }
            "host-media.finish" => Value::Null,
            "host-media.info" => {
                let item = &self.objects[handle];
                json!({"id":item.id,"kind":item.kind,"mime":item.mime,"byteLength":item.bytes.len().to_string(),"fileName":"fixture","width":32,"height":32})
            }
            "host-media.read" => {
                let item = &self.objects[handle];
                let offset = args["offset"].as_str().unwrap().parse::<usize>().unwrap();
                let count = args["maxBytes"].as_u64().unwrap() as usize;
                json!(&item.bytes[offset..(offset + count).min(item.bytes.len())])
            }
            "host-stream.emit-event" => {
                self.events.push(args["event"].clone());
                Value::Null
            }
            "host-http.execute" => {
                self.plans.push(args.clone());
                let plan = &args["plan"];
                if plan["method"] == "POST" {
                    if self.transport_error {
                        return Err(serde_json::from_value(json!({
                            "code":"RESOURCE_UNAVAILABLE","message":"fixture interrupted",
                        }))
                        .unwrap());
                    }
                    let body_handle = plan["body"]["handle"].as_str().unwrap();
                    self.body = serde_json::from_slice(&self.objects[body_handle].bytes).unwrap();
                    let raw = self.put(
                        "file",
                        "application/json",
                        self.response.to_string().into_bytes(),
                    );
                    json!({"status":self.status,"payload":{"handle":raw},"headers":[]})
                } else {
                    let image_handle = self.put("image", "image/png", self.png.clone());
                    json!({"status":self.download_status,"payload":{"handle":image_handle},"headers":[]})
                }
            }
            method => panic!("unexpected host operation: {method}"),
        })
    }
}

fn image(alpha: u8, format: ImageFormat) -> Vec<u8> {
    let picture =
        DynamicImage::ImageRgba8(ImageBuffer::from_pixel(32, 32, Rgba([36, 89, 100, alpha])));
    let mut output = Cursor::new(Vec::new());
    match format {
        ImageFormat::Jpeg => {
            DynamicImage::ImageRgb8(picture.to_rgb8()).write_to(&mut output, format)
        }
        _ => picture.write_to(&mut output, format),
    }
    .unwrap();
    output.into_inner()
}

fn selection(id: &str) -> Value {
    json!({"operationId":id,"capabilityKey":"image.generate","modelId":MODEL,"operationVersion":1})
}

#[test]
fn transparent_url_maps_exact_request_and_preserves_original() {
    let mut host = Fixture::new();
    host.response["data"][0]["url"] =
        json!("https://ark-acg-cn-beijing.tos-cn-beijing.volces.com/original.png?example=fixture");
    let input = json!({"prompt":" 花朵 ","size":"1024x1024","outputFormat":"png","responseFormat":"url","background":"transparent","watermark":true});
    let result = host.invoke(&selection(GENERATE), input, json!([]), false);
    assert_eq!(
        host.body,
        json!({
            "model":MODEL,"prompt":" 花朵 ","size":"2K","output_format":"png",
            "response_format":"url","background":"transparent","watermark":true,"stream":false,
        })
    );
    assert_eq!(host.plans.len(), 2);
    assert_eq!(host.plans[0]["plan"]["url"], ENDPOINT);
    assert_eq!(host.plans[0]["credential"], CREDENTIAL);
    assert_eq!(host.plans[1]["credential"], Value::Null);
    assert_eq!(host.plans[1]["plan"]["generatedHeaders"], json!([]));
    assert_eq!(result["kind"], "completed", "{result}");
    let output = result["outputs"][0]["media"]["handle"].as_str().unwrap();
    assert_eq!(host.objects[output].bytes, host.png);
    assert_eq!(host.events[1]["payload"]["measurements"][0]["amount"], "0");
    assert_eq!(host.events[2]["payload"]["media"]["handle"], output);
}

#[test]
fn size_tier_and_frame_reach_the_wire_for_both_operations() {
    for (tier, ratio, expected) in [("2K", "16:9", "2816x1584"), ("1K", "4:5", "916x1145")] {
        for operation in [GENERATE, EDIT] {
            let mut host = Fixture::new();
            let mut input = json!({"prompt":"fixture","resolution":tier,"aspectRatio":ratio});
            let media = if operation == EDIT {
                let id = host.put("image", "image/png", image(80, ImageFormat::Png));
                input["referenceImages"] = json!([{"mediaId":id}]);
                json!([{"id":id,"kind":"image","handle":id,"mime":"image/png"}])
            } else {
                json!([])
            };
            let result = host.invoke(&selection(operation), input, media, false);
            assert_eq!(result["kind"], "completed", "{result}");
            assert_eq!(host.body["size"], expected);
            assert!(host.body.get("aspectRatio").is_none());
        }
    }
}

#[test]
fn jpeg_base64_uses_exact_bytes_and_explicit_false() {
    let mut host = Fixture::new();
    let jpeg = image(255, ImageFormat::Jpeg);
    host.response["data"] = json!([{"b64_json":STANDARD.encode(&jpeg)}]);
    let result = host.invoke(&selection(GENERATE), json!({
        "prompt":"fixture","outputFormat":"jpeg","responseFormat":"b64_json","background":"opaque","watermark":false,
    }), json!([]), false);
    assert_eq!(host.plans.len(), 1);
    assert_eq!(host.body["size"], "2K");
    assert_eq!(host.body["watermark"], false);
    assert_eq!(result["appliedParams"]["watermark"], false);
    assert!(result["appliedParams"].get("size").is_none());
    let output = result["outputs"][0]["media"]["handle"].as_str().unwrap();
    assert_eq!(host.objects[output].bytes, jpeg);
}

#[test]
fn dimensions_and_closed_parameters_reject_before_post() {
    for valid in [
        "1K",
        "1.5K",
        "2K",
        "1280x720",
        "2880x1440",
        "3840x240",
        "240x3840",
    ] {
        assert!(parameters::dimensions_valid(valid), "{valid}");
    }
    for invalid in [
        "512x512",
        "4096x4096",
        "4000x240",
        "0x2048",
        "99999999999999x30",
        "1280X720",
    ] {
        assert!(!parameters::dimensions_valid(invalid), "{invalid}");
    }
    for input in [
        json!({"prompt":"  "}),
        json!({"prompt":"p","watermark":null}),
        json!({"prompt":"p","resolution":"custom","size":12}),
        json!({"prompt":"p","background":"transparent","outputFormat":"jpeg"}),
        json!({"prompt":"p","stream":true}),
        json!({"prompt":"p","model":MODEL}),
        json!({"prompt":"p","referenceImages":[]}),
    ] {
        let mut host = Fixture::new();
        let result = host.invoke(&selection(GENERATE), input, json!([]), false);
        assert_eq!(result["error"]["code"], "INVALID_INPUT", "{result}");
        assert!(host.plans.is_empty());
    }
}

#[test]
fn ordered_reference_shapes_cover_one_two_and_ten() {
    for count in [1, 2, 10] {
        let mut host = Fixture::new();
        let mut media = vec![];
        let mut input = vec![];
        let mut expected = vec![];
        for index in 0..count {
            let bytes = image(index as u8, ImageFormat::Png);
            expected.push(format!("data:image/png;base64,{}", STANDARD.encode(&bytes)));
            let id = host.put("image", "image/png", bytes);
            media.insert(
                0,
                json!({"id":id,"kind":"image","mime":"image/png","handle":id}),
            );
            input.push(json!({"mediaId":id}));
        }
        let result = host.invoke(
            &selection(EDIT),
            json!({"prompt":"p","referenceImages":input}),
            json!(media),
            false,
        );
        assert_eq!(result["kind"], "completed", "{result}");
        assert_eq!(
            host.body["image"],
            if count == 1 {
                json!(expected[0])
            } else {
                json!(expected)
            }
        );
    }
}

#[test]
fn invalid_reference_counts_leases_and_preparation_never_post() {
    for count in [0, 11] {
        let mut host = Fixture::new();
        let refs: Vec<_> = (0..count)
            .map(|i| json!({"mediaId":format!("image-{i}")}))
            .collect();
        let result = host.invoke(
            &selection(EDIT),
            json!({"prompt":"p","referenceImages":refs}),
            json!([]),
            false,
        );
        assert_eq!(result["error"]["code"], "INVALID_INPUT");
        assert!(host.plans.is_empty());
    }
    let mut host = Fixture::new();
    let result = host.invoke(&selection(GENERATE), json!({"prompt":"p"}), json!([]), true);
    assert_eq!(result["error"]["code"], "INVALID_INPUT");
    let bytes = image(255, ImageFormat::Jpeg);
    let id = host.put("image", "image/png", bytes);
    let refs = json!([{"mediaId":id}]);
    let media = json!([{"id":id,"kind":"image","mime":"image/png","handle":id}]);
    let result = host.invoke(
        &selection(EDIT),
        json!({"prompt":"p","referenceImages":refs}),
        media,
        false,
    );
    assert_eq!(result["error"]["code"], "INVALID_INPUT");
    let result = host.invoke(
        &selection(EDIT),
        json!({"prompt":"p","referenceImages":[{"mediaId":id},{"mediaId":id}]}),
        json!([]),
        false,
    );
    assert_eq!(result["error"]["code"], "INVALID_INPUT");
    assert!(host.plans.is_empty());
}

#[test]
fn unverified_urls_and_redirects_never_retry_generation() {
    for url in [
        "https://other.example/image.png",
        "https://ark-acg-cn-beijing.tos-cn-beijing.volces.com.example/image.png",
        "http://ark-content-generation-v2-cn-beijing.tos-cn-beijing.volces.com/test.png",
        "https://user@ark-content-generation-v2-cn-beijing.tos-cn-beijing.volces.com/test.png",
    ] {
        let mut host = Fixture::new();
        host.response["data"] = json!([{"url":url}]);
        let result = host.invoke(
            &selection(GENERATE),
            json!({"prompt":"p"}),
            json!([]),
            false,
        );
        assert_eq!(result["error"]["code"], "PERMISSION_DENIED");
        assert_eq!(host.plans.len(), 1);
        assert_eq!(host.events[0]["payload"]["role"], "provider-response");
    }
    let mut host = Fixture::new();
    host.download_status = 302;
    let result = host.invoke(
        &selection(GENERATE),
        json!({"prompt":"p"}),
        json!([]),
        false,
    );
    assert_eq!(result["error"]["code"], "UPSTREAM_ERROR");
    assert_eq!(host.plans.len(), 2);
}

#[test]
fn malformed_or_opaque_results_cannot_complete_transparent_generation() {
    let mut truncated = image(80, ImageFormat::Png);
    truncated.truncate(45);
    for bytes in [
        image(255, ImageFormat::Png),
        truncated,
        image(255, ImageFormat::Jpeg),
    ] {
        let mut host = Fixture::new();
        host.png = bytes;
        let result = host.invoke(
            &selection(GENERATE),
            json!({"prompt":"p","background":"transparent"}),
            json!([]),
            false,
        );
        assert_eq!(result["error"]["code"], "UPSTREAM_ERROR");
        assert!(!host
            .events
            .iter()
            .any(|event| event["payload"]["role"] == "artifact"));
    }
    let mut host = Fixture::new();
    host.response["data"] = json!([{"b64_json":"!"}]);
    let result = host.invoke(
        &selection(GENERATE),
        json!({"prompt":"p","responseFormat":"b64_json"}),
        json!([]),
        false,
    );
    assert_eq!(result["error"]["code"], "UPSTREAM_ERROR");
    assert_eq!(host.plans.len(), 1);
}

#[test]
fn paid_attempt_failure_preserves_request_usage_and_partial_media() {
    for (status, code) in [(401, "UNAUTHORIZED"), (429, "RATE_LIMITED")] {
        let mut host = Fixture::new();
        host.status = status;
        host.response["error"] = json!({"code":"fixture.sensitive","message":"private body"});
        let result = host.invoke(
            &selection(GENERATE),
            json!({"prompt":"p"}),
            json!([]),
            false,
        );
        assert_eq!(result["error"]["code"], code);
        assert_eq!(result["error"]["vendorRequestId"], "fixture-request");
        assert_eq!(host.plans.len(), 1);
    }
    let mut host = Fixture::new();
    host.response["data"] = json!([{"error":{"code":"ContentRejected"}},{"url":URL}]);
    let result = host.invoke(
        &selection(GENERATE),
        json!({"prompt":"p"}),
        json!([]),
        false,
    );
    assert_eq!(result["error"]["vendorCode"], "ContentRejected");
    assert_eq!(host.events[2]["payload"]["role"], "artifact");
    assert_eq!(host.plans.len(), 2);

    let mut host = Fixture::new();
    host.transport_error = true;
    let result = host.invoke(
        &selection(GENERATE),
        json!({"prompt":"p"}),
        json!([]),
        false,
    );
    assert_eq!(result["error"]["code"], "ACCEPTANCE_UNKNOWN");
    assert_eq!(host.plans.len(), 1);
}
