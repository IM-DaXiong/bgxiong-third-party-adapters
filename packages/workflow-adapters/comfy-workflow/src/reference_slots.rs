use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use bgx_comfy_workflow::canvas::{
    resolve::compile_with_evidence, CompileContextV1, OutputSelection,
};
use bgx_comfy_workflow::reference_image_slots::project_reference_image_slots;
use bgx_comfy_workflow::{
    discover_output_candidates, CompileDiagnostic, DiagnosticId, DiagnosticSeverity,
    MappingErrorCode, MediaKind,
};
use serde_json::{json, Value};

#[cfg(test)]
#[path = "reference_tests.rs"]
mod tests;

pub(crate) fn project(source: &str, input: &Value) -> Result<Value, &'static str> {
    let context: CompileContextV1 = serde_json::from_value(input["context"].clone())
        .map_err(|_| "Invalid workflow compile context")?;
    let selection: OutputSelection = serde_json::from_value(input["selection"].clone())
        .map_err(|_| "Invalid workflow output selection")?;
    let [selected] = selection.output_keys.as_slice() else {
        return Ok(blocked(
            "comfyReferenceImageOutputRequired",
            "exactly_one_image_output",
        ));
    };
    let artifact = match compile_with_evidence(source.as_bytes(), context.clone(), &selection) {
        Ok(artifact) => artifact,
        Err(diagnostics) => return Ok(crate::blocked(&diagnostics)),
    };
    let candidates = match discover_output_candidates(&artifact.api_prompt) {
        Ok(candidates) => candidates,
        Err(error) => return Ok(blocked("comfyReferenceImageGroupInvalid", &error.detail)),
    };
    let image_output = candidates.into_iter().find(|candidate| {
        candidate.media_kind == MediaKind::Image
            && artifact
                .source_of(&candidate.node_id)
                .is_some_and(|location| {
                    location.scope_path == selected.scope && location.node_id == selected.local_id
                })
    });
    let Some(output) = image_output else {
        return Ok(blocked(
            "comfyReferenceImageOutputRequired",
            "selected_output_is_not_image",
        ));
    };
    let reference = match project_reference_image_slots(
        &artifact.api_prompt,
        &context.definitions,
        &output.node_id,
    ) {
        Ok(reference) => reference,
        Err(error) => {
            let code = if error.code == MappingErrorCode::BindingAmbiguous {
                "comfyReferenceImageGroupAmbiguous"
            } else {
                "comfyReferenceImageGroupInvalid"
            };
            return Ok(blocked(code, &error.detail));
        }
    };
    let mut selected_inputs = Vec::new();
    if let Some(count) = input.get("referenceCount") {
        let count = count
            .as_u64()
            .and_then(|count| usize::try_from(count).ok())
            .ok_or("Invalid reference count")?;
        let (min, max) = reference.as_ref().map_or((0, 0), |reference| {
            (reference.min_count, reference.max_count)
        });
        if !(min..=max).contains(&count) {
            return Ok(crate::blocked(&[diagnostic(
                "comfyReferenceImageCountOutOfRange",
                [
                    ("provided", count.to_string()),
                    ("min", min.to_string()),
                    ("max", max.to_string()),
                ],
            )]));
        }
        if let Some(reference) = &reference {
            selected_inputs.extend(
                reference
                    .slots
                    .iter()
                    .take(count)
                    .map(|slot| slot.input_name.as_str()),
            );
        }
    }
    Ok(crate::ready(
        json!({"sourceDigest":context.source_digest, "compiledGraphDigest":artifact.digests.graph,
        "outputNodeId":output.node_id,"referenceImage":reference,"selectedInputs":selected_inputs}),
    ))
}

fn blocked(code: &str, reason: &str) -> Value {
    crate::blocked(&[diagnostic(code, [("reason", reason.to_string())])])
}

fn diagnostic<const N: usize>(code: &str, params: [(&str, String); N]) -> CompileDiagnostic {
    CompileDiagnostic {
        diagnostic_id: DiagnosticId(format!("{code}@referenceImage")),
        code: code.to_string(),
        severity: DiagnosticSeverity::Blocking,
        locator: None,
        class_type: None,
        input_name: None,
        params: params
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    }
}
