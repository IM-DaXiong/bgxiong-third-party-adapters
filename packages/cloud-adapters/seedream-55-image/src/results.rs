use alloc::{
    borrow::ToOwned,
    string::{String, ToString},
};

use base64::{engine::general_purpose::STANDARD, Engine};
use bgx_adapter_v2_sdk::{Host, Invocation};
use serde_json::{json, Map, Value};
use zune_core::{bytestream::ZCursor, options::DecoderOptions, result::DecodingResult};
use zune_jpeg::JpegDecoder;
use zune_png::PngDecoder;

use crate::{
    append_media, create_media, fail, finish_media, http, malformed, media_len, port, read_media,
    text, Failure, Parameters, MODEL, PLUGIN,
};

const RESULT_HOSTS: &[&str] = &[
    "ark-content-generation-v2-cn-beijing.tos-cn-beijing.volces.com",
    "ark-acg-cn-beijing.tos-cn-beijing.volces.com",
];
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 48 * 1024 * 1024;

fn media_output(
    host: &mut dyn Host,
    handle: &str,
    kind: &str,
    role: &str,
) -> Result<Value, Failure> {
    let info = port(host, "host-media.info", json!({"handle":handle}))?;
    if info["kind"] != kind || media_len(&info, usize::MAX)? == 0 {
        return Err(malformed());
    }
    Ok(json!({"kind":"media","role":role,"media":{
        "id":info["id"],"kind":kind,"handle":handle,"mime":info["mime"],
        "fileName":info["fileName"],"byteLength":info["byteLength"],
    }}))
}

struct Events<'a> {
    request: &'a Invocation,
    sequence: u64,
}

impl Events<'_> {
    fn emit(&mut self, host: &mut dyn Host, kind: &str, payload: Value) -> Result<(), Failure> {
        port(
            host,
            "host-stream.emit-event",
            json!({
                "event":{"requestId":self.request.r#request_id,"sequence":self.sequence + 1,"type":kind,"payload":payload}
            }),
        )?;
        self.sequence += 1;
        Ok(())
    }

    fn media(&mut self, host: &mut dyn Host, output: &Value) -> Result<(), Failure> {
        self.emit(
            host,
            "media_chunk",
            json!({"role":output["role"],"media":output["media"]}),
        )
    }
}

fn reported_usage(response: &Value) -> Result<Value, Failure> {
    let Some(usage) = response.get("usage").filter(|usage| !usage.is_null()) else {
        return Ok(Value::Null);
    };
    if !usage.is_object() {
        return Err(malformed());
    }
    let mut values = vec![];
    for key in [
        "input_tokens",
        "output_tokens",
        "total_tokens",
        "image_count",
        "generated_images",
    ] {
        if let Some(value) = usage.get(key) {
            let value = value.as_u64().ok_or_else(malformed)?;
            let (metric, unit) = if key.ends_with("_tokens") {
                (key.to_owned(), "tokens".to_owned())
            } else {
                (format!("{PLUGIN}/{key}"), format!("{PLUGIN}/images"))
            };
            values.push(
                json!({"metric":metric,"unit":unit,"amount":value.to_string(),"source":"reported"}),
            );
        }
    }
    if values.is_empty() {
        return Ok(Value::Null);
    }
    Ok(json!({"measurements":values}))
}

fn approved_url(raw: &str) -> Result<&str, Failure> {
    // The broker parses and checks the same URL again before opening the connection.
    if raw
        .strip_prefix("https://")
        .and_then(|tail| tail.split_once('/'))
        .is_some_and(|(host, _)| RESULT_HOSTS.contains(&host))
        && !raw.contains(['#', '\\'])
        && !raw
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
    {
        Ok(raw)
    } else {
        Err(fail(
            "PERMISSION_DENIED",
            "方舟返回的图片地址不在已授权的结果源",
        ))
    }
}

fn check_pixels(size: Option<(usize, usize)>) -> Result<(), Failure> {
    let (width, height) = size.ok_or_else(malformed)?;
    if width == 0 || height == 0 || width.saturating_mul(height) > 4_624_220 {
        return Err(fail("RESOURCE_UNAVAILABLE", "输出图片像素超出解码预算"));
    }
    Ok(())
}

fn validate_image(bytes: &[u8], params: &Parameters<'_>) -> Result<&'static str, Failure> {
    let options = DecoderOptions::new_safe()
        .set_strict_mode(true)
        .set_max_width(9_000)
        .set_max_height(9_000);
    if params.output_format == "jpeg" && bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        let mut reader = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
        reader.decode_headers().map_err(|_| malformed())?;
        check_pixels(reader.dimensions())?;
        reader.decode().map_err(|_| malformed())?;
        return Ok("image/jpeg");
    }
    const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";
    const PNG_END: &[u8] = b"\0\0\0\0IEND\xaeB\x60\x82";
    if params.output_format != "png" || !bytes.starts_with(PNG_HEADER) || !bytes.ends_with(PNG_END)
    {
        return Err(fail("UPSTREAM_ERROR", "图片实际格式与所请求的格式不一致"));
    }
    let mut reader = PngDecoder::new_with_options(ZCursor::new(bytes), options);
    reader.decode_headers().map_err(|_| malformed())?;
    check_pixels(reader.dimensions())?;
    let pixels = reader.decode().map_err(|_| malformed())?;
    let color = reader.colorspace().ok_or_else(malformed)?;
    let transparent = color.has_alpha()
        && match pixels {
            DecodingResult::U8(bytes) => bytes
                .chunks_exact(color.num_components())
                .any(|p| p.last() != Some(&u8::MAX)),
            DecodingResult::U16(words) => words
                .chunks_exact(color.num_components())
                .any(|p| p.last() != Some(&u16::MAX)),
            _ => return Err(malformed()),
        };
    if params.background != "auto" && (params.background == "transparent") != transparent {
        return Err(fail("UPSTREAM_ERROR", "PNG 原件的透明像素与请求背景不符"));
    }
    Ok("image/png")
}

fn artifact(
    host: &mut dyn Host,
    request: &Invocation,
    params: &Parameters<'_>,
    item: &Value,
) -> Result<Value, Failure> {
    if params.response_format == "url" {
        let url = approved_url(text(item, "url")?)?;
        let response = http(
            host,
            "GET",
            url,
            json!({"kind":"none"}),
            request.r#budget_ms,
        )?;
        if response["status"].as_u64() != Some(200) {
            return Err(fail("UPSTREAM_ERROR", "图片下载失败，原始响应已保留"));
        }
        let handle = text(&response["payload"], "handle")?;
        let info = port(host, "host-media.info", json!({"handle":handle}))?;
        let length = media_len(&info, MAX_IMAGE_BYTES)?;
        let mime = validate_image(&read_media(host, handle, length)?, params)?;
        if info["mime"] != mime {
            return Err(malformed());
        }
        media_output(host, handle, "image", "artifact")
    } else {
        let encoded = text(item, "b64_json")?;
        if encoded.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4 {
            return Err(fail("RESOURCE_UNAVAILABLE", "图片返回字节超出限制"));
        }
        let bytes = STANDARD.decode(encoded).map_err(|_| malformed())?;
        let mime = validate_image(&bytes, params)?;
        let handle = create_media(host, "image", mime)?;
        append_media(host, &handle, &bytes)?;
        finish_media(host, &handle)?;
        media_output(host, &handle, "image", "artifact")
    }
}

fn copy_identifier(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| {
            !text.is_empty()
                && text.len() <= 128
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
        .map(str::to_owned)
}

fn business_failure(error: &Value) -> Failure {
    let mut failure = fail("UPSTREAM_ERROR", "方舟返回生图错误，已保留收到的成果");
    failure.vendor = copy_identifier(&error["code"]);
    failure
}

pub(crate) fn collect(
    host: &mut dyn Host,
    request: &Invocation,
    params: &Parameters<'_>,
    input: &Value,
    response: &Value,
) -> Result<Value, Failure> {
    let raw = text(&response["payload"], "handle")?;
    let raw_output = media_output(host, raw, "file", "provider-response")?;
    let mut events = Events {
        request,
        sequence: 0,
    };
    events.media(host, &raw_output)?;

    let length = media_len(&raw_output["media"], MAX_RESPONSE_BYTES)?;
    let status = response["status"].as_u64().ok_or_else(malformed)?;
    let received = read_media(host, raw, length)?;
    let parsed = serde_json::from_slice::<Value>(&received);
    drop(received);
    let request_id = parsed
        .as_ref()
        .ok()
        .and_then(|response| copy_identifier(&response["request_id"]))
        .or_else(|| {
            response["headers"]
                .as_array()?
                .iter()
                .find(|header| {
                    header["name"].as_str().is_some_and(|name| {
                        name.eq_ignore_ascii_case("x-request-id")
                            || name.eq_ignore_ascii_case("x-tt-logid")
                    })
                })
                .and_then(|header| copy_identifier(&header["value"]))
        });

    let result = (|| {
        if !(200..300).contains(&status) {
            let mut error = fail(
                match status {
                    401 | 403 => "UNAUTHORIZED",
                    429 => "RATE_LIMITED",
                    _ => "UPSTREAM_ERROR",
                },
                "方舟拒绝了图片生成请求",
            );
            error.vendor = parsed
                .ok()
                .and_then(|body| copy_identifier(&body["error"]["code"]));
            return Err(error);
        }
        let response = parsed.map_err(|_| malformed())?;
        let mut failure = response
            .get("error")
            .filter(|e| !e.is_null())
            .map(business_failure);
        let usage = match reported_usage(&response) {
            Ok(usage) => usage,
            Err(error) => {
                failure.get_or_insert(error);
                Value::Null
            }
        };
        if !usage.is_null() {
            events.emit(host, "usage", usage.clone())?;
        }

        let mut outputs = vec![];
        if let Some(items) = response["data"].as_array() {
            for item in items {
                let output = if let Some(error) = item.get("error").filter(|e| !e.is_null()) {
                    Err(business_failure(error))
                } else {
                    artifact(host, request, params, item)
                };
                match output {
                    Ok(output) => {
                        events.media(host, &output)?;
                        outputs.push(output);
                    }
                    Err(error) => {
                        failure.get_or_insert(error);
                    }
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        let count = outputs.len();
        if count == 0 {
            return Err(malformed());
        }
        outputs.push(raw_output);
        let applied: Map<String, Value> = [
            "size",
            "aspectRatio",
            "resolution",
            "watermark",
            "background",
            "responseFormat",
            "outputFormat",
        ]
        .into_iter()
        .filter(|key| *key != "size" || input["resolution"] == "custom")
        .filter_map(|key| Some((key.to_owned(), input.get(key)?.clone())))
        .collect();
        Ok(json!({
            "kind":"completed","vendorTaskId":null,"appliedParams":applied,"outputs":outputs,"usage":usage,
            "data":{"model":MODEL,"providerRequestId":request_id,"imageCount":count,
                "reportedUsageJson":response.get("usage").filter(|value| !value.is_null()).map(Value::to_string)},
        }))
    })();
    result.map_err(|mut error: Failure| {
        error.request = request_id;
        error
    })
}
