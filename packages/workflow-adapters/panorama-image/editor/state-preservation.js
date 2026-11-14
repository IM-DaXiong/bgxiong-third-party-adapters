// The upstream normalizer intentionally accepts and discards some old/unknown state.
// Persisted projects must never silently lose those fields when an editor opens.
// The normalizer is supplied by the pinned upstream module; this is not a second schema.
import { validateGeometry } from './geometry-state.js';
export function inspectStatePreservation(text, normalize, serialize = JSON.stringify) {
  let raw;
  try { raw = text?.trim() ? JSON.parse(text) : {}; }
  catch { return { readOnly: true, canPreview: false, reason: '工程内容不是有效 JSON，已保留原文。', paths: ['$'] }; }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    return { readOnly: true, canPreview: false, reason: '工程格式无效，已保留原文。', paths: ['$'] };
  }
  if (!Object.keys(raw).length) return { readOnly: false, canPreview: true, reason: null, paths: [] };
  if (Object.hasOwn(raw,'bgxGeometry')) {
    try { validateGeometry(raw.bgxGeometry); }
    catch { return {readOnly:true,canPreview:true,reason:'当前版本不支持此几何输入，已保留原工程。',paths:['$.bgxGeometry']}; }
  }
  if ((raw.version ?? 1) === 1) {
    return { readOnly: true, canPreview: true, reason: '旧版工程仅供预览；当前转换可能丢失图层，已保留原文。', paths: ['$.version'] };
  }
  let normalized;
  try { normalized = normalize(raw); }
  catch { return { readOnly: true, canPreview: false, reason: '当前编辑器不支持此工程版本，已保留原文。', paths: ['$.version'] }; }
  const serialized = JSON.parse(serialize(normalized));
  const paths = [];
  function compare(original, current, stored, path) {
    if (paths.length >= 32) return;
    if (original === null || typeof original !== 'object') {
      // Upstream serializes some defaults as null (notably the default camera).
      // Accept either upstream representation without inventing those defaults here.
      if (!Object.is(original, current) && !Object.is(original, stored)) paths.push(path);
      return;
    }
    if (!current || typeof current !== 'object' || Array.isArray(original) !== Array.isArray(current)) { paths.push(path); return; }
    if (Array.isArray(original) && original.length !== current.length) { paths.push(path); return; }
    for (const key of Object.keys(original)) {
      const at = Array.isArray(original) ? `${path}[${key}]` : `${path}.${key}`;
      if (!Object.hasOwn(current, key)) paths.push(at);
      else compare(original[key], current[key], stored?.[key], at);
      if (paths.length >= 32) break;
    }
  }
  compare(raw, normalized, serialized, '$');
  return { readOnly: paths.length > 0, canPreview: true,
    reason: paths.length ? '此工程含当前编辑器无法原样保存的内容，已切换只读预览并保留原文。' : null, paths };
}
