import assert from 'node:assert/strict';
import {mkdtemp,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {measurePlacedPhotos} from './image-placement-motion.test.mjs';

// Real browser file-input/drop transport, shared placement and native archives.
// LAYER_PHOTO_FILES optionally selects camera originals (JSON array of paths).
export async function checkImagePlacement({call,evaluate,settle}) {
  const root=await mkdtemp(join(tmpdir(),'capy-image-placement-'));
  const wait=async condition=>{
    const start=Date.now();while(!await evaluate(`!!(${condition})`)){
      if(Date.now()-start>180000)throw Error(condition+': '+await evaluate('document.body.innerText.slice(-1500)'));
      await new Promise(resolve=>setTimeout(resolve,100));
    }
  };
  const invoke=async command=>{
    await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);
    const reply=await call('Runtime.evaluate',{expression:`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`,userGesture:true});
    assert.equal(reply.exceptionDetails,undefined);await settle();
  };
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const idle=()=>wait('!layerApp.state().document_file.busy');
  const placed=()=>wait('layerApp.state().commands.find(c=>c.id==="placement_original_size").enabled');
  const click=async selector=>{
    await wait(`document.querySelector(${JSON.stringify(selector)}) && !document.querySelector(${JSON.stringify(selector)}).disabled`);
    const p=await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
    await settle();
  };
  const choose=async files=>{
    await wait('document.querySelector("input[type=file]")');
    const {root}=await call('DOM.getDocument');
    const {nodeId}=await call('DOM.querySelector',{nodeId:root.nodeId,selector:'input[type=file]'});
    await evaluate(`document.querySelector('input[type=file]').addEventListener('change',e=>{placementTest.inputFiles=[...e.target.files]},{once:true})`);
    await call('DOM.setFileInputFiles',{nodeId,files});
  };
  const importFiles=async files=>{await invoke('import_image');await choose(files);await idle();await placed();};
  const drop=async(p,files)=>{
    await wait('layerApp.state().commands.find(c=>c.id==="import_image").enabled');
    for(const type of ['dragEnter','dragOver','drop'])await call('Input.dispatchDragEvent',{type,...p,data:{items:[],files,dragOperationsMask:1}});
  };
  const save=async()=>{
    await invoke('save_document_as');await idle();
    return evaluate(`(()=>{const b=placementTest.saved;return JSON.parse(new TextDecoder().decode(b.slice(52,52+Number(new DataView(b.buffer,b.byteOffset).getBigUint64(12,true)))));})()`);
  };
  const sourceIdentity=m=>m.tiled_sources.images.map(image=>({...image,tiles:image.tiles.map(t=>{const {offset,...blob}=m.blobs[t.blob];return {...t,blob};})}));
  let files;
  try {
    await call('Page.setInterceptFileChooserDialog',{enabled:true});
    await evaluate(`window.placementTest={open:window.showOpenFilePicker,save:window.showSaveFilePicker};window.showOpenFilePicker=undefined;
      window.showSaveFilePicker=async o=>({name:o.suggestedName,async createWritable(){return{async write(b){placementTest.saved=new Uint8Array(b instanceof Blob?await b.arrayBuffer():b)},async close(){},async abort(){}}}});
      layerApp.dispatch({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}});`);
    if(process.env.LAYER_PHOTO_FILES)files=JSON.parse(process.env.LAYER_PHOTO_FILES);
    else {
      files=[];
      for(const [i,w,h] of [[0,3000,2400],[1,800,600]]){
        const bytes=await evaluate(`(async()=>{const c=new OffscreenCanvas(${w},${h}),x=c.getContext('2d');const g=x.createLinearGradient(0,0,c.width,c.height);g.addColorStop(0,'red');g.addColorStop(1,'blue');x.fillStyle=g;x.fillRect(0,0,c.width,c.height);return Array.from(new Uint8Array(await(await c.convertToBlob()).arrayBuffer()))})()`);
        const path=join(root,`photo-${i}.png`);await writeFile(path,new Uint8Array(bytes));files.push(path);
      }
    }
    const bad=join(root,'malformed.png');await writeFile(bad,'not an image');
    await invoke('new_document');
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')?.click()`);
    await wait('document.querySelector("dialog[open] input[type=number]")');
    await evaluate(`{const fields=document.querySelectorAll('dialog[open] input[type=number]');fields[0].value=2000;fields[1].value=1500;[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create').click();}`);
    await idle();await wait('layerApp.app.brush_ready()');
    const base=await state(),baseCount=base.layers.length;
    assert.deepEqual(await evaluate('layerApp.app.photo_formats().map(f=>f.name)'),['TIFF','PNG','WebP','BMP','JPEG','GIF']);
    await importFiles(files);
    assert.equal((await state()).layers.length,baseCount+files.length);
    await click('.image-placement-controls [data-command=cancel_transform]');
    assert.equal((await state()).layers.length,baseCount);
    assert.equal((await state()).document_file.modified,base.document_file.modified);
    await importFiles(files);
    const labels=(await state()).layers.slice(0,files.length).map(l=>l.label);
    await click('.image-placement-controls [data-command=apply_transform]');
    const fitted=await save(),sources=sourceIdentity(fitted);
    assert.equal(sources.length,files.length);
    for(let i=0;i<files.length;i++){
      const [w,h]=sources[i].extent,scale=Math.min(1,2000/w,1500/h),pose=fitted.document.layers[i].properties.placement;
      assert.ok(Math.abs(pose[0]-scale)<1e-6);assert.ok(Math.abs(pose[3]-scale)<1e-6);
      assert.ok(Math.abs(pose[4]-(2000-w*scale)/2)<.01);assert.ok(Math.abs(pose[5]-(1500-h*scale)/2)<.01);
    }
    await invoke('undo');assert.equal((await state()).layers.length,baseCount);
    await invoke('redo');assert.deepEqual((await state()).layers.slice(0,files.length).map(l=>l.label),labels);
    await evaluate(`window.showOpenFilePicker=async()=>[{async getFile(){return new File([placementTest.saved],'placed.capy')}}]`);
    await invoke('open_document');await idle();await wait('layerApp.app.brush_ready()');
    await evaluate('window.showOpenFilePicker=undefined');
    assert.deepEqual(sourceIdentity(await save()),sources);
    await evaluate('layerApp.restartGpu()');await wait('layerApp.app.brush_ready()');
    assert.deepEqual(sourceIdentity(await save()),sources,'GPU replacement retains placed source samples');
    await invoke('scale_rotate');await placed();
    await click('.image-placement-controls [data-command=placement_original_size]');
    await click('.image-placement-controls [data-command=apply_transform]');
    const native=await save();assert.equal(native.document.layers[0].properties.placement[0],1);assert.deepEqual(sourceIdentity(native),sources);
    // A malformed second file must discard all prepared sources.
    const before=await state();await invoke('import_image');await choose([files[0],bad]);await idle();
    assert.equal((await state()).layers.length,before.layers.length);assert.ok((await state()).host_error);
    // Selection changes while the file input is open must never retarget it.
    await invoke('import_image');await wait('document.querySelector("input[type=file]")');
    const last=before.layers.at(-1).id;
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${last},mask:false}})`);
    await choose([files[0]]);await idle();assert.match((await state()).host_error,/changed/);
    assert.equal((await state()).layers.length,before.layers.length);
    // Browser external canvas file drops use the coordinates captured at drop.
    const p=await evaluate(`(()=>{const r=layerApp.canvas.getBoundingClientRect(),c=layerApp.app.camera(),a=c.work_area;return{x:r.x+(a[0]+a[2]*.6)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]*.6)*r.height/c.viewport[1]}})()`);
    await drop(p,[files[0]]);
    await idle();await placed();assert.equal((await state()).layers.length,before.layers.length+1);
    await click('.image-placement-controls [data-command=cancel_transform]');
    assert.equal((await state()).layers.length,before.layers.length);
    // Placement controls remain reachable in a narrow viewport with panels hidden.
    await importFiles([files[0]]);await invoke('zen_mode');
    await call('Emulation.setDeviceMetricsOverride',{width:360,height:640,deviceScaleFactor:1,mobile:false});await settle();
    assert.equal(await evaluate(`(()=>{const r=document.querySelector('.image-placement-controls').getBoundingClientRect();return r.x>=0&&r.right<=innerWidth&&r.y>=0&&r.bottom<=innerHeight})()`),true);
    await click('.image-placement-controls [data-command=cancel_transform]');
    await call('Emulation.clearDeviceMetricsOverride');await invoke('zen_mode');
    await call('Browser.grantPermissions',{origin:await evaluate('location.origin'),permissions:['clipboardReadWrite','clipboardSanitizedWrite']},null);
    const copied=await call('Runtime.evaluate',{expression:`navigator.clipboard.write([new ClipboardItem({['web '+placementTest.inputFiles[0].type]:placementTest.inputFiles[0]})])`,userGesture:true,awaitPromise:true});
    assert.equal(copied.exceptionDetails,undefined);
    await invoke('paste_image');await idle();await placed();
    assert.equal((await state()).layers.length,before.layers.length+1);
    await click('.image-placement-controls [data-command=apply_transform]');
    const pasted=sourceIdentity(await save());
    assert.ok(pasted.some(image=>JSON.stringify(image)===JSON.stringify(sources[0])),'Clipboard retains original source samples');
    await invoke('undo');assert.equal((await state()).layers.length,before.layers.length);
    // Cancel while an asynchronous file read is pending; release it afterwards
    // to prove that a late decoder completion cannot publish a partial batch.
    await evaluate(`placementTest.read=File.prototype.arrayBuffer;File.prototype.arrayBuffer=function(){const file=this;return new Promise(resolve=>{placementTest.release=()=>placementTest.read.call(file).then(resolve)})}`);
    await invoke('import_image');await choose([files[0]]);await wait('placementTest.release');
    await click('.file-progress button');await evaluate('File.prototype.arrayBuffer=placementTest.read;placementTest.release();delete placementTest.release');await idle();
    assert.equal((await state()).layers.length,before.layers.length);
    // An otherwise valid read that crosses GPU replacement must also retire.
    await evaluate(`File.prototype.arrayBuffer=function(){const file=this;return new Promise(resolve=>{placementTest.release=()=>placementTest.read.call(file).then(resolve)})}`);
    await invoke('import_image');await choose([files[0]]);await wait('placementTest.release');
    await evaluate('layerApp.restartGpu()');await wait('layerApp.app.brush_ready()');
    await evaluate('File.prototype.arrayBuffer=placementTest.read;placementTest.release();delete placementTest.release');await idle();
    assert.equal((await state()).layers.length,before.layers.length,'A read from the retired GPU cannot publish layers');
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'new',group:true,clipped:false}})`);await settle();
    const group=(await state()).layers.find(l=>l.group),groupCount=(await state()).layers.length;
    const rowPoint=await evaluate(`(()=>{const r=document.querySelector('.layer-row[data-layer="${group.id}"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await drop(rowPoint,[files[0]]);
    await idle();await placed();await click('.image-placement-controls [data-command=apply_transform]');
    const grouped=await save();assert.ok(grouped.document.layers.some(l=>l.properties.parent===group.id&&l.name!==group.label));
    await invoke('undo');assert.equal((await state()).layers.length,groupCount);
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'lock',id:${group.id},value:true}})`);await settle();
    await drop(rowPoint,[files[0]]);
    await settle();assert.equal((await state()).layers.length,groupCount);assert.equal((await state()).document_file.busy,false);
    for(let i=0;i<files.length;i++) {
      await invoke('open_document');await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')?.click()`);
      await choose([files[i]]);await idle();await wait('layerApp.app.brush_ready()');
      const opened=await save();
      assert.deepEqual([opened.document.width,opened.document.height],sources[i].extent,'Open uses oriented source dimensions');
      assert.deepEqual(sourceIdentity(opened),[sources[i]],'Open and placement decode the same exact source samples');
    }
    if(process.env.LAYER_IMAGE_MOTION==='1') {
      await invoke('new_document');await wait('document.querySelector("dialog[open] input[type=number]")');
      await evaluate(`{const fields=document.querySelectorAll('dialog[open] input[type=number]');fields[0].value=2000;fields[1].value=1500;[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create').click();}`);
      await idle();await wait('layerApp.app.brush_ready()');
      const started=Date.now();await importFiles(files);await click('.image-placement-controls [data-command=apply_transform]');
      const loadingMs=Date.now()-started,baseline=await save();
      await measurePlacedPhotos({call,evaluate,settle,invoke,save,sourceIdentity,baseline,loadingMs});
    }
    console.log('Image placement: Open, batches, clipboard, fit, Apply/Cancel, one-step history, exact sources after reopen, Original Size, malformed/stale/cancelled requests, canvas/group/locked drops and compact controls passed');
  } finally {
    await call('Page.setInterceptFileChooserDialog',{enabled:false});
    await evaluate('if(placementTest.read)File.prototype.arrayBuffer=placementTest.read');
    await evaluate('window.showOpenFilePicker=placementTest.open;window.showSaveFilePicker=placementTest.save;delete window.placementTest');
    await rm(root,{recursive:true,force:true});
  }
}
