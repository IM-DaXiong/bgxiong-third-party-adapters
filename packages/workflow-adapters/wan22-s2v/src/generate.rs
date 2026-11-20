use alloc::{format, vec::Vec};

use bgx_adapter_v2_sdk::{Host, Invocation};
use serde_json::{json, Value};

use crate::{build_graph, comfy, context, invalid, persist, schema, timing, Failure, FPS};

pub(crate) const OPERATION: &str = "local.bgx.wan22-s2v/generate-video";
const INPUT_SCHEMA: &str = include_str!("../schemas/generate.input.schema.json");

pub(crate) fn contract() -> Value {
    let input = schema("input", INPUT_SCHEMA);
    let output = schema("output", include_str!("../schemas/generate.output.schema.json"));
    let resume = schema("resume", include_str!("../schemas/generate.resume.schema.json"));
    let config = schema("config", crate::CONFIG);
    let controls: Vec<_> = [
        ("/prompt", "multiline", "动作要求", "prompt"),
        ("/aspectRatio", "select", "画幅", "aspectRatio"),
        ("/resolution", "select", "分辨率", "resolution"),
        ("/durationSeconds", "integer", "时长", "duration"),
        ("/fps", "integer", "帧率", "fps"),
    ].into_iter().map(|(path, widget, label, role)| json!({
        "fieldPointer":path,"widget":widget,"label":label,"description":"","group":"生成","displayRole":role
    })).chain([
        json!({"fieldPointer":"/seed","widget":"integer","label":"种子","description":"","group":"参数"}),
        json!({"fieldPointer":"/image","widget":"media","label":"人物图","description":"","group":"输入"}),
        json!({"fieldPointer":"/audio","widget":"media","label":"驱动音频","description":"","group":"输入"}),
    ]).collect();
    json!({"schemaVersion":2,"localId":"generate-video","capabilityKey":"video.generate","transportSlot":"video",
        "modelIds":["wan22-s2v-14b-video"],"configurationSchema":{"id":"config","digest":config["digest"]},
        "operations":[{"id":OPERATION,"version":1,"label":"图像和音频生视频","inputProfile":"operation-input-v1",
            "inputSchema":{"id":"input","digest":input["digest"]},"outputSchema":{"id":"output","digest":output["digest"]},
            "resumeSchema":{"id":"resume","digest":resume["digest"]},"uiSchema":{"controls":controls},
            "mediaSlots":[{"role":"first_frame","inputPointer":"/image","mediaKind":"image","minCount":1,"maxCount":1},
                          {"role":"driving_audio","inputPointer":"/audio","mediaKind":"audio","minCount":1,"maxCount":1}],
            "outputSlots":[{"role":"video","kind":"video","minCount":1,"maxCount":1}],
            "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported","resumable":true,"remoteIdempotency":false}}],
        "schemas":[input,output,config,resume]})
}

struct Parameters {
    dimensions: (u64, u64),
    seconds: u64,
    seed: u64,
    maximum_seconds: u64,
}

fn parameters(input: &Value) -> Result<Parameters, Failure> {
    let settings: Value = serde_json::from_str(INPUT_SCHEMA).expect("bundled input contract");
    let fields = &settings["properties"];
    let maximum_seconds = fields["durationSeconds"]["maximum"].as_u64().expect("duration limit");
    if input["inputMode"] != "image_to_video" || input["fps"].as_u64() != Some(FPS) || !input["prompt"].is_string() {
        return Err(invalid("需要图像与音频输入，帧率固定为 16 FPS"));
    }
    let seconds = input["durationSeconds"].as_u64().filter(|n| *n <= maximum_seconds)
        .ok_or_else(|| invalid("时长须随音频或在 1–30 秒之间"))?;
    let seed = input["seed"].as_u64().filter(|seed| *seed <= 4294967045)
        .ok_or_else(|| invalid("种子超出范围"))?;
    let option = settings["allOf"].as_array().expect("resolution groups").iter()
        .find(|group| group["if"]["properties"]["aspectRatio"]["const"] == input["aspectRatio"])
        .and_then(|group| group["then"]["properties"]["resolution"]["enum"].as_array())
        .and_then(|options| options.iter().find(|id| *id == &input["resolution"]))
        .ok_or_else(|| invalid("请选择当前画幅的分辨率档位"))?;
    let (width, height) = option.as_str().and_then(|id| id.split_once('x')).expect("dimension preset");
    let dimensions = (width.parse().expect("preset width"), height.parse().expect("preset height"));
    Ok(Parameters { dimensions, seconds, seed, maximum_seconds })
}

fn media_id<'a>(input: &'a Value, key: &str) -> Result<&'a str, Failure> {
    let row = input[key].as_array().and_then(|items| match items.as_slice() { [item] => Some(item), _ => None });
    row.and_then(|item| item["mediaId"].as_str()).filter(|id| !id.is_empty())
        .ok_or_else(|| invalid("需要一张人物图和一条驱动音频"))
}

pub(crate) fn preflight(payload: &Value) -> Value {
    let result = (|| {
        if payload["selection"]["operationId"] != OPERATION || payload["environment"]["ready"] != true {
            return Err(invalid("请先确认 Wan S2V 的运行服务与模型资源"));
        }
        let input = &payload["input"];
        parameters(input)?;
        media_id(input, "image")?;
        media_id(input, "audio")?;
        Ok::<_, Failure>(())
    })();
    let digest = &payload["requestDigest"];
    match result {
        Ok(()) => json!({"kind":"operation_input_preflight","status":"ready","requestDigest":digest,"material":{}}),
        Err(error) => json!({"kind":"operation_input_preflight","status":"rejected","requestDigest":digest,
            "diagnostics":[{"code":"local.bgx.wan22-s2v/invalid_input","fieldPointer":"","message":error.message}]}),
    }
}

fn planned_duration(params: &Parameters, source_ms: u64) -> Result<u64, Failure> {
    let selected_ms = if params.seconds == 0 { source_ms } else { params.seconds * 1000 };
    if selected_ms == 0 || selected_ms > source_ms {
        return Err(invalid("选用的时长超过驱动音频，或音频时长不可读取"));
    }
    if selected_ms > params.maximum_seconds * 1000 {
        return Err(Failure::new("INVALID_INPUT", format!("驱动音频超过容量，请把时长调至 {} 秒以内", params.maximum_seconds)));
    }
    Ok(selected_ms)
}

fn graph(input: &Value, params: &Parameters, image: &str, audio: &str, selected_ms: u64, source_ms: u64) -> Result<Value, Failure> {
    let (lengths, _, _) = timing(selected_ms)?;
    let mut graph = build_graph(&json!({"instruction":input["prompt"],"parameters":{"seed":params.seed}}), image, audio, params.dimensions, &lengths)?;
    graph["155"]["inputs"]["length"] = json!((selected_ms * FPS).div_ceil(1000));
    if selected_ms < source_ms {
        graph["wan_audio_selection"] = json!({"class_type":"TrimAudioDuration","inputs":{
            "audio":["162",0],"start_index":0.0,"duration":selected_ms as f64 / 1000.0}});
        for node in ["173", "167"] {
            graph[node]["inputs"]["audio"] = json!(["wan_audio_selection",0]);
        }
    }
    Ok(graph)
}

pub(crate) fn submit(host: &mut dyn Host, request: &Invocation, payload: &Value) -> Result<Value, Failure> {
    let input = &payload["input"];
    let params = parameters(input)?;
    let media = payload["media"].as_array().filter(|items| items.len() == 2)
        .ok_or_else(|| invalid("需要一张人物图和一条驱动音频"))?;
    let lease = |key, kind| {
        let id = media_id(input, key)?;
        media.iter().find(|item| item["id"] == id && item["kind"] == kind)
            .ok_or_else(|| invalid("素材租约与工作台输入不一致"))
    };
    let image = lease("image", "image")?;
    let audio = lease("audio", "audio")?;
    let facts = comfy::port(host,"host-media.info",json!({"handle":audio["handle"]}))?;
    let source_ms = facts["durationMs"].as_str().and_then(|value| value.parse().ok())
        .ok_or_else(|| invalid("无法测量驱动音频时长"))?;
    let selected_ms = planned_duration(&params, source_ms)?;
    let ctx = context(request, payload);
    if selected_ms < source_ms {
        let nodes = comfy::exchange(host,&ctx,"GET","/object_info/TrimAudioDuration",json!({"kind":"none"}),Vec::new(),false)?;
        if nodes.get("TrimAudioDuration").is_none() {
            return Err(Failure::new("RESOURCE_UNAVAILABLE", "ComfyUI 缺少 TrimAudioDuration 音频裁剪节点"));
        }
    }
    let image_path = comfy::upload(host, &ctx, image)?;
    let audio_path = comfy::upload(host, &ctx, audio)?;
    let prompt = graph(input, &params, &image_path, &audio_path, selected_ms, source_ms)?;
    let remote = comfy::submit(host, &ctx, &prompt, &request.r#request_id)?;
    let accepted = json!({"kind":"accepted","vendorTaskId":remote,"appliedParams":null,"usage":null,
        "resumeToken":{"promptId":remote,"frames":(selected_ms * FPS).div_ceil(1000),"audioDurationMs":selected_ms}});
    persist(host, request, &accepted)?;
    Ok(accepted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_uses_the_same_audio_for_conditioning_and_output() {
        let mut input = json!({"prompt":"","inputMode":"image_to_video","aspectRatio":"3:4",
            "resolution":"2880x3840","fps":16,"durationSeconds":0,"seed":7});
        let params = parameters(&input).unwrap();
        let selected = planned_duration(&params, 8328).unwrap();
        let g = graph(&input, &params, "image", "audio", selected, 8328).unwrap();
        assert_eq!(g["153"]["inputs"]["width"], 2880);
        assert_eq!(g["153"]["inputs"]["height"], 3840);
        assert_eq!(g["155"]["inputs"]["length"], 134);
        assert_eq!(g["167"]["inputs"]["audio"], g["173"]["inputs"]["audio"]);
        assert!(g.get("wan_audio_selection").is_none());

        input["durationSeconds"] = json!(5);
        let params = parameters(&input).unwrap();
        let selected = planned_duration(&params, 8328).unwrap();
        let g = graph(&input, &params, "image", "audio", selected, 8328).unwrap();
        assert_eq!(g["155"]["inputs"]["length"], 80);
        assert_eq!(g["167"]["inputs"]["fps"], 16);
        assert_eq!(g["wan_audio_selection"]["inputs"]["duration"], 5.0);
        assert_eq!(g["167"]["inputs"]["audio"], json!(["wan_audio_selection", 0]));
        assert_eq!(g["167"]["inputs"]["audio"], g["173"]["inputs"]["audio"]);
        assert!(planned_duration(&params, 4999).is_err());
        input["aspectRatio"] = json!("4:3");
        assert!(parameters(&input).is_err());
    }
}
