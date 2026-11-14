//! Adapter-owned geometry binding. Native inference and geometry math stay upstream.
//! `bgxGeometry` is removed before the upstream state serializer sees the document.
extern crate alloc;
use alloc::{format, string::ToString};
use serde_json::{json, Value};
use crate::mapping::Failure;

pub const STATE_KEY: &str = "bgxGeometry";

fn fields(value: &Value, names: &[&str]) -> Result<(), Failure> {
    let object=value.as_object().ok_or_else(||Failure::invalid("Geometry record must be an object"))?;
    if object.keys().any(|k| !names.contains(&k.as_str())) {return Err(Failure::invalid("Unknown geometry field; preserve the original document"));}
    Ok(())
}
fn choice<'a>(value:&'a Value, choices:&[&str])->Result<&'a str,Failure> {
    value.as_str().filter(|v|choices.contains(v)).ok_or_else(||Failure::invalid("Unsupported geometry option"))
}
fn number(value:&Value,min:f64,max:f64)->Result<f64,Failure> {
    value.as_f64().filter(|v|v.is_finite()&&*v>=min&&*v<=max).ok_or_else(||Failure::invalid("Geometry number outside upstream range"))
}
fn integer(value:&Value,min:u64,max:u64)->Result<u64,Failure> {
    value.as_u64().filter(|v|*v>=min&&*v<=max).ok_or_else(||Failure::invalid("Geometry integer outside upstream range"))
}
fn relative(text:&str)->bool {
    !text.is_empty() && !text.contains(['\\',':']) && !text.split('/').any(|v|v.is_empty()||matches!(v,"."|".."))
}

/// Source file references are bound by SceneDocument to this request's owned uploads.
pub fn validate(record:&Value)->Result<(),Failure> {
    fields(record,&["version","source","preview"])?;
    if record["version"]!=1 {return Err(Failure::invalid("Unsupported geometry version; preserve the original document"));}
    let source=&record["source"];
    fields(source,&["algorithm","images","video","options"])?;
    let algorithm=choice(&source["algorithm"],&["da3","moge","mogePanorama"])?;
    let video=source.get("video").is_some();
    if video == source.get("images").is_some() {return Err(Failure::invalid("Choose either owned images or one owned video"));}
    let images=source_files(source)?;
    for image in &images {
        fields(image,&["kind","filename","subfolder","storage"])?;
        if image["kind"]!="file" || image["filename"].as_str().is_none_or(|v|!relative(v)||v.contains('/'))
            || image["subfolder"].as_str().is_none_or(|v|!v.is_empty()&&!relative(v))
            || !matches!(image["storage"].as_str(),Some("input"|"output"|"temp"|"editor-results")) {
            return Err(Failure::invalid("Geometry source must be a named document asset"));
        }
    }
    let options=&source["options"];
    let model=options["modelName"].as_str().filter(|v|relative(v)).ok_or_else(||Failure::invalid("Choose a geometry model from the resource machine"))?;
    let _=model;
    if algorithm=="da3" {
        fields(options,&["modelName","weightDtype","resolution","resizeMethod","mode","refViewStrategy","poseMethod"])?;
        choice(&options["weightDtype"],&["default","fp16","bf16","fp32"])?;
        if integer(&options["resolution"],140,2520)?%14!=0 {return Err(Failure::invalid("DA3 resolution must be a multiple of 14"));}
        choice(&options["resizeMethod"],&["upper_bound_resize","lower_bound_resize"])?;
        let mode=choice(&options["mode"],&["mono","multiview"])?;
        if mode=="multiview" {
            if !video && images.len()<2 {return Err(Failure::invalid("DA3 multiview requires at least two images of the same scene"));}
            choice(&options["refViewStrategy"],&["saddle_balanced","saddle_sim_range","first","middle"])?;
            choice(&options["poseMethod"],&["cam_dec","ray_pose"])?;
        } else if options.get("refViewStrategy").is_some() || options.get("poseMethod").is_some() {
            return Err(Failure::invalid("Multiview parameters are not used by DA3 mono"));
        }
    } else if algorithm=="moge" {
        fields(options,&["modelName","resolutionLevel","fovXDegrees","batchSize","forceProjection","applyMask","refineSteps"])?;
        integer(&options["resolutionLevel"],0,9)?;
        number(&options["fovXDegrees"],0.0,170.0)?;
        integer(&options["batchSize"],1,64)?;
        integer(&options["refineSteps"],0,8)?;
        if !options["forceProjection"].is_boolean() || !options["applyMask"].is_boolean() {return Err(Failure::invalid("MoGe flags must be explicit booleans"));}
    } else {
        fields(options,&["modelName","resolutionLevel","splitResolution","mergeResolution","batchSize","refineSteps"])?;
        if images.len()!=1 {return Err(Failure::invalid("MoGe panorama requires exactly one equirectangular image"));}
        integer(&options["resolutionLevel"],0,9)?;
        integer(&options["splitResolution"],256,1024)?;
        integer(&options["mergeResolution"],256,8192)?;
        integer(&options["batchSize"],1,12)?;
        integer(&options["refineSteps"],0,8)?;
    }
    if let Some(preview)=record.get("preview") {
        fields(preview,&["sourceDigest","ui"])?;
        if preview["sourceDigest"].as_str().is_none_or(|v|v.len()!=64||!v.bytes().all(|b|b.is_ascii_hexdigit())) || !preview["ui"].is_object() {
            return Err(Failure::invalid("Invalid owned geometry preview"));
        }
    }
    Ok(())
}

pub fn source_files(source:&Value)->Result<alloc::vec::Vec<&Value>,Failure> {
    if let Some(video)=source.get("video") {return Ok(alloc::vec![video]);}
    source["images"].as_array().filter(|v|!v.is_empty()&&v.len()<=1024)
        .map(|v|v.iter().collect()).ok_or_else(||Failure::invalid("Choose owned geometry sources"))
}

/// Compare decoded frame facts with the saved scene before sending any files.
/// No rate conversion, implicit trim, or repeated frames are introduced here.
pub fn validate_video(state:&Value,stat:&Value)->Result<(),Failure> {
    let fps=state["timeline"]["fps"].as_f64().filter(|v|v.is_finite()&&*v>0.0).ok_or_else(||Failure::invalid("请先设置视频工程帧率"))?;
    let frames=state["timeline"]["length"].as_u64().filter(|v|*v>0).ok_or_else(||Failure::invalid("请先设置视频工程长度"))?;
    let timestamps=stat["timestampsMs"].as_array().ok_or_else(||Failure::invalid("缺少视频实际帧时间，请启用工程素材帧探测"))?;
    if !stat["mime"].as_str().is_some_and(|v|v.starts_with("video/")) || timestamps.len() as u64!=frames {
        return Err(Failure::invalid("几何来源实际视频帧数与工程长度不一致；不会自动裁切或补帧"));
    }
    if state[STATE_KEY]["source"]["options"]["mode"]=="multiview" && frames<2 {return Err(Failure::invalid("多视角几何需要至少两帧"));}
    let origin=timestamps.first().and_then(Value::as_str).and_then(|v|v.parse::<f64>().ok()).ok_or_else(||Failure::invalid("视频帧时间无效"))?;
    for (index,value) in timestamps.iter().enumerate() {
        let time=value.as_str().and_then(|v|v.parse::<f64>().ok()).filter(|v|v.is_finite()).ok_or_else(||Failure::invalid("视频帧时间无效"))?;
        if (time-origin-index as f64*1000.0/fps).abs()>1.1 {return Err(Failure::invalid("几何来源帧率与工程不一致或为可变帧率；请显式选择转换"));}
    }
    Ok(())
}

/// Attach uniquely named dependencies to a specific Studio node. Parent and child
/// documents can each own a geometry source without overwriting each other's nodes.
pub fn attach(graph:&mut Value,studio:&str,state:&Value)->Result<(),Failure> {
    let Some(record)=state.get(STATE_KEY) else {return Ok(());};
    validate(record)?;
    if graph[studio]["inputs"].get("world").is_some() {return Err(Failure::invalid("This workflow already owns the world input; choose the saved geometry operation explicitly"));}
    let prefix=format!("{studio}_bgx_geometry");
    let source=&record["source"];
    let images=source_files(source)?;
    let mut output=Value::Null;
    for (index,image) in images.iter().enumerate() {
        if image["storage"]!="input" {return Err(Failure::invalid("Geometry source must be bound to a current upload"));}
        let id=format!("{prefix}_image_{index}");
        let folder=image["subfolder"].as_str().unwrap();
        let file=image["filename"].as_str().unwrap();
        let path=if folder.is_empty(){file.to_string()}else{format!("{folder}/{file}")};
        if source.get("video").is_some() {
            // Same upstream VHS loader as the locked H3 example, with full source frames.
            graph[&id]=json!({"class_type":"VHS_LoadVideo","inputs":{"video":path,"force_rate":0,"custom_width":0,"custom_height":0,"frame_load_cap":0,"skip_first_frames":0,"select_every_nth":1,"format":"None"}});
        }else{graph[&id]=json!({"class_type":"LoadImage","inputs":{"image":path}});}
        let current=json!([id,0]);
        if index==0 {output=current;} else {
            let batch=format!("{prefix}_batch_{index}");
            graph[&batch]=json!({"class_type":"ImageBatch","inputs":{"image1":output,"image2":current}});
            output=json!([batch,0]);
        }
    }
    let options=&source["options"];
    let loader=format!("{prefix}_model");let inference=format!("{prefix}_infer");
    if source["algorithm"]=="da3" {
        graph[&loader]=json!({"class_type":"LoadDA3Model","inputs":{"model_name":options["modelName"],"weight_dtype":options["weightDtype"]}});
        let mut inputs=json!({"da3_model":[loader,0],"image":output,"resolution":options["resolution"],"resize_method":options["resizeMethod"],"mode":options["mode"]});
        if options["mode"]=="multiview" {
            inputs["mode.ref_view_strategy"]=options["refViewStrategy"].clone();
            inputs["mode.pose_method"]=options["poseMethod"].clone();
        }
        graph[&inference]=json!({"class_type":"DA3Inference","inputs":inputs});
    } else if source["algorithm"]=="moge" {
        graph[&loader]=json!({"class_type":"LoadMoGeModel","inputs":{"model_name":options["modelName"]}});
        graph[&inference]=json!({"class_type":"MoGeInference","inputs":{"moge_model":[loader,0],"image":output,
            "resolution_level":options["resolutionLevel"],"fov_x_degrees":options["fovXDegrees"],"batch_size":options["batchSize"],
            "force_projection":options["forceProjection"],"apply_mask":options["applyMask"],"refine_steps":options["refineSteps"]}});
    } else {
        graph[&loader]=json!({"class_type":"LoadMoGeModel","inputs":{"model_name":options["modelName"]}});
        graph[&inference]=json!({"class_type":"MoGePanoramaInference","inputs":{"moge_model":[loader,0],"image":output,
            "resolution_level":options["resolutionLevel"],"split_resolution":options["splitResolution"],"merge_resolution":options["mergeResolution"],
            "batch_size":options["batchSize"],"refine_steps":options["refineSteps"]}});
    }
    graph[studio]["inputs"]["world"]=json!([inference,0]);
    Ok(())
}

pub fn upstream_state(state:&Value)->Value {
    let mut native=state.clone();
    if let Some(object)=native.as_object_mut(){object.remove(STATE_KEY);}
    native
}

/// ImageBatch would resize its second input when shapes differ. Refuse that before
/// uploading anything; only the user's explicit preprocessing may alter source pixels.
pub fn validate_media(host:&mut dyn bgx_adapter_v2_sdk::Host,payload:&Value,document:&crate::scene::SceneDocument)->Result<(),Failure> {
    if let Some(record)=document.state.get(STATE_KEY) {
        validate(record)?;
        let mut dimensions=None;
        for file in source_files(&record["source"])? {
            let name=format!("{}/{}/{}",file["storage"].as_str().unwrap(),file["subfolder"].as_str().unwrap(),file["filename"].as_str().unwrap());
            let id=document.assets.get(&name).ok_or_else(||Failure::invalid("Geometry source is not an owned document asset"))?;
            let lease=crate::scene_media(payload,id,&name)?;
            let stat=crate::comfy::port(host,"host-media.info",json!({"handle":lease["handle"]}))?;
            let size=(stat["width"].as_u64().filter(|v|*v>0),stat["height"].as_u64().filter(|v|*v>0));
            let video=record["source"].get("video").is_some();
            if video {validate_video(&document.state,&stat)?;}
            if (!video && !stat["mime"].as_str().is_some_and(|v|v.starts_with("image/"))) || size.0.is_none() || size.1.is_none() {
                return Err(Failure::invalid("Geometry sources must have measured dimensions"));
            }
            if dimensions.is_some_and(|expected|expected!=size) {return Err(Failure::invalid("几何输入图片尺寸必须一致；请先明确选择尺寸处理，不会自动缩放原图"));}
            if record["source"]["algorithm"]=="mogePanorama" && size.0!=size.1.and_then(|v|v.checked_mul(2)) {
                return Err(Failure::invalid("MoGe全景输入必须是2:1等距柱状图"));
            }
            dimensions=Some(size);
        }
    }
    for source in &document.sources {validate_media(host,payload,&source.document)?;}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_video_keeps_every_frame_and_rejects_scene_timing_mismatch() {
        let mut record=record();let source=record["source"].as_object_mut().unwrap();
        source.remove("images");source.insert("video".into(),json!({"kind":"file","filename":"walk.mp4","storage":"input","subfolder":"owned"}));
        validate(&record).unwrap();
        let state=json!({"version":2,"timeline":{"fps":29.97,"length":73},"bgxGeometry":record});
        let mut graph=json!({"root":{"class_type":"PanoramaStudio","inputs":{}}});attach(&mut graph,"root",&state).unwrap();
        assert_eq!(graph["root_bgx_geometry_image_0"],json!({"class_type":"VHS_LoadVideo","inputs":{"video":"owned/walk.mp4","force_rate":0,"custom_width":0,"custom_height":0,"frame_load_cap":0,"skip_first_frames":0,"select_every_nth":1,"format":"None"}}));
        assert_eq!(graph["root_bgx_geometry_infer"]["inputs"]["image"],json!(["root_bgx_geometry_image_0",0]));
        let mut facts=json!({"mime":"video/mp4","timestampsMs":(0..73).map(|i|((i as f64*1000.0/29.97).round() as u64+1234).to_string()).collect::<alloc::vec::Vec<_>>()});
        validate_video(&state,&facts).unwrap();
        facts["timestampsMs"][20]=json!("2000");assert!(validate_video(&state,&facts).is_err());
        facts["timestampsMs"]=json!([]);assert!(validate_video(&state,&facts).is_err());
        let mut invalid=state[STATE_KEY].clone();invalid["source"]["images"]=json!([]);assert!(validate(&invalid).is_err());
        invalid=state[STATE_KEY].clone();invalid["source"]["video"]["filename"]=json!("../walk.mp4");assert!(validate(&invalid).is_err());
    }
    pub fn record()->Value {json!({"version":1,"source":{"algorithm":"da3","images":[
        {"kind":"file","filename":"front.png","subfolder":"room","storage":"input"},
        {"kind":"file","filename":"left.png","subfolder":"room","storage":"input"}],
        "options":{"modelName":"DA3-BASE.safetensors","weightDtype":"default","resolution":504,"resizeMethod":"upper_bound_resize","mode":"multiview","refViewStrategy":"first","poseMethod":"cam_dec"}}})}
    #[test]
    fn geometry_uses_native_multiview_and_rejects_unknown_or_unbound_inputs() {
        let state=json!({"version":2,"bgxGeometry":record(),"camera":{"projection":"erp","erp_range":360}});
        let mut graph=json!({"root":{"class_type":"PanoramaStudio","inputs":{}}});
        attach(&mut graph,"root",&state).unwrap();
        assert_eq!(graph["root_bgx_geometry_infer"]["inputs"]["mode.ref_view_strategy"],"first");
        assert_eq!(graph["root_bgx_geometry_infer"]["inputs"]["image"],json!(["root_bgx_geometry_batch_1",0]));
        assert_eq!(graph["root_bgx_geometry_image_1"]["inputs"]["image"],"room/left.png");
        assert!(upstream_state(&state).get(STATE_KEY).is_none());
        assert_eq!(upstream_state(&state)["camera"],state["camera"]);
        assert!(attach(&mut graph,"root",&state).is_err());
        for (pointer,value) in [("/version",json!(2)),("/source/options/resolution",json!(505)),("/source/images/0/filename",json!("../outside.png")),("/source/options/mode",json!("mono"))] {
            let mut invalid=record();*invalid.pointer_mut(pointer).unwrap()=value;assert!(validate(&invalid).is_err());
        }
        let mut invalid=record();invalid["future"]=json!(true);assert!(validate(&invalid).is_err());
    }

    #[test]
    fn parent_and_child_geometry_keep_separate_dependencies_and_original_assets() {
        use alloc::collections::BTreeMap;
        let state=json!({"version":2,"bgxGeometry":record(),"camera":{"projection":"erp","erp_range":360}});
        let assets=json!([{"name":"input/room/front.png","mediaId":"front"},{"name":"input/room/left.png","mediaId":"left"}]);
        let parent=json!({"stateText":state.to_string(),"assets":assets});
        let child=json!({"stateText":state.to_string(),"assets":[{"name":"input/room/front.png","mediaId":"childFront"},{"name":"input/room/left.png","mediaId":"childLeft"}],
            "inputNodeIds":["parent"],"connectedDocuments":[{"nodeId":"parent","inputNodeIds":[],"document":parent}]});
        let document=crate::scene::SceneDocument::parse(&child.to_string()).unwrap();
        let uploads=BTreeMap::from([("front".into(),"owned/a.png".into()),("left".into(),"owned/b.png".into()),("childFront".into(),"owned/c.png".into()),("childLeft".into(),"owned/d.png".into())]);
        let mut graph=crate::scene::render_graph(&document.bind(&uploads).unwrap(),2.0,8).unwrap();
        document.connect_parents(&mut graph,"70",&uploads,2.0,8).unwrap();
        assert_eq!(graph["70"]["inputs"]["scene"],json!(["bgx_editor_input_0",0]));
        assert_eq!(graph["bgx_editor_input_0_bgx_geometry_image_0"]["inputs"]["image"],"owned/a.png");
        assert_eq!(graph["70_bgx_geometry_image_0"]["inputs"]["image"],"owned/c.png");
        for node in graph.as_object().unwrap().values() {
            for value in node["inputs"].as_object().unwrap().values() {
                if let Some(link)=value.as_array().filter(|a|a.len()==2&&a[0].is_string()&&a[1].is_number()) {assert!(graph.get(link[0].as_str().unwrap()).is_some());}
            }
        }
        assert_eq!(document.state,state);
        let native:Value=serde_json::from_str(graph["70"]["inputs"]["state_json"].as_str().unwrap()).unwrap();
        assert!(native.get(STATE_KEY).is_none());
    }

    #[test]
    fn mismatched_images_are_rejected_before_network_without_resizing() {
        struct MediaHost { calls:usize, same_size:bool }
        impl bgx_adapter_v2_sdk::Host for MediaHost {
            fn call(&mut self,call:bgx_adapter_v2_sdk::HostPortCallV2)->Result<Value,bgx_adapter_v2_sdk::host::HostError> {
                let call=serde_json::to_value(call).unwrap();assert_eq!(call["method"],"host-media.info");
                self.calls+=1;
                Ok(json!({"id":"source","kind":"file","mime":"image/png","fileName":"source.png","byteLength":"10","sha256":"a".repeat(64),
                    "width":if self.same_size||self.calls%2==1 {100}else{200},"height":100,"durationMs":null,"timestampsMs":null}))
            }
        }
        let document=crate::scene::SceneDocument::parse(&json!({"stateText":json!({"version":2,"bgxGeometry":record()}).to_string(),
            "assets":[{"name":"input/room/front.png","mediaId":"front"},{"name":"input/room/left.png","mediaId":"left"}]}).to_string()).unwrap();
        let payload=json!({"media":[{"id":"front","kind":"file","handle":"owned-a","mime":"image/png","fileName":"a.png","byteLength":"10"},
            {"id":"left","kind":"file","handle":"owned-b","mime":"image/png","fileName":"b.png","byteLength":"10"}]});
        let mut host=MediaHost{calls:0,same_size:false};
        assert!(validate_media(&mut host,&payload,&document).is_err());assert_eq!(host.calls,2);
        host.same_size=true;validate_media(&mut host,&payload,&document).unwrap();assert_eq!(host.calls,4);
    }
}
