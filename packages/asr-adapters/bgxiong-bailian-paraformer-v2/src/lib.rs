#![cfg_attr(target_arch = "wasm32", no_std)]
use bailian_asr_client::{sdk, Model};
use sdk::{Adapter, Host, Invocation, Response};

struct Bailian;

impl Adapter for Bailian {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        bailian_asr_client::invoke(Model::Paraformer, include_str!("capabilities.json"), request, host)
    }
}

sdk::export_adapter!(Bailian);

#[cfg(test)]
mod tests;
