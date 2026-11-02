import {readPackage,authoredIdentity,packageResourceIdentity,restartStoreUrl} from './package-fixture.test.mjs';
import assert from 'node:assert/strict';
import {tracePipelineCalls} from './pipeline-trace.test.mjs';
import {mkdir,writeFile} from 'node:fs/promises';
import {createDrawing} from './new-drawing-fixture.mjs';

export async function checkDrawingTabRecovery({cdp,call,evaluate,settle}) {
  const trace=await tracePipelineCalls(cdp);
  const storeUrl=await restartStoreUrl(evaluate);
  try {
  const wait=async condition=>{
    const deadline=Date.now()+250000;
    for(;;)try{return await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+240000;function check(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}+': '+document.querySelector('#status')?.textContent));else setTimeout(check,30);}check();})`);}
    catch(error){if(!/navigated|context/i.test(String(error))||Date.now()>deadline)throw error;await new Promise(resolve=>setTimeout(resolve,50));}
  };
  const ready=async()=>{await wait('window.layerApp?.app.brush_ready()&&!layerApp.documents.busy()&&layerApp.app.document_park_ready()');await evaluate(`Promise.race([layerApp.documents.startRecovery(),new Promise((_,reject)=>{const deadline=performance.now()+45000;function check(){if(!document.querySelector('dialog[open]')&&performance.now()<deadline){setTimeout(check,30);return;}reject(Error(JSON.stringify({dialogs:[...document.querySelectorAll('dialog[open]')].map(dialog=>dialog.textContent),file:layerApp.state().document_file,host_error:layerApp.state().host_error,notice:layerApp.state().notice,status:document.querySelector('#status')?.textContent},(_,value)=>typeof value==='bigint'?String(value):value)));}check();})])`);};
  const invoke=command=>evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);
  const tabs=()=>evaluate(`JSON.parse(JSON.stringify(layerApp.app.document_tabs(0),(_,v)=>typeof v==='bigint'?Number(v):v))`);
  const select=async id=>{await ready();await evaluate(`layerApp.documents.select(BigInt(${id})).catch(error=>{throw Error(JSON.stringify({error,tabs:layerApp.app.document_tabs(0),busy:layerApp.documents.busy(),dialogs:[...document.querySelectorAll('dialog[open]')].map(dialog=>dialog.textContent)},(_,v)=>typeof v==='bigint'?String(v):v));})`);await ready();};
  const create=async()=>{
    const count=(await tabs()).tabs.length;await invoke('new_document');
    await evaluate(createDrawing());
    await wait(`layerApp.app.document_tabs(0).tabs.length===${count+1}`);await ready();return (await tabs()).selected;
  };
  const installSave=()=>evaluate(`window.showSaveFilePicker=async options=>{const directory=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-test-session-originals',{create:true});return directory.getFileHandle(crypto.randomUUID()+'.'+options.suggestedName.split('.').at(-1),{create:true});};`);
  const snapshot=()=>evaluate(`(()=>{const state=layerApp.state(),stamp=layerApp.app.session_stamp_for(layerApp.app.document_tabs(0).selected);return JSON.parse(JSON.stringify({layers:state.layers.map(({label,selected,editing,mask_selected})=>({label,selected,editing,mask_selected})),camera:stamp.state.camera,modified:state.document_file.modified,location:state.document_file.location?.name??null,checkpoint:stamp.checkpoint,selection:state.layer_tools.has_selection,undo:state.commands.find(command=>command.id==='undo')?.enabled,redo:state.commands.find(command=>command.id==='redo')?.enabled},(_,v)=>typeof v==='bigint'?Number(v):v));})()`);
  const leaveWarning=()=>evaluate(`(()=>{const event=new Event('beforeunload',{cancelable:true});window.dispatchEvent(event);return event.defaultPrevented;})()`);
  const restart=async()=>{await evaluate('layerApp.documents.autosave()');const old=await evaluate('performance.timeOrigin');await call('Page.reload');await wait(`performance.timeOrigin!==${old}&&window.layerApp?.app.brush_ready()`);await ready();};
  await ready();
  const native=await create();await invoke('pencil');await ready();
  const nativePoint=(x,y)=>evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(${x}*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(${y}*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
  const nativePixel=async()=>{
    const click=label=>evaluate(`(()=>{const button=[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent===${JSON.stringify(label)});if(!button)throw Error(${JSON.stringify(label)});button.click()})()`);
    await invoke('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Dynamic range"]')`);await click('Preview Output');
    await wait(`!!document.querySelector('canvas[aria-label="Output preview"]')&&![...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Preview Output').disabled`);
    const pixel=await evaluate(`(()=>{const c=document.querySelector('canvas[aria-label="Output preview"]'),tab=layerApp.state().tabs.find(tab=>tab.active);return Array.from(c.getContext('2d').getImageData(Math.floor(48*c.width/tab.width),Math.floor(48*c.height/tab.height),1,1).data)})()`);
    await click('Cancel');await settle();return pixel;
  };
  const nativePixelsMatch=async(predicate,message)=>{
    const deadline=Date.now()+30000;let pixel;
    do{pixel=await nativePixel();if(predicate(pixel))return pixel;await new Promise(resolve=>setTimeout(resolve,30))}while(Date.now()<deadline);
    assert.fail(`${message}: ${pixel}; ${await evaluate('JSON.stringify({revision:String(layerApp.state().document_file.revision),undo:layerApp.state().commands.find(c=>c.id==="undo").enabled,redo:layerApp.state().commands.find(c=>c.id==="redo").enabled})')}`);
  };
  const nativeBlank=await nativePixel();
  for(const [type,x] of [['mousePressed',24],['mouseMoved',48],['mouseReleased',72]]){
    await call('Input.dispatchMouseEvent',{type,...await nativePoint(x,48),button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:'pen',force:type==='mouseReleased'?0:0.7});await settle();
  }
  await ready();const nativePaint=await nativePixelsMatch(pixel=>pixel[3]>0&&pixel.some((value,index)=>Math.abs(value-nativeBlank[index])>10),'Owned native pen stroke deposits pixels');
  assert.equal(await evaluate('layerApp.state().layers.some(layer=>layer.object)'),false,'Cold restore fixture contains only native paint');
  const decoder=await call('Page.addScriptToEvaluateOnNewDocument',{source:`(()=>{
    const original=GPUDevice.prototype.createRenderPipelineAsync,releases=[];
    window.nativeRestoreHold={held:false,released:false,release(){this.released=true;for(const release of releases.splice(0))release()},restore(){GPUDevice.prototype.createRenderPipelineAsync=original}};
    GPUDevice.prototype.createRenderPipelineAsync=function(descriptor){
      const result=original.call(this,descriptor);
      if(descriptor.label!=='decode source tile')return result;
      return result.then(pipeline=>nativeRestoreHold.released?pipeline:new Promise(resolve=>{nativeRestoreHold.held=true;releases.push(()=>resolve(pipeline))}));
    };
  })()`});
  try {
    await evaluate('layerApp.documents.autosave()');const old=await evaluate('performance.timeOrigin');await call('Page.reload',{ignoreCache:true});
    await wait(`performance.timeOrigin!==${old}&&window.nativeRestoreHold?.held&&window.layerApp`);
    assert.equal(await evaluate('layerApp.app.brush_ready()'),false,'Native paint restoration waits for its actual decoder pipeline');
    await evaluate('nativeRestoreHold.release();nativeRestoreHold.restore()');
    await ready();assert.equal((await tabs()).selected,native,'Cold native paint restore preserves the drawing');
    await nativePixelsMatch(pixel=>pixel.every((value,index)=>Math.abs(value-nativePaint[index])<=2),'Cold restore preserves native pixels');
    await invoke('undo');await ready();await nativePixelsMatch(pixel=>pixel.every((value,index)=>Math.abs(value-nativeBlank[index])<=2),'Undo removes the restored native stroke');
    await invoke('redo');await ready();await nativePixelsMatch(pixel=>pixel.every((value,index)=>Math.abs(value-nativePaint[index])<=2),'Redo restores the same native samples');
  } finally {await call('Page.removeScriptToEvaluateOnNewDocument',{identifier:decoder.identifier});await evaluate('window.nativeRestoreHold?.release();window.nativeRestoreHold?.restore()');}
  await evaluate(`layerApp.documents.close(BigInt(${native}));undefined`);await wait(`!![...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Discard Changes')`);
  await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Discard Changes').click()`);await ready();
  console.log('PASS cold native-only paint restore: held genuine decoder readiness, pixels and Undo/Redo');
  const origin=await evaluate('location.origin'),persistence=setting=>call('Browser.setPermission',{permission:{name:'persistent-storage'},setting,origin},null);
  const counter=await call('Page.addScriptToEvaluateOnNewDocument',{source:'window.persistRequests=0;{const persist=StorageManager.prototype.persist;StorageManager.prototype.persist=function(){persistRequests++;return persist.call(this);};}'});
  try {
    await ready();await persistence('denied');await restart();
    assert.equal(await leaveWarning(),false,'A blank drawing permits quiet exit without asking to keep storage');
    const unsaved=[];
    for(let i=0;i<2;i++){unsaved.push(await create());await invoke('add_layer');await ready();await evaluate('layerApp.documents.autosave()');await wait('persistRequests>0');}
    assert.equal(await evaluate('persistRequests'),1,'The first checkpoint of a never-saved drawing asks once per page to keep storage');
    assert.equal(await leaveWarning(),true,'A never-saved drawing keeps the leave warning while the browser may clear its storage');
    await persistence('granted');await call('Runtime.evaluate',{expression:'void 0',userGesture:true});await restart();
    assert.equal(await evaluate('persistRequests'),0,'Kept storage needs no request');
    assert.deepEqual((await tabs()).tabs.slice(-2).map(tab=>tab.id),unsaved);
    await evaluate('layerApp.documents.autosave()');
    assert.equal(await leaveWarning(),false,'Kept storage lets never-saved drawings restart quietly');
    for(const id of unsaved) {
      const count=(await tabs()).tabs.length;await evaluate(`layerApp.documents.close(BigInt(${id}))`);
      await wait("!![...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Discard Changes')");
      await evaluate("[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Discard Changes').click()");
      await wait(`layerApp.app.document_tabs(0).tabs.length===${count-1}`);await ready();
    }
  } finally {await call('Page.removeScriptToEvaluateOnNewDocument',{identifier:counter.identifier});}
  await ready();await installSave();
  const first=(await tabs()).selected,known=(await tabs()).tabs.map(tab=>String(tab.id));
  await evaluate(`window.sessionCaptureOriginal=layerApp.app.capture_tab_session.bind(layerApp.app);layerApp.app.capture_tab_session=id=>${JSON.stringify(known)}.includes(String(id))?sessionCaptureOriginal(id):{write:async()=>{throw Error('Injected first checkpoint failure')},free(){}};`);
  const pending=await create();
  try {
    assert.equal(await evaluate('layerApp.documents.autosave().then(()=>false,()=>true)'),true,'First checkpoint failure is reported');
    assert.equal(await evaluate(`(async()=>{const store=(await import(${JSON.stringify(storeUrl)})).createRestartStore();for(const key of await store.windows()){const manifest=await store.manifest(key),drawing=manifest.drawings.find(drawing=>String(drawing.id)===String(${pending}));if(drawing)return !(await store.read(drawing.key));}return false;})()`),true,'First checkpoint failure retains authoritative membership for retry');
    assert.equal(await leaveWarning(),true,'Pending unsaved state prevents quiet browser exit');
  } finally {await evaluate('layerApp.app.capture_tab_session=sessionCaptureOriginal');}
  await evaluate('layerApp.documents.autosave()');
  const beforeLostAck=await evaluate(`(async()=>{const store=(await import(${JSON.stringify(storeUrl)})).createRestartStore();for(const key of await store.windows()){const drawing=(await store.manifest(key)).drawings.find(drawing=>String(drawing.id)===String(${pending}));if(drawing){window.lostAckKey=drawing.key;return (await store.read(drawing.key)).current.generation;}}})()`);
  await evaluate(`layerApp.app.capture_tab_session=id=>{const capture=sessionCaptureOriginal(id);if(String(id)===String(${pending})){const write=capture.write.bind(capture);capture.write=async(...args)=>{await write(...args);throw Error('Injected lost checkpoint acknowledgement');};}return capture;};layerApp.dispatch({type:'set_zoom',zoom:.31});`);
  try {assert.equal(await evaluate('layerApp.documents.autosave().then(()=>false,()=>true)'),true,'A lost acknowledgement leaves the checkpoint pending');}
  finally {await evaluate('layerApp.app.capture_tab_session=sessionCaptureOriginal');}
  await evaluate('layerApp.documents.autosave()');
  assert.deepEqual(await evaluate(`(async()=>{const record=await (await import(${JSON.stringify(storeUrl)})).createRestartStore().read(lostAckKey);return [record.current.generation,record.previous.generation];})()`),[beforeLostAck+2,beforeLostAck],'A committed but unacknowledged checkpoint retries with a newer generation and retains the acknowledged base');
  await evaluate("window.sessionCloseOriginal=layerApp.app.prepare_document_close.bind(layerApp.app);layerApp.app.prepare_document_close=()=>{throw Error('Injected close preflight failure')};");
  try {
    await evaluate(`layerApp.documents.close(BigInt(${pending}))`);await ready();
    assert.equal((await tabs()).tabs.length,2,'Failed close preflight retains the live drawing');
    assert.equal(await evaluate(`(async()=>{const store=(await import(${JSON.stringify(storeUrl)})).createRestartStore();for(const key of await store.windows()){const manifest=await store.manifest(key),drawing=manifest.drawings.find(drawing=>String(drawing.id)===String(${pending}));if(drawing)return !!(await store.read(drawing.key));}return false;})()`),true,'Failed close preflight retains durable membership and its drawing');
  } finally {await evaluate('layerApp.app.prepare_document_close=sessionCloseOriginal');}
  await evaluate(`layerApp.documents.close(BigInt(${pending}))`);await wait('layerApp.app.document_tabs(0).tabs.length===1');await ready();
  await invoke('add_layer');await ready();await invoke('save_document_as');await wait('!layerApp.state().document_file.busy');await ready();
  await invoke('add_layer');await ready();
  const second=await create();await invoke('add_layer');await ready();await invoke('undo');await ready();
  const third=await create();await invoke('add_layer');await ready();await invoke('save_document_as');await wait('!layerApp.state().document_file.busy');await ready();await invoke('select_all');await ready();assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),true);
  await evaluate(`layerApp.app.reorder_document(BigInt(${third}),BigInt(${first}));layerApp.documents.refresh()`);
  const expected=new Map();
  for(const [index,id] of [first,second,third].entries()) {
    await select(id);await evaluate(`layerApp.dispatch({type:'set_zoom',zoom:${[.37,.5,.75][index]}})`);await settle();
    expected.set(id,await snapshot());
  }
  for(const theme of ['light','dark']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await select(first);
    await evaluate('layerApp.documents.autosave()');
    assert.equal(await leaveWarning(),false,'A complete private checkpoint permits a seamless browser restart');
    const old=await evaluate('performance.timeOrigin');await call('Page.reload',{ignoreCache:true});
    await wait(`performance.timeOrigin!==${old}&&window.layerApp?.app.brush_ready()`);await ready();
    assert.equal(await evaluate("document.querySelectorAll('dialog[open]').length"),0,'Ordinary restart restores automatically');
    assert.deepEqual((await tabs()).tabs.map(tab=>tab.id),[third,first,second]);
    assert.equal((await tabs()).selected,first,'Restart preserves the active drawing');console.log(`${theme} restart tab titles: ${JSON.stringify((await tabs()).tabs.map(tab=>tab.title))}`);
    for(const id of [first,second,third]) {await select(id);const actual=await snapshot(),prior=expected.get(id);for(let axis=0;axis<2;axis++)assert.ok(Math.abs(actual.camera.center[axis]-prior.camera.center[axis])<.002,'Restart preserves the canvas center within floating-point camera precision');actual.camera.center=prior.camera.center;assert.deepEqual(actual,prior,`${theme} restart preserves drawing ${id}, history, selection, camera and manual-save checkpoint`);}
    await select(second);await invoke('redo');await ready();assert.equal((await snapshot()).layers.length,expected.get(second).layers.length+1);await invoke('undo');await ready();
    await select(first);await invoke('undo');await ready();assert.equal(await evaluate('layerApp.state().document_file.modified'),false,'Undo reaches the retained manual-save checkpoint');await invoke('redo');await ready();assert.equal(await evaluate('layerApp.state().document_file.modified'),true);
    await installSave();
  }
  for(const theme of ['light','dark']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);
    const originals=[];
    for(const kind of ['intact','missing','changed']) {
      const id=await create();await invoke('add_layer');await ready();await invoke('save_document_as');await wait('!layerApp.state().document_file.busy');await ready();
      assert.equal(await evaluate('layerApp.state().document_file.modified'),false,'A manual save establishes a clean checkpoint');
      const original={id,kind,layers:(await snapshot()).layers.length};originals.push(original);
      if(kind==='intact') {
        await invoke('export_document');await wait('!!document.querySelector("dialog[open] select[aria-label=Format]")');
        await evaluate("[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Choose File…').click()");await wait('!layerApp.state().document_file.busy');await ready();
        original.export=await evaluate(`(async()=>{const name=layerApp.app.session_stamp_for(layerApp.app.document_tabs(0).selected).state.last_export.location.name,directory=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-test-session-originals'),file=await(await directory.getFileHandle(name)).getFile();return{name,modified:file.lastModified};})()`);
        await evaluate('layerApp.documents.autosave()');
        const beforeInvalid=await evaluate(`(async()=>{const store=(await import(${JSON.stringify(storeUrl)})).createRestartStore();for(const window of await store.windows()){const drawing=(await store.manifest(window)).drawings.find(drawing=>String(drawing.id)===String(${id}));if(drawing){globalThis.invalidExportKey=drawing.key;const record=await store.read(drawing.key);return [record.current.generation,record.previous?.generation??null];}}})()`);
        await evaluate(`globalThis.invalidExportPost=Worker.prototype.postMessage;Worker.prototype.postMessage=function(value,...args){if(value.request?.operation==='restart-begin'){const request=JSON.parse(value.request.metadata);if(request.key===invalidExportKey){const envelope=JSON.parse(request.project);envelope.descriptor.metadata.last_export.recipe.jpeg_quality=0;request.project=JSON.stringify(envelope);value.request.metadata=JSON.stringify(request);}}return invalidExportPost.call(this,value,...args);};layerApp.dispatch({type:'set_zoom',zoom:.43});`);
        try {
          assert.equal(await evaluate('layerApp.documents.autosave().then(()=>false,()=>true)'),true,'The worker rejects malformed export metadata before publication');
          assert.deepEqual(await evaluate(`(async()=>{const record=await (await import(${JSON.stringify(storeUrl)})).createRestartStore().read(invalidExportKey);return [record.current.generation,record.previous?.generation??null];})()`),beforeInvalid,'Rejected export metadata retains both complete checkpoint generations');
        } finally {await evaluate('Worker.prototype.postMessage=invalidExportPost');}
        await evaluate('layerApp.documents.autosave()');
      }
      if(kind!=='intact')await evaluate(`(async()=>{const directory=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-test-session-originals'),name=layerApp.state().document_file.location.name;if(${JSON.stringify(kind)}==='missing')await directory.removeEntry(name);else{const stream=await(await directory.getFileHandle(name)).createWritable();await stream.write(new Uint8Array([42]));await stream.close();}})()`);
    }
    await restart();
    assert.equal(await evaluate("document.querySelectorAll('dialog[open]').length"),0,'Original verification never prompts during restart');
    for(const {id,kind,layers} of originals) {
      await select(id);assert.equal((await snapshot()).layers.length,layers,'The private checkpoint retains the drawing independently of its original');
      assert.equal(await evaluate('layerApp.state().document_file.modified'),kind!=='intact',`${theme} ${kind} original determines restored protection`);
      if(kind==='intact') {
        await evaluate("window.showSaveFilePicker=async()=>{throw Error('Export Again must reuse its persisted handle')}");
        await invoke('export_again');await wait('!layerApp.state().document_file.busy');await ready();
        const exported=originals.find(original=>original.id===id).export;
        assert.ok(await evaluate(`(async()=>{const directory=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-test-session-originals');return(await(await directory.getFileHandle(${JSON.stringify(exported.name)})).getFile()).lastModified>${exported.modified};})()`),'Restored Export Again writes through its retained browser handle without another picker');
        await installSave();
      }
      const count=(await tabs()).tabs.length;await evaluate(`layerApp.documents.close(BigInt(${id}))`);
      if(kind!=='intact') {
        await wait("!![...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Cancel')");
        await evaluate("[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Cancel').click()");await ready();assert.equal((await tabs()).tabs.length,count,'Cancel retains the only protected drawing');
        await evaluate(`layerApp.documents.close(BigInt(${id}))`);await wait("!![...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Discard Changes')");
        await evaluate("[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Discard Changes').click()");
      }
      await wait(`layerApp.app.document_tabs(0).tabs.length===${count-1}`);await ready();
      assert.equal(await evaluate("document.querySelectorAll('dialog[open]').length"),0,'An intact clean original closes seamlessly after validation');
    }
    await installSave();
  }
  await select(first);await evaluate(`layerApp.documents.close(BigInt(${first}))`);
  await wait("!![...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Cancel')");
  await evaluate("[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Cancel').click()");await ready();assert.equal((await tabs()).tabs.length,3,'Cancelled close preserves the drawing');
  await evaluate(`layerApp.documents.close(BigInt(${first}))`);
  await wait("!![...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Discard Changes')");
  await evaluate("[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Discard Changes').click()");await wait('layerApp.app.document_tabs(0).tabs.length===2');await ready();
  await restart();
  assert.deepEqual((await tabs()).tabs.map(tab=>tab.id),[third,second],'An acknowledged close cannot resurrect on restart');
  await select(third);await evaluate(`layerApp.dispatch({type:'set_rotation',rotation:0})`);await invoke('fit_canvas');await invoke('deselect');await invoke('rectangle_select');await ready();await settle();
  const overlayPoint=(x,y)=>evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(${x}*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(${y}*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
  for(const [type,x,y] of [['mousePressed',24,24],['mouseMoved',72,72],['mouseReleased',72,72]]) {
    await call('Input.dispatchMouseEvent',{type,...await overlayPoint(x,y),button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});await settle();
  }
  await wait('layerApp.state().layer_tools.has_selection');await invoke('save_selection_layer');await evaluate(`layerApp.dispatch({type:'layer',action:{op:'cancel_rename'}})`);await ready();
  const overlayOwner=await evaluate('String(layerApp.state().layer_tools.mask_editing.layer)');
  const overlayProperties=()=>evaluate(`JSON.parse(JSON.stringify(layerApp.state().layer_properties.controls.filter(control=>['mask_color','mask_opacity'].includes(control.key)).map(control=>[control.key,control.value])))`);
  const overlayVisible=()=>evaluate(`layerApp.state().layers.find(layer=>String(layer.id)===${JSON.stringify(overlayOwner)}).visible`);
  const overlayEye=async()=>{
    await evaluate(`layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'layers',visible:true}})`);await settle();
    await evaluate(`document.querySelector('.dock-tab[data-panel="layers"],.column-tab[data-panel="layers"]')?.click()`);await settle();
    const selector=`.layer-row[data-layer="${overlayOwner}"] > button:first-child`;
    const point=await evaluate(`(()=>{const eye=document.querySelector(${JSON.stringify(selector)});eye.scrollIntoView({block:'nearest'});const r=eye.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
    await settle();await ready();
  };
  const overlayPixels=async(visible=true)=>{
    const deadline=Date.now()+30000;
    for(;;) {
      await evaluate('layerApp.wake()');await settle();await evaluate('layerApp.app.wait_for_canvas()');
      const point=await overlayPoint(48,48),shot=await call('Page.captureScreenshot',{format:'png',clip:{...point,width:1,height:1,scale:1}});
      const pixel=await evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${shot.data}';await image.decode();const canvas=document.createElement('canvas');canvas.width=canvas.height=1;const context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(image,0,0);return Array.from(context.getImageData(0,0,1,1).data)})()`);
      if(visible?pixel[1]>pixel[0]+20&&pixel[1]>pixel[2]+10:pixel.slice(0,3).every(channel=>channel>240)&&pixel[3]===255)return;
      assert.ok(Date.now()<deadline,`Saved overlay reaches presented ${visible?'green':'white'} pixels: ${pixel}`);await new Promise(resolve=>setTimeout(resolve,50));
    }
  };
  const overlayArchive=()=>readPackage(evaluate,`await (async()=>{const root=await navigator.storage.getDirectory(),directory=await root.getDirectoryHandle('capy-test-session-originals'),file=await directory.getFileHandle(layerApp.state().document_file.location.name);return new Uint8Array(await(await file.getFile()).arrayBuffer())})()`);
  await installSave();await invoke('save_document_as');await ready();const overlayOriginal=authoredIdentity(await overlayArchive());
  for(const theme of ['light','dark']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}});layerApp.dispatch({type:'select_layer',id:BigInt(${JSON.stringify(overlayOwner)})});layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'properties',visible:true}});`);await settle();
    await evaluate(`document.querySelector('.dock-tab[data-panel="properties"],.column-tab[data-panel="properties"]')?.click()`);await settle();
    for(const [key,value] of [['mask_opacity',{kind:'number',value:.37}],['mask_color',{kind:'color',value:{space:'Srgb',rgba:[.2,.7,.3,1]}}]])await evaluate(`layerApp.dispatch({type:'effect',action:{op:'set',layer:Number(${JSON.stringify(overlayOwner)}),key:${JSON.stringify(key)},value:${JSON.stringify(value)}}})`);
    await overlayPixels();const before=await overlayProperties();
    assert.equal(await evaluate('layerApp.state().document_file.modified'),false,'Private overlay properties retain the clean artwork checkpoint');
    for(const visible of [false,true]) {
      await overlayEye();assert.equal(await overlayVisible(),visible,`${theme}: saved selection eye changes private visibility`);await overlayPixels(visible);
      assert.equal(await evaluate('layerApp.state().document_file.modified'),false,'Private overlay visibility retains the clean artwork checkpoint');
      await installSave();await invoke('save_document_as');await ready();assert.deepEqual(authoredIdentity(await overlayArchive()),overlayOriginal,'Private overlay properties and visibility leave portable artwork unchanged');
      await evaluate('layerApp.documents.autosave()');const origin=await evaluate('performance.timeOrigin');await call('Page.reload',{ignoreCache:true});await wait(`performance.timeOrigin!==${origin}&&window.layerApp?.app.brush_ready()`);await ready();
      assert.deepEqual(await overlayProperties(),before,`${theme}: private restart retains overlay color and opacity`);assert.equal(await overlayVisible(),visible,`${theme}: private restart retains eye visibility`);
      assert.equal(await evaluate('layerApp.state().document_file.modified'),false,'Restored private overlay settings retain the clean artwork checkpoint');await overlayPixels(visible);
    }
  }
  console.log('PASS saved selection overlay: native eye visibility, immediate color/opacity redraw, clean unchanged portable artwork and automatic restore pixels in both themes');
  await evaluate('layerApp.documents.autosave()');
  const injection=await call('Page.addScriptToEvaluateOnNewDocument',{source:`(()=>{let current;Object.defineProperty(window,'layerApp',{configurable:true,get:()=>current,set:value=>{current=value;const prepare=value.app.prepare_session_restart.bind(value.app);let entered=false;value.app.prepare_session_restart=(...args)=>{const pending=prepare(...args);if(!entered){entered=true;window.liveStartupLayers=value.state().layers.length+1;value.dispatch({type:'invoke',command:'add_layer'});}return pending;};}});})()`});
  try {
    const old=await evaluate('performance.timeOrigin');await call('Page.reload');await wait(`performance.timeOrigin!==${old}&&window.layerApp?.app.brush_ready()`);await ready();
    const resumed=await tabs();assert.equal(resumed.tabs.length,3,'Input during restore keeps its live drawing beside restored drawings');
    assert.ok(![third,second].includes(resumed.selected),'Restore preserves the active startup drawing after input');
    assert.equal(await evaluate('layerApp.state().layers.length===liveStartupLayers&&layerApp.state().document_file.modified'),true,'Startup edits remain present and unsaved');
    assert.deepEqual(resumed.tabs.filter(tab=>[third,second].includes(tab.id)).map(tab=>tab.id),[third,second],'Restored drawings retain their original relative order');
  } finally {await call('Page.removeScriptToEvaluateOnNewDocument',{identifier:injection.identifier});}
  await trace.check('Fresh renderer resume and drawing recovery');
  console.log('PASS seamless restart in both themes: clean/dirty tabs, order, active drawing, camera, undo/redo, save checkpoint, cancelled close and durable discard; zero immediate page/worker pipelines');
  } finally {await trace.close().catch(error=>console.error('Recovery pipeline trace cleanup:',error));}
}

export async function checkDrawingTabs({call,evaluate,settle}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{
    const end=performance.now()+60000;
    const details=()=>{const safe=read=>{try{return read()}catch(error){return String(error)}};return JSON.stringify({status:document.querySelector('#status')?.textContent,gpuNotice:document.querySelector('#gpu-notice')?.textContent,busy:layerApp.documents.busy(),brushReady:layerApp.app.brush_ready(),parkReady:safe(()=>layerApp.app.document_park_ready()),gpuReady:layerApp.app.gpu_ready(),file:layerApp.state().document_file,requests:layerApp.state().requests,tabs:layerApp.app.document_tabs(1000),startup:layerApp.startupTimes,transition:window.switchTransition&&{pending:switchTransition.pending.length,presentations:switchTransition.presentations,error:switchTransition.error},stats:safe(()=>layerApp.app.renderer_stats()),dialogs:[...document.querySelectorAll('dialog[open]')].map(n=>n.textContent)},(_,v)=>typeof v==='bigint'?String(v):v);};
    function poll(){try{if(${condition})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}+': '+details()));else setTimeout(poll,30);}catch(error){reject(Error(String(error)+': '+details()));}}
    poll();
  })`);
  const tabs=()=>evaluate(`JSON.parse(JSON.stringify(layerApp.app.document_tabs(1000),(_,v)=>typeof v==='bigint'?Number(v):v))`);
  const invoke=command=>evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);
  const ready=()=>wait(`!layerApp.documents.busy()&&layerApp.app.brush_ready()&&!layerApp.state().document_file.busy&&layerApp.app.document_park_ready()`);
  const launchLanguage=await evaluate('layerApp.app.language_tag()');
  const assertLanguage=async()=>assert.equal(await evaluate('layerApp.app.language_tag()'),launchLanguage,'Document replacement retains the launch context');
  const select=async id=>{await evaluate(`layerApp.documents.select(BigInt(${id}))`);await ready();await settle();await assertLanguage();
    assert.equal(await evaluate(`document.querySelector('button[data-command="new_document"]')?.disabled??false`),false,'Reactivation republishes enabled native controls');};
  const create=async()=>{
    await ready();const count=(await tabs()).tabs.length;await invoke('new_document');
    await evaluate(createDrawing());
    await wait(`layerApp.app.document_tabs(0).tabs.length===${count+1}`);await ready();await assertLanguage();return (await tabs()).selected;
  };
  await ready();const first=(await tabs()).selected;
  assert.equal((await tabs()).tabs.length,1);
  const initialLayers=await evaluate('layerApp.state().layers.length');
  await invoke('add_layer');await wait(`layerApp.state().layers.length===${initialLayers+1}`);await ready();
  const firstLayers=await evaluate('layerApp.state().layers.length');
  assert.equal((await tabs()).tabs[0].modified,true);
  const second=await create();
  assert.equal((await tabs()).tabs.length,2,'New appends without asking to discard the first drawing');
  assert.equal(await evaluate('layerApp.state().layers.length'),firstLayers-1);
  const beforeSecondLayers=await evaluate('layerApp.state().layers.length');
  await invoke('pencil');await invoke('add_layer');await wait(`layerApp.state().layers.length===${beforeSecondLayers+1}`);await ready();
  const secondLayers=await evaluate('layerApp.state().layers.length');
  await select(first);
  assert.equal(await evaluate('layerApp.state().layers.length'),firstLayers);
  await invoke('undo');await wait(`layerApp.state().layers.length===${firstLayers-1}`);
  assert.equal(await evaluate('layerApp.state().layers.length'),firstLayers-1,'First tab retains its own undo');
  await select(second);
  assert.equal(await evaluate('layerApp.state().layers.length'),secondLayers,'Other tab was not undone');
  const third=await create();
  await evaluate(`layerApp.app.reorder_document(BigInt(${third}),BigInt(${first}));layerApp.documents.refresh()`);
  assert.deepEqual((await tabs()).tabs.map(t=>t.id),[third,first,second]);
  await evaluate('layerApp.app.document_order_history(false);layerApp.documents.refresh()');
  assert.deepEqual((await tabs()).tabs.map(t=>t.id),[first,second,third]);
  assert.equal((await tabs()).selected,third,'Order history does not switch drawings');
  const press=async(key,code,vk,modifiers=0)=>{
    for(const type of ['rawKeyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode:vk,nativeVirtualKeyCode:vk,modifiers});
    await settle();
  };
  for(const theme of ['light','dark']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);
    for(const fallback of [false,true]){
      await evaluate(`window.focusedDrawingTab=document.querySelector('.drawing-tab[data-drawing-id="${third}"] .drawing-tab-pick');focusedDrawingTab.focus();${fallback?"document.querySelector('.drawing-tabs').moveBefore=undefined":""}`);
      await press('ArrowLeft','ArrowLeft',37,10);
      assert.deepEqual((await tabs()).tabs.map(t=>t.id),[first,third,second]);
      assert.equal(await evaluate('focusedDrawingTab===document.activeElement'),true,`${theme} fallback=${fallback}: moving a drawing earlier with the keyboard keeps focus`);
      await evaluate('layerApp.app.document_order_history(false);layerApp.documents.refresh()');await settle();
      assert.deepEqual((await tabs()).tabs.map(t=>t.id),[first,second,third]);
      await evaluate("delete document.querySelector('.drawing-tabs').moveBefore");
    }
    for(const [key,code,vk,target] of [['PageUp','PageUp',33,second],['PageDown','PageDown',34,third]]) {
      await invoke('hand');await settle();await evaluate('layerApp.canvas.focus()');
      await press(key,code,vk,1);
      await wait(`Number(layerApp.app.document_tabs(0).selected)===${target}`);await ready();
    }
  }
  await invoke('keyboard_shortcuts');await settle();
  await evaluate("layerApp.dispatch({type:'preferences',action:{type:'begin_shortcut',id:'command.NextDrawing'}})");await settle();
  await press('F8','F8',119);
  await evaluate("layerApp.dispatch({type:'preferences',action:{type:'confirm_shortcut',replace:true}});layerApp.dispatch({type:'close_settings'})");await settle();
  await select(second);await invoke('hand');await settle();await evaluate('layerApp.canvas.focus()');
  await press('F8','F8',119);
  await wait(`Number(layerApp.app.document_tabs(0).selected)===${third}`);await ready();
  await invoke('keyboard_shortcuts');await settle();
  await evaluate("layerApp.dispatch({type:'preferences',action:{type:'reset_shortcut',id:'command.NextDrawing'}});layerApp.dispatch({type:'close_settings'})");await settle();
  const compact=await evaluate('document.querySelector(".drawing-tabs").hidden');
  if(compact){await evaluate('layerApp.documents.showSelector()');await settle();}
  for(const theme of ['light','dark'])for(const device of ['mouse','pen','touch']){
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
    const positions=await evaluate(`(()=>{const list=document.querySelector(${JSON.stringify(compact?'.drawing-list-rows':'.drawing-tabs')}),rows=[...list.children];const source=rows[0].querySelector(${JSON.stringify(compact?'.drawing-grip':'.drawing-tab-pick')}).getBoundingClientRect(),target=rows.at(-1).getBoundingClientRect();return{from:[source.x+source.width/2,source.y+source.height/2],to:[target.x+target.width-${compact?'target.width/2':'8'},target.y+target.height-${compact?'8':'target.height/2'}]};})()`);
    const [x,y]=positions.from,[tx,ty]=positions.to;
    const send=async(type,px,py)=>{
      if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:12,x:px,y:py,radiusX:3,radiusY:3,force:1}]});
      else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],x:px,y:py,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device,force:type==='up'?0:.7});
      await settle();
    };
    const slide=()=>evaluate(`(()=>{const o=document.querySelector('.drawing-slide');return o&&{hidden:[...document.querySelectorAll('.drawing-tabs > .drawing-tab')].every(n=>getComputedStyle(n).opacity==='0'),offsets:[...o.children].map(n=>parseFloat(n.style.transform.slice(11)))};})()`);
    const near=(actual,expected,message)=>assert.ok(actual.length===expected.length&&actual.every((v,i)=>Math.abs(v-expected[i])<1),`${message}: ${actual} vs ${expected}`);
    await send('down',x,y);
    if(!compact){
      const pitch=await evaluate(`(()=>{const [a,b]=document.querySelectorAll('.drawing-tabs > .drawing-tab');return b.getBoundingClientRect().x-a.getBoundingClientRect().x;})()`);
      const height=await evaluate(`document.querySelector('.drawing-tabs').getBoundingClientRect().height`);
      await send('move',x+pitch*.9,y);
      let live=await slide();assert.equal(live?.hidden,true,`${device} hides live tabs while sliding`);
      near(live.offsets,[pitch*.9,-pitch,0],`${device} passes one neighbor`);
      await send('move',tx,ty);near((await slide()).offsets,[pitch*2,-pitch,-pitch],`${device} clamps to the strip and passes both neighbors`);
      await send('move',tx,ty+height*2);near((await slide()).offsets,[0,0,0],`${device} leaving the strip detaches`);
      await send('move',tx,ty);near((await slide()).offsets,[pitch*2,-pitch,-pitch],`${device} returning reattaches`);
    }else await send('move',tx,ty);
    await send('up',tx,ty);
    assert.equal(await evaluate('!!document.querySelector(".drawing-slide,.dragged-tab-source")'),false,'Release removes the slide');
    assert.deepEqual((await tabs()).tabs.map(t=>t.id),[second,third,first],`${device} reorders ${compact?'an explicit list handle':'the tab body'} without a hold`);
    assert.equal((await tabs()).selected,third,'Reordering does not activate the dragged drawing');
    await evaluate('layerApp.app.document_order_history(false);layerApp.documents.refresh()');await settle();
    assert.deepEqual((await tabs()).tabs.map(t=>t.id),[first,second,third]);
    if(!compact){
      const reverse=await evaluate(`(()=>{window.focusedDrawingTab=document.querySelector('.drawing-tab[data-drawing-id="${third}"] .drawing-tab-pick');const a=focusedDrawingTab.getBoundingClientRect(),b=document.querySelector('.drawing-tabs').firstElementChild.getBoundingClientRect();return{x:a.x+a.width/2,y:a.y+a.height/2,to:b.x+8}})()`);
      await send('down',reverse.x,reverse.y);await send('move',reverse.to,reverse.y);await send('up',reverse.to,reverse.y);
      assert.deepEqual((await tabs()).tabs.map(t=>t.id),[third,first,second],`${theme}/${device}: leftward drawing drop commits`);
      assert.equal(await evaluate('focusedDrawingTab===document.activeElement'),true,`${theme}/${device}: leftward drawing drop keeps focus`);
      await evaluate('layerApp.app.document_order_history(false);layerApp.documents.refresh()');await settle();
      await send('down',x,y);await send('move',tx,ty);await send('move',tx,ty+80);await send('up',tx,ty+80);
      assert.equal(await evaluate('!!document.querySelector(".drawing-slide,.dragged-tab-source")'),false);
      assert.deepEqual((await tabs()).tabs.map(t=>t.id),[first,second,third],`${device} release outside the strip cancels`);
      assert.equal((await tabs()).can_undo,false);
    }
  }
  if(compact)await evaluate(`document.querySelector('.drawing-list header button').click()`);
  // Selector bodies preserve pre-hold scrolling, then keep the same touch/pen
  // contact across their contextual actions. Handles were tested above.
  await evaluate('layerApp.documents.showSelector()');await settle();
  for(const device of ['mouse','pen','touch']){
    const order=[first,second,third];
    const positions=()=>evaluate(`(()=>{const rows=[...document.querySelectorAll('.drawing-list-row')],a=rows[0].querySelector('.drawing-list-pick').getBoundingClientRect(),b=rows.at(-1).getBoundingClientRect();return{from:{x:a.x+a.width/2,y:a.y+a.height/2},to:{x:a.x+a.width/2,y:b.bottom-8}}})()`);
    let p=await positions();
    const send=async(type,point=p.from)=>{
      if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd',cancel:'touchCancel'}[type],touchPoints:['up','cancel'].includes(type)?[]:[{id:18,...point,radiusX:3,radiusY:3,force:1}]});
      else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...point,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device,force:type==='up'?0:.7});
      await settle();
    };
    if(device!=='mouse'){
      await send('down');await send('move',{x:p.from.x,y:p.from.y-20});await send('up',{x:p.from.x,y:p.from.y-20});
      await evaluate('new Promise(r=>setTimeout(r,350))');
      assert.deepEqual((await tabs()).tabs.map(t=>t.id),order,`${device} pre-hold movement does not reorder`);
      assert.equal((await tabs()).selected,third,`${device} scrolling does not select the row`);
    }
    p=await positions();await send('down');await evaluate('new Promise(r=>setTimeout(r,600))');
    assert.equal(await evaluate('!!document.querySelector(".drawing-row-menu")'),device!=='mouse',`${device} hold menu policy`);
    if(device!=='mouse'){
      await send('up');await evaluate('new Promise(r=>setTimeout(r,350))');
      assert.equal(await evaluate('!!document.querySelector(".drawing-row-menu")'),true,'Release retains held actions');
      assert.equal((await tabs()).selected,third,'Holding an inactive row does not select it');
      await send('down');await evaluate('new Promise(r=>setTimeout(r,600))');
    }
    await send('move',p.to);
    assert.equal(await evaluate('!!document.querySelector(".drawing-row-menu")'),false,'Dragging dismisses held actions');
    if(device==='pen')await evaluate(`document.querySelector('.drawing-list-pick').dispatchEvent(new PointerEvent('contextmenu',{bubbles:true,cancelable:true,pointerType:'pen'}))`);
    assert.equal(await evaluate('!!document.querySelector(".drawing-row-menu")'),false,'A native context event cannot reopen actions during a drag');
    await send('up',p.to);await evaluate('new Promise(r=>setTimeout(r,350))');
    assert.deepEqual((await tabs()).tabs.map(t=>t.id),[second,third,first],`${device} row drag after hold`);
    assert.equal((await tabs()).selected,third);
    await evaluate('layerApp.app.document_order_history(false);layerApp.documents.refresh()');await settle();
  }
  await evaluate(`document.querySelector('.drawing-list header button').click()`);
  await evaluate(`layerApp.documents.close(BigInt(${first}))`);await ready();
  // First is clean after Undo, so it closes directly and picks its right neighbor.
  await wait(`layerApp.app.document_tabs(0).tabs.length===2`);await ready();
  assert.equal((await tabs()).selected,second);
  await evaluate(`layerApp.documents.close(BigInt(${second}))`);
  await wait(`!![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')`);
  await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Cancel').click()`);await ready();
  assert.equal((await tabs()).tabs.length,2,'Cancel keeps the selected background-close target');
  await evaluate(`layerApp.documents.close(BigInt(${second}))`);
  await wait(`!![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')`);
  await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes').click()`);
  await wait(`layerApp.app.document_tabs(0).tabs.length===1`);await ready();
  assert.equal((await tabs()).selected,third);
  await invoke('drawings');await wait(`!!document.querySelector('.drawing-list[open]')`);
  await evaluate(`document.querySelector('.drawing-list header button').click()`);
  await evaluate(`layerApp.documents.close(BigInt(${third}))`);
  await wait(`Number(layerApp.app.document_tabs(0).selected)!==${third}`);await ready();
  assert.equal((await tabs()).tabs.length,1,'Closing the last tab leaves a fresh browser drawing');
  assert.equal((await tabs()).tabs[0].modified,false);
  // Force the real OPFS path using a redo-only raster owner. A project-only
  // cache would lose this stroke even though the currently visible page is blank.
  const painted=(await tabs()).selected;
  await evaluate(`layerApp.app.set_document_cache_budget(0);window.tabFiles=new Map();window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){let bytes;return{async write(value){bytes=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},async close(){tabFiles.set(options.suggestedName,bytes)},async abort(){}}}});`);
  await invoke('fit_canvas');await ready();
  const point=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()`);
  for(const[type,dx,buttons]of[['mousePressed',0,1],['mouseMoved',75,1],['mouseReleased',75,0]]){await call('Input.dispatchMouseEvent',{type,x:point.x+dx,y:point.y,button:'left',buttons,clickCount:1,pointerType:'pen',force:buttons?.65:0});await settle();}
  await wait('layerApp.state().document_file.modified');await ready();
  await invoke('save_document_as');await ready();
  const tabExpected = packageResourceIdentity(await readPackage(evaluate, '[...tabFiles.values()][0]'));
  assert.ok(tabExpected.length>0);
  await invoke('undo');await ready();
  const neighbor=await create();
  await wait('Number(layerApp.app.document_tabs(0).resident_bytes)===0');
  assert.equal((await tabs()).resident_bytes,0,'Inactive redo payloads leave RAM after successful OPFS write');
  assert.ok(await evaluate(`(async()=>{let n=0;const root=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-live-tiles');for await(const dir of root.values())for await(const file of dir.values())if(file.kind==='file')n++;return n;})()`),'Private immutable chunks exist');
  await select(painted);await invoke('redo');await ready();
  await invoke('save_document_as');await ready();
  assert.deepEqual(packageResourceIdentity(await readPackage(evaluate, '[...tabFiles.values()].at(-1)')),tabExpected,'Redo from disk retains exact compressed tile identity');
  await select(neighbor);
  await wait('Number(layerApp.app.document_tabs(0).resident_bytes)===0');
  assert.equal((await tabs()).resident_bytes,0,'A reread cache is evicted on the next switch');
  const beforeCorrupt=(await tabs()).tabs.map(t=>t.id);
  assert.equal(await evaluate(`layerApp.documents.openFiles([new File(['invalid'],'broken.capy')]).then(()=>false,error=>{window.tabOpenDiagnostic=error.message??String(error);return true;})`),true);
  assert.deepEqual((await tabs()).tabs.map(t=>t.id),beforeCorrupt,'Corrupt opening cannot remove or replace a drawing');
  const reported=await evaluate(`(async()=>{const original=console.error,reported=[];console.error=(...args)=>{if(args.length===1&&args[0] instanceof Error&&args[0].message===tabOpenDiagnostic)reported.push(args[0].message);else original(...args);};try{await layerApp.documents.openFiles([new File([[...tabFiles.values()][0]],'duplicate.capy'),new File(['invalid'],'broken-middle.capy'),new File([[...tabFiles.values()][0]],'duplicate.capy')]);return reported;}finally{console.error=original;}})()`);await ready();
  assert.deepEqual(reported,[await evaluate('tabOpenDiagnostic')],'Only the deliberately corrupt middle file reports its known failure');
  assert.equal((await tabs()).tabs.length,beforeCorrupt.length+2,'A corrupt middle file does not stop later opens');
  const copies=(await tabs()).tabs.slice(-2);
  assert.equal(copies[0].title,copies[1].title);assert.notEqual(copies[0].id,copies[1].id);
  await select(copies[0].id);await invoke('add_layer');await ready();
  const changedLayers=await evaluate('layerApp.state().layers.length');
  await select(copies[1].id);
  assert.equal(await evaluate('layerApp.state().layers.length'),changedLayers-1,'Duplicate opens are independent editors');
  const dropFiles=(target,names)=>evaluate(`(async()=>{const target=${target},bounds=target.getBoundingClientRect(),transfer=new DataTransfer();
    for(const name of ${JSON.stringify(names)}){
      if(name.endsWith('.capy')){transfer.items.add(new File([[...tabFiles.values()][0]],name));continue;}
      const pixels=new OffscreenCanvas(12,8),context=pixels.getContext('2d');context.fillStyle='#c84';context.fillRect(0,0,12,8);
      transfer.items.add(new File([await pixels.convertToBlob({type:'image/png'})],name,{type:'image/png'}));
    }
    const init={dataTransfer:transfer,bubbles:true,cancelable:true,clientX:bounds.x+bounds.width/2,clientY:bounds.y+bounds.height/2};
    target.dispatchEvent(new DragEvent('dragover',init));target.dispatchEvent(new DragEvent('drop',init));})()`);
  const dropped=async(target,name)=>{
    const count=(await tabs()).tabs.length;await dropFiles(target,[name]);
    await wait(`layerApp.app.document_tabs(0).tabs.length===${count+1}`);await ready();
    const tab=(await tabs()).tabs.at(-1);
    assert.ok(tab.title.startsWith(name.replace(/\.\w+$/,'')),`${name} opens as its own drawing: ${tab.title}`);
    assert.equal((await tabs()).selected,tab.id,`${name} selects its new tab`);
  };
  await dropped('layerApp.canvas','dropped.capy');
  const beforeBatch=(await tabs()).tabs.length;
  await dropFiles('layerApp.canvas',['first.capy','second.capy']);await settle();await ready();
  assert.equal(await evaluate(`document.querySelector('#status').textContent`),'Open drawings one at a time; drop a batch containing only images to add layers.','Several drawings dropped on the canvas give the shared refusal');
  assert.equal((await tabs()).tabs.length,beforeBatch,'A refused canvas drop opens nothing');
  await dropped(`document.querySelector('#drawing-title')`,'dropped-photo.png');
  await select(copies[0].id);
  const beforeFailedSave=(await tabs()).tabs.length;
  await evaluate(`window.tabSavePicker=window.showSaveFilePicker;window.showSaveFilePicker=async()=>({name:'failed.capy',async createWritable(){throw Error('Test write failure');}});layerApp.documents.close(BigInt(${copies[0].id}))`);
  await wait(`!![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Save')`);
  await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Save').click()`);await ready();
  assert.equal((await tabs()).tabs.length,beforeFailedSave,'Failed Save does not finish Close');
  assert.equal((await tabs()).tabs.find(t=>t.id===copies[0].id).modified,true);
  await evaluate('window.showSaveFilePicker=window.tabSavePicker');
  // A storage failure must keep RAM and navigation usable while refusing more
  // admissions. Restore the injected quota failure before the next checkpoint.
  await evaluate(`window.tabCreateWritable=FileSystemFileHandle.prototype.createWritable;FileSystemFileHandle.prototype.createWritable=async function(...args){if(/^\\d+$/.test(this.name))throw new DOMException('Test cache full','QuotaExceededError');return tabCreateWritable.apply(this,args);};`);
  let failedStorageTab;
  try{
    // The selected duplicate was restored from disk. A new pen contact creates
    // a fresh immutable RAM payload that needs a real write when it is parked.
    const p=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()`);
    for(const[type,dy,buttons]of[['mousePressed',0,1],['mouseMoved',25,1],['mouseReleased',25,0]]){await call('Input.dispatchMouseEvent',{type,x:p.x,y:p.y+dy,button:'left',buttons,clickCount:1,pointerType:'pen',force:buttons?.7:0});await settle();}
    await ready();failedStorageTab=await create();
    await wait('!!layerApp.app.document_tabs(0).storage_error');
    assert.ok((await tabs()).storage_error);assert.ok((await tabs()).resident_bytes>0);
    const count=(await tabs()).tabs.length;await invoke('new_document');
    await evaluate(createDrawing());await ready();
    assert.equal((await tabs()).tabs.length,count,'Cache write failure refuses another open');
    await select(copies[0].id);
    assert.equal(await evaluate('layerApp.state().layers.length'),changedLayers,'Navigation after a disk failure preserves the editor');
  }finally{await evaluate('FileSystemFileHandle.prototype.createWritable=window.tabCreateWritable');}
  await select(failedStorageTab);await evaluate('layerApp.documents.autosave()');
  assert.equal((await tabs()).storage_error??null,null);
  assert.equal((await tabs()).resident_bytes,0,'Storage retries successfully after quota failure is removed');
  const transitionDirectory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/document-tabs/web';
  await mkdir(transitionDirectory,{recursive:true});
  const closeColor=(a,b)=>a.every((value,i)=>Math.abs(value-b[i])<=3);
  for(const theme of ['light','dark']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}});layerApp.dispatch({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}})`);
    let clip;
    const shots=[];
    const capture=async phase=>{
      const {data}=await call('Page.captureScreenshot',{format:'png',clip});
      const sample=await evaluate(`(async()=>{
        const image=new Image();image.src='data:image/png;base64,'+${JSON.stringify(data)};await image.decode();
        const canvas=document.createElement('canvas');canvas.width=image.width;canvas.height=image.height;
        const context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(image,0,0);
        const pixels=context.getImageData(Math.floor(image.width/4)-16,Math.floor(image.height/2)-16,32,32).data;
        const low=[255,255,255],high=[0,0,0];
        for(let i=0;i<pixels.length;i+=4)for(let k=0;k<3;k++){low[k]=Math.min(low[k],pixels[i+k]);high[k]=Math.max(high[k],pixels[i+k]);}
        return{rgb:low,spread:Math.max(...high.map((value,i)=>value-low[i]))};
      })()`);
      const shot={phase,data,at:performance.now(),...sample};if(phase)shots.push(shot);return shot;
    };
    const openSolid=async(color,name,expected)=>{
      await evaluate(`(async()=>{
        const pixels=new OffscreenCanvas(512,256),context=pixels.getContext('2d');context.fillStyle=${JSON.stringify(color)};context.fillRect(0,0,512,256);
        await layerApp.documents.openFiles([new File([await pixels.convertToBlob({type:'image/png'})],${JSON.stringify(name+'.png')},{type:'image/png'})]);
      })()`);
      await ready();await invoke('fit_canvas');await evaluate("layerApp.dispatch({type:'set_zoom',zoom:.75})");
      await wait('!layerApp.app.canvas_work_pending()');await evaluate('layerApp.app.wait_for_canvas()');
      clip=await evaluate(`(()=>{const camera=layerApp.app.camera(),bounds=layerApp.canvas.getBoundingClientRect();return{x:Math.round(bounds.x+camera.translation[0]*bounds.width/camera.viewport[0])+2,y:Math.round(bounds.y+camera.translation[1]*bounds.height/camera.viewport[1])+2,width:Math.floor(512*camera.zoom*bounds.width/camera.viewport[0])-4,height:Math.floor(256*camera.zoom*bounds.height/camera.viewport[1])-4,scale:1}})()`);
      const deadline=performance.now()+60000;let shot;
      do{shot=await capture(null);assert.ok(performance.now()<deadline,`${theme} ${name}: opaque reference reaches ${expected}, got ${shot.rgb}`);}while(!closeColor(shot.rgb,expected)||shot.spread>1);
      shots.push({...shot,phase:name});return{tab:(await tabs()).selected,rgb:shot.rgb};
    };
    const incoming=await openSolid('#0000ff',`blue-${theme}`,[0,0,255]);
    const outgoing=await openSolid('#ff0000',`red-${theme}`,[255,0,0]);
    await evaluate(`window.switchTransition={post:Worker.prototype.postMessage,texture:GPUCanvasContext.prototype.getCurrentTexture,frame:layerApp.app.frame,pending:[],presentations:0,incoming:false};
      Worker.prototype.postMessage=function(message,...args){if(message.request?.operation==='image-decode')switchTransition.pending.push(()=>switchTransition.post.call(this,message,...args));else return switchTransition.post.call(this,message,...args)};
      layerApp.app.frame=function(...args){switchTransition.incoming=String(this.document_tabs(0).selected)===String(${incoming.tab});const change=switchTransition.frame.apply(this,args);switchTransition.wake=!!change.canvas_wake;return change};
      GPUCanvasContext.prototype.getCurrentTexture=function(...args){const texture=switchTransition.texture.apply(this,args);if(this.canvas===layerApp.canvas&&switchTransition.incoming)switchTransition.presentations++;return texture};
      switchTransition.release=()=>{Worker.prototype.postMessage=switchTransition.post;for(const send of switchTransition.pending.splice(0))send()};
      switchTransition.selection=layerApp.documents.select(BigInt(${incoming.tab}));switchTransition.selection.catch(error=>switchTransition.error=String(error));`);
    const strip=[];
    try {
      await wait(`String(layerApp.app.document_tabs(0).selected)===String(${incoming.tab})&&switchTransition.pending.length>0&&switchTransition.presentations>0`);
      await evaluate('layerApp.app.wait_for_canvas()');await settle();
      for(let i=0;i<2;i++){
        const shot=await capture(`held-${i}`);strip.push(shot);
        assert.ok(shot.spread<=1&&closeColor(shot.rgb,outgoing.rgb),`${theme}: Waiting keeps the outgoing opaque drawing, got ${shot.rgb}`);
      }
      await evaluate('switchTransition.release();switchTransition.selection');
      const deadline=performance.now()+60000;
      do{
        strip.push(await capture(`fade-${strip.length-2}`));
        assert.ok(performance.now()<deadline,`${theme}: the transition reaches the incoming drawing`);
      }while(!closeColor(strip.at(-1).rgb,incoming.rgb)||await evaluate('switchTransition.wake||layerApp.app.canvas_work_pending()'));
      await evaluate('layerApp.app.wait_for_canvas()');await settle();strip.push(await capture('new'));
      assert.ok(closeColor(strip.at(-1).rgb,incoming.rgb),`${theme}: the completed fade shows the incoming drawing`);
      const delta=incoming.rgb.map((value,i)=>value-outgoing.rgb[i]),length=delta.reduce((sum,value)=>sum+value*value,0);
      let previous=0,mixed=false;
      for(const shot of strip){
        assert.ok(shot.spread<=1,`${theme} ${shot.phase}: opaque transition has no checkerboard`);
        assert.ok(shot.rgb.every((value,i)=>value>=Math.min(outgoing.rgb[i],incoming.rgb[i])-3&&value<=Math.max(outgoing.rgb[i],incoming.rgb[i])+3),`${theme} ${shot.phase}: no surround or unrelated pixels: ${shot.rgb}`);
        const blend=convert=>{
          const weights=[0,2].map(i=>(convert(shot.rgb[i])-convert(outgoing.rgb[i]))/(convert(incoming.rgb[i])-convert(outgoing.rgb[i])));
          return Math.abs(weights[0]-weights[1])<=.05;
        };
        const linear=value=>{value/=255;return value<=.04045?value/12.92:((value+.055)/1.055)**2.4};
        assert.ok(blend(value=>value)||blend(linear),`${theme} ${shot.phase}: pixels blend the two drawings rather than a cleared target: ${shot.rgb}`);
        const progress=shot.rgb.reduce((sum,value,i)=>sum+(value-outgoing.rgb[i])*delta[i],0)/length;
        assert.ok(progress>=previous-.03,`${theme}: transition progresses from old through mixed to new`);previous=progress;
        mixed ||= Math.max(...shot.rgb.map((value,i)=>Math.abs(value-outgoing.rgb[i])))>8&&Math.max(...shot.rgb.map((value,i)=>Math.abs(value-incoming.rgb[i])))>8;
      }
      assert.ok(mixed,`${theme}: the 150 ms fade contains a genuinely mixed presented frame`);
      console.log(`PASS ${theme} switch transition: real decode held outgoing pixels, ${strip.length} captured frames old→mixed→new, no surround/checkerboard`);
    } finally {
      await evaluate('switchTransition.release();layerApp.app.frame=switchTransition.frame;GPUCanvasContext.prototype.getCurrentTexture=switchTransition.texture;delete window.switchTransition');
      await Promise.all(shots.map(shot=>writeFile(`${transitionDirectory}/switch-transition-${theme}-${shot.phase}.png`,Buffer.from(shot.data,'base64'))));
      await writeFile(`${transitionDirectory}/switch-transition-${theme}.json`,JSON.stringify(strip.map(({phase,at,rgb,spread})=>({phase,ms:at-strip[0].at,rgb,spread})),null,2));
      if(strip.length){
        const sheet=await evaluate(`(async()=>{
          const images=await Promise.all(${JSON.stringify(strip.map(shot=>shot.data))}.map(async data=>{const image=new Image();image.src='data:image/png;base64,'+data;await image.decode();return image}));
          const width=192,height=Math.round(images[0].height*width/images[0].width),columns=Math.min(4,images.length),canvas=document.createElement('canvas');canvas.width=columns*width;canvas.height=Math.ceil(images.length/columns)*(height+20);
          const context=canvas.getContext('2d');context.fillStyle='#171717';context.fillRect(0,0,canvas.width,canvas.height);context.fillStyle='white';context.font='12px sans-serif';
          images.forEach((image,i)=>{const x=i%columns*width,y=Math.floor(i/columns)*(height+20);context.drawImage(image,x,y,width,height);context.fillText(${JSON.stringify(strip.map(shot=>shot.phase))}[i],x+4,y+height+14)});return canvas.toDataURL('image/png').split(',')[1];
        })()`);
        await writeFile(`${transitionDirectory}/switch-transition-${theme}-strip.png`,Buffer.from(sheet,'base64'));
      }
    }
  }
  const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${transitionDirectory}/lifecycle.png`,Buffer.from(shot.data,'base64'));
  console.log('PASS drawing tabs: independent sessions/history, all pointer reorder, close decisions, selector, final drawing, OPFS redo-only exact storage, corrupt/duplicate opens, canvas and tab file drops, failed save, quota failure/retry');
}
