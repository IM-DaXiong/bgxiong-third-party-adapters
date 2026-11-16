use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use serde_json::{json, Value};

use super::{schema, MODEL_ID, PLUGIN_ID};

pub(crate) struct Operation {
    key: &'static str,
    pub mode: &'static str,
    label: &'static str,
    pub path: &'static str,
    pub image: Option<ImageSlot>,
}

#[derive(Clone, Copy)]
pub(crate) struct ImageSlot {
    pub field: &'static str,
    pub wire_field: &'static str,
    label: &'static str,
    pub role: &'static str,
    max: Option<u32>,
}

const FIRST: ImageSlot = ImageSlot {
    field: "firstFrame",
    wire_field: "first_frame_b64",
    label: "首帧",
    role: "first_frame",
    max: Some(1),
};
const REFERENCES: ImageSlot = ImageSlot {
    field: "referenceImages",
    wire_field: "reference_images_b64",
    label: "参考图片",
    role: "reference_image",
    max: None,
};

const OPERATIONS: &[Operation] = &[
    Operation {
        key: "text",
        mode: "text_to_video",
        label: "通用文生视频",
        path: "text2video",
        image: None,
    },
    Operation {
        key: "image",
        mode: "image_to_video",
        label: "通用图生视频",
        path: "image2video",
        image: Some(FIRST),
    },
    Operation {
        key: "camera",
        mode: "camera_control",
        label: "通用运镜",
        path: "image2video",
        image: Some(FIRST),
    },
    Operation {
        key: "multi-image",
        mode: "multi_image_to_video",
        label: "通用多图生视频",
        path: "multi_image2video",
        image: Some(REFERENCES),
    },
];

impl Operation {
    fn id(&self) -> String {
        format!("{PLUGIN_ID}/{}", self.key)
    }

    fn declare(&self) -> Value {
        let mut spec: Value = serde_json::from_str(include_str!("../schemas/input.json"))
            .expect("generic video input schema");
        spec["properties"]["inputMode"] =
            json!({"type":"string","enum":[self.mode],"const":self.mode,"default":self.mode});
        let mut controls = vec![
            control("/inputMode", "select", "生成方式", Some("inputMode")),
            control("/prompt", "multiline", "提示词", Some("prompt")),
            control("/providerParameters", "object", "接口参数", None),
        ];
        let slots = if let Some(image) = self.image {
            let pointer = format!("/{}", image.field);
            let mut field =
                json!({"type":"array","minItems":1,"items":{"type":"string","minLength":1}});
            if let Some(maximum) = image.max {
                field["maxItems"] = json!(maximum);
            }
            spec["properties"][image.field] = field;
            spec["required"]
                .as_array_mut()
                .expect("required array")
                .push(json!(image.field));
            controls.push(control(&pointer, "media", image.label, None));
            vec![
                json!({"inputPointer":pointer,"role":image.role,"mediaKind":"image","minCount":1,"maxCount":image.max}),
            ]
        } else {
            Vec::new()
        };
        let input = schema("input", &spec.to_string());
        let config = schema("config", include_str!("../schemas/configuration.json"));
        let output = schema("output", include_str!("../schemas/output.json"));
        let resume = schema("resume", include_str!("../schemas/output.json"));
        json!({
            "schemaVersion":2,"localId":self.key,"capabilityKey":"video.generate","transportSlot":"video","modelIds":[MODEL_ID],
            "configurationSchema":{"id":"config","digest":config["digest"]},"schemas":[config,input,output,resume],
            "operations":[{"id":self.id(),"version":1,"label":self.label,"inputProfile":"operation-input-v1",
                "inputSchema":{"id":"input","digest":input["digest"]},"outputSchema":{"id":"output","digest":output["digest"]},
                "resumeSchema":{"id":"resume","digest":resume["digest"]},"uiSchema":{"controls":controls},"mediaSlots":slots,
                "outputSlots":[{"role":"video","kind":"video","minCount":1,"maxCount":null},
                    {"role":"provider-response","kind":"file","minCount":1,"maxCount":1}],
                "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported","resumable":true,"remoteIdempotency":false}}]
        })
    }
}

fn control(pointer: &str, widget: &str, label: &str, role: Option<&str>) -> Value {
    let mut item =
        json!({"fieldPointer":pointer,"widget":widget,"label":label,"group":"","description":""});
    if let Some(role) = role {
        item["displayRole"] = json!(role);
    }
    item
}

pub(crate) fn capabilities() -> Vec<Value> {
    OPERATIONS.iter().map(Operation::declare).collect()
}

pub(crate) fn operation(selection: &Value) -> Option<&'static Operation> {
    if selection["modelId"] != MODEL_ID
        || selection["capabilityKey"] != "video.generate"
        || selection["operationVersion"] != 1
    {
        return None;
    }
    OPERATIONS
        .iter()
        .find(|operation| selection["operationId"] == operation.id())
}

fn extension_is_reserved(key: &str) -> bool {
    matches!(
        key,
        "model"
            | "model_name"
            | "prompt"
            | "mode"
            | "forceMode"
            | "intentMode"
            | "image"
            | "images"
            | "start_frame"
            | "reference_images"
            | "first_frame_b64"
            | "firstFrameB64"
            | "last_frame_b64"
            | "lastFrameB64"
            | "end_frame_b64"
            | "reference_images_b64"
            | "referenceImagesB64"
            | "reference_video_b64"
            | "reference_videos_b64"
            | "referenceVideosB64"
            | "reference_audio_b64"
            | "reference_audios_b64"
            | "referenceAudiosB64"
            | "reference_video_path"
            | "reference_video_paths"
            | "reference_audio_paths"
            | "referenceMediaSlots"
            | "reference_media_slots"
    )
}

pub(crate) fn material(
    operation: &Operation,
    input: &Value,
    config: &Value,
) -> Result<Value, (&'static str, &'static str)> {
    if input["inputMode"] != operation.mode {
        return Err(("/inputMode", "生成方式与已选通用视频操作不一致"));
    }
    let prompt = input["prompt"]
        .as_str()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .ok_or(("/prompt", "请输入提示词"))?;
    let upstream = config["upstreamModelId"]
        .as_str()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or(("", "未设置通用视频账号的上游模型 ID"))?;
    let mut body = input["providerParameters"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    if body.keys().any(|key| extension_is_reserved(key)) {
        return Err((
            "/providerParameters",
            "接口参数包含由所选操作管理的提示词、模型、方式或媒体字段",
        ));
    }
    body.insert("model".into(), json!(upstream));
    body.insert("prompt".into(), json!(prompt));
    body.insert("intentMode".into(), json!(operation.mode));
    if let Some(image) = operation.image {
        let ids = input[image.field]
            .as_array()
            .filter(|ids| !ids.is_empty() && image.max.is_none_or(|max| ids.len() <= max as usize))
            .ok_or_else(|| {
                if image.max == Some(1) {
                    ("/firstFrame", "请选择一张首帧图")
                } else {
                    ("/referenceImages", "请选择参考图片")
                }
            })?;
        let ordered: Vec<_> = ids.iter().filter_map(Value::as_str).collect();
        if ids.len() != ordered.len() || ordered.iter().any(|id| id.is_empty()) {
            return Err(("", "已选素材身份无效"));
        }
        Ok(json!({"body":body,"imageIds":ordered}))
    } else {
        Ok(json!({"body":body,"imageIds":[]}))
    }
}
