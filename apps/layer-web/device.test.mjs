import {checkTonalSelections} from './tonal-selection.test.mjs';
import {checkBinaryTransfer} from './binary-transfer.test.mjs';
import {checkColorPicker} from './color-picker.test.mjs';
import {checkColorPanel} from "./color-panel.test.mjs";
import {checkToolbarComponents} from './toolbar-components.test.mjs';
import {checkSelectionTools} from "./selection-tools.test.mjs";
import {checkFilterDrawer} from "./filter-drawer.test.mjs";
import {checkBrushDrawers} from "./brush-drawers.test.mjs";
import {checkStrokeRecording} from './stroke-recording.test.mjs';
import {checkContactBrushes} from "./contact-brushes.test.mjs";
import {checkDrawingTabs,checkDrawingTabRecovery} from "./drawing-tabs.test.mjs";
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
import {checkLayerHolding} from "./layer-hold.test.mjs";
import {checkWorkspaceResize} from "./workspace-resize.test.mjs";
import {checkDeviceImagePlacement} from "./image-placement-device.test.mjs";
import {checkPenRendering} from "./pen-rendering.test.mjs";
import {checkPrediction} from "./prediction.test.mjs";
// Run against an already forwarded Android Chrome endpoint. No profile reset,
// browser flags or device settings are changed by this harness.
import {checkDrawerDragging,checkDrawerStyling,checkToolbarDrawerSwitching} from "./drawers.test.mjs";
import {checkEditor} from "./editor.test.mjs";
import {checkDeviceFullscreen} from "./fullscreen.test.mjs";
import {checkMediumTiles} from "./tiles.test.mjs";
import {checkPaintColumns} from "./paint-columns.test.mjs";
import {checkPalettes} from "./palettes.test.mjs";
import {checkZen} from "./zen.test.mjs";
import {benchPhotoNavigation} from "./photo-navigation-bench.test.mjs";
import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";
import {join} from "node:path";
import {connectTab} from "../../tools/cdp.mjs";
const endpoint=process.env.LAYER_DEVICE_CDP||"http://127.0.0.1:9228";
const url=process.env.LAYER_WEB_URL||"http://127.0.0.1:8127/";
const directory=process.env.LAYER_TEST_ARTIFACTS||"artifacts/web";
const cdp=await connectTab(endpoint,t=>t.url===url,{
  timeout:process.argv.some(x=>['--drawing-tabs','--drawing-tabs-recovery'].includes(x))?300000:180000,
  onEvent:m=>{
    if(m.method==="Log.entryAdded"&&m.params.entry.level==="error")cdp.report(m.params.entry.text);
    else if(m.method==="Runtime.consoleAPICalled"&&m.params.type==="error")cdp.report(m.params.args.map(a=>a.value||a.description).join(" "));
  },
});
const {call,evaluate,settle,errors}=cdp;
const reload=async()=>{const loaded=cdp.once("Page.loadEventFired",60000);await call("Page.reload",{ignoreCache:true});await loaded;};
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
  await evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(window.layerApp?.startupTimes.complete!=null)resolve(true);else if(performance.now()-start>${process.argv.some(x=>['--drawing-tabs','--drawing-tabs-recovery'].includes(x))?240000:55000})reject(Error(document.querySelector("#gpu-notice").textContent));else setTimeout(check,100);}check();})`);
  await workspaceIdle();
  if (process.argv.some(flag=>['--selection-tools','--tonal-selection','--color-panel','--color-picker','--paint-columns','--palettes','--zen','--proof-performance','--proof-memory'].includes(flag))) {
    // Recovery discovery can finish after startup and workspace switching.
    // Keep drawings available without letting a late prompt swallow test input.
    await evaluate(`(()=>{const keep=()=>[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();window.deviceRecoveryWatcher=new MutationObserver(keep);window.deviceRecoveryWatcher.observe(document.body,{childList:true,subtree:true,attributes:true,attributeFilter:['open']});keep();})()`);
  }
  if (process.argv.includes('--editor'))
    assert.equal(await evaluate("document.querySelectorAll('dialog[open]').length"),0,'Start with a clean fixture without recovery or other dialogs');
  if (['--workspace-resize','--drawer-switch','--drawer-style','--drawer-drag','--long-press-drag','--layer-hold','--medium-tiles','--color-panel'].some(flag=>process.argv.includes(flag))) {
    const original=(await workspaceIdle()).id;
    const capture=await evaluate('layerApp.app.workspace_capture()');
    const theme=await evaluate('layerApp.state().settings.theme ?? null');
    if(process.argv.includes('--color-panel')){await workspaceInput({type:'switch',id:'builtin:workspace:photographer'});await workspaceIdle();}
    await workspaceInput({type:'form',kind:'new'});
    await workspaceInput({type:'submit',name:`Tablet regression ${Date.now()}`});
    const created=(await workspaceIdle()).id; assert.notEqual(created,original);
    workspaceIsolation={original,created,capture,theme};
  }
  console.log("Tablet",await evaluate('(async()=>{const adapter=await navigator.gpu.requestAdapter();return{agent:navigator.userAgent,viewport:[innerWidth,innerHeight],gpu:{vendor:adapter.info.vendor,architecture:adapter.info.architecture,device:adapter.info.device,description:adapter.info.description},platform:await navigator.userAgentData?.getHighEntropyValues(["platform","model","architecture"])}})()'));
  if (process.argv.includes("--binary-transfer")) {
    await checkBinaryTransfer({evaluate},process.env.LAYER_BINARY_FIXTURE_URL); assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--tonal-selection")) {
    await checkTonalSelections({call,evaluate,settle}); assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--toolbar-components")) {
    await checkToolbarComponents({call,evaluate,settle}); assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--filter-drawer")) {
    await checkFilterDrawer({call,evaluate,settle});assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--stroke-recording")) {
    await checkStrokeRecording({call,evaluate,settle}); assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--contact-brushes")) {
    await checkContactBrushes({call,evaluate,settle},process.env.LAYER_BRUSH_PHOTO_URL);
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--pen")) {
    await checkPenRendering({call,evaluate,settle});
    await checkPrediction({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--drawing-tabs-recovery")){
    await checkDrawingTabRecovery({call,evaluate,settle});assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--drawing-tabs")){
    await checkDrawingTabs({call,evaluate,settle});assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--hdr-performance")){
    await measureHdr({call,evaluate,settle});assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--hdr")){
    await checkHdr({call,evaluate,settle});assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--proof")){
    await checkProof({call,evaluate,settle},{profileUrl:process.env.LAYER_PROOF_URL,originalUrl:process.env.LAYER_PROOF_ORIGINAL_URL});assert.deepEqual(errors,[]);
  } else if(process.argv.some(x=>['--proof-performance','--proof-memory'].includes(x))){
    const performanceRun=process.argv.includes('--proof-performance');
    const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const started=performance.now();function poll(){if(${condition})resolve();else if(performance.now()-started>120000)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,30)}poll()})`);
    await mkdir(directory,{recursive:true});
    await wait('layerApp.state().commands.find(c=>c.id==="open_document")?.enabled && !document.querySelector("dialog[open]")');
    const photo=process.env.LAYER_PHOTO_URL||'/pkg/proof-photo61mp.jpg',profile=process.env.LAYER_PROOF_URL||'/pkg/proof-cmyk.icc';
    await evaluate(`(async()=>{window.proofBench={open:window.showOpenFilePicker};const response=await fetch(${JSON.stringify(photo)});if(!response.ok)throw Error('Photo fixture unavailable');const blob=await response.blob();window.showOpenFilePicker=async()=>[{name:'proof-photo.jpg',async getFile(){return new File([blob],'proof-photo.jpg')}}];proofBench.importStart=performance.now();layerApp.dispatch({type:'invoke',command:'open_document'});})()`);
    await settle();await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')?.click()`);
    await wait('layerApp.state().tabs[0].width===9504 && !layerApp.state().document_file.busy && layerApp.app.brush_ready()');
    await evaluate('proofBench.importMs=performance.now()-proofBench.importStart;window.showOpenFilePicker=proofBench.open');
    if(performanceRun)await benchPhotoNavigation({call,evaluate},join(directory,'web-photo-normal.json'));
    await evaluate(`(async()=>{proofBench.profile=await layerApp.app.profile_library('import',undefined,new Uint8Array(await(await fetch(${JSON.stringify(profile)})).arrayBuffer()));layerApp.dispatch({type:'invoke',command:'soft_proof_setup'});})()`);
    await wait(`!![...document.querySelectorAll('dialog[open] optgroup[label="Saved Profiles"] option')].find(o=>o.textContent===proofBench.profile.name)`);
    await evaluate(`(()=>{const s=document.querySelector('dialog[open] select[aria-label="Proof profile"]');s.value=[...s.querySelectorAll('optgroup[label="Saved Profiles"] option')].find(o=>o.textContent===proofBench.profile.name).value;s.dispatchEvent(new Event('change'));proofBench.frames=[];proofBench.longTasks=[];proofBench.preparing=true;const tick=t=>{proofBench.frames.push(t);if(proofBench.preparing)requestAnimationFrame(tick)};requestAnimationFrame(tick);proofBench.observer=new PerformanceObserver(list=>{proofBench.longTasks.push(...list.getEntries().map(e=>({start:e.startTime,duration:e.duration})))});proofBench.observer.observe({type:'longtask'});proofBench.prepareStart=performance.now();})()`);
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Apply'&&!b.disabled).click()`);
    await wait(`!document.querySelector('dialog[open]') && layerApp.app.proof_status().text.startsWith('Proof:')`);await settle();
    const preparation=await evaluate(`(()=>{proofBench.preparing=false;proofBench.observer.disconnect();return JSON.parse(JSON.stringify({import_ms:proofBench.importMs,preparation_ms:performance.now()-proofBench.prepareStart,raf:proofBench.frames,long_tasks:proofBench.longTasks,proof:layerApp.app.proof_status(),color:layerApp.app.document_color(),memory:performance.memory?{used:performance.memory.usedJSHeapSize,total:performance.memory.totalJSHeapSize}:null},(_,v)=>typeof v==='bigint'?Number(v):v))})()`);
    await writeFile(join(directory,'web-proof-preparation.json'),JSON.stringify(preparation,null,2));console.log('Preparation:',JSON.stringify(preparation));
    if(performanceRun)await benchPhotoNavigation({call,evaluate},join(directory,'web-photo-proof.json'));
    else await evaluate('new Promise(r=>setTimeout(r,12000))');
    const shot=await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(join(directory,'web-photo-proof.png'),Buffer.from(shot.data,'base64'));
    assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--zen")){
    await mkdir(directory,{recursive:true});
    await checkZen({call,evaluate,settle,capture:async name=>{
      const shot=await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});
      await writeFile(join(directory,`${name}.png`),Buffer.from(shot.data,'base64'));
    }});
    assert.deepEqual(errors,[]);
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
  } else if (process.argv.includes("--color-panel")) {
    await checkColorPanel({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--selection-tools")) {
    await checkSelectionTools({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--color-picker")) {
    await checkColorPicker({call,evaluate,settle});
    assert.deepEqual(errors,[]);
  } else if (process.argv.includes("--brush-drawers")) {
    await checkBrushDrawers({call,evaluate,settle});
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
  } else if (process.argv.includes("--layer-hold")) {
    await checkLayerHolding({call,evaluate,settle});
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
  } else if(process.argv.includes("--palettes")) {
    await checkPalettes({call,evaluate,settle,reload});
    assert.deepEqual(errors,[]);
  } else if(process.argv.includes("--paint-columns")) {
    await checkPaintColumns({call,evaluate,settle});
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
  await mkdir(directory,{recursive:true});await writeFile(`${directory}/tablet-startup.json`,JSON.stringify(timings,null,2));
  console.log("Tablet touch zoom/rotate and startup passed",timings);
  }
} finally {
  try { if(workspaceIsolation) {
    await workspaceInput({type:'cancel'}); await workspaceInput({type:'cancel'});
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(workspaceIsolation.theme)}})`);
    await workspaceInput({type:'switch',id:workspaceIsolation.original}); await workspaceIdle();
    await workspaceInput({type:'form',kind:'delete',id:workspaceIsolation.created});
    await workspaceInput({type:'submit',name:''}); await workspaceIdle();
    const normalize=text=>JSON.stringify(JSON.parse(text),(key,value)=>key==='timestamp_ms'?'date':value);
    assert.equal(normalize(await evaluate('layerApp.app.workspace_capture()')),normalize(workspaceIsolation.capture),'The original workspace and its history remain intact');
  } } finally { await evaluate("window.deviceRecoveryWatcher?.disconnect();delete window.deviceRecoveryWatcher").catch(()=>{});await cdp.close(); }
}
