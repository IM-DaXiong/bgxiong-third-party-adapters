#![cfg_attr(target_arch = "wasm32", no_std)]
extern crate alloc;
use bgx_adapter_v2_sdk::{guest, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
pub const PLUGIN: &str = "local.thirdparty.runninghub-h3-all";
mod classic;
mod dynamic;
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
            if let Some(route) = dynamic::ROUTES
                .iter()
                .find(|route| p["modelId"] == route.model)
            {
                return dynamic::invoke(route, r, h);
            }
        }
        classic::RunningHubH3::invoke(r, h)
    }
}
bgx_adapter_v2_sdk::export_adapter!(RunningHubH3);
#[cfg(test)]
mod tests;
