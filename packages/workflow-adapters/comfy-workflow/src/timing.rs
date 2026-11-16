//! Resolve LTX graph timing from the selected output and the captured node definitions.
use alloc::{collections::BTreeSet, string::String};
use bgx_comfy_workflow::{
    linked_source, node_registry::definition::DefinitionSnapshotV1, WorkflowGraph,
};
use serde_json::{json, Value};

use crate::execute::Failure;

pub(crate) const INVALID_TIMING: &str = "INVALID_WORKFLOW_TIMING";
type Result<T> = core::result::Result<T, Failure>;

#[cfg(test)]
#[path = "timing_tests.rs"]
mod tests;

fn invalid(message: &'static str) -> Failure {
    Failure::new(INVALID_TIMING, message)
}

fn unsupported() -> Failure {
    invalid("所选工作流没有可验证的 LTX 时序链")
}

#[derive(PartialEq)]
pub(crate) struct Plan {
    duration_id: String,
    fps_id: String,
    original_duration: Value,
    original_fps: Value,
    seconds: u32,
    fps: u32,
    frames: u32,
}

impl Plan {
    pub(crate) fn apply(self, graph: &mut Value) -> Result<Value> {
        if graph[&self.duration_id]["inputs"]["value"] != self.original_duration
            || graph[&self.fps_id]["inputs"]["value"] != self.original_fps
        {
            return Err(invalid("已提交的映射与时序值同时改写相同输入"));
        }
        graph[&self.duration_id]["inputs"]["value"] = json!(self.seconds);
        graph[&self.fps_id]["inputs"]["value"] = json!(self.fps);
        Ok(json!({"seconds":self.seconds,"fps":self.fps,"frames":self.frames}))
    }
}

fn unique_class<'a>(graph: &Value, nodes: &'a BTreeSet<String>, class: &str) -> Result<&'a str> {
    let mut matches = nodes.iter().filter(|id| graph[*id]["class_type"] == class);
    let selected = matches.next().ok_or_else(unsupported)?;
    if matches.next().is_some() {
        return Err(unsupported());
    }
    Ok(selected)
}

fn link(input: &Value, output_index: usize) -> Result<String> {
    linked_source(input)
        .filter(|(_, index)| *index == output_index)
        .map(|(id, _)| id)
        .ok_or_else(unsupported)
}

fn expression(node: &Value, expected: &str) -> bool {
    node["class_type"] == "ComfyMathExpression"
        && node["inputs"]["expression"].as_str().is_some_and(|text| {
            text.chars()
                .filter(|c| !c.is_ascii_whitespace())
                .eq(expected.chars())
        })
}

fn bounded(
    definitions: &DefinitionSnapshotV1,
    class: &str,
    key: &str,
    value: u32,
    grid: bool,
) -> Result<()> {
    let options = &definitions
        .classes
        .get(class)
        .and_then(|def| def.input(key))
        .ok_or_else(unsupported)?
        .options;
    let min = options["min"].as_f64().ok_or_else(unsupported)?;
    let max = options["max"].as_f64().ok_or_else(unsupported)?;
    let origin = options["min"].as_u64();
    let step = options["step"].as_u64().filter(|step| *step > 0);
    if !(min..=max).contains(&f64::from(value))
        || (grid
            && !origin.zip(step).is_some_and(|(origin, step)| {
                u64::from(value)
                    .checked_sub(origin)
                    .is_some_and(|n| n % step == 0)
            }))
    {
        return Err(invalid("所选时长和帧率组合超出 LTX 节点的范围或帧数步进"));
    }
    Ok(())
}

fn positive(values: &Value, key: &str) -> Result<u32> {
    values[key]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid("时长和帧率须为正整数"))
}

pub(crate) fn plan(
    graph: &Value,
    definitions: &DefinitionSnapshotV1,
    bindings: &[Value],
    values: &Value,
) -> Result<Plan> {
    let mut videos = bindings.iter().filter(|binding| binding["kind"] == "video");
    let output = videos
        .next()
        .and_then(|binding| binding["nodeId"].as_str())
        .ok_or_else(unsupported)?;
    if videos.next().is_some() || graph[output]["class_type"] != "SaveVideo" {
        return Err(unsupported());
    }
    let playout = link(&graph[output]["inputs"]["video"], 0)?;
    let fps_math_id = link(&graph[&playout]["inputs"]["fps"], 0)?;
    let fps_math = &graph[&fps_math_id];
    if graph[&playout]["class_type"] != "CreateVideo" || !expression(fps_math, "a") {
        return Err(unsupported());
    }
    let fps_id = link(&fps_math["inputs"]["values.a"], 0)?;

    let ancestry = WorkflowGraph::from_api(graph)
        .and_then(|graph| graph.reverse_ancestry(output))
        .map_err(|_| unsupported())?;

    let latent = unique_class(graph, &ancestry, "EmptyLTXVLatentVideo")?;
    let audio = unique_class(graph, &ancestry, "LTXVEmptyLatentAudio")?;
    let conditioning = unique_class(graph, &ancestry, "LTXVConditioning")?;
    let length_id = link(&graph[latent]["inputs"]["length"], 1)?;
    let length_math = &graph[&length_id];
    if !expression(length_math, "a*b+1")
        || link(&length_math["inputs"]["values.b"], 0)? != fps_id
        || link(&graph[audio]["inputs"]["frames_number"], 1)? != length_id
        || link(&graph[audio]["inputs"]["frame_rate"], 1)? != fps_math_id
        || link(&graph[conditioning]["inputs"]["frame_rate"], 0)? != fps_math_id
    {
        return Err(unsupported());
    }
    let duration_id = link(&length_math["inputs"]["values.a"], 0)?;
    let original_duration = graph[&duration_id]["inputs"]["value"].clone();
    let original_fps = graph[&fps_id]["inputs"]["value"].clone();
    if duration_id == fps_id
        || [(&duration_id, &original_duration), (&fps_id, &original_fps)]
            .iter()
            .any(|(id, value)| {
                graph[*id]["class_type"] != "PrimitiveInt" || value.as_i64().is_none()
            })
    {
        return Err(unsupported());
    }

    let seconds = positive(values, "seconds")?;
    let fps = positive(values, "fps")?;
    let frames = seconds
        .checked_mul(fps)
        .and_then(|frames| frames.checked_add(1))
        .ok_or_else(|| invalid("LTX 帧数换算溢出"))?;
    bounded(definitions, "EmptyLTXVLatentVideo", "length", frames, true)?;
    bounded(
        definitions,
        "LTXVEmptyLatentAudio",
        "frames_number",
        frames,
        false,
    )?;
    bounded(
        definitions,
        "LTXVEmptyLatentAudio",
        "frame_rate",
        fps,
        false,
    )?;
    bounded(definitions, "LTXVConditioning", "frame_rate", fps, false)?;
    bounded(definitions, "CreateVideo", "fps", fps, false)?;
    Ok(Plan {
        duration_id,
        fps_id,
        original_duration,
        original_fps,
        seconds,
        fps,
        frames,
    })
}
