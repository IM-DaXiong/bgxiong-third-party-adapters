//! Capability declaration for local ComfyUI single-image ERP outpaint.
#![cfg_attr(target_arch = "wasm32", no_std)]
extern crate alloc;

use alloc::{format, string::String};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const PLUGIN_ID: &str = "local.thirdparty.comfy-panorama-image";
pub const MODEL_ID: &str = "panorama-image-single";
pub const OPERATION_ID: &str = "local.thirdparty.comfy-panorama-image/image-to-panorama";
pub const SCENE_OPERATION: &str = "local.thirdparty.comfy-panorama-image/scene-to-panorama";
pub const SCENE_MODEL: &str = "panorama-studio-outpaint";
pub const CAPABILITY_KEY: &str = "image.generate";
#[allow(dead_code)]
pub const FEATURE_PROJECTION: &str = "media-projection-v1";

pub const INPUT_SCHEMA: &str = include_str!("../schemas/input.json");
pub const OUTPUT_SCHEMA: &str = include_str!("../schemas/output.json");
pub const CONFIG_SCHEMA: &str = include_str!("../schemas/config.json");
pub const RESUME_SCHEMA: &str = include_str!("../schemas/resume.json");

pub(crate) fn schema(id: &str, source: &str) -> Value {
    let digest: String = Sha256::digest(source.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    json!({"id": id, "digest": digest, "schemaJson": source})
}

/// Public capability contract: image.generate + equirectangular360 + image-projection-v1.
pub fn capabilities() -> Value {
    let input = schema("input", INPUT_SCHEMA);
    let output = schema("output", OUTPUT_SCHEMA);
    let config = schema("config", CONFIG_SCHEMA);
    let resume = schema("resume", RESUME_SCHEMA);
    let mut result = json!({
        "schemaVersion": 2,
        "pluginId": PLUGIN_ID,
        "pluginVersion": env!("CARGO_PKG_VERSION"),
        "capabilityContracts": [{
            "schemaVersion": 2,
            "localId": "panorama-image-single",
            "capabilityKey": CAPABILITY_KEY,
            "transportSlot": "image",
            "modelIds": [MODEL_ID],
            "configurationSchema": {"id": "config", "digest": config["digest"]},
            "operations": [{
                "id": OPERATION_ID,
                "version": 1,
                "label": "单图扩展等距柱状全景",
                "inputSchema": {"id": "input", "digest": input["digest"]},
                "outputSchema": {"id": "output", "digest": output["digest"]},
                "resumeSchema": {"id": "resume", "digest": resume["digest"]},
                "uiSchema": {"controls": [
                    {"fieldPointer": "/references", "widget": "repeat", "label": "参考原图", "description": "恰好一张普通透视场景图", "group": "输入"},
                    {"fieldPointer": "/prompt", "widget": "multiline", "label": "提示词", "description": "用户原文；包内可追加 ERP 触发句", "group": "输入"},
                    {"fieldPointer": "/outputProjection", "widget": "select", "label": "输出投影", "description": "固定 equirectangular360", "group": "输入"},
                    {"fieldPointer": "/width", "widget": "number", "label": "宽度", "description": "固定 2048", "group": "输入"},
                    {"fieldPointer": "/height", "widget": "number", "label": "高度", "description": "固定 1024", "group": "输入"},
                    {"fieldPointer": "/seed", "widget": "number", "label": "种子", "description": "", "group": "输入"},
                    {"fieldPointer": "/advanced/steps", "widget": "number", "label": "步数", "description": "KSampler steps", "group": "高级"}
                ]},
                "mediaSlots": [{
                    "role": "references",
                    "inputPointer": "/references",
                    "mediaKind": "image",
                    "minCount": 1,
                    "maxCount": 1
                }],
                "execution": {
                    "mode": "asynchronous",
                    "streaming": false,
                    "cancellation": "unsupported",
                    "resumable": true,
                    "remoteIdempotency": false
                },
                "outputSlots": [{"role": "result", "kind": "image", "minCount": 1, "maxCount": 1}]
            }],
            "schemas": [input, output, config, resume]
        }]
    });
    let mut scene = result["capabilityContracts"][0].clone();
    let scene_input = schema("studio-input", include_str!("../schemas/studio-input.json"));
    scene["localId"] = json!(SCENE_MODEL);
    scene["modelIds"] = json!([SCENE_MODEL]);
    scene["operations"][0]["id"] = json!(SCENE_OPERATION);
    scene["operations"][0]["label"] = json!("Qwen 2.1 · 多方向全景");
    scene["operations"][0]["inputSchema"] = json!({"id":"studio-input","digest":scene_input["digest"]});
    scene["operations"][0]["mediaSlots"][0]["minCount"] = json!(0);
    scene["operations"][0]["mediaSlots"][0]["mediaKind"] = json!("file");
    scene["operations"][0]["mediaSlots"][0]["maxCount"] = json!(1024);
    scene["operations"][0]["uiSchema"]["controls"][0]["label"] = json!("工程素材");
    scene["operations"][0]["uiSchema"]["controls"][0]["description"] = json!("");
    scene["schemas"][0] = scene_input;
    let workflows=crate::workflows::contracts(&scene);
    let capture=crate::editor_prepare::contract(&scene);
    let geometry=crate::editor_geometry::contract(&scene);
    let repair=crate::editor_repair::contracts(&scene);
    let studio_video=crate::editor_video::contract(&scene);
    let h3_video=crate::h3_editor_video::contract(&scene);
    result["capabilityContracts"].as_array_mut().expect("contracts").push(scene);
    result["capabilityContracts"].as_array_mut().expect("contracts").extend(workflows);
    let video=crate::video_workflows::contract(&result["capabilityContracts"][0]);
    result["capabilityContracts"].as_array_mut().expect("contracts").push(video);
    result["capabilityContracts"].as_array_mut().unwrap().push(studio_video);
    result["capabilityContracts"].as_array_mut().unwrap().push(h3_video);
    result["capabilityContracts"].as_array_mut().expect("contracts").push(capture);
    result["capabilityContracts"].as_array_mut().unwrap().push(geometry);
    result["capabilityContracts"].as_array_mut().unwrap().extend(repair);
    result["capabilityContracts"].as_array_mut().expect("contracts").push(crate::editor_media::contract(&schema("config", CONFIG_SCHEMA)));
    result["capabilityContracts"].as_array_mut().unwrap().push(crate::editor_inputs::contract(&schema("config",CONFIG_SCHEMA)));
    result
}
