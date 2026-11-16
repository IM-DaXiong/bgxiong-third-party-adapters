use alloc::collections::BTreeSet;

use bgx_comfy_workflow::canvas::{collect_annotation_units, parse_to_ir, source_digest};
use serde_json::{json, Value};

pub(crate) fn analyze(source: &str) -> Value {
    match parse_to_ir(source.as_bytes()) {
        Ok(graph) => {
            let classes: BTreeSet<_> = graph.nodes.iter().map(|node| &node.class_type).collect();
            super::ready(json!({
                "sourceDigest": source_digest(source.as_bytes()),
                "sourceFormat": graph.format,
                "definitionClasses": classes,
                "outputKeys": graph.outputs,
                "annotations": collect_annotation_units(&serde_json::from_str(source).expect("parsed workflow source")),
                "nodeDiagnostics": graph.diagnostics,
            }))
        }
        Err(diagnostics) => super::blocked(&diagnostics),
    }
}
