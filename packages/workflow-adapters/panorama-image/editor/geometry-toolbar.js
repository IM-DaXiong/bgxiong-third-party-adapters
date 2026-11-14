import { assetDescriptor } from './geometry-state.js';

// An optional compact panel: source selection and the parameters that matter for
// the selected algorithm share rows. The main preview keeps the rest of the window.
export function geometryToolbar(editor,assets,lastTask) {
  const panel=document.createElement('section');panel.className='bgx-geometry-panel';panel.hidden=true;
  panel.setAttribute('aria-label','几何输入');document.body.append(panel);
  const toggle=document.createElement('button');toggle.textContent='几何输入';toggle.setAttribute('aria-expanded','false');
  toggle.onclick=()=>{panel.hidden=!panel.hidden;toggle.setAttribute('aria-expanded',String(!panel.hidden));if(!panel.hidden)render();};
  const original=editor.read().bgxGeometry?.source;
  let algorithm=original?.algorithm??'da3',mode=original?.options.mode??'mono',busy=false,models=null;
  const selected=new Set(), saved={da3:{},moge:{},mogePanorama:{}};
  if(original){saved[algorithm]={...original.options};for(const file of original.images)selected.add(`${file.storage}/${file.subfolder}/${file.filename}`);}
  const controls=new Map();let controlsAlgorithm=algorithm;
  const status=document.createElement('span');status.setAttribute('role','status');
  const row=()=>{const el=document.createElement('div');el.className='bgx-geometry-row';panel.append(el);return el;};
  const label=(parent,title,input)=>{const el=document.createElement('label');el.append(title,input);parent.append(el);return input;};
  const radios=(parent,title,key,choices,value,changed)=>{
    const group=document.createElement('div');group.setAttribute('role','radiogroup');group.setAttribute('aria-label',title);group.className='bgx-geometry-choices';
    for(const [id,text] of choices){const input=document.createElement('input');input.type='radio';input.name=`geometry-${key}`;input.value=id;input.checked=id===value;input.onchange=()=>changed(id);label(group,text,input);}
    parent.append(group);
  };
  const remember=()=>{for(const [key,input] of controls)saved[controlsAlgorithm][key]=input.type==='number'?Number(input.value):input.type==='checkbox'?input.checked:input.value;};
  const input=(parent,key,title,value,min,max,step=1)=>{const el=document.createElement('input');el.type='number';Object.assign(el,{min,max,step,value:saved[algorithm][key]??value});el.setAttribute('aria-label',title);controls.set(key,el);label(parent,title,el);};
  const check=(parent,key,title,value)=>{const el=document.createElement('input');el.type='checkbox';el.checked=saved[algorithm][key]??value;controls.set(key,el);label(parent,title,el);};
  const choice=(parent,key,title,choices,value)=>radios(parent,title,key,choices,saved[algorithm][key]??value,id=>{saved[algorithm][key]=id;});
  const button=(parent,title,action)=>{const el=document.createElement('button');el.textContent=title;el.disabled=busy;el.onclick=action;parent.append(el);return el;};
  async function run(action,args) {
    if(busy)return;remember();busy=true;status.textContent='处理中';
    for(const el of panel.querySelectorAll('button,input,select'))el.disabled=true;
    try {
      const before=await editor.describe();const after=await editor.act({action,arguments:args,expectedStateDigest:before.stateDigest});
      if(action==='geometry.models')models=JSON.parse(after.service.data.metadataJson);
      status.textContent=after.service.status==='pending'?'等待处理':action==='geometry.adopt'?'已采用':action==='geometry.remove'?'已移除':'已收取';
    }catch(error){status.textContent=String(error.message??error);}
    finally{busy=false;render();}
  }
  function source() {
    remember();const options={...saved[algorithm]};
    if(algorithm==='da3'){
      Object.assign(options,{weightDtype:options.weightDtype??'default',resizeMethod:options.resizeMethod??'upper_bound_resize',mode});
      if(mode==='multiview'){options.refViewStrategy??='saddle_balanced';options.poseMethod??='cam_dec';}
      else{delete options.refViewStrategy;delete options.poseMethod;}
    }
    return {algorithm,images:[...selected].map(name=>{const d=assetDescriptor(name);return {kind:'file',filename:d.filename,subfolder:d.subfolder,storage:d.type};}),options};
  }
  function render() {
    remember();controls.clear();controlsAlgorithm=algorithm;panel.replaceChildren();
    radios(row(),'几何算法','algorithm',[['da3','DA3'],['moge','MoGe'],['mogePanorama','MoGe 全景']],algorithm,id=>{remember();algorithm=id;render();});
    const modelsRow=row();button(modelsRow,'读取模型',()=>run('geometry.models',{}));
    if(!models&&saved[algorithm].modelName){const current=document.createElement('span');current.textContent=saved[algorithm].modelName;modelsRow.append(current);}
    if(models){
      const choices=models[algorithm==='da3'?'da3':'moge']??[];
      if(!choices.length){const empty=document.createElement('span');empty.textContent='资源机未安装可选几何模型';modelsRow.append(empty);}
      else if(choices.length<=4)radios(modelsRow,'模型','modelName',choices.map(name=>[name,name]),saved[algorithm].modelName??'',id=>{saved[algorithm].modelName=id;});
      else{const select=document.createElement('select');select.setAttribute('aria-label','几何模型');select.append(new Option('选择模型',''),...choices.map(name=>new Option(name,name)));select.value=saved[algorithm].modelName??'';controls.set('modelName',select);modelsRow.append(select);}
    }
    const sources=row();sources.classList.add('bgx-geometry-sources');sources.setAttribute('role','group');sources.setAttribute('aria-label','来源图片');
    for(const asset of assets().filter(a=>a.mimeType?.startsWith('image/'))){const el=document.createElement('input');el.type='checkbox';el.checked=selected.has(asset.name);el.onchange=()=>{if(el.checked)selected.add(asset.name);else selected.delete(asset.name);};label(sources,asset.displayName??asset.name.split('/').at(-1),el);}
    const parameters=row();
    if(algorithm==='da3'){
      radios(parameters,'深度模式','mode',[['mono','单目序列'],['multiview','同场景多视角']],mode,id=>{remember();mode=id;render();});
      input(parameters,'resolution','分辨率',504,140,2520,14);
      const precision=row();
      choice(precision,'weightDtype','精度',[['default','默认'],['fp16','FP16'],['bf16','BF16'],['fp32','FP32']],'default');
      choice(precision,'resizeMethod','推理缩放',[['upper_bound_resize','长边限制'],['lower_bound_resize','短边限制']],'upper_bound_resize');
      if(mode==='multiview'){
        const view=row();choice(view,'refViewStrategy','参考视角',[['saddle_balanced','均衡'],['saddle_sim_range','差异最大'],['first','第一张'],['middle','中间张']],'saddle_balanced');
        choice(view,'poseMethod','相机估计',[['cam_dec','模型估计'],['ray_pose','几何估计']],'cam_dec');
      }
    }else{
      input(parameters,'resolutionLevel','细节',9,0,9);input(parameters,'batchSize','批量',4,1,algorithm==='moge'?64:12);input(parameters,'refineSteps','细化',3,0,8);
      const extra=row();
      if(algorithm==='moge'){input(extra,'fovXDegrees','源视场角',0,0,170,.1);check(extra,'forceProjection','投影约束',true);check(extra,'applyMask','应用遮罩',true);}
      else{input(extra,'splitResolution','分片分辨率',512,256,1024);input(extra,'mergeResolution','合并分辨率',1920,256,8192);}
    }
    const actions=row();button(actions,'准备几何',()=>run('geometry.prepare',{source:source()}));
    button(actions,'收取成果',()=>run('capture.collect',{taskId:lastTask()})).disabled=busy||!lastTask();
    button(actions,'采用几何',()=>run('geometry.adopt',{taskId:lastTask()})).disabled=busy||!lastTask();
    button(actions,'移除几何',()=>run('geometry.remove',{}));actions.append(status);
  }
  return {toggle,remove(){panel.remove();toggle.remove();}};
}
