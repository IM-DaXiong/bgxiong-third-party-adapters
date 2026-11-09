#![cfg_attr(target_arch = "wasm32", no_std)]
//! Wan S2V: all graph topology, chunk quantization and Comfy mappings live here.
extern crate alloc;
mod comfy;
use alloc::{format, string::{String, ToString}, vec, vec::Vec};
use bgx_adapter_v2_sdk::{guest, Adapter, Host, Invocation, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PLUGIN: &str = "local.bgx.wan22-s2v";
const OPERATION: &str = "local.bgx.wan22-s2v/speak-standard";
const INPUT: &str = include_str!("../schemas/input.schema.json");
const OUTPUT: &str = include_str!("../schemas/output.schema.json");
const CONFIG: &str = include_str!("../schemas/config.schema.json");
const RESUME: &str = include_str!("../schemas/resume.schema.json");
#[derive(Debug)]
pub struct Failure { pub code: &'static str, pub message: String, pub vendor: Option<String> }
impl Failure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self { Self { code, message: message.into(), vendor: None } }
}
fn invalid(message: &str) -> Failure { Failure::new("INVALID_INPUT", message) }
fn schema(id: &str, text: &str) -> Value {
    let digest: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    json!({"id":id,"digest":digest,"schemaJson":text})
}
pub fn capabilities() -> Value {
    let i=schema("input",INPUT); let o=schema("output",OUTPUT);
    let c=schema("config",CONFIG); let r=schema("resume",RESUME);
    json!({"schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),"capabilityContracts":[{
        "schemaVersion":2,"localId":"driving-audio","capabilityKey":"video.speech","transportSlot":"video",
        "modelIds":["wan22-s2v-14b-standard"],"configurationSchema":{"id":"config","digest":c["digest"]},
        "operations":[{"id":OPERATION,"version":1,"label":"语音驱动",
            "inputSchema":{"id":"input","digest":i["digest"]},"outputSchema":{"id":"output","digest":o["digest"]},
            "resumeSchema":{"id":"resume","digest":r["digest"]},
            "mediaSlots":[{"role":"portrait","inputPointer":"/portrait","mediaKind":"image","minCount":1,"maxCount":1},
                {"role":"driving_audio","inputPointer":"/drivingAudio","mediaKind":"audio","minCount":1,"maxCount":1}],
            "outputSlots":[{"role":"video","kind":"video","minCount":1,"maxCount":1}],
            "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported","resumable":true,"remoteIdempotency":false},
            "uiSchema":{"controls":[
                {"fieldPointer":"/instruction","widget":"multiline","label":"动作要求","description":"","group":"输入"},
                {"fieldPointer":"/parameters/width","widget":"integer","label":"宽","description":"","group":"规格"},
                {"fieldPointer":"/parameters/height","widget":"integer","label":"高","description":"","group":"规格"},
                {"fieldPointer":"/parameters/seed","widget":"integer","label":"种子","description":"","group":"参数"}]}}],
        "schemas":[i,o,c,r]}]})
}

const FPS: u64 = 16;
const LATENTS_PER_CHUNK: u64 = 20; // ComfyUI's default 77-frame continuation.
const MAX_OUTPUT_FRAMES: u64 = 4096; // ImageFromBatch's length in the installed ComfyUI node.
const MAX_AUDIO_MS: u64 = (MAX_OUTPUT_FRAMES - 2) * 1000 / FPS;

/// The reference-latent prefix and removal of the first three decoded frames
/// leave four frames per sampled latent minus two.
fn timing(duration_ms: u64) -> Result<(Vec<u64>, u64, u64), Failure> {
    if !(1..=MAX_AUDIO_MS).contains(&duration_ms) {
        return Err(invalid("Driving audio exceeds this ComfyUI workflow's 4096-frame output capacity or is empty"));
    }
    let requested_frames = (duration_ms * FPS).div_ceil(1000);
    let requested_latents = (requested_frames + 2).div_ceil(4);
    let (chunks, latents) = if duration_ms <= 10_000 {
        (3, requested_latents.div_ceil(3) * 3)
    } else {
        (requested_latents.div_ceil(LATENTS_PER_CHUNK).max(3), requested_latents)
    };
    let lengths = (0..chunks).map(|index| {
        let chunk_latents = latents / chunks + u64::from(index < latents % chunks);
        chunk_latents * 4 - 3
    }).collect();
    let frames = latents * 4 - 2;
    Ok((lengths, frames, (frames * 1000).div_ceil(FPS)))
}
fn receipt(input: &Value) -> Value {
    json!({"portraitMediaId":input["portrait"][0]["mediaId"],"drivingAudioMediaId":input["drivingAudio"][0]["mediaId"],
        "drivingAudioSha256":input["audio"]["sha256"],"drivingAudioDurationMs":input["audio"]["durationMs"],"audioPolicy":"preserve_driving_audio"})
}
pub fn graph(input: &Value, image: &str, audio: &str) -> Result<Value,Failure> {
    let duration=input["audio"]["durationMs"].as_u64().ok_or_else(||invalid("Measured audio duration is required"))?;
    let (lengths,_,_)=timing(duration)?;
    let params=input["parameters"].as_object().ok_or_else(||invalid("Parameters are required"))?;
    if params.len()!=3 || params.keys().any(|k| !["width","height","seed"].contains(&k.as_str())) { return Err(invalid("Unknown or missing operation parameters")); }
    let seed=params["seed"].as_u64().filter(|s|*s<=4294967045).ok_or_else(||invalid("Invalid seed"))?;
    let mut g:Value=serde_json::from_str(include_str!("../workflows/api.json")).expect("frozen graph");
    for key in ["width","height"] {
        let value=params[key].as_u64().filter(|v| (16..=1280).contains(v) && v%16==0).ok_or_else(||invalid("Dimensions must be multiples of 16 between 16 and 1280"))?;
        g["153"]["inputs"][key]=json!(value);
    }
    for (node, length) in ["153", "172:76", "165:76"].into_iter().zip(&lengths) {
        g[node]["inputs"]["length"] = json!(length);
    }
    for (node, offset) in [("152",0),("172:77",1),("165:77",250)] { g[node]["inputs"]["seed"]=json!(seed+offset); }
    let mut previous = String::from("165:78");
    for (index, length) in lengths.iter().enumerate().skip(3) {
        let extend_id = format!("s2v_extend_{index}");
        let sample_id = format!("s2v_sample_{index}");
        let concat_id = format!("s2v_concat_{index}");
        let mut extend = g["165:76"].clone();
        extend["inputs"]["length"] = json!(length);
        extend["inputs"]["video_latent"] = json!([previous, 0]);
        let mut sample = g["165:77"].clone();
        sample["inputs"]["seed"] = json!((seed + 250 + index as u64 - 2) % (u64::from(u32::MAX) + 1));
        sample["inputs"]["positive"] = json!([extend_id, 0]);
        sample["inputs"]["negative"] = json!([extend_id, 1]);
        sample["inputs"]["latent_image"] = json!([extend_id, 2]);
        let mut concat = g["165:78"].clone();
        concat["inputs"]["samples1"] = json!([previous, 0]);
        concat["inputs"]["samples2"] = json!([sample_id, 0]);
        g[&extend_id] = extend;
        g[&sample_id] = sample;
        g[&concat_id] = concat;
        previous = concat_id;
    }
    g["157"]["inputs"]["samples"] = json!([previous, 0]);
    g["156"]["inputs"]["samples2"] = json!([previous, 0]);
    g["160"]["inputs"]["image"]=json!(image); g["162"]["inputs"]["audio"]=json!(audio);
    g["159"]["inputs"]["text"]=input["instruction"].clone();
    g["168"]["inputs"]["filename_prefix"]=json!("video/wan22-s2v");
    Ok(g)
}
fn preflight(p:&Value)->Result<Value,Failure> {
    let digest=&p["requestDigest"];
    if p["environmentReady"]!=true {
        return Ok(json!({"kind":"operation_preflight","status":"blocked","requestDigest":digest,
            "reasons":[{"code":"RESOURCE_UNAVAILABLE","message":"Validate the selected service and model resources first"}]}));
    }
    let input=&p["input"];
    graph(input,"preflight-image","preflight-audio")?;
    let duration=input["audio"]["durationMs"].as_u64().ok_or_else(||invalid("Audio duration is required"))?;
    let (_,frames,video_ms)=timing(duration)?;
    Ok(json!({"kind":"operation_preflight","status":"ready","requestDigest":digest,"specification":{
        "width":input["parameters"]["width"],"height":input["parameters"]["height"],"fps":16,"frameCount":frames,
        "audioDurationMs":duration,"videoDurationMs":video_ms,"maxTailDifferenceMs":750,"audioPolicy":"preserve_driving_audio"}}))
}

fn context<'a>(request:&Invocation,p:&'a Value)->comfy::Ctx<'a> { comfy::Ctx{config:&p["configuration"],budget_ms:request.r#budget_ms} }
fn persist(host:&mut dyn Host,request:&Invocation,accepted:&Value)->Result<(),Failure> {
    let unknown=||Failure::new("ACCEPTANCE_UNKNOWN","Remote accepted; checkpoint failed. Do not resubmit.");
    let saved=comfy::port(host,"host-checkpoint.read",json!({"requestId":request.r#request_id})).map_err(|_|unknown())?;
    let old=comfy::text(&saved,"revision").map_err(|_|unknown())?;
    let revision=old.parse::<u64>().ok().and_then(|v|v.checked_add(1)).ok_or_else(unknown)?;
    let mut row:Value=serde_json::from_str(comfy::text(&saved,"valueJson").map_err(|_|unknown())?).map_err(|_|unknown())?;
    row["revision"]=json!(revision.to_string());row["state"]=json!("accepted");
    row["vendorTaskId"]=accepted["vendorTaskId"].clone();row["resumeToken"]=accepted["resumeToken"].clone();row["result"]=accepted.clone();
    comfy::port(host,"host-checkpoint.write",json!({"requestId":request.r#request_id,"expectedRevision":old,"valueJson":row.to_string()})).map_err(|_|unknown())?;
    Ok(())
}
fn submit(host:&mut dyn Host,request:&Invocation,p:&Value)->Result<Value,Failure> {
    let input=&p["input"]; graph(input,"pending-image","pending-audio")?;
    let media=p["media"].as_array().filter(|v|v.len()==2).ok_or_else(||invalid("One portrait and one prepared audio lease are required"))?;
    let image_id=&input["portrait"][0]["mediaId"];let audio_id=&input["drivingAudio"][0]["mediaId"];
    let image=media.iter().find(|m|m["id"]==*image_id && m["kind"]=="image").ok_or_else(||invalid("Portrait lease missing"))?;
    let audio=media.iter().find(|m|m["id"]==*audio_id && m["kind"]=="audio").ok_or_else(||invalid("Audio lease missing"))?;
    let facts=comfy::port(host,"host-media.info",json!({"handle":audio["handle"]}))?;
    if facts["sha256"]!=input["audio"]["sha256"] || facts["durationMs"].as_str().and_then(|s|s.parse::<u64>().ok())!=input["audio"]["durationMs"].as_u64() {
        return Err(invalid("Prepared audio does not match frozen content or duration"));
    }
    let ctx=context(request,p);
    let image_path=comfy::upload(host,&ctx,image)?;let audio_path=comfy::upload(host,&ctx,audio)?;
    let graph=graph(input,&image_path,&audio_path)?;
    let remote=comfy::submit(host,&ctx,&graph,&request.r#request_id)?;
    let accepted=json!({"kind":"accepted","vendorTaskId":remote,"appliedParams":null,"usage":null,
        "resumeToken":{"promptId":remote,"receipt":receipt(input)}});
    persist(host,request,&accepted)?;Ok(accepted)
}
fn query(host:&mut dyn Host,request:&Invocation,p:&Value)->Result<Value,Failure> {
    let id=comfy::text(p,"vendorTaskId")?;
    if p["resumeToken"]["promptId"]!=id { return Err(invalid("Resume token must name the original prompt")); }
    let ctx=context(request,p);
    let (status,outputs,data,failure)=match comfy::status(host,&ctx,id,"168")? {
        comfy::Remote::Queued=>("queued",json!([]),Value::Null,Value::Null),
        comfy::Remote::Running=>("running",json!([]),Value::Null,Value::Null),
        comfy::Remote::Succeeded(files)=>{
            let output=comfy::download(host,&ctx,&files[0])?;
            let data=json!({"schemaVersion":1,"video":{"mediaId":output["media"]["id"]},"receipt":p["resumeToken"]["receipt"]});
            ("succeeded",json!([output]),data,Value::Null)
        },
        comfy::Remote::Failed(reason)=>("failed",json!([]),Value::Null,json!({"category":"vendorTaskFailed","message":"Wan S2V execution failed","vendorCode":reason,"details":{"promptId":id}})),
        comfy::Remote::Lost=>("failed",json!([]),Value::Null,json!({"category":"vendorTaskFailed","message":"Original prompt is absent from queue and history; no resubmission was attempted","vendorCode":"PROMPT_NOT_FOUND","details":{"promptId":id}})),
    };
    Ok(json!({"status":status,"outputs":outputs,"data":data,"usage":null,"progress":null,"terminalFailure":failure,"resumeToken":p["resumeToken"]}))
}

fn connectivity(host:&mut dyn Host,ctx:&comfy::Ctx)->Result<(),Failure> {
    comfy::status_probe(host,ctx)?;
    let frozen:Value=serde_json::from_str(include_str!("../workflows/api.json")).expect("frozen graph");
    for node in frozen.as_object().unwrap().values() {
        let class=comfy::text(node,"class_type")?;
        let info=comfy::exchange(host,ctx,"GET",&format!("/object_info/{class}"),json!({"kind":"none"}),vec![],false)?;
        if info.get(class).is_none() { return Err(Failure::new("RESOURCE_UNAVAILABLE",format!("Required node {class} is unavailable"))); }
        for field in ["unet_name","clip_name","vae_name","audio_encoder_name","lora_name"] {
            if let Some(expected)=node["inputs"].get(field) {
                let spec=&info[class]["input"]["required"][field];
                let options=if spec[0].is_array(){&spec[0]}else{&spec[1]["options"]};
                if !options.as_array().is_some_and(|a|a.contains(expected)) { return Err(Failure::new("RESOURCE_UNAVAILABLE",format!("Required model for {field} is unavailable"))); }
            }
        }
    }
    Ok(())
}
pub struct WanS2v;
impl Adapter for WanS2v {
    fn invoke(request:Invocation,host:&mut dyn Host)->Response {
        let wire=serde_json::to_value(&request).expect("typed invocation");
        let result=(||{
            let p:Value=serde_json::from_str(&request.r#payload_json).map_err(|_|invalid("Payload must contain JSON"))?;
            if matches!(wire["operation"].as_str(),Some("submit"|"query"|"upload")) && p["operationId"]!=OPERATION {return Err(invalid("Unknown speech operation"));}
            match wire["operation"].as_str() {
                Some("capabilities")=>Ok(capabilities()),Some("submit")=>submit(host,&request,&p),Some("query")=>query(host,&request,&p),
                Some("upload")=>{let value=comfy::upload(host,&context(&request,&p),&p["media"])?;Ok(json!({"mediaId":p["media"]["id"],"uploadRef":value}))},
                Some("probe")=>match p["kind"].as_str(){
                    Some("operation_preflight")=>{if p["operationId"]!=OPERATION{return Err(invalid("Unknown operation"));}preflight(&p)},
                    Some("package_validation")=>Ok(json!({"packageValid":true,"connection":"not_checked","authentication":"not_checked","resources":"not_checked","issues":[]})),
                    Some("connectivity")=>{connectivity(host,&context(&request,&p))?;Ok(json!({"packageValid":true,"connection":"ready","authentication":"ready","resources":"ready","issues":[]}))},
                    _=>Err(invalid("Unknown probe kind"))},
                _=>Err(Failure::new("UNSUPPORTED_FEATURE","Transport operation is unsupported")),
            }
        })();
        match result {
            Ok(value)=>guest::completed_response(&request,&value).expect("public response"),
            Err(error)=>serde_json::from_value(json!({"protocolVersion":2,"operation":request.r#operation,"error":{
                "code":error.code,"message":error.message,"retryable":false,"vendorCode":error.vendor,"vendorRequestId":null}})).expect("public error"),
        }
    }
}
bgx_adapter_v2_sdk::export_adapter!(WanS2v);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timing_preserves_audio_and_bounds_quantization_without_changing_the_frame_rate() {
        for ms in 1..=MAX_AUDIO_MS {
            let (lengths,frames,video)=timing(ms).unwrap();
            assert!(lengths.iter().all(|length| *length > 0 && *length <= 77));
            assert!(frames <= MAX_OUTPUT_FRAMES);
            assert!(frames*1000>=ms*FPS);
            assert!(video-ms<=750);
        }
        assert!(timing(0).is_err());assert!(timing(MAX_AUDIO_MS + 1).is_err());
    }
    #[test]
    fn certified_three_chunk_graph_retains_original_audio_and_all_continuation_edges() {
        let input=json!({"instruction":"","audio":{"durationMs":2000},"parameters":{"width":512,"height":512,"seed":20}});
        let g=graph(&input,"portrait.png","drive.wav").unwrap();
        assert_eq!(g["167"]["inputs"]["audio"],json!(["162",0]));
        assert_eq!(g["162"]["inputs"]["audio"],"drive.wav");
        assert_eq!(g["153"]["inputs"]["length"],9);assert_eq!(g["165:76"]["inputs"]["length"],9);
        assert_eq!(g["165:76"]["inputs"]["video_latent"],json!(["172:78",0]));
        assert_eq!(g["167"]["inputs"]["fps"],16);
    }
    #[test]
    fn native_720p_long_audio_extends_the_official_chain_and_keeps_original_audio() {
        let input = json!({"instruction":"", "audio":{"durationMs":20000}, "parameters":{"width":1280,"height":720,"seed":20}});
        let g = graph(&input, "portrait.png", "drive.wav").unwrap();
        assert_eq!(g["153"]["inputs"]["width"], 1280);
        assert_eq!(g["153"]["inputs"]["height"], 720);
        assert_eq!(g["s2v_extend_3"]["inputs"]["video_latent"], json!(["165:78",0]));
        assert_eq!(g["s2v_extend_4"]["inputs"]["video_latent"], json!(["s2v_concat_3",0]));
        assert_eq!(g["156"]["inputs"]["samples2"], json!(["s2v_concat_4",0]));
        assert_eq!(g["157"]["inputs"]["samples"], json!(["s2v_concat_4",0]));
        assert_eq!(g["167"]["inputs"]["audio"], json!(["162",0]));
    }
}
