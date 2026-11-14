//! Model-free Studio capture. Frame, coverage mask and pose come from one upstream
//! execution; never reconstruct camera lanes or render pixels in the host.
extern crate alloc;
use alloc::{collections::BTreeMap, string::ToString, vec::Vec};
use bgx_adapter_v2_sdk::{Host, Invocation};
use serde_json::{json, Value};
use crate::{comfy, mapping::Failure, scene::SceneDocument};

pub const OPERATION: &str = "local.thirdparty.comfy-panorama-image/editor-frame-capture";
pub const MODEL: &str = "panorama-editor-capture";
pub const CAPABILITY: &str = "local.thirdparty.comfy-panorama-image/editor-capture";

pub fn matches(payload: &Value) -> bool {
    payload["operationId"]==OPERATION && payload["modelId"]==MODEL
        && payload["capabilityKey"]==CAPABILITY && payload["operationVersion"]==1
}

pub fn contract(base: &Value) -> Value {
    let mut c=base.clone();
    let mut input=crate::workflows::schema("studio-render");
    input["properties"].as_object_mut().unwrap().remove("prompt");
    input["required"].as_array_mut().unwrap().retain(|v|v!="prompt");
    input["properties"]["outputProjection"]=json!({"type":"string","enum":["flat"],"default":"flat"});
    input["properties"]["sourceRevision"]=json!({"type":"string","minLength":1,"readOnly":true});
    input["required"].as_array_mut().unwrap().push(json!("sourceRevision"));
    let s=crate::capability::schema("capture-input",&input.to_string());
    let output=crate::capability::schema("capture-output",&json!({"type":"object","additionalProperties":false,
        "properties":{"mediaArtifacts":serde_json::from_str::<Value>(crate::capability::OUTPUT_SCHEMA).unwrap()["properties"]["mediaArtifacts"],
        "sourceRevision":{"type":"string"},"stickerState":{"type":"string"}},
        "required":["mediaArtifacts","sourceRevision","stickerState"]}).to_string());
    let resume=crate::capability::schema("capture-resume",&json!({"type":"object","additionalProperties":false,
        "properties":{"promptId":{"type":"string","minLength":1},"sourceRevision":{"type":"string","minLength":1},"sourceContextJson":{"type":"string","minLength":1}},
        "required":["promptId","sourceRevision","sourceContextJson"]}).to_string());
    c["localId"]=json!(MODEL); c["modelIds"]=json!([MODEL]);c["capabilityKey"]=json!(CAPABILITY);c["transportSlot"]=json!("generic");
    let op=&mut c["operations"][0];op["id"]=json!(OPERATION);op["label"]=json!("取景与遮罩");
    op["inputSchema"]=json!({"id":"capture-input","digest":s["digest"]});
    op["outputSchema"]=json!({"id":"capture-output","digest":output["digest"]});
    op["resumeSchema"]=json!({"id":"capture-resume","digest":resume["digest"]});
    op["mediaSlots"]=json!([{"role":"references","inputPointer":"/references","mediaKind":"file","minCount":0,"maxCount":1024}]);
    op["uiSchema"]=json!({"controls":[]});
    op["outputSlots"]=json!([
        {"role":"frame","kind":"image","minCount":1,"maxCount":1},
        {"role":"mask","kind":"image","minCount":1,"maxCount":1},
        {"role":"pose","kind":"file","minCount":1,"maxCount":1}]);
    c["schemas"]=json!([s,output,resume,crate::capability::schema("config",crate::capability::CONFIG_SCHEMA)]);c
}

pub fn graph(input:&Value, uploads:&BTreeMap<alloc::string::String,alloc::string::String>) -> Result<Value,Failure> {
    let doc=SceneDocument::parse(input["editorDocument"].as_str().ok_or_else(||Failure::invalid("Saved document required"))?)?;
    doc.validate_references(input)?;
    if doc.state["camera"]["projection"]!="perspective" || input["outputProjection"]!="flat"
        || input["sourceRevision"].as_str().is_none_or(str::is_empty) {
        return Err(Failure::invalid("Capture requires a saved Frame camera and source revision"));
    }
    let mp=input["advanced"]["megapixels"].as_f64().ok_or_else(||Failure::invalid("Missing megapixels"))?;
    let multiple=input["advanced"]["multiple"].as_u64().ok_or_else(||Failure::invalid("Missing output multiple"))?;
    let mut g=crate::scene::render_graph(&doc.bind(uploads)?,mp,multiple)?;
    doc.connect_parents(&mut g,"70",uploads,mp,multiple)?;
    g["67"]=json!({"class_type":"MaskToImage","inputs":{"mask":["70",2]}});
    g["68"]=json!({"class_type":"SaveImage","inputs":{"images":["67",0],"filename_prefix":"bgx_frame_mask"}});
    g["69"]=json!({"class_type":"PreviewAny","inputs":{"source":["70",4]}});
    Ok(g)
}

pub fn submit(host:&mut dyn Host,request:&Invocation,payload:&Value)->Result<Value,Failure> {
    if !matches(payload) {return Err(Failure::invalid("Capture identity differs"));}
    let input=&payload["input"];
    let document=SceneDocument::parse(input["editorDocument"].as_str().ok_or_else(||Failure::invalid("Saved document required"))?)?;
    let mut uploads=BTreeMap::new();let mut media=BTreeMap::new();
    for (name,id) in document.assets.iter().chain(document.sources.iter().flat_map(|s|s.document.assets.iter())) {
        if media.insert(id.clone(),crate::scene_media(payload,id,name)?).is_some() {return Err(Failure::invalid("Duplicate source lease"));}
        uploads.insert(id.clone(),"pending-upload.png".into());
    }
    graph(input,&uploads)?;
    let source_context=crate::editor_provenance::context(host,payload,&document)?;
    let ctx=comfy::Ctx{config:&payload["configuration"],budget_ms:request.r#budget_ms};
    for (id,source) in media {uploads.insert(id,comfy::upload(host,&ctx,&source)?);}
    let remote=comfy::submit(host,&ctx,&graph(input,&uploads)?,&request.r#request_id)?;
    let accepted=json!({"kind":"accepted","vendorTaskId":remote,
        "resumeToken":{"promptId":remote,"sourceRevision":input["sourceRevision"],"sourceContextJson":source_context.to_string()},"appliedParams":input,"usage":null});
    crate::persist_accepted(host,request,&accepted)?;Ok(accepted)
}

pub fn query(host:&mut dyn Host,request:&Invocation,payload:&Value)->Result<Value,Failure> {
    if !matches(payload) {return Err(Failure::invalid("Capture identity differs"));}
    let id=comfy::text(payload,"vendorTaskId")?;
    if payload["resumeToken"]["promptId"]!=id {return Err(Failure::invalid("Original capture identity differs"));}
    let source_revision=comfy::text(&payload["resumeToken"],"sourceRevision")?;
    let ctx=comfy::Ctx{config:&payload["configuration"],budget_ms:request.r#budget_ms};
    let mut result=json!({"status":"running","resumeToken":payload["resumeToken"],"outputs":[],
        "data":{"mediaArtifacts":[],"sourceRevision":source_revision,"stickerState":""},"progress":null,"terminalFailure":null,"usage":null});
    match comfy::status(host,&ctx,id,"66")? {
        comfy::Remote::Queued=>result["status"]=json!("queued"),
        comfy::Remote::Running=>{},
        comfy::Remote::Lost=>return Err(Failure{code:"RESOURCE_UNAVAILABLE",message:"Original capture unavailable; retain its id, do not resubmit".into()}),
        comfy::Remote::Failed(code)=>{
            result["status"]=json!("failed");result["terminalFailure"]=json!({"category":"vendorTaskFailed","message":"Studio capture failed","vendorCode":code,"details":{"promptId":id}});
        },
        comfy::Remote::Succeeded(frames)=>{
            let outputs=comfy::history_outputs(host,&ctx,id)?;
            let masks=outputs["68"]["images"].as_array().filter(|a|a.len()==1).ok_or_else(||Failure::invalid("Capture mask missing"))?;
            let mask=&masks[0];
            let pose=outputs["69"]["text"].as_array().filter(|a|a.len()==1).and_then(|a|a[0].as_str()).ok_or_else(||Failure::invalid("Upstream pose handoff missing"))?;
            let pose_json:Value=serde_json::from_str(pose).map_err(|_|Failure::invalid("Invalid upstream pose"))?;
            if pose_json["version"]!=2 || !pose_json["camera"].is_object() {return Err(Failure::invalid("Unsupported upstream pose"));}
            let mut media=Vec::new();let mut identities=Vec::new();let mut dimensions=None;let mut frame_stat=Value::Null;
            for (role,file) in [("frame",frames[0].clone()),("mask",(comfy::text(mask,"filename")?.into(),mask["subfolder"].as_str().unwrap_or("").into(),comfy::text(mask,"type")?.into()))] {
                let (asset,stat)=comfy::download_asset(host,&ctx,&file,role)?;
                if stat["kind"]!="image" {return Err(Failure::invalid("Capture output is not an image"));}
                let size=(stat["width"].as_u64().filter(|n|*n>0),stat["height"].as_u64().filter(|n|*n>0));
                if size.0.is_none() || size.1.is_none() || dimensions.is_some_and(|d|d!=size) {return Err(Failure::invalid("Capture frame and mask geometry differ"));}
                dimensions=Some(size);
                if role=="frame" {frame_stat=stat.clone();}
                identities.push(json!({"mediaId":asset["media"]["id"],"role":role,"kind":"image","projection":"flat"}));media.push(asset);
            }
            media.push(comfy::json_asset(host,"pose",&json!({"kind":"bgx.panorama.frame","version":1,"sourceRevision":source_revision,"sourceContext":serde_json::from_str::<Value>(comfy::text(&payload["resumeToken"],"sourceContextJson")?).map_err(|_|Failure::invalid("Frozen source context missing"))?,"frame":{"width":frame_stat["width"],"height":frame_stat["height"],"sha256":frame_stat["sha256"]},"stickerState":pose_json}))?);
            result["status"]=json!("succeeded");result["outputs"]=json!(media);
            result["data"]=json!({"mediaArtifacts":identities,"sourceRevision":source_revision,"stickerState":pose});
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct CaptureHost { complete:bool, mismatch:bool, writes:Vec<u8> }
    impl Host for CaptureHost {
        fn call(&mut self,call:bgx_adapter_v2_sdk::HostPortCallV2)->Result<Value,bgx_adapter_v2_sdk::host::HostError> {
            let c=serde_json::to_value(call).unwrap();
            match c["method"].as_str().unwrap() {
                "host-media.create"=>{assert_eq!(c["args"],json!({"kind":"file","mimeHint":"application/octet-stream"}));return Ok(json!("pose"));},
                "host-media.write"=>{self.writes=serde_json::from_value(c["args"]["bytes"].clone()).unwrap();return Ok(Value::Null);},
                "host-media.finish"=>return Ok(Value::Null),
                "host-media.info"=>{
                    let h=c["args"]["handle"].as_str().unwrap();let file=h=="pose";
                    return Ok(json!({"id":h,"kind":if file {"file"} else {"image"},"mime":if file {"application/octet-stream"} else {"image/png"},
                        "byteLength":"32","sha256":"a".repeat(64),"width":if file {Value::Null} else {json!(if self.mismatch && h=="mask" {7} else {6})},
                        "height":if file {Value::Null} else {json!(4)},"durationMs":null,"timestampsMs":null,"fileName":if file {"pose.bin"} else {"image.png"}}));
                },
                "host-http.execute"=>{},other=>panic!("Unexpected port {other}")
            }
            let p=&c["args"]["plan"];assert_eq!(p["method"],"GET");let url=p["url"].as_str().unwrap();
            if url.contains("/view?") {return Ok(json!({"status":200,"headers":[],"payload":{"kind":"media","handle":if url.contains("mask.png") {"mask"} else {"frame"}}}));}
            let body=if url.ends_with("/queue") {json!({"queue_running":[[1,"original",{},{}]],"queue_pending":[]})}
                else {assert!(url.ends_with("/history/original"));if !self.complete {json!({})} else {json!({"original":{"status":{"status_str":"success"},"outputs":{
                    "66":{"images":[{"filename":"frame.png","subfolder":"","type":"output"}]},
                    "68":{"images":[{"filename":"mask.png","subfolder":"","type":"output"}]},
                    "69":{"text":[json!({"kind":"pano_sticker_state","version":2,"fps":29.97,"source_aspect":1.5,"camera":{"rotation":[{"frame":121,"value":{"yaw_deg":90}}]}}).to_string()]}
                }}})}};
            Ok(json!({"status":200,"headers":[],"payload":{"kind":"buffered","bytes":serde_json::to_vec(&body).unwrap()}}))
        }
    }
    #[test]
    fn capture_collects_three_owned_outputs_without_resubmitting_and_rejects_changed_mask_size() {
        let payload=json!({"modelId":MODEL,"capabilityKey":CAPABILITY,"operationId":OPERATION,"operationVersion":1,"vendorTaskId":"original",
            "resumeToken":{"promptId":"original","sourceRevision":"frozen-revision","sourceContextJson":"{}"},"configuration":{"endpointBaseUrl":"http://127.0.0.1:8188","credentialRef":null}});
        let request=serde_json::from_value(json!({"protocolVersion":2,"operation":"query","profileId":"binding","capabilitySlot":"generic","requestId":"q","budgetMs":1000,"payloadJson":payload.to_string(),"bindingId":"binding","negotiatedVersion":2,"enabledFeatures":[]})).unwrap();
        let mut host=CaptureHost{complete:false,mismatch:false,writes:Vec::new()};
        let waiting=query(&mut host,&request,&payload).unwrap();assert_eq!(waiting["status"],"running");assert!(host.writes.is_empty());
        serde_json::from_value::<bgx_adapter_v2_sdk::QueryResult>(waiting).unwrap();
        host.complete=true;
        let done=query(&mut host,&request,&payload).unwrap();
        assert_eq!(done["outputs"].as_array().unwrap().len(),3);
        assert_eq!(done["data"]["sourceRevision"],"frozen-revision");
        let pose:Value=serde_json::from_slice(&host.writes).unwrap();
        assert_eq!(pose["stickerState"]["fps"],29.97);assert_eq!(pose["stickerState"]["camera"]["rotation"][0]["frame"],121);
        serde_json::from_value::<bgx_adapter_v2_sdk::QueryResult>(done).unwrap();
        host.mismatch=true;assert!(query(&mut host,&request,&payload).is_err());
    }
    #[test]
    fn capture_uses_one_scene_for_frame_mask_and_exact_pose_lanes() {
        let camera=json!({"projection":"perspective","aspect":1.5,"fov":{"value":65},
            "rotation":{"value":{"yaw_deg":-90,"pitch_deg":12,"roll_deg":3},
            "keyframes":[{"frame":121,"value":{"yaw_deg":180,"pitch_deg":0,"roll_deg":0},"easing":[0,0,1,1]}]}});
        let doc=json!({"stateText":json!({"version":2,"camera":camera,"timeline":{"fps":29.97,"length":122},"stickers":[]}).to_string(),"assets":[]});
        let mut input=json!({"editorDocument":doc.to_string(),"sourceRevision":"original","references":[],"outputProjection":"flat","advanced":{"megapixels":2,"multiple":8}});
        let g=graph(&input,&BTreeMap::new()).unwrap();
        let saved:Value=serde_json::from_str(g["70"]["inputs"]["state_json"].as_str().unwrap()).unwrap();
        assert_eq!(saved["camera"],camera);assert_eq!(saved["timeline"]["fps"],29.97);
        assert_eq!(g["66"]["inputs"]["images"],json!(["70",1]));
        assert_eq!(g["67"]["inputs"]["mask"],json!(["70",2]));
        assert_eq!(g["69"]["inputs"]["source"],json!(["70",4]));
        for node in g.as_object().unwrap().values() {for v in node["inputs"].as_object().unwrap().values() {
            if let Some(link)=v.as_array().filter(|a|a.len()==2 && a[0].is_string() && a[1].is_number()) {assert!(g.get(link[0].as_str().unwrap()).is_some());}
        }}
        input["sourceRevision"]=json!("");assert!(graph(&input,&BTreeMap::new()).is_err());
        input["sourceRevision"]=json!("original");input["outputProjection"]=json!("equirectangular360");assert!(graph(&input,&BTreeMap::new()).is_err());
    }
}
