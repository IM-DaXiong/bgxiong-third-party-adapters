use super::*;

const INPUT: &str = include_str!(
    "../../../../../bgxiong-adapter-sdk/profiles/speech-analysis-v1/transcribe-policy.input.schema.json"
);
const OUTPUT: &str = include_str!(
    "../../../../../bgxiong-adapter-sdk/profiles/speech-analysis-v1/transcribe-policy.output.schema.json"
);

const CONFIGURATION: &str = include_str!(
    "../../../../../bgxiong-adapter-sdk/profiles/speech-analysis-v1/transcribe-policy.configuration.schema.json"
);

pub(super) fn max_samples(model: Model) -> usize {
    match model {
        Model::Paraformer | Model::Fun => 4_800_000,
        Model::Qwen => 3_520_000,
    }
}

pub(super) fn attach_contract(root: &mut Value, model: Model) {
    let input = schema(INPUT);
    let output = schema(OUTPUT);
    let configuration = schema(CONFIGURATION);
    let contract = json!({
        "schemaVersion": 2, "localId": "transcription-policy",
        "capabilityKey": "speech.transcribe.policy", "transportSlot": "generic",
        "modelIds": [model.id()], "configurationSchema": reference(&configuration),
        "operations": [{
            "id": "std/speech.transcribe.policy", "version": 1,
            "label": "语音转写策略", "inputSchema": reference(&input),
            "outputSchema": reference(&output), "resumeSchema": null,
            "uiSchema": {"controls": []}, "mediaSlots": [], "outputSlots": [],
            "execution": {"mode": "synchronous", "streaming": false,
                "cancellation": "unsupported", "resumable": false, "remoteIdempotency": false}
        }],
        "schemas": [input, output, configuration]
    });
    root["capabilityContracts"].as_array_mut().expect("ASR contracts").push(contract);
}

pub(super) fn submit(model: Model, payload: &Value) -> Result<Value, Failure> {
    let usage = &payload["input"]["usage"];
    if payload["input"]["schemaVersion"] != 1
        || payload["operationVersion"] != 1
        || payload["modelId"] != model.id()
    {
        return Err(Failure::new("INVALID_INPUT", "百炼识别策略请求不符"));
    }
    let mut data = json!({"schemaVersion":1, "usage":usage, "maxBatchSamples":max_samples(model)});
    match usage.as_str() {
        Some("chatMessage") => data["strategy"] = json!("wholeFile"),
        Some("continuousDictation") => {
            data["strategy"] = json!("progressiveFile");
            data["minPauseFlushSamples"] = json!(32_000);
            data["maxBufferedMs"] = json!(8_000);
        }
        _ => return Err(Failure::new("UNSUPPORTED_FEATURE", "百炼识别用途不可用")),
    }
    Ok(json!({"kind":"completed","vendorTaskId":null,"data":data,"outputs":[],"appliedParams":null,"usage":null}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_both_uses_with_per_model_limits() {
        for (model, limit) in [(Model::Paraformer, 4_800_000), (Model::Fun, 4_800_000), (Model::Qwen, 3_520_000)] {
            for (usage, strategy) in [("chatMessage", "wholeFile"), ("continuousDictation", "progressiveFile")] {
                let result = submit(model, &json!({"operationVersion":1,"modelId":model.id(),
                    "input":{"schemaVersion":1,"usage":usage}})).unwrap();
                assert_eq!(result["data"]["maxBatchSamples"], limit);
                assert_eq!(result["data"]["strategy"], strategy);
                assert_eq!(result["data"].get("maxBufferedMs").is_some(), usage == "continuousDictation");
            }
        }
    }
}

