#[cfg(target_arch = "wasm32")]
use alloc::{string::ToString, vec, vec::Vec};

use bgx_comfy_workflow::canvas::{
    format::AUTHORED_OUTPUT_KEY, parse_to_ir, resolve::compile_with_evidence, CompileContextV1, NodeMode, OutputSelection,
};
use bgx_comfy_workflow::{CompileDiagnostic, DiagnosticId, DiagnosticSeverity};
use serde_json::{json, Value};

pub(crate) fn compile(source: &str, input: &Value) -> Result<Value, &'static str> {
    let context: CompileContextV1 = serde_json::from_value(input["context"].clone())
        .map_err(|_| "Invalid workflow compile context")?;
    let selection = if let Some(selection) = input.get("selection") {
        serde_json::from_value(selection.clone()).map_err(|_| "Invalid workflow output selection")?
    } else {
        match import_selection(source, &context) {
            Ok(selection) => selection,
            Err(diagnostics) => return Ok(super::blocked(&diagnostics)),
        }
    };
    match compile_with_evidence(source.as_bytes(), context, &selection) {
        Ok(artifact) => Ok(super::ready(json!(artifact))),
        Err(diagnostics) => Ok(super::blocked(&diagnostics)),
    }
}

fn import_selection(source: &str, context: &CompileContextV1) -> Result<OutputSelection, Vec<CompileDiagnostic>> {
    let graph = parse_to_ir(source.as_bytes())?;
    let root: Value = serde_json::from_str(source).expect("parsed workflow source");
    let declared = root.get(AUTHORED_OUTPUT_KEY).map(|declaration| {
        let id = match (declaration.get("version").and_then(Value::as_u64), declaration.get("nodeId")) {
            (Some(1), Some(Value::String(id))) if !id.trim().is_empty() => id.clone(),
            (Some(1), Some(Value::Number(id))) => id.to_string(),
            _ => return Err(vec![selection_error("declaredOutputInvalid", AUTHORED_OUTPUT_KEY)]),
        };
        Ok(id)
    }).transpose()?;
    let output_keys: Vec<_> = graph.nodes.iter().filter(|node| {
        matches!(node.mode, NodeMode::Normal)
            && context.definitions.classes.get(&node.class_type)
                .and_then(|definition| definition.extra.get("output_node"))
                .and_then(Value::as_bool) == Some(true)
            && declared.as_ref().is_none_or(|id| node.key.exec_id() == *id)
    }).map(|node| node.key.clone()).collect();
    if let Some(id) = declared.filter(|_| output_keys.is_empty()) {
        return Err(vec![selection_error("declaredOutputNotExecutable", &id)]);
    }
    Ok(OutputSelection { output_keys })
}

fn selection_error(reason: &str, subject: &str) -> CompileDiagnostic {
    const CODE: &str = "workflowMappingOutputMissing";
    CompileDiagnostic {
        diagnostic_id: DiagnosticId(alloc::format!("{CODE}@{subject}")),
        code: CODE.to_string(),
        severity: DiagnosticSeverity::Blocking,
        locator: None,
        class_type: None,
        input_name: None,
        params: [("reason".to_string(), reason.to_string()), ("nodeId".to_string(), subject.to_string())]
            .into_iter().collect(),
    }
}
