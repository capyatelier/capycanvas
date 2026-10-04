import {readPackage,packageResourceIdentity} from './package-fixture.test.mjs';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkDrawingTabRecovery({call,evaluate,settle}) {
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
    await wait(`!![...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Create')`);
    await evaluate(`(()=>{const form=document.querySelector('dialog[open]');for(const input of form.querySelectorAll('input[type=number]'))input.value=96;[...form.querySelectorAll('button')].find(button=>button.textContent==='Create').click();})()`);
    await wait(`layerApp.app.document_tabs(0).tabs.length===${count+1}`);await ready();return (await tabs()).selected;
  };
  const installSave=()=>evaluate(`window.showSaveFilePicker=async options=>{const directory=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-test-session-originals',{create:true});return directory.getFileHandle(crypto.randomUUID()+'.'+options.suggestedName.split('.').at(-1),{create:true});};`);
  const snapshot=()=>evaluate(`(()=>{const state=layerApp.state(),stamp=layerApp.app.session_stamp_for(layerApp.app.document_tabs(0).selected);return JSON.parse(JSON.stringify({layers:state.layers.map(({label,selected,editing,mask_selected})=>({label,selected,editing,mask_selected})),camera:stamp.state.camera,modified:state.document_file.modified,location:state.document_file.location?.name??null,checkpoint:stamp.checkpoint,selection:state.layer_tools.has_selection,undo:state.commands.find(command=>command.id==='undo')?.enabled,redo:state.commands.find(command=>command.id==='redo')?.enabled},(_,v)=>typeof v==='bigint'?Number(v):v));})()`);
  await ready();await installSave();
  const first=(await tabs()).selected;
  const pending=await create();
  await evaluate(`window.sessionCaptureOriginal=layerApp.app.capture_tab_session.bind(layerApp.app);layerApp.app.capture_tab_session=id=>String(id)===String(${pending})?{write:async()=>{throw Error('Injected first checkpoint failure')},free(){}}:sessionCaptureOriginal(id);`);
  try {
    assert.equal(await evaluate('layerApp.documents.autosave().then(()=>false,()=>true)'),true,'First checkpoint failure is reported');
    assert.equal(await evaluate(`(async()=>{const store=(await import('./restart-store.js')).createRestartStore();for(const key of await store.windows()){const manifest=await store.manifest(key),drawing=manifest.drawings.find(drawing=>String(drawing.id)===String(${pending}));if(drawing)return !(await store.read(drawing.key));}return false;})()`),true,'First checkpoint failure retains authoritative membership for retry');
    assert.equal(await evaluate(`(()=>{const event=new Event('beforeunload',{cancelable:true});window.dispatchEvent(event);return event.defaultPrevented;})()`),true,'Pending unsaved state prevents quiet browser exit');
  } finally {await evaluate('layerApp.app.capture_tab_session=sessionCaptureOriginal');}
  await evaluate('layerApp.documents.autosave()');
  const beforeLostAck=await evaluate(`(async()=>{const store=(await import('./restart-store.js')).createRestartStore();for(const key of await store.windows()){const drawing=(await store.manifest(key)).drawings.find(drawing=>String(drawing.id)===String(${pending}));if(drawing){window.lostAckKey=drawing.key;return (await store.read(drawing.key)).current.generation;}}})()`);
  await evaluate(`layerApp.app.capture_tab_session=id=>{const capture=sessionCaptureOriginal(id);if(String(id)===String(${pending})){const write=capture.write.bind(capture);capture.write=async(...args)=>{await write(...args);throw Error('Injected lost checkpoint acknowledgement');};}return capture;};layerApp.dispatch({type:'set_zoom',zoom:.31});`);
  try {assert.equal(await evaluate('layerApp.documents.autosave().then(()=>false,()=>true)'),true,'A lost acknowledgement leaves the checkpoint pending');}
  finally {await evaluate('layerApp.app.capture_tab_session=sessionCaptureOriginal');}
  await evaluate('layerApp.documents.autosave()');
  assert.deepEqual(await evaluate(`(async()=>{const record=await (await import('./restart-store.js')).createRestartStore().read(lostAckKey);return [record.current.generation,record.previous.generation];})()`),[beforeLostAck+2,beforeLostAck],'A committed but unacknowledged checkpoint retries with a newer generation and retains the acknowledged base');
  await evaluate("window.sessionCloseOriginal=layerApp.app.prepare_document_close.bind(layerApp.app);layerApp.app.prepare_document_close=()=>{throw Error('Injected close preflight failure')};");
  try {
    await evaluate(`layerApp.documents.close(BigInt(${pending}))`);await ready();
    assert.equal((await tabs()).tabs.length,2,'Failed close preflight retains the live drawing');
    assert.equal(await evaluate(`(async()=>{const store=(await import('./restart-store.js')).createRestartStore();for(const key of await store.windows()){const manifest=await store.manifest(key),drawing=manifest.drawings.find(drawing=>String(drawing.id)===String(${pending}));if(drawing)return !!(await store.read(drawing.key));}return false;})()`),true,'Failed close preflight retains durable membership and its drawing');
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
    assert.equal(await evaluate(`(()=>{const event=new Event('beforeunload',{cancelable:true});window.dispatchEvent(event);return event.defaultPrevented;})()`),false,'A complete private checkpoint permits a seamless browser restart');
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
        const beforeInvalid=await evaluate(`(async()=>{const store=(await import('./restart-store.js')).createRestartStore();for(const window of await store.windows()){const drawing=(await store.manifest(window)).drawings.find(drawing=>String(drawing.id)===String(${id}));if(drawing){globalThis.invalidExportKey=drawing.key;const record=await store.read(drawing.key);return [record.current.generation,record.previous?.generation??null];}}})()`);
        await evaluate(`globalThis.invalidExportPost=Worker.prototype.postMessage;Worker.prototype.postMessage=function(value,...args){if(value.request?.operation==='restart-begin'){const request=JSON.parse(value.request.metadata);if(request.key===invalidExportKey){const envelope=JSON.parse(request.project);envelope.descriptor.metadata.last_export.recipe.jpeg_quality=0;request.project=JSON.stringify(envelope);value.request.metadata=JSON.stringify(request);}}return invalidExportPost.call(this,value,...args);};layerApp.dispatch({type:'set_zoom',zoom:.43});`);
        try {
          assert.equal(await evaluate('layerApp.documents.autosave().then(()=>false,()=>true)'),true,'The worker rejects malformed export metadata before publication');
          assert.deepEqual(await evaluate("(async()=>{const record=await (await import('./restart-store.js')).createRestartStore().read(invalidExportKey);return [record.current.generation,record.previous?.generation??null];})()"),beforeInvalid,'Rejected export metadata retains both complete checkpoint generations');
        } finally {await evaluate('Worker.prototype.postMessage=invalidExportPost');}
        await evaluate('layerApp.documents.autosave()');
      }
      if(kind!=='intact')await evaluate(`(async()=>{const directory=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-test-session-originals'),name=layerApp.state().document_file.location.name;if(${JSON.stringify(kind)}==='missing')await directory.removeEntry(name);else{const stream=await(await directory.getFileHandle(name)).createWritable();await stream.write(new Uint8Array([42]));await stream.close();}})()`);
    }
    await evaluate('layerApp.documents.autosave()');const origin=await evaluate('performance.timeOrigin');await call('Page.reload');await wait(`performance.timeOrigin!==${origin}&&window.layerApp?.app.brush_ready()`);await ready();
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
  await evaluate('layerApp.documents.autosave()');const old=await evaluate('performance.timeOrigin');await call('Page.reload');await wait(`performance.timeOrigin!==${old}&&window.layerApp?.app.brush_ready()`);await ready();
  assert.deepEqual((await tabs()).tabs.map(tab=>tab.id),[third,second],'An acknowledged close cannot resurrect on restart');
  await evaluate('layerApp.documents.autosave()');
  const injection=await call('Page.addScriptToEvaluateOnNewDocument',{source:`(()=>{let current;Object.defineProperty(window,'layerApp',{configurable:true,get:()=>current,set:value=>{current=value;const prepare=value.app.prepare_session_restart.bind(value.app);let entered=false;value.app.prepare_session_restart=(...args)=>{const pending=prepare(...args);if(!entered){entered=true;window.liveStartupLayers=value.state().layers.length+1;value.dispatch({type:'invoke',command:'add_layer'});}return pending;};}});})()`});
  try {
    const old=await evaluate('performance.timeOrigin');await call('Page.reload');await wait(`performance.timeOrigin!==${old}&&window.layerApp?.app.brush_ready()`);await ready();
    const resumed=await tabs();assert.equal(resumed.tabs.length,3,'Input during restore keeps its live drawing beside restored drawings');
    assert.ok(![third,second].includes(resumed.selected),'Restore preserves the active startup drawing after input');
    assert.equal(await evaluate('layerApp.state().layers.length===liveStartupLayers&&layerApp.state().document_file.modified'),true,'Startup edits remain present and unsaved');
    assert.deepEqual(resumed.tabs.filter(tab=>[third,second].includes(tab.id)).map(tab=>tab.id),[third,second],'Restored drawings retain their original relative order');
  } finally {await call('Page.removeScriptToEvaluateOnNewDocument',{identifier:injection.identifier});}
  console.log('PASS seamless restart in both themes: clean/dirty tabs, order, active drawing, camera, undo/redo, save checkpoint, cancelled close and durable discard');
}

export async function checkDrawingTabs({call,evaluate,settle}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{
    const end=performance.now()+60000;
    const details=()=>{const safe=read=>{try{return read()}catch(error){return String(error)}};return JSON.stringify({status:document.querySelector('#status')?.textContent,gpuNotice:document.querySelector('#gpu-notice')?.textContent,busy:layerApp.documents.busy(),brushReady:layerApp.app.brush_ready(),parkReady:safe(()=>layerApp.app.document_park_ready()),gpuReady:layerApp.app.gpu_ready(),file:layerApp.state().document_file,requests:layerApp.state().requests,tabs:layerApp.app.document_tabs(1000),startup:layerApp.startupTimes,stats:safe(()=>layerApp.app.renderer_stats()),dialogs:[...document.querySelectorAll('dialog[open]')].map(n=>n.textContent)},(_,v)=>typeof v==='bigint'?String(v):v);};
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
    await wait(`!![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create')`);
    await evaluate(`(()=>{const dialog=document.querySelector('dialog[open]');for(const n of dialog.querySelectorAll('input[type=number]'))n.value=96;[...dialog.querySelectorAll('button')].find(b=>b.textContent==='Create').click();})()`);
    await wait(`layerApp.app.document_tabs(0).tabs.length===${count+1}`);await ready();await assertLanguage();return (await tabs()).selected;
  };
  await ready();const first=(await tabs()).selected;
  assert.equal((await tabs()).tabs.length,1);
  await invoke('add_layer');await settle();
  const firstLayers=await evaluate('layerApp.state().layers.length');
  assert.equal((await tabs()).tabs[0].modified,true);
  const second=await create();
  assert.equal((await tabs()).tabs.length,2,'New appends without asking to discard the first drawing');
  assert.equal(await evaluate('layerApp.state().layers.length'),firstLayers-1);
  await invoke('pencil');await invoke('add_layer');await settle();
  const secondLayers=await evaluate('layerApp.state().layers.length');
  await select(first);
  assert.equal(await evaluate('layerApp.state().layers.length'),firstLayers);
  await invoke('undo');await settle();
  assert.equal(await evaluate('layerApp.state().layers.length'),firstLayers-1,'First tab retains its own undo');
  await select(second);
  assert.equal(await evaluate('layerApp.state().layers.length'),secondLayers,'Other tab was not undone');
  const third=await create();
  await evaluate(`layerApp.app.reorder_document(BigInt(${third}),BigInt(${first}));layerApp.documents.refresh()`);
  assert.deepEqual((await tabs()).tabs.map(t=>t.id),[third,first,second]);
  await evaluate('layerApp.app.document_order_history(false);layerApp.documents.refresh()');
  assert.deepEqual((await tabs()).tabs.map(t=>t.id),[first,second,third]);
  assert.equal((await tabs()).selected,third,'Order history does not switch drawings');
  const compact=await evaluate('document.querySelector(".drawing-tabs").hidden');
  if(compact){await evaluate('layerApp.documents.showSelector()');await settle();}
  for(const device of ['mouse','pen','touch']){
    const positions=await evaluate(`(()=>{const list=document.querySelector(${JSON.stringify(compact?'.drawing-list-rows':'.drawing-tabs')}),rows=[...list.children];const source=rows[0].querySelector(${JSON.stringify(compact?'.drawing-grip':'.drawing-tab-pick')}).getBoundingClientRect(),target=rows.at(-1).getBoundingClientRect();return{from:[source.x+source.width/2,source.y+source.height/2],to:[target.x+target.width-${compact?'target.width/2':'8'},target.y+target.height-${compact?'8':'target.height/2'}]};})()`);
    const [x,y]=positions.from,[tx,ty]=positions.to;
    const send=async(type,px,py)=>{
      if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:12,x:px,y:py,radiusX:3,radiusY:3,force:1}]});
      else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],x:px,y:py,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device,force:type==='up'?0:.7});
      await settle();
    };
    const slide=()=>evaluate(`(()=>{const o=document.querySelector('.drawing-slide');return o&&{hidden:[...document.querySelectorAll('.drawing-tabs > .drawing-tab')].every(n=>getComputedStyle(n).visibility==='hidden'),offsets:[...o.children].map(n=>parseFloat(n.style.transform.slice(11)))};})()`);
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
  assert.equal((await tabs()).resident_bytes,0,'Inactive redo payloads leave RAM after successful OPFS write');
  assert.ok(await evaluate(`(async()=>{let n=0;const root=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-live-tiles');for await(const dir of root.values())for await(const file of dir.values())if(file.kind==='file')n++;return n;})()`),'Private immutable chunks exist');
  await select(painted);await invoke('redo');await ready();
  await invoke('save_document_as');await ready();
  assert.deepEqual(packageResourceIdentity(await readPackage(evaluate, '[...tabFiles.values()].at(-1)')),tabExpected,'Redo from disk retains exact compressed tile identity');
  await select(neighbor);
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
    assert.ok((await tabs()).storage_error);assert.ok((await tabs()).resident_bytes>0);
    const count=(await tabs()).tabs.length;await invoke('new_document');
    await wait(`!![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create')`);
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create').click()`);await ready();
    assert.equal((await tabs()).tabs.length,count,'Cache write failure refuses another open');
    await select(copies[0].id);
    assert.equal(await evaluate('layerApp.state().layers.length'),changedLayers,'Navigation after a disk failure preserves the editor');
  }finally{await evaluate('FileSystemFileHandle.prototype.createWritable=window.tabCreateWritable');}
  await select(failedStorageTab);await evaluate('layerApp.documents.autosave()');
  assert.equal((await tabs()).storage_error??null,null);
  assert.equal((await tabs()).resident_bytes,0,'Storage retries successfully after quota failure is removed');
  await mkdir('artifacts/document-tabs/web',{recursive:true});
  const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile('artifacts/document-tabs/web/lifecycle.png',Buffer.from(shot.data,'base64'));
  console.log('PASS drawing tabs: independent sessions/history, all pointer reorder, close decisions, selector, final drawing, OPFS redo-only exact storage, corrupt/duplicate opens, failed save, quota failure/retry');
}
