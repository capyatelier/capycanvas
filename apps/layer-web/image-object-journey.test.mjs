import assert from 'node:assert/strict';
import {readPackage,packageObjects,packageOccurrences,packageObject,sourceIdentity} from './package-fixture.test.mjs';
import {histogramJourney} from './histogram-journey.mjs';

export async function checkImageObjectFixture({call,evaluate,settle,canvasPixels}) {
  const onlyObjects=process.env.LAYER_OBJECT_ONLY_FIXTURE==='1';
  const urls=JSON.parse(process.env.LAYER_OBJECT_FIXTURE_URLS??'["/fixtures/shared-image-f64-builtin.capy","/fixtures/shared-image-f64-icc.capy","/fixtures/shared-image-f64-nearest.capy"]');
  assert.ok(urls.length,'Set LAYER_OBJECT_FIXTURE_URLS to valid-profile object packages');
  const wait=async condition=>{
    const deadline=Date.now()+130000;
    for(;;)try{return await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+120000;function poll(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}+': '+JSON.stringify({busy:layerApp.documents.busy(),file:layerApp.state().document_file,requests:layerApp.state().requests,brush:layerApp.app.brush_ready(),error:layerApp.state().host_error,dialog:document.querySelector('dialog[open]')?.innerText?.slice(0,300)},(_,v)=>typeof v==='bigint'?String(v):v)));else setTimeout(poll,30)}poll()})`);}
    catch(error){if(!/navigated|context/i.test(String(error))||Date.now()>deadline)throw error;await new Promise(resolve=>setTimeout(resolve,50));}
  };
  const idle=()=>wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()');
  const begin=async command=>{await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);await settle();};
  const invoke=async command=>{await begin(command);await idle();};
  const histogram=histogramJourney({evaluate,settle}).exact;
  const presented=async label=>{
    let pixels;
    for(const end=Date.now()+120000;Date.now()<end;) {
      pixels=await canvasPixels();
      if(pixels.colored>10)break;
      await new Promise(resolve=>setTimeout(resolve,100));
    }
    assert.ok(pixels.colored>10,label);
  };
  const button=label=>`[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent===${JSON.stringify(label)})`;
  const click=async label=>{await wait(`!!${button(label)}&&!${button(label)}.disabled`);await evaluate(`${button(label)}.click()`);await settle();};
  const opaque=selector=>evaluate(`(selector=>[...document.querySelectorAll(selector)].every(canvas=>{const pixels=canvas.getContext('2d',{willReadFrequently:true}).getImageData(0,0,canvas.width,canvas.height).data;return pixels.some((value,index)=>index%4===3&&value>0)}))(${JSON.stringify(selector)})`);
  const comparison=async(command,reference)=>{
    console.log(`Object capture comparison: ${command}`);
    await begin(command);await click('Preview Complete Result');
    const selector=command==='rasterize_source'?'dialog[open] canvas[aria-label="Prepared composition"]':'dialog[open] .color-comparison canvas';
    await wait(`document.querySelectorAll(${JSON.stringify(selector)}).length===${command==='rasterize_source'?1:2}`);
    assert.ok(await opaque(selector),'Comparison captures include object samples');
    if(command==='rasterize_source') {
      const paint=packageObjects(reference,'capy.paint-source/2').find(paint=>paint.data.base);
      const original=paint.data.base.image.ref;
      await click('Rasterize');await idle();
      const applied=await save(),base=packageObject(applied,paint.id).data.base;
      assert.equal(base.policy,'working_pixels');assert.notEqual(base.image.ref,original);
      assert.equal(packageObjects(applied,'capy.image/1').length,2);
      assert.deepEqual(packageObjects(applied,'capy.image-object/1'),packageObjects(reference,'capy.image-object/1'));
      await invoke('undo');const undone=await save();assert.deepEqual(sourceIdentity(undone),sourceIdentity(reference));assert.deepEqual(packageObject(undone,paint.id).data.base,paint.data.base);
      await invoke('redo');const redone=await save();assert.deepEqual(sourceIdentity(redone),sourceIdentity(applied));assert.deepEqual(packageObject(redone,paint.id).data.base,base);
      await invoke('undo');assert.deepEqual(sourceIdentity(await save()),sourceIdentity(reference));
      console.log('Object source worker Preview, Apply, immutable image sharing and undo/redo passed');
    } else {await click('Cancel');await idle();}
    console.log(`Object capture comparison passed: ${command}`);
  };
  const save=async()=>{await invoke('save_document_as');await evaluate('(async()=>{objectJourney.saved=new Uint8Array(await(await objectJourney.handle.getFile()).arrayBuffer())})()');return readPackage(evaluate,'objectJourney.saved');};
  const install=()=>evaluate(`window.objectJourney={open:window.showOpenFilePicker,save:window.showSaveFilePicker,post:Worker.prototype.postMessage,jobs:0,coordinates:0};
    Worker.prototype.postMessage=function(message,...rest){if(message.request?.operation==='image-decode')objectJourney.jobs++;if(message.request?.operation==='nearest-coordinates')objectJourney.coordinates++;return objectJourney.post.call(this,message,...rest)};
    window.showOpenFilePicker=async()=>[{name:'objects.capy',getFile:async()=>new File([objectJourney.input],'objects.capy')}];
    window.showSaveFilePicker=async o=>{const directory=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-object-fixture-originals',{create:true});return objectJourney.handle=await directory.getFileHandle(crypto.randomUUID()+'.'+o.suggestedName.split('.').at(-1),{create:true});};`);
  await install();
  try {
    for(const url of urls) {
      const prior=await evaluate('objectJourney.jobs');
      const coordinates=await evaluate('objectJourney.coordinates');
      await evaluate(`(async()=>{const response=await fetch(${JSON.stringify(url)});if(!response.ok)throw Error('Missing object fixture');objectJourney.input=new Uint8Array(await response.arrayBuffer())})()`);
      if(onlyObjects){const input=await readPackage(evaluate,'objectJourney.input');assert.equal(packageObjects(input,'capy.paint-source/2').length,0);assert.equal(packageOccurrences(input).length,1);assert.ok(packageOccurrences(input)[0].data.content.objects);}
      await invoke('open_document');
      await evaluate('layerApp.app.wait_for_canvas()');
      await wait(`objectJourney.jobs>${prior}`);
      await presented('Cold object pixels present without subsequent input');
      if(url.includes('nearest'))assert.ok(await evaluate(`objectJourney.coordinates>${coordinates}`),'Cold Nearest placement prepares exact F64 coordinates on the worker');
      console.log(`${url}: cold object pixels presented without subsequent input`);
      if(onlyObjects){console.log('Object-only initial camera:',await evaluate('JSON.stringify(layerApp.app.camera(),(_,v)=>typeof v==="bigint"?Number(v):v)'));await evaluate(`layerApp.dispatch({type:'set_zoom',zoom:1.1193})`);await settle();await presented('Object-only pixels present at GTK-equivalent zoom');}
      await evaluate(`layerApp.dispatch({type:'set_zoom',zoom:1/(64*.55)})`);await settle();
      await evaluate('layerApp.app.wait_for_canvas()');
      await presented('64× source minification finishes and displays actual object samples');
      await evaluate(`layerApp.dispatch({type:'set_zoom',zoom:.125})`);await settle();
      await evaluate('layerApp.app.wait_for_canvas()');
      const before=await save(),objects=packageObjects(before,'capy.image-object/1');
      if(onlyObjects){assert.equal(packageObjects(before,'capy.paint-source/2').length,0);assert.equal(packageOccurrences(before).length,1);}
      assert.equal(objects.length,3);assert.equal(packageObjects(before,'capy.image/1').length,1);
      assert.ok(objects.every(object=>object.data.image.ref===objects[0].data.image.ref));
      assert.equal(objects[0].data.affine[4],16777217.125,'Package preserves the F64 coefficient beyond F32 precision');
      const imageId=objects[0].data.image.ref,owner=packageOccurrences(before).find(o=>o.data.content.objects);
      assert.deepEqual(owner.data.offset,['-16777216','0']);
      const runtime=await evaluate(`Number(layerApp.state().layers[${packageOccurrences(before).findIndex(o=>o.id===owner.id)}].id)`);
      for(const theme of ['light','dark']) {
        await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
        const visible=await histogram();assert.ok(visible.pixels>0,'Object-only fixture produces GPU pixels');
        console.log(`${url} ${theme}: exact object capture completed`);
        await evaluate(`layerApp.dispatch({type:'layer',action:{op:'visibility',id:${runtime},value:false}})`);await settle();
        const hidden=await histogram();assert.notDeepEqual(hidden,visible,'Hiding the object owner changes actual rendered samples');
        await invoke('undo');assert.deepEqual(await histogram(),visible);
        await invoke('redo');assert.deepEqual(await histogram(),hidden);await invoke('undo');
        const checkpoint=await save();assert.equal(packageObjects(checkpoint,'capy.image/1')[0].id,imageId);
        await evaluate('objectJourney.input=objectJourney.saved.slice()');await invoke('open_document');
        assert.deepEqual(await histogram(),visible,'Real package worker open retains object rendering');
        const reopened=await save();assert.deepEqual(packageObjects(reopened,'capy.image-object/1'),objects);assert.deepEqual(sourceIdentity(reopened),sourceIdentity(before));
        await evaluate('layerApp.restartGpu()');await wait('layerApp.app.brush_ready()');
        assert.deepEqual(await histogram(),visible,'Renderer replacement preserves object samples');
        await comparison('assign_profile');
        const paint=packageOccurrences(before).find(occurrence=>occurrence.data.content.paint);
        if(paint){
        const paintRuntime=await evaluate(`Number(layerApp.state().layers[${packageOccurrences(before).findIndex(occurrence=>occurrence.id===paint.id)}].id)`);
        await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${paintRuntime},mask:false}})`);await settle();
        await comparison('rasterize_source',before);
        }
        await begin('change_bit_depth');
        await evaluate(`(()=>{const select=document.querySelector('dialog[open] select[aria-label="Bit depth"]');select.value='F32';select.dispatchEvent(new Event('change',{bubbles:true}))})()`);
        await click('Preview Complete Result');await wait("document.querySelectorAll('dialog[open] .color-comparison canvas').length===2");assert.ok(await opaque('dialog[open] .color-comparison canvas'));await click('Cancel');await idle();
        await begin('export_document');await click('Preview Output');
        await wait(`!!document.querySelector('dialog[open] canvas[aria-label="Output preview"]')`);assert.ok(await opaque('dialog[open] canvas[aria-label="Output preview"]'));await click('Cancel');await idle();
        assert.equal(await evaluate('layerApp.state().host_error??null'),null);
        console.log(`${url} ${theme}: cold worker preparation, shared image/F64 placement, object GPU pixels, visibility undo/redo, save/reopen and renderer replacement passed`);
      }
      await evaluate(`layerApp.dispatch({type:'layer',action:{op:'visibility',id:${runtime},value:false}})`);await settle();
      const hidden=await histogram();await invoke('undo');const visible=await histogram();
      assert.equal(await evaluate("layerApp.state().commands.find(command=>command.id==='redo').enabled"),true);
      await wait('layerApp.app.document_park_ready()');await evaluate('layerApp.documents.autosave()');
      const old=await evaluate('performance.timeOrigin');await call('Page.reload',{ignoreCache:true});
      await wait(`performance.timeOrigin!==${old}&&window.layerApp?.app.brush_ready()&&!layerApp.documents.busy()`);
      await evaluate('layerApp.documents.startRecovery()');await idle();await install();
      assert.deepEqual(await histogram(),visible,'Private browser restart retains the object graph and samples');
      await invoke('redo');assert.deepEqual(await histogram(),hidden,'Private restart retains pending object-owner redo');
      await invoke('undo');assert.deepEqual(await histogram(),visible);
      const recovered=await save();assert.deepEqual(packageObjects(recovered,'capy.image-object/1'),objects);assert.equal(packageObjects(recovered,'capy.image/1')[0].id,imageId);
      console.log(`${url}: real IndexedDB restart restores pending object visibility redo/undo and exact F64/shared image identity`);
    }
    for(const theme of ['light','dark']) {
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
      const beforePaste=await save();
      const focus=await evaluate('({x:innerWidth/2,y:3})');
      for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...focus,button:'left',clickCount:1});
      await wait('document.hasFocus()');
      await call('Browser.grantPermissions',{origin:await evaluate('location.origin'),permissions:['clipboardReadWrite','clipboardSanitizedWrite']},null);
      const copied=await evaluate('layerApp.app.clip_nonce()??null');
      const copy=await evaluate("layerApp.state().commands.find(command=>command.label==='Copy Merged')");assert.equal(copy.enabled,true);
      const sent=await call('Runtime.evaluate',{expression:`layerApp.dispatch({type:'invoke',command:${JSON.stringify(copy.id)}})`,userGesture:true});assert.equal(sent.exceptionDetails,undefined);
      await wait(`(layerApp.app.clip_nonce()??null)!==${JSON.stringify(copied)}||!!layerApp.state().host_error`);
      assert.equal(await evaluate('layerApp.state().host_error??null'),null);await idle();
      assert.ok(await evaluate(`(async()=>{const items=await navigator.clipboard.read(),item=items.find(item=>item.types.includes('image/png'));if(!item)return false;const bitmap=await createImageBitmap(await item.getType('image/png')),canvas=new OffscreenCanvas(bitmap.width,bitmap.height),context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(bitmap,0,0);const pixels=context.getImageData(0,0,canvas.width,canvas.height).data;bitmap.close();return pixels.some((value,index)=>index%4===3&&value>0)})()`),'Clipboard captures include object samples');
      await invoke('paste_in_place');const pasted=await save();
      assert.equal(packageOccurrences(pasted).length,packageOccurrences(beforePaste).length+1);
      assert.deepEqual(packageObjects(pasted,'capy.image-object/1'),packageObjects(beforePaste,'capy.image-object/1'));
      const paintIds=new Set(packageObjects(beforePaste,'capy.paint-source/2').map(paint=>paint.id));
      const pastedPaint=packageObjects(pasted,'capy.paint-source/2').find(paint=>!paintIds.has(paint.id));
      assert.equal(pastedPaint.data.base.policy,'working_pixels');
      assert.equal(packageObject(pasted,pastedPaint.data.base.image).data.interpretation.depth,packageObject(beforePaste,beforePaste.root).data.color?.depth??'u8');
      for(const source of sourceIdentity(beforePaste))assert.deepEqual(sourceIdentity(pasted).find(image=>image.id===source.id),source);
      await invoke('undo');assert.deepEqual(sourceIdentity(await save()),sourceIdentity(beforePaste));
      await invoke('redo');assert.deepEqual(sourceIdentity(await save()),sourceIdentity(pasted));await invoke('undo');
      assert.equal(await evaluate('layerApp.state().host_error??null'),null);
      console.log(`Object clipboard ${theme}: real Copy Merged PNG, full-depth in-place paste, immutable originals and undo/redo passed`);
    }
  } finally {
    await evaluate('if(window.objectJourney){Worker.prototype.postMessage=objectJourney.post;window.showOpenFilePicker=objectJourney.open;window.showSaveFilePicker=objectJourney.save;delete window.objectJourney}');
  }
}
