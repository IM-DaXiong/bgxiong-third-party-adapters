use super::*;

struct Companion {
    body: Option<Value>,
    receipt: Value,
    released: bool,
    scope: Value,
}
impl Companion {
    fn new() -> Self {
        Self {
            body: None,
            receipt: Value::Null,
            released: false,
            scope: json!({"storeId":"store","instanceId":"original-instance"}),
        }
    }
    fn http(value: Value) -> Value {
        json!({"status":200,"headers":[],"payload":{"kind":"buffered","bytes":serde_json::to_vec(&value).unwrap()}})
    }
}
impl Host for Companion {
    fn call(&mut self, call: bgx_adapter_v2_sdk::HostPortCallV2) -> Result<Value, host::HostError> {
        let call = serde_json::to_value(call).unwrap();
        let args = &call["args"];
        match call["method"].as_str().unwrap() {
            "host-checkpoint.read" => Ok(
                json!({"revision":"2","valueJson":json!({"requestId":"original","requestDigest":"a".repeat(64),"state":"acceptance_unknown","submissionScope":self.scope}).to_string()}),
            ),
            "host-media.info" => Ok(
                json!({"kind":"audio","mime":"audio/wav","byteLength":"2048","sha256":"b".repeat(64),"id":"audio","fileName":"speech.wav","width":null,"height":null,"durationMs":40,"timestampsMs":null}),
            ),
            "host-media.read" => Ok(json!({"bytes":[0,1,2],"eof":false})),
            "host-http.execute" => {
                let url = args["plan"]["url"].as_str().unwrap();
                let method = &args["plan"]["method"];
                if url.ends_with("reference-temp") {
                    return Ok(Self::http(
                        json!({"audioPath":"/uploaded/reference.wav","tempId":"temporary"}),
                    ));
                }
                if method == "DELETE" {
                    self.released = true;
                    return Ok(Self::http(json!({})));
                }
                if url.ends_with("store-identity") {
                    return Ok(Self::http(
                        json!({"storeId":"store","instanceId":"new-instance"}),
                    ));
                }
                if url.ends_with("/resolve") {
                    let bytes: Vec<u8> =
                        serde_json::from_value(args["plan"]["body"]["bytes"].clone()).unwrap();
                    let body: Value = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(body["expectedStoreId"], "store");
                    return Ok(Self::http(
                        json!({"outcome":"rejected","requestId":"original","requestDigest":"a".repeat(64),"storeId":"store","instanceId":"new-instance","tombstoneId":"durable-tombstone","fencesLateSubmit":true}),
                    ));
                }
                if url.ends_with("/audio") {
                    return Ok(
                        json!({"status":200,"headers":[],"payload":{"kind":"media","handle":"result"}}),
                    );
                }
                if method == "POST" {
                    let bytes: Vec<u8> =
                        serde_json::from_value(args["plan"]["body"]["bytes"].clone()).unwrap();
                    let body: Value = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(body["expectedStoreId"], "store");
                    assert_eq!(body["requestId"], "original");
                    self.receipt = body["receipt"].clone();
                    self.body = Some(body);
                }
                Ok(Self::http(
                    json!({"jobId":"original","requestDigest":"a".repeat(64),"storeId":"store","status":"succeeded","receipt":self.receipt,"audioSha256":"b".repeat(64),"audioBytes":2048}),
                ))
            }
            _ => Err(host::invalid_wire("Unexpected effect")),
        }
    }
}
fn invocation() -> Invocation {
    serde_json::from_value(json!({"protocolVersion":2,"operation":"submit","profileId":"speech-v1","capabilitySlot":"speech",
        "requestId":"original","budgetMs":60000,"payloadJson":"{}","bindingId":"binding","negotiatedVersion":2,"enabledFeatures":[]})).unwrap()
}
#[test]
fn three_modes_keep_original_parameters_audit_and_return_queryable_audio() {
    for mode in ["plain", "voice_design", "reference_clone"] {
        let mut input = json!({"schemaVersion":1,"text":"Original words","mode":mode,"seed":"7"});
        if mode == "voice_design" {
            input["voiceDesign"] = json!({"controlPrompt":"Calm adult"});
        }
        if mode == "reference_clone" {
            input["reference"] = json!({"audio":{"mediaId":"ref"},"promptText":"Reference words","styleControl":"Calm"});
        }
        let submit:OperationSubmitV2=serde_json::from_value(json!({"schemaVersion":2,"modelId":MODEL_ID,"capabilityKey":"audio.synthesize","operationId":"std/speech.synthesize","operationVersion":1,
            "inputSchemaDigest":"a".repeat(64),"input":input,"media":[{"id":"ref","kind":"audio","handle":"reference","mime":"audio/wav","fileName":"ref.wav","byteLength":"2048"}],
            "configuration":{"origin":"http://localhost"},"uploads":[]})).unwrap();
        let mut host = Companion::new();
        let accepted = synthesize(&mut host, &invocation(), &submit)
            .unwrap_or_else(|e| panic!("{} {}", e.code, e.message));
        assert_eq!(accepted["kind"], "accepted");
        assert_eq!(accepted["vendorTaskId"], "original");
        let sent = host.body.as_ref().unwrap();
        assert_eq!(sent["speech"]["mode"], mode);
        assert_eq!(sent["speech"]["input"], "Original words");
        assert!(!host.receipt["data"].is_null());
        if mode == "reference_clone" {
            assert!(host.released);
            assert_eq!(sent["speech"]["reference"]["promptText"], "Reference words");
        }
        let query=durable::control(&mut host,"query",&json!({"configuration":{"origin":"http://localhost"},"operationId":"std/speech.synthesize","vendorTaskId":"original","resumeToken":accepted["resumeToken"]})).unwrap_or_else(|e|panic!("{} {}",e.code,e.message));
        assert_eq!(query["status"], "succeeded");
        assert_eq!(query["data"], host.receipt["data"]);
        assert_eq!(query["outputs"][0]["role"], "audio");
    }
}
#[test]
fn resolution_echoes_original_scope_and_requires_a_durable_tombstone() {
    let mut host = Companion::new();
    let input = json!({"kind":"submission_resolution","requestId":"original","requestDigest":"a".repeat(64),"expectedRevision":"2",
        "configuration":{"origin":"http://localhost"},"submissionScope":host.scope});
    let result = durable::probe(&mut host, &input).unwrap_or_else(|e| panic!("{}", e.message));
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["submissionScope"], input["submissionScope"]);
    assert_eq!(result["rejection"]["fencesLateSubmit"], true);
    let mut missing = Companion::new();
    missing.scope = Value::Null;
    assert!(durable::submit(
        &mut missing,
        &invocation(),
        "http://localhost",
        json!({}),
        json!({})
    )
    .is_err());
    assert!(
        missing.body.is_none(),
        "missing frozen identity cannot cause POST"
    );
}
