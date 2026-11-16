use alloc::{format, string::String, vec::Vec};
use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::{host, Host};
use serde_json::{json, Value};

use super::super::{media_bytes, Failure};

const IMAGE_LIMIT: usize = 20 * 1024 * 1024;
const IMAGE_TOTAL_LIMIT: usize = 42 * 1024 * 1024;
const VIDEO_SOURCES: &str = "/referenceVideoSources";
const AUDIO_SOURCES: &str = "/referenceAudioSources";
const DOCUMENT_SOURCE: &str = "/referenceDocumentSource";
const PAGE_SOURCE: &str = "/referencePageSource";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Operation {
    pub local_id: &'static str,
    pub id: &'static str,
    pub mode: &'static str,
    pub label: &'static str,
    slots: &'static [ImageSlot],
    sources: SourcePolicy,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourcePolicy {
    None,
    Reference,
    VideoEdit,
    VideoExtend,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ImageSlot {
    field: &'static str,
    role: &'static str,
    label: &'static str,
    min: usize,
    max: usize,
}

const FIRST: ImageSlot = ImageSlot {
    field: "/firstFrame",
    role: "first_frame",
    label: "首帧",
    min: 1,
    max: 1,
};
const LAST: ImageSlot = ImageSlot {
    field: "/lastFrame",
    role: "last_frame",
    label: "尾帧",
    min: 1,
    max: 1,
};
const REFERENCE_IMAGES: ImageSlot = ImageSlot {
    field: "/referenceImages",
    role: "reference_image",
    label: "参考图片",
    min: 0,
    max: 10,
};

pub(super) const OPERATIONS: &[Operation] = &[
    Operation {
        local_id: "wan3-text",
        id: "local.bgx.dashscope-media/wan3-text",
        mode: "text_to_video",
        label: "万相 3.0 文生视频",
        slots: &[],
        sources: SourcePolicy::None,
    },
    Operation {
        local_id: "wan3-first-frame",
        id: "local.bgx.dashscope-media/wan3-first-frame",
        mode: "image_to_video",
        label: "万相 3.0 首帧",
        slots: &[FIRST],
        sources: SourcePolicy::None,
    },
    Operation {
        local_id: "wan3-first-last-frame",
        id: "local.bgx.dashscope-media/wan3-first-last-frame",
        mode: "first_last_frame",
        label: "万相 3.0 首尾帧",
        slots: &[FIRST, LAST],
        sources: SourcePolicy::None,
    },
    Operation {
        local_id: "wan3-reference",
        id: "local.bgx.dashscope-media/wan3-reference",
        mode: "reference_to_video",
        label: "万相 3.0 多模态参考",
        slots: &[REFERENCE_IMAGES],
        sources: SourcePolicy::Reference,
    },
    Operation {
        local_id: "wan3-edit",
        id: "local.bgx.dashscope-media/wan3-edit",
        mode: "video_edit",
        label: "万相 3.0 视频编辑",
        slots: &[],
        sources: SourcePolicy::VideoEdit,
    },
    Operation {
        local_id: "wan3-extend",
        id: "local.bgx.dashscope-media/wan3-extend",
        mode: "video_extend",
        label: "万相 3.0 视频延长",
        slots: &[],
        sources: SourcePolicy::VideoExtend,
    },
];

impl Operation {
    pub(super) fn requires_prompt(self) -> bool {
        self.local_id == "wan3-text"
            || matches!(
                self.sources,
                SourcePolicy::VideoEdit | SourcePolicy::VideoExtend
            )
    }

    pub(super) fn adaptive_only(self) -> bool {
        self.sources == SourcePolicy::VideoExtend
    }

    pub(super) fn description(self, schema: &mut Value, controls: &mut Vec<Value>) -> Vec<Value> {
        let mut slots = Vec::new();
        for slot in self.slots {
            let name = &slot.field[1..];
            schema["properties"][name] = json!({"type":"array","minItems":slot.min,"maxItems":slot.max,
                "items":{"type":"string","minLength":1}});
            if slot.min > 0 {
                require(schema, name);
            }
            controls.push(control(slot.field, "media", slot.label, None));
            slots.push(json!({"inputPointer":slot.field,"role":slot.role,"mediaKind":"image","minCount":slot.min,"maxCount":slot.max}));
        }
        if self.sources == SourcePolicy::None {
            return slots;
        }
        add_source(schema, controls, VIDEO_SOURCES, "参考视频地址", "multiline");
        if self.sources != SourcePolicy::Reference {
            require(schema, &VIDEO_SOURCES[1..]);
            return slots;
        }
        add_source(schema, controls, AUDIO_SOURCES, "参考音频地址", "multiline");
        add_source(schema, controls, DOCUMENT_SOURCE, "参考文件地址", "text");
        add_source(schema, controls, PAGE_SOURCE, "参考网页地址", "text");
        slots
    }

    pub(super) fn material(
        self,
        input: &Value,
    ) -> Result<Vec<Value>, (&'static str, &'static str)> {
        let mut media = Vec::new();
        for slot in self.slots {
            let values = input
                .pointer(slot.field)
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if !(slot.min..=slot.max).contains(&values.len()) {
                return Err((slot.field, "素材数量与所选万相模式不符"));
            }
            for id in values {
                let id = id
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or((slot.field, "素材引用无效"))?;
                media.push(json!({"type":slot.role,"mediaId":id}));
            }
        }
        match self.sources {
            SourcePolicy::None => {}
            SourcePolicy::Reference => {
                push_sources(
                    &mut media,
                    input,
                    VIDEO_SOURCES,
                    "reference_video",
                    0,
                    5,
                    true,
                )?;
                push_sources(
                    &mut media,
                    input,
                    AUDIO_SOURCES,
                    "reference_audio",
                    0,
                    5,
                    true,
                )?;
                let files = push_sources(&mut media, input, DOCUMENT_SOURCE, "file", 0, 1, true)?;
                let links = push_sources(&mut media, input, PAGE_SOURCE, "link", 0, 1, false)?;
                if files + links > 1 {
                    return Err((PAGE_SOURCE, "参考文件和参考网页只能选择一项"));
                }
                if files + links > 0 && input["promptExtend"] == false {
                    return Err(("/promptExtend", "参考文件或网页需要开启改写提示词"));
                }
                if media.is_empty() {
                    return Err(("/referenceImages", "至少提供一项参考素材"));
                }
            }
            SourcePolicy::VideoEdit | SourcePolicy::VideoExtend => {
                push_sources(
                    &mut media,
                    input,
                    VIDEO_SOURCES,
                    "reference_video",
                    1,
                    1,
                    true,
                )?;
            }
        }
        Ok(media)
    }
}

pub(super) fn control(pointer: &str, widget: &str, label: &str, role: Option<&str>) -> Value {
    let mut item =
        json!({"fieldPointer":pointer,"widget":widget,"label":label,"group":"","description":""});
    if let Some(role) = role {
        item["displayRole"] = json!(role);
    }
    item
}

fn require(schema: &mut Value, name: &str) {
    schema["required"]
        .as_array_mut()
        .expect("required array")
        .push(json!(name));
}

fn add_source(
    schema: &mut Value,
    controls: &mut Vec<Value>,
    pointer: &str,
    label: &str,
    widget: &str,
) {
    schema["properties"][&pointer[1..]] = json!({"type":"string","maxLength":12000});
    controls.push(control(pointer, widget, label, None));
}

fn push_sources(
    media: &mut Vec<Value>,
    input: &Value,
    pointer: &'static str,
    role: &'static str,
    min: usize,
    max: usize,
    allow_oss: bool,
) -> Result<usize, (&'static str, &'static str)> {
    let urls: Vec<_> = input
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if !(min..=max).contains(&urls.len()) {
        return Err((pointer, "素材地址数量与所选模式不符"));
    }
    for url in &urls {
        if !approved_source(url, allow_oss) {
            return Err((pointer, "请输入 HTTPS 公网地址或厂商临时 OSS 地址"));
        }
        media.push(json!({"type":role,"url":url}));
    }
    Ok(urls.len())
}

fn approved_source(url: &str, allow_oss: bool) -> bool {
    if url
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '\\' | '#'))
    {
        return false;
    }
    if allow_oss {
        if let Some(key) = url.strip_prefix("oss://dashscope-instant/") {
            return !key.is_empty()
                && !key.contains('?')
                && key
                    .split('/')
                    .all(|part| !part.is_empty() && !matches!(part, "." | ".."));
        }
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    !authority.starts_with('.')
        && !authority.ends_with('.')
        && authority.contains('.')
        && authority
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
}

pub(super) fn convert(
    host: &mut dyn Host,
    request: &mut Value,
    media: &Value,
) -> Result<bool, Failure> {
    let Some(items) = request
        .pointer_mut("/input/media")
        .and_then(Value::as_array_mut)
    else {
        return Ok(false);
    };
    let mut oss = false;
    let mut image_bytes = 0;
    for item in items {
        if let Some(id) = item["mediaId"].as_str() {
            let handle = media
                .as_array()
                .into_iter()
                .flatten()
                .find(|value| value["id"] == id)
                .filter(|value| value["kind"] == "image")
                .and_then(|value| value["handle"].as_str())
                .ok_or_else(|| Failure::new("INVALID_INPUT", "万相图片素材租约缺失"))?;
            let url = image_data_url(host, handle, &mut image_bytes)?;
            *item = json!({"type":item["type"],"url":url});
        }
        oss |= item["url"]
            .as_str()
            .is_some_and(|url| url.starts_with("oss://"));
    }
    Ok(oss)
}

fn image_data_url(host: &mut dyn Host, handle: &str, total: &mut usize) -> Result<String, Failure> {
    let port_error = |_| Failure::new("INVALID_INPUT", "无法读取万相图片素材租约");
    let stat: Value =
        host::call(host, "host-media.info", &json!({"handle":handle})).map_err(port_error)?;
    let mime = stat["mime"]
        .as_str()
        .filter(|s| matches!(*s, "image/jpeg" | "image/png" | "image/bmp" | "image/webp"))
        .ok_or_else(|| Failure::new("INVALID_INPUT", "万相图片需使用 JPEG、PNG、BMP 或 WebP"))?;
    if stat["kind"] != "image" {
        return Err(Failure::new("INVALID_INPUT", "万相图片素材类型不匹配"));
    }
    let length = stat["byteLength"]
        .as_str()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|length| (1..=IMAGE_LIMIT).contains(length))
        .ok_or_else(|| Failure::new("INVALID_INPUT", "万相图片超过单张 20 MB 或内容为空"))?;
    *total += length;
    if *total > IMAGE_TOTAL_LIMIT {
        return Err(Failure::new(
            "INVALID_INPUT",
            "万相图片超过内嵌请求大小限制，请减少本次参考图片大小",
        ));
    }
    let bytes = media_bytes(host, handle, length)?;
    if bytes.len() != length {
        return Err(Failure::new(
            "INVALID_INPUT",
            "万相图片读取长度和租约不一致",
        ));
    }
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bgx_adapter_v2_sdk::{host::HostError, HostPortCallV2};

    struct OwnedImages;

    impl Host for OwnedImages {
        fn call(&mut self, call: HostPortCallV2) -> Result<Value, HostError> {
            let call = serde_json::to_value(call).unwrap();
            let handle = call["args"]["handle"].as_str().unwrap();
            assert!(matches!(handle, "first-handle" | "last-handle"));
            match call["method"].as_str().unwrap() {
                "host-media.info" => {
                    Ok(json!({"kind":"image","mime":"image/jpeg","byteLength":"3"}))
                }
                "host-media.read" => Ok(if handle == "first-handle" {
                    json!([3, 2, 1])
                } else {
                    json!([4, 5, 6])
                }),
                _ => panic!("unexpected port"),
            }
        }
    }

    #[test]
    fn keyframes_follow_roles_and_keep_their_distinct_original_bytes() {
        let keyframes = OPERATIONS
            .iter()
            .find(|op| op.mode == "first_last_frame")
            .unwrap();
        let frozen = keyframes
            .material(&json!({"firstFrame":["first"],"lastFrame":["last"]}))
            .unwrap();
        let mut request = json!({"input":{"media":frozen}});
        let leases = json!([
            {"id":"last","kind":"image","handle":"last-handle"},
            {"id":"first","kind":"image","handle":"first-handle"}
        ]);
        assert!(!convert(&mut OwnedImages, &mut request, &leases)
            .unwrap_or_else(|error| panic!("{}", error.message)));
        assert_eq!(
            request["input"]["media"],
            json!([
                {"type":"first_frame","url":"data:image/jpeg;base64,AwIB"},
                {"type":"last_frame","url":"data:image/jpeg;base64,BAUG"}
            ])
        );
        let mut text_request = json!({"input":{"prompt":"isolated text request"}});
        assert!(!convert(&mut OwnedImages, &mut text_request, &json!([])).unwrap());
        assert!(text_request["input"].get("media").is_none());
    }

    #[test]
    fn reference_inputs_keep_order_and_reject_unsupported_sources() {
        let reference = OPERATIONS
            .iter()
            .find(|op| op.mode == "reference_to_video")
            .unwrap();
        let input = json!({
            "referenceImages":["second","first"],
            "referenceVideoSources":"https://assets.example/video.mp4\noss://dashscope-instant/copy.mp4",
            "referenceAudioSources":"https://assets.example/audio.wav"
        });
        let frozen = reference.material(&input).unwrap();
        assert_eq!(
            frozen,
            vec![
                json!({"type":"reference_image","mediaId":"second"}),
                json!({"type":"reference_image","mediaId":"first"}),
                json!({"type":"reference_video","url":"https://assets.example/video.mp4"}),
                json!({"type":"reference_video","url":"oss://dashscope-instant/copy.mp4"}),
                json!({"type":"reference_audio","url":"https://assets.example/audio.wav"}),
            ]
        );
        let embedded = json!({"referenceVideoSources":"data:video/mp4;base64,ABC"});
        assert_eq!(reference.material(&embedded).unwrap_err().0, VIDEO_SOURCES);
        let conflicting = json!({
            "referenceDocumentSource":"https://assets.example/doc.pdf",
            "referencePageSource":"https://assets.example/web"
        });
        assert_eq!(reference.material(&conflicting).unwrap_err().0, PAGE_SOURCE);
    }
}
