#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

mod media;
mod parameters;
mod provider;
mod recovery;
mod seededit_image;
#[cfg(test)]
mod tests;

use alloc::{format, string::String};
use bgx_adapter_v2_sdk::{guest, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PLUGIN_ID: &str = "local.bgx.jimeng-visual";

#[derive(Debug)]
struct Failure {
    code: &'static str,
    message: &'static str,
    vendor_code: Option<String>,
    request_id: Option<String>,
}

impl Failure {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            vendor_code: None,
            request_id: None,
        }
    }
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn schema(id: &str, text: &str) -> Value {
    let text = text.replace("\r\n", "\n");
    json!({"id":id,"digest":sha256(text.as_bytes()),"schemaJson":text})
}

fn prepare(payload: &Value) -> Value {
    let result = if payload["environment"]["ready"] != true {
        Err(("", "火山智能视觉账号尚未就绪"))
    } else if provider::endpoint(&payload["configuration"]).is_none() {
        Err(("", "火山智能视觉需要有效的签名 API 地址"))
    } else {
        parameters::operation(&payload["selection"])
            .ok_or(("", "请选择有效的火山智能视觉操作"))
            .and_then(|operation| {
                operation.validate_metadata(&payload["input"], &payload["mediaMetadata"])?;
                operation.material(&payload["input"])
            })
    };
    match result {
        Ok(material) => json!({"kind":"operation_input_preflight","status":"ready",
            "requestDigest":payload["requestDigest"],"material":material}),
        Err((field, message)) => json!({"kind":"operation_input_preflight","status":"rejected",
            "requestDigest":payload["requestDigest"],"diagnostics":[
                {"code":"local.bgx.jimeng-visual/invalid_input","fieldPointer":field,"message":message}]}),
    }
}

fn respond(request: &Invocation, host: &mut dyn Host) -> Result<Value, Failure> {
    let invocation = serde_json::to_value(&request.r#operation).expect("SDK operation");
    let payload: Value = serde_json::from_str(&request.r#payload_json)
        .map_err(|_| Failure::new("INVALID_INPUT", "火山智能视觉请求 JSON 无效"))?;
    match invocation.as_str() {
        Some("capabilities") => Ok(json!({"schemaVersion":2,"pluginId":PLUGIN_ID,
            "pluginVersion":env!("CARGO_PKG_VERSION"),"capabilityContracts":parameters::capabilities()})),
        Some("probe") if payload["kind"] == "operation_input_preflight" => Ok(prepare(&payload)),
        Some("probe") if payload["kind"] == "package_validation" => Ok(json!({
            "packageValid":true,"connection":"not_checked","authentication":"not_checked",
            "resources":"not_checked","issues":[]})),
        Some("submit" | "query") => {
            let operation = parameters::operation(&payload)
                .ok_or_else(|| Failure::new("INVALID_INPUT", "火山智能视觉操作身份无效"))?;
            if invocation == "submit" {
                provider::submit(host, operation, &payload, request.r#budget_ms)
            } else {
                recovery::query(host, operation, &payload, request.r#budget_ms)
            }
        }
        _ => Err(Failure::new(
            "UNSUPPORTED_FEATURE",
            "火山智能视觉操作不支持",
        )),
    }
}

pub struct JimengVisual;

impl Adapter for JimengVisual {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        match respond(&request, host) {
            Ok(value) => guest::completed_response(&request, &value).expect("SDK response"),
            Err(error) => serde_json::from_value(json!({"protocolVersion":2,
                "operation":request.r#operation,"error":{"code":error.code,"message":error.message,
                    "retryable":false,"vendorCode":error.vendor_code,"vendorRequestId":error.request_id}}))
                .expect("SDK error"),
        }
    }
}

bgx_adapter_v2_sdk::export_adapter!(JimengVisual);
