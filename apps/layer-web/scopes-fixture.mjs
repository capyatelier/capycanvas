import assert from 'node:assert/strict';
import {placementSave} from './image-placement-motion.test.mjs';
import {sourceIdentity,authoredIdentity,rasterIdentity} from './package-fixture.test.mjs';

export async function scopesFixture({call,evaluate,settle}) {
  const poll=async condition=>{const end=Date.now()+150000;while(Date.now()<end){if(await evaluate(condition))return;await settle();await new Promise(r=>setTimeout(r,50));}throw Error(`${condition}: ${await evaluate('document.body.innerText.slice(-1500)')}`);};
  const json=expression=>evaluate(`JSON.parse(JSON.stringify(${expression},(_,v)=>typeof v==='bigint'?Number(v):v))`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=async command=>{assert.ok(await evaluate(`layerApp.state().commands.some(c=>c.id===${JSON.stringify(command)})`),`Shared command ${command} exists`);await send({type:'invoke',command});};
  const idle=async()=>{await poll('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()&&layerApp.startupTimes.complete!==null');await evaluate('layerApp.app.wait_for_canvas()');};
  await idle();
  await evaluate(`(async()=>{
    window.scopePickers={open:window.showOpenFilePicker,save:window.showSaveFilePicker};window.placementTest={};
    const c=new OffscreenCanvas(256,256),x=c.getContext('2d',{willReadFrequently:true}),image=x.createImageData(256,256);
    for(let y=0;y<256;y++)for(let i=0;i<256;i++){const p=(y*256+i)*4;image.data.set([16+Math.floor(i*220/255),24+Math.floor(y*y*200/(255*255)),32+Math.floor(i*y*180/(255*255)),255],p);}x.putImageData(image,0,0);
    const blob=await c.convertToBlob({type:'image/png'});window.scopePhoto=new File([blob],'scope-gradient.png',{type:'image/png'});
    window.showOpenFilePicker=async()=>[{name:scopePhoto.name,getFile:async()=>scopePhoto}];
    window.showSaveFilePicker=async o=>({name:o.suggestedName,async createWritable(){return{async write(v){placementTest.saved=new Uint8Array(v instanceof Blob?await v.arrayBuffer():v)},async close(){},async abort(){}}}});
  })()`);
  const openingEpoch=await evaluate('String(layerApp.state().document_file.epoch)');
  await invoke('open_document');await poll(`String(layerApp.state().document_file.epoch)!==${JSON.stringify(openingEpoch)}`);await idle();await invoke('fit_canvas');
  const save=placementSave({evaluate,invoke,idle});
  const checkpoint=async()=>{const m=await save();return{source:sourceIdentity(m),authored:authoredIdentity(m),rasters:rasterIdentity(m)};};
  const source=(await checkpoint()).source;assert.ok(source.length);
  const documentPoint=point=>evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(${point[0]}*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(${point[1]}*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
  const visibleSamples=async()=>{await idle();const points=await Promise.all([[32,32],[96,160],[224,224]].map(documentPoint));const {data}=await call('Page.captureScreenshot',{format:'png'});return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,'+${JSON.stringify(data)};await image.decode();const c=document.createElement('canvas');c.width=image.width;c.height=image.height;const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(image,0,0);return ${JSON.stringify(points)}.map(p=>Array.from(x.getImageData(Math.round(p.x*devicePixelRatio),Math.round(p.y*devicePixelRatio),1,1).data));})()`);};
  const click=async selector=>{const p=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();};
  const reopen=async()=>{const before=await checkpoint(),epoch=await evaluate('String(layerApp.state().document_file.epoch)');await evaluate(`window.showOpenFilePicker=async()=>[{name:'scope-saved.capy',getFile:async()=>new File([placementTest.saved],'scope-saved.capy')}];`);await invoke('open_document');await poll(`String(layerApp.state().document_file.epoch)!==${JSON.stringify(epoch)}`);await idle();await invoke('fit_canvas');assert.deepEqual(await checkpoint(),before);};
  const recover=async()=>{const before=await checkpoint();await evaluate('layerApp.restartGpu()');await idle();assert.deepEqual(await checkpoint(),before,'Renderer recreation preserves source and correction bits');};
  const dispose=()=>evaluate('window.showOpenFilePicker=scopePickers.open;window.showSaveFilePicker=scopePickers.save;delete window.scopePickers;delete window.scopePhoto;');
  return{json,poll,send,invoke,idle,save,checkpoint,documentPoint,visibleSamples,click,reopen,recover,dispose};
}
