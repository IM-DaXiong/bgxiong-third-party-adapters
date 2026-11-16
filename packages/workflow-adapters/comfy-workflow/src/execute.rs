//! ComfyUI request and result interpretation over SDK-owned ports.
use alloc::{
    collections::BTreeSet,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use bgx_adapter_v2_sdk::{host, Host, Invocation};
use bgx_comfy_workflow::{
    apply_mapping_contract, canvas::CompileContextV1, digest_json,
    node_registry::{definition::mapping_definition_digest, RULE_REGISTRY_VERSION},
    ApplyRequestV1, BindingValueV1, MappingContractV1,
};
use serde_json::{json, Value};

const MAX_CONTROL_BYTES: usize = 1024 * 1024;
const MAX_HISTORY_BYTES: usize = 48 * 1024 * 1024;
const HISTORY_READ_BYTES: usize = 65536;
const PROGRESS_WAIT_MS: u64 = 800;

pub(crate) struct Failure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl Failure {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<&'static str> for Failure {
    fn from(message: &'static str) -> Self {
        Self::new("INVALID_INPUT", message)
    }
}

type Result<T> = core::result::Result<T, Failure>;

fn malformed() -> Failure {
    Failure::new(
        "UPSTREAM_ERROR",
        "ComfyUI returned an unexpected task response",
    )
}

fn unknown() -> Failure {
    Failure::new(
        "ACCEPTANCE_UNKNOWN",
        "ComfyUI acknowledgment is unavailable. Keep the original request for recovery.",
    )
}

fn status_error(status: u64) -> Failure {
    let code = match status {
        401 | 403 => "UNAUTHORIZED",
        429 => "RATE_LIMITED",
        _ => "UPSTREAM_ERROR",
    };
    Failure::new(code, format!("ComfyUI HTTP status {status}"))
}

fn port(host: &mut dyn Host, method: &str, args: Value) -> Result<Value> {
    host::call(host, method, &args).map_err(|error| {
        let error = serde_json::to_value(error).expect("host error");
        let code = match error["code"].as_str() {
            Some("INVALID_INPUT") => "INVALID_INPUT",
            Some("PERMISSION_DENIED") => "PERMISSION_DENIED",
            Some("CANCELLED") => "CANCELLED",
            Some("TIMEOUT") => "TIMEOUT",
            Some("BUDGET_EXCEEDED") => "BUDGET_EXCEEDED",
            _ => "RESOURCE_UNAVAILABLE",
        };
        Failure::new(code, format!("Managed port {method} failed"))
    })
}

fn encode_component(raw: &str) -> String {
    let mut encoded = String::new();
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

struct Endpoint<'a> {
    base: &'a str,
    credential: &'a Value,
    deadline: u64,
}

impl<'a> Endpoint<'a> {
    fn from_request(configuration: &'a Value, deadline: u64) -> Result<Self> {
        let base = configuration["endpointBaseUrl"]
            .as_str()
            .filter(|base| {
                (base.starts_with("http://") || base.starts_with("https://"))
                    && !base.contains(['#', '?'])
            })
            .ok_or("ComfyUI endpoint is missing or invalid")?;
        let credential = &configuration["credentialRef"];
        if !credential.is_null() && credential.as_str().is_none_or(str::is_empty) {
            return Err("ComfyUI credential reference is invalid".into());
        }
        Ok(Self {
            base: base.trim_end_matches('/'),
            credential,
            deadline,
        })
    }

    fn generated_headers(&self) -> Value {
        if self.credential.is_null() {
            json!([])
        } else {
            json!([{"kind":"bearer","name":"Authorization","targetField":"Authorization"}])
        }
    }

    fn http(
        &self,
        host: &mut dyn Host,
        method: &str,
        path: &str,
        data: Option<&Value>,
        sink: Value,
    ) -> Result<Value> {
        let (headers, body) = match data {
            Some(data) => (
                json!([{"name":"Content-Type","value":"application/json"}]),
                json!({"kind":"bytes","bytes":serde_json::to_vec(data).map_err(|_| malformed())?}),
            ),
            None => (json!([]), json!({"kind":"none"})),
        };
        port(
            host,
            "host-http.execute",
            json!({"plan":{
            "method":method,"url":format!("{}{path}",self.base),"headers":headers,
            "generatedHeaders":self.generated_headers(),"body":body,"sink":sink,"timeoutMs":self.deadline},
            "credential":self.credential}),
        )
    }

    fn json(
        &self,
        host: &mut dyn Host,
        method: &str,
        path: &str,
        data: Option<&Value>,
    ) -> Result<Value> {
        let response = self.http(
            host,
            method,
            path,
            data,
            json!({"kind":"buffer","maxBytes":MAX_CONTROL_BYTES}),
        )?;
        let status = response["status"].as_u64().ok_or_else(malformed)?;
        if !(200..300).contains(&status) {
            return Err(status_error(status));
        }
        let bytes: Vec<u8> = serde_json::from_value(response["payload"]["bytes"].clone())
            .map_err(|_| malformed())?;
        serde_json::from_slice(&bytes).map_err(|_| malformed())
    }

    fn history(&self, host: &mut dyn Host, id: &str) -> Result<Value> {
        let path = format!("/history/{}", encode_component(id));
        let response = self.http(host, "GET", &path, None, json!({"kind":"media"}))?;
        let handle = response["payload"]["handle"]
            .as_str()
            .ok_or_else(malformed)?;
        let read = (|| {
            let status = response["status"].as_u64().ok_or_else(malformed)?;
            if status != 200 {
                return Err(status_error(status));
            }
            let mut bytes = Vec::new();
            loop {
                let offset = bytes.len().to_string();
                let chunk: Vec<u8> = serde_json::from_value(port(
                    host,
                    "host-media.read",
                    json!({"handle":handle,"offset":offset,"maxBytes":HISTORY_READ_BYTES}),
                )?)
                .map_err(|_| malformed())?;
                if chunk.is_empty() {
                    break;
                }
                if bytes.len() + chunk.len() > MAX_HISTORY_BYTES {
                    return Err(Failure::new(
                        "BUDGET_EXCEEDED",
                        "ComfyUI history exceeds its byte budget",
                    ));
                }
                bytes.extend(chunk);
            }
            serde_json::from_slice(&bytes).map_err(|_| malformed())
        })();
        let released = port(host, "host-media.release", json!({"handle":handle}));
        let value = read?;
        released?;
        Ok(value)
    }

    fn download(&self, host: &mut dyn Host, output: &Value, kind: &str) -> Result<Value> {
        let filename = output["filename"].as_str().ok_or_else(malformed)?;
        let folder = output["subfolder"].as_str().unwrap_or_default();
        let storage = output["type"]
            .as_str()
            .filter(|value| matches!(*value, "output" | "temp"))
            .ok_or_else(malformed)?;
        let path = format!(
            "/view?filename={}&subfolder={}&type={}",
            encode_component(filename),
            encode_component(folder),
            encode_component(storage)
        );
        let response = self.http(host, "GET", &path, None, json!({"kind":"media"}))?;
        let status = response["status"].as_u64().ok_or_else(malformed)?;
        if status != 200 {
            return Err(status_error(status));
        }
        let handle = response["payload"]["handle"]
            .as_str()
            .ok_or_else(malformed)?;
        let stat = port(host, "host-media.info", json!({"handle":handle}))?;
        if stat["kind"] != kind {
            return Err(malformed());
        }
        Ok(
            json!({"kind":"media","role":kind,"media":{"id":stat["id"],"handle":handle,"kind":kind,
            "mime":stat["mime"],"fileName":stat["fileName"],"byteLength":stat["byteLength"]}}),
        )
    }

    fn progress(&self, host: &mut dyn Host, client: &str, task: &str) -> Result<Value> {
        let (protocol, address) = self
            .base
            .split_once("://")
            .ok_or("Invalid ComfyUI WebSocket base")?;
        let scheme = if protocol == "https" { "wss" } else { "ws" };
        let url = format!(
            "{scheme}://{address}/ws?clientId={}",
            encode_component(client)
        );
        let opened = port(
            host,
            "host-websocket.open",
            json!({"plan":{"url":url,"timeoutMs":self.deadline,
            "maxMessageBytes":MAX_CONTROL_BYTES,"headers":[],"generatedHeaders":self.generated_headers()},"credential":self.credential}),
        )?;
        let socket = opened["socketId"].as_str().ok_or_else(malformed)?;
        let progress = (|| loop {
            let message = port(
                host,
                "host-websocket.read",
                json!({"socketId":socket,"timeoutMs":PROGRESS_WAIT_MS}),
            )?;
            match message["kind"].as_str() {
                Some("idle") => return Ok(json!({"fraction":null,"message":"running"})),
                Some("closed") => {
                    return Err(Failure::new(
                        "RESOURCE_UNAVAILABLE",
                        "ComfyUI progress connection closed",
                    ))
                }
                Some("binary") => continue,
                Some("text") => {}
                _ => return Err(malformed()),
            }
            let text = message["text"].as_str().ok_or_else(malformed)?;
            let Ok(event) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            if event["data"]["prompt_id"] != task {
                continue;
            }
            match event["type"].as_str() {
                Some("progress") => {
                    let current = event["data"]["value"].as_f64();
                    let total = event["data"]["max"].as_f64().filter(|total| *total > 0.0);
                    let fraction = current
                        .zip(total)
                        .map(|(current, total)| (current / total).clamp(0.0, 1.0));
                    return Ok(
                        json!({"fraction":fraction,"message":event["data"]["node"].as_str()}),
                    );
                }
                Some("executing" | "execution_start") => {
                    return Ok(json!({"fraction":null,"message":event["data"]["node"].as_str()}))
                }
                _ => {}
            }
        })();
        // The broker already released a socket on an I/O error, deadline or remote close.
        if progress.is_ok() {
            port(host, "host-websocket.close", json!({"socketId":socket}))?;
        }
        progress
    }
}

pub(crate) fn probe(
    host: &mut dyn Host,
    request: &Invocation,
    configuration: &Value,
) -> Result<()> {
    let engine = Endpoint::from_request(configuration, request.r#budget_ms)?;
    if engine.json(host, "GET", "/system_stats", None)?["system"].is_object() {
        Ok(())
    } else {
        Err(malformed())
    }
}

pub(crate) fn preparation(input: &Value) -> Result<(Value, Value)> {
    let source = input["source"].as_str().ok_or("Missing workflow source")?;
    let mut compiled = crate::compile::compile(source, input)?;
    if compiled["status"] != "ready" {
        return Err("The selected workflow does not compile".into());
    }
    let bindings = input["outputBindings"]
        .as_array()
        .ok_or("Missing output bindings")?;
    let selected = &input["selection"]["outputKeys"];
    let mut seen = BTreeSet::new();
    if bindings.is_empty()
        || bindings.iter().any(|binding| {
            let Some(id) = binding["nodeId"].as_str() else {
                return true;
            };
            let source = &compiled["value"]["sourceMap"]["execToSource"][id];
            let collection = binding["collection"].as_str().unwrap_or_default();
            !seen.insert((id, collection))
                || !selected.as_array().is_some_and(|keys| {
                    keys.iter().any(|key| {
                        key["scope"] == source["scopePath"] && key["localId"] == source["nodeId"]
                    })
                })
        })
    {
        return Err("Workflow output bindings do not belong to the compiled graph".into());
    }
    let graph = compiled["value"]["apiPrompt"].take();
    let timing_input = input.get("timing").map(|values| {
        let context: CompileContextV1 = serde_json::from_value(input["context"].clone())
            .map_err(|_| "Invalid workflow compile context")?;
        Ok::<_, Failure>((values, context.definitions))
    }).transpose()?;
    let timing = timing_input.as_ref().map(|(values, definitions)| {
        crate::timing::plan(&graph, definitions, bindings, values)
    }).transpose()?;
    let mut graph = if let Some(mapping) = input.get("scalarMapping") {
        let mapped = apply_scalar_mapping(graph, input, mapping, bindings)?;
        if let Some((values, definitions)) = &timing_input {
            let mapped_timing = crate::timing::plan(&mapped, definitions, bindings, values)?;
            if timing.as_ref() != Some(&mapped_timing) {
                return Err(Failure::new(crate::timing::INVALID_TIMING, "已提交的映射改变了时序值的来源"));
            }
        }
        mapped
    } else {
        graph
    };
    let applied_timing = timing.map(|timing| timing.apply(&mut graph)).transpose()?;
    let digest = digest_json(&graph).map_err(|error| Failure::new("INVALID_INPUT", error.detail))?;
    let mut material = json!({"compiledGraphDigest":digest});
    if let Some(timing) = applied_timing {
        material["timing"] = timing;
    }
    Ok((graph, material))
}

fn apply_scalar_mapping(
    graph: Value,
    input: &Value,
    mapping: &Value,
    output_bindings: &[Value],
) -> Result<Value> {
    let contract: MappingContractV1 = serde_json::from_value(mapping["contract"].clone())
        .map_err(|_| "Invalid workflow scalar mapping contract")?;
    let values: Vec<BindingValueV1> = serde_json::from_value(mapping["values"].clone())
        .map_err(|_| "Invalid workflow scalar mapping values")?;
    let context: CompileContextV1 = serde_json::from_value(input["context"].clone())
        .map_err(|_| "Invalid workflow compile context")?;
    let definition_digest = mapping_definition_digest(&graph, &context.definitions)
        .map_err(|error| Failure::new("INVALID_INPUT", error.detail))?;
    let media_kind = serde_json::to_value(contract.output_selection.media_kind)
        .expect("serializable workflow media kind");
    if contract.snapshot.source_digest != context.source_digest
        || contract.snapshot.definition_digest != definition_digest
        || contract.snapshot.rule_registry_version != RULE_REGISTRY_VERSION
        || !output_bindings.iter().any(|binding| {
            binding["nodeId"] == contract.output_selection.node_id && binding["kind"] == media_kind
        })
    {
        return Err("Workflow scalar mapping no longer matches its original source, definitions, rules, or output".into());
    }

    let applied = apply_mapping_contract(ApplyRequestV1 {
        contract,
        workflow: graph,
        values,
    })
    .map_err(|error| Failure::new("INVALID_INPUT", error.detail))?;
    Ok(applied.workflow)
}

pub(crate) fn submit(host: &mut dyn Host, request: &Invocation, payload: &Value) -> Result<Value> {
    let input = &payload["input"];
    let (graph, material) = preparation(input)?;
    if payload["preparation"]["material"] != material {
        return Err("The prepared workflow graph changed".into());
    }
    let engine = Endpoint::from_request(&payload["configuration"], request.r#budget_ms)?;
    let client = &request.r#request_id;
    let accepted = engine
        .json(
            host,
            "POST",
            "/prompt",
            Some(&json!({"prompt":graph,"client_id":client})),
        )
        .map_err(|_| unknown())?;
    let id = accepted["prompt_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(unknown)?;
    // ReferenceHost commits the validated accepted result before delivering its receipt.
    Ok(
        json!({"kind":"accepted","vendorTaskId":id,"appliedParams":null,"resumeToken":{
        "promptId":id,"clientId":client,"outputBindings":input["outputBindings"]},"usage":null}),
    )
}

pub(crate) fn query(host: &mut dyn Host, request: &Invocation, payload: &Value) -> Result<Value> {
    let id = payload["vendorTaskId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("Invalid original ComfyUI task ID")?;
    let token = &payload["resumeToken"];
    if token["promptId"] != id {
        return Err("Resume token belongs to another task".into());
    }
    let client = token["clientId"]
        .as_str()
        .ok_or("Missing original ComfyUI client ID")?;
    let engine = Endpoint::from_request(&payload["configuration"], request.r#budget_ms)?;
    let queue = engine.json(host, "GET", "/queue", None)?;
    let history = engine.history(host, id)?;
    let entry = &history[id];
    let mut outputs = vec![];
    let mut failure = Value::Null;
    let mut progress = Value::Null;
    let queued = |kind: &str| {
        queue[kind]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row[1] == id))
    };
    let status = match entry["status"]["status_str"].as_str() {
        Some("success") => {
            for binding in token["outputBindings"]
                .as_array()
                .ok_or("Missing original output bindings")?
            {
                let node = binding["nodeId"].as_str().ok_or_else(malformed)?;
                let collection = binding["collection"].as_str().ok_or_else(malformed)?;
                let kind = binding["kind"].as_str().ok_or_else(malformed)?;
                let media = entry["outputs"][node][collection]
                    .as_array()
                    .filter(|files| !files.is_empty())
                    .ok_or_else(malformed)?;
                for output in media {
                    outputs.push(engine.download(host, output, kind)?);
                }
            }
            "succeeded"
        }
        Some("error") => {
            failure = json!({"category":"vendorTaskFailed","message":"ComfyUI workflow execution failed","vendorCode":null,"details":{"promptId":id}});
            "failed"
        }
        _ if queued("queue_running") || !entry.is_null() => {
            progress = engine.progress(host, client, id)?;
            "running"
        }
        _ if queued("queue_pending") => {
            progress = json!({"fraction":null,"message":"queued"});
            "queued"
        }
        _ => {
            return Err(Failure::new(
                "RESOURCE_UNAVAILABLE",
                "Original ComfyUI task is temporarily absent from queue and history",
            ))
        }
    };
    Ok(
        json!({"status":status,"outputs":outputs,"data":{"promptId":id},"usage":null,
        "progress":progress,"terminalFailure":failure,"resumeToken":token}),
    )
}
