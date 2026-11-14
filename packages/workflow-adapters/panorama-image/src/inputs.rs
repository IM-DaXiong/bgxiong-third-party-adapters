//! Named, owned external inputs. No resizing, timeline changes or pose inference.
extern crate alloc;
use alloc::{format,string::ToString};
use serde_json::{json,Value};
use crate::mapping::Failure;
pub const STATE_KEY:&str="bgxInputs";
fn closed(value:&Value,fields:&[&str])->Result<(),Failure>{
    if value.as_object().is_none_or(|v|v.keys().any(|k|!fields.contains(&k.as_str()))){return Err(Failure::invalid("Unknown input state; preserve the original document"));} Ok(())
}
pub fn validate(record:&Value)->Result<(),Failure>{
    closed(record,&["version","background","world","sticker","stickerState"])?;
    if record["version"]!=1{return Err(Failure::invalid("Unsupported input state version"));}
    for role in ["background","world","sticker"]{
        let Some(input)=record.get(role) else {continue};
        closed(input,&["mediaKind","source","preview"])?;
        let kind=input["mediaKind"].as_str().ok_or_else(||Failure::invalid("Input media kind missing"))?;
        if !matches!(kind,"image"|"video"|"mesh"|"splat") || (role=="background" && !matches!(kind,"image"|"video")) || (role=="world" && !matches!(kind,"mesh"|"splat")){return Err(Failure::invalid("Media type is not supported by this input"));}
        let source=&input["source"];closed(source,&["kind","filename","subfolder","storage"])?;
        if source["kind"]!="file" || source["filename"].as_str().is_none_or(|v|v.is_empty()||v.contains(['/', '\\', ':'])||matches!(v,"."|".."))
            || source["subfolder"].as_str().is_none_or(|v|v.contains(['\\', ':'])||v.split('/').any(|p|matches!(p,"."|"..")))
            || !matches!(source["storage"].as_str(),Some("input"|"output"|"temp"|"editor-results")) {return Err(Failure::invalid("Input must refer to an owned file"));}
        if input.get("preview").is_some_and(|v|!v.is_object()){return Err(Failure::invalid("Invalid input preview"));}
    }
    if let Some(pose)=record.get("stickerState"){
        if record.get("sticker").is_none() || pose.as_str().is_none_or(|s|s.len()>1_048_576){return Err(Failure::invalid("Pose requires a sticker input and original JSON text"));}
        let _:Value=serde_json::from_str(pose.as_str().unwrap()).map_err(|_|Failure::invalid("Pose is not valid JSON"))?;
    }
    Ok(())
}
pub fn attach(graph:&mut Value,studio:&str,state:&Value)->Result<(),Failure>{
    let Some(record)=state.get(STATE_KEY) else {return Ok(())};validate(record)?;
    for role in ["background","world","sticker"]{
        let Some(input)=record.get(role) else {continue};
        if graph[studio]["inputs"].get(role).is_some(){return Err(Failure::invalid(format!("Workflow already owns the {role} input")));}
        let source=&input["source"];
        if source["storage"]!="input" {return Err(Failure::invalid("Input has not been bound to its uploaded file"));}
        let file=match source["subfolder"].as_str().unwrap(){""=>source["filename"].as_str().unwrap().to_string(),folder=>format!("{folder}/{}",source["filename"].as_str().unwrap())};
        let id=format!("{studio}_bgx_input_{role}");
        let (node,key)=match input["mediaKind"].as_str().unwrap(){"image"=>("LoadImage","image"),"video"=>("BGXPanoramaVideo","file"),_=>("BGXPanoramaFile3D","file")};
        graph[&id]=json!({"class_type":node,"inputs":{key:file}});
        graph[studio]["inputs"][role]=json!([id,0]);
    }
    if let Some(pose)=record.get("stickerState"){
        if graph[studio]["inputs"].get("sticker_state").is_some(){return Err(Failure::invalid("Workflow already owns the sticker pose"));}
        graph[studio]["inputs"]["sticker_state"]=pose.clone();
    }
    Ok(())
}
#[cfg(test)] mod tests{
    use super::*;
    #[test] fn input_graph_keeps_sources_and_rejects_conflicting_or_unknown_inputs(){
        let source=json!({"kind":"file","filename":"room.glb","subfolder":"owned","storage":"input"});
        let mut state=json!({"version":2,"bgxInputs":{"version":1,"world":{"mediaKind":"mesh","source":source}}});
        let mut graph=json!({"s":{"class_type":"PanoramaStudio","inputs":{}}});
        attach(&mut graph,"s",&state).unwrap();assert_eq!(graph["s_bgx_input_world"]["inputs"]["file"],"owned/room.glb");
        assert!(attach(&mut graph,"s",&state).is_err());
        state[STATE_KEY]["world"]["source"]["subfolder"]=json!("../escape");assert!(validate(&state[STATE_KEY]).is_err());
        state[STATE_KEY]=json!({"version":1,"future":true});assert!(validate(&state[STATE_KEY]).is_err());
        state[STATE_KEY]=json!({"version":1,"background":{"mediaKind":"mesh","source":source}});assert!(validate(&state[STATE_KEY]).is_err());
        state[STATE_KEY]=json!({"version":1,"sticker":{"mediaKind":"video","source":source},"stickerState":"{\"original\":true}"});
        let mut graph=json!({"s":{"class_type":"PanoramaStudio","inputs":{}}});attach(&mut graph,"s",&state).unwrap();
        assert_eq!(graph["s_bgx_input_sticker"]["class_type"],"BGXPanoramaVideo");assert_eq!(graph["s"]["inputs"]["sticker_state"],"{\"original\":true}");
    }
}
