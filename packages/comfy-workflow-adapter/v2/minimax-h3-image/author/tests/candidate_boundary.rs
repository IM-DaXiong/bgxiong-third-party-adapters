use bgx_adapter_v2_sdk::{host::HostError, Adapter, Host, HostPortCallV2, Invocation};
use bgx_minimax_h3_image::H3Image;
use serde_json::{json, Value};

struct NoEffects;
impl Host for NoEffects {
    fn call(&mut self, _: HostPortCallV2) -> Result<Value, HostError> {
        panic!("Candidate without verified workflows must not perform host effects");
    }
}

#[test]
fn distributable_build_refuses_synthetic_workflows() {
    let payload = json!({"operationId":"local.bgx.minimax-h3-image-comfy/text-to-image",
        "modelId":"minimax-h3-image-text-to-image","media":[],
        "configuration":{"endpointBaseUrl":"http://127.0.0.1:8188"},
        "input":{"prompt":"test","width":2048,"height":2048,"seed":0}});
    let request: Invocation = serde_json::from_value(json!({"protocolVersion":2,"operation":"submit",
        "profileId":"dev","capabilitySlot":"image","requestId":"candidate-boundary","budgetMs":1000,
        "payloadJson":payload.to_string(),"bindingId":"candidate","negotiatedVersion":2,"enabledFeatures":[]})).unwrap();
    let response = serde_json::to_value(H3Image::invoke(request, &mut NoEffects)).unwrap();
    assert_eq!(response["error"]["code"], "RESOURCE_UNAVAILABLE");
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("pending resource-host handoff"));
}
