//! Widget binding for canvas nodes (PLAN CW22[L03], CW26[L03], CW31, CW32).
//!
//! `bind_widgets` turns a node's `widgets_values` into named API inputs using exactly one proven
//! layout: a reviewed special rule (`node_registry::special`, chosen by the host via
//! `special_rule_for`) or the verified standard serializer profile. Every value position is
//! consumed as a named input, consumed by a linked widget, proven UI-only, or reported as
//! `Blocked`. Positions are never inferred from a value's type or from object key order; a value
//! whose type contradicts its proven slot is a blocking diagnostic, not a hint to shift slots.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec, boxed::Box};

use alloc::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::canvas::ir::{InputValue, IrNode};
use crate::node_registry::definition::{blocking_diagnostic, codes, params, InputDef, NodeDefinitionV1};
use crate::node_registry::serializer_profile::{
    dynamic_combo_slot, frontend_rewrite_is_identity, is_v2, standard_widget_slots,
    standard_widget_slots_before_rewrite, LayoutGap, ProfileSlot, SerializerProfileV1,
    UiOnlyCheck, CONTROL_MODES,
};
use crate::node_registry::special::{SpecialRuleV1, SpecialSlot};
use crate::types::CompileDiagnostic;

/// How one `widgets_values` entry was consumed. For object-form `widgets_values` the index is
/// the ordinal of the key in canonical (sorted) key order and carries no positional meaning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum WidgetConsumption {
    NamedInput { index: u32, name: String },
    UiOnly { index: u32, reason: String },
    Linked { index: u32, name: String },
    Blocked { index: u32, diagnostic: CompileDiagnostic },
}

/// Bound `(api input name, value)` pairs plus how each saved widget value was consumed.
pub type WidgetBinding = (Vec<(String, Value)>, Vec<WidgetConsumption>);

/// [`WidgetBinding`] plus the names whose value is the frontend widget default.
pub type WidgetBindingWithDefaults = (Vec<(String, Value)>, Vec<WidgetConsumption>, BTreeSet<String>);

/// Named literal inputs produced from `widgets_values`, plus the consumption of every entry.
/// Inputs the node already carries (links, API named values, promoted overrides) are never
/// overwritten and are not repeated in the returned list.
pub fn bind_widgets(
    node: &IrNode,
    def: &NodeDefinitionV1,
    profile: &SerializerProfileV1,
    special: Option<&SpecialRuleV1>,
) -> Result<WidgetBinding, Box<CompileDiagnostic>> {
    bind_widgets_with_defaults(node, def, profile, special).map(|(out, consumed, _)| (out, consumed))
}

/// [`bind_widgets`] plus the names whose value is the frontend widget default (V2 widgets the
/// saved values do not reach); their provenance is `DefinitionDefault`.
pub fn bind_widgets_with_defaults(
    node: &IrNode,
    def: &NodeDefinitionV1,
    profile: &SerializerProfileV1,
    special: Option<&SpecialRuleV1>,
) -> Result<WidgetBindingWithDefaults, Box<CompileDiagnostic>> {
    if def.class_type != node.class_type {
        return Err(Box::new(node_diagnostic(
            node,
            codes::SPECIAL_RULE_MISMATCH,
            None,
            params([("reason", "definitionClassMismatch".into()), ("definitionClass", def.class_type.clone())]),
        )));
    }
    if let Some(rule) = special {
        if rule.class_type != node.class_type {
            return Err(Box::new(node_diagnostic(
                node,
                codes::SPECIAL_RULE_MISMATCH,
                None,
                params([("reason", "ruleClassMismatch".into()), ("ruleId", rule.rule_id.to_string())]),
            )));
        }
    }
    let v2 = is_v2(&profile.profile_version);
    let mut binder = Binder {
        node,
        out: Vec::new(),
        consumed: Vec::new(),
        v2,
        defaulted: BTreeSet::new(),
    };
    // Every frontend builds a node's widgets from its definition (initial value = definition
    // default) and `configure` only assigns the saved values, so a node saved without values
    // (`widgets_values` absent or `[]`) still serializes its widgets' defaults: ImageCompare
    // `compare_view` in the 1.53.10 oracle. This holds under every profile, not one version.
    let no_values: Vec<Value> = Vec::new();
    let positional = match &node.widgets_values {
        None => Some(&no_values),
        Some(Value::Array(values)) if values.is_empty() => Some(&no_values),
        Some(Value::Array(values)) => Some(values),
        _ => None,
    };
    match (&node.widgets_values, positional) {
        (_, Some(values)) => {
            check_len(node, values.len())?;
            let slots = match special {
                Some(rule) => special_slots(node, def, rule, v2)?,
                None => standard_slots_for_values(node, def, profile, values)?,
            };
            let mut cursor = 0usize;
            binder.consume(&slots, values, &mut cursor)?;
            for index in cursor..values.len() {
                if v2 {
                    // LGraphNode.configure assigns saved values to the serialized widgets in
                    // order and drops the surplus (values saved by another frontend layout).
                    binder.consumed.push(WidgetConsumption::UiOnly {
                        index: to_index(index),
                        reason: "ignoredByFrontend".to_string(),
                    });
                    continue;
                }
                let diagnostic = node_diagnostic(
                    node,
                    codes::WIDGET_VALUE_UNCONSUMED,
                    None,
                    params([("index", index.to_string()), ("valueCount", values.len().to_string())]),
                );
                binder.consumed.push(WidgetConsumption::Blocked {
                    index: to_index(index),
                    diagnostic,
                });
            }
        }
        (None, None) | (Some(Value::Array(_)), None) => {}
        (Some(Value::Object(named)), None) => {
            check_len(node, named.len())?;
            binder.bind_named(def, special, named)
        }
        (Some(_), None) => {
            return Err(Box::new(node_diagnostic(
                node,
                codes::WIDGET_VALUE_INVALID,
                None,
                params([("reason", "widgetsValuesShape".into())]),
            )))
        }
    }
    Ok((binder.out, binder.consumed, binder.defaulted))
}

/// Write promoted subgraph-instance widget values onto `local`, a per-instance copy of an
/// interior node (never the shared definition). Each override becomes an explicit literal
/// input, so bind_widgets keeps it over the interior widget value: absent override = interior
/// value/default, explicit `null` = explicit null, linked target = rejected. All-or-nothing.
pub fn apply_promoted_overrides(
    local: &mut IrNode,
    overrides: &[(String, Value)],
) -> Result<(), Box<CompileDiagnostic>> {
    let mut seen = BTreeSet::new();
    for (name, _) in overrides {
        let reason = if name.is_empty() {
            Some("emptyInputName")
        } else if !seen.insert(name.as_str()) {
            Some("duplicateOverride")
        } else if matches!(local.inputs.get(name), Some(InputValue::Link(_))) {
            Some("targetLinked")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(Box::new(node_diagnostic(
                local,
                codes::PROMOTED_OVERRIDE_INVALID,
                Some(name.as_str()),
                params([("reason", reason.into())]),
            )));
        }
    }
    for (name, value) in overrides {
        local
            .inputs
            .insert(name.clone(), InputValue::Literal(wrap_array_literal(value)));
    }
    Ok(())
}

/// Literal form of a widget value for `def`. Arrays are literals (official `{"__value__": [...]}`
/// wrapping), never links; links come only from graph links. Scalar types are checked against
/// the definition; explicit `null` is preserved as an explicit null.
pub fn parse_literal(value: &Value, def: &InputDef) -> Result<InputValue, Box<CompileDiagnostic>> {
    (literal_value(value, def).map(InputValue::Literal).map_err(|reason| {
        blocking_diagnostic(
            codes::LITERAL_INVALID,
            None,
            None,
            Some(def.name.as_str()),
            params([("reason", reason.into()), ("typeName", def.type_name.clone())]),
        )
    })).map_err(Box::new)
}

fn literal_value(value: &Value, def: &InputDef) -> Result<Value, &'static str> {
    if def.hidden {
        return Err("hiddenInputLiteral");
    }
    if value.is_null() {
        return Ok(Value::Null);
    }
    let wrapped = match value {
        Value::Object(map) if map.contains_key("__value__") => {
            if map.len() != 1 || !map["__value__"].is_array() {
                return Err("valueWrapperMalformed");
            }
            Some(value.clone())
        }
        Value::Array(_) => Some(wrap_array_literal(value)),
        _ => None,
    };
    let scalar_ok = match def.type_name.as_str() {
        "INT" => value.is_i64() || value.is_u64(),
        "FLOAT" => value.is_number(),
        "BOOLEAN" => value.is_boolean(),
        "STRING" => value.is_string(),
        "COMBO" => {
            let multi = def.options.get("multi_select").is_some_and(|m| !m.is_null() && m != &Value::Bool(false));
            match value {
                Value::String(_) | Value::Number(_) | Value::Bool(_) => true,
                Value::Array(_) => multi,
                _ => wrapped.is_some() && multi,
            }
        }
        // Non-scalar widget types (CURVE and pack types): arrays wrapped, objects verbatim.
        _ => return Ok(wrapped.unwrap_or_else(|| value.clone())),
    };
    if !scalar_ok {
        return Err("typeMismatch");
    }
    Ok(wrapped.unwrap_or_else(|| value.clone()))
}

/// Initial value the 1.53.10 frontend widget constructor gives a widget type whose definition has
/// no default (each entry follows the frontend widget constructors). `IMAGECOMPARE`: `["", ""]`, serialized
/// wrapped (ImageCompare `compare_view`, Qwen 2.1 edit userdata workflow).
fn frontend_initial_value(type_name: &str) -> Option<Value> {
    match type_name {
        "IMAGECOMPARE" => Some(wrap_array_literal(&json!(["", ""]))),
        // String widget constructor: `inputSpec.default ?? ""`.
        "STRING" => Some(json!("")),
        _ => None,
    }
}

/// The frontend holds widget values as JavaScript numbers, so an integral saved value or
/// `/object_info` default such as `1.0` serializes as `1` (MiniMaxH3TurboLoRA `strength` in the
/// 1.53.10 oracle).
fn js_number(value: Value) -> Value {
    match value.as_f64() {
        Some(f) if value.is_f64() && f % 1.0 == 0.0 && f.abs() <= 9_007_199_254_740_991.0 => Value::from(f as i64),
        _ => value,
    }
}

fn wrap_array_literal(value: &Value) -> Value {
    match value {
        Value::Array(_) => json!({ "__value__": value }),
        _ => value.clone(),
    }
}

struct Binder<'a> {
    node: &'a IrNode,
    out: Vec<(String, Value)>,
    consumed: Vec<WidgetConsumption>,
    /// Frontend 1.53.10 rules (`STANDARD_PROFILE_V2`): missing trailing values take defaults.
    v2: bool,
    defaulted: BTreeSet<String>,
}

impl Binder<'_> {
    fn consume(&mut self, slots: &[ProfileSlot], values: &[Value], cursor: &mut usize) -> Result<(), Box<CompileDiagnostic>> {
        let node = self.node;
        for slot in slots {
            let index = *cursor;
            // Missing trailing values keep the widget default (V2), and so does a node saved
            // with no values at all under any profile.
            if index >= values.len() && (self.v2 || values.is_empty()) {
                self.default_slot(slot)?;
                continue;
            }
            let Some(value) = values.get(index) else {
                return Err(Box::new(node_diagnostic(
                    self.node,
                    codes::WIDGET_COUNT_MISMATCH,
                    None,
                    params([("index", index.to_string()), ("valueCount", values.len().to_string())]),
                )));
            };
            *cursor += 1;
            match slot {
                ProfileSlot::Widget { api_name, def } => self.bind_value(index, api_name, def, value),
                ProfileSlot::UiOnly { reason, check } => {
                    let proven = match check {
                        // V2: the frontend assigns any saved value to the control widget and never
                        // serializes it (FlashVSR_SM_KSampler `4` in the 1.53.10 oracle).
                        UiOnlyCheck::ControlMode => {
                            self.v2 || value.as_str().is_some_and(|mode| CONTROL_MODES.contains(&mode))
                        }
                        UiOnlyCheck::UploadControl => value.is_string(),
                        UiOnlyCheck::FrontendButton => true,
                        UiOnlyCheck::DeclaredBySpecialRule => true,
                    };
                    if proven {
                        self.consumed.push(WidgetConsumption::UiOnly {
                            index: to_index(index),
                            reason: reason.clone(),
                        });
                    } else {
                        let diagnostic = node_diagnostic(
                            self.node,
                            codes::WIDGET_VALUE_INVALID,
                            None,
                            params([("index", index.to_string()), ("reason", format!("uiOnly:{reason}"))]),
                        );
                        self.consumed.push(WidgetConsumption::Blocked { index: to_index(index), diagnostic });
                    }
                }
                ProfileSlot::DynamicCombo { api_name, def: _, options } => {
                    let selected = match node.inputs.get(api_name) {
                        Some(InputValue::Link(_)) => {
                            return Err(Box::new(node_diagnostic(
                                self.node,
                                codes::SERIALIZER_EVIDENCE_REQUIRED,
                                Some(api_name.as_str()),
                                params([("reason", "dynamicComboSelectorLinked".into())]),
                            )))
                        }
                        Some(InputValue::Literal(named)) => {
                            self.consumed.push(WidgetConsumption::NamedInput { index: to_index(index), name: api_name.clone() });
                            named
                        }
                        None => {
                            self.consumed.push(WidgetConsumption::NamedInput { index: to_index(index), name: api_name.clone() });
                            self.out.push((api_name.clone(), value.clone()));
                            value
                        }
                    };
                    let option = options
                        .iter()
                        .find(|option| selected.as_str() == Some(option.key.as_str()))
                        .ok_or_else(|| {
                            node_diagnostic(
                                self.node,
                                codes::DYNAMIC_COMBO_OPTION_UNKNOWN,
                                Some(api_name.as_str()),
                                params([("index", index.to_string()), ("value", selected.to_string())]),
                            )
                        })?;
                    self.consume(&option.children, values, cursor)?;
                }
            }
        }
        Ok(())
    }

    /// V2: a widget the saved values do not reach keeps its frontend default, which is the
    /// definition `default` (the widget constructors' initial value). Linked or named inputs
    /// already have a source; UI-only widgets have no API value.
    fn default_slot(&mut self, slot: &ProfileSlot) -> Result<(), Box<CompileDiagnostic>> {
        let node = self.node;
        match slot {
            ProfileSlot::UiOnly { .. } => Ok(()),
            ProfileSlot::Widget { api_name, def } => {
                if node.inputs.contains_key(api_name) {
                    return Ok(());
                }
                let value = self.definition_default(api_name, def)?;
                self.out.push((api_name.clone(), value));
                self.defaulted.insert(api_name.clone());
                Ok(())
            }
            ProfileSlot::DynamicCombo { api_name, def, options } => {
                let selected = match node.inputs.get(api_name) {
                    Some(InputValue::Literal(v)) => v.clone(),
                    Some(InputValue::Link(_)) => {
                        return Err(Box::new(node_diagnostic(
                            node,
                            codes::SERIALIZER_EVIDENCE_REQUIRED,
                            Some(api_name.as_str()),
                            params([("reason", "dynamicComboSelectorLinked".into())]),
                        )))
                    }
                    None => {
                        let v = self.definition_default(api_name, def)?;
                        self.out.push((api_name.clone(), v.clone()));
                        self.defaulted.insert(api_name.clone());
                        v
                    }
                };
                let option = options
                    .iter()
                    .find(|o| selected.as_str() == Some(o.key.as_str()))
                    .ok_or_else(|| {
                        node_diagnostic(
                            node,
                            codes::DYNAMIC_COMBO_OPTION_UNKNOWN,
                            Some(api_name.as_str()),
                            params([("value", selected.to_string())]),
                        )
                    })?;
                for child in &option.children {
                    self.default_slot(child)?;
                }
                Ok(())
            }
        }
    }

    /// Frontend widget initial value: the definition `default`; a COMBO / DynamicCombo without
    /// one starts on its first option (SaveVideo `format` / `format.codec` in the 1.53.10 dump).
    fn definition_default(&self, api_name: &str, def: &InputDef) -> Result<Value, Box<CompileDiagnostic>> {
        let first_option = || {
            let first = def.options.get("options").and_then(Value::as_array).and_then(|o| o.first())?;
            match def.type_name.as_str() {
                "COMBO" => Some(first.clone()),
                "COMFY_DYNAMICCOMBO_V3" => first.get("key").cloned(),
                _ => None,
            }
        };
        (def.options
            .get("default")
            .cloned()
            .or_else(first_option)
            .map(js_number)
            .or_else(|| frontend_initial_value(&def.type_name))
            .ok_or_else(|| {
            node_diagnostic(
                self.node,
                codes::WIDGET_COUNT_MISMATCH,
                Some(api_name),
                params([("reason", "missingValueWithoutDefault".into())]),
            )
        })).map_err(Box::new)
    }

    fn bind_value(&mut self, index: usize, api_name: &str, def: &InputDef, value: &Value) {
        let index32 = to_index(index);
        let node = self.node;
        match node.inputs.get(api_name) {
            Some(InputValue::Link(_)) => self.consumed.push(WidgetConsumption::Linked {
                index: index32,
                name: api_name.to_string(),
            }),
            Some(InputValue::Literal(_)) => self.consumed.push(WidgetConsumption::NamedInput {
                index: index32,
                name: api_name.to_string(),
            }),
            None => match literal_value(value, def) {
                Ok(literal) => {
                    // V2: the frontend re-serializes parsed JavaScript numbers (`1.0` -> `1`).
                    let literal = if self.v2 { js_number(literal) } else { literal };
                    self.out.push((api_name.to_string(), literal));
                    self.consumed.push(WidgetConsumption::NamedInput {
                        index: index32,
                        name: api_name.to_string(),
                    });
                }
                // V2: the frontend assigns saved values to widgets verbatim (no type check on
                // load) and serializes them as they are; the server validates the prompt. Oracle:
                // FlashVSR_SM_KSampler INT `scale` <- 3.5 (1.53.10).
                Err("typeMismatch") if self.v2 => {
                    self.out.push((api_name.to_string(), js_number(wrap_array_literal(value))));
                    self.consumed.push(WidgetConsumption::NamedInput {
                        index: index32,
                        name: api_name.to_string(),
                    });
                }
                Err(reason) => {
                    let diagnostic = node_diagnostic(
                        self.node,
                        codes::WIDGET_VALUE_INVALID,
                        Some(api_name),
                        params([("index", index.to_string()), ("reason", reason.into()), ("typeName", def.type_name.clone())]),
                    );
                    self.consumed.push(WidgetConsumption::Blocked { index: index32, diagnostic });
                }
            },
        }
    }

    /// Object-form `widgets_values` (name-keyed). Keys bind to definition inputs by name; a key
    /// that is not a definition input is UI-only only if the special rule declares it.
    fn bind_named(&mut self, def: &NodeDefinitionV1, special: Option<&SpecialRuleV1>, named: &serde_json::Map<String, Value>) {
        let declared_ui_only: BTreeSet<&str> = special
            .map(|rule| {
                rule.widget_slots
                    .iter()
                    .filter_map(|slot| match slot {
                        SpecialSlot::UiOnly(key) => Some(*key),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let ordered: BTreeMap<&String, &Value> = named.iter().collect();
        for (ordinal, (key, value)) in ordered.into_iter().enumerate() {
            match def.input(key) {
                Some(input) if !input.hidden && !input.force_input => self.bind_value(ordinal, key, input, value),
                _ if declared_ui_only.contains(key.as_str()) => self.consumed.push(WidgetConsumption::UiOnly {
                    index: to_index(ordinal),
                    reason: key.clone(),
                }),
                _ => {
                    let diagnostic = node_diagnostic(
                        self.node,
                        codes::WIDGET_VALUE_UNCONSUMED,
                        Some(key.as_str()),
                        params([("reason", "namedWidgetNotInDefinition".into())]),
                    );
                    self.consumed.push(WidgetConsumption::Blocked { index: to_index(ordinal), diagnostic });
                }
            }
        }
    }
}

/// Standard slots of `def`; a class whose frontend extension rewrites its values gets them only
/// when the saved values are a proven fixed point of that rewrite (`frontend_rewrite_is_identity`).
fn standard_slots_for_values(
    node: &IrNode,
    def: &NodeDefinitionV1,
    profile: &SerializerProfileV1,
    values: &[Value],
) -> Result<Vec<ProfileSlot>, Box<CompileDiagnostic>> {
    match standard_widget_slots(def, profile) {
        Err(gap @ LayoutGap::FrontendExtensionRewrites { .. }) => {
            let slots = standard_widget_slots_before_rewrite(def, profile).map_err(|g| gap_diagnostic(node, g))?;
            // Value index == slot index only while every slot holds exactly one value.
            let single = slots.iter().all(|s| matches!(s, ProfileSlot::Widget { .. } | ProfileSlot::UiOnly { .. }));
            let named: BTreeMap<String, Value> = slots
                .iter()
                .zip(values)
                .filter_map(|(slot, value)| match slot {
                    ProfileSlot::Widget { api_name, .. } => Some((api_name.clone(), value.clone())),
                    _ => None,
                })
                .collect();
            if single && frontend_rewrite_is_identity(def, &named) {
                Ok(slots)
            } else {
                Err(Box::new(gap_diagnostic(node, gap)))
            }
        }
        other => other.map_err(|gap| Box::new(gap_diagnostic(node, gap))),
    }
}

/// Slots of a reviewed special rule: `Named(input)` binds that definition input, `UiOnly(reason)`
/// consumes one UI-only value, `DynamicGroup(input)` decodes that DynamicCombo input.
fn special_slots(node: &IrNode, def: &NodeDefinitionV1, rule: &SpecialRuleV1, v2: bool) -> Result<Vec<ProfileSlot>, Box<CompileDiagnostic>> {
    let mismatch = |input: &str, reason: &str| {
        node_diagnostic(
            node,
            codes::SPECIAL_RULE_MISMATCH,
            Some(input),
            params([("reason", reason.into()), ("ruleId", rule.rule_id.to_string())]),
        )
    };
    let value_count = match &node.widgets_values {
        Some(Value::Array(values)) => values.len(),
        _ => 0,
    };
    let frontend_input = |name: String| ProfileSlot::Widget {
        api_name: name.clone(),
        def: InputDef {
            name,
            type_name: "*".to_string(),
            required: false,
            hidden: false,
            force_input: false,
            options: json!({}),
        },
    };
    let mut slots = Vec::new();
    for (position, slot) in rule.widget_slots.iter().enumerate() {
        match slot {
            SpecialSlot::Named(name) => match def.input(name) {
                Some(input) if !input.hidden => slots.push(ProfileSlot::Widget {
                    api_name: name.to_string(),
                    def: input.clone(),
                }),
                _ => return Err(Box::new(mismatch(name, "namedInputNotInDefinition"))),
            },
            SpecialSlot::UiOnly(reason) => slots.push(ProfileSlot::UiOnly {
                reason: reason.to_string(),
                check: UiOnlyCheck::DeclaredBySpecialRule,
            }),
            SpecialSlot::DynamicGroup(name) => match def.input(name) {
                Some(input) if input.type_name == "COMFY_DYNAMICCOMBO_V3" => {
                    slots.push(dynamic_combo_slot(input, None, v2).map_err(|gap| gap_diagnostic(node, gap))?)
                }
                _ => return Err(Box::new(mismatch(name, "dynamicGroupNotDynamicCombo"))),
            },
            SpecialSlot::FrontendInput(name) => slots.push(frontend_input(name.to_string())),
            SpecialSlot::FrontendSequence(prefix) => {
                let fixed = rule.widget_slots[..position]
                    .iter()
                    .all(|s| !matches!(s, SpecialSlot::DynamicGroup(_) | SpecialSlot::FrontendSequence(_)));
                if !fixed || position + 1 != rule.widget_slots.len() {
                    return Err(Box::new(mismatch(prefix, "frontendSequenceNotLast")));
                }
                for ordinal in 1..=value_count.saturating_sub(position) {
                    slots.push(frontend_input(format!("{prefix}{ordinal}")));
                }
            }
        }
    }
    Ok(slots)
}

fn gap_diagnostic(node: &IrNode, gap: LayoutGap) -> CompileDiagnostic {
    let (code, input, p) = match gap {
        LayoutGap::InputOrderMissing => (codes::INPUT_ORDER_MISSING, None, params([("reason", "inputOrderMissing".into())])),
        LayoutGap::UnverifiedProfile { profile_version } => (
            codes::SERIALIZER_EVIDENCE_REQUIRED,
            None,
            params([("reason", "unverifiedProfile".into()), ("profileVersion", profile_version)]),
        ),
        LayoutGap::UnsupportedWidgetType { input, type_name } => (
            codes::SERIALIZER_EVIDENCE_REQUIRED,
            Some(input),
            params([("reason", "unsupportedWidgetType".into()), ("typeName", type_name)]),
        ),
        LayoutGap::UnverifiedUploadWidget { input, option } => (
            codes::SERIALIZER_EVIDENCE_REQUIRED,
            Some(input),
            params([("reason", "unverifiedUploadWidget".into()), ("option", option)]),
        ),
        LayoutGap::OptionMalformed { input, option } => (
            codes::DEFINITION_MALFORMED,
            Some(input),
            params([("reason", "optionMalformed".into()), ("option", option)]),
        ),
        LayoutGap::DynamicChildOrderMissing { input, option_key } => (
            codes::SERIALIZER_EVIDENCE_REQUIRED,
            Some(input),
            params([("reason", "dynamicChildOrderMissing".into()), ("optionKey", option_key)]),
        ),
        LayoutGap::FrontendExtensionRewrites { python_module } => (
            codes::SERIALIZER_EVIDENCE_REQUIRED,
            None,
            params([("reason", "frontendExtensionRewritesValues".into()), ("pythonModule", python_module)]),
        ),
    };
    node_diagnostic(node, code, input.as_deref(), p)
}

fn node_diagnostic(node: &IrNode, code: &str, input: Option<&str>, params: BTreeMap<String, String>) -> CompileDiagnostic {
    blocking_diagnostic(code, Some(node.source_locator.clone()), Some(node.class_type.as_str()), input, params)
}

/// Callers only pass indices below a length already checked against `u32::MAX`.
fn to_index(index: usize) -> u32 {
    index as u32
}

fn check_len(node: &IrNode, len: usize) -> Result<(), Box<CompileDiagnostic>> {
    if u32::try_from(len).is_err() {
        return Err(Box::new(node_diagnostic(
            node,
            codes::WIDGET_VALUE_INVALID,
            None,
            params([("reason", "widgetsValuesTooLong".into()), ("valueCount", len.to_string())]),
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ir::{LinkRef, NodeKey, NodeMode};
    use crate::node_registry::definition::normalize_object_info;
    use crate::node_registry::serializer_profile::standard_profile_for;
    use crate::types::NodeLocator;

    fn object_info() -> Value {
        json!({
            "Sampler": {
                "input": {
                    "required": {
                        "model": ["MODEL"],
                        "seed": ["INT", {"default": 0}],
                        "steps": ["INT", {"default": 20}],
                        "cfg": ["FLOAT", {"default": 8.0}],
                        "sampler_name": [["euler", "dpmpp_2m"]],
                        "denoise": ["FLOAT", {"forceInput": true}]
                    },
                    "optional": {"text": ["STRING", {"multiline": true}]},
                    "hidden": {"unique_id": "UNIQUE_ID"}
                },
                // Deliberately not lexicographic.
                "input_order": {
                    "required": ["steps", "model", "seed", "denoise", "sampler_name", "cfg"],
                    "optional": ["text"],
                    "hidden": ["unique_id"]
                },
                "output": ["LATENT"]
            },
            "Saver": {
                "input": {"required": {
                    "prefix": ["STRING"],
                    "format": ["COMFY_DYNAMICCOMBO_V3", {"options": [
                        {"key": "png", "inputs": {
                            "required": {"bit_depth": [["8-bit", "16-bit"]], "space": ["STRING"]},
                            "input_order": {"required": ["bit_depth", "space"]}}},
                        {"key": "avif", "inputs": {
                            "required": {"crf": ["INT"]},
                            "input_order": {"required": ["crf"]}}}
                    ]}]
                }},
                "input_order": {"required": ["prefix", "format"]},
                "output": []
            },
            "NoOrder": {
                "input": {"required": {"a": ["INT"], "b": ["INT"]}},
                "output": []
            },
            "Curve": {
                "input": {"required": {"curve": ["CURVE"], "pick": ["COMBO", {"options": ["x", "y"], "multi_select": true}]}},
                "input_order": {"required": ["curve", "pick"]},
                "output": []
            }
        })
    }

    fn def(class_type: &str) -> NodeDefinitionV1 {
        let classes = [class_type.to_string()].into_iter().collect();
        normalize_object_info(&object_info(), &classes)
            .expect("fixture definitions")
            .classes
            .remove(class_type)
            .expect("requested class")
    }

    fn profile() -> SerializerProfileV1 {
        standard_profile_for(Some("1.47.11")).expect("verified profile")
    }

    fn link(from: &str) -> InputValue {
        InputValue::Link(LinkRef {
            from: NodeKey { scope: vec![], local_id: from.into() },
            output_slot: 0,
            output_name: None,
            output_type: None,
        })
    }

    fn node(class_type: &str, widgets: Value, inputs: Vec<(&str, InputValue)>) -> IrNode {
        IrNode {
            key: NodeKey { scope: vec![], local_id: "7".into() },
            class_type: class_type.into(),
            mode: NodeMode::Normal,
            inputs: inputs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            widgets_values: Some(widgets),
            source_locator: NodeLocator { scope_path: vec![], node_id: "7".into() },
        }
    }

    fn kinds(consumed: &[WidgetConsumption]) -> Vec<String> {
        consumed
            .iter()
            .map(|c| match c {
                WidgetConsumption::NamedInput { index, name } => format!("{index}:{name}"),
                WidgetConsumption::UiOnly { index, reason } => format!("{index}:ui:{reason}"),
                WidgetConsumption::Linked { index, name } => format!("{index}:link:{name}"),
                WidgetConsumption::Blocked { index, diagnostic } => format!("{index}:blocked:{}", diagnostic.code),
            })
            .collect()
    }

    fn sampler_widgets() -> Value {
        json!([20, 42, "fixed", "euler", 7.5, "line 1\n  line 2 "])
    }

    #[test]
    fn standard_profile_binds_by_input_order_with_seed_control_and_multiline() {
        let n = node("Sampler", sampler_widgets(), vec![("model", link("1"))]);
        let (out, consumed) = bind_widgets(&n, &def("Sampler"), &profile(), None).unwrap();
        assert_eq!(
            out,
            vec![
                ("steps".to_string(), json!(20)),
                ("seed".to_string(), json!(42)),
                ("sampler_name".to_string(), json!("euler")),
                ("cfg".to_string(), json!(7.5)),
                ("text".to_string(), json!("line 1\n  line 2 ")),
            ]
        );
        assert_eq!(
            kinds(&consumed),
            ["0:steps", "1:seed", "2:ui:control_after_generate", "3:sampler_name", "4:cfg", "5:text"]
        );
    }

    #[test]
    fn linked_and_named_widgets_keep_their_slot_without_overwrite() {
        let n = node(
            "Sampler",
            sampler_widgets(),
            vec![("steps", link("3")), ("cfg", InputValue::Literal(json!(3.0)))],
        );
        let (out, consumed) = bind_widgets(&n, &def("Sampler"), &profile(), None).unwrap();
        assert!(out.iter().all(|(name, _)| name != "steps" && name != "cfg"));
        assert_eq!(kinds(&consumed)[0], "0:link:steps");
        assert_eq!(kinds(&consumed)[4], "4:cfg");
        assert_eq!(out.iter().find(|(n, _)| n == "seed").map(|(_, v)| v), Some(&json!(42)));
    }

    #[test]
    fn explicit_null_is_kept_and_missing_values_are_not_defaulted() {
        let n = node("Sampler", json!([null, 42, "fixed", "euler", 7.5, ""]), vec![]);
        let (out, _) = bind_widgets(&n, &def("Sampler"), &profile(), None).unwrap();
        assert_eq!(out[0], ("steps".to_string(), Value::Null));

        let short = node("Sampler", json!([20, 42, "fixed"]), vec![]);
        let err = bind_widgets(&short, &def("Sampler"), &profile(), None).unwrap_err();
        assert_eq!(err.code, codes::WIDGET_COUNT_MISMATCH);
        assert_eq!(err.params["index"], "3");
    }

    #[test]
    fn extra_values_and_type_contradictions_block_instead_of_shifting() {
        let mut values = sampler_widgets();
        values.as_array_mut().unwrap().push(json!("extra"));
        let n = node("Sampler", values, vec![]);
        let (_, consumed) = bind_widgets(&n, &def("Sampler"), &profile(), None).unwrap();
        assert_eq!(kinds(&consumed).last().unwrap(), &format!("6:blocked:{}", codes::WIDGET_VALUE_UNCONSUMED));

        // Seed and control swapped: a string in the INT slot is a contradiction, not a cue to realign.
        let swapped = node("Sampler", json!([20, "fixed", 42, "euler", 7.5, ""]), vec![]);
        let (out, consumed) = bind_widgets(&swapped, &def("Sampler"), &profile(), None).unwrap();
        let k = kinds(&consumed);
        assert_eq!(k[1], format!("1:blocked:{}", codes::WIDGET_VALUE_INVALID));
        assert_eq!(k[2], format!("2:blocked:{}", codes::WIDGET_VALUE_INVALID));
        assert!(out.iter().all(|(name, _)| name != "seed"));
    }

    #[test]
    fn missing_input_order_requires_definitions() {
        let n = node("NoOrder", json!([1, 2]), vec![]);
        let err = bind_widgets(&n, &def("NoOrder"), &profile(), None).unwrap_err();
        assert_eq!(err.code, codes::INPUT_ORDER_MISSING);
        assert_eq!(err.locator.as_ref().map(|l| l.node_id.as_str()), Some("7"));
    }

    #[test]
    fn empty_or_absent_widgets_serialize_definition_defaults_under_every_profile() {
        // The frontend builds widgets from the definition and serializes their initial values
        // even when nothing was saved (2026-10-15 ImageCompare `compare_view` regression), so an
        // empty node is bound like any other: defaults out, no saved value consumed.
        for widgets in [Some(json!([])), None] {
            let mut n = node("Sampler", json!([]), vec![("model", link("1"))]);
            n.widgets_values = widgets.clone();
            let (out, consumed) = bind_widgets(&n, &def("Sampler"), &profile(), None).unwrap();
            assert!(consumed.is_empty(), "{widgets:?}");
            let names: Vec<&str> = out.iter().map(|(name, _)| name.as_str()).collect();
            assert!(names.contains(&"steps") && names.contains(&"cfg"), "{widgets:?}: {names:?}");
            // Without a layout the defaults cannot be proven: surfaced, never silently skipped.
            let mut bare = node("NoOrder", json!([]), vec![]);
            bare.widgets_values = widgets;
            let err = bind_widgets(&bare, &def("NoOrder"), &profile(), None).unwrap_err();
            assert_eq!(err.code, codes::INPUT_ORDER_MISSING);
        }
    }

    #[test]
    fn dynamic_combo_consumes_selected_option_children_only() {
        let png = node("Saver", json!(["p", "png", "16-bit", "sRGB"]), vec![]);
        let (out, consumed) = bind_widgets(&png, &def("Saver"), &profile(), None).unwrap();
        assert_eq!(
            out,
            vec![
                ("prefix".to_string(), json!("p")),
                ("format".to_string(), json!("png")),
                ("format.bit_depth".to_string(), json!("16-bit")),
                ("format.space".to_string(), json!("sRGB")),
            ]
        );
        assert_eq!(consumed.len(), 4);

        let avif = node("Saver", json!(["p", "avif", 30]), vec![]);
        let (out, _) = bind_widgets(&avif, &def("Saver"), &profile(), None).unwrap();
        assert_eq!(out[2], ("format.crf".to_string(), json!(30)));

        let unknown = node("Saver", json!(["p", "webp", 90]), vec![]);
        let err = bind_widgets(&unknown, &def("Saver"), &profile(), None).unwrap_err();
        assert_eq!(err.code, codes::DYNAMIC_COMBO_OPTION_UNKNOWN);

        let linked = node("Saver", json!(["p", "png", "8-bit", "s"]), vec![("format", link("2"))]);
        let err = bind_widgets(&linked, &def("Saver"), &profile(), None).unwrap_err();
        assert_eq!(err.code, codes::SERIALIZER_EVIDENCE_REQUIRED);
    }

    static SAMPLER_RULE: SpecialRuleV1 = SpecialRuleV1 {
        rule_id: "test.sampler.v1",
        class_type: "Sampler",
        package: "test-pack",
        version: "1.0.0",
        definition_digest: "sha256:test",
        widget_slots: &[SpecialSlot::Named("cfg"), SpecialSlot::UiOnly("preview"), SpecialSlot::Named("seed")],
        notes: "",
    };

    #[test]
    fn special_rule_layout_replaces_standard_profile() {
        let n = node("Sampler", json!([6.0, {"any": "ui"}, 9]), vec![]);
        let (out, consumed) = bind_widgets(&n, &def("Sampler"), &profile(), Some(&SAMPLER_RULE)).unwrap();
        assert_eq!(out, vec![("cfg".to_string(), json!(6.0)), ("seed".to_string(), json!(9))]);
        assert_eq!(kinds(&consumed), ["0:cfg", "1:ui:preview", "2:seed"]);

        let other = node("Saver", json!(["p"]), vec![]);
        let err = bind_widgets(&other, &def("Saver"), &profile(), Some(&SAMPLER_RULE)).unwrap_err();
        assert_eq!(err.code, codes::SPECIAL_RULE_MISMATCH);
    }

    #[test]
    fn object_widgets_bind_by_name_and_block_unknown_keys() {
        let n = node("Sampler", json!({"steps": 12, "preview": {"x": 1}, "mystery": 1}), vec![]);
        let (out, consumed) = bind_widgets(&n, &def("Sampler"), &profile(), Some(&SAMPLER_RULE)).unwrap();
        assert_eq!(out, vec![("steps".to_string(), json!(12))]);
        assert_eq!(
            kinds(&consumed),
            [format!("0:blocked:{}", codes::WIDGET_VALUE_UNCONSUMED), "1:ui:preview".to_string(), "2:steps".to_string()]
        );
    }

    #[test]
    fn promoted_overrides_touch_only_the_local_copy() {
        let shared = node("Sampler", sampler_widgets(), vec![("model", link("1"))]);
        let mut first = shared.clone();
        let mut second = shared.clone();
        apply_promoted_overrides(&mut first, &[("seed".into(), json!(1))]).unwrap();
        apply_promoted_overrides(&mut second, &[("seed".into(), Value::Null), ("text".into(), json!(["a", 1]))]).unwrap();
        assert!(!shared.inputs.contains_key("seed"));
        assert!(matches!(first.inputs.get("seed"), Some(InputValue::Literal(v)) if v == &json!(1)));
        assert!(matches!(second.inputs.get("seed"), Some(InputValue::Literal(Value::Null))));
        assert!(matches!(second.inputs.get("text"), Some(InputValue::Literal(v)) if v == &json!({"__value__": ["a", 1]})));

        // The override wins over the interior widget value and the slot is still consumed.
        let (out, consumed) = bind_widgets(&first, &def("Sampler"), &profile(), None).unwrap();
        assert!(out.iter().all(|(name, _)| name != "seed"));
        assert_eq!(kinds(&consumed)[1], "1:seed");

        let mut rejected = shared.clone();
        let err = apply_promoted_overrides(&mut rejected, &[("seed".into(), json!(1)), ("model".into(), json!(2))]).unwrap_err();
        assert_eq!(err.code, codes::PROMOTED_OVERRIDE_INVALID);
        assert!(!rejected.inputs.contains_key("seed"), "rejected override set must not partially apply");
        let err = apply_promoted_overrides(&mut rejected, &[("seed".into(), json!(1)), ("seed".into(), json!(2))]).unwrap_err();
        assert_eq!(err.params["reason"], "duplicateOverride");
    }

    #[test]
    fn parse_literal_wraps_arrays_and_never_makes_links() {
        let curve = def("Curve");
        let curve_input = curve.input("curve").unwrap();
        let points = json!(["5", 0]);
        assert!(matches!(parse_literal(&points, curve_input), Ok(InputValue::Literal(v)) if v == json!({"__value__": ["5", 0]})));
        let wrapped = json!({"__value__": [[0.0, 0.0], [1.0, 1.0]]});
        assert!(matches!(parse_literal(&wrapped, curve_input), Ok(InputValue::Literal(v)) if v == wrapped));
        let object = json!({"points": [[0, 0]], "interp": "linear"});
        assert!(matches!(parse_literal(&object, curve_input), Ok(InputValue::Literal(v)) if v == object));
        assert!(parse_literal(&json!({"__value__": 3}), curve_input).is_err());
        assert!(parse_literal(&json!({"__value__": [1], "x": 1}), curve_input).is_err());

        let pick = curve.input("pick").unwrap();
        assert!(matches!(parse_literal(&json!(["x", "y"]), pick), Ok(InputValue::Literal(v)) if v == json!({"__value__": ["x", "y"]})));

        let sampler = def("Sampler");
        let steps = sampler.input("steps").unwrap();
        assert!(parse_literal(&json!(20.5), steps).is_err());
        assert!(parse_literal(&json!(["1", 0]), steps).is_err());
        assert!(matches!(parse_literal(&Value::Null, steps), Ok(InputValue::Literal(Value::Null))));
        let hidden = sampler.input("unique_id").unwrap();
        assert_eq!(parse_literal(&json!(1), hidden).unwrap_err().code, codes::LITERAL_INVALID);
    }

    #[test]
    fn consumption_serializes_as_tagged_camel_case() {
        let value = serde_json::to_value(WidgetConsumption::UiOnly { index: 2, reason: "control_after_generate".into() }).unwrap();
        assert_eq!(value, json!({"kind": "uiOnly", "index": 2, "reason": "control_after_generate"}));
        let back: WidgetConsumption = serde_json::from_value(json!({"kind": "namedInput", "index": 0, "name": "seed"})).unwrap();
        assert!(matches!(back, WidgetConsumption::NamedInput { index: 0, ref name } if name == "seed"));
    }
}
