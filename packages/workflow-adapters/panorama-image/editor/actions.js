import { buildInspectorModel } from '@panorama-upstream/web_src/studio/inspector_model.js';
import { inspectorSelection } from '@panorama-upstream/web_src/studio/session.js';
import * as timeline from '@panorama-upstream/web_src/studio/timeline_actions.js';
import { spanOf } from '@panorama-upstream/web_src/studio/timeline_model.js';
import { yawPitchToDir } from '@panorama-upstream/web_src/pano_camera_math.js';

// Public commands call the same controller and pure action functions used by the Vue UI.
// No arbitrary state patch, expression evaluation, duplicated angle or keyframe mathematics.
const specifications = {
  'selection.set': ['row', 'keys'], 'inspector.field': ['target', 'field', 'value'],
  'inspector.choice': ['target', 'field', 'value'], 'inspector.key': ['target', 'row'],
  'inspector.reset': ['target', 'row'], 'inspector.easing': ['keyframes', 'easing'],
  'inspector.easingReset': ['keyframes'], 'object.command': ['command'], 'object.visibility': ['row'],
  'timeline.length': ['value'], 'timeline.fps': ['value'], 'timeline.seek': ['frame'],
  'timeline.key': ['row'], 'timeline.keys.move': ['delta'], 'timeline.keys.delete': [],
  'timeline.keys.copy': [], 'timeline.keys.paste': [], 'timeline.clip': ['row', 'mode', 'delta'],
  'timeline.jump': ['direction'], 'timeline.loop': ['enabled'], 'timeline.play': [],
  'upstream.revert': ['key'], 'upstream.restoreDeleted': ['key'],
  'history.undo': [], 'history.redo': [], 'view.tab': ['tab'],
  'view.frameSelected': ['row'], 'view.cameraToView': [],
  'paint.tool': ['tool'], 'paint.option': ['kind', 'key', 'value', 'id', 'color', 'index'],
  'paint.stroke': ['points'], 'paint.clear': [], 'paint.confirmClear': ['confirmed'],
  'asset.add': ['name', 'placement'],
  'asset.publish': ['name'],
  'capture.frame': ['megapixels', 'multiple'], 'capture.collect': ['taskId'],
  'repair.frameReturn': ['sourceName','provenanceName','megapixels','multiple'],
  'repair.seamPrepare': ['sourceName','bandWidth','offset','blur'],
  'repair.seamReturn': ['sourceName','provenanceName'],
};
const clone = value => JSON.parse(JSON.stringify(value));
const numeric = value => { if (!Number.isFinite(value)) throw new Error('A finite number is required'); return value; };
const member = (value, values) => { if (!values.includes(value)) throw new Error(`Expected one of ${values.join(', ')}`); return value; };

export function createEditorActions(editor, flush, { assetFile, capture } = {}) {
  const controller = editor.controller;
  if (!controller || !editor.view) throw new Error('Upstream controller binding is missing');
  let clipboard = null;
  let service = null;
  const current = () => {
    const view = editor.view();
    return { state: editor.session.state, selection: editor.session.selection, playhead: view.playhead,
      tab: view.tab, readOnly: editor.session.readOnly, upstream: view.upstream, media: view.media,
      fpsLocked: view.fpsLocked, canUndo: editor.session.canUndo, canRedo: editor.session.canRedo,
      tool: view.tool, paint: view.paint, confirm: view.confirm, service,
      inspector: buildInspectorModel({ ...view, state: editor.session.state,
        selection: inspectorSelection(editor.session.selection), upstream: view.upstreamWorking }) };
  };
  async function describe() {
    const value = clone(current());
    const bytes = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify(value)));
    return { ...value, stateDigest: [...new Uint8Array(bytes)].map(b => b.toString(16).padStart(2, '0')).join(''),
      actions: Object.entries(specifications).map(([id, arguments_]) => ({ id, arguments: arguments_ })) };
  }
  const commit = (result, stopsPlayback = true) => {
    if (!result) return;
    controller.timelineEdit({ kind: 'state', ...(result.edit ?? result), ...(result.selection ? { selection: result.selection } : {}), phase: 'commit', stopsPlayback });
  };
  async function act({ action, arguments: args = {}, expectedStateDigest }) {
    const fields = specifications[action];
    if (!fields || !args || Array.isArray(args) || typeof args !== 'object' || Object.keys(args).some(key => !fields.includes(key))) throw new Error('Unknown editor action or argument');
    if (typeof expectedStateDigest !== 'string' || expectedStateDigest !== (await describe()).stateDigest) throw new Error('EDITOR_STATE_CONFLICT: reread the active editor');
    const ctx = current(), selection = editor.session.selection;
    if (ctx.readOnly && !['selection.set', 'timeline.seek', 'timeline.jump', 'timeline.loop', 'timeline.play', 'view.tab'].includes(action)) throw new Error('Editor is read-only');
    if (action === 'asset.publish') {
      if (!capture || typeof args.name !== 'string' || !args.name) throw new Error('请选择已登记的图片成果');
      await flush(); service = await capture(action, args);
    }
    else if (action.startsWith('capture.') || action.startsWith('repair.')) {
      if (!capture) throw new Error('Capture service is unavailable');
      await flush();
      if (action === 'capture.frame') {
        const megapixels = numeric(args.megapixels), multiple = numeric(args.multiple);
        if (megapixels < 0.1 || megapixels > 16 || multiple < 8 || multiple > 128 || multiple % 4) throw new Error('Invalid capture size');
        if (ctx.state.camera.projection !== 'perspective') throw new Error('请先将相机切换为 Frame 取景');
      } else if (action === 'capture.collect') {
        if (typeof args.taskId !== 'string' || !args.taskId) throw new Error('Original task is required');
      } else {
        if (typeof args.sourceName !== 'string' || !args.sourceName) throw new Error('请选择工程素材');
        if (action !== 'repair.seamPrepare' && (typeof args.provenanceName !== 'string' || !args.provenanceName)) throw new Error('请选择原始取景或接缝记录');
        if (action === 'repair.frameReturn') {
          const mp=numeric(args.megapixels), multiple=numeric(args.multiple);
          if(mp<0.1 || mp>16 || multiple<8 || multiple>128 || multiple%4) throw new Error('Invalid repair size');
        }
        if (action === 'repair.seamPrepare') {
          for (const [key,min,max] of [['bandWidth',1,2048],['offset',-2048,2048],['blur',0,256]]) {
            if(!Number.isInteger(args[key]) || args[key]<min || args[key]>max) throw new Error(`Invalid ${key}`);
          }
        }
      }
      service = await capture(action, args);
    }
    else if (action === 'selection.set') controller.select({ row: args.row ?? null, keys: args.keys ?? [] });
    else if (action.startsWith('inspector.')) {
      const type = action.slice(10) === 'easingReset' ? 'easing-reset' : action.slice(10);
      if (type === 'field') numeric(args.value);
      controller.inspectorEdit({ ...args, type, phase: 'commit' });
    } else if (action === 'object.command') controller.bar(member(args.command, ['lock', 'duplicate', 'front', 'back', 'revert', 'delete']));
    else if (action === 'object.visibility') commit(timeline.toggleVisible(ctx, selection, args.row), false);
    else if (action === 'timeline.length') commit(timeline.lengthEdit(ctx, args.value === null ? null : numeric(args.value)));
    else if (action === 'timeline.fps') { const fps = timeline.enteredFps(ctx, numeric(args.value)); if (fps !== null) controller.timelineEdit({ kind: 'fps', fps }); }
    else if (action === 'timeline.seek') controller.seek(numeric(args.frame));
    else if (action === 'timeline.key') commit(timeline.keyRow(ctx, args.row, ctx.playhead));
    else if (action === 'timeline.keys.move') commit(timeline.moveKeys(ctx, selection, numeric(args.delta), spanOf(ctx)));
    else if (action === 'timeline.keys.delete') commit(timeline.deleteKeys(ctx, selection, ctx.playhead));
    else if (action === 'timeline.keys.copy') clipboard = timeline.copyKeys(ctx, selection);
    else if (action === 'timeline.keys.paste') commit(timeline.pasteKeys(ctx, clipboard, selection, ctx.playhead));
    else if (action === 'timeline.clip') { const drag = timeline.startClipDrag(ctx, args.row); if (drag) commit(timeline.applyClipDrag(ctx, drag, member(args.mode, ['move', 'start', 'end']), numeric(args.delta))); }
    else if (action === 'timeline.jump') { const frame = timeline.jumpTarget(ctx, selection, ctx.playhead, member(args.direction, [-1, 1])); if (frame !== null) controller.seek(frame); }
    else if (action === 'timeline.loop') { if (typeof args.enabled !== 'boolean') throw new Error('enabled must be boolean'); controller.loop(args.enabled); }
    else if (action === 'timeline.play') controller.play();
    else if (action === 'upstream.revert' || action === 'upstream.restoreDeleted') controller.timelineEdit({ kind: action === 'upstream.revert' ? 'revert-upstream-layer' : 'restore-upstream-deleted', key: args.key });
    else if (action.startsWith('history.')) controller.rail(action.slice(8));
    else if (action === 'view.tab') controller.setTab(member(args.tab, ['3d', 'camera']));
    else if (action === 'view.frameSelected') controller.frameSelected(args.row);
    else if (action === 'view.cameraToView') controller.rail('camera');
    else if (action === 'paint.tool') controller.rail(member(args.tool, ['cursor', 'paint', 'mask']));
    else if (action === 'paint.option') {
      member(args.kind, ['brush', 'size', 'swatch', 'custom', 'close-color', 'color', 'recent']);
      if (args.kind === 'size' || args.kind === 'recent') numeric(args.kind === 'size' ? args.value : args.index);
      controller.paint(args);
    } else if (action === 'paint.clear') controller.paint({ kind: 'clear' });
    else if (action === 'paint.confirmClear') {
      if (typeof args.confirmed !== 'boolean' || !ctx.confirm) throw new Error('No pending clear confirmation');
      controller.confirm(args.confirmed);
    } else if (action === 'paint.stroke') {
      if (!['paint', 'mask'].includes(ctx.tool)) throw new Error('Select Paint or Mask first');
      if (!Array.isArray(args.points) || !args.points.length || args.points.length > 8192) throw new Error('A stroke requires 1–8192 direction points');
      const points = args.points.map(point => {
        if (!point || Object.keys(point).some(key => !['yaw', 'pitch'].includes(key))) throw new Error('Stroke points require yaw and pitch');
        const yaw = numeric(point.yaw), pitch = numeric(point.pitch);
        if (yaw < -180 || yaw > 180 || pitch < -90 || pitch > 90) throw new Error('Stroke direction is outside the sphere');
        return yawPitchToDir(yaw, pitch);
      });
      const drag = editor.paint().press(points[0], point => point);
      if (!drag) throw new Error('Painting is unavailable or hidden');
      try { for (const point of points.slice(1)) drag.move(point); drag.end(); }
      catch (error) { drag.cancel(); throw error; }
    } else if (action === 'asset.add') {
      if (!assetFile || typeof args.name !== 'string') throw new Error('A saved named asset is required');
      member(args.placement, ['sky', 'world']);
      const added = await controller.addFiles([await assetFile(args.name)]);
      const placed = controller.place(args.placement) ?? added;
      if (!placed) throw new Error('The upstream editor did not accept this asset');
      await placed.done;
    }
    await flush();
    return describe();
  }
  return { describe, act };
}
