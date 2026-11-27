#![cfg_attr(target_arch = "wasm32", no_std)]
extern crate alloc;
use alloc::{borrow::ToOwned, format, string::{String, ToString}, vec, vec::Vec};
use bgx_adapter_v2_sdk::{guest, host, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// no_std-safe nearest-int for timestamp ms (wasm guest has no std float math).
fn round_ms(x: f64) -> i64 {
    if x >= 0.0 {
        (x + 0.5) as i64
    } else {
        (x - 0.5) as i64
    }
}

pub const PLUGIN: &str = "local.thirdparty.h3-face-refine";
const OP_TRAJECTORY: &str = "local.thirdparty.h3-face-refine/subjects.trajectory";
const OP_SEED: &str = "local.thirdparty.h3-face-refine/subjects.seed";
pub const OP_ANALYZE: &str = "local.thirdparty.h3-face-refine/subjects.analyze";
pub const OP_PREVIEW: &str = "local.thirdparty.h3-face-refine/subjects.preview";
pub const OP_REFINE: &str = "local.thirdparty.h3-face-refine/video.refine";
pub const MODEL: &str = "h3-face-refine-1.1.2";
pub const UPSTREAM_COMMIT: &str = "d8521d14fe0d721d80cd9417fff5a559cbc21aba";

type Result<T> = core::result::Result<T, Failure>;

#[derive(Debug)]
struct Failure {
    code: &'static str,
    message: String,
    retryable: bool,
    service_rejected: bool,
}

fn fail(code: &'static str, message: impl Into<String>) -> Failure {
    Failure {
        code,
        message: message.into(),
        retryable: false,
        service_rejected: false,
    }
}

fn invalid(s: &str) -> Failure {
    fail("INVALID_INPUT", s)
}

const REFINE_INPUT_SCHEMA: &str = include_str!("../schemas/refine.input.schema.json");

fn text_digest(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn schema(id: &str, text: &str) -> Value {
    let text = text.replace("\r\n", "\n");
    json!({"id": id, "digest": text_digest(&text), "schemaJson": text})
}

fn read_only_schema(id: &str, text: &str) -> Value {
    let mut doc: Value = serde_json::from_str(text).expect("embedded profile schema");
    let (base, file) = doc["$id"].as_str().and_then(|uri| uri.rsplit_once('/'))
        .expect("embedded profile schema URI");
    let uri = format!("{base}/read-only-{file}");
    doc["$id"] = json!(uri);
    doc["readOnly"] = json!(true);
    schema(id, &doc.to_string())
}

fn port(h: &mut dyn Host, method: &str, args: Value) -> Result<Value> {
    host::call(h, method, &args).map_err(|e| {
        let v = serde_json::to_value(&e).expect("public error");
        let code = match v["code"].as_str() {
            Some("PERMISSION_DENIED") => "PERMISSION_DENIED",
            Some("TIMEOUT") => "TIMEOUT",
            Some("BUDGET_EXCEEDED") => "BUDGET_EXCEEDED",
            _ => "RESOURCE_UNAVAILABLE",
        };
        fail(code, format!("managed port {method} failed: {}", host::diagnostic(&e)))
    })
}

fn cfg_base(configuration: &Value) -> Result<&str> {
    configuration
        .get("endpointBaseUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("configuration.endpointBaseUrl is required"))
}

mod response_diagnostic;

fn http_body(h: &mut dyn Host, method: &str, url: &str, body: Value) -> Result<Value> {
    let headers = if body["kind"] == "bytes" { json!([{"name":"Content-Type","value":"application/json"}]) } else { json!([]) };
    let response = port(h, "host-http.execute", json!({"plan":{"method":method,"url":url,"headers":headers,"generatedHeaders":[],"body":body,"sink":{"kind":"buffer","maxBytes":1048576},"timeoutMs":600000},"credential":null}))?;
    let status = response["status"].as_u64().ok_or_else(|| fail("UPSTREAM_ERROR", "HTTP response missing status"))?;

    let bytes: Vec<u8> = serde_json::from_value(response["payload"]["bytes"].clone()).map_err(|_| fail("UPSTREAM_ERROR", "HTTP response missing buffered bytes"))?;
    // The HTTP router did not dispatch this request. Its body may be plain text;
    // do not lose the explicit rejection by trying to decode JSON first.
    if matches!(status, 404 | 405) {
        let mut failure = fail("RESOURCE_UNAVAILABLE", format!("Comfy HTTP {status}: required adapter endpoint unavailable; restart the configured ComfyUI service to load the installed nodes"));
        failure.service_rejected = true;
        return Err(failure);
    }
    let raw: Value = serde_json::from_slice(&bytes).map_err(|_| fail("UPSTREAM_ERROR", format!("Comfy HTTP {status}: response is not JSON; {}", response_diagnostic::describe(&bytes))))?;
    if !(200..300).contains(&status) {
        let kind = raw.pointer("/error/type").and_then(Value::as_str).unwrap_or("unclassified");
        let rejected = status == 400 && matches!(kind, "prompt_outputs_failed_validation" | "prompt_no_outputs" | "invalid_prompt");
        let mut failure = fail(if rejected { "INVALID_INPUT" } else { "UPSTREAM_ERROR" }, format!("Comfy HTTP {status}: {kind}; {}", response_diagnostic::describe(&bytes)));
        failure.service_rejected = rejected;
        return Err(failure);
    }
    Ok(raw)
}
fn http_json(h: &mut dyn Host, method: &str, url: &str, body: Option<Value>) -> Result<Value> {
    let body = match body {
        Some(v) => json!({"kind":"bytes","bytes":serde_json::to_vec(&v).map_err(|_| invalid("invalid JSON body"))?}),
        None => json!({"kind":"none"}),
    };
    http_body(h, method, url, body)
}
fn upload_source(h: &mut dyn Host, payload: &Value, base: &str, id: &str, kind: &str) -> Result<String> {
    let media = payload["media"].as_array().ok_or_else(|| invalid("missing managed media"))?;
    let matching: Vec<_> = media.iter().filter(|m| m["id"].as_str() == Some(id)).collect();
    if matching.len() != 1 { return Err(invalid("source media identity is missing or ambiguous")); }
    let m = matching[0];
    if m["kind"] != kind { return Err(invalid("managed media kind does not match slot")); }
    let stat = port(h, "host-media.info", json!({"handle":m["handle"]}))?;
    for key in ["kind", "mime", "fileName", "byteLength"] {
        if stat[key] != m[key] { return Err(invalid("managed media differs from frozen descriptor")); }
    }
    let uploaded = http_body(h, "POST", &format!("{}/upload/image", base.trim_end_matches('/')), json!({"kind":"multipart","parts":[{"kind":"file","name":"image","fileName":m["fileName"],"handle":m["handle"],"mime":m["mime"]}]}))?;
    let name = uploaded["name"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| fail("UPSTREAM_ERROR", "upload response missing name"))?;
    let subfolder = uploaded["subfolder"].as_str().unwrap_or("");
    Ok(if subfolder.is_empty() { name.to_owned() } else { format!("{subfolder}/{name}") })
}

pub fn capabilities() -> Value {
    let trajectory_in = schema("trajectory_in", include_str!("../schemas/trajectory.input.schema.json"));
    let trajectory_out = schema("trajectory_out", include_str!("../schemas/trajectory.output.schema.json"));
    let seed_in = schema("seed_in", include_str!("../schemas/seed.input.schema.json"));
    let seed_out = schema("seed_out", include_str!("../schemas/seed.output.schema.json"));
    let analyze_in = schema("analyze_in", include_str!("../schemas/analyze.input.schema.json"));
    let analyze_out = schema("analyze_out", include_str!("../schemas/analyze.output.schema.json"));
    let preview_in = schema("preview_in", include_str!("../schemas/preview.input.schema.json"));
    let preview_out = schema("preview_out", include_str!("../schemas/preview.output.schema.json"));
    let refine_in = schema("refine_in", REFINE_INPUT_SCHEMA);
    let refine_out = schema("refine_out", include_str!("../schemas/refine.output.schema.json"));
    let config = schema("configuration", include_str!("../schemas/configuration.schema.json"));
    let resume = schema("resume", include_str!("../schemas/resume.schema.json"));
    let source_read_only = read_only_schema("source_read_only", include_str!("../schemas/media-target-selection-v1/source-identity.schema.json"));
    let analysis_read_only = read_only_schema("analysis_read_only", include_str!("../schemas/media-target-selection-v1/analysis-receipt.schema.json"));
    let selection_read_only = read_only_schema("selection_read_only", include_str!("../schemas/media-target-selection-v1/selection.schema.json"));
    let tracking_read_only = read_only_schema("tracking_read_only", include_str!("../schemas/media-target-selection-v1/tracking-receipt.schema.json"));

    let mk_op = |id: &str, label: &str, i: &Value, o: &Value, controls: Vec<Value>, media: Vec<Value>, outs: Vec<Value>, mode: &str| {
        json!({
            "id": id,
            "version": 1,
            "label": label,
            "inputProfile": "operation-input-v1",
            "inputSchema": {"id": i["id"], "digest": i["digest"]},
            "outputSchema": {"id": o["id"], "digest": o["digest"]},
            "resumeSchema": {"id": "resume", "digest": resume["digest"]},
            "uiSchema": {"controls": controls},
            "mediaSlots": media,
            "outputSlots": outs,
            "execution": {
                "mode": mode,
                "streaming": false,
                "cancellation": "best_effort",
                "resumable": true,
                "remoteIdempotency": false
            }
        })
    };

    let analyze_controls = vec![
        json!({"fieldPointer":"/detectionParams/detector","widget":"text","label":"检测器","description":"ultralytics 相对名；默认 bbox/face_yolov8m.pt","group":"检测"}),
        json!({"fieldPointer":"/detectionParams/confidence","widget":"number","label":"置信度","description":"","group":"检测"}),
        json!({"fieldPointer":"/detectionParams/cutDetection","widget":"select","label":"切镜检测","description":"none|auto；默认 auto","group":"检测"}),
        json!({"fieldPointer":"/detectionParams/cutThreshold","widget":"number","label":"切镜阈值","description":"","group":"检测"}),
        json!({"fieldPointer":"/detectionParams/selectHint","widget":"select","label":"候选编号提示","description":"仅影响编号，不代替用户选择；默认 largest_face","group":"检测"}),
        json!({"fieldPointer":"/detectionParams/force","widget":"checkbox","label":"重新检测","description":"","group":"检测"}),
    ];
    let refine_controls = vec![
        json!({"fieldPointer":"/freeze/referenceMediaIds","widget":"media","label":"外观参考图","description":"提供希望采用的外观参考","group":"素材"}),
        json!({"fieldPointer":"/freeze/userParams/identityMediaIds","widget":"media","label":"身份参考图","description":"用于确认人物身份","group":"素材"}),
        json!({"fieldPointer":"/freeze/prompt","widget":"multiline","label":"提示词","description":"用户原文","group":"精修","displayRole":"prompt"}),
        json!({"fieldPointer":"/freeze/userParams/denoise","widget":"number","label":"去噪","description":"0-1；旧对照约0.45","group":"精修"}),
        json!({"fieldPointer":"/freeze/userParams/smallFaceStrength","widget":"number","label":"小脸强度","description":"小脸使用的精修强度比例，数值越大增强越明显","group":"精修"}),
        json!({"fieldPointer":"/freeze/userParams/largeFaceStrength","widget":"number","label":"大脸强度","description":"大脸使用的精修强度比例，数值越小越保留细节","group":"精修"}),
        json!({"fieldPointer":"/freeze/userParams/samModel","widget":"text","label":"SAM模型文件名","description":"启用脸形遮罩时填写已安装的模型名称","group":"可选"}),
        json!({"fieldPointer":"/freeze/userParams/samEnabled","widget":"checkbox","label":"SAM脸形遮罩","description":"缺依赖须显式不可用，不得静默改矩形","group":"可选"}),
        json!({"fieldPointer":"/freeze/userParams/scaleMode","widget":"select","label":"脸尺寸缩放模式","description":"选择按实际像素或在本视频中的相对大小调节增强","group":"可选"}),
    ];

    json!({
        "schemaVersion": 2,
        "pluginId": PLUGIN,
        "pluginVersion": env!("CARGO_PKG_VERSION"),
        "capabilityContracts": [{
            "schemaVersion": 2,
            "localId": "face-refine",
            "capabilityKey": "video.edit",
            "transportSlot": "videoFaceRefine",
            "modelIds": [MODEL],
            "configurationSchema": {"id": "configuration", "digest": config["digest"]},
            "schemas": [trajectory_in.clone(), trajectory_out.clone(), seed_in.clone(), seed_out.clone(), analyze_in.clone(), analyze_out.clone(), preview_in.clone(), preview_out.clone(),
                refine_in.clone(), refine_out.clone(), config, resume.clone(), source_read_only,
                analysis_read_only, selection_read_only, tracking_read_only],
            "operations": [
                mk_op(OP_TRAJECTORY, "按帧范围跟踪人脸", &trajectory_in, &trajectory_out, vec![],
                    vec![
                        json!({"role":"source_video","inputPointer":"/sourceMediaId","mediaKind":"video","minCount":1,"maxCount":1}),
                        json!({"role":"identity_ref","inputPointer":"/identityMediaId","mediaKind":"image","minCount":0,"maxCount":1}),
                    ],
                    vec![json!({"role":"trajectory","kind":"file","minCount":1,"maxCount":1})], "synchronous"),
                mk_op(OP_SEED, "识别当前帧人脸", &seed_in, &seed_out, vec![],
                    vec![json!({"role":"seed_images","inputPointer":"/sampleMediaIds","mediaKind":"image","minCount":1,"maxCount":5})],
                    vec![], "synchronous"),
                mk_op(OP_ANALYZE, "分析镜头与候选脸", &analyze_in, &analyze_out, analyze_controls,
                    vec![json!({"role":"source_video","inputPointer":"/source/mediaId","mediaKind":"video","minCount":1,"maxCount":1})],
                    vec![],
                    "synchronous"),
                mk_op(OP_PREVIEW, "跟踪预览", &preview_in, &preview_out, vec![],
                    vec![
                        json!({"role":"source_video","inputPointer":"/analysis/source/mediaId","mediaKind":"video","minCount":1,"maxCount":1}),
                        json!({"role":"identity_ref","inputPointer":"/identityMediaId","mediaKind":"image","minCount":0,"maxCount":1}),
                    ],
                    vec![json!({"role":"preview","kind":"image","minCount":1,"maxCount":1})],
                    "asynchronous"),
                ({ let mut operation = mk_op(OP_REFINE, "人脸精修", &refine_in, &refine_out, refine_controls,
                    vec![
                        json!({"role":"source_video","inputPointer":"/freeze/source/mediaId","mediaKind":"video","minCount":1,"maxCount":1}),
                        json!({"role":"trajectory","inputPointer":"/trajectory/mediaId","mediaKind":"file","minCount":0,"maxCount":1}),
                        json!({"role":"appearance_ref","inputPointer":"/freeze/referenceMediaIds","mediaKind":"image","minCount":0,"maxCount":9}),
                        json!({"role":"identity_ref","inputPointer":"/freeze/userParams/identityMediaIds","mediaKind":"image","minCount":0,"maxCount":1}),
                    ],
                    vec![json!({"role":"video","kind":"video","minCount":1,"maxCount":1})],
                    "asynchronous");
                  operation["version"] = json!(2);
                  operation["uiSchema"]["dynamicOptions"] = json!([{"fieldPointer":sam::MODEL_FIELD,"enabledBy":"/freeze/userParams/samEnabled"}]);
                  operation
                })
            ]
        }]
    })
}

fn response_fail(request: &Invocation, f: Failure) -> Response {
    use bgx_adapter_v2_sdk::{InvocationOperation as I, ResponseBranch1Operation as O, ResponseBranch1ErrorCode as E, ResponseBranch1, ResponseBranch1Error, ResponseBranch1ProtocolVersion};
    let operation = match request.operation { I::V0=>O::V0,I::V1=>O::V1,I::V2=>O::V2,I::V3=>O::V3,I::V4=>O::V4,I::V5=>O::V5 };
    let code = match f.code {
        "INVALID_PACKAGE"=>E::V0,"UNSUPPORTED_VERSION"=>E::V1,"UNSUPPORTED_FEATURE"=>E::V2,"INVALID_INPUT"=>E::V3,"UNAUTHORIZED"=>E::V4,"PERMISSION_DENIED"=>E::V5,"UPSTREAM_ERROR"=>E::V6,"RATE_LIMITED"=>E::V7,"TIMEOUT"=>E::V8,"ACCEPTANCE_UNKNOWN"=>E::V9,"RESOURCE_UNAVAILABLE"|"BUDGET_EXCEEDED"=>E::V10,"CANCEL_UNSUPPORTED"=>E::V11,_=>E::V12,
    };
    Response::V1(ResponseBranch1 {protocol_version:ResponseBranch1ProtocolVersion,operation,error:ResponseBranch1Error {
        code,message:format!("{}: {}",f.code,f.message),retryable:f.retryable,vendor_code:None,vendor_request_id:None,
    }})
}

fn submit_payload(request: &Invocation) -> Result<Value> {
    serde_json::from_str(&request.r#payload_json).map_err(|_| invalid("submit payload is not JSON"))
}

fn run_trajectory(h: &mut dyn Host, payload: &Value, phase: &mut Phase) -> Result<Value> {
    let base = cfg_base(&payload["configuration"])?;
    let base = base.trim_end_matches('/');
    let health = http_json(h, "GET", &format!("{base}/bgx_face_refine/health"), None)?;
    if health["trajectoryArtifacts"] != true {
        return Err(fail("RESOURCE_UNAVAILABLE", "请重启当前配置的 ComfyUI，加载支持轨迹修正的人脸精修节点"));
    }
    let input = &payload["input"];
    let source_id = input["sourceMediaId"].as_str().ok_or_else(|| invalid("source media required"))?;
    let video = upload_source(h, payload, base, source_id, "video")?;
    let identity = input["identityMediaId"].as_str().map(|id| upload_source(h, payload, base, id, "image")).transpose()?;
    let mut options: Value = serde_json::from_str(include_str!("../workflows/preview.track.api.json"))
        .map_err(|_| invalid("invalid tracker workflow"))?;
    let mut track = options["2"]["inputs"].take();
    let settings = track.as_object_mut().ok_or_else(|| invalid("missing track settings"))?;
    for name in ["images", "face_pick", "preview"] {
        settings.remove(name);
    }
    track["confidence"] = input["confidence"].as_f64().unwrap_or(0.35).into();
    track["cut_detection"] = input["cutDetection"].as_str().unwrap_or("auto").into();
    track["cut_threshold"] = input["cutThreshold"].as_f64().unwrap_or(3.0).into();
    track["identity_threshold"] = 0.28.into();
    track["identity_model"] = "insightface".into();
    let mut body = json!({
        "video":video, "frames":input["frames"], "anchorIndex":input["anchorIndex"],
        "anchorBox":input["anchorBox"], "settings":{"track":track}
    });
    if let Some(name) = identity {
        body["identityImage"] = json!(name);
    }
    if let Some(layout) = input.get("layout") {
        body["layout"] = layout.clone();
    }
    *phase = Phase::Unknown;
    let receipt = http_json(h, "POST", &format!("{base}/bgx_face_refine/trajectory"), Some(body))?;
    let artifact = receipt["artifactId"].as_str().filter(|s| s.len() == 32 &&
        s.bytes().all(|b| b.is_ascii_hexdigit())).ok_or_else(|| fail("UPSTREAM_ERROR", "invalid trajectory receipt"))?;
    let url = format!("{base}/bgx_face_refine/trajectory/{artifact}");
    let response = port(h, "host-http.execute", json!({
        "plan":{"method":"GET","url":url,"headers":[],"generatedHeaders":[],"body":{"kind":"none"},
            "sink":{"kind":"media"},"timeoutMs":600000},"credential":null
    }))?;
    if response["status"] != 200 || response["payload"]["kind"] != "media" {
        return Err(fail("UPSTREAM_ERROR", "轨迹文件读取失败"));
    }
    let handle = response["payload"]["handle"].as_str().ok_or_else(|| fail("UPSTREAM_ERROR", "missing trajectory handle"))?;
    let stat = port(h, "host-media.info", json!({"handle":handle}))?;
    if stat["kind"] != "file" || stat["byteLength"].as_str().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0) == 0 {
        return Err(fail("UPSTREAM_ERROR", "轨迹文件为空或类型不符"));
    }
    Ok(json!({"kind":"completed","vendorTaskId":null,"appliedParams":null,"outputs":[
        {"kind":"media","role":"trajectory","media":{"id":stat["id"],"kind":stat["kind"],"handle":handle,
            "mime":stat["mime"],"fileName":stat["fileName"],"byteLength":stat["byteLength"]}}
    ],"data":{"artifactId":artifact},"usage":null}))
}

fn run_seed(h: &mut dyn Host, payload: &Value, phase: &mut Phase) -> Result<Value> {
    let base = cfg_base(&payload["configuration"])?;
    let base = base.trim_end_matches('/');
    let health = http_json(h, "GET", &format!("{base}/bgx_face_refine/health"), None)?;
    if health["seedFrames"] != true {
        return Err(fail("RESOURCE_UNAVAILABLE", "请重启当前配置的 ComfyUI，加载支持当前帧识别的人脸精修节点"));
    }
    let input = &payload["input"];
    let sample_ids = input["sampleMediaIds"].as_array().ok_or_else(|| invalid("sample ids required"))?;
    let samples = input["samples"].as_array().filter(|items| (1..=5).contains(&items.len()))
        .ok_or_else(|| invalid("expected one to five ordered sample frames"))?;
    if sample_ids.len() != samples.len() ||
        samples.iter().zip(sample_ids).any(|(sample, id)| &sample["mediaId"] != id) {
        return Err(invalid("sample descriptor does not match managed media ids"));
    }
    let mut uploaded = Vec::with_capacity(samples.len());
    for sample in samples {
        let id = sample["mediaId"].as_str().ok_or_else(|| invalid("sample id required"))?;
        let upload_name = upload_source(h, payload, base, id, "image")?;
        let mut descriptor = sample.clone();
        descriptor["uploadName"] = json!(upload_name);
        uploaded.push(descriptor);
    }
    *phase = Phase::Unknown;
    let data = http_json(h, "POST", &format!("{base}/bgx_face_refine/detect-samples"),
        Some(json!({"samples":uploaded,"confidence":input.get("confidence").and_then(Value::as_f64).unwrap_or(0.35)})))?;
    if data["kind"] != "detectReceipt" || !data["candidates"].is_array() ||
        !samples.iter().any(|sample| sample["mediaId"] == data["mediaId"] && sample["imageDigest"] == data["imageDigest"]) {
        return Err(fail("UPSTREAM_ERROR", "人脸识别回执与送检画面不一致"));
    }
    Ok(json!({"kind":"completed","vendorTaskId":null,"appliedParams":null,"outputs":[],"data":data,"usage":null}))
}

fn run_analyze(h: &mut dyn Host, payload: &Value, phase: &mut Phase) -> Result<Value> {
    let configuration = payload.get("configuration").cloned().unwrap_or(json!({}));
    let base = cfg_base(&configuration)?;
    let input = payload.get("input").ok_or_else(|| invalid("missing input"))?;
    let detection = input.get("detectionParams").cloned().or_else(|| input.get("detection").cloned()).unwrap_or(json!({}));
    let source = input.get("source").ok_or_else(|| invalid("missing source identity"))?;
    let source_id = source["mediaId"].as_str().ok_or_else(|| invalid("missing source mediaId"))?;
    // Read-only readiness precedes media upload and any analysis side effect.
    let health = http_json(h, "GET", &format!("{}/bgx_face_refine/health", base.trim_end_matches('/')), None)?;
    if health["analysisSessions"] != true || health["trackingReceipt"] != true {
        return Err(fail("RESOURCE_UNAVAILABLE", "Configured ComfyUI has not loaded the installed face refinement nodes; restart it in settings"));
    }
    let nodes = http_json(h, "GET", &format!("{}/object_info/H3FaceSelect", base.trim_end_matches('/')), None)?;
    if !nodes["H3FaceSelect"].is_object() {
        return Err(fail("RESOURCE_UNAVAILABLE", "Configured ComfyUI is missing H3FaceSelect; restart it in settings to load the installed package"));
    }
    let video_path = upload_source(h, payload, base, source_id, "video")?;

    let body = json!({
        "video": video_path,
        "detector": detection.get("detector").and_then(Value::as_str).unwrap_or("bbox/face_yolov8m.pt"),
        "confidence": detection.get("confidence").and_then(Value::as_f64).unwrap_or(0.35),
        "cut_detection": detection.get("cutDetection").and_then(Value::as_str).unwrap_or("auto"),
        "cut_threshold": detection.get("cutThreshold").and_then(Value::as_f64).unwrap_or(3.0),
        "select": detection.get("selectHint").and_then(Value::as_str).unwrap_or("largest_face"),
        "force": detection.get("force").and_then(Value::as_bool).unwrap_or(false)
    });
    let url = format!("{}/h3_facerefine/scan", base.trim_end_matches('/'));
    *phase = Phase::Unknown;
    let raw = http_json(h, "POST", &url, Some(body.clone())).map_err(|failure| {
        if failure.service_rejected { *phase = Phase::ServiceRejected; }
        failure
    })?;
    if let Some(err) = raw.get("error").and_then(Value::as_str) {
        if raw["busy"] == true {
            *phase = Phase::ServiceRejected;
            let mut failure = fail("RATE_LIMITED", "ComfyUI 正在渲染，请在队列空闲后重新分析目标");
            failure.retryable = true;
            failure.service_rejected = true;
            return Err(failure);
        }
        return Err(fail("UPSTREAM_ERROR", err));
    }
    // Map upstream shot cards → media-target-selection analysis receipt (package-local profile).
    let positive = |key: &str| raw[key].as_f64().filter(|n| n.is_finite() && *n > 0.0).ok_or_else(|| fail("UPSTREAM_ERROR", format!("scan missing valid {key}")));
    let w = positive("width")?;
    let hgt = positive("height")?;
    let fps = positive("fps")?;
    let frames = raw["frames"].as_u64().filter(|n| *n > 0).ok_or_else(|| fail("UPSTREAM_ERROR", "scan missing frames"))?;
    let mut shots = Vec::new();
    if let Some(arr) = raw.get("shots").and_then(Value::as_array) {
        for (i, shot) in arr.iter().enumerate() {
            let seg = shot.get("segment").and_then(Value::as_array);
            let a = seg.and_then(|s| s.get(0)).and_then(Value::as_f64).unwrap_or(0.0);
            let b = seg.and_then(|s| s.get(1)).and_then(Value::as_f64).unwrap_or(a);
            let start_ms = round_ms((a / fps) * 1000.0);
            let end_ms = round_ms((b / fps) * 1000.0);
            let mut candidates = Vec::new();
            if let Some(boxes) = shot.get("boxes").and_then(Value::as_array) {
                let mut ordered: Vec<_> = boxes.iter().collect();
                ordered.sort_by(|a,b| {
                    let center = |v: &Value| (v[0].as_f64().unwrap_or(0.0) + v[2].as_f64().unwrap_or(0.0)) / 2.0;
                    center(a).total_cmp(&center(b))
                });
                for (ci, bx) in ordered.into_iter().enumerate() {
                    let vals = bx.as_array();
                    let x0 = vals.and_then(|v| v.get(0)).and_then(Value::as_f64).unwrap_or(0.0) * w;
                    let y0 = vals.and_then(|v| v.get(1)).and_then(Value::as_f64).unwrap_or(0.0) * hgt;
                    let x1 = vals.and_then(|v| v.get(2)).and_then(Value::as_f64).unwrap_or(0.0) * w;
                    let y1 = vals.and_then(|v| v.get(3)).and_then(Value::as_f64).unwrap_or(0.0) * hgt;
                    candidates.push(json!({
                        "candidateId": format!("c{ci}"),
                        "box": {"x": x0, "y": y0, "width": (x1 - x0), "height": (y1 - y0)},
                        "score": null,
                        "previewMediaId": null
                    }));
                }
            }
            let fi = shot.get("frame").and_then(Value::as_f64).unwrap_or(0.0);
            let representative_frame_ms = if fi < 0.0 { start_ms } else { round_ms((fi / fps) * 1000.0) };
            shots.push(json!({
                "shotId": format!("s{i}"),
                "range": {"startMs": start_ms, "endMs": end_ms},
                "representativeFrameMs": representative_frame_ms,
                "candidates": candidates
            }));
        }
    }
    let digest_src = serde_json::to_string(&shots).unwrap_or_default();
    let digest: String = Sha256::digest(digest_src.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let analysis_id: String = {
        let mut hasher = Sha256::new();
        hasher.update(source.to_string().as_bytes());
        hasher.update(detection.to_string().as_bytes());
        hasher.update(digest.as_bytes());
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            .chars()
            .take(32)
            .collect()
    };
    let end_ms = if frames > 0 {
        round_ms((frames as f64 / fps) * 1000.0)
    } else {
        0
    };
    let receipt = json!({
        "analysisId": analysis_id,
        "revision": "1",
        "digest": digest,
        "source": source,
        "coveredRange": {"startMs": 0, "endMs": end_ms},
        "page": {"cursor": null, "nextCursor": null, "complete": true},
        "shots": shots
    });
    let mut session_detection = body;
    session_detection.as_object_mut().unwrap().remove("video");
    http_json(h,"POST",&format!("{}/bgx_face_refine/analysis-session",base.trim_end_matches('/')),Some(json!({"analysis":receipt,"detection":session_detection})))?;
    Ok(json!({
        "kind": "completed",
        "vendorTaskId": null,
        "appliedParams": null,
        "outputs": [],
        "data": receipt,
        "usage": null
    }))
}


fn analysis_state(h: &mut dyn Host, base: &str, id: &str, revision: &str, digest: &str) -> Result<Value> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) { return Err(invalid("invalid analysis session id")); }
    let saved = http_json(h,"GET",&format!("{}/bgx_face_refine/analysis-session/{id}",base.trim_end_matches('/')),None)?;
    if saved["analysis"]["revision"] != revision || saved["analysis"]["digest"] != digest {
        return Err(invalid("analysis session identity changed"));
    }
    Ok(saved)
}
fn apply_detection(wf: &mut Value, select_node: &str, track_node: &str, saved: &Value) {
    for key in ["detector","confidence","cut_detection","cut_threshold"] {
        wf[select_node]["inputs"][key] = saved["detection"][key].clone();
        wf[track_node]["inputs"][key] = saved["detection"][key].clone();
    }
}

fn selection_bindings(selection: &Value, analysis: &Value) -> Result<(String, String)> {
    // Upstream H3FaceSelect manual: confirmed_pick = comma-separated L→R face indices
    // per shot; -1 marks absent. Not shot:idx.
    let mut picks = Vec::new();
    let mut absent = Vec::new();
    if let Some(shots) = selection.get("shots").and_then(Value::as_array) {
        let analyzed = analysis["shots"].as_array().ok_or_else(|| invalid("analysis shots required"))?;
        if shots.len() != analyzed.len() { return Err(invalid("selection must cover every analyzed shot")); }
        for shot in analyzed {
            let selected: Vec<_> = shots.iter().filter(|s| s["shotId"] == shot["shotId"]).collect();
            if selected.len() != 1 { return Err(invalid("selection shot identity is missing or duplicated")); }
            let s = selected[0];
            let sid = s.get("shotId").and_then(Value::as_str).unwrap_or("");
            let kind = s
                .pointer("/choice/kind")
                .or_else(|| s.pointer("/decision/kind"))
                .and_then(Value::as_str)
                .unwrap_or("");
            match kind {
                "absent" => {
                    picks.push("-1".to_string());
                    absent.push(sid.to_string());
                }
                "target" => {
                    let cid = s
                        .pointer("/choice/candidateId")
                        .or_else(|| s.pointer("/decision/candidateId"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if !shot["candidates"].as_array().is_some_and(|cs| cs.iter().any(|c| c["candidateId"] == cid)) { return Err(invalid("selection candidate is not in analysis")); }
                    let idx = cid.strip_prefix('c').ok_or_else(|| invalid("candidate identity invalid"))?;
                    let n: i64 = idx.parse().map_err(|_| invalid("candidateId must be cN"))?;
                    picks.push(n.to_string());
                }
                _ => {
                    return Err(invalid(
                        "each shot decision must be target or absent (unset is incomplete)",
                    ));
                }
            }
        }
    }
    Ok((picks.join(","), absent.join(",")))
}


fn load_workflow_template(name: &str) -> Result<Value> {
    let raw = match name {
        "preview" => include_str!("../workflows/preview.track.api.json"),
        "refine" => include_str!("../workflows/refine.manual.api.json"),
        _ => return Err(invalid("unknown workflow template")),
    };
    serde_json::from_str(raw).map_err(|e| invalid(&format!("workflow JSON invalid: {e}")))
}

fn inject_preview_workflow(face_pick: &str, video_name: &str, analysis: &Value, selection: &Value) -> Result<Value> {
    let mut wf = load_workflow_template("preview")?;
    if let Some(obj) = wf.as_object_mut() {
        obj.remove("_meta");
        if let Some(n1) = obj.get_mut("1").and_then(|v| v.as_object_mut()) {
            if let Some(inputs) = n1.get_mut("inputs").and_then(|v| v.as_object_mut()) {
                inputs.insert("video".into(), json!(video_name));
                inputs.insert("confirmed_pick".into(), json!(face_pick));
                inputs.insert("select".into(), json!("manual"));
                inputs.insert("select_index".into(), json!(0));
                inputs.insert("detector".into(), json!("bbox\\face_yolov8m.pt"));
                inputs.insert("cut_detection".into(), json!("none"));
            }
        }
        if let Some(n2) = obj.get_mut("2").and_then(|v| v.as_object_mut()) {
            if let Some(inputs) = n2.get_mut("inputs").and_then(|v| v.as_object_mut()) {
                inputs.insert("detector".into(), json!("bbox\\face_yolov8m.pt"));
                inputs.insert("cut_detection".into(), json!("none"));
                if !inputs.contains_key("crop_factor") { inputs.insert("crop_factor".into(), json!(2.5)); }
                if !inputs.contains_key("canvas_width") { inputs.insert("canvas_width".into(), json!(768)); }
                if !inputs.contains_key("canvas_height") { inputs.insert("canvas_height".into(), json!(768)); }
                if !inputs.contains_key("canvas_mode") { inputs.insert("canvas_mode".into(), json!("manual")); }
                if !inputs.contains_key("smooth_window") { inputs.insert("smooth_window".into(), json!(21)); }
                if !inputs.contains_key("size_smooth_window") { inputs.insert("size_smooth_window".into(), json!(51)); }
                if !inputs.contains_key("smooth_method") { inputs.insert("smooth_method".into(), json!("gaussian")); }
                if !inputs.contains_key("size_mode") { inputs.insert("size_mode".into(), json!("per_frame")); }
                if !inputs.contains_key("preview") { inputs.insert("preview".into(), json!(true)); }
            }
        }
    }
    wf["3"]["inputs"]["fps"] = json!(["1",6]);
    wf["tracking_receipt"] = json!({"class_type":"H3FaceTrackingReceipt","inputs":{"transform":["2",1],"fps":["1",6],"analysis_json":analysis.to_string(),"selection_json":selection.to_string()}});
    Ok(wf)
}

fn inject_refine_workflow(
    face_pick: &str,
    video_name: &str,
    prompt: &str,
    user_params: &Value,
) -> Result<Value> {
    let mut wf = load_workflow_template("refine")?;
    if let Some(obj) = wf.as_object_mut() {
        obj.remove("_meta");
    }
    let denoise = user_params
        .get("denoise")
        .and_then(Value::as_f64)
        .unwrap_or(0.45);
    let small = user_params
        .get("smallFaceStrength")
        .and_then(Value::as_f64)
        .unwrap_or(0.8);
    let large = user_params
        .get("largeFaceStrength")
        .and_then(Value::as_f64)
        .unwrap_or(0.35);
    let scale_mode = user_params
        .get("scaleMode")
        .and_then(Value::as_str)
        .unwrap_or("absolute_px");
    let appearances = user_params.get("appearanceImages").and_then(Value::as_array).ok_or_else(|| invalid("uploaded appearanceImages required"))?;
    let identity = user_params
        .get("identityImage")
        .and_then(Value::as_str);
    if let Some(obj) = wf.as_object_mut() {
        if let Some(sel) = obj.get_mut("select").and_then(|v| v.as_object_mut()) {
            if let Some(inputs) = sel.get_mut("inputs").and_then(|v| v.as_object_mut()) {
                inputs.insert("video".into(), json!(video_name));
                inputs.insert("confirmed_pick".into(), json!(face_pick));
            }
        }
        if let Some(den) = obj.get_mut("denoise").and_then(|v| v.as_object_mut()) {
            if let Some(inputs) = den.get_mut("inputs").and_then(|v| v.as_object_mut()) {
                inputs.insert("denoise_multiplier_small_face".into(), json!(small));
                inputs.insert("denoise_multiplier_large_face".into(), json!(large));
                inputs.insert("scale_mode".into(), json!(scale_mode));
            }
        }
        if let Some(r2v) = obj.get_mut("r2v").and_then(|v| v.as_object_mut()) {
            if let Some(inputs) = r2v.get_mut("inputs").and_then(|v| v.as_object_mut()) {
                inputs.insert("prompt".into(), json!(prompt));
            }
        }
        if let Some(sched) = obj.get_mut("scheduler").and_then(|v| v.as_object_mut()) {
            if let Some(inputs) = sched.get_mut("inputs").and_then(|v| v.as_object_mut()) {
                inputs.insert("denoise".into(), json!(denoise));
            }
        }
        obj.remove("ref0");
        obj.remove("ref1");
        if let Some(inputs) = obj.get_mut("r2v").and_then(|n| n["inputs"].as_object_mut()) {
            inputs.remove("ref_images.ref_image_0"); inputs.remove("ref_images.ref_image_1");
            for (index, _) in appearances.iter().enumerate() {
                inputs.insert(format!("ref_images.ref_image_{index}"), json!([format!("appearance_{index}"),0]));
            }
        }
        for (index, name) in appearances.iter().enumerate() {
            obj.insert(format!("appearance_{index}"), json!({"class_type":"LoadImage","inputs":{"image":name}}));
        }
        if let Some(name) = identity {
            obj.insert("identity_ref".into(), json!({"class_type":"LoadImage","inputs":{"image":name}}));
            obj.get_mut("track").unwrap()["inputs"]["identity_reference"] = json!(["identity_ref",0]);
        }
        obj.get_mut("create").unwrap()["inputs"]["fps"] = json!(["select",6]);
        if user_params.get("samEnabled").and_then(Value::as_bool).unwrap_or(false) {
            let model = user_params.get("samModel").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| invalid("SAM enabled: select an installed samModel"))?;
            obj.insert("sam_loader".into(),json!({"class_type":"SAMLoader","inputs":{"model_name":model,"device_mode":"AUTO"}}));
            obj.insert("sam_mask".into(),json!({"class_type":"H3FaceMaskSAM","inputs":{"crops":["track",0],"sam_model":["sam_loader",0],"transform":["track",1],"threshold":0.93,"dilation":0,"temporal_smooth":5}}));
            obj.get_mut("stitch").unwrap()["inputs"]["masks"] = json!(["sam_mask",0]);
        }
    }
    Ok(wf)
}


mod platform_parameters;
mod sam;
mod submission_rejection;
use submission_rejection::Phase;
fn bind_shared_tracker(h: &mut dyn Host, base: &str, workflow: &mut Value, context: Value) -> Result<()> {
    let mut upstream = json!({"select": workflow["select"], "track": workflow["track"]});
    let info = platform_parameters::definitions(h, base, &upstream)?;
    platform_parameters::prepare(&mut upstream, &info)?;
    let video = upstream["select"]["inputs"].as_object_mut().unwrap().remove("video").unwrap();
    let track_options = upstream["track"]["inputs"].as_object_mut().unwrap();
    track_options.remove("images");
    track_options.remove("face_pick");
    let identity = track_options.remove("identity_reference");
    let mut inputs = json!({
        "video": video, "identity_file": context["identityFile"], "settings_json": json!({
            "select": upstream["select"]["inputs"], "track": upstream["track"]["inputs"],
        }).to_string(),
        "analysis_json": context["analysis"].to_string(), "selection_json": context["selection"].to_string(),
        "render_preview": context["preview"],
    });
    if let Some(link) = identity { inputs["identity_reference"] = link; }
    if !context["trajectoryFile"].is_null() {
        inputs["trajectory_file"] = context["trajectoryFile"].clone();
        inputs["trajectory_digest"] = context["trajectoryDigest"].clone();
    }
    let graph = workflow.as_object_mut().unwrap();
    graph.remove("select");
    graph.insert("track".into(), json!({"class_type":"BGXFaceTrack","inputs":inputs}));
    for node in graph.values_mut() {
        for value in node["inputs"].as_object_mut().unwrap().values_mut() {
            if value.get(0).and_then(Value::as_str) == Some("select") {
                let output = match value[1].as_u64() {
                    Some(0) => 7,
                    Some(1) => 8,
                    Some(6) => 9,
                    _ => return Err(invalid("invalid face-track source output")),
                };
                *value = json!(["track", output]);
            }
        }
    }
    Ok(())
}

fn comfy_submit_prompt(h: &mut dyn Host, base: &str, mut workflow: Value, client_id: &str, phase: &mut Phase) -> Result<String> {
    if let Some(context) = workflow.as_object_mut().and_then(|obj| obj.remove("_tracking_context")) {
        bind_shared_tracker(h, base, &mut workflow, context)?;
    }
    let info = platform_parameters::definitions(h, base, &workflow)?;
    platform_parameters::prepare(&mut workflow, &info)?;
    let url = format!("{}/prompt", base.trim_end_matches('/'));
    let body = json!({
        "prompt": workflow,
        "client_id": client_id
    });
    *phase = Phase::Unknown;
    let raw = http_json(h, "POST", &url, Some(body)).map_err(|error| {
        if error.service_rejected { *phase = Phase::ServiceRejected; return error; }
        if matches!(error.code, "PERMISSION_DENIED" | "INVALID_INPUT" | "BUDGET_EXCEEDED") { error }
        else { fail("ACCEPTANCE_UNKNOWN", format!("Comfy submission receipt unavailable: {}; do not resubmit", error.message)) }
    })?;
    if let Some(err) = raw.get("error").cloned() {
        return Err(fail(
            "UPSTREAM_ERROR",
            format!("Comfy /prompt rejected: {err}"),
        ));
    }
    raw.get("prompt_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| fail("ACCEPTANCE_UNKNOWN", "Comfy submission receipt missing prompt_id; do not resubmit"))
}

fn comfy_query_prompt(h: &mut dyn Host, base: &str, prompt_id: &str) -> Result<Value> {
    let url = format!(
        "{}/history/{}",
        base.trim_end_matches('/'),
        prompt_id
    );
    let raw = http_json(h, "GET", &url, None)?;
    Ok(raw)
}

fn comfy_cancel(h: &mut dyn Host, base: &str, prompt_id: &str) -> Result<Value> {
    let queue = http_json(h, "GET", &format!("{}/queue", base.trim_end_matches('/')), None)?;
    let pending = queue["queue_pending"].as_array().ok_or_else(|| fail("UPSTREAM_ERROR", "invalid queue response"))?;
    if pending.iter().any(|row| row[1].as_str() == Some(prompt_id)) {
        http_json(h, "POST", &format!("{}/queue", base.trim_end_matches('/')), Some(json!({"delete":[prompt_id]})))?;
        return Ok(json!({"outcome":"remote_requested"}));
    }
    // A global interrupt cannot guarantee cancellation of this prompt only.
    Ok(json!({"outcome":"unsupported"}))
}

mod task_results;

fn omit_audio_for_silent_source(workflow: &mut Value) {
    workflow["r2v"]["inputs"].as_object_mut().unwrap().remove("ref_audios.ref_audio_0");
    workflow["create"]["inputs"].as_object_mut().unwrap().remove("audio");
}

fn run_preview(h: &mut dyn Host, payload: &Value, phase: &mut Phase) -> Result<Value> {
    let configuration = payload.get("configuration").cloned().unwrap_or(json!({}));
    let base = cfg_base(&configuration)?;
    let input = payload.get("input").ok_or_else(|| invalid("missing input"))?;
    let selection = input.get("selection").ok_or_else(|| invalid("preview requires selection"))?;
    let analysis = input.get("analysis").ok_or_else(|| invalid("preview requires analysis"))?;
    let selection_digest = selection["selectionDigest"].as_str().filter(|v| !v.is_empty()).ok_or_else(|| invalid("selectionDigest required"))?;
    let saved = analysis_state(h, base, analysis["analysisId"].as_str().ok_or_else(|| invalid("analysisId required"))?, analysis["revision"].as_str().unwrap_or(""), analysis["digest"].as_str().unwrap_or(""))?;
    if saved["analysis"] != *analysis { return Err(invalid("analysis receipt differs from retained result")); }
    let (face_pick, _absent_shots) = selection_bindings(selection, analysis)?;
    let source = &analysis["source"];
    let source_id = source["mediaId"].as_str().ok_or_else(|| invalid("analysis source.mediaId required"))?;
    let video_name = upload_source(h, payload, base, source_id, "video")?;
    let preview = inject_preview_workflow(&face_pick, &video_name, analysis, selection)?;
    let identity_name = input["identityMediaId"].as_str()
        .map(|id| upload_source(h, payload, base, id, "image")).transpose()?;
    let mut workflow = inject_refine_workflow(&face_pick, &video_name, "", &json!({
        "appearanceImages": [], "identityImage": &identity_name,
    }))?;
    apply_detection(&mut workflow, "select", "track", &saved);
    workflow.as_object_mut().unwrap().retain(|key, _| matches!(key.as_str(), "select" | "track" | "identity_ref"));
    workflow["3"] = preview["3"].clone();
    workflow["3"]["inputs"]["images"] = json!(["track",2]);
    workflow["3"]["inputs"]["fps"] = json!(["select",6]);
    workflow["tracking_receipt"] = preview["tracking_receipt"].clone();
    workflow["tracking_receipt"]["inputs"]["transform"] = json!(["track",1]);
    workflow["tracking_receipt"]["inputs"]["fps"] = json!(["select",6]);
    workflow["_tracking_context"] = json!({"analysis":analysis,"selection":selection,"preview":true,"identityFile":identity_name.unwrap_or_default()});
    let client_id = format!("h3-preview-{:.12}", selection_digest);
    let prompt_id = comfy_submit_prompt(h, base, workflow, &client_id, phase)?;
    Ok(json!({
        "kind": "accepted", "vendorTaskId": prompt_id,
        "appliedParams": null,
        "resumeToken": {"operationId":OP_PREVIEW,"promptId":prompt_id,"selectionDigest":selection_digest,"coveredRange":analysis["coveredRange"],"source":source},
        "usage": null
    }))
}

fn run_refine(h: &mut dyn Host, payload: &Value, phase: &mut Phase) -> Result<Value> {
    let configuration = payload.get("configuration").cloned().unwrap_or(json!({}));
    let base = cfg_base(&configuration)?;
    let input = payload.get("input").ok_or_else(|| invalid("missing input"))?;
    let freeze = input.get("freeze").ok_or_else(|| invalid("refine requires freeze"))?;
    let selection = input.get("selection");
    let prompt = freeze.get("prompt").and_then(Value::as_str).unwrap_or("");
    let mut user_params = freeze.get("userParams").cloned().unwrap_or(json!({}));
    sam::resolve_for_submit(h, base, &mut user_params)?;
    let trajectory = input.get("trajectory");
    let mut trajectory_name = None;
    let (saved, face_pick);
    if let Some(trajectory) = trajectory {
        let health = http_json(h, "GET", &format!("{}/bgx_face_refine/health",base.trim_end_matches('/')), None)?;
        if health["trajectoryArtifacts"] != true {
            return Err(fail("RESOURCE_UNAVAILABLE", "请重启当前配置的 ComfyUI，加载支持轨迹修正的人脸精修节点"));
        }
        let id = trajectory["mediaId"].as_str().ok_or_else(|| invalid("missing confirmed trajectory file"))?;
        trajectory_name = Some(upload_source(h, payload, base, id, "file")?);
        saved = json!({});
        face_pick = String::new();
    } else {
        let selection = selection.ok_or_else(|| invalid("refine requires selection"))?;
        saved = analysis_state(h, base, selection["analysisId"].as_str().ok_or_else(|| invalid("analysisId required"))?, selection["analysisRevision"].as_str().unwrap_or(""), selection["analysisDigest"].as_str().unwrap_or(""))?;
    if saved["analysis"]["source"] != freeze["source"] || selection["selectionDigest"] != freeze["selectionDigest"] { return Err(invalid("refine source or selection differs from frozen analysis")); }
        (face_pick, _) = selection_bindings(selection, &saved["analysis"])?;
    }
    let source_id = freeze.pointer("/source/mediaId").and_then(Value::as_str).ok_or_else(|| invalid("freeze source.mediaId required"))?;
    let video_name = upload_source(h, payload, base, source_id, "video")?;
    let references = freeze["referenceMediaIds"].as_array().ok_or_else(|| invalid("referenceMediaIds required"))?;
    if references.len() > 9 { return Err(invalid("at most nine appearance references are supported")); }
    let mut images = Vec::new();
    for id in references {
        images.push(upload_source(h, payload, base, id.as_str().ok_or_else(|| invalid("reference mediaId must be string"))?, "image")?);
    }
    let identities = match user_params.get("identityMediaIds") {
        None => Vec::new(),
        Some(v) => v.as_array().ok_or_else(|| invalid("identityMediaIds must be array"))?.clone(),
    };
    if identities.len() > 1 { return Err(invalid("at most one identity reference is supported")); }
    let identity = identities.first().map(|id| id.as_str().ok_or_else(|| invalid("identity mediaId must be string"))).transpose()?;
    let identity_name = identity.map(|id| upload_source(h, payload, base, id, "image")).transpose()?;
    user_params["appearanceImages"] = json!(images);
    user_params["identityImage"] = json!(identity_name);
    let mut workflow = inject_refine_workflow(&face_pick, &video_name, prompt, &user_params)?;
    if trajectory_name.is_none() {
        apply_detection(&mut workflow, "select", "track", &saved);
    }
    let media = http_json(h, "POST", &format!("{}/bgx_face_refine/source-audio", base.trim_end_matches('/')),
        Some(json!({"video":video_name})))?;
    match media["hasAudio"].as_bool() {
        Some(false) => omit_audio_for_silent_source(&mut workflow),
        Some(true) => {}
        None => return Err(fail("UPSTREAM_ERROR", "Comfy did not report the uploaded video's audio streams")),
    }
    workflow["_tracking_context"] = json!({
        "analysis":saved["analysis"],"selection":selection,"preview":false,
        "identityFile":identity_name.unwrap_or_default(), "trajectoryFile":trajectory_name,
        "trajectoryDigest":trajectory.map(|v| &v["trajectoryDigest"]),
    });
    let freeze_raw = serde_json::to_string(input).unwrap_or_default();
    let freeze_digest: String = Sha256::digest(freeze_raw.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let client_id = format!("h3-refine-{:.12}", freeze_digest);
    let prompt_id = comfy_submit_prompt(h, base, workflow, &client_id, phase)?;
    Ok(json!({
        "kind":"accepted", "vendorTaskId":prompt_id,
        "appliedParams": null,
        "resumeToken":{"operationId":OP_REFINE,"promptId":prompt_id,"freezeDigest":freeze_digest,"selectionDigest":freeze["selectionDigest"],"source":freeze["source"],"coveredRange":freeze["source"]["selectedRange"]},
        "usage":null
    }))
}

fn frozen_input_problem(operation: &str, input: &Value) -> Option<(&'static str, String)> {
    let source_pointer = match operation {
        OP_ANALYZE => "/source",
        OP_PREVIEW => "/analysis/source",
        OP_REFINE => "/freeze/source",
        _ => return None,
    };
    let source = input.pointer(source_pointer).unwrap_or(&Value::Null);
    let refine_digest = text_digest(&REFINE_INPUT_SCHEMA.replace("\r\n", "\n"));
    if source["packageId"] != PLUGIN || source["operationId"] != OP_REFINE
        || source["schemaDigest"] != refine_digest.as_str()
    {
        return Some((source_pointer, "Source identity does not match the selected refine operation".into()));
    }
    if operation == OP_REFINE && input.get("trajectory").is_some() {
        let trajectory = &input["trajectory"];
        if input.get("selection").is_some() || trajectory["trajectoryDigest"] != input["freeze"]["selectionDigest"] {
            return Some(("/trajectory", "精修轨迹与已确认版本不一致".into()));
        }
        return None;
    }
    let analysis = match operation {
        OP_PREVIEW => &input["analysis"],
        OP_REFINE => &input["freeze"]["analysis"],
        _ => return None,
    };
    let selection = &input["selection"];
    if selection["analysisId"] != analysis["analysisId"]
        || selection["analysisRevision"] != analysis["revision"]
        || selection["analysisDigest"] != analysis["digest"]
        || selection["selectionDigest"].as_str().is_none_or(str::is_empty)
    {
        return Some(("/selection", "Selection does not match the frozen analysis".into()));
    }
    if operation == OP_PREVIEW {
        return selection_bindings(selection, analysis).err().map(|failure| ("/selection/shots", failure.message));
    }
    let freeze = &input["freeze"];
    if freeze["packageVersion"] != env!("CARGO_PKG_VERSION") || freeze["operationId"] != OP_REFINE
        || freeze["schemaDigest"] != refine_digest.as_str()
    {
        return Some(("/freeze", "Refine package, operation or schema changed".into()));
    }
    if freeze["selectionDigest"] != selection["selectionDigest"] {
        return Some(("/freeze/selectionDigest", "Refine selection digest does not match".into()));
    }
    None
}

fn prepare_operation_input(payload: &Value) -> Value {
    let operation = payload["selection"]["operationId"].as_str().unwrap_or_default();
    match frozen_input_problem(operation, &payload["input"]) {
        Some((field, message)) => json!({
            "kind":"operation_input_preflight","status":"rejected","requestDigest":payload["requestDigest"],
            "diagnostics":[{"code":"h3-face-refine.invalid-input","fieldPointer":field,"message":message}]
        }),
        None => json!({
            "kind":"operation_input_preflight","status":"ready","requestDigest":payload["requestDigest"],"material":{}
        }),
    }
}

pub struct H3FaceRefine;

impl Adapter for H3FaceRefine {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        let operation = serde_json::to_value(&request.r#operation).expect("closed operation");
        let result = match operation.as_str().expect("operation string") {
            "capabilities" => capabilities(),
            "probe" => {
                let payload = match submit_payload(&request) {
                    Ok(value) => value,
                    Err(failure) => return response_fail(&request, failure),
                };
                if payload["kind"] == "operation_input_preflight" {
                    prepare_operation_input(&payload)
                } else if payload["kind"] == "operation_form_options" {
                    match sam::probe(host, &payload) {
                        Ok(fields) => fields,
                        Err(failure) => return response_fail(&request, failure),
                    }
                } else {
                    json!({
                        "packageValid": true,
                        "connection": "not_checked",
                        "authentication": "not_checked",
                        "resources": "not_checked",
                        "issues": []
                    })
                }
            }
            "submit" => {
                let payload = match submit_payload(&request) {
                    Ok(v) => v,
                    Err(f) => return response_fail(&request, f),
                };
                let op_id = payload
                    .get("operationId")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let mut phase = Phase::BeforeSubmission;
                let run = if let Some((field, message)) = frozen_input_problem(op_id, &payload["input"]) {
                    Err(invalid(&format!("{field}: {message}")))
                } else {
                    match op_id {
                        OP_TRAJECTORY => run_trajectory(host, &payload, &mut phase),
                        OP_SEED => run_seed(host, &payload, &mut phase),
                        OP_ANALYZE => run_analyze(host, &payload, &mut phase),
                        OP_PREVIEW => run_preview(host, &payload, &mut phase),
                        OP_REFINE => run_refine(host, &payload, &mut phase),
                        _ => Err(fail(
                            "UNSUPPORTED_FEATURE",
                            format!("unknown operationId: {op_id}"),
                        )),
                    }
                };
                match run {
                    Ok(v) => v,
                    Err(f) => {
                        if let Err(persist) = submission_rejection::record(host, &request.request_id, phase, &f) {
                            return response_fail(&request, persist);
                        }
                        return response_fail(&request, f);
                    }
                }
            }
            "query" => {
                let payload = match submit_payload(&request) { Ok(v) => v, Err(f) => return response_fail(&request, f) };
                match task_results::query(host, &payload) { Ok(v) => v, Err(f) => return response_fail(&request, f) }
            }
            "cancel" => {
                let payload = match submit_payload(&request) {
                    Ok(v) => v,
                    Err(f) => return response_fail(&request, f),
                };
                let configuration = payload.get("configuration").cloned().unwrap_or(json!({}));
                let base = match cfg_base(&configuration) {
                    Ok(b) => b.to_owned(),
                    Err(f) => return response_fail(&request, f),
                };
                let vendor_task_id = payload
                    .get("vendorTaskId")
                    .or_else(|| payload.get("taskId"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if vendor_task_id.is_empty() {
                    return response_fail(&request, invalid("cancel requires vendorTaskId"));
                }
                match comfy_cancel(host, &base, vendor_task_id) {
                    Ok(v) => v,
                    Err(f) => return response_fail(&request, f),
                }
            }
            _ => {
                return response_fail(
                    &request,
                    fail("UNSUPPORTED_FEATURE", "operation not implemented by this package"),
                );
            }
        };
        guest::completed_response(&request, &result).expect("serializable public result")
    }
}

bgx_adapter_v2_sdk::export_adapter!(H3FaceRefine);

#[cfg(test)]
mod tests {
    use super::*;
    struct TestHost { calls: usize }
    impl Host for TestHost {
        fn call(&mut self, call: bgx_adapter_v2_sdk::HostPortCallV2) -> core::result::Result<Value, host::HostError> {
            let call = serde_json::to_value(call).unwrap();
            self.calls += 1;
            if self.calls <= 2 {
                assert_eq!(call["args"]["plan"]["method"], "GET");
                let value = if self.calls == 1 { json!({"analysisSessions":true,"trackingReceipt":true}) } else { json!({"H3FaceSelect":{}}) };
                return Ok(json!({"status":200,"payload":{"bytes":serde_json::to_vec(&value).unwrap()}}));
            }
            if self.calls == 3 {
                assert_eq!(call["method"], "host-media.info");
                return Ok(json!({"kind":"video","mime":"video/mp4","fileName":"clip.mp4","byteLength":"123"}));
            }
            if self.calls == 6 {
                assert_eq!(call["method"], "host-http.execute");
                assert!(call["args"]["plan"]["url"].as_str().unwrap().ends_with("/bgx_face_refine/analysis-session"));
                return Ok(json!({"status":200,"payload":{"kind":"buffered","bytes":serde_json::to_vec(&json!({"saved":true})).unwrap()}}));
            }
            assert_eq!(call["method"], "host-http.execute");
            let plan = &call["args"]["plan"];
            let value = if self.calls == 4 {
                assert_eq!(plan["body"]["parts"][0]["handle"], "managed-clip");
                json!({"name":"uploaded.mp4","subfolder":""})
            } else {
                let bytes: Vec<u8> = serde_json::from_value(plan["body"]["bytes"].clone()).unwrap();
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(body["video"], "uploaded.mp4");
                json!({"width":640,"height":360,"fps":25,"frames":25,"shots":[{"segment":[0,25],"frame":0,"boxes":[[0.1,0.2,0.3,0.4]]}]})
            };
            Ok(json!({"status":200,"payload":{"kind":"buffered","bytes":serde_json::to_vec(&value).unwrap()}}))
        }
    }
    #[test]
    fn host_budget_failure_is_a_valid_error_response() {
        let request: Invocation = serde_json::from_value(json!({"protocolVersion":2,"operation":"submit","profileId":"test","bindingId":"test","capabilitySlot":"videoFaceRefine","requestId":"test","budgetMs":1000,"payloadJson":"{}","negotiatedVersion":2,"enabledFeatures":[]})).unwrap();
        let response = serde_json::to_value(response_fail(&request, fail("BUDGET_EXCEEDED", "HTTP response buffer exceeds public policy"))).unwrap();
        assert_eq!(response["error"]["code"], "RESOURCE_UNAVAILABLE");
        assert_eq!(response["error"]["message"], "BUDGET_EXCEEDED: HTTP response buffer exceeds public policy");
        assert_eq!(response["error"]["retryable"], false);
    }
    #[test]
    fn submission_transport_failure_is_not_safe_to_resubmit() {
        struct LostReceipt;
        impl Host for LostReceipt {
            fn call(&mut self, call: bgx_adapter_v2_sdk::HostPortCallV2) -> core::result::Result<Value, host::HostError> {
                let v = serde_json::to_value(call).unwrap();
                if v.to_string().contains("object_info") { return Ok(json!({"status":200,"payload":{"bytes":b"{}".to_vec()}})); }
                Err(serde_json::from_value(json!({"code":"TIMEOUT","message":"connection lost"})).unwrap())
            }
        }
        let error = comfy_submit_prompt(&mut LostReceipt,"http://localhost:8188",json!({}),"test", &mut Phase::BeforeSubmission).unwrap_err();
        assert_eq!(error.code,"ACCEPTANCE_UNKNOWN");
        assert!(!error.retryable);
    }
    #[test]
    fn only_explicit_service_validation_receipt_marks_service_rejected() {
        struct Reply { service_response: bool }
        impl Host for Reply {
            fn call(&mut self, call: bgx_adapter_v2_sdk::HostPortCallV2) -> core::result::Result<Value, host::HostError> {
                let v=serde_json::to_value(call).unwrap();
                if v["args"]["plan"]["method"] == "GET" { return Ok(json!({"status":200,"payload":{"bytes":b"{}".to_vec()}})); }
                if !self.service_response { return Err(host::invalid_wire("host input rejected without service receipt")); }
                let bytes=serde_json::to_vec(&json!({"error":{"type":"prompt_outputs_failed_validation"},"node_errors":{}})).unwrap();
                Ok(json!({"status":400,"payload":{"bytes":bytes}}))
            }
        }
        let mut phase=Phase::BeforeSubmission;
        let error=comfy_submit_prompt(&mut Reply{service_response:false},"http://localhost",json!({}),"test",&mut phase).unwrap_err();
        assert!(matches!(phase,Phase::Unknown));
        assert!(!error.service_rejected);
        let error=comfy_submit_prompt(&mut Reply{service_response:true},"http://localhost",json!({}),"test",&mut phase).unwrap_err();
        assert!(matches!(phase,Phase::ServiceRejected));
        assert!(error.service_rejected);
    }
    #[test]
    fn graph_uses_uploaded_refs_identity_tracking_and_actual_fps() {
        let wf = inject_refine_workflow("0", "input.mp4", "原文", &json!({"appearanceImages":["a.png","b.png"],"identityImage":"person.png"})).unwrap();
        assert!(wf.get("ref0").is_none());
        assert_eq!(wf["r2v"]["inputs"]["ref_images.ref_image_1"],json!(["appearance_1",0]));
        assert_eq!(wf["track"]["inputs"]["identity_reference"],json!(["identity_ref",0]));
        assert_eq!(wf["create"]["inputs"]["fps"],json!(["select",6]));
        assert_eq!(wf["r2v"]["inputs"]["prompt"],"原文");
        let empty = inject_refine_workflow("0", "v.mp4", "", &json!({"appearanceImages":[]})).unwrap();
        assert!(empty["r2v"]["inputs"].get("ref_images.ref_image_0").is_none());
    }
    #[test]
    fn preview_has_receipt_output_and_actual_fps() {
        let analysis = json!({"analysisId":"a"}); let selection = json!({"selectionDigest":"d"});
        let wf = inject_preview_workflow("0", "input.mp4", &analysis, &selection).unwrap();
        assert_eq!(wf["tracking_receipt"]["class_type"],"H3FaceTrackingReceipt");
        assert_eq!(wf["3"]["inputs"]["fps"],json!(["1",6]));
        assert_eq!(wf["tracking_receipt"]["inputs"]["analysis_json"],analysis.to_string());
    }

    #[test]
    fn silent_source_has_no_reference_or_output_audio_links() {
        let mut graph = inject_refine_workflow("0", "input.mp4", "", &json!({"appearanceImages": []})).unwrap();
        assert!(graph["r2v"]["inputs"].get("ref_audios.ref_audio_0").is_some());
        assert!(graph["create"]["inputs"].get("audio").is_some());
        omit_audio_for_silent_source(&mut graph);
        assert!(graph["r2v"]["inputs"].get("ref_audios.ref_audio_0").is_none());
        assert!(graph["create"]["inputs"].get("audio").is_none());
        assert_eq!(graph["create"]["inputs"]["fps"], json!(["select",6]));
    }
    #[test]
    fn frozen_input_keeps_operation_roles_and_rejects_stale_source_or_sam() {
        let declared = capabilities();
        let ops = declared["capabilityContracts"][0]["operations"].as_array().unwrap();
        assert_eq!(ops.len(), 5);
        assert_eq!(ops.iter().map(|op| op["id"].as_str().unwrap()).collect::<Vec<_>>(), [OP_TRAJECTORY, OP_SEED, OP_ANALYZE, OP_PREVIEW, OP_REFINE]);
        assert!(ops.iter().all(|op| op["inputProfile"] == "operation-input-v1"));
        assert_eq!(ops[4]["version"], 2);
        assert_eq!(ops[3]["uiSchema"]["controls"], json!([]));
        assert_eq!(ops[4]["mediaSlots"].as_array().unwrap().iter().map(|slot| slot["role"].as_str().unwrap()).collect::<Vec<_>>(),
            ["source_video", "trajectory", "appearance_ref", "identity_ref"]);
        let digest = &ops[4]["inputSchema"]["digest"];
        let source = json!({"packageId":PLUGIN,"operationId":OP_REFINE,"schemaDigest":digest});
        let mut input = json!({
            "freeze":{"source":source,"packageVersion":env!("CARGO_PKG_VERSION"),"operationId":OP_REFINE,
                "schemaDigest":digest,"analysis":{"analysisId":"a","revision":"1","digest":"analysis-digest"},
                "selectionDigest":"selection-digest","userParams":{"samEnabled":false}},
            "selection":{"analysisId":"a","analysisRevision":"1","analysisDigest":"analysis-digest","selectionDigest":"selection-digest"}
        });
        assert!(frozen_input_problem(OP_REFINE, &input).is_none());
        input["freeze"]["userParams"]["samEnabled"] = json!(true);
        assert!(frozen_input_problem(OP_REFINE, &input).is_none());
        input["freeze"]["source"]["schemaDigest"] = json!("old-schema");
        assert_eq!(frozen_input_problem(OP_REFINE, &input).unwrap().0, "/freeze/source");
        let response = prepare_operation_input(&json!({
            "selection":{"operationId":OP_REFINE},"input":input,"requestDigest":"input-digest"
        }));
        assert_eq!(response["requestDigest"], "input-digest");
        assert_eq!(response["status"], "rejected");
        assert_eq!(response["diagnostics"][0]["fieldPointer"], "/freeze/source");
    }
    #[test]
    fn analysis_missing_nodes_never_uploads_and_http_router_rejection_is_terminal() {
        struct UnreadyOrRejected { ready: bool, scan_error: Option<Value>, inner: TestHost }
        impl Host for UnreadyOrRejected {
            fn call(&mut self, call: bgx_adapter_v2_sdk::HostPortCallV2) -> core::result::Result<Value, host::HostError> {
                let value = serde_json::to_value(&call).unwrap();
                if !self.ready {
                    self.inner.calls += 1;
                    assert_eq!(value["args"]["plan"]["method"], "GET");
                    return Ok(json!({"status":404,"payload":{"bytes":b"404: Not Found".to_vec()}}));
                }
                if value["args"]["plan"]["url"] == "http://localhost/h3_facerefine/scan" {
                    if let Some(body) = &self.scan_error {
                        return Ok(json!({"status":200,"payload":{"bytes":serde_json::to_vec(body).unwrap()}}));
                    }
                    return Ok(json!({"status":405,"payload":{"bytes":b"405: Method Not Allowed".to_vec()}}));
                }
                self.inner.call(call)
            }
        }
        let payload = json!({"configuration":{"endpointBaseUrl":"http://localhost"},"input":{"source":{"mediaId":"source"}},"media":[{"id":"source","handle":"managed-clip","kind":"video","mime":"video/mp4","fileName":"clip.mp4","byteLength":"123"}]});
        let mut host = UnreadyOrRejected { ready: false, scan_error: None, inner: TestHost { calls: 0 } };
        let mut phase = Phase::BeforeSubmission;
        let failure = run_analyze(&mut host, &payload, &mut phase).unwrap_err();
        assert_eq!(failure.code, "RESOURCE_UNAVAILABLE");
        assert!(matches!(phase, Phase::BeforeSubmission));
        assert_eq!(host.inner.calls, 1);
        host.ready = true;
        host.inner.calls = 0;
        let failure = run_analyze(&mut host, &payload, &mut phase).unwrap_err();
        assert_eq!(failure.code, "RESOURCE_UNAVAILABLE");
        assert!(matches!(phase, Phase::ServiceRejected));
        assert!(failure.service_rejected);
        for busy in [true, false] {
            host.inner.calls = 0;
            host.scan_error = Some(json!({"busy":busy,"error":"scan failed"}));
            let failure = run_analyze(&mut host, &payload, &mut phase).unwrap_err();
            assert_eq!(failure.code, if busy { "RATE_LIMITED" } else { "UPSTREAM_ERROR" });
            assert_eq!(failure.retryable, busy);
            assert_eq!(matches!(phase, Phase::ServiceRejected), busy);
            assert_eq!(matches!(phase, Phase::Unknown), !busy);
        }
    }
    #[test]
    fn analysis_uses_managed_upload_and_preserves_source_identity() {
        let source = json!({"mediaId":"source","digest":"frozen-source"});
        let payload = json!({"configuration":{"endpointBaseUrl":"http://127.0.0.1:8188"},"input":{"source":source},"media":[{"id":"source","handle":"managed-clip","kind":"video","mime":"video/mp4","fileName":"clip.mp4","byteLength":"123"}]});
        let mut host = TestHost { calls: 0 };
        let result = run_analyze(&mut host, &payload, &mut Phase::BeforeSubmission).unwrap();
        assert_eq!(host.calls, 6);
        assert_eq!(result["data"]["source"], source);
        assert!(result["data"].get("vendor").is_none());
        assert_eq!(result["data"]["shots"][0]["candidates"][0]["box"]["width"], 128.0);
    }
}
