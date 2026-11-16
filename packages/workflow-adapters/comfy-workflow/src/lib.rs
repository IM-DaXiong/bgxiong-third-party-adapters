#![cfg_attr(target_arch = "wasm32", no_std)]
//! ComfyUI workflow analysis, compilation and protocol ownership.

extern crate alloc;
#[cfg(target_arch = "wasm32")]
use alloc::{string::{String, ToString}, vec, vec::Vec};
mod analyze;
mod compile;
mod execute;
mod reference_slots;
mod timing;

use bgx_adapter_v2_sdk::{guest, host, Adapter, Host, Invocation, Response};
use bgx_comfy_workflow::CompileDiagnostic;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PLUGIN: &str = "local.thirdparty.comfy-workflow";
const ANALYZE: &str = "local.thirdparty.comfy-workflow/analyze";
const COMPILE: &str = "local.thirdparty.comfy-workflow/compile";
const REFERENCE_SLOTS: &str = "local.thirdparty.comfy-workflow/reference-slots";
const EXECUTE: &str = "local.thirdparty.comfy-workflow/execute";
const VERSION: u64 = 2;
const REFERENCE_SLOTS_VERSION: u64 = 1;
const EXECUTE_VERSION: u64 = 2;
const CONFIG: &str = r#"{"type":"object","additionalProperties":false}"#;
const ANALYZE_INPUT: &str = include_str!("../schemas/input.json");
const COMPILE_INPUT: &str = include_str!("../schemas/compile-input.json");
const OUTPUT: &str = include_str!("../schemas/output.json");
const REFERENCE_SLOTS_OUTPUT: &str = include_str!("../schemas/reference-slots-output.json");
const EXECUTE_CONFIG: &str = r#"{"type":"object","properties":{"endpointBaseUrl":{"type":"string","minLength":1},"credentialRef":{"type":["string","null"]}},"required":["endpointBaseUrl","credentialRef"],"additionalProperties":false}"#;
const EXECUTE_OUTPUT: &str = r#"{"type":"object","properties":{"promptId":{"type":"string","minLength":1}},"required":["promptId"],"additionalProperties":false}"#;

fn schema(id: &str, text: &str) -> Value {
    let text = text.replace("\r\n", "\n");
    json!({"id":id,"digest":hex::encode(Sha256::digest(text.as_bytes())),"schemaJson":text})
}

fn field(pointer: &str, widget: &str, label: &str) -> Value {
    json!({"fieldPointer":pointer,"widget":widget,"label":label,"description":"","group":""})
}

fn capability(id: &str, operation: &str, label: &str, mut input: Value, output: &str, controls: Vec<Value>) -> Value {
    let config = schema("config", CONFIG);
    input["required"].as_array_mut().unwrap().retain(|key| key != "source");
    input["oneOf"] = json!([{"required":["source"]},{"required":["sourceRef"]}]);
    input["properties"]["sourceRef"] = json!({"type":"string","minLength":1});
    let input = schema("input", &input.to_string());
    let output = schema("output", output);
    json!({"schemaVersion":2,"localId":id,"capabilityKey":operation,"transportSlot":"generic",
        "modelIds":[id],"configurationSchema":{"id":"config","digest":config["digest"]},
        "operations":[{"id":operation,"version":operation_version(operation),"label":label,"inputProfile":"operation-input-v1",
            "inputSchema":{"id":"input","digest":input["digest"]},
            "outputSchema":{"id":"output","digest":output["digest"]},"resumeSchema":null,
            "uiSchema":{"controls":controls},"mediaSlots":[{"role":"workflow-source","inputPointer":"/sourceRef",
                "mediaKind":"file","minCount":0,"maxCount":1}],"outputSlots":[],
            "execution":{"mode":"synchronous","streaming":false,"cancellation":"unsupported","resumable":false,"remoteIdempotency":false}}],
        "schemas":[config,input,output]})
}

fn execution_capability() -> Value {
    let bindings: Value = serde_json::from_str(include_str!("../schemas/output-bindings.json"))
        .expect("output bindings schema");
    let mut input: Value = serde_json::from_str(COMPILE_INPUT).expect("compile schema");
    input["properties"]["outputBindings"] = bindings.clone();
    input["properties"]["timing"] = json!({
        "type":"object", "additionalProperties":false, "required":["seconds","fps"],
        "properties":{"seconds":{"type":"integer","minimum":1}, "fps":{"type":"integer","minimum":1}}
    });
    input["properties"]["scalarMapping"] = json!({
        "type":"object", "additionalProperties":false, "required":["contract","values"],
        "properties": {
            "contract":{"type":"object","additionalProperties":{}},
            "values":{"type":"array","maxItems":bgx_comfy_workflow::contract::MAX_BINDINGS,
                "items":{"type":"object","additionalProperties":false,"required":["bindingId","value"],
                    "properties":{"bindingId":{"type":"string","minLength":1},"value":{"type":["string","integer","number","boolean"]}}}}
        }
    });
    input["required"]
        .as_array_mut()
        .expect("compile fields")
        .extend([json!("selection"), json!("outputBindings")]);
    let config = schema("config", EXECUTE_CONFIG);
    let input = schema("input", &input.to_string());
    let output = schema("output", EXECUTE_OUTPUT);
    let resume = schema("resume", &json!({"type":"object","properties":{
        "promptId":{"type":"string","minLength":1}, "clientId":{"type":"string","minLength":1},
        "outputBindings":bindings},"required":["promptId","clientId","outputBindings"],"additionalProperties":false}).to_string());
    json!({"schemaVersion":2,"localId":"workflow-execute","capabilityKey":EXECUTE,"transportSlot":"generic",
        "modelIds":["workflow-execute"],"configurationSchema":{"id":"config","digest":config["digest"]},
        "operations":[{"id":EXECUTE,"version":EXECUTE_VERSION,"label":"执行 ComfyUI 工作流","inputProfile":"operation-input-v1",
            "inputSchema":{"id":"input","digest":input["digest"]},
            "outputSchema":{"id":"output","digest":output["digest"]},
            "resumeSchema":{"id":"resume","digest":resume["digest"]},
            "uiSchema":{"controls":[field("/source","multiline","工作流源图"),field("/context","object","编译上下文"),
                field("/selection","object","输出选择"),field("/outputBindings","repeat","成果绑定")]},
            "mediaSlots":[],"outputSlots":[{"role":"image","kind":"image","minCount":0,"maxCount":null},
                {"role":"video","kind":"video","minCount":0,"maxCount":null},
                {"role":"audio","kind":"audio","minCount":0,"maxCount":null}],
            "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported","resumable":true,"remoteIdempotency":false}}],
        "schemas":[config,input,output,resume]})
}

pub fn capabilities() -> Value {
    let source = field("/source", "multiline", "工作流源图");
    let controls = vec![source.clone(), field("/context", "object", "编译上下文"), field("/selection", "object", "输出选择")];
    let analyze_input = serde_json::from_str(ANALYZE_INPUT).expect("analysis input schema");
    let compile_input: Value = serde_json::from_str(COMPILE_INPUT).expect("compile input schema");
    let mut references_input = compile_input.clone();
    references_input["required"].as_array_mut().expect("compile fields").push(json!("selection"));
    references_input["properties"]["selection"]["properties"]["outputKeys"]["maxItems"] = json!(1);
    references_input["properties"]["referenceCount"] = json!({"type":"integer","minimum":0,"maximum":u32::MAX});
    json!({"schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),"capabilityContracts":[
        capability("workflow-analyze", ANALYZE, "分析 ComfyUI 工作流", analyze_input, OUTPUT, vec![source]),
        capability("workflow-compile", COMPILE, "编译 ComfyUI 工作流", compile_input, OUTPUT, controls.clone()),
        capability("workflow-reference-slots", REFERENCE_SLOTS, "读取 ComfyUI 图片参考槽", references_input, REFERENCE_SLOTS_OUTPUT, controls),
        execution_capability()
    ]})
}

fn ready(data: Value) -> Value {
    json!({"status":"ready","value":data,"diagnostics":[]})
}

fn blocked(diagnostics: &[CompileDiagnostic]) -> Value {
    json!({"status":"blocked","value":null,"diagnostics":diagnostics})
}

fn operation_version(operation: &str) -> u64 {
    match operation {
        EXECUTE => EXECUTE_VERSION,
        REFERENCE_SLOTS => REFERENCE_SLOTS_VERSION,
        _ => VERSION,
    }
}

fn workflow_operation(payload: &Value) -> Result<&str, &'static str> {
    let operation = payload["operationId"]
        .as_str()
        .ok_or("Missing workflow operation")?;
    if payload["operationVersion"] != operation_version(operation) || payload["capabilityKey"] != operation {
        return Err("Workflow operation identity changed");
    }
    match (operation, payload["modelId"].as_str()) {
        (ANALYZE, Some("workflow-analyze"))
        | (COMPILE, Some("workflow-compile"))
        | (REFERENCE_SLOTS, Some("workflow-reference-slots"))
        | (EXECUTE, Some("workflow-execute")) => Ok(operation),
        _ => Err("Unknown workflow operation"),
    }
}

fn project(payload: &Value, host: &mut dyn Host) -> Result<Value, execute::Failure> {
    let input = &payload["input"];
    let reference;
    let source = if let Some(source) = input["source"].as_str() {
        source
    } else {
        reference = read_source(host, payload)?;
        &reference
    };
    match workflow_operation(payload)? {
        ANALYZE => Ok(analyze::analyze(source)),
        COMPILE => Ok(compile::compile(source, input)?),
        REFERENCE_SLOTS => Ok(reference_slots::project(source, input)?),
        _ => Err("Expected a workflow analysis, compilation or reference projection".into()),
    }
}

fn read_source(host: &mut dyn Host, payload: &Value) -> Result<String, &'static str> {
    let id = payload["input"]["sourceRef"].as_str().ok_or("Missing workflow source")?;
    let source = payload["media"].as_array().ok_or("Missing workflow source resource")?.iter()
        .find(|item| item["id"] == id && item["kind"] == "file")
        .ok_or("Invalid workflow source resource")?;
    let handle = source["handle"].as_str().ok_or("Invalid workflow source handle")?;
    let length: usize = source["byteLength"].as_str().and_then(|bytes| bytes.parse().ok())
        .filter(|length| *length > 0).ok_or("Invalid workflow source length")?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| "Workflow source exceeds available memory")?;
    while bytes.len() < length {
        let count = (length - bytes.len()).min(64 * 1024);
        let chunk: Vec<u8> = host::call(host, "host-media.read", &json!({
            "handle":handle,"offset":bytes.len().to_string(),"maxBytes":count,
        })).map_err(|_| "Cannot read the workflow source resource")?;
        if chunk.is_empty() || chunk.len() > count {
            return Err("Workflow source resource changed during read");
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| "Workflow source is not UTF-8")
}

fn respond(
    request: &Invocation,
    invocation: &Value,
    host: &mut dyn Host,
) -> Result<Value, execute::Failure> {
    let payload: Value =
        serde_json::from_str(&request.r#payload_json).map_err(|_| "Invalid payload JSON")?;
    match invocation["operation"].as_str() {
        Some("capabilities") => Ok(capabilities()),
        Some("probe") if payload["kind"] == "package_validation" => Ok(
            json!({"packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]}),
        ),
        Some("probe") if payload["kind"] == "connectivity" => {
            execute::probe(host, request, &payload["configuration"])?;
            Ok(
                json!({"packageValid":true,"connection":"ready","authentication":"not_checked","resources":"not_checked","issues":[]}),
            )
        }
        Some("probe") if payload["kind"] == "operation_input_preflight" => {
            let material = (|| -> Result<Value, execute::Failure> {
                if payload["environment"]["ready"] != true {
                    return Err("Workflow environment is unavailable".into());
                }
                if workflow_operation(&payload["selection"])? == EXECUTE {
                    return execute::preparation(&payload["input"]).map(|(_, material)| material);
                }
                Ok(json!({}))
            })();
            let material = match material {
                Ok(material) => material,
                Err(error) => {
                    let (code, pointer, message) = if error.code == timing::INVALID_TIMING {
                        ("local.thirdparty.comfy-workflow/invalid_timing", "/timing", error.message.as_str())
                    } else {
                        ("local.thirdparty.comfy-workflow/invalid_input", "", "工作流操作输入或环境不可用")
                    };
                    return Ok(json!({"kind":"operation_input_preflight","status":"rejected","requestDigest":payload["requestDigest"],
                        "diagnostics":[{"code":code,"message":message,"fieldPointer":pointer}]}));
                }
            };
            Ok(
                json!({"kind":"operation_input_preflight","status":"ready","requestDigest":payload["requestDigest"],"material":material}),
            )
        }
        Some("submit") if workflow_operation(&payload)? == EXECUTE => {
            execute::submit(host, request, &payload)
        }
        Some("submit") => Ok(
            json!({"kind":"completed","vendorTaskId":null,"appliedParams":null,
            "outputs":[],"data":project(&payload, host)?,"usage":null}),
        ),
        Some("query") if workflow_operation(&payload)? == EXECUTE => {
            execute::query(host, request, &payload)
        }
        Some("cancel") if workflow_operation(&payload)? == EXECUTE => {
            Ok(json!({"outcome":"unsupported"}))
        }
        _ => Err(execute::Failure::new(
            "UNSUPPORTED_FEATURE",
            "Workflow operation is unsupported",
        )),
    }
}

pub struct ComfyWorkflow;

impl Adapter for ComfyWorkflow {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        let invocation = serde_json::to_value(&request).expect("typed invocation");
        match respond(&request, &invocation, host) {
            Ok(data) => guest::completed_response(&request, &data).expect("workflow result"),
            Err(error) => {
                serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,
                "error":{"code":error.code,"message":error.message,"retryable":false,
                    "vendorCode":null,"vendorRequestId":null}}))
                .expect("workflow error")
            }
        }
    }
}

bgx_adapter_v2_sdk::export_adapter!(ComfyWorkflow);
