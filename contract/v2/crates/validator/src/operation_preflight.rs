//! Metadata-only preflight. Never constructs a submit context or checkpoint.
use std::collections::BTreeSet;
use serde_json::Value;
use crate::{ContractError, SchemaRegistry};
use crate::capabilities::{Binding, CapabilityRef, CapabilityRegistry};
use crate::package::VerifiedPackage;

pub const FEATURE: &str = "operation-preflight-v1";
fn invalid(path: &str, reason: &str) -> ContractError {
    ContractError::new("INVALID_INPUT", path, reason)
}

pub fn validate_request(
    catalog: &CapabilityRegistry, selected: &CapabilityRef, binding: &Binding,
    package: &VerifiedPackage, request: &Value,
) -> Result<(), ContractError> {
    SchemaRegistry::public()?.validate_at(
        "https://bgxiong.local/vendor-adapter/v2/operation-preflight.schema.json",
        "/$defs/request", request,
    )?;
    if !package.enabled_features().contains(FEATURE)
        || !package.manifest()["requiredFeatures"].as_array().unwrap().iter().any(|f| f == FEATURE) {
        return Err(ContractError::new("UNSUPPORTED_FEATURE", "/kind", "operation preflight was not negotiated"));
    }
    // contract_for binds package version/digest, binding revision, configuration
    // digest and catalog identity before any guest is dispatched.
    let contract = catalog.contract_for(selected, binding)?;
    let environment_ready = catalog.list(None).iter()
        .find(|view| view.reference == *selected).is_some_and(|view| view.environment_ready);
    if request["environmentReady"] != environment_ready {
        return Err(invalid("/environmentReady", "readiness differs from the selected binding connectivity receipt"));
    }
    if request["modelId"] != selected.model_id
        || request["capabilityKey"] != selected.capability_key
        || request["operationId"] != selected.operation_id
        || request["operationVersion"] != selected.operation_version
        || request["inputSchemaDigest"] != selected.input_schema_digest
        || request["configuration"] != *binding.configuration()
    {
        return Err(invalid("/selection", "preflight selection or configuration changed"));
    }
    let c = crate::input::as_value(contract)?;
    let registry = crate::input::registry(&c)?;
    let operation = c["operations"].as_array().unwrap().iter()
        .find(|op| op["id"] == selected.operation_id && op["version"] == selected.operation_version)
        .ok_or_else(|| invalid("/operationId", "selected operation is unavailable"))?;
    registry.validate(operation["inputSchema"]["id"].as_str().unwrap(), &request["input"])?;
    crate::validate_configuration(contract, binding.configuration())?;
    let mut ids = BTreeSet::new();
    for media in request["mediaMetadata"].as_array().unwrap() {
        if !ids.insert(media["mediaId"].as_str().unwrap()) {
            return Err(invalid("/mediaMetadata", "duplicate media identity"));
        }
        if let Some(interval) = media.get("sourceInterval") {
            validate_interval(interval, media["sourceDurationMs"].as_u64()
                .ok_or_else(|| invalid("/sourceDurationMs", "source interval requires measured source duration"))?)?;
        } else if media.get("sourceDurationMs").is_some() {
            return Err(invalid("/sourceInterval", "source duration requires an explicit selected interval"));
        }
    }
    let mut referenced = BTreeSet::new();
    for slot in operation["mediaSlots"].as_array().into_iter().flatten() {
        let pointer = slot["inputPointer"].as_str().unwrap();
        let Some(value) = request["input"].pointer(pointer) else { continue; };
        let refs: Vec<&Value> = if let Some(items) = value.as_array() {
            items.iter().collect()
        } else { vec![value] };
        for reference in refs {
            let id = reference["mediaId"].as_str()
                .ok_or_else(|| invalid(pointer, "media slot requires managed mediaId references"))?;
            let facts = request["mediaMetadata"].as_array().unwrap().iter()
                .find(|m| m["mediaId"] == id && m["kind"] == slot["mediaKind"])
                .ok_or_else(|| invalid(pointer, "media summary is missing or has the wrong kind"))?;
            referenced.insert(facts["mediaId"].as_str().unwrap());
        }
    }
    if referenced != ids {
        return Err(invalid("/mediaMetadata", "preflight metadata must exactly cover declared input media references"));
    }
    if selected.capability_key == "video.speech" {
        if !package.enabled_features().contains(crate::video_speech::FEATURE)
            || !package.manifest()["requiredFeatures"].as_array().unwrap().iter().any(|f| f == crate::video_speech::FEATURE) {
            return Err(ContractError::new("UNSUPPORTED_FEATURE", "/capabilityKey", "video speech was not negotiated"));
        }
        crate::video_speech::validate_preflight(request)?;
    }
    Ok(())
}

pub(crate) fn validate_interval(interval: &Value, source_duration: u64) -> Result<(), ContractError> {
    let start = interval["startMs"].as_u64().ok_or_else(|| invalid("/startMs", "invalid interval start"))?;
    let end = interval["endMs"].as_u64().ok_or_else(|| invalid("/endMs", "invalid interval end"))?;
    if start >= end || end > source_duration {
        return Err(invalid("/interval", "interval must be nonempty and within measured source duration"));
    }
    Ok(())
}

pub fn validate_result(request: &Value, result: &Value) -> Result<(), ContractError> {
    let registry = SchemaRegistry::public()?;
    let id = "https://bgxiong.local/vendor-adapter/v2/operation-preflight.schema.json";
    registry.validate_at(id, "/$defs/request", request)?;
    registry.validate_at(id, "/$defs/result", result)?;
    if request["requestDigest"] != result["requestDigest"] {
        return Err(invalid("/requestDigest", "preflight response belongs to a different input"));
    }
    if result["status"] == "ready" && request["environmentReady"] != true {
        return Err(invalid("/status", "unknown or unavailable dependencies cannot produce ready preflight"));
    }
    if request["capabilityKey"] == "video.speech" {
        crate::video_speech::validate_preflight(request)?;
        let source = request["mediaMetadata"].as_array().unwrap().iter()
            .find(|m| m["mediaId"] == request["input"]["drivingAudio"][0]["mediaId"])
            .ok_or_else(|| invalid("/mediaMetadata", "driving audio metadata is missing"))?;
        if result["status"] == "ready" {
            let expected = request["input"]["audio"]["durationMs"].as_u64().unwrap();
            let spec = &result["specification"];
            let audio = spec["audioDurationMs"].as_u64().unwrap();
            let video = spec["videoDurationMs"].as_u64().unwrap();
            if audio != expected || audio.abs_diff(video) > spec["maxTailDifferenceMs"].as_u64().unwrap() {
                return Err(invalid("/specification", "preflight must preserve the selected audio and declare the video tail difference"));
            }
        }
        if let Some(candidates) = result["candidateIntervals"].as_array() {
            let duration = source["sourceDurationMs"].as_u64()
                .ok_or_else(|| invalid("/sourceDurationMs", "adjustments require original source bounds"))?;
            for candidate in candidates { validate_interval(candidate, duration)?; }
        }
    }
    Ok(())
}
