import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';

// Preserve the six upstream examples and their resource declarations. Never download weights.
const here = dirname(fileURLToPath(import.meta.url));
const source = resolve(process.argv[2] ?? '');
const lock = JSON.parse(await readFile(resolve(here, 'upstream.lock.json'), 'utf8'));
if (execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim() !== lock.commit) throw new Error('Source commit differs');
if (execFileSync('git', ['-C', source, 'diff', 'HEAD', '--name-only'], { encoding: 'utf8' }).trim()) throw new Error('Upstream has tracked changes');
const names = ['qwen-image-2.1-360-erp-outpaint', 'qwen-image-2.1-panorama-cutout', 'qwen-image-2.1-panorama-with-3d', 'qwen-image-2.1-moge3-anyangle', 'pika-h3-flex-moge3-crossview', 'LTX-2.3_360vr_distilled_3stage'];
const directory = resolve(here, '../workflow/upstream');
await mkdir(directory, { recursive: true });
const inventory = [];
for (const name of names) {
  const bytes = await readFile(resolve(source, 'example_workflows', `${name}.json`));
  const workflow = JSON.parse(bytes);
  const resources = workflow.nodes.flatMap(node => node.properties?.models ?? []);
  const nodes = [...new Set(workflow.nodes.map(node => node.type).filter(type => !['Note', 'MarkdownNote', 'Reroute'].includes(type)))].sort();
  inventory.push({ name, sha256: createHash('sha256').update(bytes).digest('hex'), nodes, resources });
  await writeFile(resolve(directory, `${name}.json`), bytes);
  if (name === names[0]) {
    // Named widgets are upstream's explicit serialization. Never guess numeric widget positions.
    const links = new Map(workflow.links.map(link => [link[0], link]));
    const graph = {};
    for (const node of workflow.nodes) {
      if (['Note', 'MarkdownNote'].includes(node.type)) continue;
      if (node.mode !== 0 || node.type === 'Reroute') throw new Error(`Unsupported executable node ${node.id}`);
      if (node.widgets_values?.length && !node.widgets_values_named) throw new Error(`Missing named widgets for ${node.type}`);
      const inputs = { ...(node.widgets_values_named ?? {}) };
      delete inputs.control_after_generate;
      delete inputs.$$comfy_animation_preview;
      for (const input of node.inputs ?? []) if (input.link != null) {
        const link = links.get(input.link);
        if (!link || link[3] !== node.id) throw new Error('Broken workflow link');
        inputs[input.name] = [String(link[1]), link[2]];
      }
      graph[String(node.id)] = { class_type: node.type, inputs };
    }
    // Normalize the output id only. The diffusion graph and all connections remain upstream's.
    graph['66'] = graph['56']; delete graph['56'];
    for (const node of Object.values(graph)) for (const [key, value] of Object.entries(node.inputs)) {
      if (Array.isArray(value) && value[0] === '56') node.inputs[key] = ['66', value[1]];
    }
    // User content and references are supplied at invocation, never keep the author's prompt/assets.
    graph['64'].inputs.prompt = '';
    graph['83'].inputs.state_json = '';
    await writeFile(resolve(here, '../workflow/qwen-2.1-scene-api.json'), JSON.stringify(graph, null, 2) + '\n');
  }
}
await writeFile(resolve(directory, 'inventory.json'), JSON.stringify({ upstream: lock.commit, examples: inventory }, null, 2) + '\n');
console.log(JSON.stringify({ commit: lock.commit, examples: inventory.length, modelsDownloaded: 0 }));
