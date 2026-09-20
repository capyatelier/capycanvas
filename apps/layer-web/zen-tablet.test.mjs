// Uses only the explicitly selected test tab; leaves other Chrome tabs untouched.
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {checkZen} from './zen.test.mjs';

const [tabId,directory='artifacts/zen-huion/web']=process.argv.slice(2);
assert.ok(tabId,'Usage: node zen-tablet.test.mjs TEST_TAB_ID [OUTPUT_DIRECTORY]');
const endpoint=process.env.LAYER_CDP_URL||'http://127.0.0.1:9240';
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
function call(method,params={}){return new Promise((resolve,reject)=>{const id=++next,timer=setTimeout(()=>{pending.delete(id);reject(Error(`Timed out: ${method}`));},120000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});}
async function evaluate(expression){
  if(process.env.LAYER_TEST_TRACE)console.log(expression.slice(0,220));
  try {
    const r=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});
    if(r.exceptionDetails)throw Error(r.exceptionDetails.exception?.description||r.exceptionDetails.text);
    return r.result.value;
  } catch(error) {throw Error(`${expression.slice(0,220)}: ${error.message}`);}
}
const settle=()=>evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))');
try {
  await mkdir(directory,{recursive:true});
  await call('Runtime.enable');await call('Page.enable');await call('Page.bringToFront');
  const deadline=Date.now()+90000;
  for (;;) {
    try {
      if(await evaluate("!!window.layerApp&&document.body.dataset.gpu==='ready'&&layerApp.app.brush_ready()&&JSON.parse(layerApp.app.workspace_view()).ready"))break;
    }catch(error){if(!/navigated|closed|context/i.test(error.message))throw error;}
    if(Date.now()>deadline)throw Error('Startup timeout');
    await new Promise(r=>setTimeout(r,250));
  }
  // A previous disposable test may have left recovery data. Preserve it while
  // dismissing the recovery prompt so it cannot pin Zen controls open.
  await evaluate(`window.zenTestRecovery=setInterval(()=>{const d=[...document.querySelectorAll('dialog[open]')].find(d=>d.querySelector('h2')?.textContent==='Recover drawing?');if(d)[...d.querySelectorAll('button')].find(b=>b.textContent==='Keep for Later')?.click()},100)`);
  for(let i=0;i<50;i++) {
    const dismissed=await evaluate(`(()=>{const dialog=[...document.querySelectorAll('dialog[open]')].find(d=>d.querySelector('h2')?.textContent==='Recover drawing?');const button=dialog&&[...dialog.querySelectorAll('button')].find(b=>b.textContent==='Keep for Later');button?.click();return !!button})()`);
    await settle();
    if(!dismissed)break;
  }
  const device=await evaluate('({userAgent:navigator.userAgent,viewport:[innerWidth,innerHeight],dpr:devicePixelRatio})');
  console.log('Device:',JSON.stringify(device));
  await evaluate(`window.zenTestTrace=[];window.zenTestEvents=new AbortController();for(const type of ['pointerdown','pointerup','pointermove','pointercancel','click'])window.addEventListener(type,e=>{const row={type,pointer:e.pointerType,x:e.clientX,y:e.clientY,target:e.target.id||e.target.tagName};setTimeout(()=>{row.hidden=document.querySelector('#workspace').classList.contains('zen-hidden');row.prevented=e.defaultPrevented;zenTestTrace.push(row);if(zenTestTrace.length>120)zenTestTrace.shift()},0)},{capture:true,signal:zenTestEvents.signal})`);
  await checkZen({call,evaluate,settle,capture:async name=>{
    const shot=await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});
    await writeFile(join(directory,`${name}.png`),Buffer.from(shot.data,'base64'));
  }});
  assert.deepEqual(errors,[],'No browser runtime exceptions');
  await writeFile(join(directory,'result.json'),JSON.stringify({result:'PASS',device},null,2));
}catch(error){
  const trace=await evaluate('window.zenTestTrace').catch(()=>null);
  await writeFile(join(directory,'failure.json'),JSON.stringify({error:String(error),trace},null,2));
  throw error;
}finally{
  await evaluate('clearInterval(window.zenTestRecovery);delete window.zenTestRecovery;window.zenTestEvents?.abort();delete window.zenTestEvents;delete window.zenTestTrace').catch(()=>{});
  for(const p of pending.values())clearTimeout(p.timer);socket.close();
}
