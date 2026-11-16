#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

mod media;
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

use parameters::{Operation, MODEL, OPERATIONS, PLUGIN};
use provider::{Failure, Result};

const CONFIGURATION: &str = r#"{"type":"object","properties":{"endpointBaseUrl":{"type":"string","minLength":1},"credentialRef":{"type":"string","minLength":1}},"required":["endpointBaseUrl","credentialRef"],"additionalProperties":false}"#;

fn schema(id: &str, text: &str) -> Value {
    let value = text.replace("\r\n", "\n");
    let digest: String = Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    json!({"id":id,"digest":digest,"schemaJson":value})
}

fn capability(operation: &Operation) -> Value {
    let configuration = schema("config", CONFIGURATION);
    let input = schema("input", &operation.input_schema().to_string());
    let output = schema("output", include_str!("../schemas/output.json"));
    let resume = schema("resume", include_str!("../schemas/resume.json"));
    let slots: Vec<_> = operation
        .slots
        .iter()
        .map(|slot| {
            json!({"inputPointer":format!("/{}",slot.field),"role":slot.role,"mediaKind":slot.kind,
            "minCount":slot.minimum,"maxCount":slot.maximum})
        })
        .collect();
    json!({
        "schemaVersion":2,"localId":operation.id,"capabilityKey":"video.generate","transportSlot":"video","modelIds":[MODEL],
        "configurationSchema":{"id":"config","digest":configuration["digest"]},"schemas":[configuration,input,output,resume],
        "operations":[{"id":operation.id(),"version":operation.version(),"label":operation.label,"inputProfile":"operation-input-v1",
            "inputSchema":{"id":"input","digest":input["digest"]},"outputSchema":{"id":"output","digest":output["digest"]},
            "resumeSchema":{"id":"resume","digest":resume["digest"]},"uiSchema":{"controls":operation.controls()},"mediaSlots":slots,
            "outputSlots":[{"role":"video","kind":"video","minCount":1,"maxCount":1},{"role":"last-frame","kind":"image","minCount":0,"maxCount":1}],
            "execution":{"mode":"asynchronous","streaming":false,"cancellation":"best_effort","resumable":true,"remoteIdempotency":false}}]
    })
}

pub fn capabilities() -> Value {
    let contracts: Vec<_> = OPERATIONS.iter().map(capability).collect();
    json!({"schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),"capabilityContracts":contracts})
}

fn preflight(payload: &Value) -> Value {
    let preparation = parameters::operation(&payload["selection"])
        .ok_or(("/inputMode", "Seedance 操作身份无效"))
        .and_then(|operation| {
            if payload["environment"]["ready"] != true {
                return Err(("", "方舟账号或连接尚未就绪"));
            }
            parameters::material(operation, &payload["input"])
        });
    match preparation {
        Ok(material) => {
            json!({"kind":"operation_input_preflight","status":"ready","requestDigest":payload["requestDigest"],"material":material})
        }
        Err((pointer, message)) => {
            json!({"kind":"operation_input_preflight","status":"rejected","requestDigest":payload["requestDigest"],
            "diagnostics":[{"code":"local.bgx.seedance-video/invalid_input","fieldPointer":pointer,"message":message}]})
        }
    }
}

fn respond(request: &Invocation, invocation: &Value, host: &mut dyn Host) -> Result<Value> {
    let payload: Value = serde_json::from_str(&request.r#payload_json)
        .map_err(|_| Failure::invalid("调用参数无效"))?;
    match invocation["operation"].as_str() {
        Some("capabilities") => Ok(capabilities()),
        Some("probe") if payload["kind"] == "package_validation" => Ok(
            json!({"packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]}),
        ),
        Some("probe") if payload["kind"] == "operation_input_preflight" => Ok(preflight(&payload)),
        Some("probe") if payload["kind"] == "connectivity" => {
            provider::probe(host, request.r#budget_ms, &payload)
        }
        Some("submit") => {
            let operation =
                parameters::operation(&payload).ok_or(Failure::invalid("Seedance 操作身份无效"))?;
            provider::submit(host, request.r#budget_ms, operation, &payload)
        }
        Some("query") if parameters::operation(&payload).is_some() => {
            provider::query(host, request.r#budget_ms, &payload)
        }
        Some("cancel") if parameters::operation(&payload).is_some() => {
            provider::cancel(host, request.r#budget_ms, &payload)
        }
        _ => Err(Failure::new(
            "UNSUPPORTED_FEATURE",
            "此 Seedance 操作不可用",
        )),
    }
}

pub struct Seedance;

impl Adapter for Seedance {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        let invocation = serde_json::to_value(&request).expect("typed invocation");
        match respond(&request,&invocation,host) {
            Ok(result) => guest::completed_response(&request,&result).expect("Seedance result response"),
            Err(e) => serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,
                "error":{"code":e.code,"message":e.message,"retryable":false,"vendorCode":e.vendor_code,
                    "vendorRequestId":e.vendor_task_id}})).expect("Seedance error response"),
        }
    }
}

bgx_adapter_v2_sdk::export_adapter!(Seedance);
