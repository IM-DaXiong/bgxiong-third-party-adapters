use alloc::string::{String, ToString};
use bgx_adapter_v2_sdk::Host;
use serde_json::{json, Value};

use crate::{failure, invalid, port, Result, API};

pub(crate) enum Authorization<'a> {
    Bearer(&'a str),
    SignedJwt { reference: &'a str, token: String },
}

pub(crate) fn authorization<'a>(
    host: &mut dyn Host,
    payload: &'a Value,
) -> Result<Authorization<'a>> {
    let config = payload["configuration"]
        .as_object()
        .ok_or_else(|| invalid("可灵 API 配置无效"))?;
    if config.keys().any(|key| {
        !matches!(
            key.as_str(),
            "endpointBaseUrl" | "credentialRef" | "credentialMode"
        )
    }) || config.get("endpointBaseUrl").and_then(Value::as_str) != Some(API)
    {
        return Err(invalid("可灵仅支持已声明的北京 API"));
    }
    let reference = config
        .get("credentialRef")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("请配置可灵凭据"))?;
    match config.get("credentialMode").map(Value::as_str) {
        None | Some(Some("bearer")) => Ok(Authorization::Bearer(reference)),
        Some(Some("jwt-hs256")) => {
            let spec = json!({
                "version":1,
                "kind":"jwt-hs256",
                "claimsJson":"{}",
                "hostTime":{"lifetimeSeconds":1800,"notBeforeLeewaySeconds":5}
            });
            let token = port(
                host,
                "host-credential.sign",
                json!({"handle":reference,"specJson":spec.to_string()}),
            )?;
            let token = token
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| failure("RESOURCE_UNAVAILABLE", "SDK 未完成可灵 JWT 签发"))?;
            Ok(Authorization::SignedJwt {
                reference,
                token: token.to_string(),
            })
        }
        _ => Err(invalid("可灵凭据模式无效")),
    }
}
