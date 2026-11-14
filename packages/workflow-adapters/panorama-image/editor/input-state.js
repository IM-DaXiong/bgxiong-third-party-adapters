import { assetDescriptor, sourceName, resolveOwnedUi } from './geometry-state.js';

const object=value=>value&&typeof value==='object'&&!Array.isArray(value);
const closed=(value,fields)=>object(value)&&Object.keys(value).every(key=>fields.includes(key));
export const inputRoles=['background','world','sticker'];
export function validateInputs(value) {
  if(!closed(value,['version',...inputRoles,'stickerState'])||value.version!==1)throw new Error('不支持的外部输入版本');
  for(const role of inputRoles){
    if(!Object.hasOwn(value,role))continue;
    const item=value[role],source=item?.source;
    if(!closed(item,['mediaKind','source','preview'])||!['image','video','mesh','splat'].includes(item.mediaKind)
      ||(role==='background'&&!['image','video'].includes(item.mediaKind))||(role==='world'&&!['mesh','splat'].includes(item.mediaKind))
      ||!closed(source,['kind','filename','subfolder','storage'])||source.kind!=='file'||typeof source.filename!=='string'||!source.filename
      ||/[\\/:]/.test(source.filename)||['.','..'].includes(source.filename)||typeof source.subfolder!=='string'||/[\\:]/.test(source.subfolder)
      ||source.subfolder.split('/').some(p=>['.','..'].includes(p))||!['input','output','temp','editor-results'].includes(source.storage)
      ||(Object.hasOwn(item,'preview')&&!object(item.preview)))throw new Error('外部输入必须引用有效工程素材');
  }
  if(Object.hasOwn(value,'stickerState')){
    if(!value.sticker||typeof value.stickerState!=='string'||new TextEncoder().encode(value.stickerState).length>1048576)throw new Error('贴图姿态需要贴图输入及原始 JSON');
    JSON.parse(value.stickerState);
  }
  return value;
}
export function inputKind(asset) {
  if(asset.mimeType?.startsWith('image/'))return 'image';
  if(asset.mimeType?.startsWith('video/'))return 'video';
  const ext=asset.name.split('.').at(-1).toLowerCase();
  if(['ply','splat','spz','ksplat'].includes(ext))return 'splat';
  if(['glb','gltf','obj','stl','fbx','usdz'].includes(ext))return 'mesh';
  return null;
}
export function inputRecord(name,mediaKind,preview) {
  const file=assetDescriptor(name);
  return {mediaKind,source:{kind:'file',filename:file.filename,subfolder:file.subfolder,storage:file.type},...(preview?{preview}: {})};
}
export function inputUi(record,assets) {
  if(!record)return {};
  validateInputs(record);const ui={};
  for(const role of inputRoles){
    const item=record[role];if(!item)continue;
    const name=sourceName(item.source);if(!assets.has(name))throw new Error('外部输入素材缺失');
    const prefix=role==='sticker'?'pano_sticker':'pano';
    if(['image','video'].includes(item.mediaKind)){
      ui[`${prefix}_input_${item.mediaKind==='image'?'images':'videos'}`]=[assetDescriptor(name)];
      if(item.preview)Object.assign(ui,resolveOwnedUi(item.preview,assets));
    }else{
      if(!item.preview)throw new Error('3D 输入需要先完成预览转换');
      const resolved=resolveOwnedUi(item.preview,assets);
      Object.assign(ui,resolved);
    }
  }
  return ui;
}
export function inputLinks(record) {
  if(!record)return [];
  validateInputs(record);
  const links=inputRoles.filter(role=>record[role]).map(role=>({name:role,type:{image:'IMAGE',video:'VIDEO',mesh:'MESH',splat:'SPLAT'}[record[role].mediaKind],node:{id:`bgx-input-${role}`,widgets:[]}}));
  if(record.stickerState)links.push({name:'sticker_state',type:'STRING',node:{id:'bgx-input-pose',widgets:[]}});
  return links;
}
export function backgroundSettings(draft) {
  const parameters=draft?.adapterParameters??{}, color=parameters['/advanced/backgroundColor']??'#00ff00', range=parameters['/advanced/backgroundRange']??'360';
  if(!/^#[0-9a-fA-F]{6}$/.test(color)||!['180','360'].includes(range))throw new Error('背景颜色或范围无效');
  return {color:[1,3,5].map(i=>parseInt(color.slice(i,i+2),16)/255),range};
}
export function upstreamBackground(layers) {
  for(const layer of [...layers].reverse()){
    const ui=layer.ui??{};
    if(!(ui.pano_input_images?.length||ui.pano_input_videos?.length))continue;
    const settings=layer.background??backgroundSettings();
    const out={pano_upstream_coverage:[settings.range],pano_upstream_bg_color:[settings.color],pano_upstream_bg_clip:[{clip:layer.state.background.clip,fps:layer.state.timeline.fps}]};
    for(const key of ['pano_input_images','pano_input_videos','pano_input_video_meta'])if(ui[key])out[key.replace('pano_','pano_upstream_')]=ui[key];
    return out;
  }
  return {};
}
