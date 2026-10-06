import {packageManifest,packageObjects,readPackage,packageResourceIdentity} from './package-fixture.test.mjs';
import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";

export async function checkRaster({call,evaluate,settle,canvasPixels}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(${condition})resolve(true);else if(performance.now()-start>30000)reject(Error(${JSON.stringify(condition)}+': '+document.querySelector('#status').textContent));else setTimeout(check,20);}check();})`).catch(error=>{throw new Error(`Raster wait failed: ${condition}`,{cause:error});});
  await wait('layerApp.startupTimes.complete!==null');
  console.log('Raster startup',await evaluate('JSON.parse(JSON.stringify({startup:layerApp.startupTimes,stats:layerApp.app.renderer_stats(),camera:layerApp.app.camera(),file:layerApp.state().document_file,status:document.querySelector("#status").textContent},(key,value)=>typeof value==="bigint"?Number(value):value))'));
  console.log('Presented pixels',await canvasPixels());
  await mkdir('artifacts/color-m1',{recursive:true});
  const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile('artifacts/color-m1/web-raster.png',Buffer.from(shot.data,'base64'));
  await evaluate(`window.rasterFiles=new Map();window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){let bytes;return{async write(value){bytes=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},async close(){rasterFiles.set(options.suggestedName,bytes)},async abort(){}}}});`);
  const invoke=async command=>{await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);await settle();if(command==='export_document'){
    await wait('!!document.querySelector("dialog[open] select[aria-label=Format]")');
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Choose File…').click()`);
  }};
  await invoke('fit_canvas');
  await invoke('export_document');await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy');
  console.log('Initial export',await evaluate('Array.from(rasterFiles.entries(),([name,bytes])=>({name,size:bytes.length,header:Array.from(bytes.slice(0,16))}))'));
  assert.ok(await evaluate('[...rasterFiles.values()].some(bytes=>bytes[0]===137)'));
  await evaluate(`window.rasterBlank=[...rasterFiles.values()][0].slice();`);
  const point=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()');
  for(const [type,dx,buttons] of [['mousePressed',0,1],['mouseMoved',75,1],['mouseReleased',75,0]]) {
    await call('Input.dispatchMouseEvent',{type,x:point.x+dx,y:point.y,button:'left',buttons,clickCount:1,pointerType:'pen',force:buttons?.65:0});await settle();
  }
  await wait('layerApp.state().document_file.modified');
  await invoke('save_document_as');await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy && !layerApp.state().document_file.modified');
  assert.deepEqual(await evaluate('Array.from([...rasterFiles].find(([name])=>name.endsWith(".capy"))[1].slice(0,4))'),[80,75,3,4]);
  console.log('Captured raster archive',await evaluate('Array.from(rasterFiles.entries(),([name,bytes])=>({name,size:bytes.length}))'));
  await evaluate(`(async()=>{window.rasterOriginal=[...rasterFiles].find(([name])=>name.endsWith('.capy'))[1].slice();
    window.rasterManifest=${packageManifest.toString()};
    window.rasterOriginalIndex=await rasterManifest(rasterOriginal);
    window.showOpenFilePicker=async()=>[{name:'restored.capy',async getFile(){return new File([rasterOriginal],'restored.capy')}}];})()`);
  const originalPackage = await readPackage(evaluate, 'rasterOriginal');
  assert.ok(packageResourceIdentity(originalPackage).length>0);
  const representation=packageObjects(originalPackage,'capy.output/2').find(output=>output.id===originalPackage.default_output.ref).data.representation;
  assert.equal(representation.member,'preview.png');assert.equal(representation.color,'srgb');
  assert.ok(representation.size.every(value=>value>0&&value<=1024));
  await invoke('export_document');await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy');
  await evaluate(`window.rasterPaintPng=[...rasterFiles].find(([name])=>name.endsWith('.png'))[1].slice();
    window.rasterHash=async bytes=>Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes))).join(',');`);
  assert.notEqual(await evaluate('rasterHash(rasterBlank)'),await evaluate('rasterHash(rasterPaintPng)'),'Committed contact changes exported pixels');
  await invoke('undo');await invoke('redo');
  await invoke('open_document');await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy && layerApp.state().document_file.location?.name==="restored.capy"');
  await wait('layerApp.app.brush_ready()');
  await invoke('save_document_as');await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy');
  assert.deepEqual(packageResourceIdentity(await readPackage(evaluate, "rasterFiles.get('restored.capy')")),packageResourceIdentity(originalPackage));
  await invoke('export_document');await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy');
  assert.equal(await evaluate(`rasterHash(rasterFiles.get('restored.png'))`),await evaluate('rasterHash(rasterPaintPng)'),'Restoring exact tiles produces identical full-canvas PNG pixels');
  await evaluate(`window.rasterGood=rasterOriginal.slice();rasterOriginal[rasterOriginal.length-1]^=1;`);
  const epoch=await evaluate('Number(layerApp.state().document_file.epoch)');
  await evaluate(`window.rasterPackageView=layerApp.app.package_view.bind(layerApp.app);window.rasterFailedPackage=null;layerApp.app.package_view=candidate=>{const model=rasterPackageView(candidate);if(model)rasterFailedPackage=model;return model};`);
  try {
    await invoke('open_document');await wait('rasterFailedPackage!==null&&!!document.querySelector("dialog[open].document-dialog")');
    const failed=await evaluate('rasterFailedPackage');
    assert.equal(failed.disposition,'failed');
    assert.deepEqual(failed.capabilities,{view:false,copy_original:true,edit:false,save:false,export:false});
    assert.ok(failed.reason.length>0);
    assert.equal(await evaluate('Number(layerApp.state().document_file.epoch)'),epoch,'Corrupt package inspection keeps the live document');
    await evaluate(`(()=>{const close=[...document.querySelectorAll('dialog[open].document-dialog button')].find(button=>button.textContent===rasterFailedPackage.close);if(!close)throw Error('Missing failed package Close action');close.click()})()`);
    await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy');
    assert.equal(await evaluate('Number(layerApp.state().document_file.epoch)'),epoch,'Corrupt archive cannot replace the document');
  } finally {await evaluate('layerApp.app.package_view=rasterPackageView');}
  await evaluate('rasterOriginal=rasterGood');
  await evaluate(`(async()=>{
    const suspend=layerApp.app.suspend_gpu.bind(layerApp.app);
    layerApp.app.suspend_gpu=()=>{
      const change=suspend();
      window.suspendedNavigator=layerApp.app.reflow_navigators();
      return change;
    };
    try{await layerApp.restartGpu();}finally{layerApp.app.suspend_gpu=suspend;}
  })()`);
  assert.equal(await evaluate('window.suspendedNavigator'),false,'A queued Navigator reflow defers while the GPU is suspended');
  await wait('layerApp.app.brush_ready() && layerApp.startupTimes.complete!==null');
  await invoke('export_document');await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy');
  assert.equal(await evaluate(`rasterHash(rasterFiles.get('restored.png'))`),await evaluate('rasterHash(rasterPaintPng)'),'GPU replacement retains committed pixels');
  console.log('Exact raster save/reopen, undo/redo, corrupt-file retention and GPU replacement passed');

  const expected=await evaluate('rasterHash(rasterPaintPng)');
  const prior=await evaluate("JSON.parse(JSON.stringify(layerApp.state().document_file,(_,value)=>typeof value==='bigint'?Number(value):value))");
  await evaluate('layerApp.documents.autosave()');
  await call('Page.reload',{ignoreCache:true});
  await new Promise(resolve=>setTimeout(resolve,1000));
  await wait('window.layerApp?.app.brush_ready()');
  await evaluate('layerApp.documents.startRecovery()');
  assert.equal(await evaluate('layerApp.state().document_file.location?.name??null'),prior.location?.name??null,'Restart retains the manual-save destination');
  await evaluate(`window.rasterFiles=new Map();window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){let bytes;return{async write(value){bytes=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},async close(){rasterFiles.set(options.suggestedName,bytes)},async abort(){}}}});`);
  await invoke('export_document');await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy');
  const recovered=await evaluate(`(async()=>Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',[...rasterFiles.values()][0]))).join(','))()`);
  assert.equal(recovered,expected,'An abandoned tab recovers exactly the same pixels');
  assert.equal(await evaluate('layerApp.state().document_file.modified'),prior.location?true:prior.modified,'Export keeps the restored checkpoint protected when no persistent original handle is available');
  console.log('Worker IndexedDB restart survives reload with its manual-save checkpoint');

}
