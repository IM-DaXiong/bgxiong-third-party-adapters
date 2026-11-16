use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use serde_json::{json, Value};

use crate::{contracts::schema, PLUGIN};

const CLASSIC_INPUT: &str = include_str!("../../schemas/kling-v3-image-text.input.json");
const CLASSIC_RESUME: &str = include_str!("../../schemas/kling-v3-image-text.resume.json");
const CLASSIC_MODELS: &[&str] = &["kling-v3"];
const OMNI_MODELS: &[&str] = &["kling-image-o1", "kling-v3-omni"];
const MODES: [ImageMode; 4] = [
    ImageMode::ClassicText,
    ImageMode::ClassicReference,
    ImageMode::OmniText,
    ImageMode::OmniReference,
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ImageMode {
    ClassicText,
    ClassicReference,
    OmniText,
    OmniReference,
}

impl ImageMode {
    fn local_id(self) -> &'static str {
        match self {
            Self::ClassicText => "kling-v3-image-text",
            Self::ClassicReference => "kling-v3-image-reference",
            Self::OmniText => "kling-omni-image-text",
            Self::OmniReference => "kling-omni-image-reference",
        }
    }

    pub(super) fn operation(self) -> String {
        format!("{PLUGIN}/{}", self.local_id())
    }

    fn label(self) -> &'static str {
        match self {
            Self::ClassicText => "可灵 V3 文生图",
            Self::ClassicReference => "可灵 V3 图生图",
            Self::OmniText => "可灵 Omni 文生图",
            Self::OmniReference => "可灵 Omni 参考图生图",
        }
    }

    pub(super) fn is_omni(self) -> bool {
        matches!(self, Self::OmniText | Self::OmniReference)
    }

    pub(super) fn path(self) -> &'static str {
        if self.is_omni() {
            "/v1/images/omni-image"
        } else {
            "/v1/images/generations"
        }
    }

    fn models(self) -> &'static [&'static str] {
        if self.is_omni() {
            OMNI_MODELS
        } else {
            CLASSIC_MODELS
        }
    }

    pub(super) fn reference_limit(self) -> usize {
        match self {
            Self::ClassicReference => 1,
            Self::OmniReference => 9,
            Self::ClassicText | Self::OmniText => 0,
        }
    }

    pub(super) fn input_schema(self) -> Value {
        if self == Self::ClassicText {
            return schema("input", CLASSIC_INPUT);
        }
        let mut definition: Value =
            serde_json::from_str(CLASSIC_INPUT).expect("valid classic image schema");
        let properties = definition["properties"]
            .as_object_mut()
            .expect("image schema properties");
        properties.remove("negativePrompt");
        if self.is_omni() {
            let ratio = properties
                .get_mut("aspectRatio")
                .expect("ratio input definition");
            ratio["enum"]
                .as_array_mut()
                .expect("ratio choices")
                .push(json!("auto"));
            ratio["default"] = json!("auto");
        }
        if self.reference_limit() > 0 {
            properties.insert("referenceImages".into(), json!({
                "type":"array","items":{"type":"string","minLength":1},"minItems":1,"maxItems":self.reference_limit()
            }));
            definition["required"] = json!(["prompt", "referenceImages"]);
        }
        schema("input", &definition.to_string())
    }

    fn resume_schema(self) -> Value {
        if self == Self::ClassicText {
            return schema("resume", CLASSIC_RESUME);
        }
        schema("resume", &json!({
            "type":"object","additionalProperties":false,"required":["taskId","modelId","operationId"],
            "properties":{"taskId":{"type":"string","minLength":1},"modelId":{"type":"string","enum":self.models()},
                "operationId":{"type":"string","const":self.operation()}}
        }).to_string())
    }

    pub(super) fn resume_token(self, model: &Value, id: &str) -> Value {
        let mut token = json!({"taskId":id,"modelId":model});
        if self != Self::ClassicText {
            token["operationId"] = json!(self.operation());
        }
        token
    }
}

pub(crate) fn capabilities() -> [Value; 4] {
    MODES.map(contract)
}

pub(super) fn selected_mode(selection: &Value, include_input: bool) -> Option<ImageMode> {
    if selection["capabilityKey"] != "image.generate" || selection["operationVersion"] != 1 {
        return None;
    }
    let model = selection["modelId"].as_str()?;
    MODES.into_iter().find(|mode| {
        selection["operationId"] == mode.operation()
            && mode.models().contains(&model)
            && (!include_input || selection["inputSchemaDigest"] == mode.input_schema()["digest"])
    })
}

fn contract(mode: ImageMode) -> Value {
    let input = mode.input_schema();
    let configuration = schema(
        "configuration",
        include_str!("../../schemas/configuration.json"),
    );
    let output = schema(
        "output",
        include_str!("../../schemas/kling-v3-image-text.output.json"),
    );
    let resume = mode.resume_schema();
    let controls: Vec<_> = [
        ("prompt", "multiline", "提示词", Some("prompt")),
        ("negativePrompt", "multiline", "反向提示词", None),
        ("size", "select", "清晰度", None),
        ("aspectRatio", "select", "画幅", None),
        ("referenceImages", "media", "参考图", Some("referenceImage")),
    ]
    .into_iter()
    .filter(|(field, _, _, _)| match *field {
        "negativePrompt" => mode == ImageMode::ClassicText,
        "referenceImages" => mode.reference_limit() > 0,
        _ => true,
    })
    .map(|(field, widget, label, role)| {
        let mut control = json!({"fieldPointer":format!("/{field}"),"widget":widget,"label":label,"description":"","group":""});
        if let Some(role) = role {
            control["displayRole"] = json!(role);
        }
        control
    })
    .collect();
    let slots = if mode.reference_limit() == 0 {
        json!([])
    } else {
        json!([{
            "inputPointer":"/referenceImages","role":"reference_image","mediaKind":"image","minCount":1,"maxCount":mode.reference_limit()
        }])
    };
    json!({"schemaVersion":2,"localId":mode.local_id(),"capabilityKey":"image.generate","transportSlot":"image","modelIds":mode.models(),
        "configurationSchema":{"id":"configuration","digest":configuration["digest"]},"schemas":[input,configuration,output,resume],
        "operations":[{"id":mode.operation(),"version":1,"label":mode.label(),"inputProfile":"operation-input-v1",
            "inputSchema":{"id":"input","digest":input["digest"]},"outputSchema":{"id":"output","digest":output["digest"]},
            "resumeSchema":{"id":"resume","digest":resume["digest"]},"uiSchema":{"controls":controls},"mediaSlots":slots,
            "outputSlots":[{"role":"images","kind":"image","minCount":1,"maxCount":null},
                           {"role":"provider-response","kind":"file","minCount":1,"maxCount":1}],
            "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported","resumable":true,"remoteIdempotency":false}}]})
}
