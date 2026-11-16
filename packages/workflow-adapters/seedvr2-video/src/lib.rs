#![cfg_attr(target_arch = "wasm32", no_std)]
//! One source video -> one SeedVR2 video. All node, model and HTTP semantics stay here.
extern crate alloc;

mod comfy;

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use bgx_adapter_v2_sdk::{guest, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PLUGIN: &str = "local.bgx.seedvr2-video";
const OPERATION: &str = "local.bgx.seedvr2-video/upscale";
const OPERATION_VERSION: u32 = 2;
const MODEL: &str = "seedvr2-video";
const INPUT: &str = include_str!("../schemas/input.schema.json");
const CONFIG: &str = include_str!("../schemas/config.schema.json");
const RESUME: &str = include_str!("../schemas/resume.schema.json");
const OUTPUT: &str = include_str!("../schemas/output.schema.json");

#[derive(Debug)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
    pub vendor: Option<String>,
}
impl Failure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            vendor: None,
        }
    }
}
fn invalid(message: impl Into<String>) -> Failure {
    Failure::new("INVALID_INPUT", message)
}
fn schema(id: &str, source: &str) -> Value {
    let source = source.replace("\r\n", "\n");
    let digest: String = Sha256::digest(source.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    json!({"id":id,"digest":digest,"schemaJson":source})
}

fn controls() -> Vec<Value> {
    let mut controls = vec![
        json!({"fieldPointer":"/source","widget":"media","label":"源视频","description":"","group":"输入"}),
    ];
    controls.extend([
        ("targetResolution", "integer", "目标短边"),
        ("maxResolution", "integer", "最长边上限"),
        ("batchSize", "integer", "每批帧数"),
        ("uniformBatchSize", "toggle", "固定每批长度"),
        ("temporalOverlap", "integer", "批次重叠帧数"),
        ("prependFrames", "integer", "前置帧数"),
        ("seed", "integer", "随机种子"),
        ("colorCorrection", "select", "颜色校正"),
        ("enhancementModel", "select", "增强模型"),
        ("inputNoiseScale", "number", "输入噪声"),
        ("latentNoiseScale", "number", "潜空间噪声"),
        ("encodeTiled", "toggle", "编码分块"),
        ("decodeTiled", "toggle", "解码分块"),
        ("encodeTileSize", "integer", "编码块大小"),
        ("decodeTileSize", "integer", "解码块大小"),
        ("encodeTileOverlap", "integer", "编码块重叠"),
        ("decodeTileOverlap", "integer", "解码块重叠"),
        ("swapIoComponents", "toggle", "换出 I/O 组件"),
        ("blocksToSwap", "integer", "换出块数"),
        ("offloadDit", "toggle", "卸载 DiT"),
        ("offloadVae", "toggle", "卸载 VAE"),
        ("vaeModel", "select", "VAE 模型"),
    ].into_iter().map(|(name,widget,label)| {
        json!({"fieldPointer":format!("/parameters/{name}"),"widget":widget,"label":label,"description":"","group":"参数"})
    }));
    controls
}

pub fn capabilities() -> Value {
    let input = schema("input", INPUT);
    let config = schema("config", CONFIG);
    let resume = schema("resume", RESUME);
    let output = schema("output", OUTPUT);
    json!({"schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),"capabilityContracts":[{
        "schemaVersion":2,"localId":"single-video","capabilityKey":"local.bgx.seedvr2-video/upscale","transportSlot":"videoUpscale",
        "modelIds":[MODEL],"configurationSchema":{"id":"config","digest":config["digest"]},
        "operations":[{"id":OPERATION,"version":OPERATION_VERSION,"label":"SeedVR2 视频放大","inputProfile":"operation-input-v1",
            "inputSchema":{"id":"input","digest":input["digest"]},"outputSchema":{"id":"output","digest":output["digest"]},
            "resumeSchema":{"id":"resume","digest":resume["digest"]},
            "uiSchema":{"controls":controls()},
            "mediaSlots":[{"role":"source","inputPointer":"/source","mediaKind":"video","minCount":1,"maxCount":1}],
            "outputSlots":[{"role":"video","kind":"video","minCount":1,"maxCount":1}],
            "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported","resumable":true,"remoteIdempotency":false}}],
        "schemas":[input,config,resume,output]
    }]})
}

/// Maps every supported UI value to this frozen graph. The SDK validates scalar schema first;
/// cross-field executable restrictions (tiling) are checked before any upload.
fn graph(input: &Value) -> Result<Value, (&'static str, &'static str)> {
    let values = input["parameters"]
        .as_object()
        .ok_or(("/parameters", "视频放大参数必须为对象"))?;
    let schema: Value = serde_json::from_str(INPUT).expect("checked input schema");
    let properties = schema["properties"]["parameters"]["properties"]
        .as_object()
        .expect("parameter properties");
    if values.keys().any(|k| !properties.contains_key(k)) {
        return Err(("/parameters", "视频放大参数含未声明字段"));
    }
    let get = |name: &str| {
        values
            .get(name)
            .unwrap_or(&properties[name]["default"])
            .clone()
    };
    let batch = get("batchSize")
        .as_u64()
        .filter(|size| *size % 4 == 1)
        .ok_or(("/parameters/batchSize", "每批帧数必须为 4n+1"))?;
    if get("temporalOverlap")
        .as_u64()
        .is_none_or(|overlap| overlap >= batch)
    {
        return Err((
            "/parameters/temporalOverlap",
            "批次重叠帧数必须小于每批帧数",
        ));
    }
    for (enabled, size, overlap, pointer) in [
        (
            "encodeTiled",
            "encodeTileSize",
            "encodeTileOverlap",
            "/parameters/encodeTileOverlap",
        ),
        (
            "decodeTiled",
            "decodeTileSize",
            "decodeTileOverlap",
            "/parameters/decodeTileOverlap",
        ),
    ] {
        if get(enabled) == true
            && get(overlap)
                .as_u64()
                .zip(get(size).as_u64())
                .is_none_or(|(o, s)| o >= s)
        {
            return Err((pointer, "分块重叠必须小于块大小"));
        }
    }
    let dit = match get("enhancementModel").as_str() {
        Some("3b_fp16") => "seedvr2_ema_3b_fp16.safetensors",
        Some("7b_fp16") => "seedvr2_ema_7b_fp16.safetensors",
        Some("7b_fp8_mixed") => "seedvr2_ema_7b_fp8_e4m3fn_mixed_block35_fp16.safetensors",
        Some("7b_sharp_fp8_mixed") => {
            "seedvr2_ema_7b_sharp_fp8_e4m3fn_mixed_block35_fp16.safetensors"
        }
        _ => return Err(("/parameters/enhancementModel", "未知的增强模型")),
    };
    let mut graph: Value =
        serde_json::from_str(include_str!("workflow-api.json")).expect("frozen API graph");
    graph["21"]["inputs"]
        .as_object_mut()
        .expect("LoadVideo inputs")
        .remove("file");
    for (name, target) in [
        ("targetResolution", "resolution"),
        ("maxResolution", "max_resolution"),
        ("seed", "seed"),
        ("colorCorrection", "color_correction"),
        ("inputNoiseScale", "input_noise_scale"),
        ("latentNoiseScale", "latent_noise_scale"),
    ] {
        graph["10"]["inputs"][target] = get(name);
    }
    for (name, target) in [
        ("batchSize", "batch_size"),
        ("uniformBatchSize", "uniform_batch_size"),
        ("temporalOverlap", "temporal_overlap"),
        ("prependFrames", "prepend_frames"),
    ] {
        graph["10"]["inputs"][target] = get(name);
    }
    graph["14"]["inputs"]["model"] = json!(dit);
    graph["14"]["inputs"]["blocks_to_swap"] = get("blocksToSwap");
    graph["14"]["inputs"]["swap_io_components"] = get("swapIoComponents");
    graph["14"]["inputs"]["offload_device"] = json!(if get("offloadDit") == true {
        "cpu"
    } else {
        "none"
    });
    graph["13"]["inputs"]["offload_device"] = json!(if get("offloadVae") == true {
        "cpu"
    } else {
        "none"
    });
    graph["13"]["inputs"]["model"] = get("vaeModel");
    for (name, target) in [
        ("encodeTiled", "encode_tiled"),
        ("encodeTileSize", "encode_tile_size"),
        ("encodeTileOverlap", "encode_tile_overlap"),
        ("decodeTiled", "decode_tiled"),
        ("decodeTileSize", "decode_tile_size"),
        ("decodeTileOverlap", "decode_tile_overlap"),
    ] {
        graph["13"]["inputs"][target] = get(name);
    }
    Ok(graph)
}
fn operation(payload: &Value) -> Result<(), Failure> {
    if payload["modelId"] != MODEL
        || payload["capabilityKey"] != OPERATION
        || payload["operationId"] != OPERATION
        || payload["operationVersion"] != OPERATION_VERSION
    {
        return Err(invalid("Unknown upscale operation"));
    }
    Ok(())
}

fn material(input: &Value) -> Result<Value, (&'static str, &'static str)> {
    let source = input["source"]["mediaId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or(("/source", "请选择一条源视频"))?;
    Ok(json!({"sourceMediaId":source,"graph":graph(input)?}))
}

fn prepare(payload: &Value) -> Value {
    let prepared = if operation(&payload["selection"]).is_err() {
        Err(("", "视频放大操作身份不匹配"))
    } else if payload["environment"]["ready"] != true {
        Err(("", "视频放大的 ComfyUI 连接尚未就绪"))
    } else {
        material(&payload["input"])
    };
    match prepared {
        Ok(material) => {
            json!({"kind":"operation_input_preflight","status":"ready","requestDigest":payload["requestDigest"],"material":material})
        }
        Err((pointer, message)) => {
            json!({"kind":"operation_input_preflight","status":"rejected","requestDigest":payload["requestDigest"],
            "diagnostics":[{"code":"local.bgx.seedvr2-video/invalid_input","fieldPointer":pointer,"message":message}]})
        }
    }
}

fn submit(host: &mut dyn Host, request: &Invocation, payload: &Value) -> Result<Value, Failure> {
    operation(payload)?;
    let prepared = material(&payload["input"]).map_err(|(_, message)| invalid(message))?;
    if payload["preparation"]["material"] != prepared {
        return Err(invalid("视频放大提交与冻结预检不一致"));
    }
    let source_id = &prepared["sourceMediaId"];
    let media = payload["media"]
        .as_array()
        .filter(|m| m.len() == 1)
        .and_then(|m| m.first())
        .filter(|m| &m["id"] == source_id && m["kind"] == "video")
        .ok_or_else(|| invalid("Exactly one original video lease is required"))?;
    let stat = comfy::port(
        host,
        "host-media.info",
        json!({"handle":comfy::text(media,"handle")?}),
    )?;
    let source = comfy::video_measurement(&stat)
        .ok_or_else(|| invalid("SDK 无法从源视频原件读出分辨率和时长"))?;
    let context = comfy::Ctx {
        config: &payload["configuration"],
        budget_ms: request.r#budget_ms,
    };
    let uploaded = comfy::upload(host, &context, media)?;
    let mut graph = prepared["graph"].clone();
    graph["21"]["inputs"]["file"] = json!(uploaded);
    let remote = comfy::submit(host, &context, &graph, &request.r#request_id)?;
    let accepted = json!({"kind":"accepted","vendorTaskId":remote,"resumeToken":{"promptId":remote,
        "source":source},"appliedParams":{},"usage":null});
    persist_accepted(host, request, &accepted)?;
    Ok(accepted)
}
fn persist_accepted(
    host: &mut dyn Host,
    request: &Invocation,
    accepted: &Value,
) -> Result<(), Failure> {
    let unknown = || Failure {
        code: "ACCEPTANCE_UNKNOWN",
        message: "Remote accepted; checkpoint failed. Do not resubmit.".into(),
        vendor: Some("CHECKPOINT_COMMIT_FAILED".into()),
    };
    let read = comfy::port(
        host,
        "host-checkpoint.read",
        json!({"requestId":request.r#request_id}),
    )
    .map_err(|_| unknown())?;
    let mut checkpoint: Value =
        serde_json::from_str(comfy::text(&read, "valueJson").map_err(|_| unknown())?)
            .map_err(|_| unknown())?;
    let old = comfy::text(&read, "revision").map_err(|_| unknown())?;
    let revision = old
        .parse::<u64>()
        .ok()
        .and_then(|r| r.checked_add(1))
        .ok_or_else(unknown)?;
    checkpoint["revision"] = json!(revision.to_string());
    checkpoint["state"] = json!("accepted");
    checkpoint["vendorTaskId"] = accepted["vendorTaskId"].clone();
    checkpoint["resumeToken"] = accepted["resumeToken"].clone();
    checkpoint["result"] = accepted.clone();
    comfy::port(host,"host-checkpoint.write",json!({"requestId":request.r#request_id,"expectedRevision":old,"valueJson":checkpoint.to_string()})).map_err(|_|unknown())?;
    Ok(())
}
fn query(host: &mut dyn Host, request: &Invocation, payload: &Value) -> Result<Value, Failure> {
    operation(payload)?;
    let context = comfy::Ctx {
        config: &payload["configuration"],
        budget_ms: request.r#budget_ms,
    };
    let id = comfy::text(payload, "vendorTaskId")?;
    if payload["resumeToken"]["promptId"] != id {
        return Err(invalid("Resume token must name the original task"));
    }
    let (status, outputs, data, failure) = match comfy::status(host, &context, id, "23")? {
        comfy::Remote::Queued => ("queued", json!([]), Value::Null, Value::Null),
        comfy::Remote::Running => ("running", json!([]), Value::Null, Value::Null),
        comfy::Remote::Succeeded(files) => {
            let result = comfy::download(host, &context, &files[0])?;
            let data = json!({"promptId":id,"source":payload["resumeToken"]["source"],
                "result":result.measurement});
            ("succeeded", json!([result.output]), data, Value::Null)
        }
        comfy::Remote::Failed(vendor) => (
            "failed",
            json!([]),
            Value::Null,
            json!({"category":"vendorTaskFailed","message":"SeedVR2 execution failed","vendorCode":vendor,"details":{"promptId":id}}),
        ),
        comfy::Remote::Lost => (
            "failed",
            json!([]),
            Value::Null,
            json!({"category":"vendorTaskFailed","message":"Original prompt is absent from the queue and history; it was not resubmitted","vendorCode":"PROMPT_NOT_FOUND","details":{"promptId":id}}),
        ),
    };
    let progress = if matches!(status, "queued" | "running") {
        json!({"fraction":null,"message":status})
    } else {
        Value::Null
    };
    Ok(
        json!({"status":status,"outputs":outputs,"data":data,"usage":null,"progress":progress,"terminalFailure":failure,"resumeToken":payload["resumeToken"]}),
    )
}
pub struct Seedvr2Video;
impl Adapter for Seedvr2Video {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        let invocation = serde_json::to_value(&request).expect("typed invocation");
        let result = (|| {
            let payload: Value = serde_json::from_str(&request.r#payload_json)
                .map_err(|_| invalid("Payload is not JSON"))?;
            match invocation["operation"].as_str() {
                Some("capabilities") => Ok(capabilities()),
                Some("submit") => submit(host, &request, &payload),
                Some("query") => query(host, &request, &payload),
                Some("upload") => {
                    let context = comfy::Ctx {
                        config: &payload["configuration"],
                        budget_ms: request.r#budget_ms,
                    };
                    let reference = comfy::upload(host, &context, &payload["media"])?;
                    Ok(json!({"mediaId":payload["media"]["id"],"uploadRef":reference}))
                }
                Some("probe") => {
                    let checked = match payload["kind"].as_str() {
                        Some("operation_input_preflight") => return Ok(prepare(&payload)),
                        Some("package_validation") => false,
                        Some("connectivity") => {
                            comfy::status_probe(
                                host,
                                &comfy::Ctx {
                                    config: &payload["configuration"],
                                    budget_ms: request.r#budget_ms,
                                },
                            )?;
                            true
                        }
                        _ => return Err(invalid("Unknown probe kind")),
                    };
                    Ok(
                        json!({"packageValid":true,"connection":if checked {"ready"}else{"not_checked"},"authentication":"not_checked","resources":"not_checked","issues":[]}),
                    )
                }
                _ => Err(Failure::new(
                    "UNSUPPORTED_FEATURE",
                    "Transport operation is unsupported",
                )),
            }
        })();
        match result {
            Ok(value) => guest::completed_response(&request,&value).expect("serializable public result"),
            Err(error) => serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,"error":{
                "code":error.code,"message":error.message,"retryable":false,"vendorCode":error.vendor,"vendorRequestId":null}})).expect("public error"),
        }
    }
}
bgx_adapter_v2_sdk::export_adapter!(Seedvr2Video);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_graph_preserves_source_and_projects_confirmed_parameters() {
        let prepared = material(&json!({"source":{"mediaId":"source-video"},"parameters":{"targetResolution":512,"maxResolution":1024,"batchSize":5,"seed":17,"enhancementModel":"7b_fp8_mixed",
            "colorCorrection":"none","encodeTiled":true,"encodeTileSize":512,"encodeTileOverlap":64,"decodeTiled":true,"decodeTileSize":256,"decodeTileOverlap":32,
            "blocksToSwap":12,"swapIoComponents":true,"offloadDit":true,"offloadVae":true,"inputNoiseScale":0.2,"latentNoiseScale":0.4}})).unwrap();
        assert_eq!(prepared["sourceMediaId"], "source-video");
        let graph = &prepared["graph"];
        assert!(graph["21"]["inputs"].get("file").is_none());
        assert_eq!(graph["10"]["inputs"]["resolution"], 512);
        assert_eq!(graph["10"]["inputs"]["max_resolution"], 1024);
        assert_eq!(graph["10"]["inputs"]["batch_size"], 5);
        assert_eq!(graph["10"]["inputs"]["image"], json!(["22", 0]));
        assert_eq!(graph["10"]["inputs"]["seed"], 17);
        assert_eq!(graph["10"]["inputs"]["input_noise_scale"], 0.2);
        assert_eq!(
            graph["14"]["inputs"]["model"],
            "seedvr2_ema_7b_fp8_e4m3fn_mixed_block35_fp16.safetensors"
        );
        assert_eq!(graph["14"]["inputs"]["blocks_to_swap"], 12);
        assert_eq!(graph["14"]["inputs"]["swap_io_components"], true);
        assert_eq!(graph["14"]["inputs"]["offload_device"], "cpu");
        assert_eq!(graph["13"]["inputs"]["offload_device"], "cpu");
        assert_eq!(graph["13"]["inputs"]["decode_tile_overlap"], 32);
        assert_eq!(graph["23"]["inputs"]["video"], json!(["24", 0]));
        assert_eq!(graph["24"]["inputs"]["images"], json!(["10", 0]));
        assert_eq!(graph["24"]["inputs"]["audio"], json!(["22", 1]));
        assert_eq!(graph["24"]["inputs"]["fps"], json!(["22", 2]));
        assert!(graph
            .as_object()
            .unwrap()
            .values()
            .any(|n| n["class_type"] == "LoadVideo"));
    }
    #[test]
    fn rejects_unconsumed_parameters_and_invalid_tile_pair() {
        assert!(graph(&json!({"parameters":{"writerCrf":18}})).is_err());
        let cases = [
            (json!({"batchSize":6}), "/parameters/batchSize"),
            (
                json!({"batchSize":5,"temporalOverlap":5}),
                "/parameters/temporalOverlap",
            ),
            (
                json!({"encodeTiled":true,"encodeTileSize":64,"encodeTileOverlap":64}),
                "/parameters/encodeTileOverlap",
            ),
        ];
        for (parameters, pointer) in cases {
            let response = prepare(
                &json!({"environment":{"ready":true},"selection":{"modelId":MODEL,
                "capabilityKey":OPERATION,"operationId":OPERATION,"operationVersion":OPERATION_VERSION},
                "input":{"source":{"mediaId":"source-video"},"parameters":parameters}}),
            );
            assert_eq!(response["status"], "rejected");
            assert_eq!(response["diagnostics"][0]["fieldPointer"], pointer);
        }
    }
    #[test]
    fn one_video_operation_is_async_resumable_and_has_matching_schema_digests() {
        let caps = capabilities();
        let contract = &caps["capabilityContracts"][0];
        assert_eq!(contract["capabilityKey"], OPERATION);
        assert_eq!(contract["transportSlot"], "videoUpscale");
        assert_eq!(contract["operations"][0]["id"], OPERATION);
        assert_eq!(contract["operations"][0]["version"], OPERATION_VERSION);
        assert_eq!(
            contract["operations"][0]["inputProfile"],
            "operation-input-v1"
        );
        assert_eq!(
            contract["operations"][0]["mediaSlots"][0]["mediaKind"],
            "video"
        );
        assert_eq!(contract["operations"][0]["outputSlots"][0]["kind"], "video");
        assert_eq!(
            contract["operations"][0]["execution"]["mode"],
            "asynchronous"
        );
        assert_eq!(
            contract["operations"][0]["execution"]["cancellation"],
            "unsupported"
        );
        assert_eq!(contract["operations"][0]["execution"]["resumable"], true);
        for value in contract["schemas"].as_array().unwrap() {
            let source = value["schemaJson"].as_str().unwrap();
            let _: Value = serde_json::from_str(source).unwrap();
            assert_eq!(
                value["digest"],
                schema(value["id"].as_str().unwrap(), source)["digest"]
            );
        }
    }
}
