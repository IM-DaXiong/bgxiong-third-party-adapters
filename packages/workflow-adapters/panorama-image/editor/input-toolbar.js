import { inputKind } from './input-state.js';

export function inputToolbar(editor,assets,importFile) {
  const panel=document.createElement('section');panel.className='bgx-geometry-panel';panel.hidden=true;panel.setAttribute('aria-label','外部输入');document.body.append(panel);
  const toggle=document.createElement('button');toggle.textContent='外部输入';toggle.setAttribute('aria-expanded','false');
  const status=document.createElement('span');status.setAttribute('role','status');let role='background',busy=false;
  toggle.onclick=()=>{panel.hidden=!panel.hidden;toggle.setAttribute('aria-expanded',String(!panel.hidden));if(!panel.hidden)render();};
  const row=()=>{const el=document.createElement('div');el.className='bgx-geometry-row';panel.append(el);return el;};
  const button=(parent,text,action)=>{const el=document.createElement('button');el.textContent=text;el.disabled=busy;el.onclick=action;parent.append(el);return el;};
  async function run(action,args){
    if(busy)return;busy=true;status.textContent='处理中';render();
    try{const before=await editor.describe();await editor.act({action,arguments:args,expectedStateDigest:before.stateDigest});status.textContent='已保存';}
    catch(error){status.textContent=error.message??String(error);}finally{busy=false;render();}
  }
  async function choose(pose=false){
    const picker=document.createElement('input');picker.type='file';picker.accept=pose?'.json':role==='world'?'.glb,.gltf,.obj,.fbx,.stl,.usdz,.ply,.splat,.spz,.ksplat':role==='background'?'image/*,video/*':'image/*,video/*,.glb,.gltf,.obj,.fbx,.stl,.usdz,.ply,.splat,.spz,.ksplat';
    picker.onchange=async()=>{const file=picker.files?.[0];if(!file)return;busy=true;status.textContent='导入中';render();
      try{const name=await importFile(file);busy=false;await run(pose?'input.pose':'input.set',pose?{name}:{role,name});}
      catch(error){status.textContent=error.message??String(error);busy=false;render();}};picker.click();
  }
  function render(){
    panel.replaceChildren();const choices=row();choices.setAttribute('role','radiogroup');choices.setAttribute('aria-label','输入用途');
    for(const [id,title] of [['background','背景'],['world','空间'],['sticker','贴图']]){
      const label=document.createElement('label'),radio=document.createElement('input');radio.type='radio';radio.name='bgx-input-role';radio.checked=role===id;radio.disabled=busy;radio.onchange=()=>{role=id;render();};label.append(radio,title);choices.append(label);
    }
    const line=row(),select=document.createElement('select');select.setAttribute('aria-label','输入素材');select.disabled=busy;
    select.append(new Option('选择素材',''));
    const allowed=role==='world'?['mesh','splat']:role==='background'?['image','video']:['image','video','mesh','splat'];
    for(const asset of assets().filter(asset=>allowed.includes(inputKind(asset))))select.append(new Option(asset.displayName??asset.name.split('/').at(-1),asset.name));
    const source=editor.read().bgxInputs?.[role]?.source;if(source)select.value=`${source.storage}/${source.subfolder}/${source.filename}`;
    select.onchange=()=>{if(select.value)run('input.set',{role,name:select.value});};line.append(select);
    button(line,'导入',()=>choose());button(line,'移除',()=>run('input.remove',{role}));
    if(role==='sticker')button(line,'导入姿态',()=>choose(true));
    line.append(status);

  }
  return {toggle,remove(){panel.remove();toggle.remove();}};
}
