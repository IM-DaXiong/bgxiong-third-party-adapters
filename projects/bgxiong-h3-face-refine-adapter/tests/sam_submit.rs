use bgx_adapter_v2_sdk::{host, Adapter, Host, HostPortCallV2};
use h3_face_refine_adapter::{capabilities, H3FaceRefine, OP_REFINE, PLUGIN};
use serde_json::{json, Value};

struct Service {
    gets: Vec<String>,
    checkpoint: Value,
}

impl Host for Service {
    fn call(&mut self, call: HostPortCallV2) -> Result<Value, host::HostError> {
        let call = serde_json::to_value(call).unwrap();
        let args = &call["args"];
        if call["method"] == "host-checkpoint.read" {
            return Ok(json!({"revision":"2","valueJson":json!({
                "revision":"2","state":"acceptance_unknown","requestId":"test",
                "requestDigest":"frozen-input","vendorTaskId":null
            }).to_string()}));
        }
        if call["method"] == "host-checkpoint.write" {
            self.checkpoint = serde_json::from_str(args["valueJson"].as_str().unwrap()).unwrap();
            return Ok(json!({"revision":"3"}));
        }
        let plan = &args["plan"];
        assert_eq!(plan["method"], "GET", "missing SAM must never upload or submit");
        let url = plan["url"].as_str().unwrap();
        assert_eq!(url, "http://localhost/object_info/SAMLoader");
        self.gets.push(url.to_owned());
        Ok(json!({"status":200,"payload":{"bytes":b"{}".to_vec()}}))
    }
}

#[test]
fn missing_loader_rejects_both_submission_modes_before_upload_or_prompt() {
    let caps = capabilities();
    let operation = caps["capabilityContracts"][0]["operations"].as_array().unwrap()
        .iter().find(|op| op["id"] == OP_REFINE).unwrap();
    let digest = &operation["inputSchema"]["digest"];
    let freeze = json!({
        "source":{"packageId":PLUGIN,"operationId":OP_REFINE,"schemaDigest":digest},
        "packageVersion":env!("CARGO_PKG_VERSION"),"operationId":OP_REFINE,"schemaDigest":digest,
        "analysis":{"analysisId":"a","revision":"1","digest":"analysis-digest"},
        "selectionDigest":"confirmed","userParams":{"samEnabled":true}
    });
    for mut input in [
        json!({"selection":{"analysisId":"a","analysisRevision":"1","analysisDigest":"analysis-digest","selectionDigest":"confirmed"}}),
        json!({"trajectory":{"mediaId":"track","trajectoryDigest":"confirmed"}}),
    ] {
        input["freeze"] = freeze.clone();
        let payload = json!({"operationId":OP_REFINE,"input":input,"configuration":{"endpointBaseUrl":"http://localhost"}});
        let invocation = serde_json::from_value(json!({
            "protocolVersion":2,"operation":"submit","profileId":"test-binding","bindingId":"test-binding",
            "capabilitySlot":"videoFaceRefine","requestId":"test","budgetMs":60000,
            "negotiatedVersion":2,"enabledFeatures":[],"payloadJson":payload.to_string()
        })).unwrap();
        let mut service = Service { gets: vec![], checkpoint: Value::Null };
        let response = serde_json::to_value(H3FaceRefine::invoke(invocation, &mut service)).unwrap();
        assert_eq!(response["error"]["code"], "RESOURCE_UNAVAILABLE");
        assert!(response["error"]["message"].as_str().unwrap().contains("未注册 SAMLoader"));
        assert_eq!(service.gets.len(), 1);
        assert_eq!(service.checkpoint["state"], "rejected");
        assert_eq!(service.checkpoint["submissionRejection"]["basis"], "before_submission");
    }
}
