#![cfg_attr(target_arch = "wasm32", no_std)]
extern crate alloc;
use bgx_adapter_v2_sdk::{guest, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
pub const PLUGIN: &str = "local.thirdparty.runninghub-h3-all";
const OPERATION_VERSION: u64 = 2;
mod classic;
mod dynamic;
mod media_input;
pub fn capabilities() -> Value {
    let mut contracts = alloc::vec::Vec::new();
    for route in &dynamic::ROUTES {
        contracts.extend(
            dynamic::capabilities(route)["capabilityContracts"]
                .as_array()
                .expect("route contracts")
                .iter()
                .cloned(),
        );
    }
    contracts.extend(
        classic::capabilities()["capabilityContracts"]
            .as_array()
            .expect("app contracts")
            .iter()
            .cloned(),
    );
    json!({"schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),"capabilityContracts":contracts})
}
pub struct RunningHubH3;
impl Adapter for RunningHubH3 {
    fn invoke(r: Invocation, h: &mut dyn Host) -> Response {
        if serde_json::to_value(&r.r#operation).expect("operation") == "capabilities" {
            return guest::completed_response(&r, &capabilities()).expect("capabilities response");
        }
        if let Ok(p) = serde_json::from_str::<Value>(&r.r#payload_json) {
            let model = if p["kind"] == "operation_input_preflight" {
                &p["selection"]["modelId"]
            } else {
                &p["modelId"]
            };
            if let Some(route) = dynamic::ROUTES
                .iter()
                .find(|route| *model == route.model)
            {
                return dynamic::invoke(route, r, h);
            }
        }
        classic::RunningHubH3::invoke(r, h)
    }
}
bgx_adapter_v2_sdk::export_adapter!(RunningHubH3);

fn prepare_operation(request: &Value, validate: impl FnOnce(&Value) -> Result<(), alloc::string::String>) -> Value {
    let reject = |code: &str, message: &str| json!({
        "kind":"operation_input_preflight", "status":"rejected", "requestDigest":request["requestDigest"],
        "diagnostics":[{"code":alloc::format!("{PLUGIN}/{code}"),"message":message,"fieldPointer":""}]
    });
    if request["environment"]["ready"] != true {
        return reject("environment_unavailable", "当前运行配置未就绪");
    }
    let mut selection = request["selection"].clone();
    if !selection.is_object() {
        return reject("selection_changed", "操作选择无效");
    }
    selection["schemaVersion"] = json!(2);
    if let Err(message) = validate(&selection) {
        return reject("invalid_parameter", &message);
    }
    json!({"kind":"operation_input_preflight","status":"ready","requestDigest":request["requestDigest"],
        "material":{"instanceType":request["configuration"]["instanceType"]}})
}

fn prepared_instance(request: &Value) -> Option<&str> {
    let configured = request["configuration"]["instanceType"].as_str()?;
    (request["preparation"]["material"]["instanceType"] == configured).then_some(configured)
}
#[cfg(test)]
mod tests;

fn video_failure_message(reason: &str) -> &str {
    if reason.trim_start().starts_with("付费积分余额不足") {
        "云端付费积分不足，请检查 RunningHub 账号的付费积分余额。"
    } else {
        reason
    }
}
