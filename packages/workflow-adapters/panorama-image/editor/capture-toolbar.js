// UI and MCP call the same declared actions. Closing never resubmits a task.
export function captureToolbar(editor, lastTask, assets = () => []) {
  const bar = document.createElement('div'); bar.className = 'bgx-capture-toolbar';
  const modes = document.createElement('div'); modes.className = 'bgx-preparation-modes';
  modes.setAttribute('role','radiogroup'); modes.setAttribute('aria-label','处理方式');
  const controls = document.createElement('div'); controls.className='bgx-preparation-controls';
  bar.append(modes,controls); document.body.append(bar); document.body.classList.add('bgx-capture-enabled');
  let mode='capture.frame', busy=false; const fields=new Map();
  const choices=[['capture.frame','取景'],['repair.frameReturn','修补回贴'],['repair.seamPrepare','接缝准备'],['repair.seamReturn','接缝还原']];
  const submit=document.createElement('button'),collect=document.createElement('button');collect.textContent='收取成果';
  const state=document.createElement('span');state.setAttribute('role','status');
  const resultChoice=document.createElement('select');resultChoice.setAttribute('aria-label','图片成果');
  const adopt=document.createElement('button');adopt.textContent='采用成果';
  const wrap=(label,input,key)=>{const el=document.createElement('label');el.textContent=label;input.setAttribute('aria-label',label);el.append(input);controls.append(el);fields.set(key,input);return input;};
  const number=(label,key,value,min,max,step)=>{const input=document.createElement('input');input.type='number';Object.assign(input,{value,min,max,step});wrap(label,input,key);};
  const select=(label,key,image)=>{
    const input=document.createElement('select');input.dataset.media=image?'image':'file';
    wrap(label,input,key);
  };
  function refreshAssets() {
    const previous=resultChoice.value;
    const results=assets().filter(a=>a.name.startsWith('editor-results/') && a.mimeType?.startsWith('image/')).map(a=>[a.name,a.displayName??a.name]);
    const signature=JSON.stringify(results);
    if(resultChoice.dataset.options!==signature){
      resultChoice.replaceChildren(new Option('选择成果',''),...results.map(([name,label])=>new Option(label,name)));
      resultChoice.value=results.some(([name])=>name===previous)?previous:results.at(-1)?.[0]??'';
      resultChoice.dataset.options=signature;
    }
    adopt.disabled=busy||!resultChoice.value;
    for(const input of fields.values()) if(input.tagName==='SELECT') {
      const value=input.value, image=input.dataset.media==='image';
      const choices=assets().filter(a=>image ? a.mimeType?.startsWith('image/') : !a.mimeType?.startsWith('image/')).map(a=>[a.name,a.displayName??a.name.split('/').at(-1)]);
      const signature=JSON.stringify(choices);if(input.dataset.options===signature)continue;
      input.replaceChildren(new Option('选择素材',''),...choices.map(([name,label])=>new Option(label,name)));
      input.value=value;input.dataset.options=signature;
    }
  }
  function render() {
    const prior=Object.fromEntries([...fields].map(([key,input])=>[key,input.value]));fields.clear();controls.replaceChildren();
    if(mode!=='capture.frame') select(mode==='repair.seamPrepare'?'全景素材':'修补素材','sourceName',true);
    if(['repair.frameReturn','repair.seamReturn'].includes(mode)) select('来源记录','provenanceName',false);
    if(['capture.frame','repair.frameReturn'].includes(mode)) {number('百万像素','megapixels',2,.1,16,.1);number('尺寸倍数','multiple',8,8,128,4);}
    if(mode==='repair.seamPrepare') {number('遮罩宽度','bandWidth',64,1,2048,1);number('接缝偏移','offset',0,-2048,2048,1);number('羽化','blur',10,0,256,1);}
    refreshAssets();
    for(const [key,input] of fields) if(prior[key]!==undefined) input.value=prior[key];
    submit.textContent=mode==='capture.frame'?'取景与遮罩':choices.find(c=>c[0]===mode)[1];
    controls.append(submit,collect,state);collect.disabled=busy||!lastTask();
  }
  for(const [value,label] of choices) {
    const el=document.createElement('label'),input=document.createElement('input');input.type='radio';input.name='preparation-mode';input.value=value;input.checked=value===mode;
    input.onchange=()=>{mode=value;render();};el.append(input,label);modes.append(el);
  }
  async function run(action,args) {
    if(busy)return;busy=true;submit.disabled=collect.disabled=adopt.disabled=true;state.textContent='处理中';
    try {const before=await editor.describe();const after=await editor.act({action,arguments:args,expectedStateDigest:before.stateDigest});state.textContent=after.service.status==='pending'?'等待处理':after.service.status==='published'?'已采用':'已收取';}
    catch(error){state.textContent=String(error.message??error);}
    finally{busy=false;submit.disabled=false;collect.disabled=!lastTask();refreshAssets();}
  }
  modes.append(resultChoice,adopt);
  submit.onclick=()=>run(mode,Object.fromEntries([...fields].map(([key,input])=>[key,input.type==='number'?Number(input.value):input.value])));
  collect.onclick=()=>run('capture.collect',{taskId:lastTask()});render();
  adopt.onclick=()=>run('asset.publish',{name:resultChoice.value});resultChoice.onchange=()=>{adopt.disabled=busy||!resultChoice.value;};
  return {addControl(control){modes.append(control);},refresh(result){collect.disabled=busy||!lastTask();state.textContent=result.status==='pending'?'等待处理':'已收取';refreshAssets();},
    remove(){bar.remove();document.body.classList.remove('bgx-capture-enabled');}};
}
