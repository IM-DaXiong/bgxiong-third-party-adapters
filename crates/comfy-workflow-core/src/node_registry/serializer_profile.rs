//! Standard ComfyUI frontend widget serialization profile (PLAN CW31/CW32, D04).
//!
//! A profile is selected only for a frontend version inside a verified range. The profile turns
//! a normalized definition into an ordered list of widget slots using the frontend's widget
//! construction rules; it never looks at widget values, titles or object key order. Anything the
//! profile cannot prove (missing `input_order`, custom JavaScript widget types, upload widgets
//! without evidence, DynamicCombo children without an explicit order) is returned as a
//! `LayoutGap`, which the binder turns into a blocking diagnostic.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};

use serde::{Deserialize, Serialize};
use alloc::collections::BTreeMap;

use serde_json::Value;

use crate::node_registry::definition::{parse_input_order, parse_input_spec, InputDef, NodeDefinitionV1};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SerializerProfileV1 {
    pub frontend_version: String,
    pub profile_version: String,
}

/// Standard widget rules: widgets follow `input_order` (required, then optional); hidden and
/// `forceInput` inputs have no widget; INT/FLOAT/BOOLEAN/STRING/COMBO/CURVE are one value each;
/// linked widget inputs keep their value slot; `control_after_generate` adds one UI-only value
/// after its INT (explicit option, or the frontend's `seed`/`noise_seed` INT default) or COMBO
/// (explicit option only); `image_upload` adds one UI-only value after its COMBO;
/// `COMFY_DYNAMICCOMBO_V3` is the selector value followed by the selected option's children.
pub const STANDARD_PROFILE_V1: &str = "comfy-frontend-standard-widgets/1";

/// Frontend 1.53.10 widget rules (CW03/CW04 oracle, `tests/oracle_fe_1_53_10.rs`, and the live
/// `LiteGraph.createNode` widget dump recorded in the lanes progress notes). Differences from V1:
/// - `widgets_values` fill the serialized widgets positionally; widgets past the saved values
///   keep their definition default, saved values past the widgets are ignored (LGraphNode
///   configure);
/// - DynamicCombo children follow the transmitted `/object_info` key order (recorded as an
///   explicit `input_order` at capture, see `definition::parse_object_info_text`) and get no
///   implicit `seed`/`noise_seed` control widget;
/// - `video_upload` / `audio_upload` COMBOs carry one serialized upload-button value like
///   `image_upload` (`audioUI` is not serialized), and that value is not checked;
/// - a `COMFY_AUTOGROW_V3` input whose template holds only socket types has no widget.
pub const STANDARD_PROFILE_V2: &str = "comfy-frontend-standard-widgets/2";

/// Whether `profile_version` uses the frontend 1.53.10 rules.
pub fn is_v2(profile_version: &str) -> bool {
    profile_version == STANDARD_PROFILE_V2
}

struct VerifiedRange {
    min: (u32, u32, u32),
    max_exclusive: (u32, u32, u32),
    profile_version: &'static str,
}

/// Frontend versions the standard profile is declared for. Evidence: the corpus U frontend
/// versions recorded in `20261014-comfy-import-inventory.json` (1.41.13 .. 1.48.5) and the
/// upstream `useIntWidget` / `input_order` ordering (ComfyUI_frontend PR #5106). Oracle
/// equality against official API exports is CW03/CW04 evidence (coordinator); a version outside
/// this table has no standard profile and needs a special rule or the frontend bridge.
const VERIFIED_RANGES: &[VerifiedRange] = &[VerifiedRange {
    min: (1, 41, 0),
    max_exclusive: (1, 49, 0),
    profile_version: STANDARD_PROFILE_V1,
}, VerifiedRange {
    // CW03/CW04 oracle: 38 built-in canvas templates exported by the official frontend 1.53.10
    // (ComfyUI 0.39.2, `app.graphToPrompt`) equal the core compile; see
    // `tests/oracle_fe_1_53_10.rs`. Only the exact captured patch is declared.
    min: (1, 53, 10),
    max_exclusive: (1, 53, 11),
    profile_version: STANDARD_PROFILE_V2,
}];

/// The standard profile for the served frontend version. Verified ranges map exactly; any other
/// version (newer, between ranges, pre-release or build suffix) takes the rules of the closest
/// verified frontend generation: before 1.49 the V1 rules, otherwise the V2 rules. An unknown or
/// unparsable version takes the current (V2) rules: a workflow that runs in native ComfyUI must
/// compile whatever frontend the server ships (2026-10-15 Qwen 2.1 edit regression).
pub fn standard_profile_for(frontend_version: Option<&str>) -> Option<SerializerProfileV1> {
    let raw = frontend_version.map(str::trim).filter(|v| !v.is_empty());
    let profile_version = match raw.and_then(parse_version_core) {
        Some(v) => VERIFIED_RANGES
            .iter()
            .find(|range| v >= range.min && v < range.max_exclusive)
            .map(|range| range.profile_version)
            .unwrap_or(if v < (1, 49, 0) { STANDARD_PROFILE_V1 } else { STANDARD_PROFILE_V2 }),
        None => STANDARD_PROFILE_V2,
    };
    Some(SerializerProfileV1 {
        frontend_version: raw.unwrap_or("unknown").to_string(),
        profile_version: profile_version.to_string(),
    })
}

/// `MAJOR.MINOR.PATCH` core of a version string: an optional leading `v`, then the numeric core;
/// a pre-release (`-rc1`) or build (`+x`) suffix and extra numeric parts are ignored.
fn parse_version_core(raw: &str) -> Option<(u32, u32, u32)> {
    let core = raw.strip_prefix('v').unwrap_or(raw);
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut next = || -> Option<u32> {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    Some((next()?, next()?, next()?))
}

/// What a UI-only value must look like to count as proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiOnlyCheck {
    /// Frontend value-control widget: one of `fixed|increment|decrement|randomize`.
    ControlMode,
    /// Upload button value: a string.
    UploadControl,
    /// Frontend 1.53.10 upload button: serialized but never read back; any value.
    FrontendButton,
    /// Declared UI-only by a reviewed special rule; any value.
    DeclaredBySpecialRule,
}

pub const CONTROL_MODES: [&str; 4] = ["fixed", "increment", "decrement", "randomize"];

#[derive(Debug, Clone, PartialEq)]
pub enum ProfileSlot {
    /// One value bound to the API input `api_name` (dotted for DynamicCombo children).
    Widget { api_name: String, def: InputDef },
    /// One UI-only value, never an API input.
    UiOnly { reason: String, check: UiOnlyCheck },
    /// Selector value, then the children of the selected option.
    DynamicCombo {
        api_name: String,
        def: InputDef,
        options: Vec<DynamicComboOptionSlots>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DynamicComboOptionSlots {
    pub key: String,
    pub children: Vec<ProfileSlot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutGap {
    /// The definition lacks an explicit `input_order` (needsDefinitions).
    InputOrderMissing,
    /// The profile object does not name a verified profile.
    UnverifiedProfile { profile_version: String },
    /// A non-forceInput input whose type has a non-standard frontend widget.
    UnsupportedWidgetType { input: String, type_name: String },
    /// An upload widget kind without verified serialization.
    UnverifiedUploadWidget { input: String, option: String },
    /// An option the profile reads has an unexpected shape.
    OptionMalformed { input: String, option: String },
    /// A DynamicCombo option has no explicit child order.
    DynamicChildOrderMissing { input: String, option_key: String },
    /// A pack frontend extension recomputes the serialized values from its own editor state, so
    /// the saved `widgets_values` do not determine the API inputs.
    FrontendExtensionRewrites { python_module: String },
}

/// `(class_type, python_module)` whose frontend extension rewrites the serialized values.
/// PointsEditor (ComfyUI-KJNodes 1.5.2, frontend 1.53.10 oracle CW38.23): the points editor
/// re-encodes `points_store`/`coordinates`/`neg_coordinates`, derives `bboxes` from `bbox_store`
/// and replaces unparsable stores with its defaults
/// (`tests/fixtures/comfy_import_v2/oracle/fe-1.53.10-cw38/api/PointsEditor.*.json`).
const FRONTEND_REWRITTEN_CLASSES: &[(&str, &str)] = &[("PointsEditor", "custom_nodes.ComfyUI-KJNodes")];

/// Whether the frontend extension of a [`FRONTEND_REWRITTEN_CLASSES`] class would serialize the
/// saved values unchanged, so the standard layout is exact for this node. `named[input]` is the
/// saved value at that widget's standard position. Only a fixed point of the rewrite proven by
/// the CW38.23 oracle counts; anything the rewrite could change (an unparsable store, derived
/// values that differ from their store, JSON whose JavaScript re-encoding is not byte-identical)
/// keeps the class blocked.
pub fn frontend_rewrite_is_identity(def: &NodeDefinitionV1, named: &BTreeMap<String, Value>) -> bool {
    match (def.class_type.as_str(), def.python_module.as_deref()) {
        ("PointsEditor", Some("custom_nodes.ComfyUI-KJNodes")) => points_editor_fixed_point(named),
        _ => false,
    }
}

/// KJNodes points editor (oracle `fe-1.53.10-cw38/api/PointsEditor.*`): `points_store` is parsed
/// and re-encoded with `JSON.stringify`, `coordinates` / `neg_coordinates` are the encoded
/// positive / negative point lists, `bbox_store` is re-encoded and `bboxes` is derived from it.
fn points_editor_fixed_point(named: &BTreeMap<String, Value>) -> bool {
    let text = |name: &str| named.get(name).and_then(Value::as_str);
    let (Some(store), Some(pos), Some(neg), Some(bbox_store), Some(bboxes)) =
        (text("points_store"), text("coordinates"), text("neg_coordinates"), text("bbox_store"), text("bboxes"))
    else {
        return false;
    };
    let Ok(Value::Object(parsed)) = serde_json::from_str::<Value>(store) else { return false };
    if parsed.len() != 2 {
        return false;
    }
    let (Some(positive), Some(negative)) = (parsed.get("positive").and_then(js_points), parsed.get("negative").and_then(js_points))
    else {
        return false;
    };
    let canonical_store = format!("{{\"positive\":{positive},\"negative\":{negative}}}");
    let Ok(Value::Array(boxes)) = serde_json::from_str::<Value>(bbox_store) else { return false };
    // Only empty box records have a key order-independent encoding; others stay unproven.
    let canonical_boxes = boxes
        .iter()
        .all(|b| b.as_object().is_some_and(|o| o.is_empty()))
        .then(|| format!("[{}]", vec!["{}"; boxes.len()].join(",")));
    store == canonical_store
        && pos == positive
        && neg == negative
        && canonical_boxes.as_deref() == Some(bbox_store)
        && bboxes == bbox_store
}

/// `JSON.stringify` of a list of `{x, y}` points, when every number re-encodes identically in
/// JavaScript (finite, no exponent form, no integral float written as `n.0`).
fn js_points(value: &Value) -> Option<String> {
    let points = value.as_array()?;
    let mut out = Vec::with_capacity(points.len());
    for point in points {
        let o = point.as_object()?;
        if o.len() != 2 {
            return None;
        }
        out.push(format!("{{\"x\":{},\"y\":{}}}", js_number_text(o.get("x")?)?, js_number_text(o.get("y")?)?));
    }
    Some(format!("[{}]", out.join(",")))
}

fn js_number_text(value: &Value) -> Option<String> {
    let n = value.as_number()?;
    if n.is_f64() {
        let f = n.as_f64()?;
        if !f.is_finite() || f % 1.0 == 0.0 || !(1e-6..1e21).contains(&f.abs()) {
            return None;
        }
    } else if n.as_i64().is_none_or(|i| i.unsigned_abs() > 9_007_199_254_740_991) {
        return None;
    }
    let text = n.to_string();
    (!text.contains(['e', 'E'])).then_some(text)
}

/// Ordered widget slots of `def` under the standard profile.
/// Every frontend serializes a node's widget values even when `widgets_values` is empty or
/// absent (the widget constructors' defaults; ImageCompare `compare_view` in the 1.53.10 oracle),
/// so such a node still needs the profile under any profile version. True when the definition yields any value slot, or a
/// layout gap (which must surface, not be skipped).
pub fn v2_serializes_widgets(def: &NodeDefinitionV1, profile: &SerializerProfileV1) -> bool {
    match standard_widget_slots(def, profile) {
        Ok(slots) => slots.iter().any(|s| !matches!(s, ProfileSlot::UiOnly { .. })),
        Err(_) => true,
    }
}

pub fn standard_widget_slots(
    def: &NodeDefinitionV1,
    profile: &SerializerProfileV1,
) -> Result<Vec<ProfileSlot>, LayoutGap> {
    if let Some(module) = def.python_module.as_deref() {
        if FRONTEND_REWRITTEN_CLASSES.iter().any(|(class, m)| *class == def.class_type && *m == module) {
            // Surface a definition gap first; the rewrite gap is only meaningful for a layout.
            standard_widget_slots_before_rewrite(def, profile)?;
            return Err(LayoutGap::FrontendExtensionRewrites { python_module: module.to_string() });
        }
    }
    standard_widget_slots_before_rewrite(def, profile)
}

/// The standard widget layout of `def` ignoring the frontend-rewrite gate. The binder uses it for
/// a rewritten class only after [`frontend_rewrite_is_identity`] proved that the extension would
/// serialize the saved values unchanged.
pub fn standard_widget_slots_before_rewrite(
    def: &NodeDefinitionV1,
    profile: &SerializerProfileV1,
) -> Result<Vec<ProfileSlot>, LayoutGap> {
    if profile.profile_version != STANDARD_PROFILE_V1 && !is_v2(&profile.profile_version) {
        return Err(LayoutGap::UnverifiedProfile {
            profile_version: profile.profile_version.clone(),
        });
    }
    if def.input_order.is_none() {
        return Err(LayoutGap::InputOrderMissing);
    }
    let mut slots = Vec::new();
    for input in def.inputs.iter().filter(|input| !input.hidden) {
        push_input_slots(&mut slots, input, None, is_v2(&profile.profile_version))?;
    }
    Ok(slots)
}

/// Slots of one DynamicCombo input (also used by special rules' `DynamicGroup`).
pub fn dynamic_combo_slot(input: &InputDef, prefix: Option<&str>, v2: bool) -> Result<ProfileSlot, LayoutGap> {
    let api_name = api_name(prefix, &input.name);
    let raw_options = input
        .options
        .get("options")
        .and_then(Value::as_array)
        .ok_or_else(|| LayoutGap::OptionMalformed {
            input: api_name.clone(),
            option: "options".to_string(),
        })?;
    let mut options = Vec::with_capacity(raw_options.len());
    for raw in raw_options {
        let malformed = || LayoutGap::OptionMalformed {
            input: api_name.clone(),
            option: "options".to_string(),
        };
        let key = raw.get("key").and_then(Value::as_str).ok_or_else(malformed)?;
        let inputs = raw.get("inputs").and_then(Value::as_object).ok_or_else(malformed)?;
        // Child order must be explicit (`inputs.input_order`); key order is not evidence.
        let order = match inputs.get("input_order") {
            Some(Value::Object(order)) => parse_input_order(order).map_err(|_| malformed())?,
            Some(_) => return Err(malformed()),
            None if option_has_no_children(inputs) => alloc::collections::BTreeMap::new(),
            None => {
                return Err(LayoutGap::DynamicChildOrderMissing {
                    input: api_name.clone(),
                    option_key: key.to_string(),
                })
            }
        };
        let mut children = Vec::new();
        for category in ["required", "optional"] {
            let section = match inputs.get(category) {
                None => None,
                Some(Value::Object(section)) => Some(section),
                Some(_) => return Err(malformed()),
            };
            let declared: alloc::collections::BTreeSet<&str> = match section {
                Some(section) => section.keys().map(String::as_str).collect(),
                None => alloc::collections::BTreeSet::new(),
            };
            let ordered: Vec<String> = match order.get(category) {
                Some(names) => names.clone(),
                None if declared.is_empty() => Vec::new(),
                None => return Err(malformed()),
            };
            let ordered_set: alloc::collections::BTreeSet<&str> =
                ordered.iter().map(String::as_str).collect();
            if ordered_set.len() != ordered.len() || ordered_set != declared {
                return Err(malformed());
            }
            for name in &ordered {
                let spec = section
                    .and_then(|s| s.get(name.as_str()))
                    .ok_or_else(malformed)?;
                let child = parse_input_spec(name, spec, category == "required", false)
                    .map_err(|_| malformed())?;
                push_input_slots(&mut children, &child, Some(api_name.as_str()), v2)?;
            }
        }
        options.push(DynamicComboOptionSlots {
            key: key.to_string(),
            children,
        });
    }
    Ok(ProfileSlot::DynamicCombo {
        api_name,
        def: input.clone(),
        options,
    })
}

/// An option whose `required`/`optional` sections are absent or empty has no child widgets.
fn option_has_no_children(inputs: &serde_json::Map<String, Value>) -> bool {
    ["required", "optional"].iter().all(|category| match inputs.get(*category) {
        None => true,
        Some(Value::Object(section)) => section.is_empty(),
        Some(_) => false,
    })
}

fn api_name(prefix: Option<&str>, name: &str) -> String {
    match prefix {
        Some(prefix) => format!("{prefix}.{name}"),
        None => name.to_string(),
    }
}

fn push_input_slots(
    slots: &mut Vec<ProfileSlot>,
    input: &InputDef,
    prefix: Option<&str>,
    v2: bool,
) -> Result<(), LayoutGap> {
    if input.force_input {
        return Ok(());
    }
    let name = api_name(prefix, &input.name);
    match input.type_name.as_str() {
        "INT" => {
            slots.push(widget(&name, input));
            let control = match input.options.get("control_after_generate") {
                // V2: DynamicCombo children get no implicit control widget (TextGenerate
                // `sampling_mode.seed` in the 1.53.10 widget dump).
                None => (!v2 || prefix.is_none()) && matches!(input.name.as_str(), "seed" | "noise_seed"),
                Some(Value::Bool(flag)) => *flag,
                Some(Value::String(_)) => true,
                Some(_) => {
                    return Err(LayoutGap::OptionMalformed {
                        input: name,
                        option: "control_after_generate".to_string(),
                    })
                }
            };
            if control {
                slots.push(control_slot());
            }
        }
        "FLOAT" | "BOOLEAN" | "STRING" | "CURVE" => slots.push(widget(&name, input)),
        "COMBO" => {
            let unverified: &[&str] = if v2 {
                &["animated_image_upload", "mesh_upload"]
            } else {
                &["video_upload", "audio_upload", "animated_image_upload", "mesh_upload"]
            };
            for upload in unverified.iter().copied() {
                if input.options.get(upload).is_some_and(|flag| flag != &Value::Bool(false)) {
                    return Err(LayoutGap::UnverifiedUploadWidget {
                        input: name,
                        option: upload.to_string(),
                    });
                }
            }
            slots.push(widget(&name, input));
            let control = match input.options.get("control_after_generate") {
                None => false,
                Some(Value::Bool(flag)) => *flag,
                Some(Value::String(_)) => true,
                Some(_) => {
                    return Err(LayoutGap::OptionMalformed {
                        input: name,
                        option: "control_after_generate".to_string(),
                    })
                }
            };
            if control {
                slots.push(control_slot());
            }
            let uploads: &[&str] = if v2 { &["image_upload", "video_upload", "audio_upload"] } else { &["image_upload"] };
            for upload in uploads.iter().copied() {
                match input.options.get(upload) {
                    None | Some(Value::Bool(false)) => {}
                    Some(Value::Bool(true)) => slots.push(ProfileSlot::UiOnly {
                        reason: upload.to_string(),
                        check: if v2 { UiOnlyCheck::FrontendButton } else { UiOnlyCheck::UploadControl },
                    }),
                    Some(_) => {
                        return Err(LayoutGap::OptionMalformed {
                            input: name,
                            option: upload.to_string(),
                        })
                    }
                }
            }
        }
        "COMFY_DYNAMICCOMBO_V3" => slots.push(dynamic_combo_slot(input, prefix, v2)?),
        "COMFY_AUTOGROW_V3" if v2 && autogrow_is_sockets_only(input) => {}
        // One value, serialized as saved (array values wrapped); 1.53.10 ImageCompare oracle. The
        // input is required by the server, so every profile must carry it.
        "IMAGECOMPARE" => slots.push(widget(&name, input)),
        other if is_frontend_widget_type(other) => {
            return Err(LayoutGap::UnsupportedWidgetType {
                input: name,
                type_name: other.to_string(),
            })
        }
        // V2: a multi-type input (`"FLOAT,INT"`) with an explicit `widgetType` gets that widget
        // (LTXVEmptyLatentAudio `frame_rate`, frontend 1.53.10 oracle CW38.14). Without
        // `widgetType` it stays a socket (ComfyNumberConvert `value`, 1.53.10 template oracle).
        multi if v2 && multi.contains(',') => {
            if let Some(widget_type) = input.options.get("widgetType") {
                let Some(widget_type) = widget_type.as_str().filter(|t| multi.split(',').any(|m| m == *t)) else {
                    return Err(LayoutGap::OptionMalformed { input: name, option: "widgetType".to_string() });
                };
                let mut single = input.clone();
                single.type_name = widget_type.to_string();
                push_input_slots(slots, &single, prefix, v2)?;
            }
        }
        // Socket types (MODEL, IMAGE, `*`, pack types without a widget constructor): no value.
        // A custom JS widget on such a type shows up as an unconsumed value and blocks.
        _ => {}
    }
    Ok(())
}

/// A `COMFY_AUTOGROW_V3` template whose every input is a socket type (no widget constructor):
/// the grown inputs are sockets (`values.a`, `ref_images.ref_image_0`) and carry no value.
fn autogrow_is_sockets_only(input: &InputDef) -> bool {
    let Some(template) = input.options.get("template").and_then(|t| t.get("input")).and_then(Value::as_object) else {
        return false;
    };
    let mut any = false;
    for (category, section) in template {
        if category != "required" && category != "optional" {
            return false;
        }
        let Some(section) = section.as_object() else { return false };
        for spec in section.values() {
            any = true;
            let Some(type_name) = spec.get(0).and_then(Value::as_str) else { return false };
            let widget = matches!(type_name, "INT" | "FLOAT" | "BOOLEAN" | "STRING" | "COMBO" | "CURVE")
                || type_name.starts_with("COMFY_")
                || is_frontend_widget_type(type_name);
            if widget {
                return false;
            }
        }
    }
    any
}

/// Frontend widget types the standard profile does not decode (custom constructors, V3 dynamic
/// growth). Sockets of these types would carry widget values, so they must not be skipped.
fn is_frontend_widget_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "COMFY_AUTOGROW_V3"
            | "COMFY_DYNAMICSLOT_V3"
            | "IMAGEUPLOAD"
            | "MARKDOWN"
            | "COLOR"
            | "BOUNDING_BOX"
            | "IMAGECOMPARE"
            | "SELECTBUTTON"
            | "TEXTAREA"
            | "LOAD3D"
            | "LOAD3D_ANIMATION"
    )
}

fn widget(api_name: &str, def: &InputDef) -> ProfileSlot {
    ProfileSlot::Widget {
        api_name: api_name.to_string(),
        def: def.clone(),
    }
}

fn control_slot() -> ProfileSlot {
    ProfileSlot::UiOnly {
        reason: "control_after_generate".to_string(),
        check: UiOnlyCheck::ControlMode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(name: &str, type_name: &str, options: Value) -> InputDef {
        InputDef {
            name: name.into(),
            type_name: type_name.into(),
            required: true,
            hidden: false,
            force_input: false,
            options,
        }
    }

    fn def(inputs: Vec<InputDef>, ordered: bool) -> NodeDefinitionV1 {
        NodeDefinitionV1 {
            class_type: "N".into(),
            input_order: ordered.then(|| inputs.iter().map(|i| i.name.clone()).collect()),
            inputs,
            outputs: vec![],
            python_module: None,
            extra: json!({}),
        }
    }

    fn profile() -> SerializerProfileV1 {
        standard_profile_for(Some("1.47.11")).expect("verified range")
    }

    fn shape(slots: &[ProfileSlot]) -> Vec<String> {
        slots
            .iter()
            .map(|slot| match slot {
                ProfileSlot::Widget { api_name, .. } => api_name.clone(),
                ProfileSlot::UiOnly { reason, .. } => format!("ui:{reason}"),
                ProfileSlot::DynamicCombo { api_name, .. } => format!("dyn:{api_name}"),
            })
            .collect()
    }

    #[test]
    fn profile_selection_covers_every_served_frontend_version() {
        assert_eq!(profile().profile_version, STANDARD_PROFILE_V1);
        assert_eq!(profile().frontend_version, "1.47.11");
        let pv = |v: Option<&str>| standard_profile_for(v).map(|p| p.profile_version);
        assert_eq!(pv(Some("1.53.10")), Some(STANDARD_PROFILE_V2.to_string()));
        for version in ["1.40.9", "1.45.21", "1.47", "v1.47.11", "1.47.11-rc1", "1.47.11.2"] {
            let expected = if version == "1.47" { STANDARD_PROFILE_V2 } else { STANDARD_PROFILE_V1 };
            assert_eq!(pv(Some(version)), Some(expected.to_string()), "{version}");
        }
        for version in [None, Some(""), Some("1.49.0"), Some("1.53.9"), Some("1.53.11"), Some("1.60.2")] {
            assert_eq!(pv(version), Some(STANDARD_PROFILE_V2.to_string()), "{version:?}");
        }
    }


    #[test]
    fn points_editor_rewrite_is_identity_only_on_its_fixed_point() {
        let mut d = def(vec![input("points_store", "STRING", json!({}))], true);
        d.class_type = "PointsEditor".into();
        d.python_module = Some("custom_nodes.ComfyUI-KJNodes".into());
        // Saved by the extension itself (certified Wan2.2 Animate original, node 229).
        let saved = [
            ("points_store", r#"{"positive":[{"x":248.528511937911,"y":262.9427464095323}],"negative":[]}"#),
            ("coordinates", r#"[{"x":248.528511937911,"y":262.9427464095323}]"#),
            ("neg_coordinates", "[]"),
            ("bbox_store", "[{}]"),
            ("bboxes", "[{}]"),
        ];
        let check_def = |d: &NodeDefinitionV1, over: &[(&str, &str)]| {
            let values: BTreeMap<String, Value> = saved
                .iter()
                .map(|(k, v)| (k.to_string(), json!(over.iter().find(|(o, _)| o == k).map_or(*v, |(_, v)| *v))))
                .collect();
            frontend_rewrite_is_identity(d, &values)
        };
        let check = |over: &[(&str, &str)]| check_def(&d, over);
        assert!(check(&[]));
        // CW38.23 oracle `valid`: bboxes is re-derived from bbox_store, so a different saved value is not a fixed point.
        assert!(!check(&[("bboxes", "[[5, 6, 50, 60]]")]));
        // Oracle `positive`: an unparsable store is replaced by the extension default.
        assert!(!check(&[("points_store", "cw38")]));
        assert!(!check(&[("points_store", r#"{"positive": [{"x":248.528511937911,"y":262.9427464095323}],"negative":[]}"#)]));
        assert!(!check(&[("coordinates", "[]")]));
        // JavaScript writes 100 for 100.0: not byte-identical.
        assert!(!check(&[
            ("points_store", r#"{"positive":[{"x":100.0,"y":2.5}],"negative":[]}"#),
            ("coordinates", r#"[{"x":100.0,"y":2.5}]"#),
        ]));
        assert!(check(&[
            ("points_store", r#"{"positive":[{"x":100,"y":2.5}],"negative":[{"x":1,"y":2}]}"#),
            ("coordinates", r#"[{"x":100,"y":2.5}]"#),
            ("neg_coordinates", r#"[{"x":1,"y":2}]"#),
        ]));
        // Box records with keys depend on JavaScript key order: unproven.
        assert!(!check(&[("bbox_store", r#"[{"startX":5}]"#), ("bboxes", r#"[{"startX":5}]"#)]));
        let mut other = d.clone();
        other.python_module = Some("custom_nodes.other".into());
        assert!(!check_def(&other, &[]));
    }

    #[test]
    fn missing_input_order_is_a_definition_gap() {
        let d = def(vec![input("steps", "INT", json!({}))], false);
        assert_eq!(standard_widget_slots(&d, &profile()), Err(LayoutGap::InputOrderMissing));
    }

    #[test]
    fn hand_built_profile_with_unknown_version_is_rejected() {
        let d = def(vec![input("steps", "INT", json!({}))], true);
        let fake = SerializerProfileV1 { frontend_version: "9.9.9".into(), profile_version: "guess".into() };
        assert!(matches!(standard_widget_slots(&d, &fake), Err(LayoutGap::UnverifiedProfile { .. })));
    }

    #[test]
    fn seed_control_hidden_force_input_and_optional_follow_frontend_rules() {
        let mut hidden = input("unique_id", "UNIQUE_ID", Value::Null);
        hidden.hidden = true;
        let mut forced = input("cfg_in", "FLOAT", json!({"forceInput": true}));
        forced.force_input = true;
        let mut optional = input("note", "STRING", json!({"multiline": true}));
        optional.required = false;
        let d = def(
            vec![
                input("model", "MODEL", Value::Null),
                input("seed", "INT", json!({})),
                input("steps", "INT", json!({})),
                input("noise_seed", "INT", json!({"control_after_generate": false})),
                input("variant", "INT", json!({"control_after_generate": "fixed"})),
                input("sampler", "COMBO", json!({"options": ["a"], "control_after_generate": true})),
                forced,
                input("image", "COMBO", json!({"options": [], "image_upload": true})),
                optional,
                hidden,
            ],
            true,
        );
        assert_eq!(
            shape(&standard_widget_slots(&d, &profile()).unwrap()),
            [
                "seed", "ui:control_after_generate", "steps", "noise_seed", "variant",
                "ui:control_after_generate", "sampler", "ui:control_after_generate", "image",
                "ui:image_upload", "note",
            ]
        );
    }

    #[test]
    fn unverified_upload_and_custom_widget_types_are_gaps() {
        let d = def(vec![input("video", "COMBO", json!({"video_upload": true}))], true);
        assert!(matches!(standard_widget_slots(&d, &profile()), Err(LayoutGap::UnverifiedUploadWidget { .. })));
        let d = def(vec![input("images", "COMFY_AUTOGROW_V3", json!({}))], true);
        assert!(matches!(standard_widget_slots(&d, &profile()), Err(LayoutGap::UnsupportedWidgetType { .. })));
        let d = def(vec![input("steps", "INT", json!({"control_after_generate": 1}))], true);
        assert!(matches!(standard_widget_slots(&d, &profile()), Err(LayoutGap::OptionMalformed { .. })));
    }

    #[test]
    fn dynamic_combo_children_need_explicit_order() {
        let ordered = input(
            "format",
            "COMFY_DYNAMICCOMBO_V3",
            json!({"options": [
                {"key": "png", "inputs": {
                    "required": {"depth": [["8", "16"]], "space": ["STRING"]},
                    "input_order": {"required": ["space", "depth"]}}},
                {"key": "raw", "inputs": {}}
            ]}),
        );
        let d = def(vec![input("prefix", "STRING", json!({})), ordered], true);
        let slots = standard_widget_slots(&d, &profile()).unwrap();
        assert_eq!(shape(&slots), ["prefix", "dyn:format"]);
        let ProfileSlot::DynamicCombo { options, .. } = &slots[1] else {
            panic!("dynamic combo slot expected");
        };
        assert_eq!(options[0].key, "png");
        assert_eq!(shape(&options[0].children), ["format.space", "format.depth"]);
        assert!(options[1].children.is_empty());

        let unordered = input(
            "format",
            "COMFY_DYNAMICCOMBO_V3",
            json!({"options": [{"key": "png", "inputs": {"required": {"depth": ["INT"]}}}]}),
        );
        let d = def(vec![unordered], true);
        assert_eq!(
            standard_widget_slots(&d, &profile()),
            Err(LayoutGap::DynamicChildOrderMissing { input: "format".into(), option_key: "png".into() })
        );
    }
}
