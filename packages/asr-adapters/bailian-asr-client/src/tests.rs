use super::*;

fn wav() -> Vec<u8> {
    let mut bytes = vec![0; 364];
    bytes[..4].copy_from_slice(b"RIFF");
    bytes[8..16].copy_from_slice(b"WAVEfmt ");
    bytes[36..40].copy_from_slice(b"data");
    for (offset, value) in [
        (4, 356u32),
        (16, 16),
        (20, 65537),
        (24, 16000),
        (28, 32000),
        (32, 1048578),
        (40, 320),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn validates_actual_wav_extent_and_qwen_encodes_the_same_bytes() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let mut bytes = wav();
    assert_eq!(wav_duration(&bytes).unwrap(), 10);
    let request = qwen::request(&bytes, &json!("zh"));
    let uri = request["messages"][0]["content"][0]["input_audio"]["data"]
        .as_str()
        .unwrap();
    assert_eq!(
        STANDARD
            .decode(uri.strip_prefix("data:audio/wav;base64,").unwrap())
            .unwrap(),
        bytes
    );
    bytes.pop();
    assert!(wav_duration(&bytes).is_err());
}

#[test]
fn async_requests_keep_model_specific_parameters_and_exact_multipart_wav() {
    assert_eq!(
        task::submission(Model::Paraformer, "oss://test", Some("zh"))["parameters"]["channel_id"],
        json!([0])
    );
    let fun = task::submission(Model::Fun, "oss://test", None);
    assert_eq!(
        fun,
        json!({"model":"fun-asr","input":{"file_urls":["oss://test"]}})
    );
    let bytes = wav();
    let form = upload::multipart(&bytes, &[("key", "example.wav")]).unwrap();
    assert_eq!(
        form.windows(bytes.len())
            .filter(|part| *part == bytes)
            .count(),
        1
    );
    assert!(upload::multipart(&bytes, &[("key", "bad\r\nvalue")]).is_err());
    assert!(upload::result_url(
        "https://dashscope-result-bj.oss-cn-beijing.aliyuncs.com/output.json"
    ));
    assert!(!upload::result_url(
        "https://dashscope-result-bj.oss-cn-beijing.aliyuncs.com@evil.test/output.json"
    ));
    assert!(!upload::result_url(
        "http://dashscope-result-bj.oss-cn-beijing.aliyuncs.com/output.json"
    ));
}
use std::collections::VecDeque;

enum Reply {
    Status(u64),
    Json(Value),
    Timeout,
}

#[derive(Default)]
struct FakeHost {
    replies: VecDeque<Reply>,
    exchanges: Vec<Value>,
    inbound: Vec<u8>,
}

impl Host for FakeHost {
    fn call(&mut self, call: sdk::HostPortCallV2) -> Result<Value, sdk::host::HostError> {
        let call = serde_json::to_value(call).unwrap();
        let args = &call["args"];
        match call["method"].as_str().unwrap() {
            "host-media.create" => Ok(json!("request-media")),
            "host-media.write" | "host-media.finish" | "host-media.release" => Ok(Value::Null),
            "host-media.info" => Ok(json!({"byteLength":self.inbound.len().to_string()})),
            "host-media.read" => {
                let start: usize = args["offset"].as_str().unwrap().parse().unwrap();
                let end =
                    (start + args["maxBytes"].as_u64().unwrap() as usize).min(self.inbound.len());
                Ok(json!(&self.inbound[start..end]))
            }
            "host-http.execute" => {
                self.exchanges.push(args.clone());
                match self.replies.pop_front().expect("unexpected HTTP call") {
                    Reply::Status(status) => Ok(json!({"status":status})),
                    Reply::Json(value) => {
                        self.inbound = serde_json::to_vec(&value).unwrap();
                        Ok(json!({"status":200,"payload":{"handle":"response-media"}}))
                    }
                    Reply::Timeout => Err(serde_json::from_value(
                        json!({"code":"TIMEOUT","message":"fixture timeout"}),
                    )
                    .unwrap()),
                }
            }
            method => panic!("unexpected host method {method}"),
        }
    }
}

fn account() -> Value {
    json!({"endpointBaseUrl":"https://fixture.cn-beijing.maas.aliyuncs.com","credentialRef":"account"})
}

#[test]
fn qwen_never_retries_a_rejected_or_uncertain_submit() {
    for (reply, code) in [
        (Reply::Status(401), "UNAUTHORIZED"),
        (Reply::Timeout, "ACCEPTANCE_UNKNOWN"),
    ] {
        let mut host = FakeHost {
            replies: [reply].into(),
            ..FakeHost::default()
        };
        let payload = json!({"configuration":account(),"input":{"language":"en"}});
        assert_eq!(
            qwen::submit(&mut host, &payload, &wav(), 10, 60000)
                .unwrap_err()
                .code,
            code
        );
        assert_eq!(host.exchanges.len(), 1);
        assert_eq!(host.exchanges[0]["plan"]["method"], "POST");
        assert_eq!(host.exchanges[0]["credential"], "account");
    }
}

#[test]
fn async_result_failures_reuse_the_job_and_delete_the_source_file() {
    let payload = json!({"configuration":account(),"vendorTaskId":"job-123",
        "resumeToken":{"schemaVersion":1,"jobId":"job-123:10:file-fe-fixture"}});
    let failed = json!({"output":{"task_id":"job-123","task_status":"SUCCEEDED",
        "results":[{"subtask_status":"FAILED"}]}});
    let mut host = FakeHost {
        replies: [Reply::Json(failed), Reply::Status(404)].into(),
        ..FakeHost::default()
    };
    assert_eq!(
        task::query(&mut host, &payload, 10000).unwrap()["status"],
        "failed"
    );
    assert_eq!(host.exchanges.len(), 2);
    assert_eq!(host.exchanges[1]["plan"]["method"], "DELETE");

    let result = json!({"output":{"task_id":"job-123","task_status":"SUCCEEDED",
        "results":[{"subtask_status":"SUCCEEDED","transcription_url":"https://dashscope-result-bj.oss-cn-beijing.aliyuncs.com/out"}]}});
    let mut host = FakeHost {
        replies: [
            Reply::Json(result.clone()),
            Reply::Status(204),
            Reply::Status(503),
            Reply::Json(result),
            Reply::Status(404),
            Reply::Json(json!({"transcripts":[{"text":"test speech"}]})),
        ]
        .into(),
        ..FakeHost::default()
    };
    assert_eq!(
        task::query(&mut host, &payload, 10000).unwrap_err().code,
        "UPSTREAM_ERROR"
    );
    let value = task::query(&mut host, &payload, 10000).unwrap();
    assert_eq!(value["data"]["text"], "test speech");
    assert!(host
        .exchanges
        .iter()
        .all(|exchange| exchange["plan"]["method"] != "POST"));
    for exchange in [&host.exchanges[2], &host.exchanges[5]] {
        assert!(exchange["credential"].is_null());
        assert_eq!(exchange["plan"]["generatedHeaders"], json!([]));
    }
}

#[test]
fn signed_file_api_link_uses_https_and_failed_submission_deletes_only_its_own_file() {
    let mut host = FakeHost {
        replies: [
            Reply::Json(json!({"id":"file-fe-fixture"})),
            Reply::Json(json!({"data":{"url":"http://dashscope-file-mgr.oss-cn-beijing.aliyuncs.com/sample.wav?Signature=fixture"}})),
            Reply::Timeout,
            Reply::Json(json!({"deleted":true,"id":"file-fe-fixture"})),
        ].into(), ..FakeHost::default()
    };
    let payload = json!({"configuration":account(),"input":{"language":"en"}});
    let error =
        task::submit(&mut host, Model::Paraformer, &payload, &wav(), 10, 10000).unwrap_err();
    assert_eq!(error.code, "ACCEPTANCE_UNKNOWN");
    let methods: Vec<_> = host
        .exchanges
        .iter()
        .map(|call| call["plan"]["method"].as_str().unwrap())
        .collect();
    assert_eq!(methods, ["POST", "GET", "POST", "DELETE"]);
    assert!(host.exchanges[3]["plan"]["url"]
        .as_str()
        .unwrap()
        .ends_with("/files/file-fe-fixture"));
}

#[test]
fn qwen_refuses_oversize_encoded_audio_before_http() {
    let mut host = FakeHost::default();
    let payload = json!({"configuration":account()});
    let wav = vec![1; 7_500_001];
    assert_eq!(
        qwen::submit(&mut host, &payload, &wav, 10, 10000)
            .unwrap_err()
            .code,
        "INVALID_INPUT"
    );
    assert!(host.exchanges.is_empty());
}
