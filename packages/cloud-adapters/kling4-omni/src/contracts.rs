use crate::PLUGIN;
use alloc::{format, string::String, vec::Vec};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Text,
    ReferenceImages,
}

impl Mode {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::ReferenceImages => "reference-images",
        }
    }

    pub fn input_mode(self) -> &'static str {
        match self {
            Self::Text => "text_to_video",
            Self::ReferenceImages => "multi_image_to_video",
        }
    }

    pub fn model(self) -> String {
        format!("kling-4.0.{}", self.slug())
    }

    pub fn operation(self) -> String {
        format!("{PLUGIN}/{}", self.slug())
    }

    fn input_schema(self) -> &'static str {
        match self {
            Self::Text => include_str!("../schemas/text.input.json"),
            Self::ReferenceImages => include_str!("../schemas/reference-images.input.json"),
        }
    }

    pub fn input_digest(self) -> String {
        digest(self.input_schema().replace("\r\n", "\n").as_bytes())
    }

    fn label(self) -> &'static str {
        match self {
            Self::Text => "Kling 4.0 · 文生视频",
            Self::ReferenceImages => "Kling 4.0 · 多图参考",
        }
    }
}

pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn schema(id: &str, raw: &str) -> Value {
    let normalized = raw.replace("\r\n", "\n");
    json!({"id":id,"digest":digest(normalized.as_bytes()),"schemaJson":normalized})
}

fn contract(mode: Mode) -> Value {
    let input = schema("input", mode.input_schema());
    let output = schema("output", include_str!("../schemas/output.json"));
    let config = schema(
        "configuration",
        include_str!("../schemas/configuration.json"),
    );
    let resume = schema("resume", include_str!("../schemas/resume.json"));
    let slots = if mode == Mode::ReferenceImages {
        json!([{"role":"reference_image","inputPointer":"/referenceImages","mediaKind":"image","minCount":1,"maxCount":10}])
    } else {
        json!([])
    };
    let controls: Vec<_> = [
        ("prompt","multiline","提示词"),
        ("durationSeconds","number","时长"),
        ("aspectRatio","select","画幅"),
        ("resolution","select","清晰度"),
    ].into_iter().map(|(field,widget,label)| json!({
        "fieldPointer":format!("/{field}"),"widget":widget,"label":label,"description":"","group":"视频输入"
    })).collect();
    json!({
        "schemaVersion":2,"localId":mode.slug(),"capabilityKey":"video.generate","transportSlot":"video",
        "modelIds":[mode.model()],"configurationSchema":{"id":"configuration","digest":config["digest"]},
        "schemas":[input,output,config,resume],
        "operations":[{
            "id":mode.operation(),"version":1,"label":mode.label(),
            "inputSchema":{"id":"input","digest":input["digest"]},
            "outputSchema":{"id":"output","digest":output["digest"]},
            "resumeSchema":{"id":"resume","digest":resume["digest"]},
            "uiSchema":{"controls":controls},"mediaSlots":slots,
            "outputSlots":[{"role":"video","kind":"video","minCount":1,"maxCount":1}],
            "execution":{"mode":"asynchronous","streaming":false,"cancellation":"unsupported","resumable":true,"remoteIdempotency":false}
        }]
    })
}

pub fn capabilities() -> Value {
    json!({"schemaVersion":2,"pluginId":PLUGIN,"pluginVersion":env!("CARGO_PKG_VERSION"),
        "capabilityContracts":[contract(Mode::Text),contract(Mode::ReferenceImages)]})
}

pub fn mode(payload: &Value, submit: bool) -> Option<Mode> {
    if payload["schemaVersion"] != 2
        || payload["capabilityKey"] != "video.generate"
        || payload["operationVersion"] != 1
    {
        return None;
    }
    [Mode::Text, Mode::ReferenceImages]
        .into_iter()
        .find(|mode| {
            payload["modelId"] == mode.model()
                && payload["operationId"] == mode.operation()
                && (!submit || payload["inputSchemaDigest"] == mode.input_digest())
        })
}
