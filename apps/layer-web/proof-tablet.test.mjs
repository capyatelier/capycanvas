// Explicitly selected CDP tab only. Does not restart Chrome or change flags.
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {checkProof} from './proof.test.mjs';
import {benchPhotoNavigation} from './photo-navigation-bench.test.mjs';

const [tabId,mode='journey',directory='artifacts/color-m3-web-android']=process.argv.slice(2);
assert.ok(tabId,'Usage: node proof-tablet.test.mjs TEST_TAB_ID [journey|performance|memory] [OUTPUT_DIRECTORY]');
const endpoint=process.env.LAYER_CDP_URL||'http://127.0.0.1:9230';
const tabs=await(await fetch(`${endpoint}/json/list`)).json(),tab=tabs.find(t=>t.id===tabId);
assert.ok(tab?.webSocketDebuggerUrl,'Select an existing test tab');
const socket=new WebSocket(tab.webSocketDebuggerUrl),pending=new Map(),errors=[];let next=0;
await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
socket.onmessage=({data})=>{
  const m=JSON.parse(data),p=pending.get(m.id);
  if(p){pending.delete(m.id);clearTimeout(p.timer);m.error?p.reject(Error(JSON.stringify(m.error))):p.resolve(m.result);}
  else if(m.method==='Runtime.exceptionThrown')errors.push(m.params.exceptionDetails);
  else if(m.method==='Page.javascriptDialogOpening'&&m.params.type==='beforeunload')call('Page.handleJavaScriptDialog',{accept:true});
};
function call(method,params={}){return new Promise((resolve,reject)=>{const id=++next,timer=setTimeout(()=>{pending.delete(id);reject(Error(`Timed out: ${method}`));},180000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});}
async function evaluate(expression){if(process.env.LAYER_TEST_TRACE)console.log(expression.slice(0,220));const r=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(r.exceptionDetails)throw Error(r.exceptionDetails.exception?.description||r.exceptionDetails.exception?.value||r.exceptionDetails.text);return r.result.value;}
const settle=()=>evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))');
const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const started=performance.now();function poll(){if(${condition})resolve();else if(performance.now()-started>120000)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,30)}poll()})`);
const click=text=>evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(text)});if(!b||b.disabled)throw Error('Missing button '+${JSON.stringify(text)});b.click()})()`);
try{
  await mkdir(directory,{recursive:true});await call('Runtime.enable');await call('Page.enable');await call('Page.bringToFront');
  await evaluate(`(async()=>{window.proofWake=await navigator.wakeLock.request('screen');window.proofRecoveryTimer=setInterval(()=>[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click(),100)})()`);
  await wait('window.layerApp && layerApp.app.brush_ready()');
  if(mode==='journey')await checkProof({call,evaluate,settle},{profileUrl:process.env.LAYER_PROOF_URL||'/pkg/proof-cmyk.icc',originalUrl:process.env.LAYER_PROOF_ORIGINAL_URL||'/pkg/proof-p3.icc'});
  else if(['performance','memory'].includes(mode)){
    await wait('JSON.parse(layerApp.app.workspace_view())?.ready && !JSON.parse(layerApp.app.workspace_view()).busy && layerApp.state().commands.find(c=>c.id==="open_document")?.enabled && !document.querySelector("dialog[open]")');
    const photo=process.env.LAYER_PHOTO_URL||'/pkg/proof-photo61mp.jpg',profile=process.env.LAYER_PROOF_URL||'/pkg/proof-cmyk.icc';
    await evaluate(`(async()=>{window.proofBench={open:window.showOpenFilePicker};const response=await fetch(${JSON.stringify(photo)});if(!response.ok)throw Error('Photo fixture unavailable');const blob=await response.blob();window.showOpenFilePicker=async()=>[{name:'proof-photo.jpg',async getFile(){return new File([blob],'proof-photo.jpg')}}];proofBench.importStart=performance.now();layerApp.dispatch({type:'invoke',command:'open_document'});})()`);
    await settle();await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')?.click()`);
    await wait('layerApp.state().tabs[0].width===9504 && !layerApp.state().document_file.busy && layerApp.app.brush_ready()');
    await evaluate('proofBench.importMs=performance.now()-proofBench.importStart;window.showOpenFilePicker=proofBench.open');
    if(mode==='performance')await benchPhotoNavigation({call,evaluate},join(directory,'web-photo-normal.json'));
    await evaluate(`(async()=>{proofBench.profile=await layerApp.app.profile_library('import',undefined,new Uint8Array(await(await fetch(${JSON.stringify(profile)})).arrayBuffer()));layerApp.dispatch({type:'invoke',command:'soft_proof_setup'});})()`);
    await wait(`!![...document.querySelectorAll('dialog[open] optgroup[label="Saved Profiles"] option')].find(o=>o.textContent===proofBench.profile.name)`);
    await evaluate(`(()=>{const s=document.querySelector('dialog[open] select[aria-label="Proof profile"]');s.value=[...s.querySelectorAll('optgroup[label="Saved Profiles"] option')].find(o=>o.textContent===proofBench.profile.name).value;s.dispatchEvent(new Event('change'));proofBench.frames=[];proofBench.longTasks=[];proofBench.preparing=true;const tick=t=>{proofBench.frames.push(t);if(proofBench.preparing)requestAnimationFrame(tick)};requestAnimationFrame(tick);proofBench.observer=new PerformanceObserver(list=>{proofBench.longTasks.push(...list.getEntries().map(e=>({start:e.startTime,duration:e.duration})))});proofBench.observer.observe({type:'longtask'});proofBench.prepareStart=performance.now();})()`);
    await click('Apply');await wait(`!document.querySelector('dialog[open]') && layerApp.app.proof_status().text.startsWith('Proof:')`);await settle();
    const preparation=await evaluate(`(()=>{proofBench.preparing=false;proofBench.observer.disconnect();return JSON.parse(JSON.stringify({import_ms:proofBench.importMs,preparation_ms:performance.now()-proofBench.prepareStart,raf:proofBench.frames,long_tasks:proofBench.longTasks,proof:layerApp.app.proof_status(),color:layerApp.app.document_color(),memory:performance.memory?{used:performance.memory.usedJSHeapSize,total:performance.memory.totalJSHeapSize}:null},(_,v)=>typeof v==='bigint'?Number(v):v))})()`);
    await writeFile(join(directory,'web-proof-preparation.json'),JSON.stringify(preparation,null,2));console.log('Preparation:',JSON.stringify(preparation));
    if(mode==='performance')await benchPhotoNavigation({call,evaluate},join(directory,'web-photo-proof.json'));
    else await evaluate('new Promise(r=>setTimeout(r,12000))');
    const shot=await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(join(directory,'web-photo-proof.png'),Buffer.from(shot.data,'base64'));
  }else throw Error('Unknown mode');
  assert.deepEqual(errors,[],'No browser runtime exceptions');console.log(`PASS: tablet proof ${mode}`);
}finally{
  await evaluate('clearInterval(window.proofRecoveryTimer);window.proofWake?.release()').catch(()=>{});
  for(const p of pending.values())clearTimeout(p.timer);socket.close();
}
