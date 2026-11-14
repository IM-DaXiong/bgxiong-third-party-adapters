//! Frozen upstream video graphs. Frame counts remain exact; seconds are never
//! rounded into a different frame count by the host.
extern crate alloc;
use alloc::{format, string::ToString};
use serde_json::{json, Value};
use crate::mapping::Failure;

pub const OPERATION: &str = "local.thirdparty.comfy-panorama-image/ltx-panorama";
pub const MODEL: &str = "panorama-ltx-2.3";
pub fn matches(payload: &Value) -> bool {
    payload["operationId"] == OPERATION && payload["modelId"] == MODEL
        && payload["capabilityKey"] == "video.generate" && payload["operationVersion"] == 1
}
pub fn contract(base: &Value) -> Value {
    let input = json!({"type":"object","additionalProperties":false,"properties":{
        "prompt":{"type":"string","minLength":1,"maxLength":8000},
        "negativePrompt":{"type":"string","default":""},
        "inputMode":{"type":"string","const":"text_to_video"},
        "outputProjection":{"type":"string","enum":["equirectangular360"],"default":"equirectangular360","title":"视频类型"},
        "width":{"type":"integer","const":2048},"height":{"type":"integer","const":1024},
        "aspectRatio":{"type":"string","const":"2:1"},"fps":{"type":"integer","const":24},
        "seed":{"type":"integer","minimum":0,"maximum":9007199254740991u64,"default":12345,"title":"随机种子"},
        "advanced":{"type":"object","additionalProperties":false,"properties":{
            "frameCount":{"type":"integer","minimum":1,"default":121,"title":"帧数"},
            "distilledStrength":{"type":"number","minimum":0,"maximum":1,"default":0.5,"title":"蒸馏强度"},
            "panoramaStrength":{"type":"number","minimum":0,"maximum":1,"default":0.6,"title":"全景强度"},
            "crf":{"type":"integer","minimum":0,"maximum":51,"default":19,"title":"编码质量 CRF"}
        },"required":["frameCount","distilledStrength","panoramaStrength","crf"],
        "default":{"frameCount":121,"distilledStrength":0.5,"panoramaStrength":0.6,"crf":19}}
    },"required":["prompt","negativePrompt","inputMode","outputProjection","width","height","aspectRatio","fps","seed","advanced"]});
    let schema=crate::capability::schema("ltx-input", &input.to_string());
    let mut c=base.clone();
    c["localId"]=json!(MODEL); c["modelIds"]=json!([MODEL]);
    c["capabilityKey"]=json!("video.generate"); c["transportSlot"]=json!("video");
    let op=&mut c["operations"][0];
    op["id"]=json!(OPERATION); op["label"]=json!("LTX 2.3 · 全景视频");
    op["inputSchema"]=json!({"id":"ltx-input","digest":schema["digest"]});
    op["uiSchema"]=json!({"controls":[]});op["mediaSlots"]=json!([]);
    op["outputSlots"]=json!([{"role":"result","kind":"video","minCount":1,"maxCount":1}]);
    c["schemas"][0]=schema;c
}
pub fn graph(input: &Value) -> Result<(Value,Value),Failure> {
    let frames=input["advanced"]["frameCount"].as_u64().filter(|n| *n>0 && (*n-1)%8==0)
        .ok_or_else(||Failure::invalid("LTX frameCount must be 8*n+1; frames are never silently rounded"))?;
    if input["outputProjection"]!="equirectangular360" || input["fps"]!=24 || input["width"]!=2048 || input["height"]!=1024 {
        return Err(Failure::invalid("The frozen LTX workflow exports 2048x1024 ERP at 24 fps"));
    }
    let seed=input["seed"].as_u64().filter(|n|*n<=9007199254740991).ok_or_else(||Failure::invalid("Invalid seed"))?;
    let prompt=input["prompt"].as_str().filter(|p|!p.trim().is_empty()).ok_or_else(||Failure::invalid("Missing video prompt"))?;
    let mut graph:Value=serde_json::from_str(include_str!("../workflow/api/LTX-2.3_360vr_distilled_3stage.json")).expect("frozen graph");
    graph["121"]["inputs"]["text"]=json!(prompt);
    graph["110"]["inputs"]["text"]=input["negativePrompt"].clone();
    graph["112"]["inputs"]["value"]=json!(frames);
    for id in ["114","115","189"] {graph[id]["inputs"]["noise_seed"]=json!(seed);}
    for (id,key) in [("134","distilledStrength"),("195","panoramaStrength")] {
        let value=input["advanced"][key].as_f64().filter(|n|(0.0..=1.0).contains(n)).ok_or_else(||Failure::invalid(format!("Invalid {key}")))?;
        graph[id]["inputs"]["strength_model"]=json!(value);
    }
    let crf=input["advanced"]["crf"].as_u64().filter(|n|*n<=51).ok_or_else(||Failure::invalid("Invalid CRF"))?;
    // Export the actual ERP decoder, not the example's perspective cutout preview.
    let mut output=graph["140"].clone();output["inputs"]["images"]=json!(["127",0]);
    output["inputs"]["crf"]=json!(crf);output["inputs"]["filename_prefix"]=json!("bgx_ltx_panorama");
    graph["66"]=output;
    for id in ["140","197","198"] {graph.as_object_mut().unwrap().remove(id);}
    Ok((graph,input.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ltx_preserves_exact_frames_audio_and_full_sphere_in_all_three_stages() {
        let mut input=json!({"prompt":"a studio","negativePrompt":"","fps":24,"width":2048,"height":1024,"outputProjection":"equirectangular360","seed":12345,
            "advanced":{"frameCount":121,"distilledStrength":0.5,"panoramaStrength":0.6,"crf":19}});
        let (g,a)=graph(&input).unwrap();
        assert_eq!(g["112"]["inputs"]["value"],121);
        assert_eq!(g["66"]["inputs"]["images"],json!(["127",0]));
        assert_eq!(g["66"]["inputs"]["audio"],json!(["128",0]));
        assert_eq!(a["advanced"]["frameCount"],121);
        for id in ["114","115","189"] {assert_eq!(g[id]["inputs"]["noise_seed"],12345);}
        for node in g.as_object().unwrap().values() {for v in node["inputs"].as_object().unwrap().values() {
            if let Some(link)=v.as_array().filter(|a|a.len()==2 && a[0].is_string() && a[1].is_number()) {assert!(g.get(link[0].as_str().unwrap()).is_some());}
        }}
        input["advanced"]["frameCount"]=json!(120);assert!(graph(&input).is_err());
        input["advanced"]["frameCount"]=json!(1);assert_eq!(graph(&input).unwrap().0["112"]["inputs"]["value"],1);
        input["outputProjection"]=json!("flat");assert!(graph(&input).is_err());
    }
}
