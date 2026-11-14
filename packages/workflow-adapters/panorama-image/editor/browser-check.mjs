import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';

// Exercises the built production bundle, not an upstream test fixture. No ComfyUI or weights.
const args = process.argv.slice(2);
const option = name => args.includes(name) ? args[args.indexOf(name) + 1] : null;
for (const name of ['--source', '--bundle', '--browser', '--results']) if (!option(name)) throw new Error(`Missing ${name}`);
const root = resolve(option('--bundle'));
const results = resolve(option('--results'));
const frameScript = option('--frame') ? await readFile(resolve(option('--frame'))) : null;
await mkdir(results, { recursive: true });
const { chromium } = await import(pathToFileURL(resolve(option('--source'), 'node_modules/playwright/index.mjs')));
const server = createServer(async (request, response) => {
  const pathname = new URL(request.url, 'http://localhost').pathname;
  if (pathname === '/favicon.ico') { response.writeHead(204).end(); return; }
  if (pathname === '/frame.js' && frameScript) { response.setHeader('Content-Type', 'application/javascript'); response.end(frameScript); return; }
  if (pathname === '/') { response.setHeader('Content-Type', 'text/html'); response.end('<!doctype html><meta charset="utf-8"><body style="margin:0" class="dark-theme">'); return; }
  const file = resolve(root, '.' + decodeURIComponent(pathname));
  if (!file.startsWith(root + sep)) { response.writeHead(403).end(); return; }
  try {
    const body = await readFile(file);
    response.setHeader('Content-Type', pathname.endsWith('.js') ? 'application/javascript' : pathname.endsWith('.css') ? 'text/css' : pathname.endsWith('.json') ? 'application/json' : 'application/octet-stream');
    response.end(body);
  } catch { response.writeHead(404).end(); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
let browser;
try {
  browser = await chromium.launch({ executablePath: resolve(option('--browser')), headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const problems = [];
  page.on('pageerror', error => problems.push(error.message));
  page.on('console', message => { if (message.type() === 'error') problems.push(`${message.text()} (${message.location().url})`); });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.evaluate(async sandbox => {
    const { openPanoramaStudio } = await import('/studio.js');
    const assets = new Map();
    const references = [['desk', 0, '#ff2020'], ['bedroom', -90, '#20ff20'], ['entrance', 180, '#2020ff']];
    const stickers = references.map(([id, yaw, color], i) => {
      const canvas = document.createElement('canvas'); canvas.width = 200; canvas.height = 100;
      const ctx = canvas.getContext('2d'); ctx.fillStyle = color; ctx.fillRect(0, 0, 200, 100);
      assets.set(`${id}.png`, canvas.toDataURL());
      return { id, placement: 'sky', source: { kind: 'file', filename: `${id}.png`, subfolder: '', storage: 'input' }, direction: { value: { yaw_deg: yaw, pitch_deg: 0 } }, size: { value: { hFOV_deg: 60, vFOV_deg: 30 } }, z_index: i + 1 };
    });
    let saved = JSON.stringify({ version: 2, stickers });
    let editor;
    let frame;
    const storedAssets = new Map();
    window.check = {
      writes: 0,
      failWrites: false,
      storedAssets,
      preferences: {},
      async open() {
        window.check.protection = null;
        if (sandbox) {
          const { openEditorFrame } = await import('/frame.js');
          const bundle = await (await fetch('/editor-bundle.json')).json();
          frame = document.createElement('iframe'); frame.style.cssText = 'position:fixed;inset:0;width:100%;height:100%;border:0'; document.body.appendChild(frame);
          const initialAssets = await Promise.all([...assets].map(async ([filename, url]) => ({ name: `input//${filename}`, bytes: await (await fetch(url)).arrayBuffer(), mimeType: 'image/png', mediaRel: `fixtures/${filename}`, sha256: window.check.geometryFixture ? 'a'.repeat(64) : 'test-only' })));
          const present=asset=>{if(bundle.assetTransport!=='blob-v1')return asset;const {bytes,blob,...metadata}=asset;return {...metadata,assetKey:asset.name};};
          editor = await openEditorFrame(frame, bundle, { nodeId: 'test_scene', stateText: saved, preferences: window.check.preferences, generationDraft:window.check.generationDraft, assets: [...initialAssets, ...storedAssets.values()].map(present),
            inputNodeIds: window.check.inputNodeIds ?? [], connectedDocuments: (window.check.connectedDocuments ?? []).map(source => ({ ...source, assets: initialAssets.map(present) })) }, async (method, params) => {
            if(method==='asset.load'){
              const asset=initialAssets.find(a=>a.name===params.assetKey)??storedAssets.get(params.assetKey);
              if(!asset)throw new Error('Unknown owned fixture asset');
              (window.check.loadedAssets??=[]).push(asset.name);
              return asset.blob??new Blob([asset.bytes],{type:asset.mimeType});
            }
            if (method === 'editor.closed') return;
            if(method==='operation.run'&&params.serviceId==='inputPreview'&&window.check.inputFixture){
              const state=JSON.parse(params.input.stateJson),sticker=state.stickers.find(item=>item.source.kind==='port')??{...structuredClone(state.stickers[0]),id:`sticker_${state.next_sticker_id}`,placement:'sky',source:{kind:'port'}};
              window.check.inputPreviews=(window.check.inputPreviews??0)+1;
              return {status:'completed',assets:[],data:{metadataJson:JSON.stringify({sticker,nextStickerId:state.next_sticker_id+1})}};
            }
            if (method === 'preferences.set') { window.check.preferences[params.key] = params.value; return; }
            if (method === 'editor.protection') { window.check.protection = params; return; }
            if (window.check.protection?.readOnly) throw new Error('Protected fixture must not write');
            if(window.check.geometryFixture && method==='operation.run' && params.serviceId==='geometryModels')return {status:'completed',taskId:'models',assets:[],data:{metadataJson:JSON.stringify({da3:['DA3-BASE.safetensors'],moge:['moge_3_vitg_fp16.safetensors']})}};
            if(window.check.geometryFixture && method==='operation.run' && params.serviceId==='geometry'){
              if(params.input.editorDocument||params.input.sourceRevision||params.sourceNames.length)throw new Error('Geometry cannot forge host document ownership');
              window.check.geometrySource=JSON.parse(params.input.geometrySourceJson);window.check.geometryTarget=params.input.geometryTarget??'world';window.check.geometrySubmits=(window.check.geometrySubmits??0)+1;
              return {status:'pending',taskId:'geometry-original',assets:[],data:null};
            }
            if(window.check.geometryFixture && method==='operation.collect' && params.taskId==='geometry-original'){
              const asset={role:'geometryFile',name:'editor-results/geometry-original/0',mediaRel:'fixtures/geometry.glb',sha256:'b'.repeat(64),mimeType:'application/octet-stream',bytes:new Uint8Array(window.check.geometryFixture).buffer};
              storedAssets.set(asset.name,asset);
              const source=window.check.geometrySource;
              const prefix=window.check.geometryTarget==='sticker'?'pano_sticker':'pano_scene';
              return {status:'completed',taskId:params.taskId,assets:[present(asset)],data:{metadataJson:JSON.stringify({version:1,source,target:window.check.geometryTarget,
                sourceAssets:(source.video?[source.video]:source.images).map(file=>({name:`${file.storage}/${file.subfolder}/${file.filename}`,sha256:'a'.repeat(64)})),
                ui:{[`${prefix}_files`]:[{assetIndex:0,format:'model/gltf-binary'}],[`${prefix}_meta`]:[{kind:'mesh',unlit:true}],...(window.check.geometryTarget==='sticker'?{pano_sticker_input_pose:[{placement:'world',position:[0,0,-3],rotation:[0,0,0],scale:1}]}:{})}})}};
            }
            if (window.check.captureFixture && method === 'operation.run' && params.serviceId === 'frameCapture') {
              if (params.sourceNames.length || params.input.editorDocument || params.input.sourceRevision) throw new Error('Companion must not forge host document or revision');
              window.check.captures = (window.check.captures ?? 0) + 1;
              return { status: 'pending', taskId: 'capture-original', assets: [], data: null };
            }
            if (window.check.captureFixture && method === 'operation.run' && ['frameReturn','seamPrepare','seamReturn'].includes(params.serviceId)) {
              if (params.sourceNames.length || params.input.editorDocument || params.input.sourceRevision) throw new Error('Repair cannot forge source ownership');
              (window.check.preparationCalls ??= []).push(params);
              return { status:'pending',taskId:'repair-fixture',assets:[],data:null };
            }
            if (window.check.captureFixture && method === 'operation.collect') {
              if (params.taskId !== 'capture-original') throw new Error('Must collect original capture');
              const png = await (await fetch(assets.get('desk.png'))).arrayBuffer();
              const pose = JSON.stringify({ sourceRevision: 'original', stickerState: { kind: 'pano_sticker_state', version: 2, fps: 24, camera: {} } });
              const captured = ['frame','mask','pose'].map((role, index) => ({ role, name: `editor-results/capture-original/${index}`, mediaRel: `fixtures/capture-${index}`, sha256: 'test-only', mimeType: role === 'pose' ? 'application/octet-stream' : 'image/png', bytes: role === 'pose' ? new TextEncoder().encode(pose).buffer : png.slice(0) }));
              for (const asset of captured) storedAssets.set(asset.name, asset);
              return { status: 'completed', taskId: params.taskId, assets: captured, data: { sourceRevision: 'original', stickerState: pose } };
            }
            if (window.check.captureFixture && method === 'asset.publish') {
              if (!storedAssets.has(params.name) || Object.keys(params).length !== 1) throw new Error('Publish must use a retained named result');
              window.check.published=params.name;return {versionId:'retained-version',reusedExisting:false};
            }
            if (method === 'operation.run' && window.check.conversionFixture) {
              if (params.serviceId !== 'file3d' || params.sourceNames.length !== 1 || !storedAssets.has(params.sourceNames[0])) throw new Error('Conversion source is not owned');
              window.check.conversions = (window.check.conversions ?? 0) + 1;
              if(params.input.fileName.endsWith('.splat')){
                const asset={role:'preview',name:'editor-results/fixture/points.ply',mediaRel:'fixtures/points.ply',sha256:'test-only',mimeType:'application/octet-stream',bytes:new Uint8Array(window.check.splatFixture).buffer};
                storedAssets.set(asset.name,asset);
                return {taskId:'splat-fixture',assets:[asset],data:{metadataJson:JSON.stringify({kind:'splat',file:{assetRole:'preview',extension:'ply'},box:{min:[-1,-1,-.1],max:[1,1,.1]}})}};
              }
              const asset = { role: 'preview', name: 'editor-results/fixture/0', mediaRel: 'fixtures/converted.bin', sha256: 'test-only', mimeType: 'model/gltf-binary', bytes: new Uint8Array(window.check.conversionFixture).buffer };
              storedAssets.set(asset.name, asset);
              return { taskId: 'fixture', assets: [asset], data: { metadataJson: JSON.stringify({ kind: 'mesh', unlit: true, file: { assetRole: 'preview', extension: 'glb' }, box: { min: [-1,-1,-1], max: [1,1,1] }, ...(window.check.animationClips ? {animation:{source:{assetRole:'preview',extension:'glb'},clips:window.check.animationClips,box:{min:[-1,-1,-1],max:[1,1,1]}}}: {}) }) } };
            }
            if (method === 'asset.store') {
              const asset = { ...params, mediaRel: `fixtures/import-${storedAssets.size}.png`, sha256: 'test-only' };
              storedAssets.set(params.name, asset);
              return asset;
            }
            if (method !== 'document.save') throw new Error(`Unexpected host method ${method}`);
            if (window.check.failWrites) throw new Error('EDITOR_REVISION_CONFLICT: test concurrent edit');
            saved = params.stateText; window.check.savedAssets = params.assets; window.check.writes++;
          });
          return;
        }
        editor = await openPanoramaStudio({ nodeId: 'test_scene', stateText: saved, stylesheetUrl: '/studio.css',
          app: { nodeOutputs: {} },
          api: {
            apiURL: route => { const url = new URL(route, location.origin); const value = assets.get(url.searchParams.get('filename')); if (!value) throw new Error(`Unexpected asset ${route}`); return value; },
            fetchApi: async route => { throw new Error(`Unexpected network operation ${route}`); },
          },
          onStateChange: async text => { saved = text; window.check.writes++; },
        });
      },
      read: () => editor.read(),
      describe: () => editor.describe(),
      act: async (action, args = {}, digest = null) => editor.act({ action, arguments: args, expectedStateDigest: digest ?? (await editor.describe()).stateDigest }),
      flush: () => editor.flush(),
      close: async () => { const value = await editor.close(); if (sandbox) { editor.dispose(); frame.remove(); } return value; },
      saved: () => JSON.parse(saved),
      savedText: () => saved,
      replaceSaved: text => { saved = text; },
    };
    await window.check.open();
  }, Boolean(option('--frame')));
  const surface = option('--frame') ? page.frameLocator('iframe') : page;
  await surface.locator('[data-stage-canvas]').waitFor();
  if (option('--frame')) {
    const isolated = page.frames().find(frame => frame !== page.mainFrame());
    assert.equal(await isolated.evaluate(() => { try { void parent.document.body; return false; } catch { return true; } }), true);
    assert.equal(await isolated.evaluate(() => typeof window.__TAURI_INTERNALS__), 'undefined');
  }
  if (!['inheritance', 'capture', 'geometry','inputs'].includes(option('--focus'))) {
  assert.deepEqual(await page.evaluate(async () => (await window.check.read()).stickers.map(item => item.direction.value.yaw_deg)), [0, -90, 180]);
  await surface.locator('.pano-studio-tl-head[data-head="bedroom"] [data-part="name"]').click();
  const rotation = surface.locator('[data-region="inspector"] [data-row="Rotation"]');
  await rotation.locator('[data-field="yaw_deg"]').click();
  await rotation.locator('input').first().fill('-100');
  await rotation.locator('input').first().press('Enter');
  assert.equal(await page.evaluate(async () => (await window.check.read()).stickers[1].direction.value.yaw_deg), -100);
  await surface.locator('[data-tool="undo"]').click();
  assert.equal(await page.evaluate(async () => (await window.check.read()).stickers[1].direction.value.yaw_deg), -90);
  await surface.locator('[data-tool="redo"]').click();
  await page.evaluate(() => window.check.close());
  assert.equal(await page.evaluate(() => window.check.saved().stickers[1].direction.value.yaw_deg), -100);
  assert.equal(await page.evaluate(() => window.check.writes), 1);
  await page.evaluate(() => window.check.open());
  assert.deepEqual(await page.evaluate(async () => (await window.check.read()).stickers.map(item => item.direction.value.yaw_deg)), [0, -100, 180]);
  await surface.getByRole('tab', { name: 'Camera', exact: true }).click();
  // Check rendered pixels, not only state: all three references must reach the ERP preview.
  let renderedColors = [];
  for (let attempt = 0; attempt < 20; attempt++) {
    const pixels = await surface.locator('[data-stage-canvas]').screenshot();
    renderedColors = await page.evaluate(async bytes => {
      const bitmap = await createImageBitmap(new Blob([new Uint8Array(bytes)], { type: 'image/png' }));
      const canvas = document.createElement('canvas'); canvas.width = bitmap.width; canvas.height = bitmap.height;
      const ctx = canvas.getContext('2d'); ctx.drawImage(bitmap, 0, 0); bitmap.close();
      const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      const count = [0, 0, 0];
      for (let i = 0; i < data.length; i += 4) {
        if (data[i] > 150 && data[i + 1] < 100 && data[i + 2] < 100) count[0]++;
        if (data[i + 1] > 150 && data[i] < 100 && data[i + 2] < 100) count[1]++;
        if (data[i + 2] > 150 && data[i] < 100 && data[i + 1] < 100) count[2]++;
      }
      return count;
    }, [...pixels]);
    if (renderedColors.every(count => count > 100)) break;
  }
  assert.ok(renderedColors.every(count => count > 100), `Missing rendered reference: ${renderedColors}`);
  await page.screenshot({ path: resolve(results, 'studio-roundtrip-1280.png') });
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.screenshot({ path: resolve(results, 'studio-roundtrip-1600.png') });
  await page.evaluate(() => window.check.close());
  assert.equal(await page.evaluate(() => window.check.writes), 1);
  if (option('--frame')) {
    await page.evaluate(() => window.check.open());
    // Two files with the same original name must keep independent immutable sources.
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aG9sAAAAASUVORK5CYII=', 'base64');
    for (let i = 0; i < 2; i++) {
      await surface.locator('[data-add-picker]').setInputFiles({ name: 'same-name.png', mimeType: 'image/png', buffer: png });
      await surface.getByRole('button', { name: 'Sky', exact: true }).click();
      await page.waitForFunction(count => window.check.storedAssets.size === count, i + 1);
      await page.waitForFunction(async count => (await window.check.read()).stickers.length === count, i + 4);
    }
    const imported = await page.evaluate(async () => (await window.check.read()).stickers.slice(3).map(item => item.source.filename));
    assert.equal(new Set(imported).size, 2);
    await page.evaluate(() => { window.check.failWrites = true; });
    await assert.rejects(page.evaluate(() => window.check.flush()), /EDITOR_REVISION_CONFLICT/);
    await page.evaluate(() => { window.check.failWrites = false; });
    await page.evaluate(() => window.check.flush());
    await page.evaluate(() => window.check.close());
    await page.evaluate(() => window.check.open());
    assert.deepEqual(await page.evaluate(async () => (await window.check.read()).stickers.slice(3).map(item => item.source.filename)), imported);
    if (option('--camera')) {
      const camera={kind:'camera',id:'camera'};
      await page.evaluate(target=>window.check.act('selection.set',{row:'camera'}),camera);
      await page.evaluate(target=>window.check.act('inspector.choice',{target,field:'projection',value:'perspective'}),camera);
      await page.evaluate(target=>window.check.act('inspector.field',{target,field:'hFOV_deg',value:65}),camera);
      await page.evaluate(target=>window.check.act('inspector.field',{target,field:'aspect',value:1.5}),camera);
      await page.evaluate(()=>window.check.act('view.tab',{tab:'camera'}));
      let state=await page.evaluate(()=>window.check.read());
      assert.equal(state.camera.projection,'perspective'); assert.equal(state.camera.aspect,1.5); assert.equal(state.camera.fov.value,65);
      await page.screenshot({path:resolve(results,'studio-frame-camera.png')});
      await page.evaluate(()=>window.check.close()); await page.evaluate(()=>window.check.open());
      state=await page.evaluate(()=>window.check.read());
      assert.equal(state.camera.projection,'perspective'); assert.equal(state.camera.fov.value,65);
      await page.evaluate(target=>window.check.act('inspector.choice',{target,field:'projection',value:'erp'}),camera);
      await page.evaluate(target=>window.check.act('inspector.choice',{target,field:'erp_range',value:180}),camera);
      assert.equal((await page.evaluate(()=>window.check.read())).camera.erp_range,180);
      await page.screenshot({path:resolve(results,'studio-180-camera.png')});
      await page.evaluate(target=>window.check.act('inspector.choice',{target,field:'erp_range',value:360}),camera);
      await page.evaluate(()=>window.check.act('view.tab',{tab:'3d'}));
    }
    if (option('--actions')) {
      const initial = await page.evaluate(() => window.check.describe());
      assert.ok(initial.actions.some(action => action.id === 'timeline.keys.move'));
      await page.evaluate(() => window.check.act('inspector.field', { target: { kind: 'sticker', id: 'bedroom' }, field: 'yaw_deg', value: -120 }));
      assert.equal(await page.evaluate(async () => (await window.check.read()).stickers[1].direction.value.yaw_deg), -120);
      await assert.rejects(page.evaluate(digest => window.check.act('history.undo', {}, digest), initial.stateDigest), /EDITOR_STATE_CONFLICT/);
      await page.evaluate(() => window.check.act('history.undo'));
      assert.equal(await page.evaluate(async () => (await window.check.read()).stickers[1].direction.value.yaw_deg), -100);
      await page.evaluate(() => window.check.act('timeline.length', { value: 120 }));
      await page.evaluate(() => window.check.act('timeline.seek', { frame: 24 }));
      await page.evaluate(() => window.check.act('timeline.key', { row: 'bedroom' }));
      assert.equal(await page.evaluate(async () => (await window.check.read()).stickers[1].direction.keyframes[0].frame), 24);
      await page.evaluate(() => window.check.act('selection.set', { row: 'bedroom', keys: [{ row: 'bedroom', lane: 'direction', frame: 24 }] }));
      await page.evaluate(() => window.check.act('timeline.keys.move', { delta: 12 }));
      assert.equal(await page.evaluate(async () => (await window.check.read()).stickers[1].direction.keyframes[0].frame), 36);
      await page.evaluate(() => window.check.act('object.command', { command: 'duplicate' }));
      assert.equal(await page.evaluate(async () => (await window.check.read()).stickers.length), 6);
      await page.evaluate(() => window.check.act('history.undo'));
      await page.evaluate(() => window.check.close());
      await page.evaluate(() => window.check.open());
      assert.equal(await page.evaluate(async () => (await window.check.read()).stickers[1].direction.keyframes[0].frame), 36);
      // History owns only this editor session and retains exactly the latest 50 successful changes.
      for (let value = 1; value <= 55; value++) await page.evaluate(value => window.check.act('inspector.field', { target: { kind: 'sticker', id: 'desk' }, field: 'yaw_deg', value }), value);
      for (let i = 0; i < 50; i++) await page.evaluate(() => window.check.act('history.undo'));
      assert.equal(await page.evaluate(async () => (await window.check.read()).stickers[0].direction.value.yaw_deg), 5);
      assert.equal((await page.evaluate(() => window.check.describe())).canUndo, false);
      if (option('--paint')) {
        await page.evaluate(() => window.check.act('paint.tool', { tool: 'mask' }));
        await page.evaluate(() => window.check.act('paint.stroke', { points: [{ yaw: -10, pitch: 0 }, { yaw: 0, pitch: 5 }, { yaw: 10, pitch: 0 }] }));
        assert.equal(await page.evaluate(async () => (await window.check.read()).painting.mask.strokes.length), 1);
        await page.evaluate(() => window.check.act('paint.clear'));
        assert.ok((await page.evaluate(() => window.check.describe())).confirm);
        await page.evaluate(() => window.check.act('paint.confirmClear', { confirmed: false }));
        assert.equal(await page.evaluate(async () => (await window.check.read()).painting.mask.strokes.length), 1);
        await page.evaluate(() => window.check.act('paint.clear'));
        await page.evaluate(() => window.check.act('paint.confirmClear', { confirmed: true }));
        assert.equal(await page.evaluate(async () => (await window.check.read()).painting.mask.strokes.length), 0);
        await page.evaluate(() => window.check.act('history.undo'));
        assert.equal(await page.evaluate(async () => (await window.check.read()).painting.mask.strokes.length), 1);
        await assert.rejects(page.evaluate(() => window.check.act('paint.stroke', { points: [{ yaw: 181, pitch: 0 }] })), /outside the sphere/);
        await page.evaluate(() => window.check.act('paint.tool', { tool: 'cursor' }));
        await page.evaluate(() => window.check.act('view.tab', { tab: '3d' }));
        await page.evaluate(() => window.check.act('asset.add', { name: 'input//desk.png', placement: 'sky' }));
        assert.equal(await page.evaluate(async () => (await window.check.read()).stickers.length), 6);
        await page.evaluate(() => window.check.close());
        await page.evaluate(() => window.check.open());
        assert.equal(await page.evaluate(async () => (await window.check.read()).painting.mask.strokes.length), 1);
      }
    }
    if (option('--3d')) {
      const glb = await readFile(resolve(option('--source'), 'tests/fixtures/gltf_anim/anim.glb'));
      const gltf=JSON.parse(glb.subarray(20,20+glb.readUInt32LE(12)).toString());
      const clips=gltf.animations.map((clip,index)=>({name:clip.name||`animation_${index}`,duration:Math.max(...clip.samplers.map(s=>gltf.accessors[s.input].max[0]))}));
      await page.evaluate(({bytes,clips}) => { window.check.conversionFixture = bytes; window.check.animationClips=clips; }, {bytes:[...glb],clips});
      await surface.locator('[data-add-picker]').setInputFiles({ name: 'room.glb', mimeType: 'model/gltf-binary', buffer: glb });
      await page.waitForFunction(() => window.check.conversions > 0);
      await page.waitForFunction(async () => (await window.check.read()).stickers.some(s => s.source.filename.endsWith('-room.glb')));
      await page.evaluate(() => window.check.flush());
      assert.equal(await page.evaluate(() => window.check.storedAssets.has('editor-results/fixture/0')), true);
      await page.evaluate(() => window.check.close());
      await page.evaluate(() => window.check.open());
      await page.waitForFunction(() => window.check.conversions >= 2);
      assert.ok(await page.evaluate(async () => (await window.check.read()).stickers.some(s => s.source.filename.endsWith('-room.glb'))));
      await page.evaluate(async () => {
        const stickers = (await window.check.read()).stickers;
        for (const sticker of stickers) if (!sticker.source.filename.endsWith('-room.glb') && sticker.visible) await window.check.act('object.visibility', { row: sticker.id });
        const mesh = stickers.find(s => s.source.filename.endsWith('-room.glb'));
        await window.check.act('selection.set', { row: mesh.id });
        await window.check.act('view.frameSelected', { row: mesh.id });
      });
      await page.waitForFunction(async()=>JSON.stringify((await window.check.describe()).inspector).includes('anim_clip'));
      await surface.getByRole('radio',{name:'wave',exact:true}).click();
      const meshId=await page.evaluate(async()=>(await window.check.read()).stickers.find(s=>s.source.filename.endsWith('-room.glb')).id);
      await page.evaluate(async id=>{
        await window.check.act('inspector.choice',{target:{kind:'sticker',id},field:'anim_clip',value:1});
        await window.check.act('inspector.field',{target:{kind:'sticker',id},field:'anim_speed',value:1.5});
        await window.check.act('timeline.seek',{frame:24});
        await window.check.flush();
      },meshId);
      assert.deepEqual(await page.evaluate(async id=>(await window.check.read()).stickers.find(s=>s.id===id).animation,meshId),{clip:clips[1].name,speed:1.5});
      await page.evaluate(()=>window.check.close());await page.evaluate(()=>window.check.open());
      await page.waitForFunction(()=>window.check.conversions>=3);
      assert.deepEqual(await page.evaluate(async id=>(await window.check.read()).stickers.find(s=>s.id===id).animation,meshId),{clip:clips[1].name,speed:1.5});
      await page.evaluate(async id=>{await window.check.act('selection.set',{row:id});await window.check.act('view.frameSelected',{row:id});await window.check.act('timeline.seek',{frame:24});},meshId);
      await surface.getByText(/Loading 3D model/).waitFor({state:'hidden',timeout:30000});
      await surface.locator('[data-stage-canvas]').screenshot();
      await page.screenshot({ path: resolve(results, 'studio-3d-managed-roundtrip.png') });
      const splat=Buffer.alloc(64*32);
      for(let i=0;i<64;i++){
        const at=i*32;splat.writeFloatLE((i%8-3.5)*.25,at);splat.writeFloatLE((Math.floor(i/8)-3.5)*.25,at+4);splat.writeFloatLE(0,at+8);
        for(const offset of [12,16,20])splat.writeFloatLE(.08,at+offset);
        splat.set([30,180,255,255,255,128,128,128],at+24);
      }
      // Upstream converts all splat formats to a 3DGS PLY for its Spark loader.
      const properties=['x','y','z','f_dc_0','f_dc_1','f_dc_2','opacity','scale_0','scale_1','scale_2','rot_0','rot_1','rot_2','rot_3'];
      const header=Buffer.from(`ply\nformat binary_little_endian 1.0\nelement vertex 64\n${properties.map(name=>`property float ${name}\n`).join('')}end_header\n`);
      const points=Buffer.alloc(64*properties.length*4);
      for(let i=0;i<64;i++)[(i%8-3.5)*.25,(Math.floor(i/8)-3.5)*.25,0,...[30,180,255].map(v=>(v/255-.5)/.28209479177387814),5,...Array(3).fill(Math.log(.08)),1,0,0,0].forEach((value,k)=>points.writeFloatLE(value,(i*properties.length+k)*4));
      await page.evaluate(bytes=>{window.check.splatFixture=bytes;},[...Buffer.concat([header,points])]);
      await surface.locator('[data-add-picker]').setInputFiles({name:'points.splat',mimeType:'application/octet-stream',buffer:splat});
      await page.waitForFunction(async()=>(await window.check.read()).stickers.some(s=>s.source.filename.endsWith('-points.splat')));
      const splatId=await page.evaluate(async()=>{
        const state=await window.check.read();const splat=state.stickers.find(s=>s.source.filename.endsWith('-points.splat'));
        for(const item of state.stickers)if(item.id!==splat.id&&item.visible)await window.check.act('object.visibility',{row:item.id});
        await window.check.act('selection.set',{row:splat.id});await window.check.act('view.frameSelected',{row:splat.id});await window.check.flush();return splat.id;
      });
      await surface.getByText(/Loading 3D model/).waitFor({state:'hidden',timeout:30000});
      await surface.locator('[data-stage-canvas]').screenshot({path:resolve(results,'studio-splat.png')});
      await page.evaluate(()=>window.check.close());await page.evaluate(()=>window.check.open());
      assert.ok(await page.evaluate(async id=>(await window.check.read()).stickers.some(s=>s.id===id&&s.source.filename.endsWith('-points.splat')),splatId));
      await surface.getByText(/Loading 3D model/).waitFor({state:'hidden',timeout:30000});
    }
    await page.evaluate(() => window.check.close());
  }
  } else { await page.evaluate(() => window.check.close()); }
  if(option('--focus')==='inputs'){
    if(!option('--frame'))throw new Error('Inputs must exercise the production frame bridge');
    const glb=await readFile(resolve(option('--source'),'tests/fixtures/gltf_anim/anim.glb'));
    await page.evaluate(bytes=>{
      window.check.inputFixture=true;window.check.conversionFixture=bytes;window.check.generationDraft={adapterParameters:{'/advanced/backgroundRange':'180','/advanced/backgroundColor':'#203040'}};
      window.check.storedAssets.set('input/3d/room.glb',{name:'input/3d/room.glb',mimeType:'model/gltf-binary',mediaRel:'fixtures/room.glb',sha256:'test-only',bytes:new Uint8Array(bytes).buffer});
    },[...glb]);
    await page.evaluate(()=>window.check.open());
    await surface.getByRole('button',{name:'外部输入',exact:true}).click();
    await surface.getByRole('combobox',{name:'输入素材',exact:true}).selectOption('input//desk.png');
    await page.waitForFunction(()=>window.check.saved().bgxInputs?.background?.source.filename==='desk.png');
    await page.evaluate(()=>window.check.act('timeline.fps',{value:29.97}));
    assert.equal(await page.evaluate(()=>window.check.saved().bgxInputs.background.source.filename),'desk.png');
    await surface.getByRole('radio',{name:'空间',exact:true}).check();
    await surface.getByRole('combobox',{name:'输入素材',exact:true}).selectOption('input/3d/room.glb');
    await page.waitForFunction(()=>window.check.saved().bgxInputs?.world?.preview);
    await page.evaluate(()=>window.check.act('input.set',{role:'sticker',name:'input//bedroom.png'}));
    assert.equal(await page.evaluate(async()=>(await window.check.read()).stickers.filter(item=>item.source.kind==='port').length),1);
    const before=await page.evaluate(()=>window.check.saved().bgxInputs);
    await page.evaluate(()=>window.check.act('input.remove',{role:'world'}));
    await page.evaluate(()=>window.check.act('history.undo'));
    assert.deepEqual(await page.evaluate(()=>window.check.saved().bgxInputs),before);
    await page.screenshot({path:resolve(results,'external-inputs.png')});
    await page.evaluate(()=>window.check.close());await page.evaluate(()=>window.check.open());
    assert.deepEqual(await page.evaluate(async()=>(await window.check.read()).bgxInputs),before);
    assert.equal(await page.evaluate(()=>window.check.conversions),1);
    assert.equal(await page.evaluate(()=>window.check.inputPreviews??0),0);
    await page.evaluate(()=>window.check.close());
    await page.evaluate(()=>{const state=window.check.saved();state.bgxInputs.future=true;window.check.replaceSaved(JSON.stringify(state));});
    await page.evaluate(()=>window.check.open());
    assert.equal((await page.evaluate(()=>window.check.describe())).protection.readOnly,true);
    await assert.rejects(page.evaluate(()=>window.check.act('input.remove',{role:'background'})),/read-only/);
    await page.evaluate(()=>window.check.close());assert.equal(await page.evaluate(()=>window.check.saved().bgxInputs.future),true);
    console.log(JSON.stringify({inputsPassed:true,checks:['background preview uses saved generation parameters','three owned input roles','fps preserves input records','port sticker visible in state','remove undo','all inputs reopen without reconversion','unknown input read-only']}));
  }
  if(option('--focus')==='geometry'){
    if(!option('--frame'))throw new Error('Geometry must exercise the production frame bridge');
    const glb=await readFile(resolve(option('--source'),'tests/fixtures/gltf_anim/anim.glb'));
    await page.evaluate(bytes=>{window.check.geometryFixture=bytes;},[...glb]);
    await page.evaluate(()=>window.check.open());
    await surface.getByRole('button',{name:'几何输入',exact:true}).click();
    await surface.getByRole('button',{name:'读取模型',exact:true}).click();
    await surface.getByRole('radio',{name:'DA3-BASE.safetensors',exact:true}).check();
    await surface.getByRole('checkbox',{name:'desk.png',exact:true}).check();
    await surface.getByRole('button',{name:'准备几何',exact:true}).click();
    await page.waitForFunction(()=>window.check.geometrySubmits===1);
    assert.equal(await page.evaluate(async()=>(await window.check.read()).bgxGeometry),undefined);
    await surface.getByRole('button',{name:'采用几何',exact:true}).click();
    await page.waitForFunction(()=>window.check.saved().bgxGeometry?.preview);
    assert.equal(await page.evaluate(async()=>(await window.check.read()).stickers.length),3);
    await page.evaluate(()=>window.check.act('history.undo'));
    assert.equal(await page.evaluate(async()=>(await window.check.read()).bgxGeometry),undefined);
    await page.evaluate(()=>window.check.act('history.redo'));
    assert.equal(await page.evaluate(async()=>(await window.check.read()).bgxGeometry.source.algorithm),'da3');
    await page.evaluate(()=>window.check.close());await page.evaluate(()=>window.check.open());
    assert.equal(await page.evaluate(async()=>(await window.check.read()).bgxGeometry.preview.ui.pano_scene_files[0].assetName),'editor-results/geometry-original/0');
    assert.equal(await page.evaluate(()=>window.check.geometrySubmits),1);
    await surface.getByRole('button',{name:'几何输入',exact:true}).click();
    assert.equal(await surface.getByRole('checkbox',{name:'desk.png',exact:true}).isChecked(),true);
    assert.equal(await surface.getByText('DA3-BASE.safetensors',{exact:true}).count(),1);
    await page.screenshot({path:resolve(results,'geometry-panel.png')});
    await page.evaluate(()=>window.check.close());
    await page.evaluate(()=>{window.check.storedAssets.set('input//walk.mp4',{name:'input//walk.mp4',mimeType:'video/mp4',mediaRel:'fixtures/walk.mp4',sha256:'a'.repeat(64),bytes:new Uint8Array([0,0,0,24]).buffer});});
    await page.evaluate(()=>window.check.open());
    await surface.getByRole('button',{name:'几何输入',exact:true}).click();
    await surface.getByRole('radio',{name:'视频',exact:true}).check();
    await surface.getByRole('radio',{name:'walk.mp4',exact:true}).check();
    await surface.getByRole('button',{name:'准备几何',exact:true}).click();
    await page.waitForFunction(()=>window.check.geometrySubmits===2);
    assert.equal(await page.evaluate(()=>window.check.geometrySource.images),undefined);
    assert.equal(await page.evaluate(()=>window.check.geometrySource.video.filename),'walk.mp4');
    await surface.getByRole('button',{name:'采用几何',exact:true}).click();
    await page.waitForFunction(()=>window.check.saved().bgxGeometry?.source.video);
    await page.evaluate(()=>window.check.close());await page.evaluate(()=>window.check.open());
    await surface.getByRole('button',{name:'几何输入',exact:true}).click();
    assert.equal(await surface.getByRole('radio',{name:'视频',exact:true}).isChecked(),true);
    assert.equal(await surface.getByRole('radio',{name:'walk.mp4',exact:true}).isChecked(),true);
    assert.equal(await page.evaluate(()=>window.check.loadedAssets.includes('input//walk.mp4')),false);
    assert.ok(await page.evaluate(()=>window.check.loadedAssets.includes('editor-results/geometry-original/0')));
    assert.equal(await page.evaluate(()=>window.check.geometrySubmits),2);
    await page.screenshot({path:resolve(results,'geometry-video-panel.png')});
    await page.evaluate(()=>window.check.act('geometry.remove'));
    await surface.getByRole('radiogroup',{name:'几何用途',exact:true}).getByRole('radio',{name:'贴图',exact:true}).check();
    await surface.getByRole('button',{name:'准备几何',exact:true}).click();
    await page.waitForFunction(()=>window.check.geometrySubmits===3);
    await surface.getByRole('button',{name:'采用几何',exact:true}).click();
    await page.waitForFunction(()=>window.check.saved().bgxGeometry?.target==='sticker');
    assert.equal(await page.evaluate(async()=>(await window.check.read()).stickers.find(s=>s.source.kind==='port').placement),'world');
    await page.evaluate(()=>window.check.close());await page.evaluate(()=>window.check.open());
    assert.equal(await page.evaluate(async()=>(await window.check.read()).bgxGeometry.target),'sticker');
    await page.evaluate(()=>window.check.close());
    await page.evaluate(()=>{const saved=window.check.saved();saved.bgxGeometry.version=999;window.check.replaceSaved(JSON.stringify(saved));});
    await page.evaluate(()=>window.check.open());
    assert.equal((await page.evaluate(()=>window.check.describe())).protection.readOnly,true);
    await assert.rejects(page.evaluate(()=>window.check.act('geometry.remove')),/read-only/);
    await page.evaluate(()=>window.check.close());
    assert.equal(await page.evaluate(()=>window.check.saved().bgxGeometry.version),999);
    console.log(JSON.stringify({geometryPassed:true,checks:['model discovery','owned source UI','prepare is not adoption','native history undo/redo','saved geometry and preview reopen','video source selection and reopen (no video decoding)','old stickers retained','no repeated inference','future geometry preserved read-only']}));
  }
  if (option('--focus') === 'capture') {
    if (!option('--frame')) throw new Error('Capture check requires production frame');
    await page.evaluate(() => {
      const state = window.check.saved(); state.camera = { ...state.camera, projection: 'perspective' };
      window.check.replaceSaved(JSON.stringify(state)); window.check.captureFixture = true;
    });
    await page.evaluate(() => window.check.open());
    await surface.getByRole('button', { name: '取景与遮罩', exact: true }).click();
    await surface.getByRole('status').filter({ hasText: '等待处理' }).waitFor();
    assert.equal(await page.evaluate(() => window.check.captures), 1);
    await page.evaluate(() => window.check.close()); await page.evaluate(() => window.check.open());
    await surface.getByRole('button', { name: '收取成果', exact: true }).click();
    await surface.getByRole('status').filter({ hasText: '已收取' }).waitFor();
    const state = await page.evaluate(() => window.check.describe());
    assert.equal(state.service.status, 'completed');assert.equal(state.service.assets.length, 3);
    assert.equal(state.service.data.sourceRevision, 'original');
    assert.equal(await page.evaluate(() => window.check.captures), 1);
    await page.screenshot({ path: resolve(results, 'studio-capture-toolbar.png') });
    await page.evaluate(() => window.check.close());
    assert.equal(await page.evaluate(() => [...window.check.storedAssets.keys()].filter(name => name.startsWith('editor-results/capture-original/')).length), 3);
    await page.evaluate(() => window.check.open());
    await page.evaluate(() => window.check.act('asset.add', { name: 'editor-results/capture-original/0', placement: 'sky' }));
    await page.evaluate(() => window.check.close());
    await page.evaluate(() => window.check.open());
    for (const [label,service] of [['修补回贴','frameReturn'],['接缝准备','seamPrepare'],['接缝还原','seamReturn']]) {
      await surface.getByRole('radio',{name:label,exact:true}).check();
      await surface.getByLabel(service==='seamPrepare'?'全景素材':'修补素材',{exact:true}).selectOption('input//desk.png');
      if(service!=='seamPrepare') await surface.getByLabel('来源记录',{exact:true}).selectOption('editor-results/capture-original/2');
      else {await surface.getByLabel('遮罩宽度',{exact:true}).fill('63');await surface.getByLabel('接缝偏移',{exact:true}).fill('-20');await surface.getByLabel('羽化',{exact:true}).fill('12');}
      await surface.getByRole('button',{name:label,exact:true}).click();
      await surface.getByRole('status').filter({hasText:'等待处理'}).waitFor();
      const calls=await page.evaluate(()=>window.check.preparationCalls);const last=calls.at(-1);
      assert.equal(last.serviceId,service);assert.equal(last.input.sourceName,'input//desk.png');
      assert.equal(last.input.outputProjection,service==='seamPrepare'?'flat':'equirectangular360');
      if(service==='seamPrepare') assert.deepEqual([last.input.bandWidth,last.input.offset,last.input.blur],[63,-20,12]);
    }
    await page.screenshot({path:resolve(results,'studio-repair-toolbar.png')});
    await surface.getByLabel('图片成果',{exact:true}).selectOption('editor-results/capture-original/0');
    await surface.getByRole('button',{name:'采用成果',exact:true}).click();
    await surface.getByRole('status').filter({hasText:'已采用'}).waitFor();
    assert.equal(await page.evaluate(()=>window.check.published),'editor-results/capture-original/0');
    assert.equal(await page.evaluate(()=>window.check.captures),1);
    await surface.getByRole('radio',{name:'接缝准备',exact:true}).check();
    const band=surface.getByLabel('遮罩宽度',{exact:true});await band.fill('72');await band.focus();
    const input=await band.elementHandle();
    await page.evaluate(()=>window.check.act('capture.collect',{taskId:'capture-original'}));
    assert.equal(await input.evaluate(el=>el===document.activeElement&&el.value==='72'),true);
    await page.evaluate(()=>window.check.close());
    console.log(JSON.stringify({repairUiPassed:true,checks:['three shared service actions','inline radio modes','owned source selectors','parameter forwarding','adopt named result without regeneration','background collection preserves active input']}));
    console.log(JSON.stringify({ capturePassed: true, checks: ['UI shared action', 'pending is not completed', 'close/reopen retains original task', 'collect all three assets', 'no resubmit', 'reopened asset usable'] }));
  }
  if (option('--inheritance')) {
    if (!option('--frame')) throw new Error('Inheritance check requires the production frame');
    const original = await page.evaluate(() => {
      const source = { version: 2, id: 'parent-scene', stickers: window.check.saved().stickers.slice(0, 3) };
      window.check.connectedDocuments = [{ target: { nodeId: 'parent' }, revision: 'parent-original', inputNodeIds: [], document: { stateText: JSON.stringify(source) } }];
      window.check.inputNodeIds = ['parent'];
      window.check.replaceSaved(JSON.stringify({ version: 2, id: 'child-scene', stickers: [] }));
      return JSON.stringify(source);
    });
    await page.evaluate(() => window.check.open());
    await surface.locator('[data-stage-canvas]').waitFor();
    const row = 'upstream:parent-scene:desk';
    await surface.locator(`.pano-studio-tl-head[data-head="${row}"]`).waitFor();
    assert.ok(await surface.getByText('From 上游全景', { exact: true }).count());
    await page.evaluate(row => window.check.act('inspector.field', { target: { kind: 'sticker', id: row }, field: 'yaw_deg', value: 40 }), row);
    await page.evaluate(() => window.check.flush());
    const own = await page.evaluate(() => window.check.saved());
    assert.equal(own.stickers.length, 0);
    assert.equal(own.upstream[0].layer, 'parent-scene');
    assert.equal(own.upstream[0].stickers[0].direction.value.yaw_deg, 40);
    assert.equal(await page.evaluate(() => window.check.connectedDocuments[0].document.stateText), original);
    assert.equal(await page.evaluate(() => window.check.savedAssets.some(asset => asset.name.includes('bgx_connected_'))), false);
    await page.screenshot({ path: resolve(results, 'studio-inherited-override.png') });
    await page.evaluate(() => window.check.close());
    await page.evaluate(() => window.check.open());
    assert.equal(await page.evaluate(async () => (await window.check.read()).upstream[0].stickers[0].direction.value.yaw_deg), 40);
    await page.evaluate(() => window.check.close());
    await page.evaluate(() => { window.check.inputNodeIds = []; window.check.connectedDocuments = []; });
    console.log(JSON.stringify({ inheritancePassed: true, checks: ['connected layer shown', 'override saved only downstream', 'parent bytes preserved', 'reopen keeps override'] }));
  }
  if (option('--preservation')) {
    if (!option('--frame')) throw new Error('Preservation check requires the production frame');
    const raw = await page.evaluate(() => {
      const value = window.check.saved();
      value.stickers = value.stickers.slice(0, 3);
      if (value.stickers[0]) value.stickers[0].futureObjectField = { keep: 'opaque' };
      value.futureSceneField = ['keep', 7];
      return JSON.stringify(value);
    });
    for (const original of [raw, JSON.stringify({ version: 1, shots: [{ yaw_deg: 10 }, { yaw_deg: 20 }], stickers: [] })]) {
      const writes = await page.evaluate(() => window.check.writes);
      await page.evaluate(text => window.check.replaceSaved(text), original);
      await page.evaluate(() => window.check.open());
      const state = await page.evaluate(() => window.check.describe());
      assert.equal(state.readOnly, true);
      assert.equal(state.protection.readOnly, true);
      await assert.rejects(page.evaluate(() => window.check.act('object.command', { command: 'duplicate' })), /read-only/);
      await page.evaluate(() => window.check.flush());
      await page.evaluate(() => window.check.close());
      assert.equal(await page.evaluate(() => window.check.savedText()), original);
      assert.equal(await page.evaluate(() => window.check.writes), writes);
    }
    await page.evaluate(() => window.check.replaceSaved('{"version":99,"future":{"keep":true}}'));
    await assert.rejects(page.evaluate(() => window.check.open()), /不支持此工程版本/);
    assert.equal(await page.evaluate(() => window.check.saved().future.keep), true);
    console.log(JSON.stringify({ preservationPassed: true, checks: ['unknown fields preserve original bytes', 'legacy multi-shot preview does not migrate or save', 'unsupported future version is rejected without mutation'] }));
  }
  assert.deepEqual(problems, []);
  if (['inheritance', 'capture', 'geometry','inputs'].includes(option('--focus'))) {
    console.log(JSON.stringify({ passed: true, scope: `${option('--focus')} only`, screenshots: results }));
  } else {
  console.log(JSON.stringify({ passed: true, checks: ['built editor opens', 'three images at distinct directions', 'UI yaw edit', 'undo/redo', 'save/reopen preserves directions', 'no redundant save', ...(option('--frame') ? ['same-name asset import/reopen', 'save conflict retains edits and allows retry', 'opaque origin isolation'] : []), ...(option('--actions') ? ['shared controller actions', 'stale active state rejected', 'keyframe move/save/reopen', 'exactly 50 undo operations'] : []), ...(option('--paint') ? ['sphere mask stroke', 'clear cancel/confirm/undo', 'saved asset add'] : []), ...(option('--3d') ? ['GLB managed conversion bridge', '3D source and result save/reopen', 'animation radio selection and speed persist', 'Spark points loaded and saved'] : []), 'no browser errors'], screenshots: results }));
  }
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
}
