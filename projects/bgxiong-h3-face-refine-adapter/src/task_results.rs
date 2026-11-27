use super::*;

fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { out.push(b as char); }
        else { out.push_str(&format!("%{b:02X}")); }
    }
    out
}
fn required<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().filter(|s| !s.is_empty()).ok_or_else(|| invalid(&format!("missing {key}")))
}
fn collect(h: &mut dyn Host, base: &str, entry: &Value, preview: bool) -> Result<Value> {
    let node = if preview { "3" } else { "save" };
    let outputs = &entry["outputs"][node];
    let lists: Vec<&Value> = ["images", "videos", "gifs"].iter()
        .filter_map(|key| outputs.get(*key)).collect();
    let mut files = Vec::new();
    for list in lists { files.extend(list.as_array().ok_or_else(|| fail("UPSTREAM_ERROR", "invalid output list"))?.iter()); }
    if files.len() != 1 { return Err(fail("UPSTREAM_ERROR", "expected exactly one declared terminal artifact")); }
    let f = files[0];
    let name = required(f, "filename")?;
    let kind = required(f, "type")?;
    if kind != "output" && kind != "temp" { return Err(fail("UPSTREAM_ERROR", "invalid artifact location")); }
    let sub = f["subfolder"].as_str().ok_or_else(|| fail("UPSTREAM_ERROR", "missing artifact subfolder"))?;
    let url = format!("{}/view?filename={}&subfolder={}&type={}", base.trim_end_matches('/'), encode(name), encode(sub), encode(kind));
    let response = port(h, "host-http.execute", json!({"plan":{"method":"GET","url":url,"headers":[],"generatedHeaders":[],"body":{"kind":"none"},"sink":{"kind":"media"},"timeoutMs":600000},"credential":null}))?;
    if response["status"] != 200 || response["payload"]["kind"] != "media" {
        return Err(fail("UPSTREAM_ERROR", "task completed but artifact download failed; query this same task again, do not resubmit"));
    }
    let handle = required(&response["payload"], "handle")?;
    let stat = port(h, "host-media.info", json!({"handle":handle}))?;
    let expected = if preview { "image" } else { "video" };
    if stat["kind"] != expected || stat["byteLength"].as_str().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0) == 0 {
        return Err(fail("UPSTREAM_ERROR", "downloaded artifact media kind/length mismatch"));
    }
    for key in ["id", "mime", "fileName"] { required(&stat, key)?; }
    Ok(json!({"kind":"media","role":if preview {"preview"} else {"video"},"media":{"id":stat["id"],"kind":stat["kind"],"handle":handle,"mime":stat["mime"],"fileName":stat["fileName"],"byteLength":stat["byteLength"]}}))
}

pub(super) fn query(h: &mut dyn Host, payload: &Value) -> Result<Value> {
    let base = cfg_base(&payload["configuration"])?;
    let id = required(payload, "vendorTaskId")?;
    let token = &payload["resumeToken"];
    if token["promptId"].as_str() != Some(id) { return Err(invalid("resume prompt identity mismatch")); }
    let op = required(token, "operationId")?;
    if !matches!(op, OP_ANALYZE | OP_PREVIEW | OP_REFINE) { return Err(invalid("unsupported resumed operation")); }
    let retry_status_read = |mut error: Failure| {
        if op == OP_ANALYZE && matches!(error.code, "TIMEOUT" | "RESOURCE_UNAVAILABLE" | "UPSTREAM_ERROR") {
            error.retryable = true;
        }
        error
    };
    let encoded_id = encode(id);
    let mut history = comfy_query_prompt(h, base, &encoded_id).map_err(retry_status_read)?;
    let mut result = json!({"status":"running","outputs":[],"data":null,"usage":null,"progress":null,"terminalFailure":null,"resumeToken":token});
    if history[id].is_null() {
        let queue = http_json(h, "GET", &format!("{}/queue", base.trim_end_matches('/')), None)
            .map_err(retry_status_read)?;
        let pending = queue["queue_pending"].as_array().ok_or_else(|| fail("UPSTREAM_ERROR", "invalid queue response"))?;
        let running = queue["queue_running"].as_array().ok_or_else(|| fail("UPSTREAM_ERROR", "invalid queue response"))?;
        if pending.iter().any(|r| r[1].as_str() == Some(id)) {
            result["status"] = json!("queued");
            return Ok(result);
        }
        if running.iter().any(|r| r[1].as_str() == Some(id)) {
            return Ok(result);
        }
        history = comfy_query_prompt(h, base, &encoded_id).map_err(retry_status_read)?;
        if history[id].is_null() {
            return Err(fail("RESOURCE_UNAVAILABLE", "运行队列和历史均查不到原任务，请保留原任务 ID"));
        }
    }
    let entry = &history[id];
    let status = &entry["status"];
    let messages = status["messages"].as_array().ok_or_else(|| fail("UPSTREAM_ERROR", "history missing status messages"))?;
    let interrupted = messages.iter().any(|m| m[0] == "execution_interrupted");
    if status["status_str"] == "error" || interrupted {
        result["status"] = json!(if interrupted { "cancelled" } else { "failed" });
        let diagnostic = messages.iter().filter(|m| m[0] == "execution_error")
            .find_map(|m| {
                let node = m[1]["node_type"].as_str()?;
                let message = m[1]["exception_message"].as_str()?;
                match node {
                    "BGXFaceTrack" | "H3FaceTrackCrop" if message.starts_with("Every shot is marked") => Some("所选镜头没有需要精修的人脸"),
                    "BGXFaceTrack" | "H3FaceTrackCrop" | "H3FaceSelect" if message.starts_with("No face detected") => Some("整段未跟踪到所选人物"),
                    "BGXFaceTrack" if message == "整段未跟踪到所选人物" => Some("整段未跟踪到所选人物"),
                    "BGXFaceTrack" if message.starts_with("跟踪数据不完整") => Some("跟踪数据不完整"),
                    _ => None,
                }
            });
        result["terminalFailure"] = json!({"category":if interrupted {"vendorTaskCancelled"} else {"vendorTaskFailed"},"message":diagnostic.unwrap_or("Comfy execution did not complete successfully"),"vendorCode":null,"details":{"messages":messages}});
        return Ok(result);
    }
    if status["completed"] != true || status["status_str"] != "success" {
        return Err(fail("UPSTREAM_ERROR", "history has no verified successful execution"));
    }
    if op == OP_ANALYZE {
        result["data"] = analysis::read(h, base, entry, token).map_err(|mut error| {
            error.retryable = true;
            error.message = format!("Comfy task {id} completed; analysis collection failed, retain this task for query without resubmission: {}", error.message);
            error
        })?;
        result["status"] = json!("succeeded");
        return Ok(result);
    }
    let output = collect(h, base, entry, op == OP_PREVIEW).map_err(|mut error| {
        error.retryable = true;
        error.message = format!("Comfy task {id} completed; artifact collection failed, retain this task for query without resubmission: {}", error.message);
        error
    })?;
    let media_id = &output["media"]["id"];
    let data = if op == OP_PREVIEW {
        let matches: Vec<_> = entry["outputs"].as_object().ok_or_else(|| fail("UPSTREAM_ERROR", "missing outputs"))?.values()
            .filter_map(|v| v.get("trackingReceipt")).collect();
        if matches.len() != 1 { return Err(fail("UPSTREAM_ERROR", "preview missing unique tracking receipt")); }
        let mut receipt = matches[0].clone();
        // UI outputs use a one-item list; never invent tracking facts from image existence.
        if let Some(items) = receipt.as_array() {
            if items.len() != 1 { return Err(fail("UPSTREAM_ERROR", "ambiguous tracking receipt")); }
            receipt = items[0].clone();
        }
        if receipt["selectionDigest"] != token["selectionDigest"] || receipt["coveredRange"] != token["coveredRange"] {
            return Err(fail("UPSTREAM_ERROR", "tracking receipt differs from frozen selection/range"));
        }
        if !receipt["ready"]["ok"].is_boolean() || !receipt["lostRanges"].is_array() || !receipt["unresolvedRanges"].is_array() {
            return Err(fail("UPSTREAM_ERROR", "incomplete tracking receipt"));
        }
        receipt["previewMediaId"] = media_id.clone();
        receipt
    } else {
        json!({"resultMediaId":media_id,"executionSummary":{"packageVersion":env!("CARGO_PKG_VERSION"),"selectionDigest":required(token,"selectionDigest")?}})
    };
    result["status"] = json!("succeeded");
    result["outputs"] = json!([output]);
    result["data"] = data;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct ScriptHost { replies: Vec<Value>, calls: Vec<Value> }
    impl Host for ScriptHost {
        fn call(&mut self, call: bgx_adapter_v2_sdk::HostPortCallV2) -> core::result::Result<Value, host::HostError> {
            self.calls.push(serde_json::to_value(call).unwrap());
            Ok(self.replies.remove(0))
        }
    }
    fn buffered(v: Value) -> Value { json!({"status":200,"payload":{"kind":"buffered","bytes":serde_json::to_vec(&v).unwrap()}}) }
    fn payload() -> Value { json!({"configuration":{"endpointBaseUrl":"http://localhost:8188"},"vendorTaskId":"p","resumeToken":{"promptId":"p","operationId":OP_REFINE,"selectionDigest":"sel"}}) }
    #[test]
    fn execution_error_is_terminal_not_empty_success() {
        let mut h = ScriptHost { replies: vec![buffered(json!({"p":{"status":{"completed":false,"status_str":"error","messages":[["execution_error",{"exception_message":"bad input"}]]}}}))], calls: vec![] };
        let r = query(&mut h, &payload()).unwrap();
        assert_eq!(r["status"], "failed");
        assert_eq!(r["terminalFailure"]["category"], "vendorTaskFailed");
        assert_eq!(h.calls.len(), 1);
    }

    #[test]
    fn completion_between_history_and_queue_is_recovered_with_original_id() {
        let history = json!({"p":{"status":{"completed":true,"status_str":"success","messages":[]},"outputs":{
            "3":{"images":[{"filename":"preview.webp","subfolder":"","type":"output"}]},
            "tracking_receipt":{"trackingReceipt":[{"selectionDigest":"sel","coveredRange":{"startMs":0,"endMs":120},
                "lostRanges":[{"startMs":40,"endMs":80,"reason":"target_not_detected"}],
                "unresolvedRanges":[],"ready":{"ok":true,"reasons":[]}}]}
        }}});
        let queue = buffered(json!({"queue_running":[],"queue_pending":[]}));
        let mut h = ScriptHost { replies: vec![buffered(json!({})), queue, buffered(history),
            json!({"status":200,"payload":{"kind":"media","handle":"preview"}}),
            json!({"id":"preview-id","kind":"image","mime":"image/webp","fileName":"digest.webp","byteLength":"100"})],
            calls: vec![] };
        let mut p = payload();
        p["resumeToken"]["operationId"] = json!(OP_PREVIEW);
        p["resumeToken"]["coveredRange"] = json!({"startMs":0,"endMs":120});
        let response = query(&mut h, &p).unwrap();
        assert_eq!(response["status"], "succeeded");
        assert_eq!(response["data"]["ready"]["ok"], true);
        let urls: Vec<_> = h.calls[..3].iter().map(|call| {
            assert_eq!(call["args"]["plan"]["method"], "GET");
            call["args"]["plan"]["url"].as_str().unwrap()
        }).collect();
        assert_eq!(urls, ["http://localhost:8188/history/p", "http://localhost:8188/queue", "http://localhost:8188/history/p"]);
    }

    #[test]
    fn queue_lookup_keeps_identity_when_pending_or_still_missing() {
        let mut h = ScriptHost { replies: vec![buffered(json!({})), buffered(json!({
            "queue_pending":[[0,"p"]],"queue_running":[],
        }))], calls: vec![] };
        assert_eq!(query(&mut h, &payload()).unwrap()["status"], "queued");
        assert_eq!(h.calls.len(), 2);
        let mut h = ScriptHost { replies: vec![buffered(json!({})), buffered(json!({
            "queue_pending":[],"queue_running":[],
        })), buffered(json!({}))], calls: vec![] };
        assert_eq!(query(&mut h, &payload()).unwrap_err().code, "RESOURCE_UNAVAILABLE");
        assert_eq!(h.calls.len(), 3);
    }
    #[test]
    fn successful_refine_downloads_declared_output_into_managed_media() {
        let history = json!({"p":{"status":{"completed":true,"status_str":"success","messages":[]},"outputs":{"save":{"images":[{"filename":"result movie.mp4","subfolder":"","type":"output"}]}}}});
        let mut h = ScriptHost { replies: vec![buffered(history), json!({"status":200,"payload":{"kind":"media","handle":"managed-result"}}), json!({"id":"media-result","kind":"video","mime":"video/mp4","fileName":"digest.mp4","byteLength":"234"})], calls: vec![] };
        let r = query(&mut h, &payload()).unwrap();
        assert_eq!(r["status"], "succeeded");
        assert_eq!(r["outputs"][0]["media"]["handle"], "managed-result");
        assert_eq!(r["data"]["resultMediaId"], "media-result");
        assert_eq!(h.calls[1]["args"]["plan"]["sink"]["kind"], "media");
        assert!(h.calls[1]["args"]["plan"]["url"].as_str().unwrap().contains("result%20movie.mp4"));
    }
    #[test]
    fn preview_image_without_tracking_facts_is_not_ready() {
        let mut p = payload(); p["resumeToken"]["operationId"] = json!(OP_PREVIEW);
        let history = json!({"p":{"status":{"completed":true,"status_str":"success","messages":[]},"outputs":{"3":{"images":[{"filename":"preview.webp","subfolder":"","type":"temp"}]}}}});
        let mut h = ScriptHost { replies: vec![buffered(history), json!({"status":200,"payload":{"kind":"media","handle":"preview"}}), json!({"id":"preview-id","kind":"image","mime":"image/webp","fileName":"digest.webp","byteLength":"100"})], calls: vec![] };
        assert!(query(&mut h, &p).is_err());
    }
    #[test]
    fn completed_history_without_artifact_is_not_success() {
        let mut h = ScriptHost { replies: vec![buffered(json!({"p":{"status":{"completed":true,"status_str":"success","messages":[]},"outputs":{}}}))], calls: vec![] };
        assert!(query(&mut h, &payload()).is_err());
        assert_eq!(h.calls.len(), 1);
    }
    #[test]
    fn cancellation_does_not_interrupt_another_running_prompt() {
        let mut h = ScriptHost { replies: vec![buffered(json!({"queue_pending":[],"queue_running":[[1,"other-task"]]}))], calls: vec![] };
        assert_eq!(comfy_cancel(&mut h, "http://localhost:8188", "p").unwrap()["outcome"], "unsupported");
        assert_eq!(h.calls.len(), 1);
    }
}
