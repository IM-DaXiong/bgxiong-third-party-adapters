#![cfg_attr(target_arch = "wasm32", no_std)]
extern crate alloc;
use alloc::{format, string::{String, ToString}, vec::Vec};
use bgx_adapter_v2_sdk::{guest, host, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};

fn http(host: &mut dyn Host, config: &Value, method: &str, path: &str, body: Value) -> Result<Value, String> {
    let origin = config["origin"].as_str().ok_or("Missing managed origin")?;
    let instance = config["instanceId"].as_str().ok_or("Missing managed instance")?;
    let mut headers = Vec::new();
    headers.push(json!({"name":"X-Bgx-Instance","value":instance}));
    if body["kind"] == "bytes" {
        headers.push(json!({"name":"Content-Type","value":"application/json"}));
    }
    let response:Value = host::call(host, "host-http.execute", &json!({"plan":{
        "method":method,"url":format!("{}{}",origin.trim_end_matches('/'),path),"headers":headers,"generatedHeaders":[],
        "body":body,"sink":{"kind":"buffer","maxBytes":1048576},"timeoutMs":120000},"credential":null}))
        .map_err(|error| format!("Managed companion request failed: {}",error.r#message))?;
    let status = response["status"].as_u64().ok_or("Invalid HTTP status")?;
    let bytes:Vec<u8> = serde_json::from_value(response["payload"]["bytes"].clone()).map_err(|_| "Invalid HTTP buffer")?;
    if !(200..300).contains(&status) {
        return Err(format!("Companion HTTP {}: {}",status,String::from_utf8_lossy(&bytes)));
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid companion JSON".to_string())
}
fn body(value:Value)->Value {json!({"kind":"bytes","bytes":serde_json::to_vec(&value).expect("JSON")})}
fn invoke(request:&Invocation,host:&mut dyn Host)->Result<Value,String> {
    let operation = serde_json::to_value(&request.r#operation).expect("operation");
    if operation == "capabilities" {return serde_json::from_str(include_str!("capabilities.json")).map_err(|e|e.to_string());}
    if operation == "probe" {return Ok(json!({"packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]}));}
    let payload:Value=serde_json::from_str(&request.r#payload_json).map_err(|e|e.to_string())?;
    let config=&payload["configuration"];
    if payload["operationId"]!="std/speech.diarize" || payload["operationVersion"]!=1 {return Err("Unsupported operation".into());}
    if operation == "submit" {
        let media_id=payload["input"]["audio"]["mediaId"].as_str().ok_or("Audio required")?;
        let media=payload["media"].as_array().ok_or("Media required")?.iter().find(|m|m["id"]==media_id).ok_or("Authorized audio missing")?;
        let uploaded=http(host,config,"POST","/v1/audio",json!({"kind":"multipart","parts":[{"kind":"file","name":"file","fileName":media["fileName"],"handle":media["handle"],"mime":media["mime"]}]}))?;
        let input=&payload["input"];
        let job=http(host,config,"POST","/v1/jobs",body(json!({"requestKey":request.r#request_id,"audioId":uploaded["audioId"],"sourceFingerprint":input["sourceFingerprint"],"durationMs":input["durationMs"],"speakerCount":input.get("speakerCount")})))?;
        let id=job["jobId"].as_str().ok_or("Missing companion job ID")?;
        return Ok(json!({"kind":"accepted","vendorTaskId":id,"appliedParams":null,"resumeToken":{"schemaVersion":1,"sourceFingerprint":input["sourceFingerprint"],"jobId":id},"usage":null}));
    }
    let id=payload["vendorTaskId"].as_str().ok_or("Missing task ID")?;
    if id.len()!=64 || !id.bytes().all(|c|c.is_ascii_hexdigit()) || payload["resumeToken"]["jobId"]!=id {return Err("Invalid frozen job identity".into());}
    if operation == "query" {
        let mut result=http(host,config,"GET",&format!("/v1/jobs/{id}"),json!({"kind":"none"}))?;
        if matches!(result["status"].as_str(),Some("succeeded"|"failed"|"cancelled"|"expired")) {result["progress"]=Value::Null;}
        result["resumeToken"]=payload["resumeToken"].clone();
        return Ok(result);
    }
    if operation == "cancel" {return http(host,config,"POST",&format!("/v1/jobs/{id}/cancel"),json!({"kind":"none"}));}
    Err("Unsupported operation".into())
}
pub struct Diarization;
impl Adapter for Diarization {
    fn invoke(request:Invocation,host:&mut dyn Host)->Response {
        match invoke(&request,host) {
            Ok(value)=>guest::completed_response(&request,&value).expect("response"),
            Err(message)=>serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,"error":{"code":"UPSTREAM_ERROR","message":message,"retryable":false,"vendorCode":null,"vendorRequestId":null}})).expect("error")
        }
    }
}
bgx_adapter_v2_sdk::export_adapter!(Diarization);
