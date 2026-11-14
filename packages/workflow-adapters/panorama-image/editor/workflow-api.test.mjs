import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { workflowApi } from './workflow-api.js';
const load = async path => JSON.parse(await readFile(new URL(path, import.meta.url), 'utf8'));
const inventory = await load('../workflow/upstream/inventory.json');
const contracts = (await load('./widget-order.lock.json')).widgets;
for (const entry of inventory.examples) test(`frozen upstream ${entry.name}`, async () => {
  const input = await load(`../workflow/upstream/${entry.name}.json`);
  const graph = workflowApi(input, contracts);
  assert.equal(Object.keys(graph).length, input.nodes.filter(node => !['Note','MarkdownNote','Reroute'].includes(node.type)).length);
  for (const node of Object.values(graph)) for (const value of Object.values(node.inputs)) {
    if (Array.isArray(value) && value.length === 2 && typeof value[0] === 'string' && typeof value[1] === 'number') assert.ok(graph[value[0]], `Missing source ${value[0]}`);
  }
  const broken = structuredClone(input);
  broken.nodes.find(node => node.inputs?.some(input => input.link != null)).inputs.find(input => input.link != null).link = -999;
  assert.throws(() => workflowApi(broken, contracts), /link/);
  if (entry.name.startsWith('LTX')) {
    const changed = structuredClone(contracts); changed.RandomNoise = [];
    assert.throws(() => workflowApi(input, changed), /Widget contract differs/);
    const route = input.nodes.find(node => node.type === 'Reroute');
    const link = input.links.find(link => link[0] === route.inputs[0].link);
    link[1] = route.id; link[2] = 0;
    assert.throws(() => workflowApi(input, contracts), /Reroute cycle/);
  }
});
