#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

mod media;
mod parameters;
mod provider;

use alloc::{format, string::String};
use bgx_adapter_v2_sdk::{guest, host, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PLUGIN: &str = "local.bgx.jimeng-openai-image";

#[derive(Debug)]
struct Failure {
    code: &'static str,
    message: &'static str,
    vendor: Option<String>,
    request: Option<String>,
}

impl Failure {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            vendor: None,
            request: None,
        }
    }

    fn invalid(message: &'static str) -> Self {
        Self::new("INVALID_INPUT", message)
    }

    fn upstream(message: &'static str) -> Self {
        Self::new("UPSTREAM_ERROR", message)
    }
}

fn port(host: &mut dyn Host, method: &str, args: Value) -> Result<Value, Failure> {
    host::call(host, method, &args).map_err(|error| {
        let detail = serde_json::to_value(error).unwrap_or_default();
        let code = match detail["code"].as_str() {
            Some("PERMISSION_DENIED") => "PERMISSION_DENIED",
            Some("INVALID_INPUT") => "INVALID_INPUT",
            Some("TIMEOUT") => "TIMEOUT",
            Some("BUDGET_EXCEEDED" | "RESOURCE_UNAVAILABLE") => "RESOURCE_UNAVAILABLE",
            _ => "UPSTREAM_ERROR",
        };
        Failure::new(code, "图像网关受管网络或媒体操作失败")
    })
}

fn schema(id: &str, source: &str) -> Value {
    let source = source.replace("\r\n", "\n");
    let digest: String = Sha256::digest(source.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    json!({"id":id,"digest":digest,"schemaJson":source})
}

fn preflight(payload: &Value) -> Value {
    let prepared = if payload["environment"]["ready"] == true {
        parameters::material(
            &payload["selection"],
            &payload["input"],
            &payload["configuration"],
        )
    } else {
        Err(("", "图像网关账号尚未就绪"))
    };
    match prepared {
        Ok(material) => {
            json!({"kind":"operation_input_preflight","status":"ready","requestDigest":payload["requestDigest"],"material":material})
        }
        Err((field, message)) => json!({
            "kind":"operation_input_preflight","status":"rejected","requestDigest":payload["requestDigest"],
            "diagnostics":[{"code":format!("{PLUGIN}/invalid_input"),"fieldPointer":field,"message":message}]
        }),
    }
}

fn respond(request: &Invocation, host: &mut dyn Host) -> Result<Value, Failure> {
    let invocation = serde_json::to_value(&request.r#operation).expect("public operation");
    let payload: Value = serde_json::from_str(&request.r#payload_json)
        .map_err(|_| Failure::invalid("图像网关操作参数无效"))?;
    match invocation.as_str() {
        Some("capabilities") => Ok(
            json!({"schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),
            "capabilityContracts":parameters::capabilities()}),
        ),
        Some("probe") if payload["kind"] == "package_validation" => {
            Ok(json!({"packageValid":true,"connection":"not_checked",
            "authentication":"not_checked","resources":"not_checked","issues":[]}))
        }
        Some("probe") if payload["kind"] == "operation_input_preflight" => Ok(preflight(&payload)),
        Some("submit") => provider::submit(host, request, &payload),
        _ => Err(Failure::new(
            "UNSUPPORTED_FEATURE",
            "所选图像网关操作不可用",
        )),
    }
}

struct Gateway;

impl Adapter for Gateway {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        match respond(&request, host) {
            Ok(result) => {
                guest::completed_response(&request, &result).expect("valid gateway response")
            }
            Err(failure) => {
                serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,
                "error":{"code":failure.code,"message":failure.message,"retryable":false,
                    "vendorCode":failure.vendor,"vendorRequestId":failure.request}}))
                .expect("valid gateway failure")
            }
        }
    }
}

bgx_adapter_v2_sdk::export_adapter!(Gateway);
