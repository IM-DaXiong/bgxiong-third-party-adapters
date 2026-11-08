//! Platform-neutral speech semantics; frame grids and model limits belong to adapters.
use serde_json::Value;
use crate::{ContractError, SchemaRegistry};
pub const FEATURE: &str = "video-speech-v1";

pub fn enable(mut context: crate::ValidatedOperationInput, features: &std::collections::BTreeSet<String>) -> Result<crate::ValidatedOperationInput, ContractError> {
    if context.contract()["capabilityKey"] != "video.speech" { return Ok(context); }
    if !features.contains(FEATURE) || !features.contains(crate::operation_preflight::FEATURE) {
        return Err(ContractError::new("UNSUPPORTED_FEATURE", "/capabilityKey", "video speech requires the speech and preflight profiles"));
    }
    if context.contract()["transportSlot"] != "video" { return Err(invalid("video speech requires the video transport")); }
    let slots = context.operation()["mediaSlots"].as_array().unwrap();
    for (role, pointer, kind) in [("portrait", "/portrait", "image"), ("driving_audio", "/drivingAudio", "audio")] {
        if !slots.iter().any(|s| s["role"] == role && s["inputPointer"] == pointer && s["mediaKind"] == kind
            && s["minCount"] == 1 && s["maxCount"] == 1) { return Err(invalid("speech baseline media slots must be explicit")); }
    }
    validate_input_shape(&context.request()["input"])?;
    context.video_speech = true;
    Ok(context)
}

fn validate_input_shape(input: &Value) -> Result<(), ContractError> {
    let mut baseline = input.clone();
    if !input["parameters"].is_object() { return Err(invalid("parameters must be a declared object")); }
    baseline["parameters"] = serde_json::json!({});
    SchemaRegistry::validate_public("profiles/video-speech-v1/input", &baseline)
}

pub fn validate_result(context: &crate::ValidatedOperationInput, result: &Value) -> Result<(), ContractError> {
    if !context.video_speech || !(result["kind"] == "completed" || result["status"] == "succeeded") { return Ok(()); }
    let data = &result["data"];
    SchemaRegistry::validate_public("profiles/video-speech-v1/output", data)?;
    let input = &context.request()["input"];
    let receipt = &data["receipt"];
    if receipt["portraitMediaId"] != input["portrait"][0]["mediaId"]
        || receipt["drivingAudioMediaId"] != input["drivingAudio"][0]["mediaId"]
        || receipt["drivingAudioSha256"] != input["audio"]["sha256"]
        || receipt["drivingAudioDurationMs"] != input["audio"]["durationMs"]
        || data["video"]["audioDurationMs"] != input["audio"]["durationMs"] {
        return Err(invalid("completed video receipt differs from the frozen driving input"));
    }
    let outputs = result["outputs"].as_array().ok_or_else(|| invalid("completed video output is missing"))?;
    if outputs.len() != 1 || outputs[0]["kind"] != "media" || outputs[0]["media"]["kind"] != "video"
        || outputs[0]["media"]["id"] != data["video"]["mediaId"] {
        return Err(invalid("speech completion requires exactly one matching managed video"));
    }
    Ok(())
}

pub fn validate_preflight(request: &Value) -> Result<(), ContractError> {
    let input = &request["input"];
    // Package parameters are already validated against the selected operation.
    // Apply the baseline shape with parameters empty, so declared package fields
    // stay legal while all baseline media and measured-audio fields remain strict.
    validate_input_shape(input)?;
    let metadata = request["mediaMetadata"].as_array()
        .ok_or_else(|| invalid("metadata is missing"))?;
    let portrait_id = input["portrait"][0]["mediaId"].as_str().unwrap();
    let audio_id = input["drivingAudio"][0]["mediaId"].as_str().unwrap();
    if portrait_id == audio_id { return Err(invalid("portrait and driving audio must have distinct identities")); }
    let portrait = metadata.iter().find(|m| m["mediaId"] == portrait_id && m["kind"] == "image")
        .ok_or_else(|| invalid("portrait metadata is missing"))?;
    let audio = metadata.iter().find(|m| m["mediaId"] == audio_id && m["kind"] == "audio")
        .ok_or_else(|| invalid("driving audio metadata is missing"))?;
    if portrait["width"].as_u64().is_none() || portrait["height"].as_u64().is_none() {
        return Err(invalid("portrait requires measured dimensions"));
    }
    for field in ["durationMs", "sampleRateHz", "channels", "sha256"] {
        if input["audio"][field] != audio[field] { return Err(invalid("driving audio facts differ from input")); }
    }
    if let Some(interval) = audio.get("sourceInterval") {
        let duration = audio["sourceDurationMs"].as_u64().ok_or_else(|| invalid("source duration is missing"))?;
        crate::operation_preflight::validate_interval(interval, duration)?;
        if interval["endMs"].as_u64().unwrap() - interval["startMs"].as_u64().unwrap()
            != input["audio"]["durationMs"].as_u64().unwrap() {
            return Err(invalid("prepared audio duration differs from selected interval"));
        }
    }
    Ok(())
}
fn invalid(reason: &str) -> ContractError { ContractError::new("INVALID_INPUT", "/videoSpeech", reason) }
