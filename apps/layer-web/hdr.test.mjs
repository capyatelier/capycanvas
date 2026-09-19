import assert from 'node:assert/strict';
import {checkProofStartingLayout,checkProofKeys} from './proof-parity.test.mjs';
import {mkdir,writeFile} from 'node:fs/promises';

// Run in headed desktop Chrome or the attached tablet's ordinary Chrome tab.
// Files go through real codecs/workers/storage; only native file-picker handles
// are supplied by the harness. Input uses the browser's touch/pen dispatch path.
export async function checkHdr({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/color-m4-web-android/browser';
  await mkdir(directory,{recursive:true});
  const capture=async name=>writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false})).data,'base64'));
  const wait=c=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){try{if(${c})resolve(true);else if(performance.now()-start>55000)reject(Error(${JSON.stringify(c)}+': '+document.body.innerText.slice(-1800)));else setTimeout(poll,30)}catch(e){reject(e)}}poll()})`);
  const click=(label,root='dialog[open]')=>evaluate(`(()=>{const b=[...document.querySelectorAll(${JSON.stringify(root+' button')})].find(b=>b.textContent===${JSON.stringify(label)});if(!b||b.disabled)throw Error('Missing enabled '+${JSON.stringify(label)});b.click()})()`);
  const set=(label,value,root='dialog[open]')=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(root+' [aria-label="'+label+'"]')});if(!n)throw Error('Missing '+${JSON.stringify(label)});n.value=${JSON.stringify(value)};n.dispatchEvent(new Event('change',{bubbles:true}))})()`);
  const invoke=async command=>{await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);};
  const hist=()=>evaluate(`(async()=>{const c=layerApp.app.capture_control();try{return JSON.parse(JSON.stringify((await layerApp.app.histogram(c)).histogram,(_,v)=>typeof v==='bigint'?Number(v):v))}finally{c.free()}})()`);
  const save=async()=>{await invoke('save_document_as');await wait('!layerApp.state().document_file.busy&&!layerApp.state().document_file.modified');return evaluate('hdrTest.manifest(hdrTest.last)');};
  const open=async name=>{const epoch=await evaluate('Number(layerApp.state().document_file.epoch)');await evaluate(`hdrTest.openName=${JSON.stringify(name)}`);await invoke('open_document');await wait(`Number(layerApp.state().document_file.epoch)!==${epoch}&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()`);};
  await wait('layerApp.startupTimes.complete!==null && layerApp.app.brush_ready()');
  await evaluate(`window.hdrTest={files:new Map(),open:showOpenFilePicker,save:showSaveFilePicker};
    hdrTest.manifest=b=>JSON.parse(new TextDecoder().decode(b.slice(52,52+Number(new DataView(b.buffer,b.byteOffset).getBigUint64(12,true)))));
    hdrTest.dismiss=setInterval(()=>[...document.querySelectorAll('dialog[open] button')].find(b=>['Keep for Later','Discard Changes'].includes(b.textContent))?.click(),50);
    window.showSaveFilePicker=async o=>({name:o.suggestedName,async createWritable(){let b;return{async write(v){b=new Uint8Array(v instanceof Blob?await v.arrayBuffer():v)},async close(){hdrTest.last=b;hdrTest.files.set(o.suggestedName,b)},async abort(){}}}});
    window.showOpenFilePicker=async()=>[{name:hdrTest.openName,async getFile(){const b=hdrTest.files.get(hdrTest.openName)??await(await fetch('/pkg/'+hdrTest.openName)).arrayBuffer();return new File([b],hdrTest.openName)}}];`);
  const results={browser:await evaluate('navigator.userAgent'),steps:[]};
  const mark=s=>{results.steps.push(s);console.log(s)};
  try {
    // Preserve prior recovery records, but finish offering them before real
    // contacts target the header. A late modal can intercept the first tap.
    await evaluate('layerApp.documents.startRecovery()');
    await checkProofStartingLayout({call,evaluate,settle});
    await open('hdr-pq.png');
    assert.equal(await evaluate('layerApp.app.document_color().depth'),'F16');
    await wait('layerApp.app.tone_status().ready||layerApp.app.tone_status().error');assert.equal(await evaluate('layerApp.app.tone_status().error??null'),null);
    let original=await hist();assert.ok(original.channels.some(c=>c.above>0));
    await wait(`document.querySelector("#hdr-status").textContent.includes("mapped SDR")`);
    mark('Independent FFmpeg PQ input opens as HDR, retains above-white samples, and completes mapped SDR analysis');
    // The GTK corner edit action, no palette footer, and HDR numeric fields.
    await evaluate(`layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'color',visible:true}})`);
    await evaluate(`[...document.querySelectorAll('.dock-tab[data-panel="color"][aria-selected="false"]')].find(n=>n.getBoundingClientRect().width>0)?.click()`);
    await wait(`!![...document.querySelectorAll('button[aria-label="Edit Color"]')].find(b=>b.getBoundingClientRect().width>0)`);
    assert.equal(await evaluate(`!![...document.querySelectorAll('.color-wheel-control button')].find(b=>/Palettes/.test(b.textContent))`),false);
    await capture('color-panel');
    await evaluate(`[...document.querySelectorAll('button[aria-label="Edit Color"]')].find(b=>b.getBoundingClientRect().width>0).click()`);
    await wait(`!!document.querySelector('dialog[aria-label="Edit Color"][open]')`);
    assert.equal(await evaluate(`document.querySelector('[aria-label="Color model"]').value`),'linear_rgb');
    for(const text of ['','-','.','17','1e999']) {
      await evaluate(`(()=>{const n=document.querySelector('[aria-label="Intensity (EV)"]');n.focus();n.value=${JSON.stringify(text)};n.dispatchEvent(new Event('input'));})()`);
      assert.equal(await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Use Color').disabled`),true);
      assert.equal(await evaluate(`document.querySelector('[aria-label="Intensity (EV)"]').value`),text);
    }
    await evaluate(`(()=>{const n=document.querySelector('[aria-label="Intensity (EV)"]');n.value='-0.5';n.dispatchEvent(new Event('input'));})()`);
    assert.equal(await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Use Color').disabled`),false);
    await evaluate(`(()=>{const n=document.querySelector('[aria-label="Intensity (EV)"]');n.value=3;n.dispatchEvent(new Event('input'));})()`);
    await capture('edit-color');
    await click('Use Color');await wait('layerApp.app.color_panel().intensity===3');
    assert.equal(await evaluate('layerApp.app.color_panel().intensity'),3);
    mark('Picker matches GTK corner action, removes palettes, and edits HDR intensity');
    await wait(`!document.querySelector('dialog[open]')`);
    await evaluate(`layerApp.dispatch({type:'select_brush',id:1});const form=layerApp.app.color_ui({type:'form',request:{color:{space:'Srgb',rgba:[0,0,0,1]},document_space:'Srgb',model:'linear_rgb',intensity:0,fields:['-4','4','1','100']}});layerApp.dispatch({type:'color',action:{op:'set_slot',slot:'foreground',color:form.value}});`);
    await wait('layerApp.app.brush_ready()');await settle();
    const center=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()`);
    assert.ok(await evaluate(`[-50,0,50].every(dx=>document.elementFromPoint(${center.x}+dx,${center.y})===layerApp.canvas)`),'The complete pen stroke must start and finish on the canvas');
    for(const[type,dx,buttons]of[['mousePressed',-50,1],['mouseMoved',0,1],['mouseMoved',50,1],['mouseReleased',50,0]]){await call('Input.dispatchMouseEvent',{type,x:center.x+dx,y:center.y,button:'left',buttons,pointerType:'pen',force:buttons?.85:0});await settle();}
    const painted=await hist();assert.ok(painted.channels.some(c=>c.below>0),'Negative finite HDR paint survives the GPU');assert.notDeepEqual(painted,original);
    await invoke('undo');assert.deepEqual(await hist(),original);await invoke('redo');assert.deepEqual(await hist(),painted);original=painted;
    mark('Pen painting preserves negative and above-white channels with exact one-step undo/redo');
    const master0=await save();
    await invoke('sdr_rendition');await wait(`!!document.querySelector('.proof-panel [aria-label="SDR balance and contrast"]')`);
    if(process.env.LAYER_PROOF_WORKSPACE) {
      const layout=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state().workspace.layout,(_,v)=>typeof v==="bigint"?Number(v):v))');
      const before=await layout();
      const point=await evaluate(`(()=>{const r=document.querySelector('.dock-group .dock-tab[data-panel="proof"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
      const target=await evaluate('({x:innerWidth*.55,y:innerHeight*.3})');
      for(const[type,p,buttons]of[['mousePressed',point,1],['mouseMoved',{x:point.x+20,y:point.y-20},1],['mouseMoved',target,1],['mouseReleased',target,0]]){await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons,pointerType:'pen',force:buttons?.7:0});await settle();}
      await wait(`!!document.querySelector('.floating-panel .dock-tab[data-panel="proof"]')`);
      const moved=await layout();assert.notDeepEqual(moved,before);
      await invoke('undo_workspace');assert.deepEqual(await layout(),before);
      await invoke('redo_workspace');assert.deepEqual(await layout(),moved);
      const start=await evaluate(`(()=>{const r=document.querySelector('.floating-panel .dock-tab[data-panel="proof"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:8,...start}]});
      await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{id:8,x:start.x+90,y:start.y+80}]});await settle();
      await call('Input.dispatchTouchEvent',{type:'touchCancel',touchPoints:[]});await settle();assert.deepEqual(await layout(),moved);
      await invoke('undo_workspace');assert.deepEqual(await layout(),before);
      const group=await evaluate(`layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('proof')).id`);
      await evaluate(`layerApp.dispatch({type:'customize',action:{type:'set_column_collapsed',group:${group},collapsed:true}})`);
      const column=await evaluate(`layerApp.app.layout(innerWidth,innerHeight).collapsed.find(c=>c.groups.some(g=>g.group===${group})).id`);
      await evaluate(`layerApp.dispatch({type:'customize',action:{type:'set_column_drawers',column:${column},drawers:true}})`);
      await invoke('sdr_rendition');await settle();
      await wait(`document.querySelector('.proof-tone-pad')?.getBoundingClientRect().width>0&&!!document.querySelector('.proof-tone-pad')?.closest('.content-drawer')`);
      assert.equal(await evaluate('layerApp.state().customization.column_drawers.length'),1);
      await invoke('sdr_rendition');assert.equal(await evaluate('layerApp.state().customization.column_drawers.length'),1);
      await evaluate(`layerApp.dispatch({type:'customize',action:{type:'set_column_collapsed',group:${group},collapsed:false}})`);await settle();
      await wait(`!!document.querySelector('.dock-group .proof-tone-pad')`);
      mark('Proof tab pen drag, layout undo/redo, touch cancellation and idempotent collapsed drawer reveal pass');
    }
    const beforeRecipe=await evaluate('layerApp.app.proof_form().rendition');
    // A floating workspace panel must not steal contacts from the active Proof.
    const occlusion=await evaluate(`(()=>{const c=document.querySelector('.proof-tone-pad'),r=c.getBoundingClientRect(),d=layerApp.app.color_ui({type:'proof_dial',size:r.width,recipe:layerApp.app.proof_form().rendition});return d.arcs.flatMap(a=>[16,48].map(i=>{const p=a.path[i];return document.elementFromPoint(r.x+p[0],r.y+p[1])===c}))})()`);
    assert.ok(occlusion.every(Boolean),'Proof arcs remain reachable above floating workspace panels');
    const r=await evaluate(`(()=>{const r=document.querySelector('.proof-tone-pad').getBoundingClientRect();return{x:r.x,y:r.y,w:r.width,h:r.height}})()`);
    await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,x:r.x+r.w*.5,y:r.y+r.h*.5}]});
    await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{id:1,x:r.x+r.w*.7,y:r.y+r.h*.35}]});
    await call('Input.dispatchTouchEvent',{type:'touchCancel',touchPoints:[]});await settle();
    assert.deepEqual(await evaluate('layerApp.app.proof_form().rendition'),beforeRecipe,'Touch cancellation restores saved appearance');
    for(const[type,x,y,buttons]of[['mousePressed',.5,.5,1],['mouseMoved',.7,.35,1],['mouseReleased',.7,.35,0]])await call('Input.dispatchMouseEvent',{type,x:r.x+r.w*x,y:r.y+r.h*y,button:'left',buttons,pointerType:'pen',force:buttons?.6:0});
    await settle();const changed=await evaluate('layerApp.app.proof_form().rendition');assert.notDeepEqual(changed,beforeRecipe);
    await invoke('undo');assert.deepEqual(await evaluate('layerApp.app.proof_form().rendition'),beforeRecipe);
    await invoke('redo');assert.deepEqual(await evaluate('layerApp.app.proof_form().rendition'),changed);
    const arc=await evaluate(`(()=>{const r=document.querySelector('.proof-tone-pad').getBoundingClientRect(),d=layerApp.app.color_ui({type:'proof_dial',size:r.width,recipe:layerApp.app.proof_form().rendition});return[16,48].map(i=>{const p=d.arcs[0].path[i];return{x:r.x+p[0],y:r.y+p[1]}})})()`);
    await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:2,...arc[0]}]});
    await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{id:2,...arc[1]}]});
    await call('Input.dispatchTouchEvent',{type:'touchCancel',touchPoints:[]});await settle();
    assert.deepEqual(await evaluate('layerApp.app.proof_form().rendition'),changed,'Arc cancellation restores the exact recipe');
    for(const[type,p,buttons]of [['mousePressed',arc[0],1],['mouseMoved',arc[1],1],['mouseReleased',arc[1],0]])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons,pointerType:'pen',force:buttons?.6:0});
    await settle();assert.notEqual((await evaluate('layerApp.app.proof_form().rendition')).exposure,changed.exposure);
    await invoke('undo');assert.deepEqual(await evaluate('layerApp.app.proof_form().rendition'),changed,'One undo restores an arc gesture');
    await checkProofKeys({call,evaluate,settle,invoke});
    assert.deepEqual(await hist(),original,'SDR appearance does not change HDR artwork');
    await writeFile(`${directory}/sdr-rendition.json`,JSON.stringify(await evaluate('layerApp.app.proof_form().rendition'))+'\n');
    const master=await save();assert.deepEqual(master.blobs,master0.blobs);assert.deepEqual(master.document.layers,master0.document.layers);
    mark('Touch cancel and pen edit on the SDR pad preserve HDR raster data; one-step undo/redo and save persist the rendition');
    await settle();await writeFile(`${directory}/proof-sdr.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    assert.ok(await evaluate(`(()=>{const c=document.querySelector('.proof-tone-pad');return c.getContext('2d').getImageData(c.width/2,c.height/2,1,1).data[3]===255})()`),await evaluate(`document.querySelector('.proof-panel').innerText`));
    await evaluate(`layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'proof',visible:false}})`);
    await evaluate(`hdrTest.files.set('hdr-master.capy',hdrTest.last.slice())`);
    await open('hdr-master.capy');await wait('layerApp.app.tone_status().ready||layerApp.app.tone_status().error');assert.equal(await evaluate('layerApp.app.tone_status().error??null'),null);
    assert.deepEqual(await hist(),original);assert.deepEqual(await evaluate('layerApp.app.proof_form().rendition'),changed);
    await evaluate('layerApp.restartGpu()');await wait('layerApp.app.brush_ready()&&layerApp.startupTimes.complete!==null');await wait('layerApp.app.tone_status().ready||layerApp.app.tone_status().error');assert.equal(await evaluate('layerApp.app.tone_status().error??null'),null);
    assert.deepEqual(await hist(),original);assert.deepEqual(await evaluate('layerApp.app.proof_form().rendition'),changed);
    mark('HDR native save/reopen and GPU recovery preserve exact histogram and saved SDR appearance');
    // Complete delivery through the visible controls and inspect resulting files.
    for(const range of ['exr','hdr','sdr']){
      await invoke('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Dynamic range"]')`);
      await set('Dynamic range',range);if(range==='sdr')await set('Bit depth','U8');
      if(range==='hdr'){
        const failure=await evaluate(`(async()=>{const c=layerApp.app.capture_control();try{const id=layerApp.state().requests.find(r=>r.kind.type==='document'&&r.kind.request.type==='export').id;const base=layerApp.app.export_form().recipes[0][1];const recipe=layerApp.app.export_draft(base,{type:'format',value:'PngHdr'}).recipe;await layerApp.app.export_image(id,recipe,c,false);return null}catch(e){return String(e)}finally{c.free()}})()`);
        assert.match(failure,/range|BT.2020/i,'Strict HDR rejects unrepresentable colors');
        await click('Preview Output');await wait(`document.querySelectorAll('dialog[open] .color-comparison canvas').length===2`);
        assert.ok(await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Choose File…').disabled`));
        assert.ok(await evaluate(`document.querySelector('dialog[open]').textContent.includes('or choose OpenEXR')`));
        await evaluate(`(()=>{const c=document.querySelector('[aria-label="Clip out-of-range HDR colors"]');c.checked=true;c.dispatchEvent(new Event('change'))})()`);
      }
      await click('Preview Output');await wait(`document.querySelectorAll('dialog[open] .color-comparison canvas').length===2`);
      await click('Choose File…');await wait('!layerApp.state().document_file.busy');
      const bytes=await evaluate('Array.from(hdrTest.last)');assert.ok(bytes.length>100);
      await writeFile(`${directory}/${range}-delivery.${range==='exr'?'exr':'png'}`,new Uint8Array(bytes));
      await evaluate(`hdrTest.files.set('${range}-delivery.${range==='exr'?'exr':'png'}',hdrTest.last.slice())`);
    }
    await open('exr-delivery.exr');assert.equal(await evaluate('layerApp.app.document_color().depth'),'F32');assert.ok((await hist()).channels.some(c=>c.below>0),'OpenEXR preserves extended negative channels');
    await open('sdr-delivery.png');assert.equal(await evaluate('layerApp.app.document_color().depth'),'U8');
    await open('hdr-delivery.png');assert.equal(await evaluate('layerApp.app.document_color().depth'),'F16');assert.ok((await hist()).channels.some(c=>c.above>0));
    mark('Float32 OpenEXR, HDR PQ and authored SDR PNG previews, exports and reopening complete through the browser UI');
    assert.equal(await evaluate('layerApp.state().host_error??null'),null);
    for(const name of (process.env.LAYER_HDR_REJECTIONS||'').split(',').filter(Boolean)){
      const epoch=await evaluate('Number(layerApp.state().document_file.epoch)'),before=await hist();
      await evaluate(`hdrTest.openName=${JSON.stringify(name)}`);await invoke('open_document');
      await wait(`!layerApp.state().document_file.busy&&!!layerApp.state().host_error`);
      assert.match(await evaluate('layerApp.state().host_error'),/12 megapixels/);
      assert.equal(await evaluate('Number(layerApp.state().document_file.epoch)'),epoch);assert.deepEqual(await hist(),before);
      mark(`${name}: explicit browser admission rejection preserves the existing master`);
    }
    await writeFile(`${directory}/workflow.json`,JSON.stringify(results,null,2)+'\n');
  } finally {
    await evaluate('clearInterval(hdrTest.dismiss);window.showOpenFilePicker=hdrTest.open;window.showSaveFilePicker=hdrTest.save');
  }
}
