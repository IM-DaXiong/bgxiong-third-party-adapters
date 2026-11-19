#![cfg_attr(target_arch = "wasm32", no_std)]
//! LTX 2.3 production package: four frozen official graphs, prompt text kept as submitted.
extern crate alloc;
mod comfy;
use alloc::{format, string::{String, ToString}, vec, vec::Vec};
use bgx_adapter_v2_sdk::{guest, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PLUGIN: &str = "local.bgx.ltx23";

#[derive(Clone, Copy)]
struct Mode {
    id: &'static str,
    model: &'static str,
    operation: &'static str,
    label: &'static str,
    input_mode: &'static str,
    schema: &'static str,
    graph: &'static str,
    prompt: &'static str,
    prompt_field: &'static str,
    duration: &'static str,
    fps: &'static str,
    width: &'static str,
    height: &'static str,
    seeds: &'static [&'static str],
    image: Option<&'static str>,
    last: Option<&'static str>,
    audio: Option<&'static str>,
    save: &'static str,
}

const T2V: Mode = Mode {
    id: "t2v", model: "ltx23-t2v", operation: "local.bgx.ltx23/t2v", label: "LTX 2.3 文生视频",
    input_mode: "text_to_video", schema: include_str!("../schemas/t2v.input.schema.json"),
    graph: include_str!("../workflows/video_ltx2_3_t2v.json"),
    prompt: "267:266", prompt_field: "value", duration: "267:225", fps: "267:260",
    width: "267:257", height: "267:258", seeds: &["267:216", "267:237"],
    image: None, last: None, audio: None, save: "75",
};
const I2V: Mode = Mode {
    id: "i2v", model: "ltx23-i2v", operation: "local.bgx.ltx23/i2v", label: "LTX 2.3 图生视频",
    input_mode: "image_to_video", schema: include_str!("../schemas/i2v.input.schema.json"),
    graph: include_str!("../workflows/video_ltx2_3_i2v.json"),
    prompt: "320:319", prompt_field: "value", duration: "320:301", fps: "320:300",
    width: "320:312", height: "320:299", seeds: &["320:276", "320:277"],
    image: Some("269"), last: None, audio: None, save: "75",
};
const IA2V: Mode = Mode {
    id: "ia2v", model: "ltx23-ia2v", operation: "local.bgx.ltx23/ia2v", label: "LTX 2.3 图音频生视频",
    input_mode: "image_to_video", schema: include_str!("../schemas/ia2v.input.schema.json"),
    graph: include_str!("../workflows/video_ltx2_3_ia2v.json"),
    prompt: "340:319", prompt_field: "value", duration: "340:331", fps: "340:323",
    width: "340:330", height: "340:324", seeds: &["340:285", "340:286"],
    image: Some("269"), last: None, audio: Some("276"), save: "341",
};
const FLF: Mode = Mode {
    id: "flf2v", model: "ltx23-flf2v", operation: "local.bgx.ltx23/flf2v", label: "LTX 2.3 首尾帧生视频",
    input_mode: "first_last_frame", schema: include_str!("../schemas/flf2v.input.schema.json"),
    graph: include_str!("../workflows/video_ltx2_3_flf2v.json"),
    prompt: "129:128", prompt_field: "text", duration: "129:102", fps: "129:114",
    width: "129:113", height: "129:98", seeds: &["129:100"],
    image: Some("31"), last: Some("39"), audio: None, save: "68",
};
const MODES: &[Mode] = &[T2V, I2V, IA2V, FLF];

#[derive(Debug)]
pub struct Failure { pub code: &'static str, pub message: String, pub vendor: Option<String> }
impl Failure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self { Self { code, message: message.into(), vendor: None } }
}
fn invalid(message: impl Into<String>) -> Failure { Failure::new("INVALID_INPUT", message) }

fn schema_record(id: &str, text: &str) -> Value {
    let digest: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    json!({"id":id,"digest":digest,"schemaJson":text})
}

fn controls(mode: &Mode) -> Vec<Value> {
    let mut rows = vec![
        json!({"fieldPointer":"/prompt","widget":"multiline","label":"提示词","description":"","group":"输入","displayRole":"prompt"}),
        json!({"fieldPointer":"/durationSeconds","widget":"integer","label":"时长","description":"","group":"规格","displayRole":"duration"}),
        json!({"fieldPointer":"/fps","widget":"integer","label":"帧率","description":"","group":"规格","displayRole":"fps"}),
        json!({"fieldPointer":"/width","widget":"integer","label":"宽","description":"","group":"规格","displayRole":"width"}),
        json!({"fieldPointer":"/height","widget":"integer","label":"高","description":"","group":"规格","displayRole":"height"}),
        json!({"fieldPointer":"/seed","widget":"integer","label":"种子","description":"","group":"参数"}),
    ];
    if mode.image.is_some() && mode.last.is_none() {
        rows.insert(1, json!({"fieldPointer":"/image","widget":"media","label":"图片","description":"","group":"输入"}));
    }
    if mode.last.is_some() {
        rows.insert(1, json!({"fieldPointer":"/firstFrame","widget":"media","label":"首帧","description":"","group":"输入"}));
        rows.insert(2, json!({"fieldPointer":"/lastFrame","widget":"media","label":"尾帧","description":"","group":"输入"}));
    }
    if mode.audio.is_some() {
        rows.insert(2, json!({"fieldPointer":"/audio","widget":"media","label":"参考音频","description":"","group":"输入"}));
    }
    rows
}

fn slots(mode: &Mode) -> Vec<Value> {
    let mut slots = Vec::new();
    if mode.last.is_some() {
        slots.push(json!({"role":"first_frame","inputPointer":"/firstFrame","mediaKind":"image","minCount":1,"maxCount":1}));
        slots.push(json!({"role":"last_frame","inputPointer":"/lastFrame","mediaKind":"image","minCount":1,"maxCount":1}));
    } else if mode.image.is_some() {
        slots.push(json!({"role":"image","inputPointer":"/image","mediaKind":"image","minCount":1,"maxCount":1}));
    }
    if mode.audio.is_some() {
        slots.push(json!({"role":"audio","inputPointer":"/audio","mediaKind":"audio","minCount":1,"maxCount":1}));
    }
    slots
}

fn operation_value(mode: &Mode, input: &Value, output: &Value, resume: &Value) -> Value {
    json!({"id":mode.operation,"version":1,"label":mode.label,"inputProfile":"operation-input-v1",
        "promptProjection":{"profileId":"ltx","version":1},
        "inputSchema":{"id":"input","digest":input["digest"]},"outputSchema":{"id":"output","digest":output["digest"]},
        "resumeSchema":{"id":"resume","digest":resume["digest"]},
        "uiSchema":{"controls":controls(mode)},"mediaSlots":slots(mode),
        "outputSlots":[{"role":"video","kind":"video","minCount":1,"maxCount":1}],
        "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported","resumable":true,"remoteIdempotency":false}})
}

pub fn capabilities() -> Value {
    let config = schema_record("config", include_str!("../schemas/config.schema.json"));
    let output = schema_record("output", include_str!("../schemas/output.schema.json"));
    let resume = schema_record("resume", include_str!("../schemas/resume.schema.json"));
    let contracts: Vec<Value> = MODES.iter().map(|mode| {
        let input = schema_record("input", mode.schema);
        json!({"schemaVersion":2,"localId":mode.id,"capabilityKey":"video.generate","transportSlot":"video",
            "modelIds":[mode.model],"configurationSchema":{"id":"config","digest":config["digest"]},
            "operations":[operation_value(mode, &input, &output, &resume)],
            "schemas":[input, output.clone(), config.clone(), resume.clone()]})
    }).collect();
    json!({"schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),"capabilityContracts":contracts})
}

fn mode_by_operation(id: &str) -> Result<Mode, Failure> {
    MODES.iter().copied().find(|mode| mode.operation == id).ok_or_else(|| invalid("Unknown LTX 2.3 operation"))
}

fn user_message(message: &str) -> &'static str {
    if message.contains("inputMode") { "输入模式与当前操作不一致" }
    else if message.contains("Prompt is required") { "请填写提示词" }
    else if message.contains("Duration") { "时长须在 1 到 120 秒之间" }
    else if message.contains("Frame rate") { "帧率须在 6 到 120 之间" }
    else if message.contains("multiple of 64") || message.contains(" is required") { "宽高须为 64 的倍数，范围 256 到 2048" }
    else if message.contains("Seed") { "种子超出范围" }
    else if message.contains("audio") { "图音频生视频需要一条音频" }
    else if message.contains("one image") { "这个操作需要一张图片" }
    else if message.contains("frames") { "首帧和尾帧都要提供" }
    else if message.contains("Prompt enhance") { "提示词扩写必须保持关闭" }
    else { "参数不符合 LTX 2.3 的要求" }
}

fn rejected(digest: &Value, message: &str) -> Value {
    json!({"kind":"operation_input_preflight","status":"rejected","requestDigest":digest,
        "diagnostics":[{"code":"local.bgx.ltx23/invalid_input","fieldPointer":"","message":message}]})
}

/// Preflight reads the operation from selection. Submit and query keep the top-level id.
fn prepare_input(payload: &Value) -> Value {
    let digest = &payload["requestDigest"];
    let Some(id) = payload["selection"]["operationId"].as_str() else {
        return rejected(digest, "没有匹配的 LTX 2.3 操作");
    };
    let mode = match mode_by_operation(id) {
        Ok(mode) => mode,
        Err(_) => return rejected(digest, "没有匹配的 LTX 2.3 操作"),
    };
    if payload["environment"]["ready"] != true {
        return rejected(digest, "LTX 2.3 的连接尚未就绪");
    }
    if let Err(error) = graph(mode, &payload["input"], &[]) {
        return rejected(digest, user_message(&error.message));
    }
    let input = &payload["input"];
    json!({"kind":"operation_input_preflight","status":"ready","requestDigest":digest,"material":{
        "operationId":mode.operation,"durationSeconds":input["durationSeconds"],"fps":input["fps"],
        "width":input["width"],"height":input["height"],"seed":input["seed"]
    }})
}

fn aligned(value: &Value, key: &str) -> Result<u64, Failure> {
    let n = value.get(key).and_then(Value::as_u64).ok_or_else(|| invalid(format!("{key} is required")))?;
    if !(256..=2048).contains(&n) || n % 64 != 0 {
        return Err(invalid(format!("{key} must be a multiple of 64 between 256 and 2048")));
    }
    Ok(n)
}

fn connectivity(host: &mut dyn Host, ctx: &comfy::Ctx) -> Result<(), Failure> {
    comfy::status_probe(host, ctx)?;
    for source in [T2V.graph, I2V.graph, IA2V.graph, FLF.graph] {
        let frozen: Value = serde_json::from_str(source).expect("frozen graph");
        for node in frozen.as_object().unwrap().values() {
            let class = comfy::text(node, "class_type")?;
            let info = comfy::exchange(host, ctx, "GET", &format!("/object_info/{class}"), json!({"kind":"none"}), vec![], false)?;
            if info.get(class).is_none() {
                return Err(Failure::new("RESOURCE_UNAVAILABLE", format!("Required node {class} is unavailable")));
            }
            for field in ["unet_name", "clip_name", "vae_name", "ckpt_name", "lora_name"] {
                if let Some(expected) = node["inputs"].get(field).filter(|v| v.is_string()) {
                    let Some(spec) = info.get(class).and_then(|c| c.get("input")).and_then(|i| i.get("required")).and_then(|r| r.get(field)) else {
                        return Err(Failure::new("RESOURCE_UNAVAILABLE", format!("Required model field {field} is not declared by {class}")));
                    };
                    let options = if spec.get(0).is_some_and(Value::is_array) { &spec[0] } else { &spec[1]["options"] };
                    if !options.as_array().is_some_and(|items| items.contains(expected)) {
                        return Err(Failure::new("RESOURCE_UNAVAILABLE", format!("Required model for {field} is unavailable")));
                    }
                }
            }
        }
    }
    Ok(())
}

fn require_off(graph: &Value) -> Result<(), Failure> {
    let Some(nodes) = graph.as_object() else { return Err(invalid("Workflow graph must be an object")); };
    for (id, node) in nodes {
        if node["class_type"] != "PrimitiveBoolean" { continue; }
        let title = node["_meta"]["title"].as_str().unwrap_or("");
        if title.to_ascii_lowercase().contains("enable prompt enhance") && node["inputs"]["value"] != false {
            return Err(invalid(format!("Prompt enhance node {id} must stay off")));
        }
    }
    Ok(())
}

/// Patch the frozen graph. Enhance stays at its baked false value and is not a parameter.
pub(crate) fn graph(mode: Mode, input: &Value, files: &[(&str, &str)]) -> Result<Value, Failure> {
    if input["inputMode"] != mode.input_mode { return Err(invalid("inputMode does not match this operation")); }
    let prompt = input["prompt"].as_str().filter(|text| !text.is_empty()).ok_or_else(|| invalid("Prompt is required"))?;
    let duration = input["durationSeconds"].as_u64().filter(|n| (1..=120).contains(n)).ok_or_else(|| invalid("Duration must be 1 to 120 seconds"))?;
    let fps = input["fps"].as_u64().filter(|n| (6..=120).contains(n)).ok_or_else(|| invalid("Frame rate must be 6 to 120"))?;
    let width = aligned(input, "width")?;
    let height = aligned(input, "height")?;
    let seed = input["seed"].as_u64().filter(|n| *n <= 2147483647).ok_or_else(|| invalid("Seed is out of range"))?;
    if mode.audio.is_some() && input["audio"].as_array().map(|a| a.len()) != Some(1) {
        return Err(invalid("Image-audio video requires one audio file and does not fall back to image-only"));
    }
    if mode.image.is_some() && mode.last.is_none() && input["image"].as_array().map(|a| a.len()) != Some(1) {
        return Err(invalid("This operation requires one image"));
    }
    if mode.last.is_some() && (input["firstFrame"].as_array().map(|a| a.len()) != Some(1) || input["lastFrame"].as_array().map(|a| a.len()) != Some(1)) {
        return Err(invalid("First and last frames are both required"));
    }
    let mut g: Value = serde_json::from_str(mode.graph).expect("frozen graph");
    require_off(&g)?;
    g[mode.prompt]["inputs"][mode.prompt_field] = json!(prompt);
    g[mode.duration]["inputs"]["value"] = json!(duration);
    g[mode.fps]["inputs"]["value"] = json!(fps);
    g[mode.width]["inputs"]["value"] = json!(width);
    g[mode.height]["inputs"]["value"] = json!(height);
    for (index, node) in mode.seeds.iter().enumerate() {
        g[node]["inputs"]["noise_seed"] = json!(seed + index as u64);
    }
    for (node, name) in files {
        let key = if g[node]["class_type"] == "LoadAudio" { "audio" } else { "image" };
        g[node]["inputs"][key] = json!(name);
    }
    require_off(&g)?;
    Ok(g)
}

fn file_for<'a>(media: &'a [Value], id: &Value, kind: &str) -> Result<&'a Value, Failure> {
    media.iter().find(|item| &item["id"] == id && item["kind"] == kind).ok_or_else(|| invalid("Required media lease is missing"))
}

fn context<'a>(request: &Invocation, p: &'a Value) -> comfy::Ctx<'a> {
    comfy::Ctx { config: &p["configuration"], budget_ms: request.r#budget_ms }
}

fn persist(host: &mut dyn Host, request: &Invocation, accepted: &Value) -> Result<(), Failure> {
    let unknown = || Failure::new("ACCEPTANCE_UNKNOWN", "Remote accepted; checkpoint failed. Do not resubmit.");
    let saved = comfy::port(host, "host-checkpoint.read", json!({"requestId": request.r#request_id})).map_err(|_| unknown())?;
    let old = comfy::text(&saved, "revision").map_err(|_| unknown())?;
    let revision = old.parse::<u64>().ok().and_then(|v| v.checked_add(1)).ok_or_else(unknown)?;
    let mut row: Value = serde_json::from_str(comfy::text(&saved, "valueJson").map_err(|_| unknown())?).map_err(|_| unknown())?;
    row["revision"] = json!(revision.to_string());
    row["state"] = json!("accepted");
    row["vendorTaskId"] = accepted["vendorTaskId"].clone();
    row["resumeToken"] = accepted["resumeToken"].clone();
    comfy::port(host, "host-checkpoint.write", json!({"requestId": request.r#request_id, "expectedRevision": old, "valueJson": row.to_string()})).map_err(|_| unknown())?;
    Ok(())
}

fn submit(host: &mut dyn Host, request: &Invocation, p: &Value) -> Result<Value, Failure> {
    let mode = mode_by_operation(p["operationId"].as_str().unwrap_or(""))?;
    let input = &p["input"];
    graph(mode, input, &[])?;
    let media = p["media"].as_array().cloned().unwrap_or_default();
    let ctx = context(request, p);
    let mut files: Vec<(String, String)> = Vec::new();
    let mut upload_named = |node: &str, id: &Value, kind: &str| -> Result<(), Failure> {
        let item = file_for(&media, id, kind)?;
        let name = comfy::upload(host, &ctx, item)?;
        files.push((node.to_string(), name));
        Ok(())
    };
    if let Some(node) = mode.image {
        let field = if mode.last.is_some() { "firstFrame" } else { "image" };
        upload_named(node, &input[field][0]["mediaId"], "image")?;
    }
    if let Some(node) = mode.last { upload_named(node, &input["lastFrame"][0]["mediaId"], "image")?; }
    if let Some(node) = mode.audio { upload_named(node, &input["audio"][0]["mediaId"], "audio")?; }
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(n, p)| (n.as_str(), p.as_str())).collect();
    let patched = graph(mode, input, &borrowed)?;
    let remote = comfy::submit(host, &ctx, &patched, &request.r#request_id)?;
    let accepted = json!({"kind":"accepted","vendorTaskId":remote,"appliedParams":null,"usage":null,"resumeToken":{"promptId":remote}});
    persist(host, request, &accepted)?;
    Ok(accepted)
}

fn query(host: &mut dyn Host, request: &Invocation, p: &Value) -> Result<Value, Failure> {
    let mode = mode_by_operation(p["operationId"].as_str().unwrap_or(""))?;
    let id = comfy::text(p, "vendorTaskId")?;
    if p["resumeToken"]["promptId"] != id { return Err(invalid("Resume token must name the original prompt")); }
    let ctx = context(request, p);
    let (status, outputs, data, failure) = match comfy::status(host, &ctx, id, mode.save)? {
        comfy::Remote::Queued => ("queued", json!([]), Value::Null, Value::Null),
        comfy::Remote::Running => ("running", json!([]), Value::Null, Value::Null),
        comfy::Remote::Succeeded(files) => {
            let output = comfy::download(host, &ctx, &files[0])?;
            ("succeeded", json!([output.clone()]), json!({"schemaVersion":1,"video":{"mediaId":output["media"]["id"]}}), Value::Null)
        }
        comfy::Remote::Failed(reason) => ("failed", json!([]), Value::Null, json!({"category":"vendorTaskFailed","message":"LTX 2.3 execution failed","vendorCode":reason,"details":{"promptId":id}})),
        comfy::Remote::Lost => ("failed", json!([]), Value::Null, json!({"category":"vendorTaskFailed","message":"Original prompt is absent from queue and history","vendorCode":"PROMPT_NOT_FOUND","details":{"promptId":id}})),
    };
    Ok(json!({"status":status,"outputs":outputs,"data":data,"usage":null,"progress":null,"terminalFailure":failure,"resumeToken":p["resumeToken"]}))
}

pub struct Ltx23;
impl Adapter for Ltx23 {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        let wire = serde_json::to_value(&request).expect("typed invocation");
        let result = (|| {
            let p: Value = serde_json::from_str(&request.r#payload_json).map_err(|_| invalid("Payload must contain JSON"))?;
            match wire["operation"].as_str() {
                Some("capabilities") => Ok(capabilities()),
                Some("submit") => submit(host, &request, &p),
                Some("query") => query(host, &request, &p),
                Some("probe") => match p["kind"].as_str() {
                    Some("operation_input_preflight") => Ok(prepare_input(&p)),
                    Some("operation_preflight") => {
                        let mode = mode_by_operation(p["operationId"].as_str().unwrap_or(""))?;
                        graph(mode, &p["input"], &[])?;
                        Ok(json!({"kind":"operation_preflight","status":"ready","requestDigest":p["requestDigest"]}))
                    }
                    Some("package_validation") => Ok(json!({"packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]})),
                    Some("connectivity") => {
                        connectivity(host, &context(&request, &p))?;
                        Ok(json!({"packageValid":true,"connection":"ready","authentication":"ready","resources":"ready","issues":[]}))
                    }
                    _ => Err(invalid("Unknown probe kind")),
                },
                _ => Err(Failure::new("UNSUPPORTED_FEATURE", "Transport operation is unsupported")),
            }
        })();
        match result {
            Ok(value) => guest::completed_response(&request, &value).expect("public response"),
            Err(error) => serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,"error":{
                "code":error.code,"message":error.message,"retryable":false,"vendorCode":error.vendor,"vendorRequestId":null}})).expect("public error"),
        }
    }
}
bgx_adapter_v2_sdk::export_adapter!(Ltx23);

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(mode: Mode) -> Value {
        let mut input = json!({"prompt":"keep this exact sentence","inputMode":mode.input_mode,"durationSeconds":15,"fps":25,"width":1280,"height":704,"seed":7});
        if mode.last.is_some() {
            input["firstFrame"] = json!([{"mediaId":"first"}]);
            input["lastFrame"] = json!([{"mediaId":"last"}]);
        } else if mode.image.is_some() {
            input["image"] = json!([{"mediaId":"still"}]);
        }
        if mode.audio.is_some() { input["audio"] = json!([{"mediaId":"voice"}]); }
        input
    }

    #[test]
    fn four_operations_keep_prompt_and_bake_enhance_off() {
        assert_eq!(MODES.len(), 4);
        for mode in MODES {
            let g = graph(*mode, &sample(*mode), &[]).unwrap();
            assert_eq!(g[mode.prompt]["inputs"][mode.prompt_field], "keep this exact sentence");
            assert_eq!(g[mode.duration]["inputs"]["value"], 15);
            assert_eq!(g[mode.fps]["inputs"]["value"], 25);
            assert_eq!(g[mode.width]["inputs"]["value"], 1280);
            assert_eq!(g[mode.height]["inputs"]["value"], 704);
            assert_eq!(g[mode.seeds[0]]["inputs"]["noise_seed"], 7);
            let enhance = g.as_object().unwrap().values().any(|node| {
                node["class_type"] == "PrimitiveBoolean"
                    && node["_meta"]["title"].as_str().unwrap_or("").to_ascii_lowercase().contains("enable prompt enhance")
                    && node["inputs"]["value"] != false
            });
            assert!(!enhance, "{}", mode.id);
            assert!(g.to_string().contains("TextGenerateLTX2Prompt") || mode.id == "flf2v");
        }
    }

    #[test]
    fn ia2v_without_audio_is_rejected_and_fps_out_of_range_is_rejected() {
        let mut input = sample(IA2V);
        input.as_object_mut().unwrap().remove("audio");
        assert!(graph(IA2V, &input, &[]).is_err());
        let mut fast = sample(T2V);
        fast["fps"] = json!(5);
        assert!(graph(T2V, &fast, &[]).is_err());
        let mut portrait = sample(I2V);
        portrait["width"] = json!(704);
        portrait["height"] = json!(1280);
        let g = graph(I2V, &portrait, &[("269", "still.png")]).unwrap();
        assert_eq!(g["269"]["inputs"]["image"], "still.png");
        assert_eq!(g["320:312"]["inputs"]["value"], 704);
    }

    #[test]
    fn capabilities_declare_ltx_projection_and_first_last_frame() {
        let caps = capabilities();
        assert_eq!(caps["capabilityContracts"].as_array().unwrap().len(), 4);
        for contract in caps["capabilityContracts"].as_array().unwrap() {
            let op = &contract["operations"][0];
            assert_eq!(op["promptProjection"]["profileId"], "ltx");
            assert_eq!(op["promptProjection"]["version"], 1);
            assert_eq!(op["execution"]["cancellation"], "unsupported");
        }
        let flf = caps["capabilityContracts"].as_array().unwrap().iter().find(|c| c["localId"] == "flf2v").unwrap();
        assert_eq!(flf["operations"][0]["inputProfile"], "operation-input-v1");
        assert_eq!(flf["operations"][0]["mediaSlots"][0]["role"], "first_frame");
        assert_eq!(flf["operations"][0]["mediaSlots"][1]["role"], "last_frame");
    }

    fn preflight_envelope(mode: Mode) -> Value {
        json!({
            "schemaVersion": 2,
            "kind": "operation_input_preflight",
            "selection": {"operationId": mode.operation, "operationVersion": 1},
            "environment": {"ready": true},
            "input": sample(mode),
            "requestDigest": "6f24b5a8cf89b9508833ddb8bbc9c70e05be5053af6c368ccffba6ca4c9f06aa"
        })
    }

    #[test]
    fn input_preflight_reads_selection_and_returns_material() {
        let audio = prepare_input(&preflight_envelope(IA2V));
        assert_eq!(audio["status"], "ready");
        assert_eq!(audio["material"]["operationId"], IA2V.operation);
        assert_eq!(audio["material"]["durationSeconds"], 15);
        assert_eq!(audio["material"]["width"], 1280);
        assert!(audio.get("operationId").is_none());
        let text = prepare_input(&preflight_envelope(T2V));
        assert_eq!(text["status"], "ready");
        assert_eq!(text["material"]["operationId"], T2V.operation);
        let mut missing = preflight_envelope(IA2V);
        missing["selection"].as_object_mut().unwrap().remove("operationId");
        let rejected = prepare_input(&missing);
        assert_eq!(rejected["status"], "rejected");
        assert_eq!(rejected["diagnostics"][0]["message"], "没有匹配的 LTX 2.3 操作");
    }
}
