#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;
mod parameters;
mod provider;
mod recovery;

use alloc::{format, string::String};
use bgx_adapter_v2_sdk::{guest, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use provider::Error;

const PLUGIN_ID: &str = "local.bgx.generic-video-api";
const MODEL_ID: &str = "generic-video-api";

fn schema(id: &str, source: &str) -> Value {
    let source = source.replace("\r\n", "\n");
    let digest: String = Sha256::digest(source.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    json!({"id":id,"digest":digest,"schemaJson":source})
}

fn capabilities() -> Value {
    json!({"schemaVersion":2,"pluginId":PLUGIN_ID,"pluginVersion":env!("CARGO_PKG_VERSION"),
        "capabilityContracts":parameters::capabilities()})
}

fn prepare(payload: &Value) -> Value {
    let config = &payload["configuration"];
    let result = if payload["environment"]["ready"] != true || provider::base_url(config).is_none()
    {
        Err(("", "通用视频账号尚未就绪"))
    } else {
        parameters::operation(&payload["selection"])
            .ok_or(("/inputMode", "请明确选择通用视频适配操作"))
            .and_then(|operation| parameters::material(operation, &payload["input"], config))
    };
    match result {
        Ok(material) => {
            json!({"kind":"operation_input_preflight","status":"ready","requestDigest":payload["requestDigest"],"material":material})
        }
        Err((pointer, message)) => {
            json!({"kind":"operation_input_preflight","status":"rejected","requestDigest":payload["requestDigest"],
            "diagnostics":[{"code":"local.bgx.generic-video-api/invalid_input","fieldPointer":pointer,"message":message}]})
        }
    }
}

fn respond(request: &Invocation, host: &mut dyn Host) -> Result<Value, Error> {
    let invocation = serde_json::to_value(&request.r#operation).expect("SDK operation");
    let payload = serde_json::from_str::<Value>(&request.r#payload_json)
        .map_err(|_| Error::new("INVALID_INPUT", "通用视频调用参数无效"))?;
    match invocation.as_str() {
        Some("capabilities") => Ok(capabilities()),
        Some("probe") if payload["kind"] == "operation_input_preflight" => Ok(prepare(&payload)),
        Some("probe") if payload["kind"] == "package_validation" => Ok(json!({
            "packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]
        })),
        Some("submit") => {
            let operation = parameters::operation(&payload)
                .ok_or_else(|| Error::new("INVALID_INPUT", "通用视频操作身份无效"))?;
            provider::submit(host, operation, &payload, request.r#budget_ms)
        }
        Some("query") if parameters::operation(&payload).is_some() => {
            recovery::query(host, &payload, request.r#budget_ms)
        }
        Some("cancel") => Err(Error::new(
            "CANCEL_UNSUPPORTED",
            "通用视频接口未声明取消操作",
        )),
        _ => Err(Error::new("UNSUPPORTED_FEATURE", "通用视频操作不可用")),
    }
}

pub struct GenericVideoApi;

impl Adapter for GenericVideoApi {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        match respond(&request, host) {
            Ok(value) => guest::completed_response(&request, &value).expect("SDK response"),
            Err(error) => {
                serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,
                "error":{"code":error.code,"message":error.message,"retryable":false,
                    "vendorCode":error.vendor_code,"vendorRequestId":error.vendor_request_id}}))
                .expect("SDK error")
            }
        }
    }
}

bgx_adapter_v2_sdk::export_adapter!(GenericVideoApi);
