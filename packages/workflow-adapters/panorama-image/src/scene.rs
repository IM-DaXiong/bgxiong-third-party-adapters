//! Studio state stays in the upstream format. Only persistent file names are translated
//! to this submission's Comfy uploads; poses, paint, keyframes and unknown fields survive.
extern crate alloc;
use alloc::{collections::BTreeMap, format, string::{String, ToString}};
use serde_json::{json, Value};
use crate::mapping::Failure;

pub struct SceneDocument {
    pub state: Value,
    pub assets: BTreeMap<String, String>,
}

impl SceneDocument {
    pub fn parse(text: &str) -> Result<Self, Failure> {
        let document: Value = serde_json::from_str(text).map_err(|_| Failure::invalid("Editor document is not JSON"))?;
        let fields = document.as_object().ok_or_else(|| Failure::invalid("Editor document must be an object"))?;
        if fields.len() != 2 || !fields.contains_key("stateText") || !fields.contains_key("assets") {
            return Err(Failure::invalid("Editor document requires stateText and assets"));
        }
        let state: Value = serde_json::from_str(document["stateText"].as_str().ok_or_else(|| Failure::invalid("stateText must be JSON text"))?)
            .map_err(|_| Failure::invalid("Studio state is not JSON"))?;
        if state["version"] != 2 { return Err(Failure::invalid("This operation requires Studio state version 2")); }
        let mut assets = BTreeMap::new();
        for asset in document["assets"].as_array().ok_or_else(|| Failure::invalid("Editor assets must be an array"))? {
            let name = asset["name"].as_str().filter(|value| !value.is_empty()).ok_or_else(|| Failure::invalid("Missing asset name"))?;
            let media = asset["mediaId"].as_str().filter(|value| !value.is_empty()).ok_or_else(|| Failure::invalid("Missing asset mediaId"))?;
            if asset.as_object().is_none_or(|fields| fields.len() != 2) || assets.insert(name.to_string(), media.to_string()).is_some() {
                return Err(Failure::invalid("Duplicate asset name or unknown asset field"));
            }
        }
        Ok(Self { state, assets })
    }

    pub fn bind(&self, uploads: &BTreeMap<String, String>) -> Result<Value, Failure> {
        let mut state = self.state.clone();
        self.bind_value(&mut state, uploads)?;
        Ok(state)
    }

    fn bind_value(&self, value: &mut Value, uploads: &BTreeMap<String, String>) -> Result<(), Failure> {
        match value {
            Value::Object(fields) => {
                let file = fields.get("kind").and_then(Value::as_str) == Some("file")
                    || fields.get("type").and_then(Value::as_str) == Some("comfy_image");
                if file {
                    let filename = fields.get("filename").and_then(Value::as_str).ok_or_else(|| Failure::invalid("Scene file has no filename"))?;
                    let folder = fields.get("subfolder").and_then(Value::as_str).unwrap_or("");
                    let storage = fields.get("storage").and_then(Value::as_str).unwrap_or("input");
                    let logical = format!("{storage}/{folder}/{filename}");
                    let media = self.assets.get(&logical).ok_or_else(|| Failure::invalid(format!("Scene source is not in the frozen document: {logical}")))?;
                    let upload = uploads.get(media).ok_or_else(|| Failure::invalid(format!("Scene asset has no uploaded lease: {media}")))?;
                    // Comfy upload returns a relative input path. Preserve its actual subfolder.
                    if upload.is_empty() || upload.starts_with('/') || upload.contains(['\\', ':']) || upload.split('/').any(|part| part.is_empty() || matches!(part, "." | "..")) {
                        return Err(Failure::invalid("Comfy upload returned an unsafe relative filename"));
                    }
                    let (folder, filename) = upload.rsplit_once('/').unwrap_or(("", upload.as_str()));
                    fields.insert("filename".into(), json!(filename));
                    fields.insert("subfolder".into(), json!(folder));
                    fields.insert("storage".into(), json!("input"));
                } else {
                    for child in fields.values_mut() { self.bind_value(child, uploads)?; }
                }
            }
            Value::Array(items) => for child in items { self.bind_value(child, uploads)?; },
            _ => {}
        }
        Ok(())
    }
}

/// Native Studio render graph: useful without diffusion weights. Output 1 is first_frame;
/// output 0 is PANO_SCENE and must never be passed to an image consumer.
pub fn render_graph(state: &Value, megapixels: f64, multiple: u64) -> Result<Value, Failure> {
    if !(0.1..=16.0).contains(&megapixels) || !(8..=128).contains(&multiple) || multiple % 4 != 0 {
        return Err(Failure::invalid("Studio output size is outside the upstream range"));
    }
    Ok(json!({
        "70":{"class_type":"PanoramaStudio","inputs":{
            "background_color":"#00ff00","background_range":"360","megapixels":megapixels,
            "multiple":multiple,"state_json":state.to_string()
        }},
        "66":{"class_type":"SaveImage","inputs":{"images":["70",1],"filename_prefix":"bgx_panorama_studio"}}
    }))
}

pub fn outpaint_graph(input: &Value, uploads: &BTreeMap<String, String>) -> Result<(Value, Value), Failure> {
    let mut basic = input.clone();
    let document = basic.as_object_mut().ok_or_else(|| Failure::invalid("Input must be an object"))?
        .remove("editorDocument").ok_or_else(|| Failure::invalid("Save a Studio document before generating"))?;
    let document = SceneDocument::parse(document.as_str().ok_or_else(|| Failure::invalid("editorDocument must be text"))?)?;
    let references = input["references"].as_array().ok_or_else(|| Failure::invalid("references must be an array"))?;
    if references.len() != document.assets.len() || document.assets.values().any(|id| !references.iter().any(|r| r["mediaId"] == *id)) {
        return Err(Failure::invalid("Scene asset list differs from the frozen media slots"));
    }
    let state = document.bind(uploads)?;
    // This diffusion graph is a single 360 still. Other projections/video have separate operations.
    if state["camera"]["projection"] != "erp" || state["camera"]["erp_range"] != 360 {
        return Err(Failure::invalid("This generation operation requires a 360 ERP camera"));
    }
    basic["references"] = json!([{"mediaId":"validated-scene"}]);
    let (_, applied) = crate::mapping::build_graph(&basic, "scene.png")?;
    let mut graph: Value = serde_json::from_str(include_str!("../workflow/qwen-2.1-scene-api.json")).expect("frozen upstream graph");
    graph["83"]["inputs"]["state_json"] = json!(state.to_string());
    graph["83"]["inputs"]["megapixels"] = json!(2.0);
    graph["83"]["inputs"]["multiple"] = json!(32);
    graph["64"]["inputs"]["prompt"] = applied["prompt"].clone();
    graph["3"]["inputs"]["seed"] = applied["seed"].clone();
    graph["3"]["inputs"]["steps"] = basic["advanced"]["steps"].clone();
    graph["66"]["inputs"]["filename_prefix"] = json!("bgx_qwen_panorama_studio");
    Ok((graph, applied))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remaps_every_frozen_source_without_changing_scene_semantics() {
        let state = json!({"version":2,"camera":{"projection":"erp","erp_range":360},
            "stickers":[
                {"id":"desk","source":{"kind":"file","filename":"desk.png","subfolder":"old","storage":"input"},"direction":{"value":{"yaw_deg":0}}},
                {"id":"door","source":{"kind":"file","filename":"door.png","storage":"temp"},"direction":{"value":{"yaw_deg":-90}}}
            ],
            "painting_layer":{"paint":{"type":"comfy_image","filename":"paint.png","storage":"input"}},
            "timeline":{"fps":24,"length":48},"future":{"preserved":true}});
        let doc = SceneDocument::parse(&json!({"stateText":state.to_string(),"assets":[
            {"name":"input/old/desk.png","mediaId":"a"},{"name":"temp//door.png","mediaId":"b"},
            {"name":"input//paint.png","mediaId":"c"}]}).to_string()).unwrap();
        let uploads = BTreeMap::from([("a".into(),"new/desk.png".into()),("b".into(),"new/door.png".into()),("c".into(),"paint.png".into())]);
        let bound = doc.bind(&uploads).unwrap();
        assert_eq!(bound["stickers"][1]["direction"],state["stickers"][1]["direction"]);
        assert_eq!(bound["stickers"][1]["source"]["storage"],"input");
        assert_eq!(bound["stickers"][0]["source"]["subfolder"],"new");
        assert_eq!(bound["timeline"],state["timeline"]);
        assert_eq!(bound["future"],state["future"]);
        assert_eq!(bound["painting_layer"]["paint"]["filename"],"paint.png");
        assert!(doc.bind(&BTreeMap::new()).is_err());
        let mut unsafe_upload = uploads.clone(); unsafe_upload.insert("a".into(),"../outside.png".into());
        assert!(doc.bind(&unsafe_upload).is_err());
        assert_eq!(render_graph(&bound,2.0,8).unwrap()["66"]["inputs"]["images"],json!(["70",1]));
        let mut input = json!({"prompt":"A studio with a bedroom door on the left and entrance behind",
            "outputProjection":"equirectangular360","width":2048,"height":1024,"seed":12345,"advanced":{"steps":20},
            "references":[{"mediaId":"a"},{"mediaId":"b"},{"mediaId":"c"}],
            "editorDocument":json!({"stateText":state.to_string(),"assets":[
                {"name":"input/old/desk.png","mediaId":"a"},{"name":"temp//door.png","mediaId":"b"},
                {"name":"input//paint.png","mediaId":"c"}]}).to_string()});
        let (graph, applied) = outpaint_graph(&input, &uploads).unwrap();
        assert_eq!(graph["83"]["class_type"], "PanoramaStudio");
        assert_eq!(graph["64"]["inputs"]["images.image_1"], json!(["83",1]));
        assert_eq!(graph["37"]["inputs"]["unet_name"], "qwen_image_2.1_int8_convrot.safetensors");
        assert_eq!(graph["3"]["inputs"]["seed"], 12345);
        assert!(graph.get("100").is_none());
        let rendered: Value = serde_json::from_str(graph["83"]["inputs"]["state_json"].as_str().unwrap()).unwrap();
        assert_eq!(rendered["stickers"][1]["direction"], state["stickers"][1]["direction"]);
        assert_eq!(applied["seed"],12345);
        input["references"] = json!([]);
        assert!(outpaint_graph(&input, &uploads).is_err());
    }
}
