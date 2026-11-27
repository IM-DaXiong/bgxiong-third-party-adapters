#![cfg_attr(target_arch = "wasm32", no_std)]
extern crate alloc;

mod http;
mod policy;
mod qwen;
mod task;
#[cfg(test)]
mod tests;
mod upload;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
pub use bgx_adapter_v2_sdk as sdk;
use http::Failure;
use sdk::{guest, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const INPUT: &str = include_str!(
    "../../../../../bgxiong-adapter-sdk/profiles/speech-analysis-v1/transcribe.input.schema.json"
);
const OUTPUT: &str = include_str!(
    "../../../../../bgxiong-adapter-sdk/profiles/speech-analysis-v1/transcribe.output.schema.json"
);
const RESUME: &str = include_str!(
    "../../../../../bgxiong-adapter-sdk/profiles/speech-analysis-v1/transcribe.resume.schema.json"
);
const CONFIG: &str = r#"{"type":"object","additionalProperties":false,"required":["endpointBaseUrl","credentialRef"],"properties":{"endpointBaseUrl":{"type":"string","minLength":1},"credentialRef":{"type":"string","minLength":1}}}"#;

#[derive(Copy, Clone, PartialEq)]
pub enum Model {
    Paraformer,
    Fun,
    Qwen,
}

impl Model {
    pub fn id(self) -> &'static str {
        match self {
            Self::Paraformer => "paraformer-v2",
            Self::Fun => "fun-asr",
            Self::Qwen => "qwen3-asr-flash",
        }
    }
}

fn schema(source: &str) -> Value {
    let normalized = source.replace("\r\n", "\n");
    let parsed: Value = serde_json::from_str(&normalized).expect("ASR schema");
    json!({"id":parsed["$id"],"digest":format!("{:x}",Sha256::digest(normalized.as_bytes())),"schemaJson":normalized})
}

fn reference(schema: &Value) -> Value {
    json!({"id":schema["id"],"digest":schema["digest"]})
}

fn capabilities(model: Model, definition: &str) -> Value {
    let mut root: Value = serde_json::from_str(definition).expect("ASR capabilities");
    let contract = &mut root["capabilityContracts"][0];
    let input = schema(INPUT);
    let output = schema(OUTPUT);
    let resume = schema(RESUME);
    let config = json!({"id":"bailian-asr-config","digest":format!("{:x}",Sha256::digest(CONFIG.as_bytes())),"schemaJson":CONFIG});
    contract["configurationSchema"] = reference(&config);
    contract["operations"][0]["inputSchema"] = reference(&input);
    contract["operations"][0]["outputSchema"] = reference(&output);
    // The standard operation identity includes its resume schema even for a synchronous package.
    contract["operations"][0]["resumeSchema"] = reference(&resume);
    contract["schemas"] = json!([input, output, config, resume]);
    policy::attach_contract(&mut root, model);
    root
}

fn received_wav(host: &mut dyn Host, payload: &Value) -> Result<(Vec<u8>, u64), Failure> {
    let id = payload["input"]["audio"]["mediaId"].as_str();
    let lease = payload["media"]
        .as_array()
        .and_then(|media| {
            let [item] = media.as_slice() else {
                return None;
            };
            (id.is_some() && item["id"].as_str() == id && item["kind"] == "audio")
                .then(|| item["handle"].as_str())
                .flatten()
        })
        .ok_or_else(|| Failure::new("INVALID_INPUT", "缺少授权的音频文件"))?;
    let wav = http::media_bytes(host, lease, 10_000_000)?;
    let duration = wav_duration(&wav)?;
    Ok((wav, duration))
}

fn wav_duration(bytes: &[u8]) -> Result<u64, Failure> {
    let invalid = || Failure::new("INVALID_INPUT", "识别需要非空的 16 kHz 单声道 PCM16 WAV");
    let header = bytes.get(..44).ok_or_else(invalid)?;
    let field = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap()) as usize;
    let data = bytes.len() - 44;
    if &header[0..4] != b"RIFF"
        || &header[8..16] != b"WAVEfmt "
        || field(4) + 8 != bytes.len()
        || field(16) != 16
        || field(20) != 0x00010001
        || field(24) != 16000
        || field(28) != 32000
        || field(32) != 0x00100002
        || &header[36..40] != b"data"
        || data == 0
        || data % 2 != 0
        || field(40) != data
        || data > 9_600_000
    {
        return Err(invalid());
    }
    Ok((data as u64).div_ceil(32))
}

fn result(text: &str, duration: u64) -> Value {
    let text = text.trim();
    json!({"schemaVersion":1,"text":text,"speechState":if text.is_empty() {"no_speech"} else {"speech"},"durationMs":duration})
}

fn dispatch(
    model: Model,
    definition: &str,
    request: &Invocation,
    host: &mut dyn Host,
) -> Result<Value, Failure> {
    let payload: Value = serde_json::from_str(&request.r#payload_json)
        .map_err(|_| Failure::new("INVALID_INPUT", "识别参数格式无效"))?;
    use sdk::InvocationOperation as Op;
    match request.r#operation {
        Op::V0 => return Ok(capabilities(model, definition)),
        Op::V1 if payload["kind"] == "package_validation" => {
            return Ok(json!({
                "packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]
            }))
        }
        _ => {}
    }
    if request.r#operation == Op::V3
        && payload["capabilityKey"] == "speech.transcribe.policy"
        && payload["operationId"] == "std/speech.transcribe.policy"
    {
        return policy::submit(model, &payload);
    }
    if payload["capabilityKey"] != "speech.transcribe"
        || payload["operationId"] != "std/speech.transcribe"
        || payload["operationVersion"] != 1
        || payload["modelId"] != model.id()
    {
        return Err(Failure::new("INVALID_INPUT", "百炼识别操作身份不符"));
    }
    match request.r#operation {
        Op::V3 => {
            if payload["input"]["timing"] != "textOnly" {
                return Err(Failure::new("INVALID_INPUT", "百炼识别仅返回原文"));
            }
            let (wav, duration) = received_wav(host, &payload)?;
            if (wav.len() - 44) / 2 > policy::max_samples(model) {
                return Err(Failure::new("INVALID_INPUT", "音频超过百炼单批识别上限"));
            }
            if model == Model::Qwen {
                qwen::submit(host, &payload, &wav, duration, request.r#budget_ms)
            } else {
                task::submit(host, model, &payload, &wav, duration, request.r#budget_ms)
            }
        }
        Op::V5 if model != Model::Qwen => task::cancel(host, &payload, request.r#budget_ms),
        Op::V4 if model != Model::Qwen => task::query(host, &payload, request.r#budget_ms),
        _ => Err(Failure::new("UNSUPPORTED_FEATURE", "此百炼识别操作不可用")),
    }
}

pub fn invoke(
    model: Model,
    definition: &str,
    request: Invocation,
    host: &mut dyn Host,
) -> Response {
    match dispatch(model, definition, &request, host) {
        Ok(value) => guest::completed_response(&request, &value).expect("ASR response"),
        Err(error) => serde_json::from_value(
            json!({"protocolVersion":2,"operation":request.r#operation,"error":{
                "code":error.code,"message":error.message,
                "retryable": request.r#operation == sdk::InvocationOperation::V4 &&
                    matches!(error.code, "TIMEOUT" | "UPSTREAM_ERROR" | "RATE_LIMITED"),
                "vendorCode":error.vendor,"vendorRequestId":error.vendor_request
            }}),
        )
        .expect("ASR error"),
    }
}
