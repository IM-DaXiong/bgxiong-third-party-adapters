use alloc::{format, string::String, vec, vec::Vec};
use serde_json::{json, Map, Value};

pub(crate) const MODEL: &str = "doubao-seedance-2-5-260628";
pub(crate) const PLUGIN: &str = "local.bgx.seedance-video";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct MediaSlot {
    pub field: &'static str,
    pub role: &'static str,
    pub kind: &'static str,
    pub label: &'static str,
    pub minimum: u32,
    pub maximum: u32,
}

const FIRST: MediaSlot = MediaSlot {
    field: "firstFrame",
    role: "first_frame",
    kind: "image",
    label: "首帧",
    minimum: 1,
    maximum: 1,
};
const LAST: MediaSlot = MediaSlot {
    field: "lastFrame",
    role: "last_frame",
    kind: "image",
    label: "尾帧",
    minimum: 1,
    maximum: 1,
};
const IMAGES: MediaSlot = MediaSlot {
    field: "referenceImages",
    role: "reference_image",
    kind: "image",
    label: "参考图片",
    minimum: 0,
    maximum: 30,
};
const AUDIOS: MediaSlot = MediaSlot {
    field: "referenceAudios",
    role: "reference_audio",
    kind: "audio",
    label: "参考音频",
    minimum: 0,
    maximum: 10,
};
const VIDEO_SOURCES: &str = "referenceVideoSources";
const VIDEO_SOURCE_POINTER: &str = "/referenceVideoSources";

pub(crate) struct Operation {
    pub id: &'static str,
    pub mode: &'static str,
    pub label: &'static str,
    pub slots: &'static [MediaSlot],
    pub adaptive_only: bool,
    pub task_type: Option<&'static str>,
}

pub(crate) const OPERATIONS: &[Operation] = &[
    Operation {
        id: "text",
        mode: "text_to_video",
        label: "Seedance 2.5 文生视频",
        slots: &[],
        adaptive_only: false,
        task_type: None,
    },
    Operation {
        id: "first-frame",
        mode: "image_to_video",
        label: "Seedance 2.5 首帧",
        slots: &[FIRST],
        adaptive_only: true,
        task_type: None,
    },
    Operation {
        id: "first-last-frame",
        mode: "first_last_frame",
        label: "Seedance 2.5 首尾帧",
        slots: &[FIRST, LAST],
        adaptive_only: true,
        task_type: None,
    },
    Operation {
        id: "reference",
        mode: "multi_image_to_video",
        label: "Seedance 2.5 全模态参考",
        slots: &[IMAGES, AUDIOS],
        adaptive_only: false,
        task_type: Some("reference"),
    },
    Operation {
        id: "edit",
        mode: "video_edit",
        label: "Seedance 2.5 视频编辑",
        slots: &[IMAGES, AUDIOS],
        adaptive_only: true,
        task_type: Some("edit"),
    },
    Operation {
        id: "extend",
        mode: "video_extend",
        label: "Seedance 2.5 视频延长",
        slots: &[IMAGES, AUDIOS],
        adaptive_only: true,
        task_type: Some("extend"),
    },
];

impl Operation {
    pub(crate) fn id(&self) -> String {
        format!("{PLUGIN}/{}", self.id)
    }

    pub(crate) fn version(&self) -> u64 {
        if self.task_type.is_some() { 2 } else { 1 }
    }

    pub(crate) fn input_schema(&self) -> Value {
        let mut schema: Value = serde_json::from_str(include_str!("../schemas/input.json"))
            .expect("Seedance input schema");
        schema["properties"]["inputMode"]["const"] = json!(self.mode);
        schema["properties"]["inputMode"]["enum"] = json!([self.mode]);
        schema["properties"]["inputMode"]["default"] = json!(self.mode);
        if self.adaptive_only {
            schema["properties"]["aspectRatio"]["enum"] = json!(["adaptive"]);
            schema["properties"]["aspectRatio"]["default"] = json!("adaptive");
        }
        if self.task_type == Some("edit") {
            schema["properties"]["durationSeconds"]["const"] = json!(-1);
        }
        if self.task_type.is_some() {
            schema["properties"][VIDEO_SOURCES] = json!({"type":"string","maxLength":12000});
            if self.task_type != Some("reference") {
                schema["required"].as_array_mut().unwrap().push(json!(VIDEO_SOURCES));
            }
        }
        if self.slots.is_empty() {
            schema["properties"]["prompt"]["minLength"] = json!(1);
            schema["required"]
                .as_array_mut()
                .unwrap()
                .push(json!("prompt"));
        }
        for slot in self.slots {
            schema["properties"][slot.field] = json!({"type":"array","minItems":slot.minimum,"maxItems":slot.maximum,
                "items":{"type":"string","minLength":1}});
            if slot.minimum > 0 {
                schema["required"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(slot.field));
            }
        }
        schema
    }

    pub(crate) fn controls(&self) -> Vec<Value> {
        let mut controls = vec![
            control("/inputMode", "select", "生成方式", Some("inputMode")),
            control("/prompt", "multiline", "提示词", Some("prompt")),
            control("/durationSeconds", "number", "时长（秒）", Some("duration")),
            control("/resolution", "select", "分辨率", Some("resolution")),
            control("/aspectRatio", "select", "画幅", Some("aspectRatio")),
            control("/generateAudio", "toggle", "生成声音", None),
            control("/returnLastFrame", "toggle", "返回尾帧", None),
            control("/outputFormat", "select", "文件格式", None),
            control("/priority", "number", "优先级", None),
            control("/watermark", "toggle", "水印", None),
            control("/safetyIdentifier", "text", "终端用户标识", None),
            control("/webSearch", "toggle", "联网搜索", None),
        ];
        controls.extend(
            self.slots
                .iter()
                .map(|slot| control(&format!("/{}", slot.field), "media", slot.label, None)),
        );
        if self.task_type.is_some() {
            controls.push(control(VIDEO_SOURCE_POINTER, "multiline", "参考视频地址或素材 ID", None));
        }
        controls
    }
}

fn control(pointer: &str, widget: &str, label: &str, role: Option<&str>) -> Value {
    let mut value =
        json!({"fieldPointer":pointer,"widget":widget,"label":label,"group":"","description":""});
    if let Some(role) = role {
        value["displayRole"] = json!(role);
    }
    value
}

pub(crate) fn operation(selection: &Value) -> Option<&'static Operation> {
    if selection["modelId"] != MODEL || selection["capabilityKey"] != "video.generate"
    {
        return None;
    }
    OPERATIONS
        .iter()
        .find(|op| selection["operationId"] == op.id() && selection["operationVersion"] == op.version())
}

fn video_sources(input: &Value) -> Result<Vec<&str>, (&'static str, &'static str)> {
    let mut sources = Vec::new();
    for source in input[VIDEO_SOURCES].as_str().unwrap_or_default().lines().map(str::trim).filter(|line| !line.is_empty()) {
        let valid_asset = source.strip_prefix("asset://").is_some_and(|id| {
            !id.is_empty() && id.len() <= 128 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        });
        let valid_url = url::Url::parse(source).is_ok_and(|url| {
            url.scheme() == "https" && url.host_str().is_some() && url.username().is_empty()
                && url.password().is_none() && url.fragment().is_none() && url.query().is_none()
        });
        if !valid_asset && !valid_url {
            return Err((VIDEO_SOURCE_POINTER, "请逐行填写公网 HTTPS 视频地址或 asset:// 素材 ID"));
        }
        if sources.len() == 10 {
            return Err((VIDEO_SOURCE_POINTER, "最多提供十条参考视频地址或素材 ID"));
        }
        sources.push(source);
    }
    Ok(sources)
}

pub(crate) fn material(
    operation: &Operation,
    input: &Value,
) -> Result<Map<String, Value>, (&'static str, &'static str)> {
    if input["inputMode"] != operation.mode {
        return Err(("/inputMode", "输入模式与操作身份不一致"));
    }
    if operation.slots.is_empty()
        && input["prompt"]
            .as_str()
            .is_none_or(|prompt| prompt.trim().is_empty())
    {
        return Err(("/prompt", "请输入提示词"));
    }
    let videos = video_sources(input)?;
    if matches!(operation.task_type, Some("edit" | "extend")) && videos.is_empty() {
        return Err((VIDEO_SOURCE_POINTER, "请填写至少一条参考视频地址或素材 ID"));
    }
    if operation.mode == "multi_image_to_video" && videos.is_empty()
        && operation
            .slots
            .iter()
            .all(|slot| input[slot.field].as_array().is_none_or(Vec::is_empty))
    {
        return Err(("/referenceImages", "请选择至少一份参考图片、视频或音频"));
    }
    let mut fields = Map::new();
    for (field, provider_field) in [
        ("durationSeconds", "duration"),
        ("resolution", "resolution"),
        ("aspectRatio", "ratio"),
        ("generateAudio", "generate_audio"),
        ("returnLastFrame", "return_last_frame"),
        ("outputFormat", "output_format"),
        ("priority", "priority"),
        ("watermark", "watermark"),
        ("safetyIdentifier", "safety_identifier"),
    ] {
        if let Some(value) = input.get(field) {
            fields.insert(String::from(provider_field), value.clone());
        }
    }
    fields.entry("duration").or_insert(json!(-1));
    if operation.adaptive_only {
        fields.entry("ratio").or_insert(json!("adaptive"));
    }
    if let Some(task) = operation.task_type {
        fields.insert(String::from("omni_reference_task_type"), json!(task));
    }
    if !videos.is_empty() {
        fields.insert(String::from(VIDEO_SOURCES), json!(videos));
    }
    if input["webSearch"] == true {
        fields.insert(String::from("tools"), json!([{"type":"web_search"}]));
    }
    Ok(fields)
}

pub(crate) fn take_video_sources(fields: &mut Map<String, Value>) -> Vec<Value> {
    match fields.remove(VIDEO_SOURCES) {
        Some(Value::Array(sources)) => sources,
        _ => Vec::new(),
    }
}
