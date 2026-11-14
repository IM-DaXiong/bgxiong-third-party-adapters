import test from 'node:test';
import assert from 'node:assert/strict';
import {inputRecord,inputUi,inputLinks,validateInputs,upstreamBackground,backgroundSettings} from './input-state.js';

test('owned inputs project background, video and animated world without remote URLs',()=>{
 const assets=new Map(['input//room.png','input//walk.mp4','input/3d/room.glb','editor-results/a/0','editor-results/a/1'].map(name=>[name,true]));
 const record={version:1,
  background:inputRecord('input//room.png','image'),
  sticker:inputRecord('input//walk.mp4','video',{pano_sticker_input_video_meta:[{kind:'VIDEO',fps:29.97,frames:73,duration:73/29.97}]}),
  world:inputRecord('input/3d/room.glb','mesh',{pano_scene_files:[{assetName:'editor-results/a/0'}],pano_scene_meta:[{kind:'mesh',animation:{source:{assetName:'editor-results/a/1'},clips:[{name:'walk',duration:2}]}}]})};
 const ui=inputUi(record,assets);assert.equal(ui.pano_scene_meta[0].animation.source.filename,'1');
 assert.equal(ui.pano_sticker_input_video_meta[0].fps,29.97);assert.equal(ui.pano_input_images[0].filename,'room.png');
 assert.deepEqual(inputLinks(record).map(x=>x.type),['IMAGE','MESH','VIDEO']);
 const inherited=upstreamBackground([{state:{bgxInputs:record,background:{clip:{start:5}},timeline:{fps:24}},ui}]);
 assert.deepEqual(inherited.pano_upstream_coverage,['360']);assert.equal(inherited.pano_upstream_bg_clip[0].clip.start,5);
 const settings=backgroundSettings({adapterParameters:{'/advanced/backgroundColor':'#1234ab','/advanced/backgroundRange':'180'}});
 const parent={state:{background:{clip:{start:5}},timeline:{fps:24}},ui,background:settings};
 const child={state:{background:{clip:{start:0}},timeline:{fps:30}},ui:{},background:backgroundSettings()};
 assert.deepEqual(upstreamBackground([parent,child]).pano_upstream_coverage,['180']);
 assert.deepEqual(upstreamBackground([parent,child]).pano_upstream_bg_color,[[18/255,52/255,171/255]]);
 assert.throws(()=>backgroundSettings({adapterParameters:{'/advanced/backgroundRange':'90'}}));
 assert.throws(()=>inputUi(record,new Map()),/缺失/);
 const invalid=structuredClone(record);invalid.world.preview.pano_scene_files=[{url:'https://example.invalid/file'}];assert.throws(()=>inputUi(invalid,assets),/受管素材/);
});
test('unknown fields, malformed pose and unowned paths cannot silently round-trip',()=>{
 for(const value of [{version:2},{version:1,future:true},{version:1,stickerState:'{}'},
  {version:1,backgroundRange:'90'},{version:1,backgroundColor:'red'},
  {version:1,world:inputRecord('input//a.png','image')},
  {version:1,background:inputRecord('input/../a.png','image')}])assert.throws(()=>validateInputs(value));
 const value={version:1,sticker:inputRecord('input//a.png','image'),stickerState:'{ "version": 2 }'};
 assert.equal(validateInputs(value).stickerState,'{ "version": 2 }');
 assert.deepEqual(inputLinks(value).map(x=>x.name),['sticker','sticker_state']);
});
