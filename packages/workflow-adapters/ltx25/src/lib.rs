#![cfg_attr(target_arch = "wasm32", no_std)]
//! LTX 2.5 production package: T2V, I2V and official A2V frozen graphs.
extern crate alloc;
mod comfy;
use alloc::{format, string::{String, ToString}, vec, vec::Vec};
use bgx_adapter_v2_sdk::{guest, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PLUGIN: &str = "local.bgx.ltx25";
const A2V_MIN_FRAMES_PRODUCT: u64 = 8;
const A2V_DEFAULT_PROMPT: &str = "The person shown in the first frame remains the same person and talks while looking toward the camera. Mouth movement, blinking, and small natural shifts in expression and head position follow the timing of the supplied speech. Face, hair, clothing, and background stay consistent with the first frame. This is a single continuous take with a static camera, holding a medium close-up of the head and shoulders, with no pan, zoom, or cut. Lighting stays unchanged from the opening shot.";
const SOURCE_JSON: &str = include_str!("../SOURCE.json");
const T2V_GRAPH: &str = include_str!("../workflows/t2v.api.json");
const I2V_GRAPH: &str = include_str!("../workflows/i2v.api.json");
const A2V_GRAPH: &str = include_str!("../workflows/a2v.api.json");
const T2V_SCHEMA: &str = include_str!("../schemas/t2v.input.schema.json");
const I2V_SCHEMA: &str = include_str!("../schemas/i2v.input.schema.json");
const A2V_SCHEMA: &str = include_str!("../schemas/a2v.input.schema.json");

#[derive(Clone, Copy)]
struct Patch { node: &'static str, field: &'static str }
#[derive(Clone, Copy)]
enum SizeScale { Full, Half }
#[derive(Clone, Copy)]
struct ImagePort { load: &'static str, required: bool, use_switch: Option<Patch> }
#[derive(Clone, Copy)]
struct AudioPort { load: &'static str, start: Patch }
#[derive(Clone, Copy)]
struct Baked { node: &'static str, field: &'static str, value: bool }
#[derive(Clone, Copy)]
struct Mode {
    id: &'static str, model: &'static str, operation: &'static str, label: &'static str,
    input_mode: &'static str, schema: &'static str, graph: &'static str,
    prompt: Patch, duration: Patch, fps: Patch, width: Patch, height: Patch, size: SizeScale,
    seeds: &'static [(Patch, u64)],
    image: Option<ImagePort>, audio: Option<AudioPort>,
    save: &'static str, baked: &'static [Baked],
}

const T2V: Mode = Mode {
    id: "t2v", model: "ltx25-t2v", operation: "local.bgx.ltx25/t2v", label: "LTX 2.5 文生视频",
    input_mode: "text_to_video", schema: T2V_SCHEMA, graph: T2V_GRAPH,
    prompt: Patch { node: "452", field: "value" },
    duration: Patch { node: "450", field: "value" },
    fps: Patch { node: "449", field: "value" },
    width: Patch { node: "451", field: "value" },
    height: Patch { node: "448", field: "value" },
    size: SizeScale::Full,
    seeds: &[(Patch { node: "429", field: "noise_seed" }, 0), (Patch { node: "421", field: "noise_seed" }, 1)],
    image: None, audio: None, save: "75",
    baked: &[Baked { node: "454", field: "value", value: false }],
};
const I2V: Mode = Mode {
    id: "i2v", model: "ltx25-i2v", operation: "local.bgx.ltx25/i2v", label: "LTX 2.5 图生视频",
    input_mode: "image_to_video", schema: I2V_SCHEMA, graph: I2V_GRAPH,
    prompt: Patch { node: "409", field: "value" },
    duration: Patch { node: "413", field: "value" },
    fps: Patch { node: "425", field: "value" },
    width: Patch { node: "418", field: "value" },
    height: Patch { node: "421", field: "value" },
    size: SizeScale::Full,
    seeds: &[(Patch { node: "423", field: "noise_seed" }, 0), (Patch { node: "442", field: "noise_seed" }, 1)],
    image: Some(ImagePort { load: "395", required: true, use_switch: None }),
    audio: None, save: "75",
    baked: &[
        Baked { node: "407", field: "value", value: false },
        Baked { node: "414", field: "value", value: false },
    ],
};
const A2V: Mode = Mode {
    id: "a2v", model: "ltx25-a2v", operation: "local.bgx.ltx25/a2v", label: "LTX 2.5 音频生视频",
    input_mode: "image_to_video", schema: A2V_SCHEMA, graph: A2V_GRAPH,
    prompt: Patch { node: "5508", field: "value" },
    duration: Patch { node: "5512", field: "value" },
    fps: Patch { node: "5511", field: "value" },
    width: Patch { node: "5514:3059", field: "width" },
    height: Patch { node: "5514:3059", field: "height" },
    size: SizeScale::Half,
    seeds: &[(Patch { node: "5516:4832", field: "noise_seed" }, 0), (Patch { node: "5517:4967", field: "noise_seed" }, 0)],
    image: Some(ImagePort { load: "2004", required: false, use_switch: Some(Patch { node: "5014:5506", field: "value" }) }),
    audio: Some(AudioPort { load: "5600", start: Patch { node: "5601", field: "value" } }),
    save: "4852",
    baked: &[Baked { node: "5014:5556", field: "switch", value: false }],
};
const MODES: [Mode; 3] = [T2V, I2V, A2V];

#[derive(Debug)]
pub struct Failure { pub code: &'static str, pub message: String, pub vendor: Option<String> }
impl Failure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self { Self { code, message: message.into(), vendor: None } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    NegativePromptInvalid, UnknownOperation, ModeMismatch, PromptEmpty, SizeInvalid, DurationInvalid, FpsInvalid, SeedInvalid,
    A2vTooFewFrames, ImageRequired, AudioRequired, AudioTooShort, ResourceMissing, ComfyRejected, FrozenGraphDrift,
}
impl Fault {
    fn code(self) -> &'static str {
        match self {
            Self::NegativePromptInvalid => "INVALID_INPUT",
            Fault::ResourceMissing => "RESOURCE_UNAVAILABLE",
            Fault::ComfyRejected | Fault::FrozenGraphDrift => "UPSTREAM_ERROR",
            _ => "INVALID_INPUT",
        }
    }
    fn message(self) -> &'static str {
        match self {
            Self::NegativePromptInvalid => "negativePrompt must be a string when present",
            Fault::UnknownOperation => "所选操作不属于 LTX 2.5",
            Fault::ModeMismatch => "所选模式与操作不一致",
            Fault::PromptEmpty => "请填写提示词",
            Fault::SizeInvalid => "宽高需在 256 到 2048 之间，且为 64 的倍数",
            Fault::DurationInvalid => "时长超出可用范围",
            Fault::FpsInvalid => "帧率超出可用范围",
            Fault::SeedInvalid => "种子超出可用范围",
            Fault::A2vTooFewFrames => "时长乘以帧率至少为 8",
            Fault::ImageRequired => "图生视频需要一张首帧图",
            Fault::AudioRequired => "音频生视频需要一段音频",
            Fault::AudioTooShort => "音频从起点算起不足所选时长",
            Fault::ResourceMissing => "LTX 2.5 缺少所需资源",
            Fault::ComfyRejected => "ComfyUI 拒绝了本次工作流",
            Fault::FrozenGraphDrift => "适配包内工作流已损坏，请重新安装",
        }
    }
    fn failure(self) -> Failure { Failure::new(self.code(), self.message()) }
    fn resource(mode_label: &str, missing: &str) -> Failure {
        Failure::new("RESOURCE_UNAVAILABLE", format!("{mode_label}缺少 {missing}"))
    }
}

#[derive(Debug)]
struct Input {
    prompt: String,
    negative_prompt: Option<String>,
    duration: u64,
    fps: u64,
    width: u64,
    height: u64,
    seed: u64,
    audio_start: f64,
    image: Option<String>,
    audio: Option<String>,
}

fn mode_by_operation(id: &str) -> Result<&'static Mode, Fault> {
    MODES.iter().find(|mode| mode.operation == id).ok_or(Fault::UnknownOperation)
}

fn media_ids(input: &Value, key: &str) -> Vec<String> {
    input.get(key).and_then(Value::as_array).map(|items| {
        items.iter().filter_map(|item| item.get("mediaId").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)).collect()
    }).unwrap_or_default()
}

fn validate_input(mode: &Mode, input: &Value) -> Result<Input, Fault> {
    if input.get("inputMode").and_then(Value::as_str) != Some(mode.input_mode) {
        return Err(Fault::ModeMismatch);
    }
    let prompt = input.get("prompt").and_then(Value::as_str).unwrap_or("").trim();
    if prompt.is_empty() { return Err(Fault::PromptEmpty); }
    let negative_prompt = match input.get("negativePrompt") {
        None => None,
        Some(Value::String(text)) => Some(if text.trim().is_empty() { String::new() } else { text.clone() }),
        Some(_) => return Err(Fault::NegativePromptInvalid),
    };
    let duration = input.get("durationSeconds").and_then(Value::as_u64).ok_or(Fault::DurationInvalid)?;
    if !(1..=120).contains(&duration) { return Err(Fault::DurationInvalid); }
    let fps = input.get("fps").and_then(Value::as_u64).ok_or(Fault::FpsInvalid)?;
    if !(6..=120).contains(&fps) { return Err(Fault::FpsInvalid); }
    let width = input.get("width").and_then(Value::as_u64).ok_or(Fault::SizeInvalid)?;
    let height = input.get("height").and_then(Value::as_u64).ok_or(Fault::SizeInvalid)?;
    if !(256..=2048).contains(&width) || !(256..=2048).contains(&height) || width % 64 != 0 || height % 64 != 0 {
        return Err(Fault::SizeInvalid);
    }
    let seed = input.get("seed").and_then(Value::as_u64).ok_or(Fault::SeedInvalid)?;
    if seed > 2147483647 { return Err(Fault::SeedInvalid); }
    let images = media_ids(input, "image");
    let audios = media_ids(input, "audio");
    if let Some(port) = mode.image {
        if port.required && images.len() != 1 { return Err(Fault::ImageRequired); }
        if images.len() > 1 { return Err(Fault::ImageRequired); }
    } else if !images.is_empty() {
        return Err(Fault::ModeMismatch);
    }
    let mut audio_start = 0.0;
    if mode.audio.is_some() {
        if audios.len() != 1 { return Err(Fault::AudioRequired); }
        audio_start = input.get("audioStartSeconds").and_then(Value::as_f64).ok_or(Fault::DurationInvalid)?;
        if audio_start < 0.0 { return Err(Fault::DurationInvalid); }
        if fps.saturating_mul(duration) < A2V_MIN_FRAMES_PRODUCT { return Err(Fault::A2vTooFewFrames); }
    } else if !audios.is_empty() {
        return Err(Fault::ModeMismatch);
    }
    Ok(Input {
        prompt: prompt.to_string(), negative_prompt, duration, fps, width, height, seed, audio_start,
        image: images.into_iter().next(), audio: audios.into_iter().next(),
    })
}

fn write_number(graph: &mut Value, patch: Patch, value: f64) -> Result<(), Fault> {
    let node = graph.get_mut(patch.node).ok_or(Fault::FrozenGraphDrift)?;
    let class = node.get("class_type").and_then(Value::as_str).ok_or(Fault::FrozenGraphDrift)?.to_string();
    let inputs = node.get_mut("inputs").and_then(Value::as_object_mut).ok_or(Fault::FrozenGraphDrift)?;
    if class == "PrimitiveFloat" {
        inputs.insert(patch.field.to_string(), json!(value));
    } else {
        inputs.insert(patch.field.to_string(), json!(value as u64));
    }
    Ok(())
}

fn write_text(graph: &mut Value, patch: Patch, value: &str) -> Result<(), Fault> {
    let node = graph.get_mut(patch.node).ok_or(Fault::FrozenGraphDrift)?;
    let inputs = node.get_mut("inputs").and_then(Value::as_object_mut).ok_or(Fault::FrozenGraphDrift)?;
    inputs.insert(patch.field.to_string(), json!(value));
    Ok(())
}

fn assert_baked(mode: &Mode, graph: &Value) -> Result<(), Fault> {
    for baked in mode.baked {
        let actual = graph.get(baked.node).and_then(|n| n.get("inputs")).and_then(|i| i.get(baked.field));
        if actual != Some(&json!(baked.value)) { return Err(Fault::FrozenGraphDrift); }
    }
    Ok(())
}


// Request-local transformation; frozen workflow files remain byte-for-byte immutable.
const NAG_NODE: &str = "bgx_ltx25_negative_guidance";
struct NegativePorts {
    sha: &'static str, guiders: [&'static str; 2], model: &'static str,
    conditioning: &'static str, encoder: &'static str, text: Patch,
}
fn negative_ports(mode: &Mode) -> Result<NegativePorts, Fault> {
    Ok(match mode.id {
        "t2v" => NegativePorts { sha:"eb58ada6316eec5744ce12d1e70c36723dc27431aa5223f102300f10f1a703c7", guiders:["419","427"], model:"439", conditioning:"430", encoder:"433", text:Patch{node:"433",field:"text"} },
        "i2v" => NegativePorts { sha:"c81f6f9260231f4858671f2c69bd6a6e2391263fa34ab0c6711879f15979d9ed", guiders:["427","445"], model:"411", conditioning:"415", encoder:"419", text:Patch{node:"419",field:"text"} },
        "a2v" => NegativePorts { sha:"b744c59b30d414d335acd4338ac2972b7920a7a0693b1a9225696939d1f84b35", guiders:["5516:4828","5517:4964"], model:"5575:5569", conditioning:"5014:1241", encoder:"5014:2612", text:Patch{node:"5509",field:"value"} },
        _ => return Err(Fault::UnknownOperation),
    })
}
fn apply_negative_prompt(mode: &Mode, graph: &mut Value, negative: Option<&str>) -> Result<(), Fault> {
    let Some(text) = negative else { return Ok(()); };
    let ports = negative_ports(mode)?;
    let digest: String = Sha256::digest(mode.graph.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    if digest != ports.sha || graph.get(NAG_NODE).is_some()
        || graph[ports.model]["class_type"] != "UNETLoader"
        || graph[ports.conditioning]["class_type"] != "LTXVConditioning"
        || graph[ports.encoder]["class_type"] != "CLIPTextEncode"
        || graph[ports.conditioning]["inputs"]["negative"] != json!([ports.encoder,0]) {
        return Err(Fault::FrozenGraphDrift);
    }
    let guider_class = if mode.id == "a2v" { "CFGGuider" } else { "LTXVDualCFGGuider" };
    for guider in ports.guiders {
        if graph[guider]["class_type"] != guider_class
            || graph[guider]["inputs"]["model"] != json!([ports.model,0])
            || graph[guider]["inputs"]["negative"] != json!([ports.conditioning,1]) {
            return Err(Fault::FrozenGraphDrift);
        }
    }
    if mode.id == "a2v" && (graph[ports.encoder]["inputs"]["text"] != json!([ports.text.node,0])
        || graph[ports.text.node]["class_type"] != "PrimitiveStringMultiline") {
        return Err(Fault::FrozenGraphDrift);
    }
    let text = if text.trim().is_empty() { "" } else { text };
    write_text(graph, ports.text, text)?;
    if text.is_empty() { return Ok(()); }
    // Verified against KJNodes V3 define_schema and live /object_info/LTX2_NAG.
    // Only video conditioning is patched: user-supplied audio is not re-guided.
    graph[NAG_NODE] = json!({"class_type":"LTX2_NAG","inputs":{
        "model":[ports.model,0],"nag_scale":11.0,"nag_alpha":0.25,"nag_tau":2.5,
        "nag_cond_video":[ports.conditioning,1],"inplace":false
    }});
    for guider in ports.guiders { graph[guider]["inputs"]["model"] = json!([NAG_NODE,0]); }
    Ok(())
}

fn build_graph(mode: &Mode, input: &Input, files: &[(&str, &str)]) -> Result<Value, Fault> {
    let mut graph: Value = serde_json::from_str(mode.graph).map_err(|_| Fault::FrozenGraphDrift)?;
    assert_baked(mode, &graph)?;
    write_text(&mut graph, mode.prompt, &input.prompt)?;
    write_number(&mut graph, mode.duration, input.duration as f64)?;
    write_number(&mut graph, mode.fps, input.fps as f64)?;
    let (width, height) = match mode.size {
        SizeScale::Full => (input.width, input.height),
        SizeScale::Half => (input.width / 2, input.height / 2),
    };
    write_number(&mut graph, mode.width, width as f64)?;
    write_number(&mut graph, mode.height, height as f64)?;
    for (patch, offset) in mode.seeds {
        write_number(&mut graph, *patch, (input.seed + *offset) as f64)?;
    }
    if let Some(port) = mode.audio {
        write_number(&mut graph, port.start, input.audio_start)?;
    }
    for (node, name) in files {
        let class = graph.get(*node).and_then(|n| n.get("class_type")).and_then(Value::as_str).unwrap_or("");
        let field = if class == "LoadAudio" { "audio" } else { "image" };
        let target = graph.get_mut(*node).ok_or(Fault::FrozenGraphDrift)?;
        let inputs = target.get_mut("inputs").and_then(Value::as_object_mut).ok_or(Fault::FrozenGraphDrift)?;
        inputs.insert(field.to_string(), json!(*name));
    }
    if let Some(port) = mode.image {
        if input.image.is_none() && !port.required {
            // RT-A1
            graph[port.load] = json!({"class_type":"EmptyImage","inputs":{"width":512,"height":512,"batch_size":1,"color":0},"_meta":{"title":"No first frame"}});
            if let Some(sw) = port.use_switch {
                write_number(&mut graph, sw, 0.0)?;
                // PrimitiveBoolean should be false, not 0. Fix below.
            }
        } else if let Some(sw) = port.use_switch {
            write_number(&mut graph, sw, 1.0)?;
        }
    }
    // Boolean switches must be JSON booleans, not integers.
    if let Some(port) = mode.image {
        if let Some(sw) = port.use_switch {
            let flag = input.image.is_some();
            write_text(&mut graph, sw, "")?; // placeholder replaced next
            graph[sw.node]["inputs"][sw.field] = json!(flag);
        }
    }
    apply_negative_prompt(mode, &mut graph, input.negative_prompt.as_deref())?;
    assert_baked(mode, &graph)?;
    Ok(graph)
}

fn combo_options(spec: &Value) -> Option<&Vec<Value>> {
    if let Some(first) = spec.get(0) {
        if first.is_array() { return first.as_array(); }
    }
    spec.get(1).and_then(|v| v.get("options")).and_then(Value::as_array)
}

fn combo_option_matches(item: &Value, expected: &str) -> bool {
    item.as_str() == Some(expected)
        || item.get("key").and_then(Value::as_str) == Some(expected)
}

fn is_upload_field(class: &str, field: &str) -> bool {
    (class == "LoadImage" && field == "image") || (class == "LoadAudio" && field == "audio")
}

/// Only model/weight/file combos are treated as installable resources.
/// Parameter enumerations such as SaveVideo.format/codec, sampler_name, scheduler,
/// device, and weight_dtype must not be reported as missing files.
fn is_model_resource_field(field: &str) -> bool {
    matches!(
        field,
        "unet_name"
            | "vae_name"
            | "clip_name"
            | "ckpt_name"
            | "lora_name"
            | "model_name"
            | "audio_vae"
    )
}

struct Issue { message: String }

fn missing_resource(mode: &Mode, missing: &str) -> Issue {
    Issue { message: format!("{}缺少 {}", mode.label, missing) }
}

/// Node classes this mode must probe on ComfyUI. A2V also needs EmptyImage when
/// the first-frame slot is empty (runtime swaps LoadImage for EmptyImage).
fn resource_classes(mode: &Mode) -> Vec<String> {
    let graph: Value = serde_json::from_str(mode.graph).expect("frozen graph");
    let mut classes: Vec<String> = Vec::new();
    for node in graph.as_object().expect("frozen graph object").values() {
        let Some(class) = node.get("class_type").and_then(Value::as_str) else { continue; };
        if !classes.iter().any(|existing| existing == class) {
            classes.push(class.to_string());
        }
    }
    if mode.id == "a2v" && !classes.iter().any(|existing| existing == "EmptyImage") {
        classes.push("EmptyImage".to_string());
    }
    classes
}


fn needs_negative_guidance(input: &Input) -> bool {
    input.negative_prompt.as_ref().is_some_and(|text| !text.trim().is_empty())
}
fn request_resource_classes(mode: &Mode, input: &Input) -> Vec<String> {
    let mut classes = resource_classes(mode);
    if needs_negative_guidance(input) { classes.push("LTX2_NAG".to_string()); }
    classes
}
fn nag_contract_compatible(info: &Value) -> bool {
    let node = &info["LTX2_NAG"];
    let required = &node["input"]["required"];
    let optional = &node["input"]["optional"];
    [("model","MODEL"),("nag_scale","FLOAT"),("nag_alpha","FLOAT"),("nag_tau","FLOAT")]
        .iter().all(|(key,kind)| required[*key][0] == *kind)
        && optional["nag_cond_video"][0] == "CONDITIONING"
        && optional["inplace"][0] == "BOOLEAN"
        && node["output"][0] == "MODEL"
}
fn check_request_resources(mode: &Mode, input: &Input, info: &Value) -> Vec<Issue> {
    let mut issues = check_mode_resources(mode, info);
    if needs_negative_guidance(input) && !nag_contract_compatible(info) {
        issues.push(missing_resource(mode,"LTX2_NAG (missing or incompatible input/output contract)"));
    }
    issues
}

fn apply_prompt_sampler_seed(graph: &mut Value, object_info: &Value, seed: u64) {
    let options = &object_info["TextGenerateLTX2Prompt"]["input"]["required"]["sampling_mode"][1]["options"];
    let requires_seed = options.as_array().is_some_and(|options| {
        options.iter().any(|option| {
            option["key"] == "on" && option["inputs"]["required"]["seed"][0] == "INT"
        })
    });
    if !requires_seed {
        return;
    }
    for node in graph.as_object_mut().expect("prepared graph is an object").values_mut() {
        if node["class_type"] != "TextGenerateLTX2Prompt" || node["inputs"]["sampling_mode"] != "on" {
            continue;
        }
        if let Some(inputs) = node["inputs"].as_object_mut() {
            inputs.entry("sampling_mode.seed").or_insert_with(|| json!(seed));
        }
    }
}

fn check_mode_resources(mode: &Mode, object_info: &Value) -> Vec<Issue> {
    let mut issues = Vec::new();
    let graph: Value = serde_json::from_str(mode.graph).expect("frozen graph");
    let nodes = graph.as_object().expect("frozen graph object");
    if mode.id == "a2v" && object_info.get("EmptyImage").is_none() {
        issues.push(missing_resource(mode, "EmptyImage"));
    }
    for node in nodes.values() {
        let Some(class) = node.get("class_type").and_then(Value::as_str) else { continue; };
        if object_info.get(class).is_none() {
            issues.push(missing_resource(mode, class));
            continue;
        }
        let Some(inputs) = node.get("inputs").and_then(Value::as_object) else { continue; };
        for (field, value) in inputs {
            if !is_model_resource_field(field) { continue; }
            let Some(expected) = value.as_str() else { continue; };
            if expected.is_empty() || is_upload_field(class, field) { continue; }
            let info = &object_info[class];
            let required = info.get("input").and_then(|i| i.get("required")).and_then(|r| r.get(field));
            let optional = info.get("input").and_then(|i| i.get("optional")).and_then(|r| r.get(field));
            let Some(spec) = required.or(optional) else { continue; };
            if let Some(options) = combo_options(spec) {
                if !options.iter().any(|item| combo_option_matches(item, expected)) {
                    issues.push(missing_resource(mode, expected));
                }
            }
        }
    }
    issues
}

fn check_audio_window(mode: &Mode, input: &Input, duration_ms: Option<u64>) -> Result<(), Fault> {
    if mode.audio.is_none() { return Ok(()); }
    let Some(ms) = duration_ms else { return Ok(()); }; // U-02: allow when duration is unknown
    let needed = (input.audio_start + input.duration as f64) * 1000.0;
    if needed > ms as f64 { Err(Fault::AudioTooShort) } else { Ok(()) }
}

fn schema_record(id: &str, text: &str) -> Value {
    let digest: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    json!({"id":id,"digest":digest,"schemaJson":text})
}

fn controls(mode: &Mode) -> Vec<Value> {
    let mut rows = vec![
        json!({"fieldPointer":"/prompt","widget":"multiline","label":"提示词","description":"","group":"输入","displayRole":"prompt"}),
        json!({"fieldPointer":"/negativePrompt","widget":"multiline","label":"负面提示词","description":"","group":"输入","displayRole":"negativePrompt"}),
        json!({"fieldPointer":"/durationSeconds","widget":"integer","label":"时长","description":"","group":"规格","displayRole":"duration"}),
        json!({"fieldPointer":"/fps","widget":"integer","label":"帧率","description":"","group":"规格","displayRole":"fps"}),
        json!({"fieldPointer":"/width","widget":"integer","label":"宽","description":"","group":"规格","displayRole":"width"}),
        json!({"fieldPointer":"/height","widget":"integer","label":"高","description":"","group":"规格","displayRole":"height"}),
        json!({"fieldPointer":"/seed","widget":"integer","label":"种子","description":"","group":"参数"}),
    ];
    if mode.image.is_some() {
        rows.insert(1, json!({"fieldPointer":"/image","widget":"media","label":"图片","description":"","group":"输入"}));
    }
    if mode.audio.is_some() {
        let at = if mode.image.is_some() { 2 } else { 1 };
        rows.insert(at, json!({"fieldPointer":"/audio","widget":"media","label":"参考音频","description":"","group":"输入"}));
        rows.insert(at + 1, json!({"fieldPointer":"/audioStartSeconds","widget":"number","label":"音频起点","description":"","group":"输入"}));
    }
    rows
}

fn slots(mode: &Mode) -> Vec<Value> {
    let mut slots = Vec::new();
    if let Some(port) = mode.image {
        let min = if port.required { 1 } else { 0 };
        slots.push(json!({"role":"image","inputPointer":"/image","mediaKind":"image","minCount":min,"maxCount":1}));
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

fn rejected(digest: &Value, message: &str) -> Value {
    json!({"kind":"operation_input_preflight","status":"rejected","requestDigest":digest,
        "diagnostics":[{"code":"local.bgx.ltx25/invalid_input","fieldPointer":"","message":message}]})
}

fn prepare_input(payload: &Value) -> Value {
    let digest = &payload["requestDigest"];
    let Some(id) = payload["selection"]["operationId"].as_str() else {
        return rejected(digest, Fault::UnknownOperation.message());
    };
    let mode = match mode_by_operation(id) {
        Ok(mode) => mode,
        Err(fault) => return rejected(digest, fault.message()),
    };
    if let Err(fault) = validate_input(mode, &payload["input"]) {
        return rejected(digest, fault.message());
    }
    if let Some(meta) = payload.get("mediaMetadata").and_then(Value::as_array) {
        if let Ok(parsed) = validate_input(mode, &payload["input"]) {
            if let Some(image) = parsed.image.as_ref() {
                let ok = meta.iter().any(|item| item.get("mediaId").and_then(Value::as_str) == Some(image.as_str()) && item.get("kind").and_then(Value::as_str) == Some("image"));
                if !ok { return rejected(digest, Fault::ImageRequired.message()); }
            }
            if let Some(audio) = parsed.audio.as_ref() {
                let ok = meta.iter().any(|item| item.get("mediaId").and_then(Value::as_str) == Some(audio.as_str()) && item.get("kind").and_then(Value::as_str) == Some("audio"));
                if !ok { return rejected(digest, Fault::AudioRequired.message()); }
            }
        }
    }
    let input = &payload["input"];
    let mut result = json!({"kind":"operation_input_preflight","status":"ready","requestDigest":digest,"material":{
        "operationId":mode.operation,"durationSeconds":input["durationSeconds"],"fps":input["fps"],
        "width":input["width"],"height":input["height"],"seed":input["seed"]
    }});
    if let Some(text) = validate_input(mode, input).ok().and_then(|input| input.negative_prompt) {
        result["material"]["negativePrompt"] = json!(text);
    }
    result
}

fn file_for<'a>(media: &'a [Value], id: &str, kind: &str) -> Result<&'a Value, Failure> {
    media.iter().find(|item| item["id"] == id && item["kind"] == kind).ok_or_else(|| Failure::new("INVALID_INPUT", "Required media lease is missing"))
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
    row["result"] = accepted.clone();
    comfy::port(host, "host-checkpoint.write", json!({"requestId": request.r#request_id, "expectedRevision": old, "valueJson": row.to_string()})).map_err(|_| unknown())?;
    Ok(())
}

fn connectivity(host: &mut dyn Host, ctx: &comfy::Ctx) -> Result<Value, Failure> {
    if let Err(error) = comfy::status_probe(host, ctx) {
        return Ok(json!({"packageValid":true,"connection":"failed","authentication":"ready","resources":"not_checked","issues":[{"code":error.code,"message":error.message}]}));
    }
    let mut classes: Vec<String> = Vec::new();
    for mode in &MODES {
        for class in resource_classes(mode) {
            if !classes.iter().any(|existing| existing == &class) {
                classes.push(class);
            }
        }
    }
    let info = comfy::object_info_for_classes(host, ctx, &classes)?;
    let mut issues = Vec::new();
    for mode in &MODES {
        for issue in check_mode_resources(mode, &info) {
            issues.push(json!({"code":"RESOURCE_UNAVAILABLE","message":issue.message}));
        }
    }
    if issues.is_empty() {
        Ok(json!({"packageValid":true,"connection":"ready","authentication":"ready","resources":"ready","issues":[]}))
    } else {
        Ok(json!({"packageValid":true,"connection":"ready","authentication":"ready","resources":"failed","issues":issues}))
    }
}

fn submit(host: &mut dyn Host, request: &Invocation, p: &Value) -> Result<Value, Failure> {
    let mode = mode_by_operation(p["operationId"].as_str().unwrap_or("")).map_err(Fault::failure)?;
    let parsed = validate_input(mode, &p["input"]).map_err(Fault::failure)?;
    let ctx = context(request, p);
    let classes = request_resource_classes(mode, &parsed);
    let info = comfy::object_info_for_classes(host, &ctx, &classes)?;
    if let Some(issue) = check_request_resources(mode, &parsed, &info).into_iter().next() {
        return Err(Failure::new("RESOURCE_UNAVAILABLE", issue.message));
    }
    if let Some(port) = mode.audio {
        let media = p["media"].as_array().cloned().unwrap_or_default();
        let audio_id = parsed.audio.as_deref().ok_or_else(|| Fault::AudioRequired.failure())?;
        let item = file_for(&media, audio_id, "audio")?;
        let handle = comfy::text(item, "handle").map_err(|_| Failure::new("INVALID_INPUT", "Required media lease is missing"))?;
        let duration_ms = comfy::media_duration_ms(host, handle)?;
        check_audio_window(mode, &parsed, duration_ms).map_err(Fault::failure)?;
        let _ = port;
    }
    let media = p["media"].as_array().cloned().unwrap_or_default();
    let mut files: Vec<(String, String)> = Vec::new();
    if let (Some(port), Some(id)) = (mode.image, parsed.image.as_deref()) {
        let item = file_for(&media, id, "image")?;
        files.push((port.load.to_string(), comfy::upload(host, &ctx, item)?));
    }
    if let (Some(port), Some(id)) = (mode.audio, parsed.audio.as_deref()) {
        let item = file_for(&media, id, "audio")?;
        files.push((port.load.to_string(), comfy::upload(host, &ctx, item)?));
    }
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(n, p)| (n.as_str(), p.as_str())).collect();
    let mut patched = build_graph(mode, &parsed, &borrowed).map_err(Fault::failure)?;
    apply_prompt_sampler_seed(&mut patched, &info, parsed.seed);
    let remote = match comfy::submit(host, &ctx, &patched, &request.r#request_id) {
        Ok(id) => id,
        Err(mut error) => {
            if error.code == "UPSTREAM_ERROR" || error.message.contains("400") {
                error.code = Fault::ComfyRejected.code();
                error.message = Fault::ComfyRejected.message().to_string();
                // vendor/details stay out of the user-facing message; log is host-side.
            }
            return Err(error);
        }
    };
    let accepted = json!({"kind":"accepted","vendorTaskId":remote,"appliedParams":null,"usage":null,"resumeToken":{"promptId":remote}});
    persist(host, request, &accepted)?;
    Ok(accepted)
}

fn succeeded_data(media_id: &str) -> Value { json!({"description": media_id}) }

fn query(host: &mut dyn Host, request: &Invocation, p: &Value) -> Result<Value, Failure> {
    let mode = mode_by_operation(p["operationId"].as_str().unwrap_or("")).map_err(Fault::failure)?;
    let id = comfy::text(p, "vendorTaskId")?;
    if p["resumeToken"]["promptId"] != id { return Err(Failure::new("INVALID_INPUT", "Resume token must name the original prompt")); }
    let ctx = context(request, p);
    let (status, outputs, data, failure) = match comfy::status(host, &ctx, id, mode.save)? {
        comfy::Remote::Queued => ("queued", json!([]), Value::Null, Value::Null),
        comfy::Remote::Running => ("running", json!([]), Value::Null, Value::Null),
        comfy::Remote::Succeeded(files) => {
            let output = comfy::download(host, &ctx, &files[0])?;
            let media_id = output["media"]["id"].as_str().unwrap_or("");
            ("succeeded", json!([output.clone()]), succeeded_data(media_id), Value::Null)
        }
        comfy::Remote::Failed(reason) => ("failed", json!([]), Value::Null, json!({"category":"vendorTaskFailed","message":"LTX 2.5 execution failed","vendorCode":reason,"details":{"promptId":id}})),
        comfy::Remote::Lost => ("failed", json!([]), Value::Null, json!({"category":"vendorTaskFailed","message":"Original prompt is absent from queue and history","vendorCode":"PROMPT_NOT_FOUND","details":{"promptId":id}})),
    };
    Ok(json!({"status":status,"outputs":outputs,"data":data,"usage":null,"progress":null,"terminalFailure":failure,"resumeToken":p["resumeToken"]}))
}

pub struct Ltx25;
impl Adapter for Ltx25 {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        let wire = serde_json::to_value(&request).expect("typed invocation");
        let result = (|| {
            let p: Value = serde_json::from_str(&request.r#payload_json).map_err(|_| Failure::new("INVALID_INPUT", "Payload must contain JSON"))?;
            match wire["operation"].as_str() {
                Some("capabilities") => Ok(capabilities()),
                Some("submit") => submit(host, &request, &p),
                Some("query") => query(host, &request, &p),
                Some("probe") => match p["kind"].as_str() {
                    Some("operation_input_preflight") => Ok(prepare_input(&p)),
                    Some("operation_preflight") => {
                        let mode = mode_by_operation(p["operationId"].as_str().unwrap_or("")).map_err(Fault::failure)?;
                        validate_input(mode, &p["input"]).map_err(Fault::failure)?;
                        Ok(json!({"kind":"operation_preflight","status":"ready","requestDigest":p["requestDigest"]}))
                    }
                    Some("package_validation") => Ok(json!({"packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]})),
                    Some("connectivity") => connectivity(host, &context(&request, &p)),
                    _ => Err(Failure::new("INVALID_INPUT", "Unknown probe kind")),
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
bgx_adapter_v2_sdk::export_adapter!(Ltx25);
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn sample(mode: &Mode) -> Value {
        let mut input = json!({
            "prompt": "A person walks across a quiet street in daylight.",
            "inputMode": mode.input_mode,
            "durationSeconds": 5,
            "fps": 24,
            "width": 1280,
            "height": 704,
            "seed": 7
        });
        if let Some(port) = mode.image {
            if port.required {
                input["image"] = json!([{"mediaId":"img-1"}]);
            }
        }
        if mode.audio.is_some() {
            input["audio"] = json!([{"mediaId":"aud-1"}]);
            input["audioStartSeconds"] = json!(0.0);
        }
        input
    }

    fn hash(text: &str) -> String {
        Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn capabilities_declare_three_operations() {
        let caps = capabilities();
        assert_eq!(caps["pluginId"], PLUGIN);
        let contracts = caps["capabilityContracts"].as_array().unwrap();
        assert_eq!(contracts.len(), 3);
        for (mode, contract) in MODES.iter().zip(contracts) {
            assert_eq!(contract["localId"], mode.id);
            assert_eq!(contract["capabilityKey"], "video.generate");
            assert_eq!(contract["transportSlot"], "video");
            let op = &contract["operations"][0];
            assert_eq!(op["id"], mode.operation);
            let schema: Value = serde_json::from_str(mode.schema).unwrap();
            assert_eq!(schema["properties"]["negativePrompt"], json!({"type":"string"}));
            assert!(schema["required"].as_array().unwrap().iter().all(|key| key != "negativePrompt"));
            assert_eq!(op["uiSchema"]["controls"].as_array().unwrap().iter()
                .filter(|control| control["fieldPointer"] == "/negativePrompt"
                    && control["displayRole"] == "negativePrompt").count(), 1);
            assert_eq!(op["promptProjection"], json!({"profileId":"ltx","version":1}));
            assert_eq!(op["execution"]["cancellation"], "unsupported");
            assert_eq!(op["execution"]["resumable"], true);
            let slots = op["mediaSlots"].as_array().unwrap();
            match mode.id {
                "t2v" => assert!(slots.is_empty()),
                "i2v" => {
                    assert_eq!(slots[0]["role"], "image");
                    assert_eq!(slots[0]["minCount"], 1);
                    assert_eq!(slots[0]["maxCount"], 1);
                    assert_eq!(slots[0]["inputPointer"], "/image");
                }
                "a2v" => {
                    assert_eq!(slots[0]["role"], "image");
                    assert_eq!(slots[0]["minCount"], 0);
                    assert_eq!(slots[0]["maxCount"], 1);
                    assert_eq!(slots[1]["role"], "audio");
                    assert_eq!(slots[1]["minCount"], 1);
                    assert_eq!(slots[1]["inputPointer"], "/audio");
                }
                _ => panic!("unexpected mode"),
            }
        }
    }

    #[test]
    fn only_a2v_declares_default_prompt() {
        let t2v: Value = serde_json::from_str(T2V_SCHEMA).unwrap();
        let i2v: Value = serde_json::from_str(I2V_SCHEMA).unwrap();
        let a2v: Value = serde_json::from_str(A2V_SCHEMA).unwrap();
        assert!(t2v["properties"]["prompt"].get("default").is_none());
        assert!(i2v["properties"]["prompt"].get("default").is_none());
        assert_eq!(a2v["properties"]["prompt"]["default"], A2V_DEFAULT_PROMPT);
        let caps = capabilities();
        let a2v_schema = caps["capabilityContracts"].as_array().unwrap().iter()
            .find(|c| c["localId"] == "a2v").unwrap()["schemas"].as_array().unwrap()
            .iter().find(|s| s["id"] == "input").unwrap();
        let parsed: Value = serde_json::from_str(a2v_schema["schemaJson"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["properties"]["prompt"]["default"], A2V_DEFAULT_PROMPT);
    }

    #[test]
    fn frozen_graphs_match_source() {
        let source: Value = serde_json::from_str(SOURCE_JSON).unwrap();
        for (name, graph) in [("t2v", T2V_GRAPH), ("i2v", I2V_GRAPH), ("a2v", A2V_GRAPH)] {
            assert_eq!(hash(graph), source["frozen"]["files"][name]["sha256"]);
        }
    }

    #[test]
    fn frozen_graphs_have_no_removed_classes() {
        for mode in &MODES {
            let graph: Value = serde_json::from_str(mode.graph).unwrap();
            for node in graph.as_object().unwrap().values() {
                let class = node["class_type"].as_str().unwrap();
                assert!(!matches!(class, "GemmaAPITextEncode" | "StringContains" | "ResolutionSelector"));
            }
            assert_baked(mode, &graph).unwrap();
        }
    }

    #[test]
    fn t2v_patch_writes_public_fields() {
        let input = validate_input(&T2V, &sample(&T2V)).unwrap();
        let graph = build_graph(&T2V, &input, &[]).unwrap();
        assert_eq!(graph["452"]["inputs"]["value"], input.prompt);
        assert_eq!(graph["450"]["inputs"]["value"], 5);
        assert_eq!(graph["449"]["inputs"]["value"], 24);
        assert_eq!(graph["451"]["inputs"]["value"], 1280);
        assert_eq!(graph["448"]["inputs"]["value"], 704);
        assert_eq!(graph["429"]["inputs"]["noise_seed"], 7);
        assert_eq!(graph["421"]["inputs"]["noise_seed"], 8);
        assert_eq!(graph["454"]["inputs"]["value"], false);

        let mut compatible = graph.clone();
        apply_prompt_sampler_seed(&mut compatible, &json!({}), input.seed);
        assert_eq!(compatible, graph);
        let object_info = json!({"TextGenerateLTX2Prompt":{"input":{"required":{"sampling_mode":[
            "COMFY_DYNAMICCOMBO_V3", {"options":[{"key":"on","inputs":{"required":{"seed":["INT",{}]}}}]}
        ]}}}});
        apply_prompt_sampler_seed(&mut compatible, &object_info, input.seed);
        assert_eq!(compatible["443"]["inputs"]["sampling_mode.seed"], 7);
        apply_prompt_sampler_seed(&mut compatible, &object_info, 11);
        assert_eq!(compatible["443"]["inputs"]["sampling_mode.seed"], 7);
    }

    #[test]
    fn i2v_requires_image() {
        let mut input = sample(&I2V);
        input.as_object_mut().unwrap().remove("image");
        assert_eq!(validate_input(&I2V, &input).unwrap_err(), Fault::ImageRequired);
        // Missing image must not be accepted as I2V (no silent downgrade to T2V).
        assert!(validate_input(&I2V, &input).is_err());
    }

    #[test]
    fn a2v_without_image_uses_empty_image() {
        let parsed = validate_input(&A2V, &sample(&A2V)).unwrap();
        assert!(parsed.image.is_none());
        let graph = build_graph(&A2V, &parsed, &[("5600", "speech.wav")]).unwrap();
        assert_eq!(graph["2004"]["class_type"], "EmptyImage");
        assert_eq!(graph["2004"]["inputs"]["width"], 512);
        assert_eq!(graph["2004"]["inputs"]["height"], 512);
        assert_eq!(graph["5014:5506"]["inputs"]["value"], false);
    }

    #[test]
    fn a2v_with_image_enables_first_frame() {
        let mut raw = sample(&A2V);
        raw["image"] = json!([{"mediaId":"img-1"}]);
        let parsed = validate_input(&A2V, &raw).unwrap();
        let graph = build_graph(&A2V, &parsed, &[("2004", "first.png"), ("5600", "speech.wav")]).unwrap();
        assert_eq!(graph["2004"]["class_type"], "LoadImage");
        assert_eq!(graph["2004"]["inputs"]["image"], "first.png");
        assert_eq!(graph["5014:5506"]["inputs"]["value"], true);
    }

    #[test]
    fn a2v_writes_half_latent_size() {
        let parsed = validate_input(&A2V, &sample(&A2V)).unwrap();
        let graph = build_graph(&A2V, &parsed, &[("5600", "speech.wav")]).unwrap();
        assert_eq!(graph["5514:3059"]["inputs"]["width"], 640);
        assert_eq!(graph["5514:3059"]["inputs"]["height"], 352);
    }

    #[test]
    fn a2v_requires_audio() {
        let mut raw = sample(&A2V);
        raw.as_object_mut().unwrap().remove("audio");
        assert_eq!(validate_input(&A2V, &raw).unwrap_err(), Fault::AudioRequired);
    }

    #[test]
    fn a2v_rejects_too_few_frames() {
        let mut raw = sample(&A2V);
        raw["durationSeconds"] = json!(1);
        raw["fps"] = json!(6);
        assert_eq!(validate_input(&A2V, &raw).unwrap_err(), Fault::A2vTooFewFrames);
    }

    #[test]
    fn a2v_audio_window() {
        let parsed = validate_input(&A2V, &sample(&A2V)).unwrap();
        assert_eq!(check_audio_window(&A2V, &parsed, Some(2000)).unwrap_err(), Fault::AudioTooShort);
        assert!(check_audio_window(&A2V, &parsed, Some(5000)).is_ok());
        assert!(check_audio_window(&A2V, &parsed, None).is_ok());
        let mut raw = sample(&A2V);
        raw["audioStartSeconds"] = json!(2.0);
        let late = validate_input(&A2V, &raw).unwrap();
        assert_eq!(check_audio_window(&A2V, &late, Some(6000)).unwrap_err(), Fault::AudioTooShort);
        assert!(check_audio_window(&A2V, &late, Some(7000)).is_ok());
    }

    #[test]
    fn seeds_follow_offset_table() {
        for mode in &MODES {
            let parsed = validate_input(mode, &sample(mode)).unwrap();
            let files: Vec<(&str, &str)> = if mode.audio.is_some() { vec![("5600", "speech.wav")] } else { vec![] };
            let graph = build_graph(mode, &parsed, &files).unwrap();
            for (patch, offset) in mode.seeds {
                assert_eq!(graph[patch.node]["inputs"][patch.field], parsed.seed + *offset);
            }
        }
    }

    #[test]
    fn size_rejects_non_multiple_of_64() {
        let mut raw = sample(&T2V);
        raw["height"] = json!(720);
        assert_eq!(validate_input(&T2V, &raw).unwrap_err(), Fault::SizeInvalid);
        raw["height"] = json!(704);
        raw["width"] = json!(1000);
        assert_eq!(validate_input(&T2V, &raw).unwrap_err(), Fault::SizeInvalid);
    }

    #[test]
    fn preflight_uses_selection_operation_id() {
        let payload = json!({
            "kind":"operation_input_preflight",
            "requestDigest":"abc",
            "selection":{"operationId":"local.bgx.ltx25/i2v"},
            "input": sample(&T2V)
        });
        let result = prepare_input(&payload);
        assert_eq!(result["status"], "rejected");
        assert_eq!(result["diagnostics"][0]["message"], Fault::ModeMismatch.message());
        let ok = json!({
            "kind":"operation_input_preflight",
            "requestDigest":"abc",
            "selection":{"operationId":"local.bgx.ltx25/t2v"},
            "input": sample(&T2V)
        });
        assert_eq!(prepare_input(&ok)["status"], "ready");
        let unknown = json!({
            "kind":"operation_input_preflight",
            "requestDigest":"abc",
            "selection":{"operationId":"local.bgx.ltx25/missing"},
            "input": sample(&T2V)
        });
        assert_eq!(prepare_input(&unknown)["diagnostics"][0]["message"], Fault::UnknownOperation.message());
    }

    #[test]
    fn a2v_resource_classes_include_audio_and_empty_image() {
        let classes = resource_classes(&A2V);
        assert!(classes.iter().any(|c| c == "LoadAudio"));
        assert!(classes.iter().any(|c| c == "EmptyImage"));
        assert!(classes.iter().any(|c| c == "LoadImage"));
        assert!(!classes.iter().any(|c| c == "KSampler"));
        let t2v = resource_classes(&T2V);
        assert!(!t2v.iter().any(|c| c == "LoadAudio"));
        assert!(!t2v.iter().any(|c| c == "EmptyImage"));
    }

    fn declared_model_info() -> Value {
        let mut info = json!({"EmptyImage":{"input":{"required":{}}}});
        for mode in &MODES {
            let graph: Value = serde_json::from_str(mode.graph).unwrap();
            for node in graph.as_object().unwrap().values() {
                let class = node["class_type"].as_str().unwrap();
                if info.get(class).is_none() {
                    info[class] = json!({"input":{"required":{}}});
                }
                if let Some(inputs) = node.get("inputs").and_then(Value::as_object) {
                    for (field, value) in inputs {
                        if let Some(name) = value.as_str() {
                            if is_model_resource_field(field) && !name.is_empty() {
                                let options = info[class]["input"]["required"][field].get(0).cloned().unwrap_or(json!([]));
                                let mut names = options.as_array().cloned().unwrap_or_default();
                                if !names.iter().any(|item| combo_option_matches(item, name)) {
                                    names.push(json!(name));
                                }
                                info[class]["input"]["required"][field] = json!([names, {}]);
                            }
                        }
                    }
                }
            }
        }
        info["SaveVideo"]["input"]["required"]["format"] = json!([
            "COMFY_DYNAMICCOMBO_V3",
            {"options":[{"key":"auto","label":"Auto"},{"key":"mp4"}]}
        ]);
        info["SaveVideo"]["input"]["required"]["codec"] = json!([
            "COMFY_DYNAMICCOMBO_V3",
            {"options":[{"key":"auto","label":"Auto"}]}
        ]);
        info
    }

    fn assert_no_duplicate_product_prefix(issues: &[Issue]) {
        for issue in issues {
            assert!(
                !issue.message.contains("LTX 2.5 LTX 2.5"),
                "duplicate product prefix: {}",
                issue.message
            );
            if issue.message.contains("缺少") {
                assert!(
                    issue.message.starts_with("LTX 2.5 ") && !issue.message.starts_with("LTX 2.5 LTX 2.5"),
                    "unexpected resource message: {}",
                    issue.message
                );
            }
        }
    }

    #[test]
    fn combo_option_matches_string_and_dynamic_key() {
        assert!(combo_option_matches(&json!("auto"), "auto"));
        assert!(combo_option_matches(&json!({"key":"auto","label":"Auto"}), "auto"));
        assert!(!combo_option_matches(&json!({"key":"mp4"}), "auto"));
        assert!(!combo_option_matches(&json!("mp4"), "auto"));
        assert!(!combo_option_matches(&json!({"label":"auto"}), "auto"));
    }

    #[test]
    fn resource_check_reports_per_mode() {
        let empty = json!({});
        let t2v_issues = check_mode_resources(&T2V, &empty);
        assert!(t2v_issues.iter().any(|i| i.message.starts_with("LTX 2.5 文生视频缺少 ")));
        assert_no_duplicate_product_prefix(&t2v_issues);
        let i2v_issues = check_mode_resources(&I2V, &empty);
        assert!(i2v_issues.iter().any(|i| i.message.starts_with("LTX 2.5 图生视频缺少 ")));
        assert_no_duplicate_product_prefix(&i2v_issues);
        let a2v_issues = check_mode_resources(&A2V, &empty);
        assert!(a2v_issues.iter().any(|i| i.message == "LTX 2.5 音频生视频缺少 EmptyImage"));
        assert_no_duplicate_product_prefix(&a2v_issues);
        let mut info = json!({"EmptyImage":{"input":{"required":{}}}});
        // include every class from all graphs with empty required so only missing filenames are reported
        for mode in &MODES {
            let graph: Value = serde_json::from_str(mode.graph).unwrap();
            for node in graph.as_object().unwrap().values() {
                let class = node["class_type"].as_str().unwrap();
                if info.get(class).is_none() {
                    info[class] = json!({"input":{"required":{}}});
                }
                if let Some(inputs) = node.get("inputs").and_then(Value::as_object) {
                    for (field, value) in inputs {
                        if let Some(name) = value.as_str() {
                            if !is_upload_field(class, field) {
                                let options = info[class]["input"]["required"][field].get(0).cloned().unwrap_or(json!([]));
                                let mut names = options.as_array().cloned().unwrap_or_default();
                                if !names.iter().any(|item| item == name) {
                                    names.push(json!(name));
                                }
                                info[class]["input"]["required"][field] = json!([names, {}]);
                            }
                        }
                    }
                }
            }
        }
        assert!(check_mode_resources(&T2V, &info).is_empty());
        assert!(check_mode_resources(&I2V, &info).is_empty());
        // drop one A2V weight and skip LoadAudio.audio
        info["UNETLoader"]["input"]["required"]["unet_name"] = json!([["other.safetensors"], {}]);
        let missing = check_mode_resources(&A2V, &info);
        assert!(missing.iter().any(|i| i.message == "LTX 2.5 音频生视频缺少 ltx-2.5-22b-distilled-transformer-comfy-int8-convrot.safetensors"));
        assert!(!missing.iter().any(|i| i.message.contains("example.png")));
        assert_no_duplicate_product_prefix(&missing);
    }

    #[test]
    fn save_video_dynamic_combo_is_not_reported_as_missing() {
        let info = declared_model_info();
        for mode in &MODES {
            let issues = check_mode_resources(mode, &info);
            assert!(issues.is_empty(), "{} unexpected issues: {:?}", mode.id, issues.iter().map(|i| i.message.as_str()).collect::<Vec<_>>());
        }
        let options = combo_options(&info["SaveVideo"]["input"]["required"]["format"]).unwrap();
        assert!(options.iter().any(|item| combo_option_matches(item, "auto")));
        assert!(options.iter().all(|item| item.as_str().is_none()));
    }

    #[test]
    fn parameter_enums_are_not_treated_as_resources() {
        let mut info = declared_model_info();
        // Deliberately omit "auto" / sampler values so a naive combo scan would false-positive.
        info["SaveVideo"]["input"]["required"]["format"] = json!([["mp4"], {}]);
        info["SaveVideo"]["input"]["required"]["codec"] = json!([["h264"], {}]);
        info["KSamplerSelect"]["input"]["required"]["sampler_name"] = json!([["euler"], {}]);
        info["CLIPLoader"]["input"]["required"]["device"] = json!([["cpu"], {}]);
        info["UNETLoader"]["input"]["required"]["weight_dtype"] = json!([["fp8_e4m3fn"], {}]);
        for mode in &MODES {
            let issues = check_mode_resources(mode, &info);
            assert!(issues.is_empty(), "{} treated a parameter enum as a missing resource: {:?}", mode.id, issues.iter().map(|i| i.message.as_str()).collect::<Vec<_>>());
        }
    }

    #[test]
    fn string_combo_still_detects_missing_model_files() {
        let mut info = declared_model_info();
        info["UNETLoader"]["input"]["required"]["unet_name"] = json!([["other.safetensors"], {}]);
        let t2v = check_mode_resources(&T2V, &info);
        let i2v = check_mode_resources(&I2V, &info);
        let a2v = check_mode_resources(&A2V, &info);
        assert!(t2v.iter().any(|i| i.message == "LTX 2.5 文生视频缺少 ltx-2.5-22b-distilled-transformer-comfy-int8-convrot.safetensors"));
        assert!(i2v.iter().any(|i| i.message == "LTX 2.5 图生视频缺少 ltx-2.5-22b-distilled-transformer-comfy-int8-convrot.safetensors"));
        assert!(a2v.iter().any(|i| i.message == "LTX 2.5 音频生视频缺少 ltx-2.5-22b-distilled-transformer-comfy-int8-convrot.safetensors"));
        assert_no_duplicate_product_prefix(&t2v);
        assert_no_duplicate_product_prefix(&i2v);
        assert_no_duplicate_product_prefix(&a2v);
        // Traditional string options still match when the file is present.
        info["UNETLoader"]["input"]["required"]["unet_name"] = json!([
            ["ltx-2.5-22b-distilled-transformer-comfy-int8-convrot.safetensors"],
            {}
        ]);
        assert!(check_mode_resources(&T2V, &info).is_empty());
        assert!(check_mode_resources(&I2V, &info).is_empty());
        assert!(check_mode_resources(&A2V, &info).is_empty());
    }

    #[test]
    fn resource_messages_use_mode_label_once() {
        let failure = Fault::resource(A2V.label, "auto");
        assert_eq!(failure.code, "RESOURCE_UNAVAILABLE");
        assert_eq!(failure.message, "LTX 2.5 音频生视频缺少 auto");
        assert!(!failure.message.contains("LTX 2.5 LTX 2.5"));
        let missing = check_mode_resources(&A2V, &json!({}));
        assert!(missing.iter().any(|i| i.message == "LTX 2.5 音频生视频缺少 EmptyImage"));
        assert_no_duplicate_product_prefix(&missing);
    }

    #[test]
    fn query_success_returns_description_only() {
        let data = succeeded_data("media-99");
        assert_eq!(data, json!({"description":"media-99"}));
        assert_eq!(data.as_object().unwrap().len(), 1);
    }

    #[test]
    fn negative_unrecorded_preserves_frozen_graph_exactly_all_modes() {
        for mode in &MODES {
            let frozen: Value = serde_json::from_str(mode.graph).unwrap();
            let mut actual = frozen.clone();
            apply_negative_prompt(mode, &mut actual, None).unwrap();
            assert_eq!(actual, frozen, "{}", mode.id);
            assert!(actual.get(NAG_NODE).is_none());
        }
    }
    #[test]
    fn negative_clear_changes_only_negative_text_all_modes() {
        for mode in &MODES {
            let frozen: Value = serde_json::from_str(mode.graph).unwrap();
            let ports = negative_ports(mode).unwrap();
            let mut expected = frozen.clone();
            expected[ports.text.node]["inputs"][ports.text.field] = json!("");
            for text in ["", " \t\n", "\u{3000}"] {
                let mut actual = frozen.clone();
                apply_negative_prompt(mode, &mut actual, Some(text)).unwrap();
                assert_eq!(actual, expected, "{}", mode.id);
                assert!(actual.get(NAG_NODE).is_none());
            }
        }
    }
    #[test]
    fn negative_filled_wires_verified_nag_video_contract_all_modes() {
        for mode in &MODES {
            let mut graph: Value = serde_json::from_str(mode.graph).unwrap();
            let ports = negative_ports(mode).unwrap();
            let text = "  subtitles, 水印  ";
            apply_negative_prompt(mode, &mut graph, Some(text)).unwrap();
            assert_eq!(graph[ports.text.node]["inputs"][ports.text.field], text);
            assert_eq!(graph[NAG_NODE]["class_type"], "LTX2_NAG");
            let inputs = &graph[NAG_NODE]["inputs"];
            assert_eq!(inputs["model"], json!([ports.model,0]));
            assert_eq!(inputs["nag_cond_video"], json!([ports.conditioning,1]));
            assert!(inputs["nag_scale"].as_f64().unwrap() > 0.0);
            assert_eq!(inputs["nag_alpha"], 0.25);
            assert_eq!(inputs["nag_tau"], 2.5);
            assert_eq!(inputs["inplace"], false);
            assert!(inputs.get("nag_cond_audio").is_none());
            for guider in ports.guiders {
                assert_eq!(graph[guider]["inputs"]["model"], json!([NAG_NODE,0]));
                assert_eq!(graph[guider]["inputs"]["negative"], json!([ports.conditioning,1]));
            }
        }
    }
    #[test]
    fn negative_fixed_seed_filled_clear_missing_no_request_leak_all_modes() {
        for mode in &MODES {
            let original = sample(mode);
            let baseline = build_graph(mode, &validate_input(mode, &original).unwrap(), &[]).unwrap();
            for negative in [Some("watermark"), Some(""), None, Some(" \n"), None] {
                let mut input = original.clone();
                if let Some(text) = negative { input["negativePrompt"] = json!(text); }
                let parsed = validate_input(mode, &input).unwrap();
                let graph = build_graph(mode, &parsed, &[]).unwrap();
                assert_eq!(graph.get(NAG_NODE).is_some(), negative.is_some_and(|s| !s.trim().is_empty()));
                if negative.is_none() { assert_eq!(graph, baseline); }
                for (patch,offset) in mode.seeds {
                    assert_eq!(graph[patch.node]["inputs"][patch.field], json!(7+offset));
                }
            }
        }
    }
    #[test]
    fn negative_invalid_type_rejected_and_missing_is_not_materialized() {
        for mode in &MODES {
            let missing = sample(mode);
            assert!(validate_input(mode, &missing).unwrap().negative_prompt.is_none());
            assert!(missing.get("negativePrompt").is_none());
            for bad in [Value::Null,json!(false),json!(4),json!([]),json!({})] {
                let mut input = missing.clone();input["negativePrompt"] = bad;
                assert_eq!(validate_input(mode, &input).unwrap_err(), Fault::NegativePromptInvalid);
            }
        }
    }
    #[test]
    fn negative_graph_drift_rejected_before_insertion_all_modes() {
        for mode in &MODES {
            let ports = negative_ports(mode).unwrap();
            let frozen: Value = serde_json::from_str(mode.graph).unwrap();
            for node in [ports.model,ports.encoder,ports.conditioning,ports.guiders[0],ports.guiders[1]] {
                let mut graph = frozen.clone();graph[node]["class_type"] = json!("NotTheExpectedClass");
                assert_eq!(apply_negative_prompt(mode, &mut graph, Some("watermark")).unwrap_err(), Fault::FrozenGraphDrift);
            }
            let mut graph = frozen.clone();graph[ports.guiders[0]]["inputs"]["model"] = json!([ports.model,1]);
            assert_eq!(apply_negative_prompt(mode, &mut graph, Some("watermark")).unwrap_err(), Fault::FrozenGraphDrift);
        }
    }


    #[test]
    fn negative_resources_are_strictly_request_conditional_all_modes() {
        let native_info = declared_model_info();
        for mode in &MODES {
            for text in [None,Some(""),Some(" \n"),Some("watermark")] {
                let mut input = sample(mode);
                if let Some(text) = text { input["negativePrompt"] = json!(text); }
                let parsed = validate_input(mode, &input).unwrap();
                let needed = text.is_some_and(|text| !text.trim().is_empty());
                assert_eq!(request_resource_classes(mode,&parsed).iter().any(|c| c=="LTX2_NAG"),needed);
                assert_eq!(!check_request_resources(mode,&parsed,&native_info).is_empty(),needed);
                let mut info = native_info.clone();
                info["LTX2_NAG"] = json!({"input":{"required":{"model":["MODEL",{}],"nag_scale":["FLOAT",{}],"nag_alpha":["FLOAT",{}],"nag_tau":["FLOAT",{}]},"optional":{"nag_cond_video":["CONDITIONING",{}],"inplace":["BOOLEAN",{}]}},"output":["MODEL"]});
                assert!(check_request_resources(mode,&parsed,&info).is_empty());
                info["LTX2_NAG"]["input"]["optional"]["nag_cond_video"] = json!(["STRING",{}]);
                assert_eq!(!check_request_resources(mode,&parsed,&info).is_empty(),needed);
            }
        }
    }
    #[test]
    fn negative_preflight_preserves_missing_clear_and_user_text_all_modes() {
        for mode in &MODES {
            for text in [None,Some(""),Some(" \n"),Some(" watermark ")] {
                let mut input = sample(mode);
                if let Some(text) = text {input["negativePrompt"] = json!(text);}
                let result = prepare_input(&json!({"selection":{"operationId":mode.operation},"input":input,"requestDigest":"test"}));
                assert_eq!(result["status"],"ready");
                let got = result["material"].get("negativePrompt");
                match text {
                    None => assert!(got.is_none()),
                    Some(text) if text.trim().is_empty() => assert_eq!(got,Some(&json!(""))),
                    Some(text) => assert_eq!(got,Some(&json!(text))),
                }
            }
        }
    }


    #[test]
    #[ignore = "writes request-local smoke fixtures only when BGX_P0_SMOKE_DIR is set"]
    fn export_negative_smoke_graphs() {
        let root=std::path::PathBuf::from(std::env::var("BGX_P0_SMOKE_DIR").expect("explicit isolated evidence directory required"));
        std::fs::create_dir_all(&root).unwrap();
        let image=std::env::var("BGX_P0_IMAGE_REF").expect("synthetic image upload reference required");
        let audio=std::env::var("BGX_P0_AUDIO_REF").expect("synthetic audio upload reference required");
        for mode in &MODES {
            for (state,text) in [("native",None),("filled",Some("watermark, subtitles")),("clear",Some("")),("unrecorded",None)] {
                let mut input=sample(mode);input["width"]=json!(256);input["height"]=json!(256);
                input["durationSeconds"]=json!(1);input["fps"]=json!(24);input["seed"]=json!(7);
                if mode.image.is_some() {input["image"]=json!([{"mediaId":"img-1"}]);}
                if let Some(text)=text {input["negativePrompt"]=json!(text);}
                let parsed=validate_input(mode,&input).unwrap();let mut files=vec![];
                if let Some(port)=mode.image {files.push((port.load,image.as_str()));}
                if let Some(port)=mode.audio {files.push((port.load,audio.as_str()));}
                let mut graph=build_graph(mode,&parsed,&files).unwrap();
                graph[mode.save]["inputs"]["filename_prefix"]=json!(format!("video/bgx_custom_nodes_p0/{0}_{state}",mode.id));
                std::fs::write(root.join(format!("{0}_{state}.json",mode.id)),serde_json::to_vec_pretty(&graph).unwrap()).unwrap();
            }
        }
    }

}
