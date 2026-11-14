import { openStudioEditor } from '@panorama-upstream/web_src/studio/editor.js';
import { editorSession } from '@panorama-upstream/web_src/pano_node_bridge.js';
import { waitForPendingStickerUploads } from '@panorama-upstream/web_src/pano_sticker_operations.js';
import { createEditorActions } from './actions.js';
import { loadSceneState, sceneStateText } from '@panorama-upstream/web_src/pano_scene_state.js';
import { inspectStatePreservation } from './state-preservation.js';
import { connectedScenes } from './connected-scenes.js';
import { captureToolbar } from './capture-toolbar.js';

/** Installation companion entry. The product bridge only knows documents and opaque assets;
 * Comfy source naming and route translation belong to this adapter.
 */
export async function openAdapterEditor(initial, host) {
  const protection = inspectStatePreservation(initial.stateText ?? '{}', loadSceneState, sceneStateText);
  if (!protection.canPreview) throw new Error(protection.reason);
  if (protection.readOnly) await host.call('editor.protection', { readOnly: true, message: protection.reason });
  const assets = new Map();
  const inputAssets = new Map();
  const urls = new Set();
  const key = source => `${source.storage ?? source.type ?? 'input'}/${source.subfolder ?? ''}/${source.filename}`;
  const addAsset = asset => {
    const url = URL.createObjectURL(new Blob([asset.bytes], { type: asset.mimeType }));
    urls.add(url);
    assets.set(asset.name, { ...asset, url });
  };
  for (const asset of initial.assets ?? []) addAsset(asset);
  const connected = connectedScenes(initial, asset => {
    const url = URL.createObjectURL(new Blob([asset.bytes], { type: asset.mimeType }));
    urls.add(url); inputAssets.set(asset.name, { ...asset, url });
  });
  const marker = document.createElement('style');
  marker.id = 'pano-studio-style-link'; marker.dataset.loaded = 'true'; document.head.appendChild(marker);
  const api = {
    apiURL(route) {
      const url = new URL(route, 'https://adapter.invalid');
      if (url.pathname !== '/view') throw new Error(`Unsupported media URL: ${url.pathname}`);
      const name = key({ filename: url.searchParams.get('filename'), subfolder: url.searchParams.get('subfolder') ?? '', storage: url.searchParams.get('type') ?? 'input' });
      const asset = assets.get(name) ?? inputAssets.get(name);
      if (!asset) throw new Error(`Missing persistent asset: ${url.searchParams.get('filename')}`);
      return asset.url;
    },
    async fetchApi(route, options = {}) {
      if (route === '/upload/image' && options.method === 'POST' && options.body instanceof FormData) {
        const file = options.body.get('image');
        if (!(file instanceof Blob)) throw new Error('Missing uploaded media');
        const subfolder = String(options.body.get('subfolder') || 'panorama_stickers');
        if (!['panorama_stickers', 'panorama_paint', '3d'].includes(subfolder)) throw new Error(`Unsupported upload folder: ${subfolder}`);
        // Reusing a display filename must not replace the source of an older sticker.
        const filename = `${crypto.randomUUID()}-${file.name.replace(/[\\/]/g, '_')}`;
        const name = `input/${subfolder}/${filename}`;
        const bytes = await file.arrayBuffer();
        const mimeType = file.type || 'application/octet-stream';
        const persisted = await host.call('asset.store', { name, mimeType, bytes });
        addAsset({ ...persisted, name, bytes, mimeType });
        return { status: 200, ok: true, json: async () => ({ name: filename, subfolder, type: 'input' }) };
      }
      if (route === '/panorama_suite/file_3d' && options.method === 'POST') {
        const source = JSON.parse(options.body);
        const original = assets.get(key(source));
        if (!original) throw new Error('3D source does not belong to this document');
        const result = await host.call('operation.run', { serviceId: 'file3d', sourceNames: [original.name], input: { fileName: source.filename } });
        for (const asset of result.assets) addAsset(asset);
        const metadata = JSON.parse(result.data.metadataJson);
        const localSource = descriptor => {
          const asset = result.assets.find(asset => asset.role === descriptor.assetRole);
          if (!asset) throw new Error('Converted asset role is missing');
          // Named document identity is stable across close/reopen and template transport.
          const parts = asset.name.split('/');
          return { filename: parts.pop(), subfolder: parts.slice(1).join('/'), type: parts[0] };
        };
        metadata.file = localSource(metadata.file);
        if (metadata.animation?.source) metadata.animation.source = localSource(metadata.animation.source);
        return { status: 200, ok: true, json: async () => metadata };
      }
      throw new Error(`Editor route is not connected: ${route}`);
    },
  };
  const memory = new Map(Object.entries(initial.preferences ?? {}));
  let preferenceTail = Promise.resolve();
  const pendingPreferences = new Map();
  async function flushPreferences() {
    await preferenceTail;
    for (const [key, value] of pendingPreferences) {
      await host.call('preferences.set', { key, value });
      if (pendingPreferences.get(key) === value) pendingPreferences.delete(key);
    }
  }
  const captureTaskKey = `capture.lastTask:${initial.nodeId}`;
  let toolbar = null;
  const capture = async (action, args) => {
    if (action === 'asset.publish') return { status: 'published', ...await host.call('asset.publish', { name: args.name }) };
    let result;
    if (action === 'capture.collect') result = await host.call('operation.collect', { taskId: args.taskId });
    else {
      const serviceId = {'capture.frame':'frameCapture','repair.frameReturn':'frameReturn','repair.seamPrepare':'seamPrepare','repair.seamReturn':'seamReturn'}[action];
      const input = {outputProjection: ['frameCapture','seamPrepare'].includes(serviceId) ? 'flat' : 'equirectangular360',
        advanced: {megapixels: args.megapixels ?? 2, multiple: args.multiple ?? 8}};
      for (const key of ['sourceName','provenanceName','bandWidth','offset','blur']) if (args[key] !== undefined) input[key]=args[key];
      result = await host.call('operation.run', {serviceId,sourceNames:[],input});
    }
    await host.call('preferences.set', { key: captureTaskKey, value: result.taskId });
    memory.set(captureTaskKey, result.taskId);
    const roles={frame:'取景',mask:'遮罩',softMask:'羽化遮罩',pose:'取景记录',provenance:'接缝记录',shifted:'接缝工作图',repaired:'修补全景'};
    for (const asset of result.assets) {
      const key=`assetLabel:${initial.nodeId}:${asset.name}`;
      const label=memory.get(key)??`${roles[asset.role]??'处理成果'} ${[...memory.keys()].filter(k=>k.startsWith(`assetLabel:${initial.nodeId}:`)).length+1}`;
      addAsset({...asset,displayName:label});memory.set(key,label);
      await host.call('preferences.set',{key,value:label});
    }
    toolbar?.refresh(result);
    return { ...result, assets: result.assets.map(({ bytes, ...asset }) => asset) };
  };
  const editor = await openPanoramaStudio({ nodeId: initial.nodeId, stateText: initial.stateText ?? '',
    capture,
    readOnly: protection.readOnly,
    inputs: connected.inputs, graphNodes: connected.nodes,
    app: { nodeOutputs: connected.layers.length ? { [initial.nodeId]: { pano_upstream_layers: connected.layers } } : {} }, api, stylesheetUrl: 'preloaded',
    assetFile: name => {
      const asset = assets.get(name);
      if (!asset) throw new Error('Named asset does not belong to this document');
      return new File([asset.bytes], name.split('/').at(-1), { type: asset.mimeType });
    },
    storage: { getItem: key => memory.get(key) ?? null, setItem: (key, value) => {
      value = String(value); memory.set(key, value); pendingPreferences.set(key, value);
      preferenceTail = preferenceTail.then(() => host.call('preferences.set', { key, value }))
        .then(() => { if (pendingPreferences.get(key) === value) pendingPreferences.delete(key); }).catch(() => {});
    } },
    onStateChange: stateText => protection.readOnly ? undefined : host.call('document.save', {
      stateText,
      assets: [...assets.values()].map(({ name, mediaRel, sha256, mimeType }) => ({ name, mediaRel, sha256, mimeType })),
    }),
    onClosed: () => { void host.call('editor.closed', null).catch(() => {}); },
  });
  toolbar = protection.readOnly ? null : captureToolbar(editor, () => memory.get(captureTaskKey), () => [...assets.values()].map(asset=>({...asset,displayName:memory.get(`assetLabel:${initial.nodeId}:${asset.name}`)??asset.displayName})));
  return { read: editor.read, describe: async () => ({ ...await editor.describe(), protection }), act: editor.act, async flush() { const result = await editor.flush(); await flushPreferences(); return result; }, async close() {
    const result = await editor.close();
    await flushPreferences();
    for (const url of urls) URL.revokeObjectURL(url);
    marker.remove();
    toolbar?.remove();
    return result;
  } };
}

/** Thin context bridge. All scene semantics, UI, paint and timeline stay in upstream.
 * The host owns persistent media and transport; no host credentials enter the editor.
 * api implements the upstream media routes, app supplies actual connected input results.
 */
export async function openPanoramaStudio({ nodeId, stateText = '', inputs = [], graphNodes = [], app, api, stylesheetUrl, storage, onStateChange, onClosed, readOnly = false, assetFile, capture }) {
  if (typeof nodeId !== 'string' || !nodeId) throw new Error('A stable scene node identity is required');
  if (typeof stateText !== 'string') throw new Error('Scene state must be JSON text');
  if (!api || typeof api.apiURL !== 'function' || typeof api.fetchApi !== 'function') throw new Error('Persistent media transport is required');
  await loadStylesheet(stylesheetUrl);
  let closed = false;
  let error = null;
  let lastUnsaved = null;
  let opened = false;
  let closeNotified = false;
  let pendingWrite = Promise.resolve();
  const widget = {
    name: 'state_json', value: stateText,
    callback(text) {
      lastUnsaved = text;
      // Serialize persistence, never let a slower previous write replace the newest state.
      pendingWrite = pendingWrite.then(async () => {
        await onStateChange?.(text);
        if (lastUnsaved === text) lastUnsaved = null;
        error = null;
      }).catch(cause => { error = cause; });
    },
  };
  const origins = new Map(inputs.map((input, i) => [i + 1, input.node]));
  const node = {
    id: nodeId, title: '全景工作台', __panoStudio: true, __panoSceneStateV2: true,
    widgets: [widget],
    inputs: inputs.map((input, i) => ({ name: input.name, type: input.type, link: i + 1 })),
    graph: { getNodeById: id => origins.get(id) ?? null, _nodes: graphNodes },
    isInputConnected: index => !!inputs[index],
    getInputLink: index => inputs[index] ? { origin_id: index + 1, origin_slot: inputs[index].slot ?? 0 } : null,
    getInputNode: index => inputs[index]?.node ?? null,
    setDirtyCanvas() {
      if (opened && !editorSession(node).open && !closeNotified) {
        closeNotified = true; onClosed?.();
      }
    },
  };
  const editor = await openStudioEditor(node, { app, api, storage, readOnly });
  if (!editor) throw new Error('Scene editor is already open');
  opened = true;
  async function flush() {
    if (!closed) await editorSession(node).flushBeforeQueue?.();
    await waitForPendingStickerUploads(node);
    await pendingWrite;
    if (error && lastUnsaved !== null) {
      await onStateChange?.(lastUnsaved);
      lastUnsaved = null; error = null;
    }
    return widget.value;
  }
  return {
    read: () => structuredClone(editor.session.state),
    ...createEditorActions(editor, flush, { assetFile, capture }),
    flush,
    async close() {
      if (!closed) { closed = true; editor.close(); }
      return flush();
    },
  };
}

async function loadStylesheet(url) {
  if (!url) throw new Error('Editor stylesheet URL is required');
  const existing = document.getElementById('pano-studio-style-link');
  if (existing?.dataset.loaded === 'true') return;
  if (existing) throw new Error('Editor stylesheet is already loading');
  const link = document.createElement('link');
  link.id = 'pano-studio-style-link';
  link.rel = 'stylesheet';
  link.href = url;
  await new Promise((resolve, reject) => {
    link.onload = () => { link.dataset.loaded = 'true'; resolve(); };
    link.onerror = () => { link.remove(); reject(new Error('Cannot load editor stylesheet')); };
    document.head.appendChild(link);
  });
}
