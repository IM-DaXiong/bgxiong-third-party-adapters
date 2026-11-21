#![cfg_attr(target_arch = "wasm32", no_std)]

#[macro_use]
extern crate alloc;

use alloc::{
    borrow::ToOwned,
    string::String,
    vec::Vec,
};

mod parameters;
mod results;

use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::{guest, host, Adapter, Host, Invocation, Response};
use parameters::{Invalid, Parameters};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PLUGIN: &str = "local.bgx.seedream-55-image";
const MODEL: &str = "doubao-seedream-5-5-261028";
const GENERATE: &str = "local.bgx.seedream-55-image/generate";
const EDIT: &str = "local.bgx.seedream-55-image/edit";
const OP_VERSION: u64 = 1;
const API_ORIGIN: &str = "https://ark.cn-beijing.volces.com";
const ENDPOINT: &str = "https://ark.cn-beijing.volces.com/api/v3/images/generations";
const CREDENTIAL: &str = "seedream-55-image-user-key";
const CONFIG: &str = r#"{"type":"object","properties":{"endpointBaseUrl":{"type":"string","const":"https://ark.cn-beijing.volces.com"},"credentialRef":{"type":"string","const":"seedream-55-image-user-key"}},"required":["credentialRef"],"additionalProperties":false}"#;
const CHUNK: usize = 48 * 1024;
// One response fits comfortably in the SDK frame even as a JSON byte array.
const RESULT_READ_CHUNK: usize = 256 * 1024;
const MAX_REFERENCE_BYTES: usize = 30_000_000;
const MAX_REQUEST_BYTES: usize = 401_100_000;

#[derive(Debug)]
struct Failure {
    code: String,
    message: &'static str,
    vendor: Option<String>,
    request: Option<String>,
}

fn fail(code: &str, message: &'static str) -> Failure {
    Failure {
        code: code.to_owned(),
        message,
        vendor: None,
        request: None,
    }
}

fn malformed() -> Failure {
    fail("UPSTREAM_ERROR", "方舟返回的图片响应无效")
}

impl From<Invalid> for Failure {
    fn from(error: Invalid) -> Self {
        fail("INVALID_INPUT", error.1)
    }
}

fn port(host: &mut dyn Host, method: &str, args: Value) -> Result<Value, Failure> {
    host::call(host, method, &args).map_err(host_failure)
}

fn host_failure(error: host::HostError) -> Failure {
    let code = serde_json::to_value(&error).unwrap_or(Value::Null);
    fail(code["code"].as_str().unwrap_or("UPSTREAM_ERROR"), "适配器宿主操作失败")
}

fn result_stage(host: &mut dyn Host, code: &str, message: &str) {
    let _ = host::call::<_, ()>(host, "host-log.log", &json!({
        "level":"info","code":code,"message":message,
    }));
}

fn schema(id: &str, content: &str) -> Value {
    let normalized = content.replace("\r\n", "\n");
    let digest = format!("{:x}", Sha256::digest(normalized.as_bytes()));
    json!({"id":id,"digest":digest,"schemaJson":normalized})
}

fn control(pointer: &str, widget: &str, label: &str, group: &str, role: Option<&str>) -> Value {
    let mut value = json!({
        "fieldPointer":pointer,"widget":widget,"label":label,"description":"","group":group,
    });
    if let Some(role) = role {
        value["displayRole"] = json!(role);
    }
    value
}

fn capability(edit: bool) -> Value {
    let input = schema(
        "input",
        if edit {
            include_str!("../schemas/edit-input.json")
        } else {
            include_str!("../schemas/input.json")
        },
    );
    let config = schema("config", CONFIG);
    let output = schema("output", include_str!("../schemas/output.json"));
    let mut controls = vec![
        control("/prompt", "multiline", "提示词", "输入", Some("prompt")),
        control("/size", "text", "输出规格", "参数", Some("size")),
        control(
            "/resolution",
            "select",
            "清晰度",
            "参数",
            Some("resolution"),
        ),
        control(
            "/aspectRatio",
            "select",
            "画幅",
            "参数",
            Some("aspectRatio"),
        ),
        control("/responseFormat", "select", "响应格式", "参数", None),
        control("/outputFormat", "select", "输出格式", "参数", None),
        control("/background", "select", "背景", "参数", None),
        control("/watermark", "toggle", "水印", "参数", None),
    ];
    let slots = if edit {
        controls.push(control(
            "/referenceImages",
            "media",
            "参考图",
            "输入",
            Some("referenceImage"),
        ));
        json!([{"role":"reference","inputPointer":"/referenceImages","mediaKind":"image","minCount":1,"maxCount":10}])
    } else {
        json!([])
    };
    json!({
        "schemaVersion":2,"localId":if edit {"image-edit"} else {"image"},
        "capabilityKey":"image.generate","transportSlot":"image","modelIds":[MODEL],
        "configurationSchema":{"id":"config","digest":config["digest"]},"schemas":[input,config,output],
        "operations":[{
            "id":if edit {EDIT} else {GENERATE},"version":OP_VERSION,
            "label":if edit {"Seedream 5.5 参考图生图"} else {"Seedream 5.5 文生图"},
            "inputProfile":"operation-input-v1",
            "inputSchema":{"id":"input","digest":input["digest"]},
            "outputSchema":{"id":"output","digest":output["digest"]},"resumeSchema":null,
            "uiSchema":{"controls":controls},"mediaSlots":slots,
            "outputSlots":[
                {"role":"artifact","kind":"image","minCount":1,"maxCount":null},
                {"role":"provider-response","kind":"file","minCount":1,"maxCount":1}
            ],
            "execution":{"mode":"synchronous","streaming":true,"cancellation":"unsupported","resumable":false,"remoteIdempotency":false}
        }]
    })
}

fn capabilities() -> Value {
    json!({
        "schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),
        "capabilityContracts":[capability(false),capability(true)],
    })
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, Failure> {
    value[key].as_str().ok_or_else(malformed)
}

fn media_len(info: &Value, limit: usize) -> Result<usize, Failure> {
    let length = text(info, "byteLength")?
        .parse::<usize>()
        .map_err(|_| malformed())?;
    if length > limit {
        return Err(fail("RESOURCE_UNAVAILABLE", "媒体文件超出适配包字节预算"));
    }
    Ok(length)
}

fn read_media(host: &mut dyn Host, handle: &str, size: usize) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::with_capacity(size);
    while bytes.len() < size {
        let count = RESULT_READ_CHUNK.min(size - bytes.len());
        let part = host.read_media(handle, bytes.len() as u64, count as u32).map_err(host_failure)?;
        if part.is_empty() || part.len() > count {
            return Err(malformed());
        }
        bytes.extend_from_slice(&part);
    }
    Ok(bytes)
}

fn create_media(host: &mut dyn Host, kind: &str, mime: &str) -> Result<String, Failure> {
    port(
        host,
        "host-media.create",
        json!({"kind":kind,"mimeHint":mime}),
    )?
    .as_str()
    .map(str::to_owned)
    .ok_or_else(malformed)
}

fn append_media(host: &mut dyn Host, handle: &str, bytes: &[u8]) -> Result<(), Failure> {
    for chunk in bytes.chunks(CHUNK) {
        host.write_media(handle, chunk).map_err(host_failure)?;
    }
    Ok(())
}

fn finish_media(host: &mut dyn Host, handle: &str) -> Result<(), Failure> {
    port(host, "host-media.finish", json!({"handle":handle}))?;
    Ok(())
}

fn reference_mime(bytes: &[u8], info: &Value) -> Result<&'static str, Failure> {
    let invalid = || fail("INVALID_INPUT", "参考图片格式或尺寸无效");
    let mime = infer::get(bytes).ok_or_else(invalid)?.mime_type();
    if !matches!(
        mime,
        "image/png"
            | "image/jpeg"
            | "image/webp"
            | "image/bmp"
            | "image/tiff"
            | "image/gif"
            | "image/heif"
            | "image/heic"
    ) {
        return Err(invalid());
    }
    // The media broker measures dimensions by decoding the leased original.
    let w = info["width"].as_u64().ok_or_else(invalid)?;
    let h = info["height"].as_u64().ok_or_else(invalid)?;
    if w <= 14 || h <= 14 || w.saturating_mul(h) > 36_000_000 || w > h * 16 || h > w * 16 {
        return Err(invalid());
    }
    Ok(mime)
}

fn request_body(
    host: &mut dyn Host,
    params: &Parameters<'_>,
    media: &Value,
) -> Result<String, Failure> {
    let missing = || fail("INVALID_INPUT", "每张参考图必须匹配唯一的原图租约");
    let media = media
        .as_array()
        .filter(|media| media.len() == params.references.len())
        .ok_or_else(missing)?;
    let body = json!({
        "model":MODEL,"prompt":params.prompt,"size":params.size,"stream":false,
        "response_format":params.response_format,"output_format":params.output_format,
        "watermark":params.watermark,"background":params.background,
    });
    let mut prefix = serde_json::to_vec(&body).map_err(|_| malformed())?;
    let mut refs = vec![];
    let mut upper_bound = prefix.len() + 16;
    for id in &params.references {
        let mut matches = media.iter().filter(|m| m["id"] == *id);
        let lease = matches
            .next()
            .filter(|m| m["kind"] == "image")
            .ok_or_else(missing)?;
        if matches.next().is_some() {
            return Err(missing());
        }
        let handle = text(lease, "handle")?;
        let info = port(host, "host-media.info", json!({"handle":handle}))?;
        if info["kind"] != "image" || info["id"] != *id || info["mime"] != lease["mime"] {
            return Err(missing());
        }
        let length = media_len(&info, MAX_REFERENCE_BYTES)?;
        upper_bound += length.div_ceil(3) * 4 + 64;
        if upper_bound > MAX_REQUEST_BYTES {
            return Err(fail("RESOURCE_UNAVAILABLE", "参考图请求超出总字节预算"));
        }
        refs.push((handle, text(lease, "mime")?, length, info));
    }

    let body_handle = create_media(host, "file", "application/octet-stream")?;
    if refs.is_empty() {
        append_media(host, &body_handle, &prefix)?;
    } else {
        prefix.pop();
        prefix.extend_from_slice(br#","image":"#);
        let multiple = refs.len() > 1;
        if multiple {
            prefix.push(b'[');
        }
        append_media(host, &body_handle, &prefix)?;
        for (index, (handle, expected, length, info)) in refs.into_iter().enumerate() {
            let bytes = read_media(host, handle, length)?;
            let detected = reference_mime(&bytes, &info)?;
            if expected != detected
                && !matches!(
                    (expected, detected),
                    ("image/heif", "image/heic") | ("image/heic", "image/heif")
                )
            {
                return Err(missing());
            }
            let header = format!(
                "{}\"data:{detected};base64,",
                if index == 0 { "" } else { "," }
            );
            append_media(host, &body_handle, header.as_bytes())?;
            // CHUNK is a multiple of three. Only the final chunk can contain padding.
            for chunk in bytes.chunks(CHUNK) {
                append_media(host, &body_handle, STANDARD.encode(chunk).as_bytes())?;
            }
            append_media(host, &body_handle, b"\"")?;
        }
        append_media(host, &body_handle, if multiple { b"]}" } else { b"}" })?;
    }
    finish_media(host, &body_handle)?;
    Ok(body_handle)
}

fn http(
    host: &mut dyn Host,
    method: &str,
    url: &str,
    body: Value,
    budget: u64,
) -> Result<Value, Failure> {
    let submit = method == "POST";
    port(
        host,
        "host-http.execute",
        json!({
            "plan":{
                "method":method,"url":url,
                "headers":if submit {json!([{"name":"Content-Type","value":"application/json"}])} else {json!([])},
                "generatedHeaders":if submit {json!([{"name":"Authorization","kind":"bearer","targetField":"Authorization"}])} else {json!([])},
                "body":body,"sink":{"kind":"media"},"timeoutMs":budget,
            },
            "credential":submit.then_some(CREDENTIAL),
        }),
    )
}

fn submit(host: &mut dyn Host, request: &Invocation, payload: &Value) -> Result<Value, Failure> {
    let parameters = Parameters::parse(payload, &payload["input"])?;
    if payload["preparation"]["material"] != parameters.material() {
        return Err(fail("INVALID_INPUT", "提交参数与预检结果不一致"));
    }
    if payload["configuration"]["endpointBaseUrl"] != API_ORIGIN
        || payload["configuration"]["credentialRef"] != CREDENTIAL
    {
        return Err(fail("UNAUTHORIZED", "请绑定北京火山方舟账号"));
    }
    result_stage(host, "SEEDREAM_PREPARE", "Preparing generation request body");
    let handle = request_body(host, &parameters, &payload["media"]).map_err(|mut error| {
        if error.code != "INVALID_INPUT" {
            error.code = "RESOURCE_UNAVAILABLE".into();
            error.message = "本地参考图或生图请求体处理失败";
        }
        error
    })?;
    result_stage(host, "SEEDREAM_REQUEST_READY", "Generation request body prepared");
    let response = http(
        host,
        "POST",
        ENDPOINT,
        json!({"kind":"media","handle":handle}),
        0,
    )
    .map_err(|error| match error.code.as_str() {
        "INVALID_INPUT" | "UNAUTHORIZED" | "PERMISSION_DENIED" | "UNSUPPORTED_FEATURE" => error,
        _ => fail("ACCEPTANCE_UNKNOWN", "方舟请求受理状态未知，请核对原任务"),
    })?;
    result_stage(host, "SEEDREAM_RESPONSE", "Generation response received");
    results::collect(host, request, &parameters, &payload["input"], &response)
}

pub struct Seedream;

impl Adapter for Seedream {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        let result = (|| {
            let payload: Value = serde_json::from_str(&request.r#payload_json)
                .map_err(|_| fail("INVALID_INPUT", "操作参数不是合法 JSON"))?;
            let operation = serde_json::to_value(&request.r#operation).expect("operation");
            match operation.as_str() {
                Some("capabilities") => Ok(capabilities()),
                Some("probe") if payload["kind"] == "package_validation" => Ok(json!({
                    "packageValid":true,"connection":"not_checked","authentication":"not_checked",
                    "resources":"not_checked","issues":[],
                })),
                Some("probe") if payload["kind"] == "operation_input_preflight" => {
                    Ok(parameters::preflight(&payload))
                }
                Some("submit") => submit(host, &request, &payload),
                _ => Err(fail("UNSUPPORTED_FEATURE", "此操作只支持同步生成")),
            }
        })();
        match result {
            Ok(value) => guest::completed_response(&request, &value).expect("response"),
            Err(error) => serde_json::from_value(json!({
                "protocolVersion":2,"operation":request.r#operation,
                "error":{"code":error.code,"message":error.message,"retryable":false,
                    "vendorCode":error.vendor,"vendorRequestId":error.request},
            }))
            .expect("error response"),
        }
    }
}

bgx_adapter_v2_sdk::export_adapter!(Seedream);

#[cfg(test)]
mod tests;
