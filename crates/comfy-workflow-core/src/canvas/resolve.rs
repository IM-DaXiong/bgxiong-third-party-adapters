//! Resolve one evidenced serialization capability before compilation.

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};
use alloc::collections::BTreeMap;
use serde_json::Value;
use crate::node_registry::definition::NodeDefinitionV1;
use crate::node_registry::serializer_profile::{standard_profile_for, v2_serializes_widgets};
use crate::node_registry::special::{special_rule_for, PackageVersion};
use crate::{CompileDiagnostic, DiagnosticId, DiagnosticSeverity, NodeLocator};
use super::{bridge_evidence_digest, parse_to_ir, source_digest, CapabilityPlanV1, CompiledArtifactV1,
    CompileContextV1, GraphIrV1, IrNode, NodeMode, OutputSelection, ResolvedCapability, SourceFormat};

const NO_STANDARD_PROFILE: &str = "none";
mod codes {
    pub const DEFINITION_MISSING: &str = "workflowDefinitionMissing";
    pub const NEEDS_SERIALIZATION_EVIDENCE: &str = "workflowNeedsSerializationEvidence";
    pub const BRIDGE_EVIDENCE_NODE_UNCOVERED: &str = "workflowBridgeEvidenceNodeUncovered";
    pub const SERIALIZER_PROFILE_DRIFT: &str = "workflowSerializerProfileDrift";
}

pub(crate) fn diagnostic<const N: usize>(
    code: &str,
    locator: Option<&NodeLocator>,
    class_type: Option<&str>,
    params: [(&str, String); N],
) -> CompileDiagnostic {
    let where_ = match locator {
        Some(l) => format!("{}#{}", l.scope_path.join("/"), l.node_id),
        None => String::from("graph"),
    };
    CompileDiagnostic {
        diagnostic_id: DiagnosticId(format!("{code}@{where_}")),
        code: code.to_string(),
        severity: DiagnosticSeverity::Blocking,
        locator: locator.cloned(),
        class_type: class_type.map(str::to_string),
        input_name: None,
        params: params
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    }
}

/// Compile selected outputs from the same source and definition snapshot used for resolution.
/// Passing bridge evidence selects that exact route for the whole graph.
pub fn compile_with_evidence(
    raw: &[u8],
    mut context: CompileContextV1,
    selection: &OutputSelection,
) -> Result<CompiledArtifactV1, Vec<CompileDiagnostic>> {
    if let Some(evidence) = &context.bridge_evidence {
        super::bridge_evidence::verify_bridge_evidence(evidence, &context)?;
    }
    let ir = parse_to_ir(raw)?;
    context.capability_plan = Some(resolve_capabilities(&ir, &context)?);
    super::compile::compile_ir(&ir, raw, &context, selection)
}

/// Digest of one normalized class definition, the identity a special rule is pinned to.
pub fn class_definition_digest(def: &NodeDefinitionV1) -> String {
    let bytes = serde_json::to_vec(def).expect("NodeDefinitionV1 serializes to JSON");
    source_digest(&bytes)
}

/// Package identity of a definition; `None` when the server did not report `python_module`, in
/// which case no special rule can be pinned (a class name is not a package identity). Core
/// modules (`nodes`, `comfy_extras.*`, `comfy_api_nodes.*`) ship with the server, so their
/// version is the reported server version; custom node packs have no reported version.
fn package_of(def: &NodeDefinitionV1, server_version: Option<&str>) -> Option<PackageVersion> {
    def.python_module.as_ref().map(|module| {
        let core = module == "nodes" || module.starts_with("comfy_extras.") || module.starts_with("comfy_api_nodes.");
        PackageVersion {
            package: module.clone(),
            version: if core { server_version.map(str::to_string) } else { None },
            commit: None,
        }
    })
}

fn executes(node: &IrNode) -> bool {
    matches!(node.mode, NodeMode::Normal)
}

fn has_positional_widgets(node: &IrNode) -> bool {
    match &node.widgets_values {
        None | Some(Value::Null) => false,
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(_) => true,
    }
}

/// One evidenced capability per executable node, chosen before compile. No silent fallback:
///
/// - bridge evidence present (already verified by `verify_bridge_evidence`) => the whole graph is
///   bridge-compiled; every executable node must be covered by its SourceMap.
/// - API prompt / no positional widgets => `NamedApi` (named inputs are their own evidence).
/// - a reviewed special rule pinned to this package + definition digest => `SpecialRule`.
/// - a proven standard profile for the snapshot frontend AND explicit `input_order` => `StandardProfile`.
/// - otherwise a blocking `needsSerializationEvidence` diagnostic for that node.
pub fn resolve_capabilities(
    ir: &GraphIrV1,
    ctx: &CompileContextV1,
) -> Result<CapabilityPlanV1, Vec<CompileDiagnostic>> {
    let mut per_node = BTreeMap::new();
    let mut diagnostics = Vec::new();

    if let Some(ev) = &ctx.bridge_evidence {
        let digest = bridge_evidence_digest(ev).map_err(|d| vec![*d])?;
        for node in ir.nodes.iter().filter(|n| executes(n)) {
            let exec_id = node.key.exec_id();
            if ev.source_map.exec_to_source.contains_key(&exec_id) {
                per_node.insert(
                    exec_id,
                    ResolvedCapability::FrontendBridge {
                        evidence_digest: digest.clone(),
                    },
                );
            } else {
                diagnostics.push(diagnostic(
                    codes::BRIDGE_EVIDENCE_NODE_UNCOVERED,
                    Some(&node.source_locator),
                    Some(&node.class_type),
                    [("execId", exec_id)],
                ));
            }
        }
        return finish(per_node, diagnostics);
    }

    let profile = standard_profile_for(ctx.definitions.frontend_version.as_deref());
    let recorded = profile
        .as_ref()
        .map(|p| p.profile_version.as_str())
        .unwrap_or(NO_STANDARD_PROFILE);
    if recorded != ctx.serializer_profile_version {
        return Err(vec![diagnostic(
            codes::SERIALIZER_PROFILE_DRIFT,
            None,
            None,
            [
                ("expected", ctx.serializer_profile_version.clone()),
                ("actual", recorded.to_string()),
            ],
        )]);
    }

    for node in ir.nodes.iter().filter(|n| executes(n)) {
        let exec_id = node.key.exec_id();
        let v2_widgets = || {
            profile.as_ref().is_some_and(|p| {
                ctx.definitions
                    .classes
                    .get(&node.class_type)
                    .is_some_and(|def| v2_serializes_widgets(def, p))
            })
        };
        if matches!(ir.format, SourceFormat::ApiPrompt) || (!has_positional_widgets(node) && !v2_widgets()) {
            per_node.insert(exec_id, ResolvedCapability::NamedApi);
            continue;
        }
        let Some(def) = ctx.definitions.classes.get(&node.class_type) else {
            diagnostics.push(diagnostic(
                codes::DEFINITION_MISSING,
                Some(&node.source_locator),
                Some(&node.class_type),
                [("execId", exec_id)],
            ));
            continue;
        };
        let def_digest = class_definition_digest(def);
        let rule = package_of(def, ctx.definitions.server_version.as_deref())
            .and_then(|pkg| special_rule_for(&node.class_type, &pkg, &def_digest));
        if let Some(rule) = rule {
            per_node.insert(
                exec_id,
                ResolvedCapability::SpecialRule {
                    rule_id: rule.rule_id.to_string(),
                },
            );
            continue;
        }
        match (&profile, &def.input_order) {
            (Some(p), Some(_)) => {
                per_node.insert(
                    exec_id,
                    ResolvedCapability::StandardProfile {
                        profile_version: p.profile_version.clone(),
                    },
                );
            }
            (p, order) => diagnostics.push(diagnostic(
                codes::NEEDS_SERIALIZATION_EVIDENCE,
                Some(&node.source_locator),
                Some(&node.class_type),
                [
                    ("execId", exec_id),
                    ("standardProfile", p.is_some().to_string()),
                    ("inputOrder", order.is_some().to_string()),
                    ("definitionDigest", def_digest),
                ],
            )),
        }
    }
    finish(per_node, diagnostics)
}

fn finish(
    per_node: BTreeMap<String, ResolvedCapability>,
    diagnostics: Vec<CompileDiagnostic>,
) -> Result<CapabilityPlanV1, Vec<CompileDiagnostic>> {
    if diagnostics.is_empty() {
        Ok(CapabilityPlanV1 { per_node })
    } else {
        Err(diagnostics)
    }
}
