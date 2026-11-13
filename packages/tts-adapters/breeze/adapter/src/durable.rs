use super::*;

fn request_json(
    h: &mut dyn Host,
    origin: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Outcome<Value> {
    let response = port(
        h,
        "host-http.execute",
        json!({"plan":{
        "method":method,"url":format!("{}{path}",origin.trim_end_matches('/')),
        "headers":[{"name":"Content-Type","value":"application/json; charset=utf-8"}],"generatedHeaders":[],
        "body":body.map(|b|json!({"kind":"bytes","bytes":serde_json::to_vec(&b).unwrap()})).unwrap_or_else(||json!({"kind":"none"})),
        "sink":{"kind":"buffer","maxBytes":1048576},"timeoutMs":60000},"credential":null}),
    )?;
    let status = number(&response, "status")?;
    if !(200..300).contains(&status) {
        return Err(fail(
            "UPSTREAM_ERROR",
            "Durable companion request failed; original request was not resubmitted",
        ));
    }
    let bytes: Vec<u8> =
        serde_json::from_value(response["payload"]["bytes"].clone()).map_err(|_| malformed())?;
    serde_json::from_slice(&bytes).map_err(|_| malformed())
}

fn accepted(remote: &Value) -> Outcome<Value> {
    let id = text(remote, "jobId")?;
    if !valid_id(id) {
        return Err(malformed());
    }
    Ok(
        json!({"kind":"accepted","vendorTaskId":id,"appliedParams":remote["receipt"]["appliedParams"],
        "resumeToken":{"schemaVersion":1,"token":{"jobId":id,"requestDigest":remote["requestDigest"],
            "storeId":remote["storeId"],"receipt":remote["receipt"]}},"usage":null}),
    )
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}

pub(super) fn submit(
    h: &mut dyn Host,
    request: &Invocation,
    origin: &str,
    speech: Value,
    receipt: Value,
) -> Outcome<Value> {
    let original = port(
        h,
        "host-checkpoint.read",
        json!({"requestId":request.r#request_id}),
    )?;
    let checkpoint: Value =
        serde_json::from_str(text(&original, "valueJson")?).map_err(|_| malformed())?;
    let digest = text(&checkpoint, "requestDigest")?;
    let scope = &checkpoint["submissionScope"];
    let store = text(scope, "storeId")?;
    if checkpoint["requestId"] != request.r#request_id
        || checkpoint["state"] != "acceptance_unknown"
    {
        return Err(malformed());
    }
    let remote = request_json(
        h,
        origin,
        "POST",
        "/v1/jobs",
        Some(json!({"requestId":request.r#request_id,
        "requestDigest":digest,"expectedStoreId":store,"speech":speech,"receipt":receipt})),
    )?;
    if remote["jobId"] != request.r#request_id
        || remote["requestDigest"] != digest
        || remote["storeId"] != store
    {
        return Err(malformed());
    }
    accepted(&remote)
}

pub(super) fn probe(h: &mut dyn Host, p: &Value) -> Outcome<Value> {
    let kind = p["kind"].as_str().unwrap_or("");
    if !matches!(kind, "submission_identity" | "submission_resolution") {
        return Ok(
            json!({"packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]}),
        );
    }
    let origin = text(&p["configuration"], "origin")?;
    let mut result = json!({"kind":kind,"requestId":p["requestId"],"requestDigest":p["requestDigest"],"expectedRevision":p["expectedRevision"]});
    if kind == "submission_identity" {
        let remote = request_json(h, origin, "GET", "/v1/store-identity", None)?;
        result["status"] = json!("ready");
        result["submissionScope"] =
            json!({"storeId":text(&remote,"storeId")?,"instanceId":text(&remote,"instanceId")?});
        return Ok(result);
    }
    let remote = request_json(
        h,
        origin,
        "POST",
        "/v1/jobs/resolve",
        Some(json!({"requestId":p["requestId"],
        "requestDigest":p["requestDigest"],"expectedStoreId":p["submissionScope"]["storeId"]})),
    )?;
    result["submissionScope"] = p["submissionScope"].clone();
    let status = text(&remote, "outcome")?;
    if status != "unknown"
        && (remote["requestDigest"] != p["requestDigest"]
            || remote["storeId"] != p["submissionScope"]["storeId"]
            || remote["requestId"] != p["requestId"])
    {
        return Err(malformed());
    }
    result["status"] = json!(status);
    match status {
        "accepted" => result["accepted"] = accepted(&remote)?,
        "rejected" if remote["fencesLateSubmit"] == true => {
            result["rejection"] =
                json!({"tombstoneId":text(&remote,"tombstoneId")?,"fencesLateSubmit":true})
        }
        "unknown" => result["reason"] = json!("Original durable store identity is unavailable"),
        _ => return Err(malformed()),
    }
    Ok(result)
}

pub(super) fn control(h: &mut dyn Host, operation: &str, p: &Value) -> Outcome<Value> {
    let origin = text(&p["configuration"], "origin")?;
    let id = text(p, "vendorTaskId")?;
    let token = &p["resumeToken"]["token"];
    if !valid_id(id) || token["jobId"] != id {
        return Err(malformed());
    }
    if p["operationId"] != "std/speech.synthesize" {
        return Err(fail("UNSUPPORTED_FEATURE", "Unknown speech operation"));
    }
    let path = if operation == "cancel" {
        format!("/v1/jobs/{id}/cancel")
    } else {
        format!("/v1/jobs/{id}")
    };
    let body = (operation == "cancel").then(|| json!({"requestId":id,"requestDigest":token["requestDigest"],"expectedStoreId":token["storeId"]}));
    let remote = request_json(
        h,
        origin,
        if operation == "cancel" { "POST" } else { "GET" },
        &path,
        body,
    )?;
    if remote["jobId"] != id
        || remote["requestDigest"] != token["requestDigest"]
        || remote["storeId"] != token["storeId"]
        || remote["receipt"] != token["receipt"]
    {
        return Err(malformed());
    }
    let status = text(&remote, "status")?;
    if operation == "cancel" {
        return Ok(
            json!({"outcome":if status=="cancelled" {"remote_confirmed"} else {"remote_requested"}}),
        );
    }
    if !matches!(
        status,
        "queued" | "running" | "succeeded" | "failed" | "cancelled"
    ) {
        return Err(malformed());
    }
    let mut outputs = vec![];
    if status == "succeeded" {
        let response = port(
            h,
            "host-http.execute",
            json!({"plan":{"method":"GET","url":format!("{}/v1/jobs/{id}/audio",origin.trim_end_matches('/')),
            "headers":[],"generatedHeaders":[],"body":{"kind":"none"},"sink":{"kind":"media"},"timeoutMs":60000},"credential":null}),
        )?;
        if number(&response, "status")? != 200 {
            return Err(malformed());
        }
        let handle = text(&response["payload"], "handle")?;
        let stat = port(h, "host-media.info", json!({"handle":handle}))?;
        if stat["kind"] != "audio"
            || stat["sha256"] != remote["audioSha256"]
            || stat["byteLength"] != number(&remote, "audioBytes")?.to_string()
        {
            return Err(malformed());
        }
        outputs.push(json!({"kind":"media","role":"audio","media":{"id":stat["id"],"kind":"audio","handle":handle,
            "mime":stat["mime"],"fileName":stat["fileName"],"byteLength":stat["byteLength"]}}));
    }
    let failure = match status {
        "failed" | "cancelled" => {
            json!({"category":if status=="failed" {"vendorTaskFailed"} else {"vendorTaskCancelled"},
            "message":remote["failure"].as_str().unwrap_or("Speech job was cancelled"),"vendorCode":null,"details":{"jobId":id}})
        }
        _ => Value::Null,
    };
    Ok(
        json!({"status":status,"outputs":outputs,"resumeToken":p["resumeToken"],"usage":null,"terminalFailure":failure,
        "data":if status=="succeeded" {token["receipt"]["data"].clone()} else {Value::Null},
        "progress":if matches!(status,"queued"|"running") {json!({"fraction":null,"message":status})} else {Value::Null}}),
    )
}
