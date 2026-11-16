use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use serde_json::{json, Value};
use url::Url;

use super::{schema, PLUGIN};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operation {
    DallE3,
    GatewayText,
    GatewayReference,
}

const OPERATIONS: [Operation; 3] = [
    Operation::DallE3,
    Operation::GatewayText,
    Operation::GatewayReference,
];
const MAX_REFERENCES: usize = 9;

impl Operation {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::DallE3 => "dall-e-3-text",
            Self::GatewayText => "gateway-text",
            Self::GatewayReference => "gateway-reference",
        }
    }

    fn model(self) -> &'static str {
        match self {
            Self::DallE3 => "dall-e-3",
            Self::GatewayText | Self::GatewayReference => "configured-gateway-model",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::DallE3 => "DALL·E 3 文生图",
            Self::GatewayText => "网关文生图",
            Self::GatewayReference => "网关参考图生图",
        }
    }

    fn capability(self) -> Value {
        let mut config = json!({"type":"object","additionalProperties":false,
        "required":["endpointBaseUrl","credentialRef"],"properties":{
            "endpointBaseUrl":{"type":"string","minLength":1},
            "credentialRef":{"type":"string","minLength":1}
        }});
        if self != Self::DallE3 {
            config["properties"]["gatewayModelId"] = json!({"type":"string","minLength":1});
            config["required"] = json!(["endpointBaseUrl", "credentialRef", "gatewayModelId"]);
        }
        let mut input: Value = serde_json::from_str(include_str!("../schemas/input.json"))
            .expect("valid image input schema");
        let mut controls = vec![
            json!({"fieldPointer":"/prompt","widget":"multiline","label":"提示词","description":"","group":"","displayRole":"prompt"}),
            json!({"fieldPointer":"/size","widget":"text","label":"输出尺寸","description":"","group":""}),
            json!({"fieldPointer":"/responseFormat","widget":"select","label":"响应格式","description":"","group":""}),
        ];
        let slots = if self == Self::GatewayReference {
            input["properties"]["referenceImages"] = json!({"type":"array","items":{"type":"string","minLength":1},"minItems":1,"maxItems":MAX_REFERENCES});
            input["required"] = json!(["prompt", "referenceImages"]);
            controls.push(json!({"fieldPointer":"/referenceImages","widget":"media","label":"参考图","description":"","group":"","displayRole":"referenceImage"}));
            vec![
                json!({"inputPointer":"/referenceImages","role":"reference","mediaKind":"image","minCount":1,"maxCount":MAX_REFERENCES}),
            ]
        } else {
            Vec::new()
        };
        let configuration = schema("configuration", &config.to_string());
        let input = schema("input", &input.to_string());
        let output = schema("output", include_str!("../schemas/output.json"));
        json!({
            "schemaVersion":2,"localId":self.key(),"capabilityKey":"image.generate","transportSlot":"image","modelIds":[self.model()],
            "configurationSchema":{"id":"configuration","digest":configuration["digest"]},
            "schemas":[configuration,input,output],"operations":[{
                "id":format!("{PLUGIN}/{}",self.key()),"version":1,"inputProfile":"operation-input-v1","label":self.label(),
                "inputSchema":{"id":"input","digest":input["digest"]},"outputSchema":{"id":"output","digest":output["digest"]},
                "resumeSchema":null,"uiSchema":{"controls":controls},"mediaSlots":slots,
                "outputSlots":[{"role":"images","kind":"image","minCount":1,"maxCount":null},
                               {"role":"provider-response","kind":"file","minCount":1,"maxCount":1}],
                "execution":{"mode":"synchronous","streaming":true,"cancellation":"unsupported","resumable":false,"remoteIdempotency":false}
            }]
        })
    }
}

pub(crate) fn capabilities() -> Vec<Value> {
    OPERATIONS.into_iter().map(Operation::capability).collect()
}

fn select(selection: &Value) -> Option<Operation> {
    if selection["capabilityKey"] != "image.generate" || selection["operationVersion"] != 1 {
        return None;
    }
    let id = selection["operationId"]
        .as_str()?
        .strip_prefix(PLUGIN)?
        .strip_prefix('/')?;
    OPERATIONS
        .into_iter()
        .find(|op| op.key() == id && selection["modelId"] == op.model())
}

pub(crate) fn endpoint(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    if path.ends_with("/images/generations") {
        url.set_path(&path.to_string());
    } else {
        if url.query().is_some() {
            return None;
        }
        let base = path
            .strip_suffix("/chat/completions")
            .unwrap_or(path)
            .trim_end_matches('/');
        let suffix = if base.ends_with("/v1") {
            "/images/generations"
        } else {
            "/v1/images/generations"
        };
        url.set_path(&format!("{base}{suffix}"));
    }
    Some(url.into())
}

pub(crate) fn material(
    selection: &Value,
    input: &Value,
    config: &Value,
) -> Result<Value, (&'static str, &'static str)> {
    let op = select(selection).ok_or(("", "图像网关操作或模型身份无效"))?;
    if config["endpointBaseUrl"]
        .as_str()
        .and_then(endpoint)
        .is_none()
    {
        return Err(("", "请在适配器账号中配置有效的网关地址"));
    }
    if config["credentialRef"]
        .as_str()
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err(("", "图像网关需要已绑定的 Bearer API Key"));
    }
    let model = if op == Operation::DallE3 {
        String::from("dall-e-3")
    } else {
        let model: String = config["gatewayModelId"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .filter(|value| !value.is_whitespace())
            .collect();
        if model.is_empty() {
            return Err(("", "请在适配器账号中指定网关模型或推理接入点 ID"));
        }
        model
    };
    let prompt = input["prompt"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or(("/prompt", "请输入提示词"))?;
    let references = input["referenceImages"].as_array().map_or(0, Vec::len);
    let references_valid = if op == Operation::GatewayReference {
        (1..=MAX_REFERENCES).contains(&references)
    } else {
        references == 0
    };
    if !references_valid {
        return Err(("/referenceImages", "参考图数量与所选操作不符"));
    }
    let format = input["responseFormat"].as_str().unwrap_or("url");
    if !matches!(format, "url" | "b64_json") {
        return Err(("/responseFormat", "响应格式须为 URL 或 Base64"));
    }
    let mut body = json!({"model":model,"prompt":prompt,"response_format":format});
    if let Some(size) = input["size"]
        .as_str()
        .map(str::trim)
        .filter(|size| !size.is_empty())
    {
        body["size"] = json!(size);
    }
    Ok(body)
}
