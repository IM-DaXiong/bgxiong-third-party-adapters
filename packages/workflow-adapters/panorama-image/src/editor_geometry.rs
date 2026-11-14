//! Geometry preparation returns upstream preview files as owned artifacts. A completed
//! remote task is only queried; collection never launches another inference.
extern crate alloc;
use alloc::{collections::BTreeMap, format, string::{String,ToString}, vec::Vec};
use bgx_adapter_v2_sdk::{Host,Invocation};
use serde_json::{json,Value};
use crate::{comfy,geometry,mapping::Failure,scene::SceneDocument};
pub const OPERATION:&str="local.thirdparty.comfy-panorama-image/editor-geometry";
pub const MODEL:&str="panorama-editor-geometry";
pub const CAPABILITY:&str="local.thirdparty.comfy-panorama-image/editor-geometry";
pub const MODELS_OPERATION:&str="local.thirdparty.comfy-panorama-image/editor-geometry-models";

fn matches(p:&Value)->bool {p["operationId"]==OPERATION&&p["operationVersion"]==1&&p["modelId"]==MODEL&&p["capabilityKey"]==CAPABILITY}
pub fn contract(base:&Value)->Value {
    let mut c=crate::editor_prepare::contract(base);
    // Use the same frozen document contract as rendering, not the capture's Frame restriction.
    let mut input=crate::workflows::schema("studio-render");
    input["properties"].as_object_mut().unwrap().remove("prompt");
    input["required"].as_array_mut().unwrap().retain(|v|v!="prompt");
    input["properties"]["geometrySourceJson"]=json!({"type":"string","minLength":2});
    input["properties"]["sourceRevision"]=json!({"type":"string","minLength":1,"readOnly":true});
    input["required"].as_array_mut().unwrap().extend([json!("geometrySourceJson"),json!("sourceRevision")]);
    let input=crate::capability::schema("geometry-input",&input.to_string());
    let output=crate::capability::schema("geometry-output",&json!({"type":"object","additionalProperties":false,"properties":{"metadataJson":{"type":"string"}},"required":["metadataJson"]}).to_string());
    let resume=crate::capability::schema("geometry-resume",&json!({"type":"object","additionalProperties":false,
        "properties":{"promptId":{"type":"string","minLength":1},"sourceRevision":{"type":"string","minLength":1},"geometrySourceJson":{"type":"string","minLength":2},"sourceAssetsJson":{"type":"string","minLength":2}},
        "required":["promptId","sourceRevision","geometrySourceJson","sourceAssetsJson"]}).to_string());
    c["localId"]=json!(MODEL);c["modelIds"]=json!([MODEL]);c["capabilityKey"]=json!(CAPABILITY);
    let op=&mut c["operations"][0];op["id"]=json!(OPERATION);op["label"]=json!("深度几何准备");
    op["inputSchema"]=json!({"id":"geometry-input","digest":input["digest"]});op["outputSchema"]=json!({"id":"geometry-output","digest":output["digest"]});op["resumeSchema"]=json!({"id":"geometry-resume","digest":resume["digest"]});
    op["outputSlots"]=json!([
        {"role":"geometryFile","kind":"file","minCount":1,"maxCount":1024},
        {"role":"geometryColor","kind":"video","minCount":0,"maxCount":1},
        {"role":"geometryImage","kind":"image","minCount":0,"maxCount":1024}]);
    let models_input=crate::capability::schema("geometry-models-input",r#"{"type":"object","additionalProperties":false,"properties":{"sources":{"type":"array","maxItems":0,"items":{"type":"object","additionalProperties":false,"properties":{"mediaId":{"type":"string"}},"required":["mediaId"]}}},"required":["sources"]}"#);
    let mut discover=op.clone();discover["id"]=json!(MODELS_OPERATION);discover["label"]=json!("读取几何模型");
    discover["inputSchema"]=json!({"id":"geometry-models-input","digest":models_input["digest"]});
    discover["resumeSchema"]=Value::Null;discover["mediaSlots"]=json!([{"role":"sources","inputPointer":"/sources","mediaKind":"file","minCount":0,"maxCount":1}]);discover["outputSlots"]=json!([]);
    discover["execution"]=json!({"mode":"synchronous","streaming":false,"cancellation":"unsupported","resumable":false,"remoteIdempotency":false});
    c["operations"].as_array_mut().unwrap().push(discover);
    c["schemas"]=json!([input,output,resume,models_input,crate::capability::schema("config",crate::capability::CONFIG_SCHEMA)]);c
}

pub fn models(host:&mut dyn Host,request:&Invocation,payload:&Value)->Result<Value,Failure> {
    if payload["operationId"]!=MODELS_OPERATION || payload["modelId"]!=MODEL || payload["capabilityKey"]!=CAPABILITY || payload["operationVersion"]!=1 {return Err(Failure::invalid("Geometry discovery identity differs"));}
    let ctx=comfy::Ctx{config:&payload["configuration"],budget_ms:request.r#budget_ms};let mut models=json!({});
    for (kind,node) in [("da3","LoadDA3Model"),("moge","LoadMoGeModel")] {
        let schema=comfy::exchange(host,&ctx,"GET",&format!("/object_info/{node}"),json!({"kind":"none"}),alloc::vec![],false)?;
        let names=schema[node]["input"]["required"]["model_name"][0].as_array().ok_or_else(||Failure::invalid(format!("Resource machine does not expose {node} model choices")))?;
        if names.iter().any(|v|v.as_str().is_none_or(|s|s.is_empty()||s.contains(['\\',':'])||s.split('/').any(|v|matches!(v,"."|"..")))) {return Err(Failure::invalid("Invalid upstream model name"));}
        models[kind]=json!(names);
    }
    Ok(json!({"kind":"completed","vendorTaskId":null,"outputs":[],"data":{"metadataJson":models.to_string()},"appliedParams":null,"usage":null}))
}

fn document(input:&Value)->Result<SceneDocument,Failure> {
    let mut doc=SceneDocument::parse(comfy::text(input,"editorDocument")?)?;
    let source:Value=serde_json::from_str(comfy::text(input,"geometrySourceJson")?).map_err(|_|Failure::invalid("Invalid geometry source"))?;
    let record=json!({"version":1,"source":source});geometry::validate(&record)?;
    doc.state[geometry::STATE_KEY]=record;
    doc.validate_references(input)?;
    Ok(doc)
}
pub fn graph(input:&Value,uploads:&BTreeMap<String,String>)->Result<Value,Failure> {
    let doc=document(input)?;
    let mp=input["advanced"]["megapixels"].as_f64().ok_or_else(||Failure::invalid("Missing preview size"))?;
    let multiple=input["advanced"]["multiple"].as_u64().ok_or_else(||Failure::invalid("Missing preview multiple"))?;
    let bound=doc.bind(uploads)?;
    let preview=json!({"version":2,"camera":bound["camera"],"timeline":bound["timeline"]});
    let mut graph=crate::scene::render_graph(&preview,mp,multiple)?;
    // Preview is its own source, not a re-inference of every connected parent.
    geometry::attach(&mut graph,"70",&doc.bind(uploads)?)?;
    Ok(graph)
}
pub fn submit(host:&mut dyn Host,request:&Invocation,payload:&Value)->Result<Value,Failure> {
    if !matches(payload){return Err(Failure::invalid("Geometry service identity differs"));}
    let input=&payload["input"];let doc=document(input)?;
    geometry::validate_media(host,payload,&doc)?;
    let mut uploads=BTreeMap::new();let mut media=BTreeMap::new();let mut sources=Vec::new();
    // Only root geometry source files are uploaded. The entire saved document remains frozen.
    for file in geometry::source_files(&doc.state[geometry::STATE_KEY]["source"])? {
        let name=format!("{}/{}/{}",file["storage"].as_str().unwrap(),file["subfolder"].as_str().unwrap(),file["filename"].as_str().unwrap());
        let id=doc.assets.get(&name).ok_or_else(||Failure::invalid("Geometry source missing"))?;
        let lease=crate::scene_media(payload,id,&name)?;
        let stat=comfy::port(host,"host-media.info",json!({"handle":lease["handle"]}))?;
        sources.push(json!({"name":name,"sha256":stat["sha256"]}));
        media.insert(id.clone(),lease);
    }
    // Binding validates all references in the scene, including existing stickers; provide
    // placeholders for unrelated assets then keep those nodes out of this preparation graph.
    for id in doc.assets.values(){uploads.insert(id.clone(),"pending-upload.png".into());}
    graph(input,&uploads)?;
    let ctx=comfy::Ctx{config:&payload["configuration"],budget_ms:request.r#budget_ms};
    for (id,lease) in media {uploads.insert(id,comfy::upload(host,&ctx,&lease)?);}
    let id=comfy::submit(host,&ctx,&graph(input,&uploads)?,&request.r#request_id)?;
    let accepted=json!({"kind":"accepted","vendorTaskId":id,"resumeToken":{"promptId":id,"sourceRevision":input["sourceRevision"],
        "geometrySourceJson":input["geometrySourceJson"],"sourceAssetsJson":json!(sources).to_string()},"appliedParams":input,"usage":null});
    crate::persist_accepted(host,request,&accepted)?;Ok(accepted)
}

fn collect_files(host:&mut dyn Host,ctx:&comfy::Ctx,value:&mut Value,artifacts:&mut Vec<Value>,files:&mut BTreeMap<String,usize>)->Result<(),Failure> {
    if value.is_object() && value.get("filename").is_some() {
        let name=comfy::text(value,"filename")?.to_string();
        let folder=value["subfolder"].as_str().unwrap_or("").to_string();
        let storage=value["type"].as_str().or_else(||value["storage"].as_str()).ok_or_else(||Failure::invalid("Geometry preview storage missing"))?.to_string();
        if name.contains(['/', '\\', ':']) || name.is_empty() || !matches!(storage.as_str(),"input"|"output"|"temp")
            || folder.contains(['\\',':']) || folder.split('/').any(|part|matches!(part,"."|"..")) {
            return Err(Failure::invalid("Unsafe upstream preview descriptor"));
        }
        let key=json!([name,folder,storage]).to_string();
        let index=if let Some(index)=files.get(&key){*index}else{
            if artifacts.len()>=1024 {return Err(Failure::invalid("Geometry preview exceeds the artifact contract; no frames were silently discarded"));}
            let (mut artifact,stat)=comfy::download_asset(host,ctx,&(name.clone(),folder,storage),"geometryFile")?;
            artifact["role"]=json!(match stat["kind"].as_str(){Some("file")=>"geometryFile",Some("image")=>"geometryImage",Some("video")=>"geometryColor",_=>return Err(Failure::invalid("Unsupported geometry preview media kind"))});
            let index=artifacts.len();artifacts.push(artifact);files.insert(key,index);index
        };
        let format=value["format"].clone();
        *value=json!({"assetIndex":index,"format":format});
    }else{match value{
        Value::Array(values)=>for item in values{collect_files(host,ctx,item,artifacts,files)?;},
        Value::Object(values)=>for item in values.values_mut(){collect_files(host,ctx,item,artifacts,files)?;},
        _=>{}
    }}
    Ok(())
}
pub fn query(host:&mut dyn Host,request:&Invocation,payload:&Value)->Result<Value,Failure> {
    if !matches(payload){return Err(Failure::invalid("Geometry service identity differs"));}
    let id=comfy::text(payload,"vendorTaskId")?;let resume=&payload["resumeToken"];
    if resume["promptId"]!=id {return Err(Failure::invalid("Original geometry task identity differs"));}
    let ctx=comfy::Ctx{config:&payload["configuration"],budget_ms:request.r#budget_ms};
    let mut result=json!({"status":"running","resumeToken":resume,"outputs":[],"data":{"metadataJson":""},"progress":null,"terminalFailure":null,"usage":null});
    match comfy::status(host,&ctx,id,"66")? {
        comfy::Remote::Queued=>result["status"]=json!("queued"),comfy::Remote::Running=>{},
        comfy::Remote::Lost=>return Err(Failure{code:"RESOURCE_UNAVAILABLE",message:"Original geometry result unavailable; retain its id, do not resubmit".into()}),
        comfy::Remote::Failed(code)=>{result["status"]=json!("failed");result["terminalFailure"]=json!({"category":"vendorTaskFailed","message":"Geometry preparation failed","vendorCode":code,"details":{"promptId":id}});},
        comfy::Remote::Succeeded(_)=>{
            let outputs=comfy::history_outputs(host,&ctx,id)?;
            let mut ui=json!({"pano_scene_files":outputs["70"]["pano_scene_files"],"pano_scene_meta":outputs["70"]["pano_scene_meta"]});
            if ui["pano_scene_files"].as_array().is_none_or(Vec::is_empty) || ui["pano_scene_meta"].as_array().is_none_or(|v|v.len()!=1)
                || !matches!(ui["pano_scene_meta"][0]["kind"].as_str(),Some("mesh"|"splat"|"depth_grid")) {return Err(Failure::invalid("Upstream geometry preview is missing"));}
            let mut artifacts=Vec::new();collect_files(host,&ctx,&mut ui,&mut artifacts,&mut BTreeMap::new())?;
            if artifacts.is_empty(){return Err(Failure::invalid("Upstream geometry preview has no owned files"));}
            let source:Value=serde_json::from_str(comfy::text(resume,"geometrySourceJson")?).map_err(|_|Failure::invalid("Frozen geometry source is invalid"))?;
            let assets:Value=serde_json::from_str(comfy::text(resume,"sourceAssetsJson")?).map_err(|_|Failure::invalid("Frozen geometry source identities are invalid"))?;
            result["status"]=json!("succeeded");result["outputs"]=json!(artifacts);
            result["data"]["metadataJson"]=json!(json!({"version":1,"source":source,"sourceAssets":assets,"ui":ui}).to_string());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct PreviewHost {gets:Vec<String>, invalid:bool}
    impl Host for PreviewHost {
        fn call(&mut self,call:bgx_adapter_v2_sdk::HostPortCallV2)->Result<Value,bgx_adapter_v2_sdk::host::HostError> {
            let call=serde_json::to_value(call).unwrap();
            if call["method"]=="host-media.info" {
                let handle=call["args"]["handle"].as_str().unwrap();let video=handle.contains("color.mp4");
                return Ok(json!({"id":handle,"kind":if video{"video"}else{"file"},"mime":if video{"video/mp4"}else{"application/octet-stream"},
                    "byteLength":"16","sha256":"a".repeat(64),"fileName":if video{"color.mp4"}else{"geometry.bin"},
                    "width":if video{json!(2)}else{Value::Null},"height":if video{json!(2)}else{Value::Null},"durationMs":if video{json!("100")}else{Value::Null},"timestampsMs":null}));
            }
            assert_eq!(call["method"],"host-http.execute");let plan=&call["args"]["plan"];assert_eq!(plan["method"],"GET");
            let url=plan["url"].as_str().unwrap();self.gets.push(url.into());
            if url.contains("/view?") {return Ok(json!({"status":200,"headers":[],"payload":{"kind":"media","handle":url}}));}
            let file=|name:&str|json!({"filename":name,"subfolder":"panorama","type":"temp","format":"application/octet-stream"});
            let body=if url.ends_with("/queue"){json!({"queue_running":[],"queue_pending":[]})}else{
                assert!(url.ends_with("/history/original"));
                json!({"original":{"status":{"status_str":"success"},"outputs":{
                    "66":{"images":[file("render.png")]},
                    "70":{"pano_scene_files":[[file(if self.invalid{"../unsafe.bin"}else{"first.bin"}),file("second.bin")],null,[file("first.bin")]],
                        "pano_scene_meta":[{"kind":"depth_grid","width":2,"height":2,"depth":{"dtype":"float32","byte_order":"little","bytes":16},
                            "intrinsics":[[1,0,0,0,1,0,0,0,1]],"video":file("color.mp4"),"video_fps":29.97,"frames":3,
                            "animation":{"source":file("clip.glb"),"clips":[{"name":"Walk","duration":2.5}],"box":{"min":[-1,-2,-3],"max":[1,2,3]}}}]}
                }}})
            };
            Ok(json!({"status":200,"headers":[],"payload":{"kind":"buffered","bytes":serde_json::to_vec(&body).unwrap()}}))
        }
    }
    #[test]
    fn collects_nested_geometry_depth_color_and_animation_without_resubmit_or_duplicate_files() {
        let payload=json!({"operationId":OPERATION,"operationVersion":1,"modelId":MODEL,"capabilityKey":CAPABILITY,"vendorTaskId":"original",
            "resumeToken":{"promptId":"original","sourceRevision":"revision","geometrySourceJson":"{}","sourceAssetsJson":"[]"},
            "configuration":{"endpointBaseUrl":"http://127.0.0.1:8188","credentialRef":null}});
        let request=serde_json::from_value(json!({"protocolVersion":2,"operation":"query","profileId":"binding","capabilitySlot":"generic","requestId":"query","budgetMs":1000,
            "payloadJson":payload.to_string(),"bindingId":"binding","negotiatedVersion":2,"enabledFeatures":[]})).unwrap();
        let mut host=PreviewHost{gets:Vec::new(),invalid:false};
        let result=query(&mut host,&request,&payload).unwrap();
        serde_json::from_value::<bgx_adapter_v2_sdk::QueryResult>(result.clone()).unwrap();
        assert_eq!(result["status"],"succeeded");assert_eq!(result["outputs"].as_array().unwrap().len(),4);
        let metadata:Value=serde_json::from_str(result["data"]["metadataJson"].as_str().unwrap()).unwrap();
        assert_eq!(metadata["ui"]["pano_scene_files"][0][0],metadata["ui"]["pano_scene_files"][2][0]);
        assert!(metadata["ui"]["pano_scene_files"][1].is_null());
        assert_eq!(metadata["ui"]["pano_scene_meta"][0]["video_fps"],29.97);
        assert_eq!(metadata["ui"]["pano_scene_meta"][0]["animation"]["clips"][0]["duration"],2.5);
        assert_eq!(host.gets.iter().filter(|v|v.contains("/view?")).count(),4);
        let mut bad=PreviewHost{gets:Vec::new(),invalid:true};assert!(query(&mut bad,&request,&payload).is_err());
        assert!(!bad.gets.iter().any(|v|v.contains("/view?")));
    }
}
