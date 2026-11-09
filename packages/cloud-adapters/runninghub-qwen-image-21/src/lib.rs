#![cfg_attr(target_arch = "wasm32", no_std)]
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

pub const PLUGIN: &str = "local.thirdparty.runninghub-qwen-image-21";

type Result<T> = core::result::Result<T, Failure>;
#[derive(Debug)]
struct Failure {
    code: &'static str,
    message: String,
    vendor: Option<String>,
    request: Option<String>,
    retryable: bool,
}
fn fail(code: &'static str, message: impl Into<String>) -> Failure {
    Failure {
        code,
        message: message.into(),
        vendor: None,
        request: None,
        retryable: false,
    }
}
fn invalid(s: &str) -> Failure {
    fail("INVALID_INPUT", s)
}
fn malformed() -> Failure {
    fail(
        "UPSTREAM_ERROR",
        "RunningHub返回不完整或未知结构；未自动重试",
    )
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().ok_or_else(malformed)
}
fn port(h: &mut dyn Host, m: &str, a: Value) -> Result<Value> {
    let value = host::call(h, m, &a).map_err(|e| {
        let v = serde_json::to_value(&e).expect("public error");
        let code = match v["code"].as_str() {
            Some("PERMISSION_DENIED") => "PERMISSION_DENIED",
            Some("TIMEOUT") => "TIMEOUT",
            Some("BUDGET_EXCEEDED") => "BUDGET_EXCEEDED",
            _ => "RESOURCE_UNAVAILABLE",
        };
        fail(code, format!("受管端口{m}失败：{}", host::diagnostic(&e)))
    })?;
    // Decode every consumed managed result against the frozen public SDK DTO.
    // A mock missing required facts must fail here just like a real host reply.
    macro_rules! checked {
        ($dto:ty) => {{
            let typed: $dto = serde_json::from_value(value)
                .map_err(|_| fail("INVALID_INPUT", format!("受管端口{m}返回违反公开SDK合同")))?;
            Ok(serde_json::to_value(typed).expect("public managed result"))
        }};
    }
    match m {
        "host-media.info" => checked!(bgx_adapter_v2_sdk::HostPortCallV2MediaStat),
        "host-http.execute" => checked!(bgx_adapter_v2_sdk::HostPortCallV2HttpResponse),
        "host-checkpoint.read" => {
            checked!(Option<bgx_adapter_v2_sdk::HostPortCallV2CheckpointRead>)
        }
        "host-checkpoint.write" => checked!(bgx_adapter_v2_sdk::HostPortCallV2CheckpointWrite),
        _ => Err(invalid("本包未声明的受管端口")),
    }
}
fn schema(id: &str, s: &str) -> Value {
    let normalized = s.replace("\r\n", "\n");
    let s = normalized.as_str();
    let d: String = Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    json!({"id":id,"digest":d,"schemaJson":s})
}

fn http(
    h: &mut dyn Host,
    r: &Invocation,
    c: &Value,
    method: &str,
    url: String,
    body: Value,
    download: bool,
) -> Result<Value> {
    let response = port(
        h,
        "host-http.execute",
        json!({"plan":{"method":method,"url":url,"headers":if body["kind"]=="bytes"{json!([{"name":"Content-Type","value":"application/json"}])}else{json!([])},"generatedHeaders":if download{json!([])}else{json!([{"name":"Authorization","kind":"bearer","targetField":"Authorization"}])},"body":body,"sink":if download{json!({"kind":"media"})}else{json!({"kind":"buffer","maxBytes":1048576})},"timeoutMs":r.r#budget_ms},"credential":if download{Value::Null}else{c["credentialRef"].clone()}}),
    )?;
    let expected = if download { "media" } else { "buffered" };
    if response["payload"]["kind"] != expected {
        return Err(fail("INVALID_INPUT", "受管HTTP回执类型与声明sink不一致"));
    }
    Ok(response)
}
fn json_body(v: &Value) -> Result<Value> {
    let b = serde_json::to_vec(v).map_err(|_| invalid("请求JSON无效"))?;
    if b.len() > 1048576 {
        return Err(fail(
            "BUDGET_EXCEEDED",
            "请求超过公开SDK的1MiB缓冲预算，未截断提示词",
        ));
    }
    Ok(json!({"kind":"bytes","bytes":b}))
}

fn persist(h: &mut dyn Host, r: &Invocation, a: &Value) -> Result<()> {
    let read = port(
        h,
        "host-checkpoint.read",
        json!({"requestId":r.r#request_id}),
    )?;
    let rev = text(&read, "revision")?;
    let next = rev
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(malformed)?;
    let mut v: Value = serde_json::from_str(text(&read, "valueJson")?).map_err(|_| malformed())?;
    v["revision"] = json!(next.to_string());
    v["state"] = json!("accepted");
    v["vendorTaskId"] = a["vendorTaskId"].clone();
    v["resumeToken"] = a["resumeToken"].clone();
    v["result"] = a.clone();
    let receipt = port(
        h,
        "host-checkpoint.write",
        json!({"requestId":r.r#request_id,"expectedRevision":rev,"valueJson":v.to_string()}),
    )?;
    if receipt["revision"] != next.to_string() {
        return Err(invalid("快照写入回执版本与请求不一致"));
    }
    Ok(())
}
/// Only the independent adapter knows this failure happened before the billable request.
fn reject_before_submission(
    h: &mut dyn Host,
    r: &Invocation,
    reason: &str,
    evidence: Value,
) -> Result<()> {
    let read = port(
        h,
        "host-checkpoint.read",
        json!({"requestId":r.r#request_id}),
    )?;
    let checkpoint: Value =
        serde_json::from_str(text(&read, "valueJson")?).map_err(|_| malformed())?;
    let candidate = bgx_adapter_v2_sdk::submission_rejection::checkpoint_candidate(
        &checkpoint,
        "before_submission",
        reason,
        &evidence,
    )
    .map_err(invalid)?;
    let receipt = port(
        h,
        "host-checkpoint.write",
        json!({"requestId":r.r#request_id,
        "expectedRevision":read["revision"],"valueJson":candidate.to_string()}),
    )?;
    if receipt["revision"] != candidate["revision"] {
        return Err(invalid("未提交事实的保存回执不一致"));
    }
    Ok(())
}

fn usage(v: &Value) -> Value {
    let mut m = Vec::new();
    for (key, metric, unit) in [
        ("consumeCoins", "credits", "RH_coin"),
        ("consumeMoney", "money", "vendor_currency"),
        (
            "thirdPartyConsumeMoney",
            "third_party_money",
            "vendor_currency",
        ),
        ("taskCostTime", "task_time", "vendor_reported_time_unit"),
    ] {
        let raw = &v["usage"][key];
        let amount = if let Some(s) = raw.as_str() {
            s.to_string()
        } else if raw.is_number() {
            raw.to_string()
        } else {
            continue;
        };
        let mut parts = amount.split('.');
        let w = parts.next().unwrap_or("");
        let f = parts.next();
        if w.is_empty()
            || !w.bytes().all(|b| b.is_ascii_digit())
            || (w.len() > 1 && w.starts_with('0'))
            || parts.next().is_some()
            || f.is_some_and(|x| x.is_empty() || !x.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        m.push(json!({"metric":format!("{PLUGIN}/{metric}"),"unit":format!("{PLUGIN}/{unit}"),"amount":amount,"source":"reported"}));
    }
    if m.is_empty() {
        Value::Null
    } else {
        json!({"measurements":m})
    }
}

pub struct RunningHubQwen;
impl Adapter for RunningHubQwen {
    fn invoke(r: Invocation, h: &mut dyn Host) -> Response {
        let result = (|| {
            let p: Value =
                serde_json::from_str(&r.r#payload_json).map_err(|_| invalid("请求不是JSON"))?;
            match serde_json::to_value(&r.r#operation)
                .unwrap()
                .as_str()
                .unwrap()
            {
                "capabilities" => Ok(capabilities()),
                "probe" => match p["kind"].as_str() {
                    Some("package_validation") => Ok(
                        json!({"packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]}),
                    ),
                    Some("connectivity") => Err(fail(
                        "UNSUPPORTED_FEATURE",
                        "不以计费生成冒充连通探测，请由用户手动测试",
                    )),
                    _ => Err(invalid("未知probe类型")),
                },
                "submit" => submit(h, &r, &p),
                "query" => query(h, &r, &p),
                "cancel" => {
                    workflow(&p)?;
                    Ok(json!({"outcome":"unsupported"}))
                }
                "upload" => Err(fail(
                    "UNSUPPORTED_FEATURE",
                    "本包随submit上传原件，不提供独立uploadRef",
                )),
                _ => Err(fail("UNSUPPORTED_FEATURE", "不支持的操作")),
            }
        })();
        match result{Ok(v)=>guest::completed_response(&r,&v).expect("public result"),Err(e)=>serde_json::from_value(json!({"protocolVersion":2,"operation":r.r#operation,"error":{"code":e.code,"message":e.message,"retryable":e.retryable,"vendorCode":e.vendor,"vendorRequestId":e.request}})).expect("public error")}
    }
}
bgx_adapter_v2_sdk::export_adapter!(RunningHubQwen);

const API: &str = "https://www.runninghub.cn";
const EDIT_GRAPH: &str = include_str!("edit-api.json");
const MAX_PROMPT_BYTES: usize = 32768;
const MAX_SEED: u64 = 9007199254740991;

const RATIOS: [(&str, &str, u64, u64); 8] = [
    ("1:1", "1:1 (Square)", 1, 1),
    ("2:3", "2:3 (Portrait Photo)", 2, 3),
    ("3:2", "3:2 (Photo)", 3, 2),
    ("3:4", "3:4 (Portrait Standard)", 3, 4),
    ("4:3", "4:3 (Standard)", 4, 3),
    ("9:16", "9:16 (Portrait Widescreen)", 9, 16),
    ("16:9", "16:9 (Widescreen)", 16, 9),
    ("21:9", "21:9 (Ultrawide)", 21, 9),
];

#[derive(Clone, Copy)]
struct Workflow {
    name: &'static str,
    label: &'static str,
    id: &'static str,
    edit: bool,
}
const TEXT: Workflow = Workflow {
    name: "text-to-image",
    label: "千问 Image 2.1 文生图",
    id: "2119749068753825793",
    edit: false,
};
const EDIT: Workflow = Workflow {
    name: "image-edit",
    label: "千问 Image 2.1 多参考图编辑",
    id: "2119750262788284418",
    edit: true,
};

impl Workflow {
    fn operation(self) -> String {
        format!("{PLUGIN}/{}", self.name)
    }

    fn model(self) -> String {
        format!("runninghub-qwen-image-21-{}", self.name)
    }

    fn input_schema(self) -> Value {
        let ratios: Vec<_> = RATIOS.iter().map(|ratio| ratio.0).collect();
        let mut spec = json!({
            "type":"object","additionalProperties":false,"required":["prompt"],
            "properties":{
                "prompt":{"type":"string","minLength":1,"maxLength":MAX_PROMPT_BYTES},
                "negativePrompt":{"type":"string","maxLength":MAX_PROMPT_BYTES,"default":""},
                "aspectRatio":{"type":"string","enum":ratios,"default":"1:1"},
                "megapixels":{"type":"number","minimum":0.1,"maximum":4.0,"default":1.0},
                "seed":{"type":"integer","minimum":-1,"maximum":MAX_SEED,"default":-1}
            }
        });
        if self.edit {
            spec["required"] = json!(["prompt", "referenceImages"]);
            spec["properties"]["referenceImages"] = json!({
                "type":"array","minItems":1,"maxItems":10,
                "items":{"type":"object","additionalProperties":false,"required":["mediaId"],
                    "properties":{"mediaId":{"type":"string","minLength":1}}}
            });
        }
        schema("input", &spec.to_string())
    }

    fn contract(self) -> Value {
        let input = self.input_schema();
        let config = schema(
            "configuration",
            r#"{"type":"object","additionalProperties":false,"required":["credentialRef"],"properties":{"credentialRef":{"type":"string","minLength":1}}}"#,
        );
        let output = schema(
            "output",
            r#"{"type":"object","additionalProperties":false,"required":["workflowId","remoteTaskId"],"properties":{"workflowId":{"type":"string"},"remoteTaskId":{"type":"string"}}}"#,
        );
        let resume = schema(
            "resume",
            r#"{"type":"object","additionalProperties":false,"required":["workflowId","taskId"],"properties":{"workflowId":{"type":"string"},"taskId":{"type":"string"}}}"#,
        );
        let mut controls: Vec<_> = [
            ("prompt", "multiline", "提示词", "由工作台原样提交"),
            ("negativePrompt", "multiline", "负向提示词", "可留空"),
            ("aspectRatio", "select", "画幅比例", "决定输出宽高比"),
            (
                "megapixels",
                "number",
                "像素量（MP）",
                "范围 0.1～4，默认 1",
            ),
            ("seed", "number", "随机种子", "-1 由平台随机分配"),
        ]
        .iter()
        .map(|(field, widget, label, description)| {
            json!({
                "fieldPointer":format!("/{field}"),"widget":widget,"label":label,
                "description":description,"group":"生图"
            })
        })
        .collect();
        let media = if self.edit {
            controls.push(
                json!({"fieldPointer":"/referenceImages","widget":"media","label":"有序参考图",
                "description":"按工作台顺序上传 1～10 张","group":"生图"}),
            );
            json!([{"role":"reference_image","inputPointer":"/referenceImages","mediaKind":"image","minCount":1,"maxCount":10}])
        } else {
            json!([])
        };
        json!({
            "schemaVersion":2,"localId":self.name,"capabilityKey":"image.generate",
            "transportSlot":"image","modelIds":[self.model()],
            "configurationSchema":{"id":"configuration","digest":config["digest"]},
            "schemas":[input,config,output,resume],
            "operations":[{
                "id":self.operation(),"version":1,"label":self.label,
                "inputSchema":{"id":"input","digest":input["digest"]},
                "outputSchema":{"id":"output","digest":output["digest"]},
                "resumeSchema":{"id":"resume","digest":resume["digest"]},
                "uiSchema":{"controls":controls},"mediaSlots":media,
                "outputSlots":[{"role":"artifact","kind":"image","minCount":1,"maxCount":1}],
                "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported",
                    "resumable":true,"remoteIdempotency":false}
            }]
        })
    }
}

pub fn capabilities() -> Value {
    json!({"schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),
        "capabilityContracts":[TEXT.contract(),EDIT.contract()]})
}

fn workflow(p: &Value) -> Result<Workflow> {
    [TEXT, EDIT]
        .into_iter()
        .find(|w| p["modelId"] == w.model() && p["operationId"] == w.operation())
        .filter(|_| {
            p["schemaVersion"] == 2
                && p["capabilityKey"] == "image.generate"
                && p["operationVersion"] == 1
        })
        .ok_or_else(|| invalid("模型、能力或操作身份不匹配"))
}

struct Input<'a> {
    prompt: &'a str,
    negative: &'a str,
    ratio: &'static (&'static str, &'static str, u64, u64),
    megapixels: f64,
    seed: Option<u64>,
}
impl<'a> Input<'a> {
    fn parse(v: &'a Value) -> Result<Self> {
        let prompt = v["prompt"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= MAX_PROMPT_BYTES)
            .ok_or_else(|| invalid("提示词为空或超过 32768 字节"))?;
        let negative = v
            .get("negativePrompt")
            .map_or(Some(""), Value::as_str)
            .filter(|s| s.len() <= MAX_PROMPT_BYTES)
            .ok_or_else(|| invalid("负向提示词无效"))?;
        let ratio = v
            .get("aspectRatio")
            .map_or(Some("1:1"), Value::as_str)
            .and_then(|s| RATIOS.iter().find(|ratio| ratio.0 == s))
            .ok_or_else(|| invalid("不支持的画幅比例"))?;
        let megapixels = v
            .get("megapixels")
            .map_or(Some(1.0), Value::as_f64)
            .filter(|n| (0.1..=4.0).contains(n))
            .ok_or_else(|| invalid("像素量须在 0.1～4 MP"))?;
        let seed = match v.get("seed") {
            None => None,
            Some(n) if n.as_i64() == Some(-1) => None,
            Some(n) => Some(
                n.as_u64()
                    .filter(|s| *s <= MAX_SEED)
                    .ok_or_else(|| invalid("随机种子超出范围"))?,
            ),
        };
        Ok(Self {
            prompt,
            negative,
            ratio,
            megapixels,
            seed,
        })
    }

    fn dimensions(&self) -> (u64, u64) {
        let pixels = (self.megapixels * 1048576.0) as u64;
        let (_, _, x, y) = *self.ratio;
        let align = |n: u64| ((n.isqrt() + 16) / 32) * 32;
        (align(pixels * x / y), align(pixels * y / x))
    }
}

fn node(node_id: &str, field: &str, value: &str) -> Value {
    json!({"nodeId":node_id,"fieldName":field,"fieldValue":value})
}

fn text_body(input: &Input<'_>) -> Value {
    let mut nodes = vec![
        node("459:474", "on_false", input.prompt),
        node("459:471", "prompt", input.prompt),
        node("459:452", "negative_prompt", input.negative),
        node("13", "aspect_ratio", input.ratio.1),
        node("13", "megapixels", &input.megapixels.to_string()),
    ];
    if let Some(seed) = input.seed {
        nodes.push(node("459:458", "seed", &seed.to_string()));
        nodes.push(node("459:471", "sampling_mode.seed", &seed.to_string()));
    }
    json!({"addMetadata":false,"instanceType":"default","usePersonalQueue":false,"nodeInfoList":nodes})
}

fn edit_body(input: &Input<'_>, images: &[String]) -> Value {
    let mut graph: Value = serde_json::from_str(EDIT_GRAPH).expect("package graph");
    let (width, height) = input.dimensions();
    graph["456"]["inputs"]["width"] = json!(width);
    graph["456"]["inputs"]["height"] = json!(height);
    graph["474"]["inputs"]["prompt"] = json!(input.prompt);
    graph["474"]["inputs"]["negative_prompt"] = json!(input.negative);
    for (index, filename) in images.iter().enumerate() {
        let id = (4700 + index).to_string();
        graph[&id] = json!({"class_type":"LoadImage","inputs":{"image":filename}});
        graph["474"]["inputs"][format!("images.image_{}", index + 1)] = json!([id, 0]);
    }
    let mut nodes = Vec::new();
    if let Some(seed) = input.seed {
        graph["458"]["inputs"]["seed"] = json!(seed);
        nodes.push(node("458", "seed", &seed.to_string()));
    }
    json!({"addMetadata":false,"instanceType":"default","usePersonalQueue":false,
        "workflow":graph.to_string(),"nodeInfoList":nodes})
}

fn image_mime(m: &Value) -> bool {
    matches!(
        m["mime"].as_str(),
        Some("image/png" | "image/jpeg" | "image/webp")
    )
}

fn selected_images(p: &Value, w: Workflow) -> Result<Vec<&Value>> {
    let media = p["media"]
        .as_array()
        .ok_or_else(|| invalid("缺少受管媒体列表"))?;
    if !p["uploads"].as_array().is_some_and(Vec::is_empty) {
        return Err(invalid("此工作流使用工作台原图"));
    }
    if !w.edit {
        return if media.is_empty() && p["input"].get("referenceImages").is_none() {
            Ok(Vec::new())
        } else {
            Err(invalid("文生图不接收参考图"))
        };
    }
    let refs = p["input"]["referenceImages"]
        .as_array()
        .filter(|images| (1..=10).contains(&images.len()) && images.len() == media.len())
        .ok_or_else(|| invalid("图像编辑需要 1～10 张有序参考图"))?;
    let mut result = Vec::new();
    for reference in refs {
        let id = reference["mediaId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| invalid("参考图缺少媒体 ID"))?;
        let mut matching = media.iter().filter(|m| m["id"] == id);
        let m = matching.next().ok_or_else(|| invalid("找不到参考图原件"))?;
        if matching.next().is_some()
            || result.contains(&m)
            || m["kind"] != "image"
            || !image_mime(m)
            || text(m, "handle")?.is_empty()
            || text(m, "fileName")?.is_empty()
            || text(m, "byteLength")?
                .parse::<u64>()
                .ok()
                .is_none_or(|n| n == 0)
        {
            return Err(invalid("参考图须为不同的受管 PNG、JPEG 或 WebP 原件"));
        }
        result.push(m);
    }
    Ok(result)
}

fn response(r: &Value) -> Result<Value> {
    let status = r["status"].as_u64().ok_or_else(malformed)?;
    if !(200..300).contains(&status) {
        return Err(fail(
            match status {
                401 | 403 => "UNAUTHORIZED",
                429 => "RATE_LIMITED",
                _ => "UPSTREAM_ERROR",
            },
            format!("RunningHub HTTP {status}"),
        ));
    }
    let bytes: Vec<u8> =
        serde_json::from_value(r["payload"]["bytes"].clone()).map_err(|_| malformed())?;
    serde_json::from_slice(&bytes).map_err(|_| malformed())
}

fn upload(h: &mut dyn Host, r: &Invocation, c: &Value, m: &Value) -> Result<String> {
    let stat = port(h, "host-media.info", json!({"handle":m["handle"]}))?;
    for key in ["kind", "mime", "fileName", "byteLength"] {
        if stat[key] != m[key] {
            return Err(invalid("参考图与受管媒体描述不一致"));
        }
    }
    let mut request = r.clone();
    request.r#budget_ms = request.r#budget_ms.min(45000);
    let body = json!({"kind":"multipart","parts":[
        {"kind":"file","name":"file","fileName":m["fileName"],"handle":m["handle"],"mime":m["mime"]}
    ]});
    let v = response(&http(
        h,
        &request,
        c,
        "POST",
        format!("{API}/openapi/v2/media/upload/binary"),
        body,
        false,
    )?)?;
    if v["code"] != 0 && v["code"] != 200 {
        return Err(fail("UPSTREAM_ERROR", "RunningHub 拒绝参考图上传"));
    }
    v["data"]["fileName"]
        .as_str()
        .or_else(|| v["data"]["filename"].as_str())
        .filter(|s| {
            (s.starts_with("openapi/") || s.starts_with("api/"))
                && s.split('/').all(|part| {
                    !part.is_empty()
                        && part != "."
                        && part != ".."
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                })
        })
        .map(ToString::to_string)
        .ok_or_else(malformed)
}

fn task_id(v: &Value) -> Option<&str> {
    v.as_str()
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

fn vendor_code(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| {
        !s.is_empty()
            && *s != "0"
            && s.len() <= 64
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    })
}

fn submit(h: &mut dyn Host, r: &Invocation, p: &Value) -> Result<Value> {
    let w = workflow(p)?;
    if p["inputSchemaDigest"] != w.input_schema()["digest"] {
        return Err(invalid("输入 schema 摘要不匹配"));
    }
    let c = &p["configuration"];
    if c["credentialRef"].as_str().is_none_or(str::is_empty) {
        return Err(invalid("缺少 RunningHub 凭据引用"));
    }
    let input = Input::parse(&p["input"])?;
    let chosen = selected_images(p, w)?;
    let mut images = Vec::new();
    for m in chosen {
        match upload(h, r, c, m) {
            Ok(name) => images.push(name),
            Err(mut error) => {
                reject_before_submission(
                    h,
                    r,
                    "参考图上传失败，生成尚未提交",
                    json!({"phase":"media_upload","mediaId":m["id"],"errorCode":error.code}),
                )
                .map_err(|_| fail("ACCEPTANCE_UNKNOWN", "参考图上传失败，未能保存提交状态"))?;
                error.message = format!("参考图上传失败，生成尚未提交：{}", error.message);
                return Err(error);
            }
        }
    }
    let body = if w.edit {
        edit_body(&input, &images)
    } else {
        text_body(&input)
    };
    let wire = json_body(&body)?;
    let response_unknown = || {
        fail(
            "ACCEPTANCE_UNKNOWN",
            "RunningHub 提交结果无法确认，请按任务记录核对",
        )
    };
    let reply = http(
        h,
        r,
        c,
        "POST",
        format!("{API}/openapi/v2/run/workflow/{}", w.id),
        wire,
        false,
    )
    .map_err(|e| {
        if e.code == "PERMISSION_DENIED" {
            e
        } else {
            response_unknown()
        }
    })?;
    let v = response(&reply).map_err(|e| {
        if matches!(e.code, "UNAUTHORIZED" | "RATE_LIMITED") {
            e
        } else {
            response_unknown()
        }
    })?;
    if v["status"] == "FAILED" && task_id(&v["taskId"]).is_none() {
        let mut e = fail("UPSTREAM_ERROR", "RunningHub 拒绝此工作流");
        e.vendor = vendor_code(&v["errorCode"]).map(ToString::to_string);
        return Err(e);
    }
    let id = task_id(&v["taskId"]).ok_or_else(response_unknown)?;
    let accepted = json!({"kind":"accepted","vendorTaskId":id,"appliedParams":null,
        "resumeToken":{"workflowId":w.id,"taskId":id},"usage":usage(&v)});
    persist(h, r, &accepted).map_err(|_| {
        let mut e = response_unknown();
        e.request = Some(id.into());
        e
    })?;
    Ok(accepted)
}

fn output_item(v: &Value, w: Workflow) -> Result<&Value> {
    let results = v["results"].as_array().ok_or_else(malformed)?;
    let ids: &[&str] = if w.edit { &["9000"] } else { &["484", "461"] };
    for id in ids {
        let mut found = results.iter().filter(|item| item["nodeId"] == *id);
        if let Some(item) = found.next() {
            return if found.next().is_none() {
                Ok(item)
            } else {
                Err(malformed())
            };
        }
    }
    Err(fail(
        "UPSTREAM_ERROR",
        "RunningHub 回执缺少预期出图节点，请核对云端工作流",
    ))
}

fn download(h: &mut dyn Host, r: &Invocation, c: &Value, v: &Value, w: Workflow) -> Result<Value> {
    let item = output_item(v, w)?;
    let url = text(item, "url")?;
    if !url.starts_with("https://") {
        return Err(malformed());
    }
    let reply = http(h, r, c, "GET", url.into(), json!({"kind":"none"}), true)?;
    if reply["status"] != 200 {
        return Err(fail("UPSTREAM_ERROR", "RunningHub 成果下载失败"));
    }
    let handle = text(&reply["payload"], "handle")?;
    let stat = port(h, "host-media.info", json!({"handle":handle}))?;
    if stat["kind"] != "image"
        || !image_mime(&stat)
        || text(&stat, "byteLength")?
            .parse::<u64>()
            .ok()
            .is_none_or(|n| n == 0)
    {
        return Err(fail("UPSTREAM_ERROR", "RunningHub 成果不是可用图片"));
    }
    Ok(json!({"kind":"media","role":"artifact","media":{
        "id":stat["id"],"kind":"image","handle":handle,"mime":stat["mime"],
        "fileName":stat["fileName"],"byteLength":stat["byteLength"]
    }}))
}

fn query(h: &mut dyn Host, r: &Invocation, p: &Value) -> Result<Value> {
    let w = workflow(p)?;
    let c = &p["configuration"];
    let id = task_id(&p["vendorTaskId"]).ok_or_else(|| invalid("缺少原任务号"))?;
    if p["resumeToken"]["workflowId"] != w.id || p["resumeToken"]["taskId"] != id {
        return Err(invalid("续查工作流或任务号不匹配"));
    }
    let v = response(&http(
        h,
        r,
        c,
        "POST",
        format!("{API}/openapi/v2/query"),
        json_body(&json!({"taskId":id}))?,
        false,
    )?)?;
    if v["taskId"] != id {
        return Err(malformed());
    }
    let mut outputs = Vec::new();
    let mut progress = Value::Null;
    let mut terminal = Value::Null;
    let state = match text(&v, "status")? {
        "QUEUED" => {
            progress = json!({"fraction":null,"message":"RunningHub 排队中"});
            "queued"
        }
        "RUNNING" => {
            progress = json!({"fraction":null,"message":"RunningHub 生成中"});
            "running"
        }
        "SUCCESS" => {
            outputs.push(download(h, r, c, &v, w).map_err(|mut e| {
                e.message = format!("任务 {id} 已生成，取回图片失败：{}", e.message);
                e.request = Some(id.into());
                e.retryable = true;
                e
            })?);
            "succeeded"
        }
        "FAILED" => {
            terminal = json!({"category":"vendorTaskFailed","message":"RunningHub 生成失败",
                "vendorCode":vendor_code(&v["errorCode"]),"details":{"taskId":id}});
            "failed"
        }
        _ => return Err(malformed()),
    };
    Ok(
        json!({"status":state,"outputs":outputs,"data":{"workflowId":w.id,"remoteTaskId":id},
        "usage":usage(&v),"progress":progress,"terminalFailure":terminal,"resumeToken":p["resumeToken"]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_to_image_passes_the_workbench_values() {
        let parameters = json!({
            "prompt":"  自己输入的提示词\n保留换行", "negativePrompt":"低清晰度",
            "aspectRatio":"9:16", "megapixels":2.5, "seed":9007199254740991_u64
        });
        let wire = text_body(&Input::parse(&parameters).unwrap());
        assert_eq!(
            wire["nodeInfoList"],
            json!([
                node("459:474", "on_false", "  自己输入的提示词\n保留换行"),
                node("459:471", "prompt", "  自己输入的提示词\n保留换行"),
                node("459:452", "negative_prompt", "低清晰度"),
                node("13", "aspect_ratio", "9:16 (Portrait Widescreen)"),
                node("13", "megapixels", "2.5"),
                node("459:458", "seed", "9007199254740991"),
                node("459:471", "sampling_mode.seed", "9007199254740991")
            ])
        );
        assert!(wire.get("workflow").is_none());
    }

    #[test]
    fn image_edit_has_exactly_the_selected_reference_links() {
        let parameters =
            json!({"prompt":"编辑人物","aspectRatio":"16:9","megapixels":2.0,"seed":42});
        let input = Input::parse(&parameters).unwrap();
        for count in [1, 2, 10] {
            let images: Vec<_> = (1..=count)
                .map(|n| format!("openapi/ref-{n}.png"))
                .collect();
            let wire = edit_body(&input, &images);
            let graph: Value = serde_json::from_str(wire["workflow"].as_str().unwrap()).unwrap();
            assert_eq!(graph["456"]["inputs"]["width"], 1920);
            assert_eq!(graph["456"]["inputs"]["height"], 1088);
            assert_eq!(graph["474"]["inputs"]["prompt"], "编辑人物");
            assert_eq!(graph["458"]["inputs"]["seed"], 42);
            for n in 0..10 {
                let slot = format!("images.image_{}", n + 1);
                let id = (4700 + n).to_string();
                if n < count {
                    assert_eq!(graph["474"]["inputs"][&slot], json!([id, 0]));
                    assert_eq!(graph[&id]["inputs"]["image"], images[n]);
                } else {
                    assert!(graph["474"]["inputs"].get(slot).is_none());
                    assert!(graph.get(id).is_none());
                }
            }
        }
    }

    #[test]
    fn resolves_media_in_workbench_order_and_checks_count() {
        let media = |id| {
            json!({"id":id,"kind":"image","mime":"image/png",
            "handle":id,"fileName":"image.png","byteLength":"100"})
        };
        let payload = json!({"input":{"referenceImages":[{"mediaId":"first"},{"mediaId":"second"}]},
            "media":[media("second"),media("first")],"uploads":[]});
        let selected = selected_images(&payload, EDIT).unwrap();
        assert_eq!(selected[0]["id"], "first");
        assert_eq!(selected[1]["id"], "second");
        assert!(selected_images(&payload, TEXT).is_err());
        let mut duplicate = payload;
        duplicate["input"]["referenceImages"][1]["mediaId"] = json!("first");
        assert!(selected_images(&duplicate, EDIT).is_err());
        let empty = json!({"input":{"referenceImages":[]},"media":[],"uploads":[]});
        assert!(selected_images(&empty, EDIT).is_err());
    }

    #[test]
    fn edit_output_requires_the_rewritten_workflow() {
        let old = json!({"results":[{"nodeId":"461","url":"https://example.com/image.png"}]});
        assert!(output_item(&old, EDIT).is_err());
        assert_eq!(output_item(&old, TEXT).unwrap()["nodeId"], "461");
        let both = json!({"results":[{"nodeId":"461"},{"nodeId":"484"},{"nodeId":"9000"}]});
        assert_eq!(output_item(&both, TEXT).unwrap()["nodeId"], "484");
        assert_eq!(output_item(&both, EDIT).unwrap()["nodeId"], "9000");
    }

    #[test]
    fn invalid_values_fail_before_upload() {
        for input in [
            json!({"prompt":" "}),
            json!({"prompt":"edit","aspectRatio":"any"}),
            json!({"prompt":"edit","megapixels":4.1}),
            json!({"prompt":"edit","seed":9007199254740992_u64}),
        ] {
            assert!(Input::parse(&input).is_err());
        }
    }
}
