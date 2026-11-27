use super::*;

const NODE: &str = "BGXFaceAnalyze";
const NODE_ID: &str = "1";
const MISSING_NODES: &str = "当前 ComfyUI 地址可访问，但未加载支持目标分析的小脸精修节点。请在 ComfyUI 所在机器安装适配器配套节点，重启服务后再试。";

pub(super) fn submit(h: &mut dyn Host, payload: &Value, request_id: &str, phase: &mut Phase) -> Result<Value> {
    let base = cfg_base(&payload["configuration"])?.trim_end_matches('/');
    let input = payload.get("input").ok_or_else(|| invalid("missing input"))?;
    let detection = input.get("detectionParams").or_else(|| input.get("detection")).cloned().unwrap_or_else(|| json!({}));
    let source = input.get("source").ok_or_else(|| invalid("missing source identity"))?;
    let source_id = source["mediaId"].as_str().ok_or_else(|| invalid("missing source mediaId"))?;
    let request_id = normalized_uuid(request_id)?;
    let readiness = http_json(h, "GET", &format!("{base}/bgx_face_refine/health"), None).map_err(|error| {
        if error.http_status == Some(404) { fail("RESOURCE_UNAVAILABLE", MISSING_NODES) } else { error }
    })?;
    if readiness["queuedAnalysis"] != true || readiness["analysisSessions"] != true || readiness["trackingReceipt"] != true {
        return Err(fail("RESOURCE_UNAVAILABLE", MISSING_NODES));
    }
    let mut workflow = json!({(NODE_ID):{"class_type":NODE,"inputs":{
        "video":"",
        "detector":detection["detector"].as_str().unwrap_or("bbox/face_yolov8m.pt"),
        "confidence":detection["confidence"].as_f64().unwrap_or(0.35),
        "cut_detection":detection["cutDetection"].as_str().unwrap_or("auto"),
        "cut_threshold":detection["cutThreshold"].as_f64().unwrap_or(3.0),
        "select":detection["selectHint"].as_str().unwrap_or("largest_face"),
        "force":detection["force"].as_bool().unwrap_or(false),
        "request_id":request_id
    }}});
    let definitions = platform_parameters::definitions(h, base, &workflow).map_err(|error| {
        if error.http_status == Some(404) || error.message.starts_with("node unavailable:") {
            fail("RESOURCE_UNAVAILABLE", MISSING_NODES)
        } else { error }
    })?;
    platform_parameters::prepare(&mut workflow, &definitions)?;
    let mut session_detection = workflow[NODE_ID]["inputs"].clone();
    let options = session_detection.as_object_mut().expect("built analysis graph inputs");
    options.remove("video");
    options.remove("request_id");
    let video = upload_source(h, payload, base, source_id, "video")?;
    workflow[NODE_ID]["inputs"]["video"] = json!(video);
    let prompt_id = post_comfy_prompt(h, base, workflow, &format!("bgx-analysis-{request_id}"), phase)?;
    Ok(json!({
        "kind":"accepted",
        "vendorTaskId":prompt_id,
        "appliedParams":null,
        "resumeToken":{
            "promptId":prompt_id,"operationId":OP_ANALYZE,"analysisRequestId":request_id,
            "source":source,"detection":detection,"sessionDetection":session_detection
        },
        "usage":null
    }))
}

fn normalized_uuid(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    if bytes.len() != 32 && (bytes.len() != 36 || [8,13,18,23].iter().any(|&p| bytes[p] != b'-')) {
        return Err(invalid("analysis request identity must be a UUID"));
    }
    let compact = value.replace('-', "");
    if compact.len() != 32 || !compact.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("analysis request identity must be a UUID"));
    }
    Ok(compact.to_ascii_lowercase())
}

fn request_identity(token: &Value) -> Result<&str> {
    token["analysisRequestId"].as_str()
        .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .ok_or_else(|| invalid("invalid queued analysis identity"))
}

pub(super) fn read(h: &mut dyn Host, base: &str, entry: &Value, token: &Value) -> Result<Value> {
    let request_id = request_identity(token)?;
    let output = &entry["outputs"][NODE_ID]["bgx_analysis"];
    if output != &json!([request_id]) {
        return Err(fail("UPSTREAM_ERROR", "queued analysis output does not match the frozen request"));
    }
    let raw = http_json(h, "GET", &format!("{}/bgx_face_refine/queued-analysis/{request_id}", base.trim_end_matches('/')), None)?;
    let receipt = result_receipt(&raw, &token["source"], &token["detection"])?;
    let response = http_json(h, "POST", &format!("{}/bgx_face_refine/analysis-session", base.trim_end_matches('/')), Some(json!({
        "analysis":receipt,"detection":token["sessionDetection"]
    })))?;
    if response["saved"] != true {
        return Err(fail("UPSTREAM_ERROR", "ComfyUI did not confirm the analysis session was saved"));
    }
    Ok(receipt)
}

pub(super) fn cancel(h: &mut dyn Host, base: &str, prompt_id: &str, token: &Value) -> Result<Value> {
    if token["promptId"].as_str() != Some(prompt_id) || token["operationId"] != OP_ANALYZE {
        return Err(invalid("queued analysis cancellation identity mismatch"));
    }
    let request_id = request_identity(token)?;
    let endpoint = format!("{}/bgx_face_refine/queued-analysis/{request_id}/cancel", base.trim_end_matches('/'));
    http_json(h, "POST", &endpoint, Some(json!({"promptId":prompt_id})))
}

fn result_receipt(raw: &Value, source: &Value, detection: &Value) -> Result<Value> {
    // Map the scanner result into the media-target-selection receipt.
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
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "http://localhost:8188";
    const TASK_ID: &str = "01234567-89ab-4cde-8f01-23456789abcd";
    const ARTIFACT_ID: &str = "0123456789ab4cde8f0123456789abcd";
    const PROMPT_ID: &str = "79aea936-d53c-4b1c-95e6-533f20859ddb";

    struct Script {
        calls: Vec<Value>,
        replies: std::collections::VecDeque<core::result::Result<Value, host::HostError>>,
    }
    impl Script {
        fn responses(replies: impl IntoIterator<Item=Value>) -> Self {
            Self { calls: vec![], replies: replies.into_iter().map(Ok).collect() }
        }
        fn calls(&self) -> Vec<(&str, &str)> {
            self.calls.iter().map(|v| {
                let plan = &v["args"]["plan"];
                (plan["method"].as_str().unwrap_or(""), plan["url"].as_str().unwrap_or(""))
            }).collect()
        }
        fn body(&self, idx: usize) -> Value {
            let bytes: Vec<u8> = serde_json::from_value(self.calls[idx]["args"]["plan"]["body"]["bytes"].clone()).unwrap();
            serde_json::from_slice(&bytes).unwrap()
        }
    }
    impl Host for Script {
        fn call(&mut self, call: bgx_adapter_v2_sdk::HostPortCallV2) -> core::result::Result<Value, host::HostError> {
            self.calls.push(serde_json::to_value(call).unwrap());
            self.replies.pop_front().expect("unexpected host call")
        }
    }

    fn buffered(body: Value) -> Value {
        json!({"status":200,"payload":{"kind":"buffered","bytes":serde_json::to_vec(&body).unwrap()}})
    }

    fn healthy() -> Value {
        buffered(json!({"analysisSessions":true,"trackingReceipt":true,"queuedAnalysis":true}))
    }

    fn submission() -> Value {
        json!({"configuration":{"endpointBaseUrl":BASE},"input":{"source":{"mediaId":"source","digest":"frozen-source"}},
            "media":[{"id":"source","handle":"managed-clip","kind":"video","mime":"video/mp4","fileName":"clip.mp4","byteLength":"123"}]})
    }

    #[test]
    fn missing_companion_stops_before_upload_without_masking_connection_error() {
        let payload = submission();
        let not_found = json!({"status":404,"payload":{"bytes":b"404 Not Found".to_vec()}});
        for replies in [
            vec![not_found],
            vec![buffered(json!({"analysisSessions":true,"trackingReceipt":true}))],
            vec![healthy(), buffered(json!({}))],
        ] {
            let mut h = Script::responses(replies);
            let mut phase = Phase::BeforeSubmission;
            let error = submit(&mut h, &payload, TASK_ID, &mut phase).unwrap_err();
            assert_eq!(error.code, "RESOURCE_UNAVAILABLE");
            assert_eq!(error.message, MISSING_NODES);
            assert!(matches!(phase, Phase::BeforeSubmission));
            assert!(h.calls().iter().all(|(method, _)| *method == "GET"));
            assert!(h.replies.is_empty());
        }
        let mut h = Script::responses([]);
        h.replies.push_back(Err(serde_json::from_value(json!({"code":"TIMEOUT","message":"Comfy connection timed out"})).unwrap()));
        let error = submit(&mut h, &payload, TASK_ID, &mut Phase::BeforeSubmission).unwrap_err();
        assert_eq!(error.code, "TIMEOUT");
        assert_ne!(error.message, MISSING_NODES);
        for status in [401, 403, 405] {
            let mut h = Script::responses([json!({"status":status,"payload":{"bytes":b"{}".to_vec()}})]);
            let error = submit(&mut h, &payload, TASK_ID, &mut Phase::BeforeSubmission).unwrap_err();
            assert_eq!(error.http_status, Some(status));
            assert_ne!(error.message, MISSING_NODES);
        }
    }

    #[test]
    fn native_queue_returns_prompt_identity_and_reads_result_without_resubmission() {
        let mut h = Script::responses([
            healthy(),
            buffered(json!({"BGXFaceAnalyze":{"input":{"required":{"cut_detection":[["none","auto"]]}}}})),
            json!({"kind":"video","mime":"video/mp4","fileName":"clip.mp4","byteLength":"123"}),
            buffered(json!({"name":"uploaded.mp4","subfolder":""})),
            buffered(json!({"prompt_id":PROMPT_ID})),
        ]);
        let s = submission();
        let accepted = submit(&mut h, &s, TASK_ID, &mut Phase::BeforeSubmission).unwrap();
        let sent = h.body(4);
        assert_eq!(accepted["kind"], "accepted");
        assert_eq!(accepted["vendorTaskId"], PROMPT_ID);
        assert_eq!(sent["prompt"]["1"]["class_type"], NODE);
        assert_eq!(sent["prompt"]["1"]["inputs"]["video"], "uploaded.mp4");
        assert_eq!(sent["prompt"]["1"]["inputs"]["request_id"], ARTIFACT_ID);
        assert_eq!(sent["prompt"]["1"]["inputs"]["cut_detection"], "auto");
        assert_eq!(h.calls(), [
            ("GET", "http://localhost:8188/bgx_face_refine/health"),
            ("GET", "http://localhost:8188/object_info/BGXFaceAnalyze"),
            ("", ""),
            ("POST", "http://localhost:8188/upload/image"),
            ("POST", "http://localhost:8188/prompt"),
        ]);

        let request = json!({"configuration":s["configuration"],"vendorTaskId":PROMPT_ID,"resumeToken":accepted["resumeToken"]});
        let queue = buffered(json!({"queue_running":[[0,"another"]],"queue_pending":[[1,PROMPT_ID]]}));
        let history = buffered(json!({(PROMPT_ID):{
            "status":{"completed":true,"status_str":"success","messages":[]},
            "outputs":{"1":{"bgx_analysis":[ARTIFACT_ID]}}
        }}));
        let scan = buffered(json!({"width":640,"height":360,"fps":25,"frames":25,
            "shots":[{"segment":[0,25],"frame":0,"boxes":[[0.1,0.2,0.3,0.4]]}]}));
        let mut h = Script::responses([buffered(json!({})), queue, history, scan, buffered(json!({"saved":true}))]);
        assert_eq!(task_results::query(&mut h, &request).unwrap()["status"], "queued");
        let result = task_results::query(&mut h, &request).unwrap();
        assert_eq!(result["status"], "succeeded");
        assert_eq!(result["outputs"], json!([]));
        assert_eq!(result["data"]["source"], s["input"]["source"]);
        assert_eq!(result["data"]["shots"][0]["candidates"][0]["box"]["width"], 128.0);
        assert_eq!(h.body(4)["detection"]["cut_detection"], "auto");
        assert_eq!(h.body(4)["analysis"], result["data"]);
        assert!(h.replies.is_empty());
    }

    #[test]
    fn a_query_timeout_keeps_the_accepted_analysis_recoverable() {
        let request = json!({"configuration":{"endpointBaseUrl":BASE},"vendorTaskId":PROMPT_ID,
            "resumeToken":{"operationId":OP_ANALYZE,"promptId":PROMPT_ID,"analysisRequestId":ARTIFACT_ID}});
        for replies in [vec![], vec![buffered(json!({}))]] {
            let mut h = Script::responses(replies);
            h.replies.push_back(Err(serde_json::from_value(json!({"code":"TIMEOUT","message":"connection timed out"})).unwrap()));
            let error = task_results::query(&mut h, &request).unwrap_err();
            assert_eq!(error.code, "TIMEOUT");
            assert!(error.retryable);
            assert!(h.calls().iter().all(|(method, _)| *method == "GET"));
        }
    }

    #[test]
    fn analysis_result_and_cancellation_require_the_original_identity() {
        let mut h = Script::responses([buffered(json!({"outcome":"remote_confirmed"}))]);
        let token = json!({"operationId":OP_ANALYZE,"promptId":PROMPT_ID,"analysisRequestId":ARTIFACT_ID});
        let wrong_result = json!({"outputs":{"1":{"bgx_analysis":["unrelated"]}}});
        assert!(read(&mut h, BASE, &wrong_result, &token).is_err());
        assert!(cancel(&mut h, BASE, "unrelated-prompt", &token).is_err());
        assert!(h.calls.is_empty());
        assert_eq!(cancel(&mut h, BASE, PROMPT_ID, &token).unwrap()["outcome"], "remote_confirmed");
        assert_eq!(h.calls().len(), 1);
        assert!(h.calls()[0].1.ends_with(&format!("/queued-analysis/{ARTIFACT_ID}/cancel")));
        assert_eq!(h.body(0), json!({"promptId":PROMPT_ID}));
    }
}
