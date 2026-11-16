//! Single compile entry (CW23[L02]–CW27, CW37[L02]). Consumes an explicit `CapabilityPlanV1`:
//! each node's serialization source is chosen before compile by the host resolver; a failing
//! source is a blocking diagnostic, never a silent retry with another source.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec, boxed::Box};

use alloc::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::digest::{digest_json, digest_text};
use crate::node_registry::definition::{DefinitionSnapshotV1, NodeDefinitionV1};
use crate::node_registry::serializer_profile::{standard_profile_for, SerializerProfileV1};
use crate::node_registry::special::{all_rules, SpecialRuleV1};
use crate::types::{CompileDiagnostic, DiagnosticId, DiagnosticSeverity, NodeLocator};

use super::ir::{
    codes, is_blocking, parse_to_ir, Diag, GraphIrV1, InputValue, IrNode, NodeAttrsV1, NodeKey, NodeMode, OverrideSource,
};
use super::provenance::{
    account_widgets, ArtifactDigests, CompiledArtifactV1, InputProvenance, SourceMapV1, ValueOrigin,
};
use super::widgets::{apply_promoted_overrides, bind_widgets_with_defaults, WidgetConsumption};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ResolvedCapability {
    NamedApi,
    StandardProfile { profile_version: String },
    SpecialRule { rule_id: String },
    FrontendBridge { evidence_digest: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityPlanV1 {
    /// exec_id → chosen capability.
    pub per_node: BTreeMap<String, ResolvedCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeEvidenceV1 {
    pub source_digest: String,
    pub definition_digest: String,
    pub frontend_version: String,
    pub plugin_digest: String,
    pub api_prompt: Value,
    pub source_map: SourceMapV1,
    pub consumed_widgets: Vec<WidgetConsumption>,
    /// Owner exec id (bridge numbering, resolved through `source_map`) of each
    /// `consumed_widgets` entry; same length. Added by L02 so consumption is attributable.
    pub consumed_widget_owners: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompileContextV1 {
    pub source_format_version: String,
    pub source_digest: String,
    pub definitions: DefinitionSnapshotV1,
    pub serializer_profile_version: String,
    pub rule_set_version: u32,
    pub bridge_evidence: Option<BridgeEvidenceV1>,
    pub capability_plan: Option<CapabilityPlanV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputSelection {
    pub output_keys: Vec<NodeKey>,
}

/// `sha256:<hex>` over the exact source bytes. Hosts must sign `CompileContextV1::source_digest`
/// with this function.
pub fn source_digest(raw: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(raw)))
}

/// Digest that `ResolvedCapability::FrontendBridge::evidence_digest` must carry.
pub fn bridge_evidence_digest(ev: &BridgeEvidenceV1) -> Result<String, Box<CompileDiagnostic>> {
    let value = serde_json::to_value(ev)
        .map_err(|e| Diag::blocking(codes::DIGEST_FAILED).param("detail", e).build())?;
    digest_json(&value).map_err(|e| Diag::blocking(codes::DIGEST_FAILED).param("detail", e.detail).build().into())
}

pub fn compile(
    raw: &[u8],
    ctx: &CompileContextV1,
    sel: &OutputSelection,
) -> Result<CompiledArtifactV1, Vec<CompileDiagnostic>> {
    let ir = parse_to_ir(raw)?;
    compile_ir(&ir, raw, ctx, sel)
}

fn locator_in(set: &BTreeSet<NodeKey>, loc: &NodeLocator) -> bool {
    set.contains(&NodeKey { scope: loc.scope_path.clone(), local_id: loc.node_id.clone() })
}

/// Pruned-branch findings stay visible but do not block the selected outputs.
fn downgrade_outside_closure(mut d: CompileDiagnostic) -> CompileDiagnostic {
    d.severity = DiagnosticSeverity::NonBlocking;
    d.params.insert(codes::OUTSIDE_SELECTED_CLOSURE.to_string(), "true".to_string());
    d.diagnostic_id = DiagnosticId(format!("{}|{}", d.diagnostic_id.0, codes::OUTSIDE_SELECTED_CLOSURE));
    d
}

struct Closure {
    order: Vec<NodeKey>,
    members: BTreeSet<NodeKey>,
}

fn output_closure(
    index: &BTreeMap<NodeKey, &IrNode>,
    outputs: &[NodeKey],
    diags: &mut Vec<CompileDiagnostic>,
) -> Closure {
    // 0 = visiting, 1 = done. Post-order gives dependencies first.
    let mut state: BTreeMap<NodeKey, u8> = BTreeMap::new();
    let mut order = Vec::new();
    fn visit(
        key: &NodeKey,
        index: &BTreeMap<NodeKey, &IrNode>,
        state: &mut BTreeMap<NodeKey, u8>,
        order: &mut Vec<NodeKey>,
        diags: &mut Vec<CompileDiagnostic>,
    ) {
        match state.get(key).copied() {
            Some(1) => return,
            Some(_) => {
                diags.push(Diag::blocking(codes::EXECUTION_CYCLE).at(key).build());
                return;
            }
            None => {}
        }
        let Some(node) = index.get(key) else { return };
        state.insert(key.clone(), 0);
        for (name, input) in &node.inputs {
            let InputValue::Link(link) = input else { continue };
            match index.get(&link.from) {
                None => diags.push(
                    Diag::blocking(codes::LINK_ORIGIN_MISSING)
                        .at(key)
                        .class(&node.class_type)
                        .input(name)
                        .param("origin", link.from.exec_id())
                        .build(),
                ),
                Some(src) if src.mode != NodeMode::Normal => diags.push(
                    Diag::blocking(codes::LINK_TO_INACTIVE)
                        .at(key)
                        .class(&node.class_type)
                        .input(name)
                        .param("origin", link.from.exec_id())
                        .build(),
                ),
                Some(_) => visit(&link.from, index, state, order, diags),
            }
        }
        state.insert(key.clone(), 1);
        order.push(key.clone());
    }
    for out in outputs {
        visit(out, index, &mut state, &mut order, diags);
    }
    let members = order.iter().cloned().collect();
    Closure { order, members }
}

struct NodeResult {
    inputs: Map<String, Value>,
    provenance: Vec<InputProvenance>,
}

fn link_value(link: &super::ir::LinkRef) -> Value {
    json!([link.from.exec_id(), link.output_slot])
}

fn override_origin(source: &OverrideSource) -> ValueOrigin {
    match source {
        OverrideSource::Promoted { instance } => ValueOrigin::PromotedOverride { instance: instance.clone() },
        OverrideSource::Primitive { node } => ValueOrigin::PrimitiveWidget { node: node.clone() },
    }
}

fn base_links(node: &IrNode, out: &mut NodeResult) {
    for (name, input) in &node.inputs {
        if let InputValue::Link(link) = input {
            out.inputs.insert(name.clone(), link_value(link));
            out.provenance.push(InputProvenance {
                exec_id: node.key.exec_id(),
                input_name: name.clone(),
                origin: ValueOrigin::Link,
            });
        }
    }
}

fn compile_named_api(node: &IrNode, attrs: &NodeAttrsV1, diags: &mut Vec<CompileDiagnostic>) -> NodeResult {
    let mut out = NodeResult { inputs: Map::new(), provenance: Vec::new() };
    base_links(node, &mut out);
    for (name, input) in &node.inputs {
        if let InputValue::Literal(v) = input {
            out.inputs.insert(name.clone(), v.clone());
            out.provenance.push(InputProvenance {
                exec_id: node.key.exec_id(),
                input_name: name.clone(),
                origin: ValueOrigin::NamedApi,
            });
        }
    }
    diags.extend(account_widgets(&node.key, &node.class_type, node.widgets_values.as_ref(), &[]));
    for o in &attrs.overrides {
        diags.push(
            Diag::blocking(codes::OVERRIDE_TARGET_UNUSED)
                .at(&node.key)
                .class(&node.class_type)
                .input(&o.input_name)
                .build(),
        );
    }
    out
}

fn compile_widgets(
    node: &IrNode,
    attrs: &NodeAttrsV1,
    def: &NodeDefinitionV1,
    profile: &SerializerProfileV1,
    special: Option<&SpecialRuleV1>,
    diags: &mut Vec<CompileDiagnostic>,
) -> NodeResult {
    let exec_id = node.key.exec_id();
    let mut out = NodeResult { inputs: Map::new(), provenance: Vec::new() };
    base_links(node, &mut out);
    let mut local = node.clone();
    if !attrs.overrides.is_empty() {
        let pairs: Vec<(String, Value)> =
            attrs.overrides.iter().map(|o| (o.input_name.clone(), o.value.clone())).collect();
        if let Err(d) = apply_promoted_overrides(&mut local, &pairs) {
            diags.push(*d);
            return out;
        }
    }
    let override_by_name: BTreeMap<&str, &OverrideSource> =
        attrs.overrides.iter().map(|o| (o.input_name.as_str(), &o.source)).collect();
    // Literal inputs on the local copy: promoted/primitive overrides (written by
    // `apply_promoted_overrides`) and API-style named values already present in a flat UI dict.
    for (name, input) in &local.inputs {
        if let InputValue::Literal(v) = input {
            let origin = match override_by_name.get(name.as_str()) {
                Some(source) => override_origin(source),
                None => ValueOrigin::NamedApi,
            };
            out.inputs.insert(name.clone(), v.clone());
            out.provenance.push(InputProvenance { exec_id: exec_id.clone(), input_name: name.clone(), origin });
        }
    }
    let (named, consumptions, defaulted) = match bind_widgets_with_defaults(&local, def, profile, special) {
        Ok(r) => r,
        Err(d) => {
            diags.push(*d);
            return out;
        }
    };
    diags.extend(account_widgets(&node.key, &node.class_type, local.widgets_values.as_ref(), &consumptions));
    let mut emitted = BTreeSet::new();
    for (name, value) in named {
        if out.inputs.contains_key(&name) || !emitted.insert(name.clone()) {
            diags.push(
                Diag::blocking(codes::INPUT_DOUBLE_SOURCE).at(&node.key).class(&node.class_type).input(&name).build(),
            );
            continue;
        }
        let widget_index = consumptions.iter().find_map(|c| match c {
            WidgetConsumption::NamedInput { index, name: n } if *n == name => Some(*index),
            _ => None,
        });
        let origin = if let Some(index) = widget_index {
            ValueOrigin::SourceWidget { index }
        } else if defaulted.contains(&name)
            || def
            .inputs
            .iter()
            .find(|i| i.name == name)
            .and_then(|i| i.options.get("default"))
            .is_some_and(|d| *d == value)
        {
            ValueOrigin::DefinitionDefault
        } else {
            diags.push(
                Diag::blocking(codes::VALUE_ORIGIN_UNKNOWN).at(&node.key).class(&node.class_type).input(&name).build(),
            );
            continue;
        };
        out.provenance.push(InputProvenance { exec_id: exec_id.clone(), input_name: name.clone(), origin });
        out.inputs.insert(name, value);
    }
    for o in &attrs.overrides {
        if !out.inputs.contains_key(&o.input_name) {
            diags.push(
                Diag::blocking(codes::OVERRIDE_TARGET_UNUSED)
                    .at(&node.key)
                    .class(&node.class_type)
                    .input(&o.input_name)
                    .build(),
            );
        }
    }
    out
}

fn locator_eq(a: &NodeLocator, b: &NodeLocator) -> bool {
    a.scope_path == b.scope_path && a.node_id == b.node_id
}

fn compile_bridge(
    node: &IrNode,
    ev: &BridgeEvidenceV1,
    diags: &mut Vec<CompileDiagnostic>,
) -> NodeResult {
    let exec_id = node.key.exec_id();
    let mut out = NodeResult { inputs: Map::new(), provenance: Vec::new() };
    let bridge_id = ev
        .source_map
        .exec_to_source
        .iter()
        .find(|(_, loc)| locator_eq(loc, &node.source_locator))
        .map(|(id, _)| id.clone());
    let Some(bridge_id) = bridge_id else {
        diags.push(Diag::blocking(codes::BRIDGE_NODE_MISSING).at(&node.key).class(&node.class_type).build());
        return out;
    };
    let Some(bnode) = ev.api_prompt.get(&bridge_id).and_then(Value::as_object) else {
        diags.push(Diag::blocking(codes::BRIDGE_NODE_MISSING).at(&node.key).class(&node.class_type).build());
        return out;
    };
    if bnode.get("class_type").and_then(Value::as_str) != Some(node.class_type.as_str()) {
        diags.push(
            Diag::blocking(codes::BRIDGE_EVIDENCE_MISMATCH)
                .at(&node.key)
                .class(&node.class_type)
                .param("field", "classType")
                .build(),
        );
        return out;
    }
    let Some(binputs) = bnode.get("inputs").and_then(Value::as_object) else {
        diags.push(
            Diag::blocking(codes::BRIDGE_EVIDENCE_MISMATCH).at(&node.key).class(&node.class_type).param("field", "inputs").build(),
        );
        return out;
    };
    let bridge_to_key = |id: &str| -> Option<NodeKey> {
        ev.source_map
            .exec_to_source
            .get(id)
            .map(|loc| NodeKey { scope: loc.scope_path.clone(), local_id: loc.node_id.clone() })
    };
    for (name, value) in binputs {
        let as_link = value.as_array().filter(|a| a.len() == 2).and_then(|a| Some((a[0].as_str()?, a[1].as_u64()?)));
        match (as_link, node.inputs.get(name)) {
            (Some((src, slot)), Some(InputValue::Link(link))) => {
                if bridge_to_key(src).as_ref() != Some(&link.from) || u64::from(link.output_slot) != slot {
                    diags.push(Diag::blocking(codes::BRIDGE_LINK_MISMATCH).at(&node.key).class(&node.class_type).input(name).build());
                    continue;
                }
                out.inputs.insert(name.clone(), link_value(link));
                out.provenance.push(InputProvenance { exec_id: exec_id.clone(), input_name: name.clone(), origin: ValueOrigin::Link });
            }
            (None, None) => {
                out.inputs.insert(name.clone(), value.clone());
                out.provenance.push(InputProvenance {
                    exec_id: exec_id.clone(),
                    input_name: name.clone(),
                    origin: ValueOrigin::BridgeSerialized,
                });
            }
            _ => diags.push(Diag::blocking(codes::BRIDGE_LINK_MISMATCH).at(&node.key).class(&node.class_type).input(name).build()),
        }
    }
    for (name, input) in &node.inputs {
        if matches!(input, InputValue::Link(_)) && !binputs.contains_key(name) {
            diags.push(Diag::blocking(codes::BRIDGE_LINK_MISMATCH).at(&node.key).class(&node.class_type).input(name).build());
        }
    }
    let own: Vec<WidgetConsumption> = ev
        .consumed_widgets
        .iter()
        .zip(&ev.consumed_widget_owners)
        .filter(|(_, owner)| **owner == bridge_id)
        .map(|(c, _)| c.clone())
        .collect();
    diags.extend(account_widgets(&node.key, &node.class_type, node.widgets_values.as_ref(), &own));
    out
}

fn check_bridge(ctx: &CompileContextV1, evidence_digest: &str, key: &NodeKey, diags: &mut Vec<CompileDiagnostic>) -> bool {
    let Some(ev) = &ctx.bridge_evidence else {
        diags.push(Diag::blocking(codes::BRIDGE_EVIDENCE_MISSING).at(key).build());
        return false;
    };
    let digest = match bridge_evidence_digest(ev) {
        Ok(d) => d,
        Err(d) => {
            diags.push(*d);
            return false;
        }
    };
    let checks = [
        ("evidenceDigest", digest == evidence_digest),
        ("sourceDigest", ev.source_digest == ctx.source_digest),
        ("definitionDigest", ev.definition_digest == ctx.definitions.digest),
        ("consumedWidgetOwners", ev.consumed_widgets.len() == ev.consumed_widget_owners.len()),
    ];
    let mut ok = true;
    for (field, pass) in checks {
        if !pass {
            diags.push(Diag::blocking(codes::BRIDGE_EVIDENCE_MISMATCH).at(key).param("field", field).build());
            ok = false;
        }
    }
    ok
}

fn resolve_profile(
    ctx: &CompileContextV1,
    key: &NodeKey,
    class_type: &str,
    diags: &mut Vec<CompileDiagnostic>,
) -> Option<SerializerProfileV1> {
    match standard_profile_for(ctx.definitions.frontend_version.as_deref()) {
        Some(p) if p.profile_version == ctx.serializer_profile_version => Some(p),
        Some(p) => {
            diags.push(
                Diag::blocking(codes::CAPABILITY_MISMATCH)
                    .at(key)
                    .class(class_type)
                    .param("profileVersion", p.profile_version)
                    .param("contextProfileVersion", &ctx.serializer_profile_version)
                    .build(),
            );
            None
        }
        None => {
            diags.push(
                Diag::blocking(codes::SERIALIZER_PROFILE_UNAVAILABLE)
                    .at(key)
                    .class(class_type)
                    .param(
                        "frontendVersion",
                        ctx.definitions.frontend_version.clone().unwrap_or_else(|| "unknown".to_string()),
                    )
                    .build(),
            );
            None
        }
    }
}

pub(crate) fn compile_ir(
    ir: &GraphIrV1,
    raw: &[u8],
    ctx: &CompileContextV1,
    sel: &OutputSelection,
) -> Result<CompiledArtifactV1, Vec<CompileDiagnostic>> {
    let mut diags: Vec<CompileDiagnostic> = Vec::new();
    let actual_digest = source_digest(raw);
    if actual_digest != ctx.source_digest {
        diags.push(Diag::blocking(codes::SOURCE_DIGEST_MISMATCH).param("actual", &actual_digest).build());
    }
    if !ctx.definitions.complete {
        diags.push(Diag::blocking(codes::DEFINITIONS_INCOMPLETE).build());
    }
    let Some(plan) = &ctx.capability_plan else {
        diags.push(Diag::blocking(codes::CAPABILITY_PLAN_MISSING).build());
        return Err(diags);
    };
    if sel.output_keys.is_empty() {
        diags.push(Diag::blocking(codes::OUTPUT_SELECTION_EMPTY).build());
        return Err(diags);
    }
    let index: BTreeMap<NodeKey, &IrNode> = ir.nodes.iter().map(|n| (n.key.clone(), n)).collect();
    let mut outputs = Vec::new();
    for key in &sel.output_keys {
        let Some(node) = index.get(key) else {
            diags.push(Diag::blocking(codes::OUTPUT_NOT_FOUND).at(key).build());
            continue;
        };
        if node.mode != NodeMode::Normal {
            diags.push(Diag::blocking(codes::OUTPUT_INACTIVE).at(key).class(&node.class_type).build());
            continue;
        }
        let is_output = ctx
            .definitions
            .classes
            .get(&node.class_type)
            .and_then(|d| d.extra.get("output_node"))
            .and_then(Value::as_bool);
        if is_output != Some(true) {
            diags.push(Diag::blocking(codes::OUTPUT_NOT_OUTPUT_NODE).at(key).class(&node.class_type).build());
            continue;
        }
        outputs.push(key.clone());
    }
    let closure = output_closure(&index, &outputs, &mut diags);
    for d in &ir.diagnostics {
        match &d.locator {
            Some(loc) if !locator_in(&closure.members, loc) && is_blocking(d) => {
                diags.push(downgrade_outside_closure(d.clone()))
            }
            _ => diags.push(d.clone()),
        }
    }

    let mut api = Map::new();
    let mut provenance = Vec::new();
    let mut source_map = SourceMapV1::default();
    for key in &closure.order {
        let node = index[key];
        let exec_id = key.exec_id();
        let Some(def) = ctx.definitions.classes.get(&node.class_type) else {
            diags.push(Diag::blocking(codes::DEFINITION_MISSING).at(key).class(&node.class_type).build());
            continue;
        };
        let Some(cap) = plan.per_node.get(&exec_id) else {
            diags.push(Diag::blocking(codes::CAPABILITY_MISSING).at(key).class(&node.class_type).build());
            continue;
        };
        let Some(attrs) = ir.node_attrs.get(&exec_id) else {
            diags.push(Diag::blocking(codes::NODE_INVALID).at(key).class(&node.class_type).param("field", "attrs").build());
            continue;
        };
        let result = match cap {
            ResolvedCapability::NamedApi => compile_named_api(node, attrs, &mut diags),
            ResolvedCapability::StandardProfile { profile_version } => {
                let Some(profile) = resolve_profile(ctx, key, &node.class_type, &mut diags) else { continue };
                if &profile.profile_version != profile_version {
                    diags.push(
                        Diag::blocking(codes::CAPABILITY_MISMATCH)
                            .at(key)
                            .class(&node.class_type)
                            .param("planProfileVersion", profile_version)
                            .build(),
                    );
                    continue;
                }
                compile_widgets(node, attrs, def, &profile, None, &mut diags)
            }
            ResolvedCapability::SpecialRule { rule_id } => {
                let Some(rule) = all_rules().iter().find(|r| r.rule_id == rule_id.as_str()) else {
                    diags.push(
                        Diag::blocking(codes::SPECIAL_RULE_NOT_FOUND).at(key).class(&node.class_type).param("ruleId", rule_id).build(),
                    );
                    continue;
                };
                if rule.class_type != node.class_type {
                    diags.push(
                        Diag::blocking(codes::CAPABILITY_MISMATCH).at(key).class(&node.class_type).param("ruleId", rule_id).build(),
                    );
                    continue;
                }
                let Some(profile) = resolve_profile(ctx, key, &node.class_type, &mut diags) else { continue };
                compile_widgets(node, attrs, def, &profile, Some(rule), &mut diags)
            }
            ResolvedCapability::FrontendBridge { evidence_digest } => {
                if !check_bridge(ctx, evidence_digest, key, &mut diags) {
                    continue;
                }
                let Some(ev) = &ctx.bridge_evidence else { continue };
                compile_bridge(node, ev, &mut diags)
            }
        };
        let mut entry = Map::new();
        entry.insert("class_type".into(), Value::String(node.class_type.clone()));
        entry.insert("inputs".into(), Value::Object(result.inputs));
        if let Some(title) = &attrs.title {
            entry.insert("_meta".into(), json!({ "title": title }));
        }
        api.insert(exec_id.clone(), Value::Object(entry));
        provenance.extend(result.provenance);
        source_map.exec_to_source.insert(exec_id, node.source_locator.clone());
    }

    let retained: Vec<String> = closure.order.iter().map(NodeKey::exec_id).collect();
    let pruned: Vec<(String, String)> = ir
        .nodes
        .iter()
        .filter(|n| !closure.members.contains(&n.key))
        .map(|n| {
            let reason = match n.mode {
                NodeMode::Muted => "muted",
                NodeMode::Bypassed => "bypassed",
                NodeMode::Normal => "notInOutputClosure",
            };
            (n.key.exec_id(), reason.to_string())
        })
        .collect();

    let api_prompt = Value::Object(api);
    let graph = match digest_json(&api_prompt) {
        Ok(d) => d,
        Err(e) => {
            diags.push(Diag::blocking(codes::DIGEST_FAILED).param("detail", e.detail).build());
            String::new()
        }
    };
    if diags.iter().any(is_blocking) {
        return Err(diags);
    }
    Ok(CompiledArtifactV1 {
        api_prompt,
        source_map,
        inputs: provenance,
        retained,
        pruned,
        diagnostics: diags,
        digests: ArtifactDigests {
            graph,
            definition: ctx.definitions.digest.clone(),
            serializer: digest_text(&ctx.serializer_profile_version),
            rules: digest_text(&format!("rules:v{}", ctx.rule_set_version)),
        },
    })
}
