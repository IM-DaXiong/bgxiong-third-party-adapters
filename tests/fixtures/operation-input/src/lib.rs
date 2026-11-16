#![cfg_attr(target_arch = "wasm32", no_std)]
extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec,
};
use bgx_adapter_v2_sdk::{guest, host, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PLUGIN: &str = "local.bgx.operation-input-fixture";
const MODEL: &str = "operation-input-fixture";
const OPERATION: &str = "local.bgx.operation-input-fixture/generate";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const INPUT_SCHEMA: &str = include_str!("../schemas/input.json");
const CONFIG_SCHEMA: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const PNG: &[u8] = include_bytes!("../assets/fixture.png");
const MIX_POINTER: &str = "/layout~1options/mix~0ratio";

fn operation_version() -> u64 {
    if VERSION == "1.0.0" {
        1
    } else {
        2
    }
}

fn schema(id: &str, source: &str) -> Value {
    let normalized = source.replace("\r\n", "\n");
    let digest: String = Sha256::digest(normalized.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    json!({"id":id,"digest":digest,"schemaJson":normalized})
}

fn control(pointer: &str, widget: &str, label: &str) -> Value {
    json!({"fieldPointer":pointer,"widget":widget,"label":label,"description":"","group":"装饰设置"})
}

fn capabilities() -> Value {
    let input = schema("input", INPUT_SCHEMA);
    let received: Value =
        serde_json::from_str(INPUT_SCHEMA).expect("declared fixture input schema");
    let output_definition = json!({
        "type":"object","required":["received","operationVersion"],"additionalProperties":false,
        "properties":{"received":received,"operationVersion":{"type":"integer"}}
    });
    let output = schema("output", &output_definition.to_string());
    let config = schema("configuration", CONFIG_SCHEMA);
    let mut controls = vec![
        json!({"fieldPointer":"/prompt","widget":"multiline","label":"提示词","description":"","group":"输入","displayRole":"prompt"}),
        control("/inkTone", "select", "墨色"),
        control("/glossEnabled", "toggle", "启用光泽"),
    ];
    if operation_version() == 2 {
        controls.extend([
            control("/grainLevel", "number", "颗粒强度"),
            control(MIX_POINTER, "number", "混合比例"),
            control("/passes", "repeat", "处理顺序"),
        ]);
    }
    json!({
        "schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":VERSION,
        "capabilityContracts":[{
            "schemaVersion":2,"localId":"image","capabilityKey":"image.generate","transportSlot":"image",
            "modelIds":[MODEL],
            "configurationSchema":{"id":"configuration","digest":config["digest"]},
            "schemas":[input,output,config],
            "operations":[{
                "id":OPERATION,"version":operation_version(),"label":"参数输入测试图",
                "inputProfile":"operation-input-v1",
                "inputSchema":{"id":"input","digest":input["digest"]},
                "outputSchema":{"id":"output","digest":output["digest"]},
                "resumeSchema":null,"uiSchema":{"controls":controls},"mediaSlots":[],
                "execution":{"mode":"synchronous","streaming":false,"cancellation":"unsupported",
                    "resumable":false,"remoteIdempotency":false},
                "outputSlots":[{"role":"artifact","kind":"image","minCount":1,"maxCount":1}]
            }]
        }]
    })
}

fn selected(value: &Value) -> bool {
    value["capabilityKey"] == "image.generate"
        && value["modelId"] == MODEL
        && value["operationId"] == OPERATION
        && value["operationVersion"] == operation_version()
}

fn material(input: &Value) -> Result<Value, (String, &'static str)> {
    if input["prompt"]
        .as_str()
        .is_none_or(|prompt| prompt.trim().is_empty())
    {
        return Err(("/prompt".into(), "提示词不能为空"));
    }
    if operation_version() == 2 {
        if input["inkTone"] == "clay" && input["glossEnabled"] == true {
            return Err(("/glossEnabled".into(), "陶土墨色无法叠加光泽"));
        }
        if input["inkTone"] == "sage"
            && input
                .pointer(MIX_POINTER)
                .and_then(Value::as_f64)
                .is_some_and(|ratio| ratio > 0.75)
        {
            return Err((MIX_POINTER.into(), "鼠尾草墨色的混合比例最多为 0.75"));
        }
        for (index, pass) in input["passes"].as_array().into_iter().flatten().enumerate() {
            if pass["mode"] == "overlay"
                && pass["gain/value"].as_f64().is_some_and(|gain| gain < 0.1)
            {
                return Err((
                    format!("/passes/{index}/gain~1value"),
                    "叠加处理的增益至少为 0.1",
                ));
            }
        }
    }
    Ok(json!({"received":input,"operationVersion":operation_version()}))
}

fn preflight(payload: &Value) -> Value {
    let evaluated = if payload["environment"]["ready"] == true && selected(&payload["selection"]) {
        material(&payload["input"])
    } else {
        Err((String::new(), "操作或环境身份已变化"))
    };
    match evaluated {
        Ok(material) => json!({"kind":"operation_input_preflight","status":"ready",
            "requestDigest":payload["requestDigest"],"material":material}),
        Err((field_pointer, message)) => {
            json!({"kind":"operation_input_preflight","status":"rejected",
            "requestDigest":payload["requestDigest"],
            "diagnostics":[{"code":"local.bgx.operation-input-fixture/invalid_input",
                "fieldPointer":field_pointer,"message":message}]})
        }
    }
}

fn receipt(host: &mut dyn Host, material: Value) -> Result<Value, &'static str> {
    let unavailable = |_| "无法写入测试图原件";
    let handle = host::call::<_, String>(
        host,
        "host-media.create",
        &json!({"kind":"image","mimeHint":"image/png"}),
    )
    .map_err(unavailable)?;
    host::call::<_, Value>(
        host,
        "host-media.write",
        &json!({"handle":handle,"bytes":PNG}),
    )
    .map_err(unavailable)?;
    host::call::<_, Value>(host, "host-media.finish", &json!({"handle":handle}))
        .map_err(unavailable)?;
    let facts: Value =
        host::call(host, "host-media.info", &json!({"handle":handle})).map_err(unavailable)?;
    Ok(
        json!({"kind":"completed","vendorTaskId":null,"appliedParams":null,"usage":null,
        "outputs":[{"kind":"media","role":"artifact","media":{
            "id":facts["id"],"kind":"image","handle":handle,"mime":facts["mime"],
            "fileName":facts["fileName"],"byteLength":facts["byteLength"]
        }}],"data":material}),
    )
}

fn failure(request: &Invocation, code: &str, message: &str) -> Response {
    serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,
        "error":{"code":code,"message":message,"retryable":false,"vendorCode":null,"vendorRequestId":null}}))
        .expect("public error")
}

struct Fixture;

impl Adapter for Fixture {
    fn invoke(request: Invocation, host: &mut dyn Host) -> Response {
        let Ok(payload) = serde_json::from_str::<Value>(&request.r#payload_json) else {
            return failure(&request, "INVALID_INPUT", "测试请求格式无效");
        };
        let operation = serde_json::to_value(&request.r#operation).expect("public operation");
        let result = match operation.as_str() {
            Some("capabilities") => capabilities(),
            Some("probe") if payload["kind"] == "operation_input_preflight" => preflight(&payload),
            Some("probe") => json!({"packageValid":true,"connection":"not_checked",
                "authentication":"not_checked","resources":"not_checked","issues":[]}),
            Some("submit") if selected(&payload) => {
                let accepted = match material(&payload["input"]) {
                    Ok(accepted) if payload["preparation"]["material"] == accepted => accepted,
                    _ => return failure(&request, "INVALID_INPUT", "测试请求与冻结预检不一致"),
                };
                match receipt(host, accepted) {
                    Ok(result) => result,
                    Err(message) => return failure(&request, "RESOURCE_UNAVAILABLE", message),
                }
            }
            _ => return failure(&request, "UNSUPPORTED_FEATURE", "测试包未声明此操作"),
        };
        guest::completed_response(&request, &result).expect("public result")
    }
}

bgx_adapter_v2_sdk::export_adapter!(Fixture);
