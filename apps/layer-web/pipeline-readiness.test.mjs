import assert from 'node:assert/strict';
import {tracePipelineCalls} from './pipeline-trace.test.mjs';
import {readPackage,packageComposition,packageObjects,rasterIdentity} from './package-fixture.test.mjs';

export async function checkPipelineReadiness({cdp,call,evaluate,settle}) {
  const trace=await tracePipelineCalls(cdp);
  try{
  for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,x:1,y:1,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
  await evaluate('layerApp.documents.startRecovery().then(()=>layerApp.documents.autosave())');
  const loaded=cdp.once('Page.loadEventFired',120000);
  await call('Page.reload',{ignoreCache:true});
  await loaded;
  const wait=(expression,timeout=60000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};(function check(){try{if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}+' '+JSON.stringify({times:window.layerApp?.startupTimes,notice:document.querySelector('#gpu-notice')?.textContent,visibility:document.visibilityState})));else setTimeout(check,30);}catch(e){reject(e)}})();})`);
  const click=label=>evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)});if(!b)throw Error('Missing '+${JSON.stringify(label)});b.click()})()`);
  await wait('window.layerApp?.app.brush_ready()&&!layerApp.app.shader_work_pending(false)',120000);
  await evaluate('layerApp.documents.startRecovery()');
  console.log('Pipeline startup boundary:',await evaluate(`JSON.stringify({times:layerApp.startupTimes,required:layerApp.app.shader_work_pending(false),optional:layerApp.app.shader_work_pending(true),dialogs:[...document.querySelectorAll('dialog[open],#header details[open],:popover-open:not(.hover-tooltip)')].map(node=>node.textContent.slice(0,180))})`));
  await trace.check('Startup');
  console.log('Pipeline regression: startup uses only asynchronous page and worker pipelines');
  await evaluate(`(()=>{const hold=Object.assign(document.createElement('div'),{id:'hold-optional-compiles',popover:'manual'});document.body.append(hold);hold.showPopover();})()`);
  await evaluate(`layerApp.dispatch({type:'invoke',command:'new_document'})`);
  await wait('!!document.querySelector("dialog[open]")');
  if(await evaluate('!![...document.querySelectorAll("dialog[open] button")].find(b=>b.textContent==="Discard Changes")'))await click('Discard Changes');
  await wait(`!!document.querySelector('dialog[open] select[aria-label="Color space"]')`);
  await evaluate(`(()=>{const d=document.querySelector('dialog[open]');d.querySelector('select[aria-label="Color space"]').value='DisplayP3';d.querySelector('select[aria-label="Bit depth"]').value='U8';})()`);
  await click('Create');
  await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()&&layerApp.app.document_color().space==="DisplayP3"',120000);
  await settle();
  const output=async()=>{
    await evaluate(`layerApp.dispatch({type:'invoke',command:'export_document'})`);
    await wait(`!!document.querySelector('dialog[open] [aria-label="Dynamic range"]')`);
    await click('Preview Output');
    await wait(`!!document.querySelector('canvas[aria-label="Output preview"]')&&![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Preview Output').disabled`);
    const pixel=await evaluate(`(()=>{const c=document.querySelector('canvas[aria-label="Output preview"]');return Array.from(c.getContext('2d').getImageData(Math.floor(c.width/2),Math.floor(c.height/2),1,1).data)})()`);
    await click('Cancel');await settle();return pixel;
  };
  const blank=await output();
  await evaluate(`layerApp.dispatch({type:'invoke',command:'select_all'})`);
  await wait('layerApp.app.brush_ready()&&!layerApp.app.shader_work_pending(false)');
  await settle();
  await evaluate(`(()=>{
    const originals={},held=[];
    window.pipelineWait={calls:[],held,release(){for(const resolve of held.splice(0))resolve()},restore(){for(const [method,original] of Object.entries(originals))GPUDevice.prototype[method]=original}};
    for(const method of ['createComputePipelineAsync','createRenderPipelineAsync']){
      const original=originals[method]=GPUDevice.prototype[method];
      GPUDevice.prototype[method]=function(descriptor){
        pipelineWait.calls.push({method,label:descriptor.label,recipe:__pipelineTraceFingerprint(this,descriptor)});
        return original.call(this,descriptor).then(pipeline=>new Promise(resolve=>held.push(()=>resolve(pipeline))));
      };
    }
  })()`);
  await evaluate(`layerApp.dispatch({type:'invoke',command:'fill_selection'})`);
  const revision=await evaluate('String(layerApp.state().document_file.revision)');
  await wait('pipelineWait.held.length>0');
  const count=await evaluate('pipelineWait.calls.length');
  await evaluate(`layerApp.dispatch({type:'invoke',command:'deselect'})`);
  await settle();
  await new Promise(resolve=>setTimeout(resolve,150));
  assert.match(revision,/^\d+$/,'The browser returned the actual artwork revision');
  assert.equal(await evaluate('String(layerApp.state().document_file.revision)'),revision,'Deselect waits for Fill pipeline readiness before changing the drawing');
  assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),true,'Queued Deselect preserves selection until Fill completes');
  assert.equal(await evaluate('pipelineWait.calls.length'),count,'Deselect does not duplicate compilation');
  const recipes=await evaluate('pipelineWait.calls.map(call=>call.recipe)');
  assert.equal(new Set(recipes).size,recipes.length,'Each held pipeline recipe compiles once');
  console.log('Pipeline regression: Fill and Deselect wait without duplicate compilation');
  await evaluate('pipelineWait.restore();pipelineWait.release()');
  await wait('!layerApp.state().layer_tools.has_selection&&layerApp.app.brush_ready()');
  await settle();
  const filled=await output();
  assert.ok(filled.slice(0,3).some(value=>value<200),'Deferred Fill deposits GPU pixels');
  await evaluate(`layerApp.dispatch({type:'invoke',command:'undo'})`);
  await settle();
  await wait('layerApp.state().layer_tools.has_selection&&layerApp.app.brush_ready()');
  assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),true,'Undo restores the selection');
  assert.deepEqual(await output(),filled,'Undoing Deselect keeps the filled pixels');
  await evaluate(`layerApp.dispatch({type:'invoke',command:'undo'})`);
  await settle();
  assert.deepEqual(await output(),blank,'Undo removes the deferred Fill in one edit');
  await evaluate(`layerApp.dispatch({type:'invoke',command:'redo'});layerApp.dispatch({type:'invoke',command:'redo'})`);
  await settle();
  assert.deepEqual(await output(),filled,'Redo restores the deferred Fill once');
  assert.equal(trace.calls.filter(call=>call.realm==='page'&&recipes.includes(call.recipe)).length,recipes.length,'Completion, Undo and Redo do not resubmit held pipeline recipes');
  console.log('Pipeline regression: deferred Fill pixels and Undo/Redo pass');
  await evaluate(`document.querySelector('#hold-optional-compiles').remove();layerApp.dispatch({type:'invoke',command:'pencil'})`);
  await wait('layerApp.app.brush_ready()');
  for(const [type,fraction] of [['mousePressed',0.4],['mouseMoved',0.55],['mouseReleased',0.7]]){
    const point=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]*${fraction})*r.width/c.viewport[0],y:r.y+(a[1]+a[3]*0.5)*r.height/c.viewport[1]}})()`);
    await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:'pen',force:type==='mouseReleased'?0:0.7});
    await settle();
  }
  await wait('layerApp.app.brush_ready()');
  const painted=await output();
  await evaluate(`layerApp.dispatch({type:'invoke',command:'scale_rotate'})`);
  await wait(`layerApp.state().tool_settings.some(s=>s.id==='transform_angle')&&layerApp.state().commands.find(c=>c.id==='apply_transform').enabled`);
  await evaluate(`layerApp.dispatch({type:'set_tool_setting',id:'transform_angle',value:0.3})`);
  const beforeTransform=await evaluate('String(layerApp.state().document_file.revision)');
  await evaluate(`layerApp.dispatch({type:'invoke',command:'apply_transform'})`);
  await wait(`layerApp.state().document_file.revision>BigInt(${JSON.stringify(beforeTransform)})&&layerApp.state().canvas_bar?.label!=='Applying transform…'`,120000);
  await evaluate(`layerApp.dispatch({type:'invoke',command:'undo'})`);
  await settle();
  assert.deepEqual(await output(),painted,'Transform Undo restores the painted artwork');
  await new Promise(resolve=>setTimeout(resolve,5500));
  await evaluate(`layerApp.dispatch({type:'invoke',command:'scale_rotate'})`);
  await wait(`layerApp.state().tool_settings.some(s=>s.id==='transform_angle')&&layerApp.state().commands.find(c=>c.id==='apply_transform').enabled`);
  await trace.holdTransforms();
  const beforeSwitch=await evaluate('String(layerApp.state().document_file.revision)');
  await evaluate(`layerApp.dispatch({type:'set_tool_setting',id:'transform_angle',value:0.2});layerApp.dispatch({type:'invoke',command:'pencil'})`);
  await trace.waitHeld();
  assert.equal(await evaluate(`layerApp.state().canvas_bar?.label`),'Applying transform…','Tool switch waits while genuine worker pipeline readiness is held');
  await trace.releaseTransforms();
  await wait(`layerApp.state().document_file.revision>BigInt(${JSON.stringify(beforeSwitch)})&&layerApp.state().brush.tool==='pencil'&&layerApp.app.brush_ready()&&layerApp.state().canvas_bar?.label!=='Applying transform…'`,120000);
  assert.equal(await evaluate(`document.body.dataset.gpu`),'ready','Tool switch commits the transform without losing the GPU');
  await trace.check('Cold transform Apply and tool switch',{worker:true});
  console.log('Pipeline regression: cold transform Apply, Undo and tool switch use only asynchronous pipelines');
  const layer=await evaluate('String(layerApp.state().layers.find(l=>l.editing).id)');
  await evaluate(`window.pipelineArchive={picker:window.showSaveFilePicker};window.showSaveFilePicker=async options=>{const directory=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-test-pipeline-originals',{create:true});return pipelineArchive.handle=await directory.getFileHandle(crypto.randomUUID()+'.'+options.suggestedName.split('.').at(-1),{create:true});}`);
  const save=async()=>{
    await evaluate(`pipelineArchive.handle=null;layerApp.dispatch({type:'invoke',command:'save_document_as'})`);
    await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()&&!!pipelineArchive.handle');
    return readPackage(evaluate,'new Uint8Array(await(await pipelineArchive.handle.getFile()).arrayBuffer())');
  };
  try{
    const size=packageComposition(await save()).data.size;
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'add_mask',id:${layer}n,replace:false}});layerApp.dispatch({type:'layer',action:{op:'link_mask',id:${layer}n,value:true}});layerApp.dispatch({type:'layer',action:{op:'select',id:${layer}n,mask:true}})`);
    await wait('layerApp.app.brush_ready()&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.state().layer_tools.editing_layer.mask_selected');
    await evaluate(`layerApp.dispatch({type:'invoke',command:'eraser'});layerApp.dispatch({type:'select_brush',id:3});layerApp.dispatch({type:'set_brush_size',value:90})`);
    await wait('layerApp.app.brush_ready()&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.state().layer_tools.editing_layer.mask_selected&&layerApp.state().layer_tools.tool==="paint"&&layerApp.state().brush.tool==="eraser"&&layerApp.state().brush.diameter===90');
    for(const [type,fraction] of [['mousePressed',0.42],['mouseMoved',0.5],['mouseReleased',0.58]]){
      const point=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(${size[0]*fraction}*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(${size[1]*0.5}*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
      await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:'pen',force:type==='mouseReleased'?0:0.7});await settle();
    }
    await wait('layerApp.app.brush_ready()&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&!layerApp.app.shader_work_pending(false)');
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${layer}n,mask:false}})`);
    await wait('layerApp.app.brush_ready()&&!layerApp.state().layer_tools.editing_layer.mask_selected');
    const masked=await save(),coverage=packageObjects(masked,'capy.coverage-source/2');
    assert.ok(packageObjects(masked,'capy.paint-source/2').some(source=>source.data.tiles.some(tile=>tile.plane==='color')),'Linked-mask fixture contains authored color');
    assert.ok(coverage.some(source=>(source.data.default_coverage??1)===1&&source.data.tiles?.length),`Linked-mask fixture contains sparse authored default-one coverage: ${JSON.stringify(coverage)}`);
    assert.equal(await evaluate(`layerApp.state().layers.find(l=>String(l.id)===${JSON.stringify(layer)}).mask_linked`),true);
    await evaluate(`layerApp.dispatch({type:'invoke',command:'scale_rotate'})`);
    await wait(`layerApp.state().commands.find(c=>c.id==='apply_transform').enabled&&layerApp.app.brush_ready()`);
    await evaluate(`layerApp.dispatch({type:'set_tool_setting',id:'transform_angle',value:0.3})`);
    await wait('layerApp.app.brush_ready()');
    await new Promise(resolve=>setTimeout(resolve,5500));
    const revision=await evaluate('String(layerApp.state().document_file.revision)');
    await evaluate(`pipelineArchive.post=Worker.prototype.postMessage;Worker.prototype.postMessage=function(value,...args){if(value.request?.operation==='snapshot'&&JSON.parse(value.request.metadata)[2]?.TransformPixels){Worker.prototype.postMessage=pipelineArchive.post;this.dispatchEvent(new MessageEvent('message',{data:{gpu_event:{role:'runtime',kind:'panic',message:'capy-test: Pipeline used before preparation'}}}));return;}return pipelineArchive.post.call(this,value,...args)};layerApp.dispatch({type:'invoke',command:'apply_transform'})`);
    await wait(`document.querySelector('.canvas-notice-text')?.textContent.includes('capy-test: Pipeline used before preparation')&&layerApp.state().canvas_bar?.label!=='Applying transform…'&&layerApp.app.brush_ready()`,30000);
    assert.equal(await evaluate('String(layerApp.state().document_file.revision)'),revision,'Failed worker Apply leaves artwork unchanged');
    assert.equal(await evaluate('document.body.dataset.gpu'),'ready','A worker panic leaves the independent canvas usable');
    assert.deepEqual(rasterIdentity(await save()),rasterIdentity(masked),'Failed worker Apply preserves color and coverage resources');
    await evaluate(`layerApp.dispatch({type:'invoke',command:'scale_rotate'})`);
    await wait(`layerApp.state().commands.find(c=>c.id==='apply_transform').enabled&&layerApp.app.brush_ready()`);
    await evaluate(`layerApp.dispatch({type:'set_tool_setting',id:'transform_angle',value:0.3})`);
    await wait('layerApp.app.brush_ready()');
    await new Promise(resolve=>setTimeout(resolve,5500));
    const previousWorkers=new Set(trace.calls.filter(record=>record.realm==='worker').map(record=>record.session)),firstCall=trace.calls.length;
    await evaluate(`layerApp.dispatch({type:'invoke',command:'apply_transform'})`);
    await wait(`layerApp.state().document_file.revision>BigInt(${JSON.stringify(revision)})&&layerApp.state().canvas_bar?.label!=='Applying transform…'&&layerApp.app.brush_ready()`,120000);
    const workerCalls=trace.calls.slice(firstCall).filter(record=>record.realm==='worker'&&record.task==='TransformPixels');
    assert.ok(workerCalls.length&&workerCalls.every(record=>!previousWorkers.has(record.session)),'Linked-mask Apply uses a fresh worker GPU device');
    const initializations=trace.jobs.filter(record=>record.initialization&&workerCalls.some(call=>call.session===record.session));
    assert.ok(initializations.length&&initializations.every(record=>record.module&&record.initialization==='instantiate'),'Cold Apply instantiates the shared compiled Wasm module');
    assert.ok(workerCalls.every(record=>record.label!=='tile layer composition'),'Raw linked-mask capture never compiles generic scene composition');
    await trace.check('Cold linked-mask Apply after worker panic',{worker:true});
    const accepted=await save();
    for(const type of ['capy.paint-source/2','capy.coverage-source/2'])assert.notDeepEqual(rasterIdentity(accepted).filter(source=>source.type===type),rasterIdentity(masked).filter(source=>source.type===type),`Apply transforms ${type} authored pixels`);
    await evaluate(`layerApp.dispatch({type:'invoke',command:'undo'})`);await wait('layerApp.app.brush_ready()');
    assert.deepEqual(rasterIdentity(await save()),rasterIdentity(masked),'One Undo restores exact linked color and coverage resources');
    await evaluate(`layerApp.dispatch({type:'invoke',command:'redo'})`);await wait('layerApp.app.brush_ready()');
    assert.deepEqual(rasterIdentity(await save()),rasterIdentity(accepted),'One Redo restores exact linked transform resources');
    console.log('Pipeline regression: cold authored linked-mask Apply, shared Wasm Module, worker panic cause/retry and exact Undo/Redo pass without generic composition or blocking pipelines');
  }finally{await evaluate(`window.showSaveFilePicker=pipelineArchive.picker;if(pipelineArchive.post)Worker.prototype.postMessage=pipelineArchive.post;delete window.pipelineArchive`);}
  }finally{await trace.close().catch(error=>console.error('Pipeline trace cleanup:',error));}
}
