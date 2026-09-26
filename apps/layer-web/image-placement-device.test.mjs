import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {measurePlacedPhotos,placementSave,sourceIdentity} from './image-placement-motion.test.mjs';

// Tablet Chrome cannot select desktop paths. Fetch the original encoded files
// from the test server, then use the normal picker/document request controller.
// Pointer input still goes through Chrome's native CDP input implementation.
export async function checkDeviceImagePlacement({call,evaluate,settle}) {
  const urls=JSON.parse(process.env.LAYER_PHOTO_URLS??'[]');
  assert.equal(urls.length,2,'Supply the 24 MP and 61 MP original URLs');
  const wait=async condition=>{const deadline=Date.now()+180000;while(!await evaluate(`!!(${condition})`)){
    if(Date.now()>deadline)throw Error(condition+': '+await evaluate('document.body.innerText.slice(-1000)'));
    await new Promise(resolve=>setTimeout(resolve,100));
  }};
  const invoke=async command=>{await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);await settle();};
  const idle=()=>wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');
  const placed=()=>wait('layerApp.state().commands.find(c=>c.id==="placement_original_size").enabled');
  const save=placementSave({evaluate,invoke,idle});
  const press=async id=>{
    await wait(`(n=>n && n.getAttribute('aria-disabled')!=='true' && !n.closest('.canvas-action-bar.suppressed'))(document.querySelector('.canvas-action-bar [data-command=${id}]'))`);
    const p=await evaluate(`(()=>{const b=document.querySelector('.canvas-action-bar [data-command=${id}]'),r=b.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:92,...p}]});
    await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();
  };
  await evaluate(`window.placementTest={open:window.showOpenFilePicker,save:window.showSaveFilePicker,files:[]};
    window.showOpenFilePicker=async()=>placementTest.files.map(file=>({getFile:async()=>file}));
    window.showSaveFilePicker=async o=>({name:o.suggestedName,async createWritable(){return{async write(b){placementTest.saved=new Uint8Array(b instanceof Blob?await b.arrayBuffer():b)},async close(){},async abort(){}}}});
    layerApp.dispatch({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}});`);
  try {
    for(const url of urls)await evaluate(`(async()=>{const response=await fetch(${JSON.stringify(url)});if(!response.ok)throw Error('Missing original');placementTest.files.push(new File([await response.blob()],${JSON.stringify(url.split('/').at(-1))},{type:'image/jpeg'}))})()`);
    await invoke('new_document');await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')?.click()`);
    await wait('document.querySelector("dialog[open] input[type=number]")');
    await evaluate(`{const f=document.querySelectorAll('dialog[open] input[type=number]');f[0].value=2000;f[1].value=1500;[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create').click();}`);await idle();
    const base=await evaluate('layerApp.state().layers.length');
    await invoke('import_image');await idle();await placed();await press('cancel_transform');
    assert.equal(await evaluate('layerApp.state().layers.length'),base);
    const start=Date.now();await invoke('import_image');await idle();await placed();await press('apply_transform');const loadingMs=Date.now()-start;
    const baseline=await save(),sources=sourceIdentity(baseline);
    for(let i=0;i<sources.length;i++){
      const [w,h]=sources[i].extent,pose=baseline.document.layers[i].properties.placement;
      assert.ok(Math.abs(pose[0]-Math.min(1,2000/w,1500/h))<1e-6);
    }
    await invoke('undo');assert.equal(await evaluate('layerApp.state().layers.length'),base);
    await invoke('redo');assert.equal(await evaluate('layerApp.state().layers.length'),base+2);
    await evaluate(`placementTest.photos=placementTest.files;placementTest.files=[new File([placementTest.saved],'tablet-placement.capy')]`);
    await invoke('open_document');await idle();assert.deepEqual(sourceIdentity(await save()),sources);
    await invoke('scale_rotate');await press('placement_original_size');await press('apply_transform');
    assert.equal((await save()).document.layers[0].properties.placement[0],1);
    await evaluate('placementTest.files=placementTest.photos');
    const hardware=await evaluate(`(async()=>{const a=await navigator.gpu.requestAdapter();return{agent:navigator.userAgent,platform:await navigator.userAgentData?.getHighEntropyValues(['model','architecture','platform']),gpu:{vendor:a.info.vendor,architecture:a.info.architecture,description:a.info.description},viewport:[innerWidth,innerHeight],device_memory_gib:navigator.deviceMemory}})()`);
    const readMemory=async()=>{
      const result={wasm_js_heap:await evaluate('performance.memory?.usedJSHeapSize??null')};
      if(process.env.CAPY_ANDROID_SERIAL){
        const {stdout}=await promisify(execFile)(process.env.ADB??'adb',['-s',process.env.CAPY_ANDROID_SERIAL,'shell','dumpsys','meminfo','--package','com.android.chrome']);
        result.chrome_package_pss_bytes=[...stdout.matchAll(/TOTAL PSS:\s+(\d+)/g)].reduce((sum,m)=>sum+Number(m[1])*1024,0);
      }
      return result;
    };
    await measurePlacedPhotos({call,evaluate,settle,invoke,save,baseline,loadingMs,hardware,readMemory});
    console.log('Tablet Chrome original-file batch placement, touch controls, fit, history, reopen, Original Size and clipped photo motion passed');
  } finally {await evaluate('window.showOpenFilePicker=placementTest.open;window.showSaveFilePicker=placementTest.save;delete window.placementTest');}
}
