import {checkUiUpdates,checkSettingsUpdates} from "./ui-updates.test.mjs";
import {checkDrawingTabs,checkDrawingTabRecovery} from "./drawing-tabs.test.mjs";
import {checkDrawingTabsOffline} from "./drawing-tabs-offline.test.mjs";
import {measureHdr} from "./hdr-performance.test.mjs";
import {checkHdr} from "./hdr.test.mjs";
import {checkProof} from "./proof.test.mjs";
import {checkWorkspaceManager} from "./workspace-manager.test.mjs";
import {checkStagedStartup} from "./startup.test.mjs";
import {checkFilterPreviews} from "./filter-previews.test.mjs";
import {checkTitleBarFeedback} from "./title-bar-feedback.test.mjs";
import {checkIcons} from "./icons.test.mjs";
import {checkWorkspaceStore} from "./workspace-store.test.mjs";
import {checkLongPressDragging} from "./long-press-drag.test.mjs";
import {checkWorkspaceResize} from "./workspace-resize.test.mjs";
import {checkDeviceImagePlacement} from "./image-placement-device.test.mjs";
// Run against an already forwarded Android Chrome endpoint. No profile reset,
// browser flags or device settings are changed by this harness.
import {checkDrawerDragging,checkDrawerStyling,checkToolbarDrawerSwitching} from "./drawers.test.mjs";
import {checkEditor} from "./editor.test.mjs";
import {checkDeviceFullscreen} from "./fullscreen.test.mjs";
import {checkMediumTiles} from "./tiles.test.mjs";
import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";
const endpoint=process.env.LAYER_DEVICE_CDP||"http://127.0.0.1:9228";
const url=process.env.LAYER_WEB_URL||"http://127.0.0.1:8127/";
let tab;
const openingDeadline=Date.now()+10000;
while(!tab && Date.now()<openingDeadline) {
  const tabs=await(await fetch(`${endpoint}/json/list`,{signal:AbortSignal.timeout(10000)})).json();tab=tabs.find(t=>t.url===url);
  if(!tab)await new Promise(resolve=>setTimeout(resolve,100));
}
if(!tab)throw Error(`Open ${url} on the tablet first`);
const socket=new WebSocket(tab.webSocketDebuggerUrl);
await new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(Error('Tablet Chrome connection timed out')),10000);socket.onopen=()=>{clearTimeout(timeout);resolve();};socket.onerror=e=>{clearTimeout(timeout);reject(e);};});
let sequence=0,onLoad;const pending=new Map(),errors=[];
socket.onmessage=event=>{
  const m=JSON.parse(event.data);
  if(m.id){const p=pending.get(m.id);if(!p)return;pending.delete(m.id);clearTimeout(p.timer);m.error?p.reject(Error(`${p.method}: ${JSON.stringify(m.error)}`)):p.resolve(m.result);}
  else if(m.method==="Page.loadEventFired"){onLoad?.();onLoad=null;}
  else if(m.method==="Page.javascriptDialogOpening" && m.params.type==="beforeunload"){call("Page.handleJavaScriptDialog",{accept:true}).catch(()=>{});}
  else if(m.method==="Runtime.exceptionThrown")errors.push(m.params.exceptionDetails.exception?.description||m.params.exceptionDetails.text);
  else if(m.method==="Log.entryAdded"&&m.params.entry.level==="error"&&!(process.argv.includes('--drawing-tabs-offline')&&m.params.entry.url?.includes('/__capy-tabs-offline-probe?')))errors.push(m.params.entry.text);
  else if(m.method==="Runtime.consoleAPICalled"&&m.params.type==="error")errors.push(m.params.args.map(a=>a.value||a.description).join(" "));
};
const call=(method,params={})=>new Promise((resolve,reject)=>{
  const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error(`CDP timeout: ${method}`));},process.argv.some(x=>['--drawing-tabs','--drawing-tabs-recovery','--drawing-tabs-offline'].includes(x))?300000:180000);
  pending.set(id,{resolve,reject,timer,method});socket.send(JSON.stringify({id,method,params}));
});
const evaluate=async expression=>{
  const result=await call("Runtime.evaluate",{expression,awaitPromise:true,returnByValue:true});
  if(result.exceptionDetails)throw Error(result.exceptionDetails.exception?.description||result.exceptionDetails.text);
  return result.result.value;
};
const reload=async()=>{const loaded=new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error("Navigation timed out")),20000);onLoad=()=>{clearTimeout(timer);resolve();};});await call("Page.reload",{ignoreCache:!process.argv.includes('--drawing-tabs-offline')});await loaded;};
const settle=()=>evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))');
const canvasPixels=async()=>{
  const shot=await call("Page.captureScreenshot",{format:"png"});
  return evaluate(`(async()=>{const image=new Image();image.src="data:image/png;base64,${shot.data}";await image.decode();const canvas=document.createElement("canvas");canvas.width=image.width;canvas.height=image.height;const ctx=canvas.getContext("2d",{willReadFrequently:true});ctx.drawImage(image,0,0);const rgba=ctx.getImageData(0,0,canvas.width,canvas.height).data;let white=0;for(let i=0;i<rgba.length;i+=4)if(rgba[i]>245&&rgba[i+1]>245&&rgba[i+2]>245)white++;return{white,total:rgba.length/4};})()`);
};
const workspaceIdle=()=>evaluate(`new Promise((resolve,reject)=>{const deadline=performance.now()+30000;function check(){const v=JSON.parse(layerApp.app.workspace_view());if(v?.ready&&!v.busy&&!v.dirty)resolve(v);else if(performance.now()>deadline)reject(Error(JSON.stringify(v)));else setTimeout(check,100);}check();})`);
const workspaceInput=value=>evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(value))});null`);
let workspaceIsolation;
try {
  for(const domain of ["Page","Runtime","Log"])await call(`${domain}.enable`);
  await call("Page.bringToFront");
  // Runtime.enable replays errors from the previous navigation. This run
  // validates the page loaded below, including any startup errors it produces.
  errors.length=0;
  // A previous automation-only drawing may not have sticky user activation.
  // Allow its ordinary beforeunload confirmation, which this harness accepts.
  await call("Runtime.evaluate",{expression:"void 0",userGesture:true});
  await reload();
  await evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(window.layerApp?.startupTimes.complete!=null)resolve(true);else if(performance.now()-start>${process.argv.some(x=>['--drawing-tabs','--drawing-tabs-recovery','--drawing-tabs-offline'].includes(x))?240000:55000})reject(Error(document.querySelector("#gpu-notice").textContent));else setTimeout(check,100);}check();})`);
  await workspaceIdle();
  if (process.argv.includes('--ui-speed') || process.argv.includes('--editor'))
    assert.equal(await evaluate("document.querySelectorAll('dialog[open]').length"),0,'Start with a clean fixture without recovery or other dialogs');
  if (['--ui-speed','--workspace-resize','--drawer-switch','--drawer-style','--drawer-drag','--long-press-drag','--medium-tiles'].some(flag=>process.argv.includes(flag))) {
    const original=(await workspaceIdle()).id;
    const capture=await evaluate('layerApp.app.workspace_capture()');
    await workspaceInput({type:'form',kind:'new'});
    await workspaceInput({type:'submit',name:`Tablet regression ${Date.now()}`});
    const created=(await workspaceIdle()).id; assert.notEqual(created,original);
    workspaceIsolation={original,created,capture};
  }
  console.log("Tablet",await evaluate('(async()=>{const adapter=await navigator.gpu.requestAdapter();return{agent:navigator.userAgent,viewport:[innerWidth,innerHeight],gpu:{vendor:adapter.info.vendor,architecture:adapter.info.architecture,device:adapter.info.device,description:adapter.info.description},platform:await navigator.userAgentData?.getHighEntropyValues(["platform","model","architecture"])}})()'));
  if (process.argv.includes("--ui-speed")) {
    await checkStagedStartup({call,evaluate,settle,canvasPixels,uiOnly:true});
    await checkUiUpdates({evaluate});
    await checkSettingsUpdates({evaluate,settle});
    await checkTitleBarFeedback({call,evaluate,settle});
    await checkWorkspaceResize({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--drawing-tabs-offline")) {
    await checkDrawingTabsOffline({call,evaluate,settle});assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--drawing-tabs-recovery")){
    await checkDrawingTabRecovery({call,evaluate,settle});assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--drawing-tabs")){
    await checkDrawingTabs({call,evaluate,settle});assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--hdr-performance")){
    await measureHdr({call,evaluate,settle});assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--hdr")){
    await checkHdr({call,evaluate,settle});assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--proof")){
    await checkProof({call,evaluate,settle});assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--filter-previews")) {
    await checkFilterPreviews({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--staged-startup")) {
    await checkStagedStartup({call,evaluate,settle,canvasPixels});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--image-placement")) {
    await checkDeviceImagePlacement({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--title-bar-feedback")) {
    await checkTitleBarFeedback({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--icons")) {
    await checkIcons({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--workspace-manager")) {
    await checkWorkspaceManager({call,evaluate,settle,reload,touch:true});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--workspace-store")) {
    await checkWorkspaceStore({evaluate});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--drawer-switch")) {
    await checkToolbarDrawerSwitching({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--drawer-style")) {
    await checkDrawerStyling({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--editor")) {
    await checkEditor({call,evaluate,settle,canvasPixels});
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--workspace-resize")) {
    await checkWorkspaceResize({call,evaluate,settle});
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--long-press-drag")) {
    await checkLongPressDragging({call,evaluate,settle});
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--drawer-drag")) {
    await checkDrawerDragging({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--medium-tiles")) {
    await checkMediumTiles({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--fullscreen")) {
    await checkDeviceFullscreen({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else {
  await checkEditor({call,evaluate,settle,canvasPixels});
  const before=await evaluate('layerApp.state().camera.zoom');
  const [cx,cy]=await evaluate('[innerWidth*.5,innerHeight*.5]');
  await call("Input.dispatchTouchEvent",{type:"touchStart",touchPoints:[{id:1,x:cx-60,y:cy},{id:2,x:cx+60,y:cy}]});
  for(let i=1;i<=12;i++){await call("Input.dispatchTouchEvent",{type:"touchMove",touchPoints:[{id:1,x:cx-60-i*4,y:cy-i*2},{id:2,x:cx+60+i*4,y:cy+i*2}]});await settle();}
  await call("Input.dispatchTouchEvent",{type:"touchEnd",touchPoints:[]});await settle();
  assert.ok(await evaluate('layerApp.state().camera.zoom')>before*1.4,"Tablet two-finger zoom changes the camera");
  assert.ok(Math.abs(await evaluate('layerApp.state().camera.rotation'))>.1,"Tablet two-finger rotation changes the camera");
  assert.deepEqual(errors,[]);
  const timings=[];
  for(let i=0;i<3;i++){
    // Fresh Wasm session each navigation; Chrome keeps its normal browser cache.
    await reload();
    timings.push(await evaluate('new Promise((resolve,reject)=>{const start=performance.now();function check(){if(window.layerApp?.startupTimes.complete!=null)resolve(layerApp.startupTimes);else if(performance.now()-start>55000)reject(Error("startup timeout"));else setTimeout(check,100);}check();})'));
  }
  const directory=process.env.LAYER_TEST_ARTIFACTS||"artifacts/web";
  await mkdir(directory,{recursive:true});await writeFile(`${directory}/tablet-startup.json`,JSON.stringify(timings,null,2));
  console.log("Tablet touch zoom/rotate and startup passed",timings);
  }
} finally {
  try { if(workspaceIsolation) {
    await workspaceInput({type:'cancel'}); await workspaceInput({type:'cancel'});
    await workspaceInput({type:'switch',id:workspaceIsolation.original}); await workspaceIdle();
    await workspaceInput({type:'form',kind:'delete',id:workspaceIsolation.created});
    await workspaceInput({type:'submit',name:''}); await workspaceIdle();
    const normalize=text=>JSON.stringify(JSON.parse(text),(key,value)=>key==='timestamp_ms'?'date':value);
    assert.equal(normalize(await evaluate('layerApp.app.workspace_capture()')),normalize(workspaceIsolation.capture),'The original workspace and its history remain intact');
  } } finally { socket.close();for(const p of pending.values())clearTimeout(p.timer); }
}
