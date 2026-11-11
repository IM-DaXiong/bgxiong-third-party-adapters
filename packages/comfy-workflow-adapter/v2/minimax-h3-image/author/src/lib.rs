#![cfg_attr(target_arch = "wasm32", no_std)]
//! MiniMax H3-Image adapter with resource-host verified native workflows.
//! Workflow commits, model dependencies and digests are recorded in SOURCE.json.
//! Platform/node/HTTP semantics stay inside this package. No application source is required.
extern crate alloc;
use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use bgx_adapter_v2_sdk::{guest, host, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PLUGIN_ID: &str = "local.bgx.minimax-h3-image-comfy";
const TEXT_OPERATION: &str = "local.bgx.minimax-h3-image-comfy/text-to-image";
const EDIT_OPERATION: &str = "local.bgx.minimax-h3-image-comfy/reference-edit";
const TEXT_MODEL: &str = "minimax-h3-image-text-to-image";
const EDIT_MODEL: &str = "minimax-h3-image-reference-edit";
const OUTPUT_NODE: &str = "8";
const REFERENCE_NODES: [&str; 2] = ["9", "10"];
const MAX_PROMPT: usize = 8000;
const MAX_SEED: u64 = 9007199254740991;
const TEXT_SCHEMA: &str = include_str!("../schemas/text.json");
const EDIT_SCHEMA: &str = include_str!("../schemas/edit.json");
const OUTPUT_SCHEMA: &str = r#"{"type":"object","properties":{"description":{"type":"string"}},"required":["description"],"additionalProperties":false}"#;
const CONFIG_SCHEMA: &str = r#"{"type":"object","properties":{"endpointBaseUrl":{"type":"string","minLength":1}},"required":["endpointBaseUrl"],"additionalProperties":false}"#;
const RESUME_SCHEMA: &str = include_str!("../schemas/resume.json");

type Outcome<T> = Result<T, Failure>;
#[derive(Debug)]
struct Failure {
    code: &'static str,
    message: &'static str,
    vendor: Option<String>,
    request: Option<String>,
}
fn fail(code: &'static str, message: &'static str) -> Failure {
    Failure {
        code,
        message,
        vendor: None,
        request: None,
    }
}
fn malformed() -> Failure {
    fail(
        "UPSTREAM_ERROR",
        "ComfyUI returned an unexpected structured response",
    )
}
fn text<'a>(v: &'a Value, key: &str) -> Outcome<&'a str> {
    v.get(key).and_then(Value::as_str).ok_or_else(malformed)
}
fn number(v: &Value, key: &str) -> Outcome<u64> {
    v.get(key).and_then(Value::as_u64).ok_or_else(malformed)
}
fn policy(_name: &str) -> u64 {
    1_048_576
} // protocol/schema-policy.json limits.maxBufferedHttpBytes
fn port(h: &mut dyn Host, method: &str, args: Value) -> Outcome<Value> {
    host::call(h, method, &args).map_err(|e| {
        let v = serde_json::to_value(&e).expect("public host error");
        let code = match v["code"].as_str() {
            Some("TIMEOUT") => "TIMEOUT",
            Some("INVALID_INPUT") => "INVALID_INPUT",
            Some("PERMISSION_DENIED") => "PERMISSION_DENIED",
            _ => "RESOURCE_UNAVAILABLE",
        };
        fail(
            code,
            "A managed host operation failed; no retry was attempted",
        )
    })
}
fn schema(id: &str, source: &str) -> Value {
    let source = source.replace("\r\n", "\n");
    let digest: String = Sha256::digest(source.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    json!({"id":id,"digest":digest,"schemaJson":source})
}
fn capabilities() -> Value {
    let contracts: Vec<Value> = [
        (TEXT_OPERATION, TEXT_MODEL, "text-to-image", "MiniMax-H3-Image · 文生图", TEXT_SCHEMA, vec![]),
        (EDIT_OPERATION, EDIT_MODEL, "reference-edit", "MiniMax-H3-Image · 参考图编辑", EDIT_SCHEMA,
            vec![json!({"role":"referenceImages","inputPointer":"/referenceImages","mediaKind":"image","minCount":1,"maxCount":2})]),
    ].into_iter().map(|(operation, model, local, label, input_schema, slots)| {
        let input = schema("input", input_schema);
        let output = schema("output", OUTPUT_SCHEMA);
        let config = schema("config", CONFIG_SCHEMA);
        let resume = schema("resume", RESUME_SCHEMA);
        json!({"schemaVersion":2,"localId":local,"capabilityKey":"image.generate","transportSlot":"image","modelIds":[model],
            "configurationSchema":{"id":"config","digest":config["digest"]},
            "operations":[{"id":operation,"version":1,"label":label,
                "inputSchema":{"id":"input","digest":input["digest"]},"outputSchema":{"id":"output","digest":output["digest"]},
                "resumeSchema":{"id":"resume","digest":resume["digest"]},
                "uiSchema":{"controls":[
                    {"fieldPointer":"/prompt","widget":"multiline","label":"提示词","description":"","group":"Input"},
                    {"fieldPointer":"/seed","widget":"number","label":"随机种子","description":"","group":"Input"},
                    {"fieldPointer":"/aspectRatio","widget":"select","label":"画幅比例","description":"","group":"Input"},
                    {"fieldPointer":"/resolution","widget":"select","label":"分辨率","description":"","group":"Input"}]},
                "mediaSlots":slots,
                "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported","resumable":true,"remoteIdempotency":false},
                "outputSlots":[{"role":"artifact","kind":"image","minCount":1,"maxCount":1}]}],
            "schemas":[input,output,config,resume]})
    }).collect();
    json!({"schemaVersion":2,"pluginId":PLUGIN_ID,"pluginVersion":env!("CARGO_PKG_VERSION"),"capabilityContracts":contracts})
}
fn is_edit(operation: &str) -> Outcome<bool> {
    match operation {
        TEXT_OPERATION => Ok(false),
        EDIT_OPERATION => Ok(true),
        _ => Err(fail("INVALID_INPUT", "Unknown operation")),
    }
}
fn workflow(edit: bool) -> Outcome<Value> {
    let source = if edit {
        include_str!("../workflows/minimax_h3_image_edit.json")
    } else {
        include_str!("../workflows/minimax_h3_image_t2i.json")
    };
    serde_json::from_str(source).map_err(|_| malformed())
}
fn base(config: &Value) -> Outcome<String> {
    let b = text(config, "endpointBaseUrl")
        .map_err(|_| fail("INVALID_INPUT", "configuration.endpointBaseUrl is required"))?;
    if !(b.starts_with("http://") || b.starts_with("https://")) || b.ends_with('/') {
        return Err(fail(
            "INVALID_INPUT",
            "endpointBaseUrl must be an absolute http(s) origin without trailing slash",
        ));
    }
    Ok(b.to_string())
}
fn http(
    h: &mut dyn Host,
    method: &str,
    url: String,
    body: Option<&Value>,
    sink: Value,
    budget: u64,
) -> Outcome<Value> {
    let (body, headers) = match body {
        Some(v) => (
            json!({"kind":"bytes","bytes":serde_json::to_vec(v).map_err(|_| malformed())?}),
            vec![json!({"name":"Content-Type","value":"application/json"})],
        ),
        None => (json!({"kind":"none"}), vec![]),
    };
    port(
        h,
        "host-http.execute",
        json!({"plan":{"method":method,"url":url,"headers":headers,"generatedHeaders":[],"body":body,"sink":sink,"timeoutMs":budget},"credential":null}),
    )
}
fn buffered_json(response: &Value) -> Option<Value> {
    let bytes: Vec<u8> = serde_json::from_value(response["payload"]["bytes"].clone()).ok()?;
    serde_json::from_slice(&bytes).ok()
}
fn get_json(h: &mut dyn Host, url: String, budget: u64) -> Outcome<Value> {
    let r = http(
        h,
        "GET",
        url,
        None,
        json!({"kind":"buffer","maxBytes":policy("maxBufferedHttpBytes")}),
        budget,
    )?;
    let status = number(&r, "status")?;
    let v = buffered_json(&r).ok_or_else(malformed)?;
    if status != 200 {
        return Err(Failure {
            code: "UPSTREAM_ERROR",
            message: "ComfyUI rejected the read request",
            vendor: Some(format!("HTTP_{status}")),
            request: None,
        });
    }
    Ok(v)
}
fn probe(h: &mut dyn Host, p: &Value, budget: u64) -> Outcome<Value> {
    match text(p, "kind")? {
        "package_validation" => Ok(
            json!({"packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]}),
        ),
        "connectivity" => {
            let stats = get_json(
                h,
                format!("{}/system_stats", base(&p["configuration"])?),
                budget,
            )?;
            if !stats["system"]["comfyui_version"].is_string() {
                return Err(malformed());
            }
            Ok(
                json!({"packageValid":true,"connection":"ready","authentication":"not_checked","resources":"not_checked","issues":[]}),
            )
        }
        _ => Err(fail("INVALID_INPUT", "Unknown probe kind")),
    }
}
// Official 16MP size table; 2K uses half dimensions rounded to the node's 32px grid.
// Platform dimension mapping belongs to this adapter, never the host or SDK.
fn dimensions(input: &Value) -> Outcome<(u64, u64)> {
    let (width, height) = match input["aspectRatio"].as_str() {
        Some("1:1") => (4096, 4096),
        Some("4:3") => (4736, 3552),
        Some("3:4") => (3552, 4736),
        Some("3:2") => (5024, 3360),
        Some("2:3") => (3360, 5024),
        Some("16:9") => (5472, 3072),
        Some("9:16") => (3072, 5472),
        Some("21:9") => (6272, 2688),
        Some("2.35:1") => (6304, 2688),
        Some("2.39:1") => (6336, 2656),
        _ => return Err(fail("INVALID_INPUT", "Unsupported aspect ratio")),
    };
    match input["resolution"].as_str() {
        Some("2K") => Ok((((width / 2 + 16) / 32) * 32, ((height / 2 + 16) / 32) * 32)),
        Some("4K") => Ok((width, height)),
        _ => Err(fail("INVALID_INPUT", "Unsupported resolution")),
    }
}
fn validate_input(input: &Value, edit: bool) -> Outcome<()> {
    let object = input
        .as_object()
        .ok_or_else(|| fail("INVALID_INPUT", "input must be an object"))?;
    if object.keys().any(|key| {
        !["prompt", "seed", "aspectRatio", "resolution"].contains(&key.as_str())
            && !(edit && key == "referenceImages")
    }) {
        return Err(fail("INVALID_INPUT", "Unknown image input"));
    }
    input["prompt"]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.chars().count() <= MAX_PROMPT)
        .ok_or_else(|| fail("INVALID_INPUT", "prompt must be 1..8000 characters"))?;
    input["seed"]
        .as_u64()
        .filter(|seed| *seed <= MAX_SEED)
        .ok_or_else(|| fail("INVALID_INPUT", "seed must be an integer in 0..2^53-1"))?;
    dimensions(input)?;
    if edit {
        reference_ids(input)?;
    }
    Ok(())
}
fn build_graph(input: &Value, edit: bool) -> Outcome<Value> {
    validate_input(input, edit)?;
    let mut graph = workflow(edit)?;
    graph["4"]["inputs"]["prompt"] = input["prompt"].clone();
    graph["6"]["inputs"]["seed"] = input["seed"].clone();
    let (width, height) = dimensions(input)?;
    graph["4"]["inputs"]["width"] = json!(width);
    graph["4"]["inputs"]["height"] = json!(height);
    if edit && reference_ids(input)?.len() == 1 {
        graph["4"]["inputs"]
            .as_object_mut()
            .ok_or_else(malformed)?
            .remove("images.image_2");
        graph
            .as_object_mut()
            .ok_or_else(malformed)?
            .remove(REFERENCE_NODES[1]);
    }
    Ok(graph)
}
/// Check every node definition and selected model before uploading or submitting.
fn preflight(h: &mut dyn Host, configuration: &Value, graph: &Value, budget: u64) -> Outcome<()> {
    let endpoint = base(configuration)?;
    let stats = get_json(h, format!("{endpoint}/system_stats"), budget)?;
    let version = text(&stats["system"], "comfyui_version")?;
    let numbers: Vec<u32> = version
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|_| fail("RESOURCE_UNAVAILABLE", "Unsupported ComfyUI version"))?;
    if numbers.len() != 3 || numbers.as_slice() < [0, 45, 0].as_slice() {
        return Err(fail(
            "RESOURCE_UNAVAILABLE",
            "ComfyUI 0.45.0 or newer is required",
        ));
    }
    for node in graph.as_object().ok_or_else(malformed)?.values() {
        let class = text(node, "class_type")?;
        let definitions = get_json(h, format!("{endpoint}/object_info/{}", pct(class)), budget)?;
        let definition = definitions
            .get(class)
            .filter(|d| d.is_object())
            .ok_or_else(|| fail("RESOURCE_UNAVAILABLE", "Required ComfyUI node is missing"))?;
        for (key, value) in node["inputs"].as_object().ok_or_else(malformed)? {
            if !key.ends_with("_name") {
                continue;
            }
            let options = definition["input"]["required"][key][0]
                .as_array()
                .ok_or_else(|| {
                    fail(
                        "RESOURCE_UNAVAILABLE",
                        "Required model declaration is missing",
                    )
                })?;
            if !options.contains(value) {
                return Err(fail("RESOURCE_UNAVAILABLE", "Required model is missing"));
            }
        }
    }
    Ok(())
}
fn reference_ids(input: &Value) -> Outcome<Vec<&str>> {
    let refs = input["referenceImages"]
        .as_array()
        .filter(|r| (1..=2).contains(&r.len()))
        .ok_or_else(|| {
            fail(
                "INVALID_INPUT",
                "One or two ordered original images are required",
            )
        })?;
    let mut ids = Vec::new();
    for reference in refs {
        let object = reference
            .as_object()
            .filter(|r| r.len() == 1)
            .ok_or_else(|| fail("INVALID_INPUT", "Reference must contain only mediaId"))?;
        let id = object
            .get("mediaId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| fail("INVALID_INPUT", "Reference mediaId is required"))?;
        if ids.contains(&id) {
            return Err(fail(
                "INVALID_INPUT",
                "Each reference needs its own media identity",
            ));
        }
        ids.push(id);
    }
    Ok(ids)
}
/// Verify the complete lease set before the first upload. Input order alone determines roles.
fn reference_media<'a>(p: &'a Value, ids: &[&str]) -> Outcome<Vec<&'a Value>> {
    let media = p["media"]
        .as_array()
        .filter(|m| m.len() == ids.len())
        .ok_or_else(|| fail("INVALID_INPUT", "All original image leases are required"))?;
    ids.iter()
        .map(|id| {
            let matching: Vec<_> = media
                .iter()
                .filter(|m| m["id"] == *id && m["kind"] == "image")
                .collect();
            if matching.len() != 1 {
                return Err(fail(
                    "INVALID_INPUT",
                    "An original image has no unique matching lease",
                ));
            }
            let item = matching[0];
            for key in ["fileName", "handle", "mime"] {
                if text(item, key)?.is_empty() {
                    return Err(fail(
                        "INVALID_INPUT",
                        "Original lease metadata is incomplete",
                    ));
                }
            }
            Ok(item)
        })
        .collect()
}
fn upload_reference(
    h: &mut dyn Host,
    configuration: &Value,
    media: &Value,
    budget: u64,
) -> Outcome<String> {
    let body = json!({"kind":"multipart","parts":[
        {"kind":"file","name":"image","fileName":text(media,"fileName")?,"handle":text(media,"handle")?,"mime":text(media,"mime")?},
        {"kind":"field","name":"type","value":"input"}]});
    let args = json!({"plan":{"method":"POST","url":format!("{}/upload/image",base(configuration)?),
        "headers":[],"generatedHeaders":[],"body":body,"sink":{"kind":"buffer","maxBytes":policy("maxBufferedHttpBytes")},"timeoutMs":budget},"credential":null});
    let response = port(h, "host-http.execute", args).map_err(|_| {
        fail(
            "ACCEPTANCE_UNKNOWN",
            "Upload response was lost; no retry was attempted",
        )
    })?;
    let value = buffered_json(&response).ok_or_else(malformed)?;
    if number(&response, "status")? != 200 {
        return Err(fail(
            "UPSTREAM_ERROR",
            "ComfyUI rejected the original image upload",
        ));
    }
    let name = text(&value, "name")?;
    if name.is_empty() {
        return Err(malformed());
    }
    Ok(match value.get("subfolder").and_then(Value::as_str) {
        Some(sub) if !sub.is_empty() => format!("{sub}/{name}"),
        _ => name.to_string(),
    })
}
fn persist_accepted(h: &mut dyn Host, request: &Invocation, accepted: &Value) -> Outcome<()> {
    let read = port(
        h,
        "host-checkpoint.read",
        json!({"requestId":request.r#request_id}),
    )?;
    let mut checkpoint: Value =
        serde_json::from_str(text(&read, "valueJson")?).map_err(|_| malformed())?;
    let old = text(&read, "revision")?.to_string();
    let revision = old
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(malformed)?;
    checkpoint["revision"] = json!(revision.to_string());
    checkpoint["state"] = json!("accepted");
    checkpoint["vendorTaskId"] = accepted["vendorTaskId"].clone();
    checkpoint["resumeToken"] = accepted["resumeToken"].clone();
    checkpoint["result"] = accepted.clone();
    port(h, "host-checkpoint.write", json!({"requestId":request.r#request_id,"expectedRevision":old,"valueJson":checkpoint.to_string()})).map_err(|_| Failure {
        code: "ACCEPTANCE_UNKNOWN", message: "ComfyUI accepted the prompt but checkpoint storage failed; do not resubmit",
        vendor: Some("CHECKPOINT_COMMIT_FAILED".into()), request: accepted["vendorTaskId"].as_str().map(str::to_string),
    })?;
    Ok(())
}
fn submit(h: &mut dyn Host, request: &Invocation, p: &Value) -> Outcome<Value> {
    let edit = is_edit(text(p, "operationId")?)?;
    let expected_model = if edit { EDIT_MODEL } else { TEXT_MODEL };
    if text(p, "modelId")? != expected_model {
        return Err(fail("INVALID_INPUT", "Operation and model do not match"));
    }
    validate_input(&p["input"], edit)?;
    let ids = if edit {
        reference_ids(&p["input"])?
    } else {
        vec![]
    };
    let media = reference_media(p, &ids)?;
    // Resolve every lease before the first network side effect.
    for item in &media {
        let info = port(h, "host-media.info", json!({"handle":text(item,"handle")?}))?;
        if info["kind"] != "image"
            || info["byteLength"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
                == 0
        {
            return Err(fail(
                "INVALID_INPUT",
                "Reference image is empty or unavailable",
            ));
        }
    }
    let mut graph = build_graph(&p["input"], edit)?;
    preflight(h, &p["configuration"], &graph, request.r#budget_ms)?;
    for (node, original) in REFERENCE_NODES.iter().zip(media) {
        graph[*node]["inputs"]["image"] = json!(upload_reference(
            h,
            &p["configuration"],
            original,
            request.r#budget_ms
        )?);
    }
    let url = format!("{}/prompt", base(&p["configuration"])?);
    let body = json!({"prompt":graph,"client_id":request.r#request_id});
    // Exactly one POST; any transport loss or unreadable reply is ACCEPTANCE_UNKNOWN (never resubmitted).
    let r = http(
        h,
        "POST",
        url,
        Some(&body),
        json!({"kind":"buffer","maxBytes":policy("maxBufferedHttpBytes")}),
        request.r#budget_ms,
    )
    .map_err(|_| {
        fail(
            "ACCEPTANCE_UNKNOWN",
            "Submit response was lost or interrupted; do not resubmit",
        )
    })?;
    let status = number(&r, "status").map_err(|_| {
        fail(
            "ACCEPTANCE_UNKNOWN",
            "Submit response is incomplete; do not resubmit",
        )
    })?;
    let v = buffered_json(&r).ok_or_else(|| {
        fail(
            "ACCEPTANCE_UNKNOWN",
            "Submit response is not JSON; do not resubmit",
        )
    })?;
    if status != 200 {
        let vendor = v["error"]["type"].as_str().map(str::to_string);
        return Err(Failure {
            code: if (400..500).contains(&status) {
                "INVALID_INPUT"
            } else {
                "ACCEPTANCE_UNKNOWN"
            },
            message: "ComfyUI rejected the prompt",
            vendor,
            request: None,
        });
    }
    let id = v
        .get("prompt_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            fail(
                "ACCEPTANCE_UNKNOWN",
                "Submit reply lacks prompt_id; do not resubmit",
            )
        })?;
    if v.get("node_errors")
        .and_then(Value::as_object)
        .is_some_and(|o| !o.is_empty())
    {
        return Err(Failure {
            code: "ACCEPTANCE_UNKNOWN",
            message: "Submit reply contains a prompt ID and node errors; inspect the known task",
            vendor: Some("NODE_ERRORS_WITH_PROMPT_ID".into()),
            request: Some(id.to_string()),
        });
    }
    let (width, height) = dimensions(&p["input"])?;
    let accepted = json!({"kind":"accepted","vendorTaskId":id,"appliedParams":null,"resumeToken":{"requestId":request.r#request_id,"width":width,"height":height},"usage":null});
    persist_accepted(h, request, &accepted).map_err(|_| Failure {
        code: "ACCEPTANCE_UNKNOWN",
        message: "Accepted prompt checkpoint failed; recover the known task without resubmission",
        vendor: Some("CHECKPOINT_COMMIT_FAILED".into()),
        request: Some(id.to_string()),
    })?;
    Ok(accepted)
}
fn pct(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char)
        } else {
            o.push_str(&format!("%{b:02X}"))
        }
    }
    o
}
fn query_result(
    status: &str,
    outputs: Vec<Value>,
    description: String,
    token: Value,
    vendor: Option<String>,
    details: Value,
) -> Value {
    let failure = match status {
        "failed" => {
            json!({"category":"vendorTaskFailed","message":"The ComfyUI prompt failed","vendorCode":vendor,"details":details})
        }
        _ => Value::Null,
    };
    json!({"status":status,"outputs":outputs,"data":{"description":description},"usage":null,
        "progress":if matches!(status,"queued"|"running"){json!({"fraction":null,"message":status})}else{Value::Null},
        "terminalFailure":failure,"resumeToken":token})
}
fn in_queue(list: &Value, id: &str) -> bool {
    list.as_array().is_some_and(|a| {
        a.iter()
            .any(|e| e.get(1).and_then(Value::as_str) == Some(id))
    })
}
/// PNG signature + IHDR width/height from the real downloaded bytes (not from the file name).
fn png_dims(bytes: &[u8]) -> Option<(u64, u64)> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || bytes[..8] != SIG || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as u64;
    let hgt = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) as u64;
    Some((w, hgt))
}
fn download(
    h: &mut dyn Host,
    base: &str,
    image: &Value,
    budget: u64,
    width: u64,
    height: u64,
) -> Outcome<Value> {
    if image["type"] != "output" {
        return Err(fail(
            "UPSTREAM_ERROR",
            "Selected output is not a saved output image",
        ));
    }
    let name = text(image, "filename")?;
    let sub = image.get("subfolder").and_then(Value::as_str).unwrap_or("");
    let url = format!(
        "{base}/view?filename={}&subfolder={}&type=output",
        pct(name),
        pct(sub)
    );
    let r = http(h, "GET", url, None, json!({"kind":"media"}), budget)?;
    let status = number(&r, "status")?;
    if status != 200 {
        return Err(Failure {
            code: "UPSTREAM_ERROR",
            message: "ComfyUI did not return the output image",
            vendor: Some(format!("HTTP_{status}")),
            request: None,
        });
    }
    let handle = text(&r["payload"], "handle")?.to_string();
    let stat = port(h, "host-media.info", json!({"handle":handle}))?;
    let head: Vec<u8> = serde_json::from_value(port(
        h,
        "host-media.read",
        json!({"handle":handle,"offset":"0","maxBytes":32}),
    )?)
    .map_err(|_| malformed())?;
    let Some((w, hgt)) = png_dims(&head) else {
        return Err(fail(
            "UPSTREAM_ERROR",
            "Downloaded output is not a PNG image",
        ));
    };
    if (w, hgt) != (width, height) {
        return Err(fail(
            "UPSTREAM_ERROR",
            "Downloaded image size differs from the accepted input dimensions",
        ));
    }
    if stat["kind"] != "image" || stat["byteLength"] == "0" {
        return Err(fail(
            "UPSTREAM_ERROR",
            "Downloaded media is not a non-empty image",
        ));
    }
    Ok(
        json!({"kind":"media","role":"artifact","media":{"id":stat["id"],"kind":stat["kind"],"handle":handle,"mime":stat["mime"],"fileName":stat["fileName"],"byteLength":stat["byteLength"]}}),
    )
}
fn query(h: &mut dyn Host, request: &Invocation, p: &Value) -> Outcome<Value> {
    is_edit(text(p, "operationId")?)?;
    let base = base(&p["configuration"])?;
    let id = text(p, "vendorTaskId")?.to_string();
    let budget = request.r#budget_ms;
    let history = get_json(h, format!("{base}/history/{}", pct(&id)), budget)?;
    let token = p["resumeToken"].clone();
    let Some(entry) = history.get(&id) else {
        let q = get_json(h, format!("{base}/queue"), budget)?;
        let status = if in_queue(&q["queue_running"], &id) {
            "running"
        } else if in_queue(&q["queue_pending"], &id) {
            "queued"
        } else {
            return Err(Failure {
                code: "UPSTREAM_ERROR",
                message: "Prompt is neither in history nor in the queue",
                vendor: Some("PROMPT_NOT_FOUND".into()),
                request: Some(id),
            });
        };
        return Ok(query_result(
            status,
            vec![],
            format!("ComfyUI prompt {id}: {status}"),
            token,
            None,
            json!({"promptId":id}),
        ));
    };
    let st = text(&entry["status"], "status_str")?;
    match st {
        "success" => {
            if entry["status"]["completed"] != true {
                return Err(malformed());
            }
            let images = entry["outputs"][OUTPUT_NODE]["images"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if images.len() != 1 {
                return Err(Failure {
                    code: "UPSTREAM_ERROR",
                    message: "Prompt completed without exactly one image on the frozen output node",
                    vendor: Some("OUTPUT_MISSING".into()),
                    request: Some(id),
                });
            }
            let media = download(
                h,
                &base,
                &images[0],
                budget,
                number(&token, "width")?,
                number(&token, "height")?,
            )?;
            Ok(query_result(
                "succeeded",
                vec![media],
                format!("ComfyUI prompt {id}: success"),
                token,
                None,
                json!({"promptId":id,"outputNode":OUTPUT_NODE}),
            ))
        }
        "error" => {
            let vendor = entry["status"]["messages"]
                .as_array()
                .and_then(|m| {
                    m.iter()
                        .find(|e| e.get(0) == Some(&json!("execution_error")))
                })
                .and_then(|e| e[1]["exception_type"].as_str())
                .map(str::to_string)
                .or(Some("execution_error".to_string()));
            Ok(query_result(
                "failed",
                vec![],
                format!("ComfyUI prompt {id}: error"),
                token,
                vendor,
                json!({"promptId":id}),
            ))
        }
        _ => Err(malformed()),
    }
}
fn finish(request: &Invocation, result: Outcome<Value>) -> Response {
    match result {
        Ok(value) => guest::completed_response(request, &value).expect("standard serializable result"),
        Err(e) => serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,"error":{"code":e.code,"message":e.message,"retryable":false,"vendorCode":e.vendor,"vendorRequestId":e.request}})).expect("public error"),
    }
}
pub struct H3Image;
impl Adapter for H3Image {
    fn invoke(request: Invocation, h: &mut dyn Host) -> Response {
        let value = serde_json::to_value(&request).expect("typed invocation");
        let result = (|| {
            let p: Value = serde_json::from_str(&request.r#payload_json)
                .map_err(|_| fail("INVALID_INPUT", "Payload is not JSON"))?;
            match text(&value, "operation")? {
                "capabilities" => Ok(capabilities()),
                "probe" => probe(h, &p, request.r#budget_ms),
                "submit" => submit(h, &request, &p),
                "query" => query(h, &request, &p),
                _ => Err(fail("UNSUPPORTED_FEATURE", "Only capabilities, probe, submit and query are implemented; remote cancel is unsupported")),
            }
        })();
        finish(&request, result)
    }
}
bgx_adapter_v2_sdk::export_adapter!(H3Image);

#[cfg(test)]
mod tests;
