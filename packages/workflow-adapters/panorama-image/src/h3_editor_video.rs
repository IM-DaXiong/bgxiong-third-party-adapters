//! Frozen H3 CrossView graph driven by the user's Studio scene and owned video.
extern crate alloc;
use alloc::{collections::BTreeMap, string::{String,ToString}};
use serde_json::{json,Value};
use crate::{mapping::Failure,scene::SceneDocument};

pub const OPERATION:&str="local.thirdparty.comfy-panorama-image/h3-studio-video";
pub const MODEL:&str="panorama-h3-studio-video";
pub fn matches(payload:&Value)->bool {
    payload["operationId"]==OPERATION && payload["modelId"]==MODEL
        && payload["capabilityKey"]=="video.generate" && payload["operationVersion"]==1
}
pub fn contract(base:&Value)->Value {
    let mut c=crate::editor_video::contract(base);
    let mut schema:Value=serde_json::from_str(c["schemas"][0]["schemaJson"].as_str().unwrap()).unwrap();
    schema["properties"]["prompt"]=json!({"type":"string","minLength":1,"maxLength":8000});
    schema["properties"]["seed"]=json!({"type":"integer","minimum":0,"maximum":9007199254740991u64,"default":12345,"title":"随机种子"});
    schema["required"].as_array_mut().unwrap().push(json!("seed"));
    schema["properties"]["advanced"]["properties"]["multiple"]=json!({"type":"integer","minimum":16,"maximum":128,"multipleOf":16,"default":16,"title":"尺寸倍数"});
    schema["properties"]["advanced"]["default"]["multiple"]=json!(16);
    for (key,field,default) in [
        ("sourceName",json!({"type":"string","minLength":1,"title":"参考视频素材"}),json!("")),
        ("referenceStrength",json!({"type":"number","minimum":0,"maximum":2,"title":"参考强度"}),json!(1)),
        ("crossviewStrength",json!({"type":"number","minimum":0,"maximum":2,"title":"视角强度"}),json!(0.8)),
        ("depthResolution",json!({"type":"integer","minimum":0,"maximum":9,"title":"深度分辨等级"}),json!(9)),
    ] {
        schema["properties"]["advanced"]["properties"][key]=field;
        schema["properties"]["advanced"]["properties"][key]["default"]=default.clone();
        schema["properties"]["advanced"]["default"][key]=default;
        schema["properties"]["advanced"]["required"].as_array_mut().unwrap().push(json!(key));
    }
    // A missing source must be selected before submission, not fabricated by a default.
    schema["properties"]["advanced"]["properties"]["sourceName"].as_object_mut().unwrap().remove("minLength");
    let s=crate::capability::schema("h3-studio-video-input",&schema.to_string());
    c["localId"]=json!(MODEL);c["modelIds"]=json!([MODEL]);
    c["operations"][0]["id"]=json!(OPERATION);c["operations"][0]["label"]=json!("H3 · 工程新视角视频");
    c["operations"][0]["inputSchema"]=json!({"id":"h3-studio-video-input","digest":s["digest"]});c["schemas"][0]=s;c
}
fn document(input:&Value)->Result<SceneDocument,Failure> {
    let d=SceneDocument::parse(input["editorDocument"].as_str().ok_or_else(||Failure::invalid("请先保存全景工程"))?)?;
    d.validate_references(input)?;Ok(d)
}
pub fn source_id(input:&Value)->Result<String,Failure> {
    let d=document(input)?;
    let name=input["advanced"]["sourceName"].as_str().filter(|v|!v.is_empty()).ok_or_else(||Failure::invalid("请选择工程中的参考视频素材"))?;
    d.assets.get(name).cloned().ok_or_else(||Failure::invalid("参考视频不属于当前冻结工程"))
}
fn timing(state:&Value)->Result<u64,Failure> {
    if state["timeline"]["fps"].as_f64()!=Some(24.0){return Err(Failure::invalid("H3需要24 fps工程；请显式调整，不会自动转换帧率"));}
    state["timeline"]["length"].as_u64().filter(|n|(5..=3600).contains(n)&&(*n-5)%17==0)
        .ok_or_else(||Failure::invalid("H3工程长度须为17n+5帧（5至3600），不会自动裁切或补帧"))
}
/// These facts come from the SDK's measured lease, never from editable parameters.
pub fn validate_source(input:&Value,stat:&Value)->Result<(),Failure> {
    let frames=timing(&document(input)?.state)?;
    if !stat["mime"].as_str().is_some_and(|m|m.starts_with("video/")) {return Err(Failure::invalid("参考素材必须是视频"));}
    let timestamps=stat["timestampsMs"].as_array().ok_or_else(||Failure::invalid("缺少视频实际帧时间，请启用工程视频的帧探测"))?;
    if timestamps.len() as u64!=frames {return Err(Failure::invalid("参考视频实际帧数与工程长度不一致；请先显式剪辑"));}
    for (index,value) in timestamps.iter().enumerate() {
        let measured=value.as_str().and_then(|s|s.parse::<u64>().ok()).ok_or_else(||Failure::invalid("视频帧时间无效"))?;
        if (measured as f64-index as f64*1000.0/24.0).abs()>1.0 {return Err(Failure::invalid("参考视频不是恒定24 fps；请先显式转换帧率"));}
    }
    Ok(())
}
pub fn graph(input:&Value,uploads:&BTreeMap<String,String>)->Result<(Value,Value),Failure> {
    let doc=document(input)?;let state=doc.bind(uploads)?;timing(&state)?;
    let source=source_id(input)?;
    let path=uploads.get(&source).ok_or_else(||Failure::invalid("参考视频上传凭据缺失"))?;
    let (render,mut applied)=crate::editor_video::graph(input,uploads)?;
    let prompt=input["prompt"].as_str().filter(|s|!s.trim().is_empty()).ok_or_else(||Failure::invalid("请输入H3视频提示词"))?;
    let seed=input["seed"].as_u64().filter(|n|*n<=9007199254740991).ok_or_else(||Failure::invalid("随机种子无效"))?;
    let multiple=input["advanced"]["multiple"].as_u64().filter(|n|*n>=16 && *n%16==0).ok_or_else(||Failure::invalid("H3尺寸倍数须为16的整数倍"))?;
    let mp=input["resolutionValue"]["value"].as_f64().unwrap();
    let mut g:Value=serde_json::from_str(include_str!("../workflow/api/pika-h3-flex-moge3-crossview.json")).expect("frozen H3 graph");
    g["156"]=render["70"].clone();g["156"]["inputs"]["world"]=json!(["142",0]);
    doc.connect_parents(&mut g,"156",uploads,mp,multiple)?;
    g["152"]["inputs"]["video"]=json!(path);
    // Actual frame count and rate were verified before upload. Never trim or resample.
    g["152"]["inputs"]["frame_load_cap"]=json!(0);
    g["152"]["inputs"]["force_rate"]=json!(0);
    g["149"]["inputs"]["resize_type.megapixels"]=json!(mp);
    g["123"]["inputs"]["prompt"]=json!(prompt);g["105"]["inputs"]["noise_seed"]=json!(seed);
    for (node,key) in [("108","referenceStrength"),("139","crossviewStrength")] {
        let value=input["advanced"][key].as_f64().filter(|v|(0.0..=2.0).contains(v)).ok_or_else(||Failure::invalid("H3强度超出范围"))?;
        g[node]["inputs"]["strength_model"]=json!(value);
    }
    g["142"]["inputs"]["resolution_level"]=json!(input["advanced"]["depthResolution"].as_u64().filter(|v|*v<=9).ok_or_else(||Failure::invalid("深度等级无效"))?);
    g["66"]=g["155"].clone();g["66"]["inputs"]["filename_prefix"]=json!("bgx_h3_studio");
    for id in ["155","145","147"] {g.as_object_mut().unwrap().remove(id);}
    applied["prompt"]=json!(prompt);applied["seed"]=json!(seed);Ok((g,applied))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input()->Value {json!({"editorDocument":json!({"stateText":json!({"version":2,"timeline":{"fps":24,"length":73},"camera":{"projection":"erp","erp_range":360},"stickers":[]}).to_string(),"assets":[{"name":"input//studio.mp4","mediaId":"video"}]}).to_string(),"references":[{"mediaId":"video"}],"prompt":"crossview. a studio","seed":12345,"outputProjection":"equirectangular360","resolutionValue":{"kind":"megapixels","value":0.5},"advanced":{"multiple":16,"backgroundColor":"#00ff00","backgroundRange":"360","sourceName":"input//studio.mp4","referenceStrength":1,"crossviewStrength":0.8,"depthResolution":9}})}
    #[test]
    fn saved_scene_video_and_audio_keep_native_h3_connections() {
        let input=input();let (g,applied)=graph(&input,&BTreeMap::from([("video".into(),"owned/source.mp4".into())])).unwrap();
        assert_eq!(g["152"]["inputs"]["video"],"owned/source.mp4");assert_eq!(g["152"]["inputs"]["frame_load_cap"],0);
        assert_eq!(g["123"]["inputs"]["ref_video_audios.ref_video_audio_0"],json!(["152",2]));
        assert_eq!(g["91"]["inputs"]["audio"],json!(["77",0]));assert_eq!(g["153"]["inputs"]["image"],json!(["143",0]));
        assert_eq!(g["66"]["inputs"]["video"],json!(["91",0]));assert_eq!(g["91"]["inputs"]["fps"],24);
        assert!(applied.as_object().unwrap().keys().all(|k|input.get(k).is_some()));
        for node in g.as_object().unwrap().values(){for v in node["inputs"].as_object().unwrap().values(){if let Some(link)=v.as_array().filter(|a|a.len()==2&&a[0].is_string()&&a[1].is_number()){assert!(g.get(link[0].as_str().unwrap()).is_some(),"missing {link:?}");}}}
    }
    #[test]
    fn measured_frames_reject_missing_facts_rate_change_or_implicit_trim() {
        let input=input();let mut stat=json!({"mime":"video/mp4","timestampsMs":(0..73).map(|i|((i as f64*1000.0/24.0).round() as u64).to_string()).collect::<alloc::vec::Vec<_>>()});
        validate_source(&input,&stat).unwrap();stat["timestampsMs"][50]=json!("1700");assert!(validate_source(&input,&stat).is_err());
        stat["timestampsMs"]=json!([]);assert!(validate_source(&input,&stat).is_err());stat["timestampsMs"]=Value::Null;assert!(validate_source(&input,&stat).is_err());
        for (fps,length) in [(29.97,73),(24.0,72),(24.0,0)] {assert!(timing(&json!({"timeline":{"fps":fps,"length":length}})).is_err());}
    }
    #[test]
    fn source_validation_precedes_network_and_checkpoint_records_one_submit() {
        use bgx_adapter_v2_sdk::{Host,host};
        struct H { valid:bool, uploads:usize, prompts:usize, saved:bool }
        impl Host for H {
            fn call(&mut self,c:bgx_adapter_v2_sdk::HostPortCallV2)->Result<Value,host::HostError>{
                let c=serde_json::to_value(c).unwrap();
                match c["method"].as_str().unwrap(){
                    "host-media.info"=>return Ok(json!({"id":"video","kind":"file","mime":"video/mp4","byteLength":"100","sha256":"a".repeat(64),"width":256,"height":128,"durationMs":"3042","timestampsMs":if self.valid {json!((0..73).map(|i|((i as f64*1000.0/24.0).round() as u64).to_string()).collect::<alloc::vec::Vec<_>>())}else{Value::Null},"fileName":"video.mp4"})),
                    "host-checkpoint.read"=>return Ok(json!({"revision":"1","valueJson":"{}"})),
                    "host-checkpoint.write"=>{let v:Value=serde_json::from_str(c["args"]["valueJson"].as_str().unwrap()).unwrap();assert_eq!(v["vendorTaskId"],"one-h3-task");self.saved=true;return Ok(json!({"revision":"2"}));},
                    "host-http.execute"=>{},
                    other=>panic!("unexpected {other}"),
                }
                let plan=&c["args"]["plan"];let url=plan["url"].as_str().unwrap();
                let body=if url.ends_with("/upload/image"){self.uploads+=1;json!({"name":"source.mp4","subfolder":"owned","type":"input"})}else{
                    assert!(url.ends_with("/prompt"));self.prompts+=1;assert_eq!(self.uploads,1);
                    let bytes:alloc::vec::Vec<u8>=serde_json::from_value(plan["body"]["bytes"].clone()).unwrap();let g=serde_json::from_slice::<Value>(&bytes).unwrap();assert_eq!(g["prompt"]["152"]["inputs"]["video"],"owned/source.mp4");json!({"prompt_id":"one-h3-task","node_errors":{}})
                };Ok(json!({"status":200,"headers":[],"payload":{"kind":"buffered","bytes":serde_json::to_vec(&body).unwrap()}}))
            }
        }
        let payload=json!({"operationId":OPERATION,"modelId":MODEL,"operationVersion":1,"capabilityKey":"video.generate","input":input(),"media":[{"id":"video","kind":"file","mime":"video/mp4","handle":"lease-video","fileName":"video.mp4"}],"configuration":{"endpointBaseUrl":"http://127.0.0.1:8188","credentialRef":null}});
        let request=serde_json::from_value(json!({"protocolVersion":2,"operation":"submit","profileId":"binding","capabilitySlot":"video","requestId":"h3-test","budgetMs":1000,"payloadJson":payload.to_string(),"bindingId":"binding","negotiatedVersion":2,"enabledFeatures":[]})).unwrap();
        let mut h=H{valid:false,uploads:0,prompts:0,saved:false};assert!(crate::submit(&mut h,&request,&payload).is_err());assert_eq!((h.uploads,h.prompts),(0,0));
        h.valid=true;let result=crate::submit(&mut h,&request,&payload).unwrap();assert_eq!(result["vendorTaskId"],"one-h3-task");assert_eq!((h.uploads,h.prompts),(1,1));assert!(h.saved);
    }
}
