import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { workflowApi } from './workflow-api.js';

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
const widgetContract = JSON.parse(await readFile(resolve(here, 'widget-order.lock.json'), 'utf8'));
const apiDirectory = resolve(here, '../workflow/api');
await mkdir(apiDirectory, { recursive: true });
const apiInventory = [];
for (const name of names) {
  const bytes = await readFile(resolve(source, 'example_workflows', `${name}.json`));
  const workflow = JSON.parse(bytes);
  const resources = workflow.nodes.flatMap(node => node.properties?.models ?? []);
  // Notes are upstream source evidence, not verified downloads. Keep them separate
  // from stale properties.models and never turn a model page into a guessed file URL.
  const noteResources = workflow.nodes.filter(node => ['Note', 'MarkdownNote'].includes(node.type))
    .flatMap(node => (Array.isArray(node.widgets_values) ? node.widgets_values : Object.values(node.widgets_values ?? {}))
      .filter(value => typeof value === 'string').flatMap(value => [...value.matchAll(/\[([^\]\n]+\.(?:safetensors|gguf|ckpt|pth|pt|bin))\]\((https:\/\/[^\s)]+)\)/gi)]
        .map(match => ({ name: match[1], url: match[2], sourceNodeId: node.id, status: 'UPSTREAM_NOTE_UNVERIFIED' }))));
  const nodes = [...new Set(workflow.nodes.map(node => node.type).filter(type => !['Note', 'MarkdownNote', 'Reroute'].includes(type)))].sort();
  const apiGraph = workflowApi(workflow, widgetContract.widgets);
  const basename = value => value.replaceAll('\\', '/').split('/').at(-1);
  const modelBindings = [];
  const resourceWarnings = [];
  for (const node of workflow.nodes) {
    const declared = node.properties?.models ?? [];
    if (!declared.length) continue;
    const used = Object.entries(apiGraph[String(node.id)]?.inputs ?? {}).filter(([, value]) => typeof value === 'string' && /\.(safetensors|gguf|ckpt|pth|pt|bin)$/i.test(value));
    for (const [field, filename] of used) {
      const resource = declared.find(item => basename(item.name) === basename(filename));
      modelBindings.push({ nodeId: node.id, nodeType: node.type, field, filename, resource: resource ?? null,
        noteResources: noteResources.filter(item => basename(item.name) === basename(filename)) });
      if (!resource) resourceWarnings.push({ nodeId: node.id, field, filename, reason: 'Workflow filename differs from its model download declaration; resolve before downloading.' });
    }
  }
  inventory.push({ name, sha256: createHash('sha256').update(bytes).digest('hex'), nodes, resources, noteResources, modelBindings, resourceWarnings });
  await writeFile(resolve(directory, `${name}.json`), bytes);
  const apiBytes = JSON.stringify(apiGraph, null, 2) + '\n';
  await writeFile(resolve(apiDirectory, `${name}.json`), apiBytes);
  apiInventory.push({ name, sha256: createHash('sha256').update(apiBytes).digest('hex'), nodes: Object.keys(apiGraph).length });
  if (name === names[0]) {
    const graph = structuredClone(apiGraph);
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
await writeFile(resolve(apiDirectory, 'inventory.json'), JSON.stringify({ upstream: lock.commit, status: 'API_TEMPLATES_ONLY',
  widgetContractSha256: createHash('sha256').update(await readFile(resolve(here, 'widget-order.lock.json'))).digest('hex'),
  examples: apiInventory }, null, 2) + '\n');
console.log(JSON.stringify({ commit: lock.commit, examples: inventory.length, modelsDownloaded: 0 }));
