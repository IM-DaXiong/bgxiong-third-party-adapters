// Package-owned input binding and preview identity. Rendering/math stay upstream.
const object = value => value && typeof value === 'object' && !Array.isArray(value);
const keys = (value, allowed) => object(value) && Object.keys(value).every(key => allowed.includes(key));
const clone = value => structuredClone(value);
export function validateGeometry(value) {
  if (!keys(value, ['version', 'source', 'preview']) || value.version !== 1
    || !keys(value.source, ['algorithm', 'images', 'video', 'options'])
    || !['da3', 'moge', 'mogePanorama'].includes(value.source.algorithm)
    || Object.hasOwn(value.source,'video') === Object.hasOwn(value.source,'images')
    || (!Object.hasOwn(value.source,'video') && (!Array.isArray(value.source.images) || !value.source.images.length || value.source.images.length > 1024))) throw new Error('当前版本无法保全此几何输入');
  const allowed = value.source.algorithm === 'da3' ? ['modelName','weightDtype','resolution','resizeMethod','mode','refViewStrategy','poseMethod']
    : value.source.algorithm === 'moge' ? ['modelName','resolutionLevel','fovXDegrees','batchSize','forceProjection','applyMask','refineSteps']
    : ['modelName','resolutionLevel','splitResolution','mergeResolution','batchSize','refineSteps'];
  if (!keys(value.source.options, allowed)) throw new Error('几何输入含未知参数');
  for (const file of geometrySources(value.source)) {
    if (!keys(file, ['kind','filename','subfolder','storage']) || file.kind !== 'file' || !file.filename
      || /[\\/:]/.test(file.filename) || typeof file.subfolder !== 'string' || /[\\:]/.test(file.subfolder)
      || file.subfolder.split('/').some(part => ['.','..'].includes(part))
      || !['input','output','temp','editor-results'].includes(file.storage)) throw new Error('几何来源必须是工程素材');
  }
  if (value.preview && (!keys(value.preview,['sourceDigest','ui']) || !/^[a-f0-9]{64}$/i.test(value.preview.sourceDigest) || !object(value.preview.ui))) throw new Error('几何预览记录无效');
  return value;
}
export function assetDescriptor(name) {
  const parts=name.split('/');return {filename:parts.pop(),subfolder:parts.slice(1).join('/'),type:parts[0]};
}
export function sourceName(file) {return `${file.storage ?? file.type}/${file.subfolder ?? ''}/${file.filename}`;}
export function geometrySources(source) {return Object.hasOwn(source,'video') ? [source.video] : source.images;}
export function geometryUi(geometry, assets) {
  if (!geometry?.preview) return {};
  validateGeometry(geometry);
  const ui=clone(geometry.preview.ui);
  function resolve(value) {
    if (!object(value) && !Array.isArray(value)) return value;
    if (object(value) && Object.hasOwn(value,'assetName')) {
      if (Object.keys(value).some(key => !['assetName','format'].includes(key)) || !assets.has(value.assetName)) throw new Error('几何预览素材缺失');
      return {...assetDescriptor(value.assetName),...(value.format ? {format:value.format} : {})};
    }
    if (object(value) && ['filename','url','src'].some(key=>Object.hasOwn(value,key))) throw new Error('几何预览必须使用受管素材');
    return Array.isArray(value) ? value.map(resolve) : Object.fromEntries(Object.entries(value).map(([key,item])=>[key,resolve(item)]));
  }
  return resolve(ui);
}
function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  return object(value) ? Object.fromEntries(Object.keys(value).sort().map(key=>[key,canonical(value[key])])) : value;
}
export async function adoptGeometry(result, assets) {
  if (result.status !== 'completed') throw new Error('请先收取几何成果');
  const metadata=JSON.parse(result.data.metadataJson);
  if (!keys(metadata,['version','source','sourceAssets','ui']) || metadata.version!==1 || !Array.isArray(metadata.sourceAssets)) throw new Error('几何成果记录无效');
  const record={version:1,source:clone(metadata.source)};validateGeometry(record);
  const names=geometrySources(record.source).map(sourceName);
  if (metadata.sourceAssets.length!==names.length || metadata.sourceAssets.some((source,index)=>source.name!==names[index] || !/^[a-f0-9]{64}$/i.test(source.sha256) || assets.get(source.name)?.sha256!==source.sha256)) throw new Error('几何来源素材已经改变，请保留原成果并重新选择');
  function bind(value) {
    if (!object(value) && !Array.isArray(value)) return value;
    if (object(value) && Object.hasOwn(value,'assetIndex')) {
      const asset=result.assets[value.assetIndex];
      if (!Number.isSafeInteger(value.assetIndex) || !asset || !assets.has(asset.name) || !['geometryFile','geometryImage','geometryColor'].includes(asset.role)) throw new Error('几何成果附件缺失');
      return {assetName:asset.name,...(value.format ? {format:value.format} : {})};
    }
    return Array.isArray(value)?value.map(bind):Object.fromEntries(Object.entries(value).map(([key,item])=>[key,bind(item)]));
  }
  const digest=await crypto.subtle.digest('SHA-256',new TextEncoder().encode(JSON.stringify(canonical({source:record.source,assets:metadata.sourceAssets}))));
  record.preview={sourceDigest:[...new Uint8Array(digest)].map(b=>b.toString(16).padStart(2,'0')).join(''),ui:bind(metadata.ui)};
  geometryUi(record,assets);return record;
}
