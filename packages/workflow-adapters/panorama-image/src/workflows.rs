//! Product operations assembled from the frozen upstream examples. State, camera and
//! pose handoff remain upstream-owned; this module only binds their graph inputs.
extern crate alloc;
use alloc::{collections::BTreeMap, format, string::{String, ToString}};
use serde_json::{json, Value};
use crate::{mapping::Failure, scene::SceneDocument};

pub const OPERATIONS: [(&str,&str,&str);4] = [
    ("studio-render", "panorama-studio-render", "全景工作室 · 原始渲染"),
    ("studio-cutout", "panorama-studio-cutout", "Qwen 2.1 · 局部修补返回"),
    ("studio-3d", "panorama-studio-3d", "Qwen 2.1 · 三维场景生成"),
    ("studio-moge", "panorama-studio-moge", "Qwen 2.1 · 深度新视角"),
];
pub fn operation(id: &str) -> Option<&'static str> {
    OPERATIONS.iter().find(|(suffix,_,_)|id==format!("{}/{}",crate::capability::PLUGIN_ID,suffix)).map(|v|v.0)
}
pub fn matches(payload: &Value) -> bool {
    operation(payload["operationId"].as_str().unwrap_or("")).is_some_and(|op|
        OPERATIONS.iter().any(|(suffix,model,_)|*suffix==op && payload["modelId"]==*model))
}
pub fn schema(op: &str) -> Value {
    let mut schema: Value=serde_json::from_str(include_str!("../schemas/studio-input.json")).expect("schema");
    let properties=schema["properties"].as_object_mut().expect("properties");
    properties.remove("width"); properties.remove("height");
    properties.insert("outputProjection".into(),json!({"type":"string","enum":if op=="studio-cutout" {json!(["equirectangular360"])} else {json!(["flat","equirectangular180","equirectangular360"])},"default":if op=="studio-cutout" {"equirectangular360"} else {"flat"},"title":"图片类型"}));
    properties.get_mut("references").unwrap()["description"]=json!("");
    properties.insert("advanced".into(),json!({"type":"object","additionalProperties":false,
        "properties":{
            "megapixels":{"type":"number","minimum":0.1,"maximum":16,"default":2.0,"title":"百万像素"},
            "multiple":{"type":"integer","minimum":8,"maximum":128,"multipleOf":4,"default":8,"title":"尺寸倍数"},
            "steps":{"type":"integer","minimum":1,"maximum":50,"default":25,"title":"采样步数"},
            "backgroundColor":{"type":"string","pattern":"^#[0-9a-fA-F]{6}$","default":"#00ff00","title":"背景颜色"},
            "backgroundRange":{"type":"string","enum":["360","180"],"default":"360","title":"背景范围"}
        },"required":["megapixels","multiple","steps"],"default":{"megapixels":2.0,"multiple":8,"steps":25,"backgroundColor":"#00ff00","backgroundRange":"360"}}));
    schema["required"].as_array_mut().unwrap().retain(|v|v!="width" && v!="height");
    if op=="studio-render" {
        schema["properties"]["prompt"]["minLength"]=json!(0);
        schema["properties"]["prompt"]["default"]=json!("");
        schema["properties"].as_object_mut().unwrap().remove("seed");
        schema["required"].as_array_mut().unwrap().retain(|v|v!="seed");
        schema["properties"]["advanced"]["properties"].as_object_mut().unwrap().remove("steps");
        schema["properties"]["advanced"]["default"].as_object_mut().unwrap().remove("steps");
        schema["properties"]["advanced"]["required"].as_array_mut().unwrap().retain(|v|v!="steps");
    }
    if op=="studio-moge" {
        schema["properties"]["sourceName"]=json!({"type":"string","minLength":1,"title":"深度源素材名称"});
        schema["required"].as_array_mut().unwrap().push(json!("sourceName"));
    }
    schema
}
pub fn contracts(base: &Value) -> alloc::vec::Vec<Value> {
    OPERATIONS.iter().map(|(op,model,label)| {
        let mut c=base.clone(); let schema_text=schema(op).to_string();
        let s=crate::capability::schema("workflow-input",&schema_text);
        c["localId"]=json!(model); c["modelIds"]=json!([model]);
        c["operations"][0]["id"]=json!(format!("{}/{}",crate::capability::PLUGIN_ID,op));
        c["operations"][0]["label"]=json!(label);
        c["operations"][0]["inputSchema"]=json!({"id":"workflow-input","digest":s["digest"]});
        c["operations"][0]["uiSchema"]=json!({"controls":[]});
        c["schemas"][0]=s; c
    }).collect()
}
pub(crate) fn projection(state: &Value) -> Result<&'static str,Failure> {
    match state["camera"]["projection"].as_str() {
        Some("perspective")=>Ok("flat"),
        Some("erp")=>match state["camera"]["erp_range"].as_u64() {Some(180)=>Ok("equirectangular180"),Some(360)=>Ok("equirectangular360"),_=>Err(Failure::invalid("Save an explicit panorama camera range"))},
        _=>Err(Failure::invalid("Save a camera before generating")),
    }
}
pub fn graph(op:&str,input:&Value,uploads:&BTreeMap<String,String>)->Result<(Value,Value),Failure> {
    let doc=SceneDocument::parse(input["editorDocument"].as_str().ok_or_else(||Failure::invalid("Missing saved editor document"))?)?;
    doc.validate_references(input)?;
    let state=doc.bind(uploads)?;
    let camera_projection=projection(&state)?;
    let expected=if op=="studio-cutout" {"equirectangular360"} else {camera_projection};
    if input["outputProjection"]!=expected || (op=="studio-cutout" && camera_projection!="flat") {
        return Err(Failure::invalid("Output projection differs from the saved camera; local repair requires Frame mode"));
    }
    let mp=input["advanced"]["megapixels"].as_f64().ok_or_else(||Failure::invalid("Missing megapixels"))?;
    let multiple=input["advanced"]["multiple"].as_u64().ok_or_else(||Failure::invalid("Missing size multiple"))?;
    let render=crate::scene::render_graph(&state,mp,multiple)?;
    let (mut graph,studio,output)=match op {
        "studio-render"=>(render.clone(),"70","66"),
        "studio-cutout"=>(serde_json::from_str(include_str!("../workflow/api/qwen-image-2.1-panorama-cutout.json")).expect("frozen graph"),"105","106"),
        "studio-3d"=>(serde_json::from_str(include_str!("../workflow/api/qwen-image-2.1-panorama-with-3d.json")).expect("frozen graph"),"107","104"),
        "studio-moge"=>(serde_json::from_str(include_str!("../workflow/api/qwen-image-2.1-moge3-anyangle.json")).expect("frozen graph"),"97","96"),
        _=>return Err(Failure::invalid("Unknown Studio workflow")),
    };
    let world=graph[studio]["inputs"].get("world").cloned();
    graph[studio]=render["70"].clone();
    // Absent fields keep the explicitly declared upstream defaults. Invalid
    // caller values are never corrected, and parent scenes keep their own inputs.
    let color=input["advanced"].get("backgroundColor").map_or(Some("#00ff00"),Value::as_str)
        .filter(|s|s.len()==7&&s.starts_with('#')&&s[1..].bytes().all(|b|b.is_ascii_hexdigit()))
        .ok_or_else(||Failure::invalid("Invalid background color"))?;
    let range=input["advanced"].get("backgroundRange").map_or(Some("360"),Value::as_str)
        .filter(|s|matches!(*s,"180"|"360")).ok_or_else(||Failure::invalid("Invalid background range"))?;
    graph[studio]["inputs"]["background_color"]=json!(color);
    graph[studio]["inputs"]["background_range"]=json!(range);
    if let Some(world)=world {graph[studio]["inputs"]["world"]=world;}
    doc.connect_parents(&mut graph,studio,uploads,mp,multiple)?;
    if op=="studio-cutout" {
        let mut returned:Value=serde_json::from_str(graph["103"]["inputs"]["state_json"].as_str().ok_or_else(||Failure::invalid("Frozen return state is missing"))?).map_err(|_|Failure::invalid("Frozen return state is invalid"))?;
        returned["camera"]=json!({"projection":"erp","erp_range":360});
        graph["103"]["inputs"]["state_json"]=json!(returned.to_string());
        graph["103"]["inputs"]["megapixels"]=json!(mp); graph["103"]["inputs"]["multiple"]=json!(multiple);
    }
    if op=="studio-moge" {
        let name=input["sourceName"].as_str().ok_or_else(||Failure::invalid("Choose the saved depth source asset"))?;
        let id=doc.assets.get(name).ok_or_else(||Failure::invalid("Depth source is not in this frozen document"))?;
        graph["80"]["inputs"]["image"]=json!(uploads.get(id).ok_or_else(||Failure::invalid("Depth source lease is missing"))?);
    } else {graph.as_object_mut().unwrap().remove("80");}
    if op!="studio-render" {
        let seed=input["seed"].as_u64().filter(|n|*n<=9007199254740991).ok_or_else(||Failure::invalid("Invalid seed"))?;
        let steps=input["advanced"]["steps"].as_u64().filter(|n|(1..=50).contains(n)).ok_or_else(||Failure::invalid("Invalid steps"))?;
        graph["3"]["inputs"]["seed"]=json!(seed); graph["3"]["inputs"]["steps"]=json!(steps);
        graph["64"]["inputs"]["prompt"]=input["prompt"].clone();
    }
    let result=graph[output].clone();
    graph["66"]=result; graph["66"]["inputs"]["filename_prefix"]=json!(format!("bgx_{op}"));
    // Only the selected result is a product output; discard duplicate SaveImage side
    // effects, keeping upstream scene/sticker_state connections inside the graph.
    graph.as_object_mut().unwrap().retain(|id,node|id=="66" || node["class_type"]!="SaveImage");
    let mut applied=json!({"prompt":input["prompt"],"outputProjection":expected,"advanced":input["advanced"]});
    if op!="studio-render" {applied["seed"]=input["seed"].clone();}
    Ok((graph,applied))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_examples_keep_camera_handoff_and_have_no_missing_graph_inputs() {
        for (op,_,_) in OPERATIONS {
            let prompt_schema=schema(op)["properties"]["prompt"].clone();
            assert_eq!(prompt_schema["minLength"],if op=="studio-render" {0} else {1});
            if op=="studio-render" {assert_eq!(prompt_schema["default"],"");}
            for projection in ["flat","equirectangular180","equirectangular360"] {
                if op=="studio-cutout" && projection!="flat" {continue;}
                let camera=match projection {"flat"=>json!({"projection":"perspective","aspect":1.5,"rotation":{"value":{"yaw_deg":-90,"pitch_deg":12,"roll_deg":3}},"fov":{"value":65}}),p=>json!({"projection":"erp","erp_range":if p=="equirectangular180" {180} else {360}})};
                let state=json!({"version":2,"camera":camera,"timeline":{"fps":24,"length":null},"stickers":[]});
                let doc=json!({"stateText":state.to_string(),"assets":[{"name":"input//source.png","mediaId":"source"}]});
                let mut input=json!({"editorDocument":doc.to_string(),"references":[{"mediaId":"source"}],"sourceName":"input//source.png",
                    "prompt":"a studio","seed":12345,"outputProjection":if op=="studio-cutout" {"equirectangular360"} else {projection},"advanced":{"megapixels":2,"multiple":8,"steps":20}});
                let uploads=BTreeMap::from([("source".into(),"uploaded/source.png".into())]);
                if op=="studio-render" {input["prompt"]=json!("");}
                let (graph,applied)=graph(op,&input,&uploads).unwrap();
                assert_eq!(graph["66"]["class_type"],"SaveImage");
                assert_eq!(applied["outputProjection"],input["outputProjection"]);
                let studio=match op {"studio-render"=>"70","studio-cutout"=>"105","studio-3d"=>"107",_=>"97"};
                assert_eq!(graph[studio]["inputs"]["background_color"],"#00ff00");
                assert_eq!(graph[studio]["inputs"]["background_range"],"360");
                input["advanced"]["backgroundColor"]=json!("#1234ab");
                input["advanced"]["backgroundRange"]=json!("180");
                let (configured,receipt)=super::graph(op,&input,&uploads).unwrap();
                assert_eq!(configured[studio]["inputs"]["background_color"],"#1234ab");
                assert_eq!(configured[studio]["inputs"]["background_range"],"180");
                assert_eq!(receipt["advanced"],input["advanced"]);
                for (field,invalid) in [("backgroundColor",json!("bad")),("backgroundRange",json!("90")),("backgroundColor",Value::Null)] {
                    let mut rejected=input.clone();rejected["advanced"][field]=invalid;
                    assert!(super::graph(op,&rejected,&uploads).is_err());
                }
                for node in graph.as_object().unwrap().values() {
                    for value in node["inputs"].as_object().unwrap().values() {
                        if let Some(link)=value.as_array().filter(|a|a.len()==2 && a[0].is_string() && a[1].is_number()) {assert!(graph.get(link[0].as_str().unwrap()).is_some(),"{op}: missing input {link:?}");}
                    }
                }
                if op=="studio-cutout" {
                    assert_eq!(graph["103"]["inputs"]["sticker_state"],json!(["105",4]));
                    assert_eq!(graph["103"]["inputs"]["sticker"],json!(["8",0]));
                    let saved:Value=serde_json::from_str(graph["105"]["inputs"]["state_json"].as_str().unwrap()).unwrap();
                    assert_eq!(saved["camera"],camera);
                }
                input["references"]=json!([]);
                assert!(super::graph(op,&input,&uploads).is_err());
            }
        }
    }
}
