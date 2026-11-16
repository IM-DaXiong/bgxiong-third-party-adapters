#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

mod http;
mod image;
mod video;

use alloc::{format, string::String, vec::Vec};
use bgx_adapter_v2_sdk::{guest, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use http::{download, exchange, media_bytes, media_output, put_media, Failure};

const PLUGIN: &str = "local.bgx.dashscope-media";
const CONFIG: &str = r#"{"type":"object","additionalProperties":false,"required":["endpointBaseUrl","credentialRef"],"properties":{"endpointBaseUrl":{"type":"string","minLength":1},"credentialRef":{"type":"string","minLength":1}}}"#;

fn schema(id: &str, source: &str) -> Value {
    let digest: String = Sha256::digest(source.as_bytes())
        .iter()
        .map(|value| format!("{value:02x}"))
        .collect();
    json!({"id":id,"digest":digest,"schemaJson":source})
}

fn capabilities() -> Value {
    let contracts: Vec<_> = image::capabilities()
        .into_iter()
        .chain(video::capabilities())
        .collect();
    json!({"schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),"capabilityContracts":contracts})
}

fn preflight(payload: &Value) -> Value {
    let selection = &payload["selection"];
    let input = &payload["input"];
    let material = if payload["environment"]["ready"] != true {
        Err(("", "百炼工作空间账号尚未就绪"))
    } else {
        match selection["capabilityKey"].as_str() {
            Some("video.generate") => video::material(selection, input),
            Some("image.generate") => image::material(selection, input),
            _ => Err(("", "百炼操作身份无效")),
        }
    };
    match material {
        Ok(material) => {
            json!({"kind":"operation_input_preflight","status":"ready","requestDigest":payload["requestDigest"],"material":material})
        }
        Err((pointer, message)) => {
            json!({"kind":"operation_input_preflight","status":"rejected","requestDigest":payload["requestDigest"],
            "diagnostics":[{"code":"local.bgx.dashscope-media/invalid_input","fieldPointer":pointer,"message":message}]})
        }
    }
}

fn respond(request: &Invocation, host: &mut dyn Host) -> Result<Value, Failure> {
    let operation = serde_json::to_value(&request.r#operation).expect("public operation");
    let payload: Value = serde_json::from_str(&request.r#payload_json)
        .map_err(|_| Failure::new("INVALID_INPUT", "百炼操作参数无效"))?;
    let capability = payload["capabilityKey"].as_str();
    match (operation.as_str(), capability) {
        (Some("capabilities"), _) => Ok(capabilities()),
        (Some("probe"), _) if payload["kind"] == "operation_input_preflight" => {
            Ok(preflight(&payload))
        }
        (Some("probe"), _) if payload["kind"] == "package_validation" => Ok(json!({
            "packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]
        })),
        (Some("submit"), Some("video.generate")) => {
            video::submit(host, &payload, request.r#budget_ms)
        }
        (Some("query"), Some("video.generate")) => {
            video::query(host, &payload, request.r#budget_ms)
        }
        (Some("submit"), Some("image.generate")) => {
            image::submit(host, &payload, request.r#budget_ms)
        }
        _ => Err(Failure::new("UNSUPPORTED_FEATURE", "此百炼操作不可用")),
    }
}

pub struct DashScope;
impl Adapter for DashScope {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        match respond(&request, host) {
            Ok(value) => guest::completed_response(&request, &value).expect("DashScope result"),
            Err(failure) => {
                serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,
                "error":{"code":failure.code,"message":failure.message,"retryable":false,
                    "vendorCode":failure.vendor,"vendorRequestId":failure.vendor_request}}))
                .expect("DashScope error")
            }
        }
    }
}

bgx_adapter_v2_sdk::export_adapter!(DashScope);
