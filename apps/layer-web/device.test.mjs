// Run against an already forwarded Android Chrome endpoint. No profile reset,
// browser flags or device settings are changed by this harness.
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
  const tabs=await(await fetch(`${endpoint}/json/list`)).json();tab=tabs.find(t=>t.url===url);
  if(!tab)await new Promise(resolve=>setTimeout(resolve,100));
}
if(!tab)throw Error(`Open ${url} on the tablet first`);
const socket=new WebSocket(tab.webSocketDebuggerUrl);
await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
let sequence=0,onLoad;const pending=new Map(),errors=[];
socket.onmessage=event=>{
  const m=JSON.parse(event.data);
  if(m.id){const p=pending.get(m.id);if(!p)return;pending.delete(m.id);clearTimeout(p.timer);m.error?p.reject(Error(JSON.stringify(m.error))):p.resolve(m.result);}
  else if(m.method==="Page.loadEventFired"){onLoad?.();onLoad=null;}
  else if(m.method==="Page.javascriptDialogOpening" && m.params.type==="beforeunload"){call("Page.handleJavaScriptDialog",{accept:true}).catch(()=>{});}
  else if(m.method==="Runtime.exceptionThrown")errors.push(m.params.exceptionDetails.exception?.description||m.params.exceptionDetails.text);
  else if(m.method==="Log.entryAdded"&&m.params.entry.level==="error")errors.push(m.params.entry.text);
  else if(m.method==="Runtime.consoleAPICalled"&&m.params.type==="error")errors.push(m.params.args.map(a=>a.value||a.description).join(" "));
};
const call=(method,params={})=>new Promise((resolve,reject)=>{
  const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error(`CDP timeout: ${method}`));},60000);
  pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));
});
const evaluate=async expression=>{
  const result=await call("Runtime.evaluate",{expression,awaitPromise:true,returnByValue:true});
  if(result.exceptionDetails)throw Error(result.exceptionDetails.exception?.description||result.exceptionDetails.text);
  return result.result.value;
};
const reload=async()=>{const loaded=new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error("Navigation timed out")),20000);onLoad=()=>{clearTimeout(timer);resolve();};});await call("Page.reload",{ignoreCache:true});await loaded;};
const settle=()=>evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))');
const canvasPixels=async()=>{
  const shot=await call("Page.captureScreenshot",{format:"png"});
  return evaluate(`(async()=>{const image=new Image();image.src="data:image/png;base64,${shot.data}";await image.decode();const canvas=document.createElement("canvas");canvas.width=image.width;canvas.height=image.height;const ctx=canvas.getContext("2d",{willReadFrequently:true});ctx.drawImage(image,0,0);const rgba=ctx.getImageData(0,0,canvas.width,canvas.height).data;let white=0;for(let i=0;i<rgba.length;i+=4)if(rgba[i]>245&&rgba[i+1]>245&&rgba[i+2]>245)white++;return{white,total:rgba.length/4};})()`);
};
try {
  for(const domain of ["Page","Runtime","Log"])await call(`${domain}.enable`);
  await call("Page.bringToFront");
  await reload();
  await evaluate('new Promise((resolve,reject)=>{const start=performance.now();function check(){if(window.layerApp?.startupTimes.complete!=null)resolve(true);else if(performance.now()-start>55000)reject(Error(document.querySelector("#gpu-notice").textContent));else setTimeout(check,100);}check();})');
  console.log("Tablet",await evaluate('(async()=>{const adapter=await navigator.gpu.requestAdapter();return{agent:navigator.userAgent,viewport:[innerWidth,innerHeight],gpu:{vendor:adapter.info.vendor,architecture:adapter.info.architecture,device:adapter.info.device,description:adapter.info.description},platform:await navigator.userAgentData?.getHighEntropyValues(["platform","model","architecture"])}})()'));
  if(process.argv.includes("--medium-tiles")) {
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
} finally {socket.close();for(const p of pending.values())clearTimeout(p.timer);}
