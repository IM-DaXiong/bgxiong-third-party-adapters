//! Render the saved Studio timeline through its native VIDEO output. Timing and
//! camera interpolation remain in upstream; the host never reconstructs frames.
extern crate alloc;
use alloc::{collections::BTreeMap, string::{String,ToString}};
use serde_json::{json,Value};
use crate::{mapping::Failure,scene::SceneDocument};

pub const OPERATION:&str="local.thirdparty.comfy-panorama-image/studio-video";
pub const MODEL:&str="panorama-studio-video";
pub fn matches(payload:&Value)->bool {
    payload["operationId"]==OPERATION && payload["modelId"]==MODEL
        && payload["capabilityKey"]=="video.generate" && payload["operationVersion"]==1
}
pub fn contract(base:&Value)->Value {
    let mut schema:Value=serde_json::from_str(include_str!("../schemas/studio-input.json")).expect("schema");
    let p=schema["properties"].as_object_mut().unwrap();
    for key in ["width","height","seed"] {p.remove(key);}
    p.insert("prompt".into(),json!({"type":"string","default":""}));
    p.insert("inputMode".into(),json!({"type":"string","const":"editor_to_video"}));
    p.insert("outputProjection".into(),json!({"type":"string","enum":["flat","equirectangular180","equirectangular360"],"default":"flat","title":"视频类型"}));
    p.insert("resolutionValue".into(),json!({"type":"object","additionalProperties":false,
        "properties":{"kind":{"type":"string","const":"megapixels"},"value":{"type":"number","minimum":0.1,"maximum":16,"multipleOf":0.1}},
        "required":["kind","value"],"default":{"kind":"megapixels","value":2}}));
    p.insert("advanced".into(),json!({"type":"object","additionalProperties":false,"properties":{
        "multiple":{"type":"integer","minimum":8,"maximum":128,"multipleOf":4,"default":8,"title":"尺寸倍数"},
        "backgroundColor":{"type":"string","pattern":"^#[0-9a-fA-F]{6}$","default":"#00ff00","title":"背景颜色"},
        "backgroundRange":{"type":"string","enum":["360","180"],"default":"360","title":"背景范围"}
    },"required":["multiple","backgroundColor","backgroundRange"],"default":{"multiple":8,"backgroundColor":"#00ff00","backgroundRange":"360"}}));
    schema["required"]=json!(["prompt","inputMode","editorDocument","references","outputProjection","resolutionValue","advanced"]);
    let schema=crate::capability::schema("studio-video-input",&schema.to_string());
    let mut c=base.clone();c["localId"]=json!(MODEL);c["modelIds"]=json!([MODEL]);c["capabilityKey"]=json!("video.generate");c["transportSlot"]=json!("video");
    c["operations"][0]["id"]=json!(OPERATION);c["operations"][0]["label"]=json!("全景工作室 · 时间线视频");
    c["operations"][0]["inputSchema"]=json!({"id":"studio-video-input","digest":schema["digest"]});
    c["operations"][0]["uiSchema"]=json!({"controls":[]});
    c["operations"][0]["outputSlots"]=json!([{"role":"result","kind":"video","minCount":1,"maxCount":1}]);
    c["schemas"][0]=schema;c
}
pub fn graph(input:&Value,uploads:&BTreeMap<String,String>)->Result<(Value,Value),Failure> {
    let doc=SceneDocument::parse(input["editorDocument"].as_str().ok_or_else(||Failure::invalid("Save the editor timeline before rendering"))?)?;
    doc.validate_references(input)?;
    let state=doc.bind(uploads)?;
    let projection=crate::workflows::projection(&state)?;
    if input["outputProjection"]!=projection {return Err(Failure::invalid("Video projection differs from the saved camera"));}
    // Explicit length avoids a hidden fallback to one still frame. Fractional fps
    // is legal upstream and is handed through unchanged, including 29.97/23.976.
    let _fps=state["timeline"]["fps"].as_f64().filter(|n|n.is_finite()&&(1.0..=1000.0).contains(n)).ok_or_else(||Failure::invalid("Save a positive timeline fps"))?;
    let _frames=state["timeline"]["length"].as_u64().filter(|n|(1..=10_000_000).contains(n)).ok_or_else(||Failure::invalid("请先设置时间线长度；不会自动裁切或补帧"))?;
    let mp=input["resolutionValue"]["value"].as_f64().ok_or_else(||Failure::invalid("Missing megapixels"))?;
    let multiple=input["advanced"]["multiple"].as_u64().ok_or_else(||Failure::invalid("Missing size multiple"))?;
    let color=input["advanced"]["backgroundColor"].as_str().filter(|s|s.len()==7&&s.starts_with('#')&&s[1..].bytes().all(|b|b.is_ascii_hexdigit())).ok_or_else(||Failure::invalid("Invalid background color"))?;
    let range=input["advanced"]["backgroundRange"].as_str().filter(|s|matches!(*s,"180"|"360")).ok_or_else(||Failure::invalid("Invalid background range"))?;
    let mut graph=crate::scene::render_graph(&state,mp,multiple)?;
    graph["70"]["inputs"]["background_color"]=json!(color);graph["70"]["inputs"]["background_range"]=json!(range);
    doc.connect_parents(&mut graph,"70",uploads,mp,multiple)?;
    graph["66"]=json!({"class_type":"SaveVideo","inputs":{"video":["70",3],"filename_prefix":"bgx_studio_timeline","format":"auto","format.codec":"auto","codec":"auto"}});
    Ok((graph,json!({"outputProjection":projection,"resolutionValue":input["resolutionValue"],"advanced":input["advanced"]})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeline_video_keeps_fractional_fps_exact_frames_and_native_video() {
        for fps in [23.976,24.0,29.97,60.0] {
            let state=json!({"version":2,"timeline":{"fps":fps,"length":73},"camera":{"projection":"erp","erp_range":180},"stickers":[]});
            let mut input=json!({"editorDocument":json!({"stateText":state.to_string(),"assets":[]}).to_string(),"references":[],"outputProjection":"equirectangular180","resolutionValue":{"kind":"megapixels","value":2},"advanced":{"multiple":8,"backgroundColor":"#1234ab","backgroundRange":"180"}});
            let (g,applied)=graph(&input,&BTreeMap::new()).unwrap();
            assert_eq!(g["66"]["inputs"]["video"],json!(["70",3]));
            assert_eq!(g["70"]["inputs"]["state_json"],state.to_string());
            assert_eq!(g["70"]["inputs"]["background_color"],"#1234ab");
            assert_eq!(serde_json::from_str::<Value>(g["70"]["inputs"]["state_json"].as_str().unwrap()).unwrap()["timeline"],state["timeline"]);
            assert!(applied.as_object().unwrap().keys().all(|key|input.get(key).is_some()));
            input["outputProjection"]=json!("flat");assert!(graph(&input,&BTreeMap::new()).is_err());
            input["outputProjection"]=json!("equirectangular180");
            let mut missing=state;missing["timeline"]["length"]=Value::Null;
            input["editorDocument"]=json!(json!({"stateText":missing.to_string(),"assets":[]}).to_string());
            assert!(graph(&input,&BTreeMap::new()).is_err());
        }
    }
}
