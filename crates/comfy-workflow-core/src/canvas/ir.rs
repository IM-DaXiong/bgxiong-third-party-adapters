//! Canvas graph IR (CW10[L02], CW19, CW20): scoped node identity, strict Literal/LinkRef split,
//! node modes, per-instance overrides and source locators.
//!
//! `parse_to_ir` is the single parser for every supported source format. Executable structure it
//! cannot prove is reported as a structured diagnostic; nothing is defaulted (no id/slot `0`).

#[cfg(target_arch = "wasm32")]
use alloc::{string::String, string::ToString, vec::Vec};

use alloc::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{CompileDiagnostic, DiagnosticId, DiagnosticSeverity, NodeLocator};

use super::format::{detect_format_bytes, SourceFormat};

/// Stable diagnostic codes emitted by the canvas core. Hosts map them to user text; they are never
/// parsed for meaning.
pub mod codes {
    pub const JSON_EMPTY: &str = "comfyCanvas.jsonEmpty";
    pub const JSON_PARSE_FAILED: &str = "comfyCanvas.jsonParseFailed";
    pub const TOP_LEVEL_NOT_OBJECT: &str = "comfyCanvas.topLevelNotObject";
    pub const GRAPH_EMPTY: &str = "comfyCanvas.graphEmpty";
    pub const FORMAT_AMBIGUOUS: &str = "comfyCanvas.formatAmbiguous";
    pub const FORMAT_UNRECOGNIZED: &str = "comfyCanvas.formatUnrecognized";
    pub const FORMAT_VERSION_UNSUPPORTED: &str = "comfyCanvas.formatVersionUnsupported";
    pub const NODE_INVALID: &str = "comfyCanvas.nodeInvalid";
    pub const NODE_ID_INVALID: &str = "comfyCanvas.nodeIdInvalid";
    pub const NODE_ID_COLLISION: &str = "comfyCanvas.nodeIdCollision";
    pub const NODE_MODE_INVALID: &str = "comfyCanvas.nodeModeInvalid";
    pub const MISSING_CLASS_TYPE: &str = "comfyCanvas.missingClassType";
    pub const LINK_INVALID: &str = "comfyCanvas.linkInvalid";
    pub const LINK_ID_COLLISION: &str = "comfyCanvas.linkIdCollision";
    pub const LINK_MISSING: &str = "comfyCanvas.linkMissing";
    pub const LINK_INCONSISTENT: &str = "comfyCanvas.linkInconsistent";
    pub const LINK_ORIGIN_MISSING: &str = "comfyCanvas.linkOriginMissing";
    pub const LINK_SLOT_OUT_OF_RANGE: &str = "comfyCanvas.linkSlotOutOfRange";
    pub const LINK_TYPE_MISMATCH: &str = "comfyCanvas.linkTypeMismatch";
    pub const LINK_SOURCE_DROPPED: &str = "comfyCanvas.linkSourceDropped";
    pub const LINK_RESOLUTION_CYCLE: &str = "comfyCanvas.linkResolutionCycle";
    pub const ANNOTATION_AS_SOURCE: &str = "comfyCanvas.annotationAsSource";
    pub const PRIMITIVE_VALUE_MISSING: &str = "comfyCanvas.primitiveValueMissing";
    pub const BYPASS_AMBIGUOUS: &str = "comfyCanvas.bypassAmbiguous";
    pub const SUBGRAPH_DEFINITION_INVALID: &str = "comfyCanvas.subgraphDefinitionInvalid";
    pub const SUBGRAPH_DEFINITION_DUPLICATE: &str = "comfyCanvas.subgraphDefinitionDuplicate";
    pub const SUBGRAPH_DEFINITION_CYCLE: &str = "comfyCanvas.subgraphDefinitionCycle";
    pub const SUBGRAPH_DEPTH_EXCEEDED: &str = "comfyCanvas.subgraphDepthExceeded";
    pub const BOUNDARY_PORT_MISSING: &str = "comfyCanvas.boundaryPortMissing";
    pub const BOUNDARY_PORT_MISMATCH: &str = "comfyCanvas.boundaryPortMismatch";
    pub const BOUNDARY_PORT_AMBIGUOUS: &str = "comfyCanvas.boundaryPortAmbiguous";
    pub const BOUNDARY_INPUT_UNCONNECTED: &str = "comfyCanvas.boundaryInputUnconnected";
    pub const BOUNDARY_OUTPUT_UNCONNECTED: &str = "comfyCanvas.boundaryOutputUnconnected";
    pub const PROMOTED_WIDGET_COUNT_MISMATCH: &str = "comfyCanvas.promotedWidgetCountMismatch";
    pub const LEGACY_GROUP_NODE_UNSUPPORTED: &str = "comfyCanvas.legacyGroupNodeUnsupported";
    pub const API_LINK_TARGET_MISSING: &str = "comfyCanvas.apiLinkTargetMissing";
    pub const API_WIDGETS_SIDECAR_NOT_EXECUTED: &str = "comfyCanvas.apiWidgetsSidecarNotExecuted";
    pub const OUTPUT_SELECTION_EMPTY: &str = "comfyCanvas.outputSelectionEmpty";
    pub const OUTPUT_NOT_FOUND: &str = "comfyCanvas.outputNotFound";
    pub const OUTPUT_INACTIVE: &str = "comfyCanvas.outputInactive";
    pub const OUTPUT_NOT_OUTPUT_NODE: &str = "comfyCanvas.outputNotOutputNode";
    pub const EXECUTION_CYCLE: &str = "comfyCanvas.executionCycle";
    pub const LINK_TO_INACTIVE: &str = "comfyCanvas.linkToInactive";
    pub const SOURCE_DIGEST_MISMATCH: &str = "comfyCanvas.sourceDigestMismatch";
    pub const DEFINITIONS_INCOMPLETE: &str = "comfyCanvas.definitionsIncomplete";
    pub const DEFINITION_MISSING: &str = "comfyCanvas.definitionMissing";
    pub const CAPABILITY_PLAN_MISSING: &str = "comfyCanvas.capabilityPlanMissing";
    pub const CAPABILITY_MISSING: &str = "comfyCanvas.capabilityMissing";
    pub const CAPABILITY_MISMATCH: &str = "comfyCanvas.capabilityMismatch";
    pub const SERIALIZER_PROFILE_UNAVAILABLE: &str = "comfyCanvas.serializerProfileUnavailable";
    pub const SPECIAL_RULE_NOT_FOUND: &str = "comfyCanvas.specialRuleNotFound";
    pub const BRIDGE_EVIDENCE_MISSING: &str = "comfyCanvas.bridgeEvidenceMissing";
    pub const BRIDGE_EVIDENCE_MISMATCH: &str = "comfyCanvas.bridgeEvidenceMismatch";
    pub const BRIDGE_NODE_MISSING: &str = "comfyCanvas.bridgeNodeMissing";
    pub const BRIDGE_LINK_MISMATCH: &str = "comfyCanvas.bridgeLinkMismatch";
    pub const INPUT_DOUBLE_SOURCE: &str = "comfyCanvas.inputDoubleSource";
    pub const VALUE_ORIGIN_UNKNOWN: &str = "comfyCanvas.valueOriginUnknown";
    pub const WIDGET_UNACCOUNTED: &str = "comfyCanvas.widgetUnaccounted";
    pub const WIDGET_DOUBLE_ACCOUNTED: &str = "comfyCanvas.widgetDoubleAccounted";
    pub const WIDGET_INDEX_OUT_OF_RANGE: &str = "comfyCanvas.widgetIndexOutOfRange";
    pub const OVERRIDE_TARGET_UNUSED: &str = "comfyCanvas.overrideTargetUnused";
    pub const DIGEST_FAILED: &str = "comfyCanvas.digestFailed";
    pub const OUTSIDE_SELECTED_CLOSURE: &str = "comfyCanvas.outsideSelectedClosure";
}

/// Scoped node identity: `scope` is the path of subgraph instance local ids from the root graph,
/// `local_id` the node id inside that scope (string form of the source id, never re-numbered).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeKey {
    pub scope: Vec<String>,
    pub local_id: String,
}

fn escape_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for ch in segment.chars() {
        match ch {
            '%' => out.push_str("%25"),
            ':' => out.push_str("%3A"),
            _ => out.push(ch),
        }
    }
    out
}

fn unescape_segment(segment: &str) -> Option<String> {
    let mut out = String::with_capacity(segment.len());
    let mut rest = segment;
    while let Some(pos) = rest.find('%') {
        out.push_str(&rest[..pos]);
        let escape = rest.get(pos..pos + 3)?;
        match escape {
            "%25" => out.push('%'),
            "%3A" => out.push(':'),
            _ => return None,
        }
        rest = &rest[pos + 3..];
    }
    out.push_str(rest);
    if out.is_empty() {
        return None;
    }
    Some(out)
}

impl NodeKey {
    pub fn root(local_id: impl Into<String>) -> Self {
        Self { scope: Vec::new(), local_id: local_id.into() }
    }

    /// Deterministic, reversible execution id: segments joined by `:` (the official subgraph
    /// execution-id separator); `%` and `:` inside a segment are percent-escaped. Root nodes keep
    /// their source id verbatim unless it contains an escaped character.
    pub fn exec_id(&self) -> String {
        let mut parts: Vec<String> = self.scope.iter().map(|s| escape_segment(s)).collect();
        parts.push(escape_segment(&self.local_id));
        parts.join(":")
    }

    pub fn from_exec_id(s: &str) -> Option<NodeKey> {
        if s.is_empty() {
            return None;
        }
        let mut segments = Vec::new();
        for raw in s.split(':') {
            segments.push(unescape_segment(raw)?);
        }
        let local_id = segments.pop()?;
        Some(NodeKey { scope: segments, local_id })
    }

    pub fn locator(&self) -> NodeLocator {
        NodeLocator { scope_path: self.scope.clone(), node_id: self.local_id.clone() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkRef {
    pub from: NodeKey,
    pub output_slot: u32,
    pub output_name: Option<String>,
    pub output_type: Option<String>,
}

/// Literal vs link are distinct by construction; an array literal is never a link.
/// Adjacently tagged because a literal may be any JSON value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum InputValue {
    Literal(Value),
    Link(LinkRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeMode {
    Normal,
    Muted,
    Bypassed,
}

/// Where a per-instance override value comes from (CW22 / CW23).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum OverrideSource {
    /// Subgraph instance widget value bound to a subgraph input port.
    Promoted { instance: NodeKey },
    /// Frontend `PrimitiveNode` value applied to the target widget (virtual node, never executed).
    Primitive { node: NodeKey },
}

/// Value to write into this node's local copy through `widgets::apply_promoted_overrides`.
/// The shared subgraph definition is never mutated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WidgetOverride {
    pub input_name: String,
    pub value: Value,
    pub source: OverrideSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IrNode {
    pub key: NodeKey,
    pub class_type: String,
    pub mode: NodeMode,
    pub inputs: BTreeMap<String, InputValue>,
    pub widgets_values: Option<Value>,
    pub source_locator: NodeLocator,
}

/// Per-node source attributes kept beside the registered `IrNode` shape.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeAttrsV1 {
    /// Display title (`title` on canvas, `_meta.title` in API); kept for `_meta`, never semantic.
    pub title: Option<String>,
    /// Source `properties` object, retained verbatim (CW27: node attributes are preserved).
    pub properties: Option<Value>,
    /// Instance-local widget overrides (promoted subgraph widgets, primitive targets), applied to
    /// a local copy through `widgets::apply_promoted_overrides`.
    pub overrides: Vec<WidgetOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphIrV1 {
    pub format: SourceFormat,
    pub nodes: Vec<IrNode>,
    /// Structural sink candidates: Normal nodes no other Normal node consumes. Output selection is
    /// explicit (`OutputSelection`); this list is discovery input only.
    pub outputs: Vec<NodeKey>,
    /// Node-scoped diagnostics. They block compile only when their node is in the selected
    /// output closure; otherwise they are reported as non-blocking pruned-branch findings.
    pub diagnostics: Vec<CompileDiagnostic>,
    /// exec_id → source attributes of every node in `nodes`.
    pub node_attrs: BTreeMap<String, NodeAttrsV1>,
}

impl GraphIrV1 {
    pub fn node(&self, key: &NodeKey) -> Option<&IrNode> {
        self.nodes.iter().find(|n| &n.key == key)
    }

    pub fn attrs(&self, key: &NodeKey) -> Option<&NodeAttrsV1> {
        self.node_attrs.get(&key.exec_id())
    }
}

/// Diagnostic builder shared by the canvas modules.
pub(crate) struct Diag {
    code: &'static str,
    severity: DiagnosticSeverity,
    locator: Option<NodeLocator>,
    class_type: Option<String>,
    input_name: Option<String>,
    params: BTreeMap<String, String>,
}

impl Diag {
    pub(crate) fn blocking(code: &'static str) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Blocking,
            locator: None,
            class_type: None,
            input_name: None,
            params: BTreeMap::new(),
        }
    }

    pub(crate) fn non_blocking(code: &'static str) -> Self {
        let mut d = Self::blocking(code);
        d.severity = DiagnosticSeverity::NonBlocking;
        d
    }

    pub(crate) fn at(mut self, key: &NodeKey) -> Self {
        self.locator = Some(key.locator());
        self
    }

    pub(crate) fn class(mut self, class_type: &str) -> Self {
        self.class_type = Some(class_type.to_string());
        self
    }

    pub(crate) fn input(mut self, input_name: &str) -> Self {
        self.input_name = Some(input_name.to_string());
        self
    }

    pub(crate) fn param(mut self, key: &str, value: impl ToString) -> Self {
        self.params.insert(key.to_string(), value.to_string());
        self
    }

    pub(crate) fn build(self) -> CompileDiagnostic {
        let mut id = String::from(self.code);
        if let Some(loc) = &self.locator {
            id.push('|');
            let key = NodeKey { scope: loc.scope_path.clone(), local_id: loc.node_id.clone() };
            id.push_str(&key.exec_id());
        }
        if let Some(input) = &self.input_name {
            id.push('|');
            id.push_str(input);
        }
        for (k, v) in &self.params {
            id.push('|');
            id.push_str(k);
            id.push('=');
            id.push_str(v);
        }
        CompileDiagnostic {
            diagnostic_id: DiagnosticId(id),
            code: self.code.to_string(),
            severity: self.severity,
            locator: self.locator,
            class_type: self.class_type,
            input_name: self.input_name,
            params: self.params,
        }
    }
}

pub(crate) fn is_blocking(d: &CompileDiagnostic) -> bool {
    matches!(d.severity, DiagnosticSeverity::Blocking)
}

/// Parse any supported source format into the scoped IR. Returns `Err` only for graph-level
/// blocking problems (unparseable JSON, format, id collisions, definition cycles); node-scoped
/// problems travel in `GraphIrV1::diagnostics`.
pub fn parse_to_ir(raw: &[u8]) -> Result<GraphIrV1, Vec<CompileDiagnostic>> {
    let (value, format) = detect_format_bytes(raw).map_err(|d| vec![*d])?;
    let ir = match format {
        SourceFormat::ApiPrompt | SourceFormat::FlatUi => super::format::parse_dict_graph(&value, format)?,
        SourceFormat::Canvas04 | SourceFormat::Canvas10 => super::subgraph::parse_canvas(&value, format)?,
    };
    let global: Vec<CompileDiagnostic> = ir
        .diagnostics
        .iter()
        .filter(|d| is_blocking(d) && d.locator.is_none())
        .cloned()
        .collect();
    if !global.is_empty() {
        return Err(global);
    }
    Ok(ir)
}

/// Structural sink candidates of a parsed node set.
pub(crate) fn structural_sinks(nodes: &[IrNode]) -> Vec<NodeKey> {
    let mut consumed = alloc::collections::BTreeSet::new();
    for node in nodes.iter().filter(|n| n.mode == NodeMode::Normal) {
        for input in node.inputs.values() {
            if let InputValue::Link(link) = input {
                consumed.insert(link.from.clone());
            }
        }
    }
    nodes
        .iter()
        .filter(|n| n.mode == NodeMode::Normal && !consumed.contains(&n.key))
        .map(|n| n.key.clone())
        .collect()
}
