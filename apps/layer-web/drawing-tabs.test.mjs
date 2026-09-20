import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkDrawingTabRecovery({call,evaluate,settle}) {
  // A cold full shader catalog on the Huion can outlive the usual UI timeout
  // (149 s measured). This test deliberately reloads the whole browser app;
  // ordinary tab switches are covered separately without catalog recreation.
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+240000;function poll(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,30);}poll();})`);
  const ready=()=>wait('window.layerApp?.app.brush_ready()&&!layerApp.documents.busy()&&layerApp.app.document_park_ready()');
  const invoke=command=>evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);
  await ready();
  const base=await evaluate('layerApp.state().layers.length');
  await invoke('add_layer');await ready();await evaluate("layerApp.app.save_recovery('drawing-tabs-recovery-a')");
  await invoke('add_layer');await ready();await evaluate("layerApp.app.save_recovery('drawing-tabs-recovery-b')");
  await invoke('undo');await ready();await invoke('undo');await ready();
  assert.equal(await evaluate('layerApp.state().document_file.modified'),false);
  await evaluate('layerApp.documents.autosave()');
  const previousOrigin=await evaluate('performance.timeOrigin');
  await call('Page.reload',{ignoreCache:true});
  const navigationDeadline=Date.now()+30000;
  for(;;){
    try{if(await evaluate(`performance.timeOrigin!==${previousOrigin}&&document.readyState==='complete'`))break;}
    catch(error){if(!/navigated|context|closed/i.test(String(error)))throw error;}
    if(Date.now()>navigationDeadline)throw Error('Recovery test navigation did not finish');
    await new Promise(resolve=>setTimeout(resolve,50));
  }
  await ready();
  await evaluate('window.recoveryCompleted=false;layerApp.documents.startRecovery().then(()=>recoveryCompleted=true);null');
  for(const count of [2,3]){
    await wait(`!![...document.querySelectorAll('dialog[open] h2')].find(n=>n.textContent==='Recover drawing?')`);
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(n=>n.textContent==='Recover').click()`);
    await wait(`layerApp.app.document_tabs(0).tabs.length===${count}`);
  }
  await wait('window.recoveryCompleted');await ready();
  const tabs=await evaluate(`JSON.parse(JSON.stringify(layerApp.app.document_tabs(0),(_,v)=>typeof v==='bigint'?Number(v):v))`);
  assert.equal(tabs.tabs.length,3);assert.deepEqual(tabs.tabs.map(t=>t.modified),[false,true,true]);
  const counts=[];
  for(const tab of tabs.tabs.slice(1)){
    await evaluate(`layerApp.documents.select(BigInt(${tab.id}))`);await ready();
    counts.push(await evaluate('layerApp.state().layers.length'));
    assert.equal(await evaluate('layerApp.state().document_file.location??null'),null);
  }
  assert.deepEqual(counts.sort((a,b)=>a-b),[base+1,base+2],'Each recovery record restores the project captured for that owner');
  await evaluate(`layerApp.documents.select(BigInt(${tabs.tabs[0].id}))`);await ready();
  assert.equal(await evaluate(`(()=>{const e=new Event('beforeunload',{cancelable:true});window.dispatchEvent(e);return e.defaultPrevented;})()`),true,'Inactive unsaved drawings protect the browser window');
  console.log('PASS drawing tab recovery: multiple append offers, independent captured owners, no master destinations, inactive unsaved unload protection');
}

export async function checkDrawingTabs({call,evaluate,settle}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+60000;function poll(){if(${condition})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}+' '+document.querySelector('#status').textContent));else setTimeout(poll,30);}poll();})`);
  const tabs=()=>evaluate(`JSON.parse(JSON.stringify(layerApp.app.document_tabs(1000),(_,v)=>typeof v==='bigint'?Number(v):v))`);
  const invoke=command=>evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);
  const ready=()=>wait(`!layerApp.documents.busy()&&layerApp.app.brush_ready()&&!layerApp.state().document_file.busy&&layerApp.app.document_park_ready()`);
  const select=async id=>{await evaluate(`layerApp.documents.select(BigInt(${id}))`);await ready();await settle();
    assert.equal(await evaluate(`document.querySelector('button[data-command="new_document"]')?.disabled??false`),false,'Reactivation republishes enabled native controls');};
  const create=async()=>{
    await ready();const count=(await tabs()).tabs.length;await invoke('new_document');
    await wait(`!![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create')`);
    await evaluate(`(()=>{const dialog=document.querySelector('dialog[open]');for(const n of dialog.querySelectorAll('input[type=number]'))n.value=96;[...dialog.querySelectorAll('button')].find(b=>b.textContent==='Create').click();})()`);
    await wait(`layerApp.app.document_tabs(0).tabs.length===${count+1}`);await ready();return (await tabs()).selected;
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
    if(device==='touch'){
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:12,x,y,radiusX:3,radiusY:3,force:1}]});
      await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{id:12,x:tx,y:ty,radiusX:3,radiusY:3,force:1}]});
      await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    }else{
      for(const[type,px,py,buttons]of[['mousePressed',x,y,1],['mouseMoved',tx,ty,1],['mouseReleased',tx,ty,0]])await call('Input.dispatchMouseEvent',{type,x:px,y:py,button:'left',buttons,clickCount:1,pointerType:device,force:buttons?.7:0});
    }
    await settle();
    assert.deepEqual((await tabs()).tabs.map(t=>t.id),[second,third,first],`${device} reorders ${compact?'an explicit list handle':'the tab body'} without a hold`);
    assert.equal((await tabs()).selected,third,'Reordering does not activate the dragged drawing');
    await evaluate('layerApp.app.document_order_history(false);layerApp.documents.refresh()');await settle();
    assert.deepEqual((await tabs()).tabs.map(t=>t.id),[first,second,third]);
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
  await evaluate(`window.tabManifest=bytes=>JSON.parse(new TextDecoder().decode(bytes.slice(52,52+Number(new DataView(bytes.buffer,bytes.byteOffset).getBigUint64(12,true)))));window.tabExpected=tabManifest([...tabFiles.values()][0]).blobs;`);
  assert.ok(await evaluate('tabExpected.length>0'));
  await invoke('undo');await ready();
  const neighbor=await create();
  assert.equal((await tabs()).resident_bytes,0,'Inactive redo payloads leave RAM after successful OPFS write');
  assert.ok(await evaluate(`(async()=>{let n=0;const root=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-live-tiles');for await(const dir of root.values())for await(const file of dir.values())if(file.kind==='file')n++;return n;})()`),'Private immutable chunks exist');
  await select(painted);await invoke('redo');await ready();
  await invoke('save_document_as');await ready();
  assert.deepEqual(await evaluate('tabManifest([...tabFiles.values()].at(-1)).blobs'),await evaluate('tabExpected'),'Redo from disk retains exact compressed tile identity');
  await select(neighbor);
  assert.equal((await tabs()).resident_bytes,0,'A reread cache is evicted on the next switch');
  const beforeCorrupt=(await tabs()).tabs.map(t=>t.id);
  assert.equal(await evaluate(`layerApp.documents.openFiles([new File(['invalid'],'broken.capy')]).then(()=>false,()=>true)`),true);
  assert.deepEqual((await tabs()).tabs.map(t=>t.id),beforeCorrupt,'Corrupt opening cannot remove or replace a drawing');
  await evaluate(`layerApp.documents.openFiles([new File([[...tabFiles.values()][0]],'duplicate.capy'),new File(['invalid'],'broken-middle.capy'),new File([[...tabFiles.values()][0]],'duplicate.capy')])`);await ready();
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
