use alloc::{format, string::ToString, vec, vec::Vec};
use serde_json::{json, Value};

use crate::{schema, PLUGIN_ID};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Text,
    FirstFrame,
    FirstLastFrame,
    Camera,
    SeedEdit,
    Portrait,
}

#[derive(Clone, Copy)]
pub(crate) struct Operation {
    id: &'static str,
    model: &'static str,
    pub request_key: &'static str,
    label: &'static str,
    pub mode: Mode,
}

struct Slot {
    field: &'static str,
    pointer: &'static str,
    role: &'static str,
    label: &'static str,
}

const FIRST: Slot = Slot {
    field: "firstFrame",
    pointer: "/firstFrame",
    role: "first_frame",
    label: "首帧",
};
const LAST: Slot = Slot {
    field: "lastFrame",
    pointer: "/lastFrame",
    role: "last_frame",
    label: "尾帧",
};
const SOURCE: Slot = Slot {
    field: "source",
    pointer: "/source",
    role: "base_image",
    label: "底图",
};
const MODEL_720: &str = "jimeng_t2v_v30";
const MODEL_1080: &str = "jimeng_t2v_v30_1080p";
const MODEL_PRO: &str = "jimeng_t2v_v30_pro";
const ASPECT_RATIOS: &[&str] = &["16:9", "4:3", "1:1", "3:4", "9:16", "21:9"];
const CAMERA_STRENGTHS: &[&str] = &["weak", "medium", "strong"];

const fn operation_entry(
    id: &'static str,
    model: &'static str,
    request_key: &'static str,
    label: &'static str,
    mode: Mode,
) -> Operation {
    Operation {
        id,
        model,
        request_key,
        label,
        mode,
    }
}

const OPERATIONS: &[Operation] = &[
    operation_entry(
        "video-720-text",
        MODEL_720,
        "jimeng_t2v_v30",
        "即梦 3.0 720P · 文生视频",
        Mode::Text,
    ),
    operation_entry(
        "video-720-first",
        MODEL_720,
        "jimeng_i2v_first_v30",
        "即梦 3.0 720P · 首帧",
        Mode::FirstFrame,
    ),
    operation_entry(
        "video-720-first-last",
        MODEL_720,
        "jimeng_i2v_first_tail_v30",
        "即梦 3.0 720P · 首尾帧",
        Mode::FirstLastFrame,
    ),
    operation_entry(
        "video-720-camera",
        MODEL_720,
        "jimeng_i2v_recamera_v30",
        "即梦 3.0 720P · 运镜",
        Mode::Camera,
    ),
    operation_entry(
        "video-1080-text",
        MODEL_1080,
        "jimeng_t2v_v30_1080p",
        "即梦 3.0 1080P · 文生视频",
        Mode::Text,
    ),
    operation_entry(
        "video-1080-first",
        MODEL_1080,
        "jimeng_i2v_first_v30_1080",
        "即梦 3.0 1080P · 首帧",
        Mode::FirstFrame,
    ),
    operation_entry(
        "video-1080-first-last",
        MODEL_1080,
        "jimeng_i2v_first_tail_v30_1080",
        "即梦 3.0 1080P · 首尾帧",
        Mode::FirstLastFrame,
    ),
    operation_entry(
        "video-pro-text",
        MODEL_PRO,
        "jimeng_ti2v_v30_pro",
        "即梦 3.0 Pro · 文生视频",
        Mode::Text,
    ),
    operation_entry(
        "video-pro-first",
        MODEL_PRO,
        "jimeng_ti2v_v30_pro",
        "即梦 3.0 Pro · 首帧",
        Mode::FirstFrame,
    ),
    operation_entry(
        "seededit",
        "seededit_v3.0",
        "seededit_v3.0",
        "即梦 SeedEdit 3.0",
        Mode::SeedEdit,
    ),
    operation_entry(
        "portrait",
        "i2i_portrait_photo",
        "i2i_portrait_photo",
        "图生图 3.0 · 人像写真",
        Mode::Portrait,
    ),
];

type Diagnostic = (&'static str, &'static str);

fn control(pointer: &str, widget: &str, label: &str, role: Option<&str>) -> Value {
    let mut field =
        json!({"fieldPointer":pointer,"widget":widget,"label":label,"group":"","description":""});
    if let Some(role) = role {
        field["displayRole"] = json!(role);
    }
    field
}

impl Mode {
    fn slots(self) -> &'static [Slot] {
        match self {
            Self::Text => &[],
            Self::FirstFrame | Self::Camera => &[FIRST],
            Self::FirstLastFrame => &[FIRST, LAST],
            Self::SeedEdit | Self::Portrait => &[SOURCE],
        }
    }

    pub(crate) fn output_kind(self) -> &'static str {
        match self {
            Self::SeedEdit | Self::Portrait => "image",
            _ => "video",
        }
    }

    fn input_mode(self) -> &'static str {
        match self {
            Self::Text => "text_to_video",
            Self::FirstFrame => "image_to_video",
            Self::FirstLastFrame => "first_last_frame",
            Self::Camera => "camera_control",
            Self::SeedEdit | Self::Portrait => "image_to_image",
        }
    }

    pub(crate) fn submit_action(self) -> &'static str {
        match self {
            Self::Portrait => "CVSubmitTask",
            _ => "CVSync2AsyncSubmitTask",
        }
    }

    pub(crate) fn query_action(self) -> &'static str {
        match self {
            Self::Portrait => "CVGetResult",
            _ => "CVSync2AsyncGetResult",
        }
    }
}

impl Operation {
    fn key(self) -> &'static str {
        if self.mode.output_kind() == "video" {
            "video.generate"
        } else {
            "image.generate"
        }
    }

    fn prompt_optional(self) -> bool {
        self.mode == Mode::Portrait || (self.mode == Mode::FirstFrame && self.model == MODEL_PRO)
    }

    fn declaration(self) -> Value {
        let mut spec: Value =
            serde_json::from_str(include_str!("../schemas/input.json")).expect("base input schema");
        if self.prompt_optional() {
            spec["required"] = json!([]);
            spec["properties"]["prompt"]["minLength"] = json!(0);
        }
        let mode = self.mode.input_mode();
        spec["properties"]["inputMode"] = json!({"type":"string","const":mode,"default":mode});
        let mut controls = vec![control("/prompt", "multiline", "提示词", Some("prompt"))];
        if self.mode.output_kind() == "video" {
            spec["properties"]["durationSeconds"] =
                json!({"type":"integer","enum":[5,10],"default":5});
            spec["properties"]["seed"] =
                json!({"type":"integer","minimum":-1,"maximum":i32::MAX,"default":-1});
            controls.extend([
                control(
                    "/durationSeconds",
                    "select",
                    "时长",
                    Some("durationSeconds"),
                ),
                control("/seed", "number", "种子", Some("seed")),
            ]);
        }
        if self.mode == Mode::Text {
            spec["properties"]["aspectRatio"] =
                json!({"type":"string","enum":ASPECT_RATIOS,"default":"16:9"});
            controls.push(control(
                "/aspectRatio",
                "select",
                "画幅",
                Some("aspectRatio"),
            ));
        }
        if self.mode == Mode::Camera {
            spec["properties"]["cameraTemplate"] =
                json!({"type":"string","minLength":1,"maxLength":128});
            spec["properties"]["cameraStrength"] =
                json!({"type":"string","enum":CAMERA_STRENGTHS,"default":"medium"});
            spec["required"]
                .as_array_mut()
                .expect("required array")
                .push(json!("cameraTemplate"));
            controls.extend([
                control("/cameraTemplate", "text", "运镜模板", None),
                control("/cameraStrength", "select", "运镜强度", None),
            ]);
        }
        if self.mode == Mode::Portrait {
            for (field, pointer, label) in
                [("width", "/width", "宽度"), ("height", "/height", "高度")]
            {
                spec["properties"][field] =
                    json!({"type":"integer","minimum":512,"maximum":2048,"default":1328});
                controls.push(control(pointer, "number", label, Some(field)));
            }
        }
        let slots = self.mode.slots().iter().map(|slot| {
            spec["properties"][slot.field] = json!({"type":"object","additionalProperties":false,
                "required":["mediaId"],"properties":{"mediaId":{"type":"string","minLength":1}}});
            spec["required"].as_array_mut().expect("required array").push(json!(slot.field));
            controls.push(control(slot.pointer, "media", slot.label, None));
            json!({"inputPointer":slot.pointer,"role":slot.role,"mediaKind":"image","minCount":1,"maxCount":1})
        }).collect::<Vec<_>>();
        let input = schema("input", &spec.to_string());
        let config = schema("config", include_str!("../schemas/configuration.json"));
        let output = schema("output", include_str!("../schemas/output.json"));
        let resume = schema("resume", include_str!("../schemas/resume.json"));
        let kind = self.mode.output_kind();
        json!({"schemaVersion":2,"localId":self.id,"capabilityKey":self.key(),
            "transportSlot":kind,"modelIds":[self.model],"schemas":[config,input,output,resume],
            "configurationSchema":{"id":"config","digest":config["digest"]},
            "operations":[{"id":format!("{PLUGIN_ID}/{}",self.id),"version":1,"label":self.label,
                "inputProfile":"operation-input-v1","inputSchema":{"id":"input","digest":input["digest"]},
                "outputSchema":{"id":"output","digest":output["digest"]},
                "resumeSchema":{"id":"resume","digest":resume["digest"]},
                "uiSchema":{"controls":controls},"mediaSlots":slots,
                "outputSlots":[{"role":kind,"kind":kind,"minCount":1,"maxCount":null},
                    {"role":"provider-response","kind":"file","minCount":1,"maxCount":1}],
                "execution":{"mode":"asynchronous","resumable":true,"streaming":false,
                    "cancellation":"unsupported","remoteIdempotency":false}}]})
    }

    pub(crate) fn image_ids(self, input: &Value) -> Result<Vec<&str>, Diagnostic> {
        self.mode
            .slots()
            .iter()
            .map(|slot| {
                input[slot.field]["mediaId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or((slot.pointer, "此操作要求该角色的受管图片"))
            })
            .collect()
    }

    pub(crate) fn material(self, input: &Value) -> Result<Value, Diagnostic> {
        if input
            .get("inputMode")
            .is_some_and(|mode| mode != self.mode.input_mode())
        {
            return Err(("/inputMode", "输入模式与所选操作不一致"));
        }
        let prompt = input
            .get("prompt")
            .map_or(Some(""), Value::as_str)
            .map(str::trim)
            .filter(|text| {
                (self.prompt_optional() || !text.is_empty()) && text.chars().count() <= 800
            })
            .ok_or(("/prompt", "提示词超出支持范围或未提供"))?;
        let mut body = json!({"req_key":self.request_key,"prompt":prompt});
        if self.mode.output_kind() == "video" {
            let seconds = input
                .get("durationSeconds")
                .map_or(Some(5), Value::as_i64)
                .filter(|s| matches!(s, 5 | 10))
                .ok_or(("/durationSeconds", "即梦视频支持 5 秒或 10 秒"))?;
            let seed = input
                .get("seed")
                .map_or(Some(-1), Value::as_i64)
                .filter(|s| (-1..=i64::from(i32::MAX)).contains(s))
                .ok_or(("/seed", "随机种子超出可用范围"))?;
            body["frames"] = json!(24 * seconds + 1);
            body["seed"] = json!(seed);
        }
        match self.mode {
            Mode::Text => {
                let aspect = input
                    .get("aspectRatio")
                    .map_or(Some("16:9"), Value::as_str)
                    .filter(|s| ASPECT_RATIOS.contains(s))
                    .ok_or(("/aspectRatio", "画幅须使用此操作声明的比例"))?;
                body["aspect_ratio"] = json!(aspect);
            }
            Mode::Camera => {
                let template = input["cameraTemplate"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && s.len() <= 128)
                    .ok_or(("/cameraTemplate", "请提供火山视觉运镜模板 ID"))?;
                let strength = input
                    .get("cameraStrength")
                    .map_or(Some("medium"), Value::as_str)
                    .filter(|s| CAMERA_STRENGTHS.contains(s))
                    .ok_or(("/cameraStrength", "请选择声明内的运镜强度"))?;
                body["template_id"] = json!(template);
                body["camera_strength"] = json!(strength);
            }
            Mode::SeedEdit => {
                body["seed"] = json!(-1);
                body["scale"] = json!(0.5);
            }
            Mode::Portrait => {
                for (field, pointer) in [("width", "/width"), ("height", "/height")] {
                    let size = input
                        .get(field)
                        .map_or(Some(1328), Value::as_u64)
                        .filter(|value| (512..=2048).contains(value))
                        .ok_or((pointer, "人像尺寸须为 512 到 2048 像素"))?;
                    body[field] = json!(size);
                }
                body["gen_mode"] = json!("creative");
                body["gpen"] = json!(0.4);
                body["skin"] = json!(0.3);
            }
            Mode::FirstFrame | Mode::FirstLastFrame => {}
        }
        let ids = self.image_ids(input)?;
        Ok(json!({"body":body,"imageIds":ids}))
    }

    pub(crate) fn validate_metadata(
        self,
        input: &Value,
        metadata: &Value,
    ) -> Result<(), Diagnostic> {
        let ids = self.image_ids(input)?;
        let mut first_dimensions = None;
        for (slot, id) in self.mode.slots().iter().zip(ids) {
            let meta = metadata
                .as_array()
                .into_iter()
                .flatten()
                .find(|meta| meta["mediaId"] == id)
                .ok_or((slot.pointer, "冻结输入缺少对应图片凭据"))?;
            let dims = self.validate_image_metadata(slot.pointer, meta)?;
            if self.mode == Mode::FirstLastFrame {
                if let (Some((w, h)), Some((first_w, first_h))) = (dims, first_dimensions) {
                    if w * first_h != h * first_w {
                        return Err((slot.pointer, "尾帧与首帧画幅须一致"));
                    }
                }
                first_dimensions = dims;
            }
        }
        Ok(())
    }

    pub(crate) fn validate_image_metadata(
        self,
        pointer: &'static str,
        meta: &Value,
    ) -> Result<Option<(u64, u64)>, Diagnostic> {
        if meta["kind"] != "image"
            || !matches!(meta["mime"].as_str(), Some("image/jpeg" | "image/png"))
        {
            return Err((pointer, "该素材须为此操作支持的图片格式"));
        }
        let dims = meta["width"].as_u64().zip(meta["height"].as_u64());
        if let Some((w, h)) = dims {
            let low = w.min(h);
            let high = w.max(h);
            if low == 0
                || high > 4096
                || (self.mode.output_kind() == "video" && low < 320)
                || (self.mode != Mode::Portrait && high > low.saturating_mul(3))
            {
                return Err((pointer, "图片尺寸或画幅超出此操作允许的范围"));
            }
        }
        Ok(dims)
    }
}

pub(crate) fn capabilities() -> Vec<Value> {
    OPERATIONS
        .iter()
        .map(|operation| operation.declaration())
        .collect()
}

pub(crate) fn operation(selection: &Value) -> Option<&'static Operation> {
    (selection["operationVersion"] == 1).then_some(())?;
    OPERATIONS.iter().find(|operation| {
        selection["capabilityKey"] == operation.key()
            && selection["modelId"] == operation.model
            && selection["operationId"] == format!("{PLUGIN_ID}/{}", operation.id)
    })
}
