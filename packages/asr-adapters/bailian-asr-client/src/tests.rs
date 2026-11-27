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
