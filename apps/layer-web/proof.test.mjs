import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

// Real DOM, Wasm workers, storage and WebGPU. ICC fixtures are supplied locally,
// never redistributed. May run through test.mjs or an existing tablet CDP tab.
export async function checkProof({call,evaluate,settle}, {profileUrl='/pkg/proof-cmyk.icc',originalUrl='/pkg/proof-p3.icc'}={}) {
  const rawEvaluate=evaluate;
  evaluate=expression=>rawEvaluate(expression.startsWith('(await ')?`(async()=>${expression})()`:expression);
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){try{if(${condition})resolve(true);else if(performance.now()-start>120000)reject(Error(${JSON.stringify(condition)}+': '+document.body.innerText.slice(-1400)));else setTimeout(poll,25)}catch(e){reject(e)}}poll()})`);
  const hide=()=>evaluate(`layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'proof',visible:false}})`);
  const click=async label=>{
    if(label==='Close'){await hide();return;}
    if(label==='Cancel'&&!await evaluate(`!!document.querySelector('dialog[open]')`)){await click('Off');await hide();return;}
    await evaluate(`(()=>{const b=[...document.querySelectorAll(':is(dialog[open],.proof-panel) button')].find(b=>b.textContent===${JSON.stringify(label)});if(!b||b.disabled)throw Error('Missing enabled button '+${JSON.stringify(label)});b.click();})()`);
  };
  const invoke=async command=>{await wait(`!layerApp.documents.busy()&&layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);};
  const set=async(label,value)=>evaluate(`(()=>{const s=document.querySelector(':is(dialog[open],.proof-panel) [aria-label="'+${JSON.stringify(label)}+'"]');s.value=${JSON.stringify(value)};s.dispatchEvent(new Event('change'));})()`);
  const setup=async()=>{await invoke('soft_proof_setup');await wait(`!!document.querySelector('.proof-panel select[aria-label="Proof profile"]')`);if(await evaluate('layerApp.app.proof_form().mode')!=='print')await click('Print');};
  const ready=async()=>{const expected=await evaluate(`document.querySelector('.proof-panel [aria-label="Proof profile"]')?.selectedOptions[0]?.textContent`);await wait(`${expected?`layerApp.app.proof_form().recipe.name===${JSON.stringify(expected)} &&`:''} !document.querySelector('.proof-panel [role="status"]')?.textContent && layerApp.app.proof_status().text.startsWith('Proof:') && !layerApp.app.proof_status().needed`);if(await evaluate(`!!document.querySelector('.proof-panel')`))await click('Close');};
  const choose=async(name,group='Saved Profiles')=>{const selector=`.proof-panel optgroup[label="${group}"] option`;await wait(`!![...document.querySelectorAll(${JSON.stringify(selector)})].find(o=>o.textContent===${JSON.stringify(name)})`);await evaluate(`(()=>{const s=document.querySelector('.proof-panel select[aria-label="Proof profile"]');s.value=[...document.querySelectorAll(${JSON.stringify(selector)})].find(o=>o.textContent===${JSON.stringify(name)}).value;s.dispatchEvent(new Event('change'));})()`);};
  const histogram=()=>evaluate(`(async()=>{const c=layerApp.app.capture_control();try{return JSON.parse(JSON.stringify((await layerApp.app.histogram(c)).histogram,(_,v)=>typeof v==='bigint'?Number(v):v))}catch(e){throw Error(String(e))}finally{c.free()}})()`);
  const save=async()=>{await invoke('save_document_as');await wait('!layerApp.state().document_file.busy && !layerApp.state().document_file.modified');return evaluate('proofTest.manifest([...proofTest.files.values()].at(-1))');};
  const backing=m=>({blobs:m.blobs,sources:m.tiled_sources?.images,layers:m.document.layers,color:m.document.color});
  const exportPng=async()=>{
    await invoke('export_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Format"]')`);
    await set('Format','Png');await set('Output profile','0');await set('Bit depth','U8');await set('Dither','None');
    await click('Choose File…');await wait('!layerApp.state().document_file.busy');
    return evaluate(`(async()=>{const bytes=[...proofTest.files.entries()].findLast(([k])=>k.endsWith('.png'))[1];return [...new Uint8Array(await crypto.subtle.digest('SHA-256',bytes))]})()`);
  };
  await wait('window.layerApp && layerApp.app.brush_ready()');
  await wait('JSON.parse(layerApp.app.workspace_view())?.ready && !JSON.parse(layerApp.app.workspace_view()).busy');
  await evaluate(`window.proofTest={files:new Map(),open:window.showOpenFilePicker,save:window.showSaveFilePicker};
    proofTest.recoveryTimer=setInterval(()=>[...document.querySelectorAll(':is(dialog[open],.proof-panel) button')].find(b=>b.textContent==='Keep for Later')?.click(),50);
    proofTest.manifest=bytes=>JSON.parse(new TextDecoder().decode(bytes.slice(52,52+Number(new DataView(bytes.buffer,bytes.byteOffset).getBigUint64(12,true)))));
    window.showSaveFilePicker=async o=>({name:o.suggestedName,async createWritable(){let bytes;return{async write(v){bytes=new Uint8Array(v instanceof Blob?await v.arrayBuffer():v)},async close(){proofTest.files.set(o.suggestedName,bytes)},async abort(){}}}});`);
  try {
    await invoke('new_document');await wait(`!!document.querySelector('dialog[open]')`);
    await evaluate(`[...document.querySelectorAll(':is(dialog[open],.proof-panel) button')].find(b=>b.textContent==='Discard Changes')?.click()`);
    await wait(`!!document.querySelector('dialog[open] select[aria-label="Color space"]')`);
    await set('Color space','DisplayP3');await set('Bit depth','U16');
    await evaluate(`(()=>{const d=document.querySelector('dialog[open]:has(select[aria-label="Color space"])');for(const[label,value]of[['Width',513],['Height',257]]){const n=[...d.querySelectorAll('input')].find(n=>n.getAttribute('aria-label')?.startsWith(label));n.value=value;}})()`);
    await click('Create');await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');
    await invoke('soft_proof');await wait(`!!document.querySelector('.proof-panel select[aria-label="Proof profile"]')`);
    await evaluate('new Promise(r=>setTimeout(r,400))');
    assert.equal(await evaluate('layerApp.state().soft_proof'),false);
    assert.equal(await evaluate('layerApp.app.proof_form().document_profile'),null);
    assert.equal(await evaluate(`document.querySelector('.proof-panel [aria-label="Proof profile"]').value`),'');
    assert.equal(await evaluate(`!!document.querySelector('.proof-panel header')`),false);
    assert.equal(await evaluate(`document.querySelector('[aria-label="Simulate"]').value`),'black_ink');
    assert.equal(await evaluate(`document.querySelector('[aria-label="Black point compensation"]').checked`),true);
    await click('Cancel');await wait(`!document.querySelector('dialog[open]')`);
    assert.equal(await evaluate('layerApp.app.proof_form().document_profile??null'),null);
    await evaluate(`(async()=>{proofTest.original=await layerApp.app.profile_library('import',undefined,new Uint8Array(await(await fetch(${JSON.stringify(originalUrl)})).arrayBuffer()));proofTest.target=await layerApp.app.profile_library('import',undefined,new Uint8Array(await(await fetch(${JSON.stringify(profileUrl)})).arrayBuffer()));})()`);
    const names=await evaluate('[proofTest.original.name,proofTest.target.name]');
    await setup();await choose(names[0]);
    await invoke('export_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Format"]')`);
    assert.equal(await evaluate('layerApp.app.proof_form().recipe.name'),names[0],'Export waits for the selected print target');
    await click('Cancel');await ready();
    // The embedded original remains usable after its local entry is removed.
    await evaluate(`(async()=>{for(const p of await layerApp.app.profile_library('list'))if(p.name===proofTest.original.name)await layerApp.app.profile_library('remove',p.id)})()`);
    const base=await save();
    await setup();await choose(names[1]);
    assert.equal(await evaluate(`document.querySelector('optgroup[label="Document Profile"] option').textContent`),names[0]);
    await click('Cancel');await wait(`!document.querySelector('dialog[open]')`);
    assert.equal(await evaluate(`(await layerApp.app.profile_library('list')).some(p=>p.name===proofTest.original.name)`),false);
    assert.deepEqual(await save(),base);
    // A durable-store failure must leave recipe and history intact, with retry.
    await setup();await choose(names[1]);
    await evaluate(`proofTest.library=layerApp.app.profile_library.bind(layerApp.app);layerApp.app.profile_library=(op,...args)=>op==='import'?Promise.reject(Error('Injected profile storage failure')):proofTest.library(op,...args);`);
    await wait(`document.querySelector('.proof-panel .error-message')?.textContent.includes('Injected profile storage failure')`);
    assert.equal(await evaluate('layerApp.app.proof_form().recipe.name'),names[0]);
    await evaluate('layerApp.app.profile_library=proofTest.library');
    await choose(names[1]);await ready();
    if(process.env.LAYER_TEST_ARTIFACTS){await setup();await mkdir(process.env.LAYER_TEST_ARTIFACTS,{recursive:true});await writeFile(`${process.env.LAYER_TEST_ARTIFACTS}/proof-print.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false})).data,'base64'));await hide();}
    assert.equal(await evaluate(`(await layerApp.app.profile_library('list')).some(p=>p.name===proofTest.original.name)`),true);
    assert.deepEqual(await evaluate(`(await layerApp.app.profile_library('get',(await layerApp.app.profile_library('list')).find(p=>p.name===proofTest.original.name).id)).profile`),await evaluate('proofTest.original.profile'));
    const replaced=await save();assert.deepEqual(backing(replaced),backing(base));
    assert.equal(replaced.tiled_sources.proof.name,names[1]);
    assert.equal(replaced.tiled_sources.profiles.length,1,'Only the active proof ICC is embedded');
    console.log('First use, defaults, Document Profile retention, cancel, preservation failure/retry and exact local ICC copy passed');
    // Paint while proofing, compare one-step history and independent exports.
    await evaluate(`layerApp.dispatch({type:'select_brush',id:1});layerApp.dispatch({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'DisplayP3',rgba:[1,0,.7,1]}}});layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'navigator',visible:true}});`);
    await settle();
    const point=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()`);
    const before=await histogram();
    for(const[type,dx,buttons]of[['mousePressed',-80,1],['mouseMoved',0,1],['mouseMoved',80,1],['mouseReleased',80,0]]){await call('Input.dispatchMouseEvent',{type,x:point.x+dx,y:point.y,button:'left',buttons,clickCount:1,pointerType:'pen',force:buttons?.65:0});await settle();}
    const painted=await histogram();assert.notDeepEqual(painted,before);
    await invoke('undo');await settle();assert.deepEqual(await histogram(),before);
    await invoke('redo');await settle();assert.deepEqual(await histogram(),painted);
    await evaluate(`for(const column of layerApp.app.layout(innerWidth,innerHeight).collapsed)layerApp.dispatch({type:'customize',action:{type:'set_column_collapsed',group:column.groups[0].group,collapsed:false}})`);await settle();
    const clips=[{x:point.x-85,y:point.y-12,width:170,height:24,scale:1},await evaluate(`(()=>{const r=[...document.querySelectorAll('.navigator-overview')].find(n=>n.getBoundingClientRect().width>0).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,scale:1}})()` )];
    const screenshots=async()=>{await settle();await evaluate('layerApp.app.wait_for_canvas()');const images=[];for(const clip of clips)images.push((await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false,clip})).data);return images;};
    const sameView=async(actual,expected,label)=>{
      if(process.env.LAYER_TEST_ARTIFACTS){const name=label.toLowerCase().replaceAll(' ','-');await writeFile(`${process.env.LAYER_TEST_ARTIFACTS}/${name}-expected.png`,Buffer.from(expected,'base64'));await writeFile(`${process.env.LAYER_TEST_ARTIFACTS}/${name}-actual.png`,Buffer.from(actual,'base64'));}
      // Fractional DPR and the Android compositor can round antialiased edges
      // differently after device recreation. Exact backing/export assertions
      // remain byte-for-byte; this checks the presented screenshot separately.
      const diff=await evaluate(`(async()=>{const read=async data=>{const b=await createImageBitmap(await(await fetch('data:image/png;base64,'+data)).blob());const c=new OffscreenCanvas(b.width,b.height),x=c.getContext('2d');x.drawImage(b,0,0);b.close();return x.getImageData(0,0,c.width,c.height).data};const a=await read(${JSON.stringify(actual)}),b=await read(${JSON.stringify(expected)});if(a.length!==b.length)throw Error('Screenshot extent changed');let max=0,sum=0;for(let i=0;i<a.length;i++){const d=Math.abs(a[i]-b[i]);max=Math.max(max,d);sum+=d}return {max,mean:sum/a.length}})()`);
      assert.ok(diff.max<=8&&diff.mean<=.5,`${label}: ${JSON.stringify(diff)}`);
    };
    await settle();const proofShots=await screenshots();
    await invoke('soft_proof');await settle();const normalShots=await screenshots();
    proofShots.forEach((shot,i)=>assert.notEqual(shot,normalShots[i],i?'Navigator must show proof viewing':'Canvas must show proof viewing'));
    await invoke('soft_proof');await settle();
    console.log('Presented canvas and Navigator both change with proof viewing');
    const saved=await save();const on=await exportPng();
    await invoke('gamut_warning');await invoke('soft_proof');assert.equal(await evaluate('layerApp.state().document_file.modified'),false);
    assert.deepEqual(await histogram(),painted);assert.deepEqual(await exportPng(),on);
    assert.equal(await evaluate('layerApp.state().gamut_warning'),false,'Proof Off clears gamut warning as on GTK');await settle();assert.equal(await evaluate('document.querySelector("#proof-status").hidden'),true);
    assert.deepEqual(await exportPng(),on);
    // Reopen on a host with neither target installed: only active ICC travels.
    await evaluate(`(async()=>{proofTest.master=[...proofTest.files.entries()].findLast(([k])=>k.endsWith('.capy'))[1].slice();for(const p of await layerApp.app.profile_library('list'))await layerApp.app.profile_library('remove',p.id);window.showOpenFilePicker=async()=>[{name:'proof-portable.capy',async getFile(){return new File([proofTest.master],'proof-portable.capy')}}]})()`);
    await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');
    assert.equal(await evaluate('layerApp.state().soft_proof||layerApp.state().gamut_warning'),false);
    assert.equal(await evaluate('document.querySelector("#proof-status").hidden'),true);
    assert.equal(await evaluate('layerApp.app.proof_form().recipe.name'),names[1]);
    assert.deepEqual(backing(await save()),backing(saved));
    await invoke('soft_proof');await ready();assert.deepEqual(await histogram(),painted);
    // Reopening fits the drawing to the current work area. Capture recovery
    // references at that camera; portable data/exports are checked separately.
    const recoveryProofShots=await screenshots();
    await invoke('soft_proof');const recoveryNormalShots=await screenshots();
    recoveryProofShots.forEach((shot,i)=>assert.notEqual(shot,recoveryNormalShots[i]));
    await invoke('soft_proof');await ready();
    // Hold an old device's compiler completion across replacement. Real shader
    // work still runs; a late failure must not stall or stop the new renderer.
    await evaluate(`proofTest.compiler=layerApp.app.compile_startup_step.bind(layerApp.app);layerApp.app.compile_startup_step=()=>{const work=proofTest.compiler().then(()=>null,e=>e);return new Promise((resolve,reject)=>{proofTest.releaseCompiler=()=>work.then(e=>reject(e||Error('Retired compiler completion')))})}`);
    await evaluate('layerApp.restartGpu()');await wait('!!proofTest.releaseCompiler');
    await evaluate('layerApp.app.compile_startup_step=proofTest.compiler;layerApp.restartGpu()');
    await wait('layerApp.app.brush_ready() && layerApp.startupTimes.complete!==null');
    await evaluate('proofTest.releaseCompiler();new Promise(r=>setTimeout(r,100))');
    assert.equal(await evaluate('document.body.dataset.gpu'),'ready','A retired compiler cannot stop the replacement GPU');
    console.log('GPU replacement ignores stalled and late compiler work');
    await ready();await settle();
    const recoveredShots=await screenshots();
    for(let i=0;i<2;i++)await sameView(recoveredShots[i],recoveryProofShots[i],i?'Navigator recovers its proof resources':'Canvas recovers its proof resources');
    await invoke('soft_proof');await settle();
    const recoveredNormal=await screenshots();
    for(let i=0;i<2;i++)await sameView(recoveredNormal[i],recoveryNormalShots[i],i?'Recovered Navigator remains live':'Recovered canvas remains live');
    await invoke('soft_proof');await ready();
    assert.deepEqual(await histogram(),painted);assert.deepEqual(await exportPng(),on);
    assert.equal(await evaluate('layerApp.state().host_error??null'),null);
    console.log('Proofed editing/history, toggle dirty state, histograms, portable native save/reopen, independent PNG bytes and GPU replacement passed');
    // Cancel after preparation actually starts; no recipe or library mutation.
    await setup();await choose('Adobe RGB (1998)','Standard Color Spaces');await wait(`document.querySelector('.proof-panel [role="status"]')?.textContent.includes('Preparing')`);await hide();await invoke('soft_proof');await wait(`!document.querySelector('.proof-panel [role="status"]')?.textContent`);
    assert.equal(await evaluate('layerApp.app.proof_form().recipe.name'),names[1]);
    assert.equal(await evaluate('(await layerApp.app.profile_library("list")).length'),0);
    console.log('Worker preparation cancellation passed');
  } finally {
    await evaluate('clearInterval(proofTest.recoveryTimer);window.showOpenFilePicker=proofTest.open;window.showSaveFilePicker=proofTest.save;if(proofTest.library)layerApp.app.profile_library=proofTest.library;if(proofTest.compiler)layerApp.app.compile_startup_step=proofTest.compiler;');
  }
}
