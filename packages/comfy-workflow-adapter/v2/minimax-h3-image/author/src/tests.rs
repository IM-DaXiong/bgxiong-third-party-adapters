use super::*;
use bgx_adapter_v2_sdk::{host::HostError, HostPortCallV2};

#[test]
fn schema_digest_is_stable_across_checkout_line_endings() {
    assert_eq!(schema("input", "{\r\n}\r\n"), schema("input", "{\n}\n"));
}

#[derive(Default)]
struct RecordedHost {
    calls: Vec<Value>,
    checkpoint: Value,
    fault: &'static str,
}

fn buffered(value: Value) -> Value {
    json!({"status":200,"payload":{"bytes":serde_json::to_vec(&value).unwrap()}})
}

impl Host for RecordedHost {
    fn call(&mut self, call: HostPortCallV2) -> Result<Value, HostError> {
        let call = serde_json::to_value(call).unwrap();
        self.calls.push(call.clone());
        let args = &call["args"];
        match call["method"].as_str().unwrap() {
            "host-media.info" => {
                if self.fault == "missing-file" && args["handle"] == "lease-b" {
                    return Err(host::invalid_wire("Missing file"));
                }
                Ok(
                    json!({"id":"output-media","kind":"image","mime":"image/png","fileName":"result.png","byteLength":"4096"}),
                )
            }
            "host-media.read" => {
                let mut bytes = vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13];
                bytes.extend_from_slice(b"IHDR");
                let width: u32 = if self.fault == "wrong-size" {
                    1024
                } else {
                    2048
                };
                bytes.extend_from_slice(&width.to_be_bytes());
                bytes.extend_from_slice(&2048u32.to_be_bytes());
                if self.fault == "bad-png" {
                    bytes[0] = 0;
                }
                Ok(json!(bytes))
            }
            "host-checkpoint.read" => {
                if self.fault == "checkpoint-read" {
                    return Err(host::invalid_wire("Storage unavailable"));
                }
                Ok(json!({"revision":"0","valueJson":"{\"revision\":\"0\"}"}))
            }
            "host-checkpoint.write" => {
                if self.fault == "checkpoint-write" {
                    return Err(host::invalid_wire("Storage unavailable"));
                }
                self.checkpoint =
                    serde_json::from_str(args["valueJson"].as_str().unwrap()).unwrap();
                Ok(json!({}))
            }
            "host-http.execute" => {
                let plan = &args["plan"];
                let url = plan["url"].as_str().unwrap();
                if url.ends_with("/system_stats") {
                    let version = if self.fault == "old-service" {
                        "0.44.2"
                    } else {
                        "0.45.0"
                    };
                    return Ok(buffered(json!({"system":{"comfyui_version":version}})));
                }
                if url.ends_with("/object_info") {
                    let mut definitions = json!({});
                    for name in [
                        "FixturePrompt",
                        "FixtureSampler",
                        "FixtureLatent",
                        "FixtureModel",
                        "FixtureSave",
                        "FixtureLoad",
                    ] {
                        definitions[name] = json!({"input":{"required":{}}});
                    }
                    definitions["FixtureModel"]["input"]["required"]["model_name"] =
                        json!([["fixture-model.safetensors"]]);
                    if self.fault == "missing-model" {
                        definitions["FixtureModel"]["input"]["required"]["model_name"] =
                            json!([[]]);
                    }
                    if self.fault == "missing-node" {
                        definitions
                            .as_object_mut()
                            .unwrap()
                            .remove("FixtureSampler");
                    }
                    return Ok(buffered(definitions));
                }
                if url.ends_with("/upload/image") {
                    let handle = plan["body"]["parts"][0]["handle"].as_str().unwrap();
                    return Ok(buffered(
                        json!({"name":format!("{handle}.png"),"subfolder":"uploaded"}),
                    ));
                }
                if url.ends_with("/prompt") {
                    if self.fault == "lost-submit" {
                        return Err(host::invalid_wire("Transport interrupted"));
                    }
                    if self.fault == "unreadable-submit" {
                        return Ok(json!({"status":200,"payload":{"bytes":[0]}}));
                    }
                    if self.fault == "server-error" {
                        let mut r = buffered(json!({"error":{"type":"internal"}}));
                        r["status"] = json!(500);
                        return Ok(r);
                    }
                    if self.fault == "node-errors" {
                        return Ok(buffered(
                            json!({"prompt_id":"remote-17","node_errors":{"sampler":"invalid"}}),
                        ));
                    }
                    return Ok(buffered(json!({"prompt_id":"remote-17","node_errors":{}})));
                }
                if url.ends_with("/history/remote-17") {
                    if self.fault == "queued" || self.fault == "running" {
                        return Ok(buffered(json!({})));
                    }
                    let node = if self.fault == "wrong-output" {
                        "unrelated"
                    } else {
                        OUTPUT_NODE
                    };
                    return Ok(buffered(
                        json!({"remote-17":{"status":{"status_str":"success","completed":true},"outputs":{node:{"images":[{"filename":"result.png","subfolder":"","type":"output"}]}}}}),
                    ));
                }
                if url.ends_with("/queue") {
                    let key = if self.fault == "running" {
                        "queue_running"
                    } else {
                        "queue_pending"
                    };
                    return Ok(buffered(json!({key:[[0,"remote-17"]]})));
                }
                if url.contains("/view?") {
                    return Ok(json!({"status":200,"payload":{"handle":"output-lease"}}));
                }
                panic!("Unexpected HTTP request {url}");
            }
            method => panic!("Unexpected host call {method}"),
        }
    }
}

fn payload(count: usize) -> Value {
    let edit = count > 0;
    let mut input = json!({"prompt":"照搬 <Picture 1>，保留原文。\nOnly change the cup.","width":2048,"height":2048,"seed":7});
    let media: Vec<Value> = ["a","b"].into_iter().take(count).map(|id| json!({"id":id,"kind":"image","handle":format!("lease-{id}"),"fileName":format!("{id}.png"),"mime":"image/png","byteLength":"100"})).collect();
    if edit {
        input["referenceImages"] = json!(media
            .iter()
            .map(|m| json!({"mediaId":m["id"]}))
            .collect::<Vec<_>>());
    }
    json!({"operationId":if edit {EDIT_OPERATION} else {TEXT_OPERATION},"modelId":if edit {EDIT_MODEL} else {TEXT_MODEL},"input":input,"media":media,"configuration":{"endpointBaseUrl":"http://fixture.invalid:8188"}})
}

fn invoke(operation: &str, payload: &Value, host: &mut RecordedHost) -> Value {
    let request: Invocation = serde_json::from_value(json!({"protocolVersion":2,"operation":operation,"profileId":"dev","capabilitySlot":"image","requestId":"request-1","budgetMs":10000,"payloadJson":payload.to_string(),"bindingId":"candidate","negotiatedVersion":2,"enabledFeatures":[]})).unwrap();
    serde_json::to_value(H3Image::invoke(request, host)).unwrap()
}

fn data(response: &Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    serde_json::from_str(response["dataJson"].as_str().unwrap()).unwrap()
}

fn prompt_calls(host: &RecordedHost) -> Vec<&Value> {
    host.calls
        .iter()
        .filter(|c| {
            c["args"]["plan"]["url"]
                .as_str()
                .is_some_and(|u| u.ends_with("/prompt"))
        })
        .collect()
}

#[test]
fn two_capabilities_project_separate_models_and_required_media() {
    let caps = data(&invoke(
        "capabilities",
        &json!({}),
        &mut RecordedHost::default(),
    ));
    let contracts = caps["capabilityContracts"].as_array().unwrap();
    assert_eq!(contracts.len(), 2);
    for (index, contract) in contracts.iter().enumerate() {
        assert_eq!(contract["capabilityKey"], "image.generate");
        let operation = &contract["operations"][0];
        assert_eq!(operation["mediaSlots"].as_array().unwrap().len(), index);
        assert_eq!(
            operation["uiSchema"]["controls"].as_array().unwrap().len(),
            4
        );
        let input: Value =
            serde_json::from_str(contract["schemas"][0]["schemaJson"].as_str().unwrap()).unwrap();
        assert!(input["properties"].get("negativePrompt").is_none());
        assert!(input["properties"].get("steps").is_none());
        if index == 1 {
            assert_eq!(operation["mediaSlots"][0]["minCount"], 1);
            assert_eq!(operation["mediaSlots"][0]["maxCount"], 2);
        }
    }
}

#[test]
fn text_single_and_double_reference_submit_checkpoint_and_resume() {
    for count in 0..=2 {
        let p = payload(count);
        let mut host = RecordedHost::default();
        let accepted = data(&invoke("submit", &p, &mut host));
        assert_eq!(accepted["vendorTaskId"], "remote-17");
        assert_eq!(host.checkpoint["result"], accepted);
        let calls = prompt_calls(&host);
        assert_eq!(calls.len(), 1);
        let bytes: Vec<u8> =
            serde_json::from_value(calls[0]["args"]["plan"]["body"]["bytes"].clone()).unwrap();
        let submitted: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            submitted["prompt"]["prompt"]["inputs"]["text"],
            p["input"]["prompt"]
        );
        assert_eq!(submitted["prompt"]["sampler"]["inputs"]["steps"], 6);
        assert_eq!(submitted["prompt"]["sampler"]["inputs"]["cfg"], 1);
        assert_eq!(submitted["prompt"].get("reference2").is_some(), count == 2);
        // A fresh host instance simulates process restart, using the persisted receipt only.
        let mut resumed = RecordedHost::default();
        let query = json!({"operationId":p["operationId"],"configuration":p["configuration"],"vendorTaskId":host.checkpoint["vendorTaskId"],"resumeToken":host.checkpoint["resumeToken"]});
        assert_eq!(
            data(&invoke("query", &query, &mut resumed))["status"],
            "succeeded"
        );
        assert_eq!(prompt_calls(&resumed).len(), 0);
    }
}

#[test]
fn upload_order_follows_input_and_returned_names() {
    let mut p = payload(2);
    p["input"]["referenceImages"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    let mut host = RecordedHost::default();
    data(&invoke("submit", &p, &mut host));
    let calls = prompt_calls(&host);
    let bytes: Vec<u8> =
        serde_json::from_value(calls[0]["args"]["plan"]["body"]["bytes"].clone()).unwrap();
    let submitted: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        submitted["prompt"]["reference1"]["inputs"]["image"],
        "uploaded/lease-b.png"
    );
    assert_eq!(
        submitted["prompt"]["reference2"]["inputs"]["image"],
        "uploaded/lease-a.png"
    );
}

#[test]
fn invalid_inputs_and_incomplete_leases_have_no_network_effects() {
    for (key, value) in [
        ("width", json!(2049)),
        ("height", json!(4096)),
        ("seed", json!(-1)),
        ("prompt", json!(" ")),
        ("negativePrompt", json!("x")),
        ("referenceImages", json!([])),
        (
            "referenceImages",
            json!([{"mediaId":"a"},{"mediaId":"b"},{"mediaId":"c"}]),
        ),
    ] {
        let mut p = payload(2);
        p["input"][key] = value;
        let mut host = RecordedHost::default();
        assert_eq!(
            invoke("submit", &p, &mut host)["error"]["code"],
            "INVALID_INPUT"
        );
        assert!(host.calls.is_empty());
    }
    for fault in ["missing-lease", "missing-file"] {
        let mut p = payload(2);
        if fault == "missing-lease" {
            p["media"].as_array_mut().unwrap().pop();
        }
        let mut host = RecordedHost {
            fault,
            ..Default::default()
        };
        assert!(invoke("submit", &p, &mut host).get("error").is_some());
        assert!(host
            .calls
            .iter()
            .all(|c| c["method"] != "host-http.execute"));
    }
}

#[test]
fn service_node_and_model_preflight_stop_before_upload() {
    for fault in ["old-service", "missing-node", "missing-model"] {
        let mut host = RecordedHost {
            fault,
            ..Default::default()
        };
        assert_eq!(
            invoke("submit", &payload(2), &mut host)["error"]["code"],
            "RESOURCE_UNAVAILABLE"
        );
        assert!(host
            .calls
            .iter()
            .all(|c| c["args"]["plan"]["method"] != "POST"));
    }
}

#[test]
fn uncertain_acceptance_is_never_retried_and_known_id_is_preserved() {
    for fault in [
        "lost-submit",
        "unreadable-submit",
        "server-error",
        "node-errors",
        "checkpoint-read",
        "checkpoint-write",
    ] {
        let mut host = RecordedHost {
            fault,
            ..Default::default()
        };
        let response = invoke("submit", &payload(0), &mut host);
        assert_eq!(response["error"]["code"], "ACCEPTANCE_UNKNOWN");
        assert_eq!(response["error"]["retryable"], false);
        assert_eq!(prompt_calls(&host).len(), 1);
        if fault.starts_with("checkpoint") || fault == "node-errors" {
            assert_eq!(response["error"]["vendorRequestId"], "remote-17");
        }
    }
}

#[test]
fn polling_and_output_validation_use_the_original_remote_task() {
    let query = json!({"operationId":TEXT_OPERATION,"configuration":{"endpointBaseUrl":"http://fixture.invalid:8188"},"vendorTaskId":"remote-17","resumeToken":{"requestId":"request-1","width":2048,"height":2048}});
    for fault in ["queued", "running", "bad-png", "wrong-size", "wrong-output"] {
        let mut host = RecordedHost {
            fault,
            ..Default::default()
        };
        let response = invoke("query", &query, &mut host);
        if ["queued", "running"].contains(&fault) {
            assert_eq!(data(&response)["status"], fault);
        } else {
            assert_eq!(response["error"]["code"], "UPSTREAM_ERROR");
        }
        assert!(prompt_calls(&host).is_empty());
    }
}
