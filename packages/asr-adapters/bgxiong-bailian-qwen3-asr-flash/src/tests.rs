#[test]
fn declaration_contains_the_exact_model_and_operation() {
    let capabilities = include_str!("capabilities.json");
    assert!(capabilities.contains("\"qwen3-asr-flash\""));
    assert!(capabilities.contains("\"std/speech.transcribe\""));
}
