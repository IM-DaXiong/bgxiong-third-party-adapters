//! Canvas/UI `widgets_values` -> API named inputs (widget order SSOT).
//!
//! Widget order is node knowledge. Every layout below is audited against the node's
//! `INPUT_TYPES` (comfy-core or a reviewed custom-node package) and the certified workflows in
//! `ComfyUI-API/`. Unknown class types carrying positional widgets are rejected with
//! `NODE_DEFINITION_INCOMPLETE`; never derive an order from object_info, socket names, titles or
//! position. Existing node links `[id, slot]` are never overwritten.
//!
//! LEGACY (pre-CW28): this unversioned table serves only the host's legacy `compiler.rs` /
//! `normalizer.rs` path. It is frozen: versioned special layouts live in `special.rs` and are
//! selected through `special::special_rule_for`; the standard path is `canvas::widgets::bind_widgets`.
//! Once CW28 removes the legacy host callers, this table and its merge/overlay helpers are deleted.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};

use serde_json::{Map, Value};

use crate::error::{MappingError, MappingErrorCode};

/// One positional entry of a canvas node's `widgets_values` array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetSlot {
    /// Widget backed API input. `linked_consumes`: the canvas export still keeps this widget's
    /// value slot when the socket is linked (modern frontend widget sockets).
    Input {
        name: &'static str,
        linked_consumes: bool,
    },
    /// UI-only widget value (e.g. `control_after_generate`); occupies a slot, never an input.
    UiOnly,
}

/// Audited serialization shape of a node's `widgets_values`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetLayout {
    /// Ordered slots; trailing extra values are ignored, missing values leave inputs unset.
    Ordered(&'static [WidgetSlot]),
    /// Known node whose widget values never become API inputs (annotations, previews,
    /// subgraph widget proxies, nodes without widget inputs).
    NoApiInputs,
    /// `TextGenerate` DynamicCombo: `sampling_mode.*` dotted keys.
    TextGenerateDynamicCombo,
    /// Audited `COMFY_DYNAMICCOMBO_V3` node: head widgets, the combo key, then the selected
    /// option's child widgets as `<combo>.<child>` API keys.
    DynamicCombo(&'static DynamicComboLayout),
}

/// One audited DynamicCombo option: its key and child widgets in serialization order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComboOption {
    pub key: &'static str,
    pub children: &'static [&'static str],
}

/// Audited DynamicCombo serialization. Option keys outside `options` are rejected (the child
/// widget count depends on the option, so an unknown key cannot be decoded without guessing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicComboLayout {
    pub head: &'static [WidgetSlot],
    pub combo: &'static str,
    pub options: &'static [ComboOption],
}

// Macros (not const fn) so slot slices stay promotable to `'static`.
macro_rules! w {
    ($name:literal) => {
        WidgetSlot::Input {
            name: $name,
            linked_consumes: false,
        }
    };
}

macro_rules! wl {
    ($name:literal) => {
        WidgetSlot::Input {
            name: $name,
            linked_consumes: true,
        }
    };
}

const UI: WidgetSlot = WidgetSlot::UiOnly;

const EMPTY_LATENT_FAMILY: &[WidgetSlot] = &[wl!("width"), wl!("height"), wl!("batch_size")];

/// comfy-core `SaveImageAdvanced` (ComfyUI 0.38 `comfy_extras.nodes_images`): `format` is a
/// DynamicCombo; audited against INPUT_TYPES and the server-saved Qwen Image 2.1 template.
static SAVE_IMAGE_ADVANCED: DynamicComboLayout = DynamicComboLayout {
    head: &[wl!("filename_prefix")],
    combo: "format",
    options: &[
        ComboOption {
            key: "png",
            children: &["bit_depth", "input_color_space"],
        },
        ComboOption {
            key: "exr",
            children: &["bit_depth", "input_color_space"],
        },
        ComboOption {
            key: "avif",
            children: &["bit_depth", "input_color_space", "crf", "save_mode"],
        },
    ],
};

/// ComfyUI 0.13+ subgraph instance / workflow widget proxy (`class_type` is a 36-char UUID).
fn is_subgraph_proxy_class_type(class_type: &str) -> bool {
    let ct = class_type.trim();
    ct.len() == 36
        && ct.chars().filter(|c| *c == '-').count() == 4
        && ct.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Audited widget layout for `class_type`; `None` means unknown and must be rejected by callers
/// that hold positional `widgets_values`.
pub fn widget_layout(class_type: &str) -> Option<WidgetLayout> {
    use WidgetLayout::{DynamicCombo, NoApiInputs, Ordered, TextGenerateDynamicCombo};
    if class_type.starts_with("Empty") && class_type.ends_with("LatentImage") {
        return Some(Ordered(EMPTY_LATENT_FAMILY));
    }
    if is_subgraph_proxy_class_type(class_type) {
        return Some(NoApiInputs);
    }
    Some(match class_type {
        "ImageScaleToTotalPixels" => Ordered(&[w!("upscale_method"), w!("megapixels")]),
        "KSamplerSelect" => Ordered(&[w!("sampler_name")]),
        "BasicScheduler" => Ordered(&[w!("scheduler"), w!("steps"), w!("denoise")]),
        "DualCFGGuider" => Ordered(&[w!("cfg"), w!("cfg2_negative"), w!("style")]),
        "FluxGuidance" => Ordered(&[w!("guidance")]),
        "VAELoader" => Ordered(&[w!("vae_name")]),
        "UNETLoader" => Ordered(&[w!("unet_name"), w!("weight_dtype")]),
        "CLIPLoader" => Ordered(&[w!("clip_name"), w!("type"), w!("device")]),
        "CheckpointLoaderSimple" => Ordered(&[w!("ckpt_name")]),
        // Comfy core UpscaleModelLoader: one required combo (utility-gan_upscaler).
        "UpscaleModelLoader" => Ordered(&[w!("model_name")]),
        "LoraLoader" => Ordered(&[w!("lora_name"), w!("strength_model"), w!("strength_clip")]),
        "LoraLoaderModelOnly" => Ordered(&[w!("lora_name"), w!("strength_model")]),
        "CLIPTextEncode" => Ordered(&[w!("text")]),
        "PrimitiveStringMultiline" => Ordered(&[w!("value")]),
        "ResolutionSelector" => Ordered(&[w!("aspect_ratio"), w!("megapixels"), w!("multiple")]),
        "MiniMaxH3ImageToVideo" | "MiniMaxH3ReferenceToVideo" => {
            Ordered(&[wl!("prompt"), wl!("width"), wl!("height"), wl!("length")])
        }
        "MiniMaxH3TurboLoRA" => Ordered(&[w!("lora_name"), w!("strength"), w!("low_vram")]),
        // ApplyVDNH3: model is a link; widgets = required except model (INV-H3-VDN-02).
        "ApplyVDNH3" => Ordered(&[
            w!("vdn_checkpoint"),
            w!("apply_turbo_adapter"),
            w!("strength"),
            w!("lora_mode"),
            w!("branch_weights"),
            w!("retain_buffers"),
            w!("verbose"),
            w!("attention_backend"),
        ]),
        // SpectrumApplyMiniMaxH3: model is a link; widgets = required (except model) + optional.
        "SpectrumApplyMiniMaxH3" => Ordered(&[
            w!("enabled"),
            w!("blend_weight"),
            w!("degree"),
            w!("ridge_lambda"),
            w!("window_size"),
            w!("flex_window"),
            w!("warmup_steps"),
            w!("tail_actual_steps"),
            w!("max_history"),
            w!("debug"),
            w!("history_storage"),
            w!("bootstrap_first_forecast"),
            w!("anchor_residual_feedback"),
            w!("selective_rollback_correction"),
            w!("offline_smoothing_replay"),
            w!("audio_blend_weight"),
            w!("offline_archive_storage"),
            w!("model_aware_mode"),
            w!("model_aware_risk_threshold"),
            w!("model_aware_trust_shrinkage"),
            w!("model_aware_replay_generic_correction"),
            w!("generic_correction_mode"),
            w!("generic_correction_limiter"),
            w!("generic_correction_limit"),
            w!("generic_correction_attenuation"),
        ]),
        "Ideogram4Scheduler" => {
            Ordered(&[w!("steps"), w!("width"), w!("height"), w!("mu"), w!("std")])
        }
        "DualModelGuider" => Ordered(&[w!("cfg")]),
        // Official Ideogram 4 template (comfy_extras nodes_string / CFGOverride / math).
        "JsonExtractString" => Ordered(&[wl!("json_string"), wl!("key")]),
        "StringReplace" => Ordered(&[wl!("string"), wl!("find"), wl!("replace")]),
        // Krea 2 / comfy_extras nodes_string: linked a/b still occupy widgets slots.
        "StringConcatenate" => Ordered(&[wl!("string_a"), wl!("string_b"), wl!("delimiter")]),
        "CFGOverride" => Ordered(&[wl!("cfg"), wl!("start_percent"), wl!("end_percent")]),
        "ComfyMathExpression" => Ordered(&[wl!("expression")]),
        "TextGenerate" | "TextGenerateLTX2Prompt" => TextGenerateDynamicCombo,
        "RandomNoise" => Ordered(&[wl!("noise_seed"), UI]),
        "SaveImage" => Ordered(&[w!("filename_prefix")]),
        "LoadImage" => Ordered(&[w!("image")]),
        "LoadVideo" => Ordered(&[w!("file"), w!("upload")]),
        // comfy-core LoadAudio: [audio, audioUI, upload]; only `audio` is an API input.
        "LoadAudio" => Ordered(&[w!("audio")]),
        "CreateVideo" => Ordered(&[w!("fps")]),
        "SaveVideo" => Ordered(&[w!("filename_prefix"), w!("format"), w!("codec")]),
        "FlashVSR_SM_Model" => Ordered(&[
            w!("dit"),
            w!("proj_pt"),
            w!("emb_pt"),
            w!("version"),
            w!("tiny_long"),
            w!("offload"),
        ]),
        "FlashVSR_SM_Vae" => Ordered(&[w!("vae"), w!("tcd_encoder"), w!("lite_vae")]),
        "FlashVSR_SM_Decoder" => Ordered(&[w!("full_tiled"), w!("color_fix"), w!("fix_method")]),
        // widgets_values includes control_after_generate after seed (INV-VU-FLASH-12).
        "FlashVSR_SM_KSampler" => Ordered(&[
            wl!("width"),
            wl!("height"),
            wl!("seed"),
            UI,
            wl!("scale"),
            wl!("kv_ratio"),
            wl!("local_range"),
            wl!("steps"),
            wl!("cfg"),
            wl!("sparse_ratio"),
        ]),
        // SeedVR2 Video Upscaler (INV-VU / Wave 0 open-box): canvas widgets -> API inputs.
        "SeedVR2LoadVAEModel" => Ordered(&[
            w!("model"),
            w!("device"),
            w!("encode_tiled"),
            w!("encode_tile_size"),
            w!("encode_tile_overlap"),
            w!("decode_tiled"),
            w!("decode_tile_size"),
            w!("decode_tile_overlap"),
            w!("tile_debug"),
            w!("offload_device"),
            w!("cache_model"),
        ]),
        "SeedVR2LoadDiTModel" => Ordered(&[
            w!("model"),
            w!("device"),
            w!("blocks_to_swap"),
            w!("swap_io_components"),
            w!("offload_device"),
            w!("cache_model"),
            w!("attention_mode"),
        ]),
        // widgets_values includes control_after_generate after seed (same as KSampler).
        "SeedVR2VideoUpscaler" => Ordered(&[
            wl!("seed"),
            UI,
            wl!("resolution"),
            wl!("max_resolution"),
            wl!("batch_size"),
            wl!("uniform_batch_size"),
            wl!("color_correction"),
            wl!("temporal_overlap"),
            wl!("prepend_frames"),
            wl!("input_noise_scale"),
            wl!("latent_noise_scale"),
            wl!("offload_device"),
            wl!("enable_debug"),
        ]),
        "SeedVR2TorchCompileSettings" => Ordered(&[
            w!("backend"),
            w!("mode"),
            w!("fullgraph"),
            w!("dynamic"),
            w!("dynamo_cache_size_limit"),
            w!("dynamo_recompile_limit"),
        ]),
        // Not matched by the Empty*LatentImage family rule (suffix differs).
        "EmptySD3LatentImage64" => Ordered(&[w!("width"), w!("height"), w!("batch_size")]),
        "KSampler" => Ordered(&[
            wl!("seed"),
            UI,
            wl!("steps"),
            wl!("cfg"),
            wl!("sampler_name"),
            wl!("scheduler"),
            wl!("denoise"),
        ]),
        "KSamplerAdvanced" => Ordered(&[
            wl!("add_noise"),
            wl!("noise_seed"),
            UI,
            wl!("steps"),
            wl!("cfg"),
            wl!("sampler_name"),
            wl!("scheduler"),
            wl!("start_at_step"),
            wl!("end_at_step"),
            wl!("return_with_leftover_noise"),
        ]),
        // Convert Widget to Input proxies: widgets_values[0]=value, [1]=control_after_generate.
        "PrimitiveInt" | "PrimitiveFloat" | "PrimitiveBoolean" | "PrimitiveString" => {
            Ordered(&[w!("value")])
        }
        // comfy-core nodes used by certified FLUX2 / Krea2 workflows (widget sockets keep slots).
        "CFGGuider" => Ordered(&[wl!("cfg")]),
        "ComfySwitchNode" => Ordered(&[wl!("switch")]),
        "Flux2Scheduler" => Ordered(&[wl!("steps"), wl!("width"), wl!("height")]),
        "FluxKontextMultiReferenceLatentMethod" => Ordered(&[wl!("reference_latents_method")]),
        "ModelSamplingFlux" => Ordered(&[
            wl!("max_shift"),
            wl!("base_shift"),
            wl!("width"),
            wl!("height"),
        ]),
        "TextEncodeQwenImageEditPlus" => Ordered(&[wl!("prompt")]),
        // ComfyUI 0.38 comfy_extras.nodes_qwen (Qwen Image 2.1). Modern frontend keeps linked
        // widget slots; `images` is COMFY_AUTOGROW_V3 (sockets only, no widget slot).
        "QwenImage21Cache" => Ordered(&[wl!("device"), wl!("dtype")]),
        "TextEncodeQwenImage21" => {
            Ordered(&[wl!("prompt"), wl!("negative_prompt"), wl!("resolution")])
        }
        "SaveImageAdvanced" => DynamicCombo(&SAVE_IMAGE_ADVANCED),
        // ComfyUI-H3-FaceRefine (bgxiong-ComfyUI MiniMax-H3 pack, nodes.py INPUT_TYPES).
        "H3FaceTrackCrop" => Ordered(&[
            wl!("detector"),
            wl!("confidence"),
            wl!("crop_factor"),
            wl!("canvas_width"),
            wl!("canvas_height"),
            wl!("canvas_mode"),
            wl!("smooth_window"),
            wl!("size_smooth_window"),
            wl!("smooth_method"),
            wl!("size_mode"),
            wl!("identity_track"),
            wl!("identity_threshold"),
            wl!("select"),
            wl!("fallback_detector"),
            wl!("fallback_head_frac"),
        ]),
        "H3FaceStitch" => Ordered(&[
            wl!("paste_region"),
            wl!("mask_dilation"),
            wl!("feather"),
            wl!("colour_match"),
            wl!("blend"),
            wl!("undetected_frames"),
            wl!("feather_scales_with_crop"),
        ]),
        "H3PerFrameDenoise" => Ordered(&[
            wl!("strength_small_face"),
            wl!("strength_large_face"),
            wl!("scale_mode"),
            wl!("face_px_small"),
            wl!("face_px_large"),
            wl!("gamma"),
            wl!("smooth_frames"),
        ]),
        "MiniMaxH3TurboSampler"
        | "GetVideoComponents"
        | "BasicGuider"
        | "SamplerCustomAdvanced"
        | "GetImageSize"
        | "VAEEncode"
        | "VAEDecode"
        | "ReferenceLatent"
        | "MarkdownNote"
        | "Note"
        | "PreviewAny" => NoApiInputs,
        _ => return None,
    })
}

fn unknown_layout(class_type: &str) -> MappingError {
    MappingError::new(
        MappingErrorCode::NodeDefinitionIncomplete,
        format!(
            "no audited widget order for class_type `{class_type}`; export the workflow in API \
             format or add a node_registry widget layout"
        ),
    )
}

fn is_node_link(v: &Value) -> bool {
    let Some(a) = v.as_array() else {
        return false;
    };
    a.len() == 2 && a[0].is_string() && (a[1].is_u64() || a[1].is_i64())
}

fn input_value_missing(v: Option<&Value>) -> bool {
    match v {
        None => true,
        Some(Value::Null) => true,
        Some(Value::String(s)) => s.trim().is_empty(),
        Some(Value::Object(m)) => m.is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        _ => false,
    }
}

/// Positional index of `field` in an audited layout (UI-only slots are counted).
fn widget_index(layout: WidgetLayout, field: &str) -> Option<usize> {
    match layout {
        WidgetLayout::Ordered(slots) => ordered_slot_index(slots, field),
        WidgetLayout::TextGenerateDynamicCombo => (field == "prompt").then_some(0),
        WidgetLayout::DynamicCombo(layout) => {
            ordered_slot_index(layout.head, field).or_else(|| {
                (field == layout.combo).then_some(layout.head.len())
            })
        }
        WidgetLayout::NoApiInputs => None,
    }
}

fn ordered_slot_index(slots: &[WidgetSlot], field: &str) -> Option<usize> {
    slots
        .iter()
        .position(|slot| matches!(slot, WidgetSlot::Input { name, .. } if *name == field))
}

fn combo_option_unknown(class_type: &str, combo: &str, value: &Value) -> MappingError {
    MappingError::new(
        MappingErrorCode::NodeDefinitionIncomplete,
        format!("no audited `{combo}` option {value} for class_type `{class_type}`"),
    )
}

/// Position of a DynamicCombo child (`<combo>.<child>`) under the option currently stored in
/// `widgets_values`. Unknown option keys are rejected, never decoded by guess.
fn dynamic_combo_child_index(
    class_type: &str,
    layout: &DynamicComboLayout,
    wv: &[Value],
    field: &str,
) -> Result<Option<usize>, MappingError> {
    let Some(child) = field
        .strip_prefix(layout.combo)
        .and_then(|rest| rest.strip_prefix('.'))
    else {
        return Ok(None);
    };
    let key_at = layout.head.len();
    let Some(raw) = wv.get(key_at) else {
        return Ok(None);
    };
    let option = layout
        .options
        .iter()
        .find(|o| raw.as_str() == Some(o.key))
        .ok_or_else(|| combo_option_unknown(class_type, layout.combo, raw))?;
    Ok(option
        .children
        .iter()
        .position(|c| *c == child)
        .map(|pos| key_at + 1 + pos))
}

/// Write `value` onto a canvas node's `widgets_values` at the named widget field.
/// Used when flattening subgraph instances: parent widgets overlay internal targets.
/// `Ok(false)`: the field is not a widget of this (known) node. Unknown nodes are rejected.
pub fn overlay_widget_field(
    node: &mut Value,
    field: &str,
    value: &Value,
) -> Result<bool, MappingError> {
    let ct = node
        .get("type")
        .and_then(|v| v.as_str())
        .or_else(|| node.get("class_type").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let layout = widget_layout(&ct).ok_or_else(|| unknown_layout(&ct))?;
    let idx = match (widget_index(layout, field), layout) {
        (Some(idx), _) => Some(idx),
        (None, WidgetLayout::DynamicCombo(combo)) => {
            let wv: &[Value] = node
                .get("widgets_values")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            dynamic_combo_child_index(&ct, combo, wv, field)?
        }
        (None, _) => None,
    };
    let Some(idx) = idx else {
        return Ok(false);
    };
    let Some(obj) = node.as_object_mut() else {
        return Ok(false);
    };
    let wv = obj
        .entry("widgets_values")
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(arr) = wv.as_array_mut() else {
        return Ok(false);
    };
    while arr.len() <= idx {
        arr.push(Value::Null);
    }
    arr[idx] = value.clone();
    Ok(true)
}

/// Merge canvas/UI `widgets_values` into API `inputs` by audited widget order.
/// Never overwrites an existing node link; skips fields already filled with scalars.
/// Empty/absent widgets are a no-op for every node; positional widgets on an unknown node
/// are rejected instead of guessed.
pub fn merge_widgets_values_into_inputs(
    class_type: &str,
    inputs: &mut Map<String, Value>,
    widgets: Option<&[Value]>,
) -> Result<(), MappingError> {
    let Some(wv) = widgets.filter(|w| !w.is_empty()) else {
        return Ok(());
    };
    let layout = widget_layout(class_type).ok_or_else(|| unknown_layout(class_type))?;
    match layout {
        WidgetLayout::NoApiInputs => {}
        WidgetLayout::TextGenerateDynamicCombo => merge_text_generate_widgets(inputs, wv),
        WidgetLayout::Ordered(slots) => {
            merge_ordered(slots, inputs, wv);
        }
        WidgetLayout::DynamicCombo(layout) => merge_dynamic_combo(class_type, layout, inputs, wv)?,
    }
    Ok(())
}

/// Fill `field` from one widget value unless the input is linked or already set.
fn fill_widget_value(inputs: &mut Map<String, Value>, field: &str, val: &Value) {
    if inputs.get(field).is_some_and(is_node_link)
        || !input_value_missing(inputs.get(field))
        || is_node_link(val)
    {
        return;
    }
    inputs.insert(field.into(), val.clone());
}

fn merge_dynamic_combo(
    class_type: &str,
    layout: &DynamicComboLayout,
    inputs: &mut Map<String, Value>,
    wv: &[Value],
) -> Result<(), MappingError> {
    let key_at = layout.head.len();
    let Some(raw) = wv.get(key_at) else {
        merge_ordered(layout.head, inputs, wv);
        return Ok(());
    };
    let option = layout
        .options
        .iter()
        .find(|o| raw.as_str() == Some(o.key))
        .ok_or_else(|| combo_option_unknown(class_type, layout.combo, raw))?;
    merge_ordered(layout.head, inputs, &wv[..key_at]);
    fill_widget_value(inputs, layout.combo, raw);
    for (pos, child) in option.children.iter().enumerate() {
        let Some(val) = wv.get(key_at + 1 + pos) else {
            break;
        };
        fill_widget_value(inputs, &format!("{}.{child}", layout.combo), val);
    }
    Ok(())
}

fn merge_ordered(slots: &[WidgetSlot], inputs: &mut Map<String, Value>, wv: &[Value]) {
    let mut wi = 0usize;
    for slot in slots {
        if wi >= wv.len() {
            break;
        }
        let WidgetSlot::Input {
            name,
            linked_consumes,
        } = *slot
        else {
            wi += 1;
            continue;
        };
        if inputs.get(name).is_some_and(is_node_link) {
            if linked_consumes {
                wi += 1;
            }
            continue;
        }
        if !input_value_missing(inputs.get(name)) {
            wi += 1;
            continue;
        }
        let val = &wv[wi];
        wi += 1;
        if is_node_link(val) {
            continue;
        }
        inputs.insert(name.into(), val.clone());
    }
}

/// Official TextGenerate widgets -> API DynamicCombo dotted keys (LTX / Krea Save API shape).
/// Order: prompt, max_length, sampling_mode key, [on: temp/top_k/top_p/min_p/rep/seed/presence],
/// thinking, use_default_template. Linked slots still consume widgets_values entries.
fn merge_text_generate_widgets(inputs: &mut Map<String, Value>, wv: &[Value]) {
    let mut wi = 0usize;

    let consume_or_fill =
        |inputs: &mut Map<String, Value>, field: &str, wv: &[Value], wi: &mut usize| {
            if *wi >= wv.len() {
                return;
            }
            if inputs.get(field).is_some_and(is_node_link) {
                *wi += 1;
                return;
            }
            if !input_value_missing(inputs.get(field)) {
                *wi += 1;
                return;
            }
            let val = &wv[*wi];
            *wi += 1;
            if is_node_link(val) {
                return;
            }
            inputs.insert(field.into(), val.clone());
        };

    consume_or_fill(inputs, "prompt", wv, &mut wi);
    consume_or_fill(inputs, "max_length", wv, &mut wi);

    if wi >= wv.len() {
        return;
    }
    let mode_missing = !inputs.get("sampling_mode").is_some_and(is_node_link)
        && input_value_missing(inputs.get("sampling_mode"));
    let mode_val = &wv[wi];
    wi += 1;
    let mode_str = mode_val.as_str().unwrap_or("").trim().to_ascii_lowercase();
    if mode_missing && !is_node_link(mode_val) {
        inputs.insert("sampling_mode".into(), mode_val.clone());
    }

    const ON_NESTED: &[&str] = &[
        "temperature",
        "top_k",
        "top_p",
        "min_p",
        "repetition_penalty",
        "seed",
        "presence_penalty",
    ];
    let fill_on_children = mode_str == "on" || mode_str.is_empty();
    if fill_on_children {
        for nested in ON_NESTED {
            if wi >= wv.len() {
                break;
            }
            let key = format!("sampling_mode.{nested}");
            if inputs.get(&key).is_some_and(is_node_link) {
                wi += 1;
                continue;
            }
            if !input_value_missing(inputs.get(key.as_str())) {
                wi += 1;
                continue;
            }
            let val = &wv[wi];
            wi += 1;
            if is_node_link(val) {
                continue;
            }
            inputs.insert(key, val.clone());
        }
    }

    consume_or_fill(inputs, "thinking", wv, &mut wi);
    consume_or_fill(inputs, "use_default_template", wv, &mut wi);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn merge(class_type: &str, inputs: &mut Map<String, Value>, widgets: Option<&[Value]>) {
        merge_widgets_values_into_inputs(class_type, inputs, widgets).expect("audited layout");
    }

    fn overlay(node: &mut Value, field: &str, value: &Value) -> bool {
        overlay_widget_field(node, field, value).expect("audited layout")
    }

    #[test]
    fn unknown_third_party_widgets_are_rejected_not_guessed() {
        let mut m = Map::new();
        let err = merge_widgets_values_into_inputs(
            "ThirdPartyMagicSampler",
            &mut m,
            Some(&[json!(42), json!("fixed"), json!(20)]),
        )
        .unwrap_err();
        assert_eq!(err.code, MappingErrorCode::NodeDefinitionIncomplete);
        assert!(m.is_empty(), "rejected merge must not write any input");
    }

    #[test]
    fn qwen_image_21_nodes_keep_linked_widget_slots() {
        // Server-saved Qwen Image 2.1 template: linked sockets still occupy widget slots.
        let mut cache = Map::new();
        cache.insert("model".into(), json!(["1", 0]));
        merge("QwenImage21Cache", &mut cache, Some(&[json!("auto"), json!("default")]));
        assert_eq!(cache["device"], json!("auto"));
        assert_eq!(cache["dtype"], json!("default"));

        let mut enc = Map::new();
        enc.insert("prompt".into(), json!(["9", 0]));
        merge("TextEncodeQwenImage21", &mut enc, Some(&[json!(""), json!("blurry"), json!(1024)]));
        assert_eq!(enc["prompt"], json!(["9", 0]), "link must not be overwritten");
        assert_eq!(enc["negative_prompt"], json!("blurry"));
        assert_eq!(enc["resolution"], json!(1024));
    }

    #[test]
    fn save_image_advanced_dynamic_combo_decodes_selected_option_children() {
        let mut png = Map::new();
        merge(
            "SaveImageAdvanced",
            &mut png,
            Some(&[json!("Qwen_image_2.1"), json!("png"), json!("8-bit"), json!("sRGB")]),
        );
        assert_eq!(png["filename_prefix"], json!("Qwen_image_2.1"));
        assert_eq!(png["format"], json!("png"));
        assert_eq!(png["format.bit_depth"], json!("8-bit"));
        assert_eq!(png["format.input_color_space"], json!("sRGB"));
        assert_eq!(png.len(), 4);

        let mut avif = Map::new();
        merge(
            "SaveImageAdvanced",
            &mut avif,
            Some(&[json!("p"), json!("avif"), json!("10-bit"), json!("sRGB"), json!(30), json!("lossy")]),
        );
        assert_eq!(avif["format.crf"], json!(30));
        assert_eq!(avif["format.save_mode"], json!("lossy"));
    }

    #[test]
    fn dynamic_combo_unknown_option_is_rejected_without_writes() {
        let mut m = Map::new();
        let err = merge_widgets_values_into_inputs(
            "SaveImageAdvanced",
            &mut m,
            Some(&[json!("p"), json!("webp"), json!(90)]),
        )
        .unwrap_err();
        assert_eq!(err.code, MappingErrorCode::NodeDefinitionIncomplete);
        assert!(m.is_empty());
        let mut node = json!({"type": "SaveImageAdvanced", "widgets_values": ["p", "webp", 90]});
        assert!(overlay_widget_field(&mut node, "format.quality", &json!(1)).is_err());
    }

    #[test]
    fn dynamic_combo_overlay_targets_selected_option_child() {
        let mut node = json!({
            "type": "SaveImageAdvanced",
            "widgets_values": ["p", "png", "8-bit", "sRGB"]
        });
        assert!(overlay(&mut node, "format.bit_depth", &json!("16-bit")));
        assert!(overlay(&mut node, "filename_prefix", &json!("q")));
        assert_eq!(node["widgets_values"], json!(["q", "png", "16-bit", "sRGB"]));
    }

    #[test]
    fn unknown_node_without_widgets_is_not_rejected() {
        let mut m = Map::new();
        m.insert("images".into(), json!(["3", 0]));
        merge_widgets_values_into_inputs("ThirdPartyPassthrough", &mut m, Some(&[])).unwrap();
        merge_widgets_values_into_inputs("ThirdPartyPassthrough", &mut m, None).unwrap();
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn overlay_on_unknown_node_is_rejected() {
        let mut node = json!({"type": "ThirdPartyMagicSampler", "widgets_values": [1, 2]});
        let err = overlay_widget_field(&mut node, "steps", &json!(9)).unwrap_err();
        assert_eq!(err.code, MappingErrorCode::NodeDefinitionIncomplete);
        assert_eq!(node["widgets_values"], json!([1, 2]));
    }

    #[test]
    fn overlay_ksampler_steps_skips_control_after_generate_slot() {
        let mut node = json!({
            "type": "KSampler",
            "widgets_values": [7, "fixed", 20, 4.0, "euler", "simple", 1.0]
        });
        assert!(overlay(&mut node, "steps", &json!(30)));
        assert_eq!(node["widgets_values"][1], json!("fixed"));
        assert_eq!(node["widgets_values"][2], json!(30));
    }

    #[test]
    fn annotations_previews_and_subgraph_proxies_map_no_inputs() {
        for ct in [
            "Note",
            "MarkdownNote",
            "PreviewAny",
            "4c314f31-ecda-4b08-ae98-faaba1bf613f",
        ] {
            let mut m = Map::new();
            merge(ct, &mut m, Some(&[json!("text"), json!(null)]));
            assert!(m.is_empty(), "{ct} must not produce API inputs");
        }
    }

    #[test]
    fn h3_face_track_crop_certified_widgets_map_by_input_types_order() {
        let mut m = Map::new();
        m.insert("images".into(), json!(["1", 0]));
        m.insert("identity_reference".into(), json!(["2", 0]));
        merge(
            "H3FaceTrackCrop",
            &mut m,
            Some(&[
                json!("bbox\\face_yolov8m.pt"),
                json!(0.35),
                json!(2.5),
                json!(512),
                json!(512),
                json!("auto_capped_768"),
                json!(21),
                json!(51),
                json!("gaussian"),
                json!("per_frame"),
                json!(true),
                json!(0.28),
                json!("largest"),
                json!("segm\\person_yolov8m-seg.pt"),
                json!(0.5),
            ]),
        );
        assert_eq!(m["canvas_mode"], json!("auto_capped_768"));
        assert_eq!(m["size_mode"], json!("per_frame"));
        assert_eq!(m["fallback_detector"], json!("segm\\person_yolov8m-seg.pt"));
        assert_eq!(m["fallback_head_frac"], json!(0.5));
        assert!(is_node_link(&m["identity_reference"]));
    }

    #[test]
    fn h3_face_stitch_and_per_frame_denoise_map_all_widgets() {
        let mut stitch = Map::new();
        merge(
            "H3FaceStitch",
            &mut stitch,
            Some(&[
                json!("face_only"),
                json!(24),
                json!(24),
                json!(1),
                json!(1),
                json!("fade_out"),
                json!(false),
            ]),
        );
        assert_eq!(stitch["feather"], json!(24));
        assert_eq!(stitch["undetected_frames"], json!("fade_out"));
        assert_eq!(stitch["feather_scales_with_crop"], json!(false));

        let mut denoise = Map::new();
        merge(
            "H3PerFrameDenoise",
            &mut denoise,
            Some(&[
                json!(0.8),
                json!(0.35),
                json!("absolute_px"),
                json!(30),
                json!(120),
                json!(1),
                json!(9),
            ]),
        );
        assert_eq!(denoise["scale_mode"], json!("absolute_px"));
        assert_eq!(denoise["smooth_frames"], json!(9));
    }

    #[test]
    fn widget_socket_layouts_keep_slots_when_linked() {
        let mut sched = Map::new();
        sched.insert("width".into(), json!(["5", 0]));
        sched.insert("height".into(), json!(["5", 1]));
        merge(
            "Flux2Scheduler",
            &mut sched,
            Some(&[json!(20), json!(1248), json!(832)]),
        );
        assert_eq!(sched["steps"], json!(20));
        assert!(is_node_link(&sched["width"]));
        assert!(is_node_link(&sched["height"]));

        let mut sampling = Map::new();
        sampling.insert("model".into(), json!(["1", 0]));
        sampling.insert("width".into(), json!(["6", 0]));
        sampling.insert("height".into(), json!(["6", 1]));
        merge(
            "ModelSamplingFlux",
            &mut sampling,
            Some(&[json!(1.15), json!(0.5), json!(1024), json!(1024)]),
        );
        assert_eq!(sampling["max_shift"], json!(1.15));
        assert_eq!(sampling["base_shift"], json!(0.5));
        assert!(is_node_link(&sampling["width"]));
    }

    #[test]
    fn ksampler_advanced_skips_control_after_generate() {
        let mut m = Map::new();
        merge(
            "KSamplerAdvanced",
            &mut m,
            Some(&[
                json!("enable"),
                json!(11),
                json!("randomize"),
                json!(25),
                json!(6.5),
                json!("euler"),
                json!("normal"),
                json!(0),
                json!(10000),
                json!("disable"),
            ]),
        );
        assert_eq!(m["noise_seed"], json!(11));
        assert_eq!(m["steps"], json!(25));
        assert_eq!(m["return_with_leftover_noise"], json!("disable"));
        assert!(!m.contains_key("control_after_generate"));
    }

    #[test]
    fn omnigen2_scale_sampler_scheduler_guider() {
        let mut scale = Map::new();
        scale.insert("image".into(), json!(["16", 0]));
        merge(
            "ImageScaleToTotalPixels",
            &mut scale,
            Some(&[json!("area"), json!(1)]),
        );
        assert_eq!(
            scale.get("upscale_method").and_then(|v| v.as_str()),
            Some("area")
        );
        assert_eq!(scale.get("megapixels").and_then(|v| v.as_f64()), Some(1.0));
        assert_eq!(scale.get("image").and_then(|v| v[0].as_str()), Some("16"));

        let mut sampler = Map::new();
        merge("KSamplerSelect", &mut sampler, Some(&[json!("euler")]));
        assert_eq!(
            sampler.get("sampler_name").and_then(|v| v.as_str()),
            Some("euler")
        );

        let mut sched = Map::new();
        sched.insert("model".into(), json!(["12", 0]));
        merge(
            "BasicScheduler",
            &mut sched,
            Some(&[json!("simple"), json!(20), json!(1)]),
        );
        assert_eq!(
            sched.get("scheduler").and_then(|v| v.as_str()),
            Some("simple")
        );
        assert_eq!(sched.get("steps").and_then(|v| v.as_u64()), Some(20));

        let mut guider = Map::new();
        guider.insert("model".into(), json!(["12", 0]));
        guider.insert("cond1".into(), json!(["15", 0]));
        merge(
            "DualCFGGuider",
            &mut guider,
            Some(&[json!(5), json!(2), json!("regular")]),
        );
        assert_eq!(guider.get("cfg").and_then(|v| v.as_u64()), Some(5));
        assert_eq!(
            guider.get("style").and_then(|v| v.as_str()),
            Some("regular")
        );
    }

    #[test]
    fn does_not_overwrite_links() {
        let mut m = Map::new();
        m.insert("upscale_method".into(), json!(["99", 0]));
        merge(
            "ImageScaleToTotalPixels",
            &mut m,
            Some(&[json!("area"), json!(1)]),
        );
        assert!(is_node_link(m.get("upscale_method").unwrap()));
    }

    #[test]
    fn latent_widgets_fill_wh() {
        let mut m = Map::new();
        merge(
            "EmptySD3LatentImage",
            &mut m,
            Some(&[json!(1024), json!(768), json!(1)]),
        );
        assert_eq!(m.get("width").and_then(|v| v.as_u64()), Some(1024));
        assert_eq!(m.get("height").and_then(|v| v.as_u64()), Some(768));
        assert_eq!(m.get("batch_size").and_then(|v| v.as_u64()), Some(1));
    }

    #[test]
    fn primitive_int_merges_value_from_widgets() {
        let mut m = Map::new();
        merge(
            "PrimitiveInt",
            &mut m,
            Some(&[json!(1536), json!("fixed")]),
        );
        assert_eq!(m.get("value").and_then(|v| v.as_u64()), Some(1536));
        assert_eq!(
            m.len(),
            1,
            "control_after_generate must not become an input"
        );
    }

    #[test]
    fn primitive_boolean_merges_value_from_widgets() {
        let mut m = Map::new();
        merge(
            "PrimitiveBoolean",
            &mut m,
            Some(&[json!(true), json!("fixed")]),
        );
        assert_eq!(m.get("value").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn primitive_float_and_string_merge_value() {
        let mut f = Map::new();
        merge(
            "PrimitiveFloat",
            &mut f,
            Some(&[json!(1.5), json!("fixed")]),
        );
        assert_eq!(f.get("value").and_then(|v| v.as_f64()), Some(1.5));

        let mut s = Map::new();
        merge(
            "PrimitiveString",
            &mut s,
            Some(&[json!("hello"), json!("fixed")]),
        );
        assert_eq!(s.get("value").and_then(|v| v.as_str()), Some("hello"));
    }

    #[test]
    fn primitive_does_not_overwrite_existing_link() {
        let mut m = Map::new();
        m.insert("value".into(), json!(["99", 0]));
        merge(
            "PrimitiveInt",
            &mut m,
            Some(&[json!(1536), json!("fixed")]),
        );
        assert!(is_node_link(m.get("value").unwrap()));
    }

    #[test]
    fn json_extract_linked_json_string_merges_key() {
        let mut m = Map::new();
        m.insert("json_string".into(), json!(["148", 0]));
        merge(
            "JsonExtractString",
            &mut m,
            Some(&[json!(""), json!("mu")]),
        );
        assert!(is_node_link(m.get("json_string").unwrap()));
        assert_eq!(m.get("key").and_then(|v| v.as_str()), Some("mu"));
    }

    #[test]
    fn json_extract_linked_key_merges_json_string() {
        let mut m = Map::new();
        m.insert("key".into(), json!(["156", 0]));
        merge(
            "JsonExtractString",
            &mut m,
            Some(&[json!("{\"Default\":{}}"), json!("Default")]),
        );
        assert_eq!(
            m.get("json_string").and_then(|v| v.as_str()),
            Some("{\"Default\":{}}")
        );
        assert!(is_node_link(m.get("key").unwrap()));
    }

    #[test]
    fn string_replace_linked_string_merges_find_replace() {
        let mut m = Map::new();
        m.insert("string".into(), json!(["147", 0]));
        merge(
            "StringReplace",
            &mut m,
            Some(&[json!(""), json!("'"), json!("\"")]),
        );
        assert!(is_node_link(m.get("string").unwrap()));
        assert_eq!(m.get("find").and_then(|v| v.as_str()), Some("'"));
        assert_eq!(m.get("replace").and_then(|v| v.as_str()), Some("\""));
    }

    #[test]
    fn string_replace_linked_string_and_replace_merges_find_only() {
        let mut m = Map::new();
        m.insert("string".into(), json!(["114", 0]));
        m.insert("replace".into(), json!(["115", 0]));
        merge(
            "StringReplace",
            &mut m,
            Some(&[json!(""), json!("{{original_prompt}}"), json!("")]),
        );
        assert!(is_node_link(m.get("string").unwrap()));
        assert!(is_node_link(m.get("replace").unwrap()));
        assert_eq!(
            m.get("find").and_then(|v| v.as_str()),
            Some("{{original_prompt}}")
        );
    }

    #[test]
    fn cfg_override_merges_cfg_percent_widgets() {
        let mut m = Map::new();
        m.insert("model".into(), json!(["155", 0]));
        merge(
            "CFGOverride",
            &mut m,
            Some(&[json!(3), json!(0.7), json!(1)]),
        );
        assert!(is_node_link(m.get("model").unwrap()));
        assert_eq!(m.get("cfg").and_then(|v| v.as_u64()), Some(3));
        assert_eq!(m.get("start_percent").and_then(|v| v.as_f64()), Some(0.7));
        assert_eq!(m.get("end_percent").and_then(|v| v.as_u64()), Some(1));
    }

    #[test]
    fn comfy_math_expression_merges_expression_not_schema_default() {
        let mut m = Map::new();
        m.insert("values.a".into(), json!(["31", 0]));
        merge(
            "ComfyMathExpression",
            &mut m,
            Some(&[json!("max(((a + 15) // 16) * 16, 256)")]),
        );
        assert_eq!(
            m.get("expression").and_then(|v| v.as_str()),
            Some("max(((a + 15) // 16) * 16, 256)")
        );
        assert_ne!(m.get("expression").and_then(|v| v.as_str()), Some("a + b"));
    }

    #[test]
    fn string_concatenate_linked_ab_merges_empty_delimiter() {
        let mut m = Map::new();
        m.insert("string_a".into(), json!(["18", 0]));
        m.insert("string_b".into(), json!(["19", 0]));
        merge(
            "StringConcatenate",
            &mut m,
            Some(&[json!(""), json!(""), json!("")]),
        );
        assert!(is_node_link(m.get("string_a").unwrap()));
        assert!(is_node_link(m.get("string_b").unwrap()));
        assert_eq!(m.get("delimiter").and_then(|v| v.as_str()), Some(""));
    }

    #[test]
    fn string_concatenate_unbound_b_merges_trigger_and_delimiter() {
        let mut m = Map::new();
        m.insert("string_a".into(), json!(["20", 0]));
        merge(
            "StringConcatenate",
            &mut m,
            Some(&[
                json!(""),
                json!("muted minimalist sketch style"),
                json!(", "),
            ]),
        );
        assert!(is_node_link(m.get("string_a").unwrap()));
        assert_eq!(
            m.get("string_b").and_then(|v| v.as_str()),
            Some("muted minimalist sketch style")
        );
        assert_eq!(m.get("delimiter").and_then(|v| v.as_str()), Some(", "));
    }

    #[test]
    fn text_generate_widgets_become_sampling_mode_dotted_inputs() {
        let mut m = Map::new();
        m.insert("clip".into(), json!(["11", 0]));
        m.insert("prompt".into(), json!(["17", 0]));
        m.insert("max_length".into(), json!(["49", 0]));
        m.insert("thinking".into(), json!(["50", 0]));
        merge(
            "TextGenerate",
            &mut m,
            Some(&[
                json!(""),
                json!(512),
                json!("on"),
                json!(0.7),
                json!(64),
                json!(0.95),
                json!(0.05),
                json!(1.05),
                json!(0),
                json!(0),
                json!(true),
                json!(true),
            ]),
        );
        assert!(is_node_link(m.get("prompt").unwrap()));
        assert!(is_node_link(m.get("max_length").unwrap()));
        assert!(is_node_link(m.get("thinking").unwrap()));
        assert_eq!(m.get("sampling_mode").and_then(|v| v.as_str()), Some("on"));
        assert_eq!(
            m.get("sampling_mode.temperature").and_then(|v| v.as_f64()),
            Some(0.7)
        );
        assert_eq!(
            m.get("sampling_mode.top_k").and_then(|v| v.as_u64()),
            Some(64)
        );
        assert_eq!(
            m.get("sampling_mode.top_p").and_then(|v| v.as_f64()),
            Some(0.95)
        );
        assert_eq!(
            m.get("sampling_mode.min_p").and_then(|v| v.as_f64()),
            Some(0.05)
        );
        assert_eq!(
            m.get("sampling_mode.repetition_penalty")
                .and_then(|v| v.as_f64()),
            Some(1.05)
        );
        assert_eq!(
            m.get("sampling_mode.seed").and_then(|v| v.as_u64()),
            Some(0)
        );
        assert_eq!(
            m.get("sampling_mode.presence_penalty")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
        assert_eq!(
            m.get("use_default_template").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(
            !m.contains_key("control_after_generate"),
            "control_after_generate must not become an input"
        );
    }

    #[test]
    fn resolution_selector_merges_multiple_not_batch_size() {
        let mut m = Map::new();
        merge(
            "ResolutionSelector",
            &mut m,
            Some(&[json!("1:1 (Square)"), json!(0.4), json!(32)]),
        );
        assert_eq!(
            m.get("aspect_ratio").and_then(|v| v.as_str()),
            Some("1:1 (Square)")
        );
        assert_eq!(m.get("megapixels").and_then(|v| v.as_f64()), Some(0.4));
        assert_eq!(m.get("multiple").and_then(|v| v.as_u64()), Some(32));
        assert!(!m.contains_key("batch_size"));
    }

    #[test]
    fn minimax_h3_image_to_video_merges_prompt_skips_linked_width() {
        let mut m = Map::new();
        m.insert("width".into(), json!(["115", 0]));
        m.insert("height".into(), json!(["115", 1]));
        merge(
            "MiniMaxH3ImageToVideo",
            &mut m,
            Some(&[
                json!("USER_PROMPT_FROM_WIDGETS"),
                json!(1344),
                json!(768),
                json!(73),
            ]),
        );
        assert_eq!(
            m.get("prompt").and_then(|v| v.as_str()),
            Some("USER_PROMPT_FROM_WIDGETS")
        );
        assert!(is_node_link(m.get("width").unwrap()));
        assert!(is_node_link(m.get("height").unwrap()));
        assert_eq!(m.get("length").and_then(|v| v.as_u64()), Some(73));
    }

    #[test]
    fn apply_vdn_h3_merges_checkpoint_skips_linked_model() {
        let mut m = Map::new();
        m.insert("model".into(), json!(["105:6", 0]));
        merge(
            "ApplyVDNH3",
            &mut m,
            Some(&[
                json!("stage-dmd-step-250"),
                json!(true),
                json!(1.0),
                json!("merge"),
                json!("auto"),
                json!("auto"),
                json!(false),
                json!("grouped"),
            ]),
        );
        assert!(is_node_link(m.get("model").unwrap()));
        assert_eq!(
            m.get("vdn_checkpoint").and_then(|v| v.as_str()),
            Some("stage-dmd-step-250")
        );
        assert_eq!(
            m.get("apply_turbo_adapter").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(m.get("strength").and_then(|v| v.as_f64()), Some(1.0));
        assert_eq!(m.get("lora_mode").and_then(|v| v.as_str()), Some("merge"));
        assert_eq!(
            m.get("branch_weights").and_then(|v| v.as_str()),
            Some("auto")
        );
        assert_eq!(
            m.get("attention_backend").and_then(|v| v.as_str()),
            Some("grouped")
        );
    }

    #[test]
    fn overlay_widget_field_writes_prompt_on_canvas_node() {
        let mut node = json!({
            "type": "MiniMaxH3ImageToVideo",
            "widgets_values": ["baked", 64, 64, 73]
        });
        assert!(overlay(
            &mut node,
            "prompt",
            &json!("INSTANCE_PROMPT")
        ));
        assert_eq!(node["widgets_values"][0], json!("INSTANCE_PROMPT"));
        assert_eq!(node["widgets_values"][3], json!(73));
    }

    #[test]
    fn flashvsr_ksampler_skips_control_after_generate() {
        let mut m = Map::new();
        m.insert("model".into(), json!(["15", 0]));
        m.insert("image".into(), json!(["8", 0]));
        merge(
            "FlashVSR_SM_KSampler",
            &mut m,
            Some(&[
                json!(320),
                json!(192),
                json!(42),
                json!("fixed"),
                json!(4),
                json!(3.5),
                json!(7),
                json!(1),
                json!(1.0),
                json!(2.0),
            ]),
        );
        assert_eq!(m.get("width").and_then(|v| v.as_u64()), Some(320));
        assert_eq!(m.get("height").and_then(|v| v.as_u64()), Some(192));
        assert_eq!(m.get("seed").and_then(|v| v.as_u64()), Some(42));
        assert_eq!(m.get("scale").and_then(|v| v.as_u64()), Some(4));
        assert_eq!(m.get("kv_ratio").and_then(|v| v.as_f64()), Some(3.5));
        assert_eq!(m.get("local_range").and_then(|v| v.as_u64()), Some(7));
        assert_eq!(m.get("cfg").and_then(|v| v.as_f64()), Some(1.0));
        assert!(!m.contains_key("control_after_generate"));
        assert_eq!(m.get("model").and_then(|v| v[0].as_str()), Some("15"));
    }
}
