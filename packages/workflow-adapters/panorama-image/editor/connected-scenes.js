import { loadSceneState, sceneStateText } from '@panorama-upstream/web_src/pano_scene_state.js';
import { applyUpstreamOverrides } from '@panorama-upstream/web_src/pano_scene_inherit.js';
import { inspectStatePreservation } from './state-preservation.js';

// Adapt host-owned connected documents to the upstream Scene UI envelope. No parent state
// is persisted by this editor. Actual generation connects the corresponding Studio nodes.
export function connectedScenes(initial, addAsset) {
  const sources = new Map();
  const nodes = [];
  function layersOf(ids) {
    if (!Array.isArray(ids) || ids.length > 1) throw new Error('全景工作室只接受一个上游场景，请调整连线。');
    if (!ids.length) return [];
    if (!sources.has(ids[0])) throw new Error('上游场景快照缺失或存在循环，请重新打开。');
    return sources.get(ids[0]);
  }
  for (const [index, source] of (initial.connectedDocuments ?? []).entries()) {
    const id = source.target.nodeId;
    if (sources.has(id)) throw new Error('上游场景身份重复。');
    const protection = inspectStatePreservation(source.document.stateText, loadSceneState, sceneStateText);
    if (protection.readOnly) throw new Error('上游工程包含当前版本无法保全的内容，请先在原节点检查。');
    const names = new Map();
    for (const [assetIndex, asset] of source.assets.entries()) {
      const name = `input/bgx_connected_${index}/${assetIndex}-${asset.name.split('/').at(-1)}`;
      names.set(asset.name, name); addAsset({ ...asset, name });
    }
    const state = JSON.parse(source.document.stateText);
    function bind(value) {
      if (!value || typeof value !== 'object') return;
      if (value.kind === 'file' || value.type === 'comfy_image') {
        const original = `${value.storage ?? 'input'}/${value.subfolder ?? ''}/${value.filename}`;
        const name = names.get(original);
        if (!name) throw new Error(`上游工程素材缺失：${value.filename}`);
        const parts = name.split('/');
        value.filename = parts.pop(); value.storage = parts.shift(); value.subfolder = parts.join('/');
      } else Object.values(value).forEach(bind);
    }
    bind(state);
    const own = loadSceneState(state);
    const inherited = layersOf(source.inputNodeIds);
    // Upstream applies a record only to the nearest layer with the same scene id.
    const layers = inherited.map((layer, at) => ({ ...layer, state: inherited.slice(at + 1).some(next => next.state.id === layer.state.id)
      ? layer.state : applyUpstreamOverrides(layer.state, own.upstream.find(record => record.layer === layer.state.id)) }));
    layers.push({ state: own, ui: {} });
    sources.set(id, layers);
    nodes.push({ id, type: 'PanoramaStudio', title: '上游全景', widgets: [{ name: 'state_json', value: source.document.stateText }], outputs: [{ type: 'PANO_SCENE' }] });
  }
  const layers = layersOf(initial.inputNodeIds ?? []);
  const parent = initial.inputNodeIds?.[0];
  return { nodes, layers, inputs: parent ? [{ name: 'scene', type: 'PANO_SCENE', node: nodes.find(node => node.id === parent), slot: 0 }] : [] };
}
