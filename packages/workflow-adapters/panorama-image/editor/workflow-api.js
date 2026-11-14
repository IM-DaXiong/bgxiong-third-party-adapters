// Authoring-time conversion. No workflow is accepted at runtime from the host.
// Named widgets are authoritative. Legacy arrays require a reviewed input-order contract;
// an extra/missing value, unknown mode, broken link or reroute cycle is a hard error.
const displayOnly = new Set(['control_after_generate', '$$comfy_animation_preview', 'upload', 'choose video to upload', 'videopreview']);
const notes = new Set(['Note', 'MarkdownNote']);
export function workflowApi(workflow, widgetContracts = {}) {
  const nodes = new Map(workflow.nodes.map(node => [node.id, node]));
  const links = new Map(workflow.links.map(link => [link[0], link]));
  if (nodes.size !== workflow.nodes.length || links.size !== workflow.links.length) throw new Error('Duplicate workflow identity');
  function origin(linkId, visited = new Set()) {
    if (visited.has(linkId)) throw new Error('Reroute cycle');
    visited.add(linkId);
    const link = links.get(linkId);
    if (!link) throw new Error(`Missing link ${linkId}`);
    const source = nodes.get(link[1]);
    if (!source || source.mode !== 0 || notes.has(source.type)) throw new Error(`Invalid source for link ${linkId}`);
    if (source.type === 'Reroute') {
      if (source.inputs?.length !== 1 || source.inputs[0].link == null) throw new Error('Disconnected reroute');
      return origin(source.inputs[0].link, visited);
    }
    if (!Number.isInteger(link[2]) || link[2] < 0 || link[2] >= (source.outputs?.length ?? 0)) throw new Error(`Invalid output slot for link ${linkId}`);
    return [String(source.id), link[2]];
  }
  const graph = {};
  for (const node of workflow.nodes) {
    if (notes.has(node.type) || node.type === 'Reroute') continue;
    if (node.mode !== 0) throw new Error(`Unsupported executable mode at ${node.id}`);
    let named = node.widgets_values_named;
    if (!named && node.widgets_values && !Array.isArray(node.widgets_values)) named = node.widgets_values;
    const inputs = {};
    if (named) {
      for (const [key, value] of Object.entries(named)) if (!displayOnly.has(key)) inputs[key] = value;
    } else if (node.widgets_values?.length) {
      const contract = widgetContracts[node.type];
      if (!contract || contract.length !== node.widgets_values.length) throw new Error(`Widget contract differs for ${node.type}#${node.id}`);
      contract.forEach((field, index) => {
        const value = node.widgets_values[index];
        if (field.displayOnly) {
          if (!field.values.some(expected => Object.is(expected, value))) throw new Error(`Unexpected display widget at ${node.type}#${node.id}:${index}`);
          return;
        }
        if ((field.type === 'INT' && !Number.isSafeInteger(value)) || (field.type === 'FLOAT' && (typeof value !== 'number' || !Number.isFinite(value)))
          || (['STRING','COMBO'].includes(field.type) && typeof value !== 'string') || (field.type === 'BOOLEAN' && typeof value !== 'boolean')) {
          throw new Error(`Widget type differs for ${node.type}.${field.name}`);
        }
        inputs[field.name] = value;
      });
    }
    for (const [index, input] of (node.inputs ?? []).entries()) if (input.link != null) {
      const link = links.get(input.link);
      if (!link || link[3] !== node.id || link[4] !== index) throw new Error(`Broken target link at ${node.id}.${input.name}`);
      inputs[input.name] = origin(input.link);
    }
    graph[String(node.id)] = { class_type: node.type, inputs };
  }
  return graph;
}
