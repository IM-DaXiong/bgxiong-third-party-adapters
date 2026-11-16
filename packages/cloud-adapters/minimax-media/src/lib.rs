#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

mod h3;
mod parameters;
mod provider;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use bgx_adapter_v2_sdk::{guest, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use parameters::{ImageOperation, MODELS, PLUGIN};
use provider::Failure;

const CONFIG: &str = r#"{"type":"object","additionalProperties":false,"required":["endpointBaseUrl","credentialRef"],"properties":{"endpointBaseUrl":{"type":"string","minLength":1},"credentialRef":{"type":"string","minLength":1}}}"#;
const OUTPUT: &str = r#"{"type":"object","additionalProperties":false,"required":["providerRequestId","imageCount"],"properties":{"providerRequestId":{"type":["string","null"]},"imageCount":{"type":"integer","const":1}}}"#;

fn schema(id: &str, source: &str) -> Value {
    let digest: String = Sha256::digest(source.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    json!({"id":id,"digest":digest,"schemaJson":source})
}

fn capability(operation: ImageOperation) -> Value {
    let configuration = schema("configuration", CONFIG);
    let input = schema("input", &operation.input_schema().to_string());
    let output = schema("output", OUTPUT);
    let count = operation.reference_count();
    json!({"schemaVersion":2,"localId":operation.key(),"capabilityKey":"image.generate","transportSlot":"image","modelIds":MODELS,
        "configurationSchema":{"id":"configuration","digest":configuration["digest"]},"schemas":[configuration,input,output],
        "operations":[{"id":operation.id(),"version":1,"inputProfile":"operation-input-v1","label":operation.label(),
            "inputSchema":{"id":"input","digest":input["digest"]},"outputSchema":{"id":"output","digest":output["digest"]},
            "resumeSchema":null,"uiSchema":{"controls":operation.controls()},
            "mediaSlots":[{"inputPointer":"/subjectImages","role":"character","mediaKind":"image","minCount":count,"maxCount":count}],
            "outputSlots":[{"role":"images","kind":"image","minCount":1,"maxCount":1},
                {"role":"provider-response","kind":"file","minCount":1,"maxCount":1}],
            "execution":{"mode":"synchronous","streaming":false,"cancellation":"unsupported","resumable":false,"remoteIdempotency":false}}]
    })
}

fn capabilities() -> Value {
    let contracts: Vec<_> = [ImageOperation::Text, ImageOperation::Character]
        .into_iter()
        .map(capability)
        .chain(core::iter::once(h3::capability()))
        .collect();
    json!({"schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),"capabilityContracts":contracts})
}

fn preflight(payload: &Value) -> Value {
    let selection = &payload["selection"];
    let input = &payload["input"];
    let material = if payload["environment"]["ready"] != true {
        Err(("", "MiniMax 账号尚未就绪"))
    } else if selection["capabilityKey"] == "video.generate" {
        h3::material(selection, input)
    } else {
        parameters::material(selection, input)
    };
    match material {
        Ok(material) => {
            json!({"kind":"operation_input_preflight","status":"ready","requestDigest":payload["requestDigest"],"material":material})
        }
        Err((pointer, message)) => {
            json!({"kind":"operation_input_preflight","status":"rejected","requestDigest":payload["requestDigest"],
            "diagnostics":[{"code":"local.bgx.minimax-media/invalid_input","fieldPointer":pointer,"message":message}]})
        }
    }
}

fn respond(request: &Invocation, host: &mut dyn Host) -> Result<Value, Failure> {
    let operation = serde_json::to_value(&request.r#operation).expect("public operation");
    let payload: Value = serde_json::from_str(&request.r#payload_json)
        .map_err(|_| Failure::new("INVALID_INPUT", "MiniMax 调用参数无效"))?;
    match operation.as_str() {
        Some("capabilities") => Ok(capabilities()),
        Some("probe") if payload["kind"] == "operation_input_preflight" => Ok(preflight(&payload)),
        Some("probe") if payload["kind"] == "package_validation" => {
            Ok(json!({"packageValid":true,"connection":"not_checked",
            "authentication":"not_checked","resources":"not_checked","issues":[]}))
        }
        Some("submit") if payload["capabilityKey"] == "video.generate" => {
            h3::submit(host, &payload, request.r#budget_ms)
        }
        Some("submit") => provider::submit(host, &payload, request.r#budget_ms),
        Some("query") => h3::query(host, &payload, request.r#budget_ms),
        _ => Err(Failure::new("UNSUPPORTED_FEATURE", "此 MiniMax 操作不可用")),
    }
}

pub struct MiniMax;
impl Adapter for MiniMax {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        match respond(&request, host) {
            Ok(value) => guest::completed_response(&request, &value).expect("MiniMax result"),
            Err(failure) => {
                serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,
                "error":{"code":failure.code,"message":failure.message,"retryable":false,
                    "vendorCode":failure.vendor,"vendorRequestId":failure.vendor_request}}))
                .expect("MiniMax error")
            }
        }
    }
}

bgx_adapter_v2_sdk::export_adapter!(MiniMax);
