use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};

pub(super) fn request(wav: &[u8], language: &Value) -> Value {
    let mut value = json!({
        "model":"qwen3-asr-flash", "stream":false,
        "messages":[{"role":"user","content":[{"type":"input_audio","input_audio":{
            "data":format!("data:audio/wav;base64,{}", STANDARD.encode(wav))
        }}]}]
    });
    // Use only the vendor's common short language tags.
    if let Some(language @ ("zh" | "en" | "ja" | "ko" | "yue")) = language.as_str() {
        value["asr_options"] = json!({"language":language});
    }
    value
}

pub(super) fn submit(
    host: &mut dyn Host,
    payload: &Value,
    wav: &[u8],
    duration: u64,
    budget: u64,
) -> Result<Value, Failure> {
    let (origin, key) = http::origin(&payload["configuration"])?;
    let body = http::json_body(
        host,
        &request(wav, &payload["input"]["language"]),
        14_000_000,
    )?;
    let reply = http::exchange(
        host,
        "POST",
        &format!("{origin}/compatible-mode/v1/chat/completions"),
        Some(key),
        json!([{"name":"Content-Type","value":"application/json"}]),
        body,
        budget,
        true,
    )?;
    let choice = &reply["choices"][0];
    if reply["model"] != "qwen3-asr-flash" || choice["finish_reason"] != "stop" {
        return Err(Failure::new("UPSTREAM_ERROR", "千问识别结果未完整返回"));
    }
    let text = choice["message"]["content"]
        .as_str()
        .ok_or_else(|| Failure::new("UPSTREAM_ERROR", "千问识别响应缺少正文"))?;
    let usage = reply["usage"]["seconds"].as_number().map(|seconds| {
        json!({"measurements":[{"metric":"duration","unit":"seconds","amount":seconds.to_string(),"source":"reported"}]})
    });
    Ok(
        json!({"kind":"completed","vendorTaskId":null,"appliedParams":null,
        "outputs":[],"data":result(text,duration),"usage":usage}),
    )
}
