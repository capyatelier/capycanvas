import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {cpus} from 'node:os';
import {join} from 'node:path';

const summary=values=>{
  if(!values.length)return null;
  const v=values.slice().sort((a,b)=>a-b),p=q=>v[Math.round((v.length-1)*q)];
  return {count:v.length,p50:p(.5),p95:p(.95),max:v.at(-1)};
};
import {readPackage,packageObject,packageObjects,packageResources,rasterIdentity,imageIdentity} from './package-fixture.test.mjs';
export {imageIdentity} from './package-fixture.test.mjs';
export const placementSave=({evaluate,invoke,idle})=>async()=>{await invoke('save_document_as');await idle();return readPackage(evaluate,'placementTest.saved');};

// Hardware WebGPU execution and host frame timings. CDP supplies real browser
// pointer input; these numbers do not claim physical pen-to-photon latency.
export async function measurePlacedPhotos({call,evaluate,settle,invoke,save,baseline,loadingMs,hardware,readMemory}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/image-placement/web-motion';await mkdir(directory,{recursive:true});
  const report={...hardware??{cpu:cpus()[0]?.model,gpu:(await call('SystemInfo.getInfo',{},null)).gpu.devices},loading_ms:loadingMs,runs:[]};
  const sources=imageIdentity(baseline),objects=packageObjects(baseline,'capy.image-object/1');
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const layer=(await state()).layers.find(l=>l.object_count===objects.length).id;
  let profileIndex=0;
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const screen=async(x,y)=>evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(${x}*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(${y}*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
  const pointer=async(type,p,device='pen')=>{
    if(device==='touch')return call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseMoved:'touchMove',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:91,...p}]});
    return call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:device,force:type==='mouseReleased'?0:.65});
  };
  async function memory() {
    if(readMemory)return readMemory();
    const {processInfo}=await call('SystemInfo.getProcessInfo',{},null);let pss=0;
    for(const p of processInfo){try{const m=(await readFile(`/proc/${p.id}/smaps_rollup`,'utf8')).match(/^Pss:\s+(\d+)/m);pss+=Number(m?.[1]??0)*1024;}catch{}}
    return {chrome_pss_bytes:pss,wasm_js_heap:await evaluate('performance.memory?.usedJSHeapSize??null')};
  }
  async function motion(device,kind) {
    const start=await screen(1000,750);
    const duration=kind==='measure'?5000:1000;
    const profile=process.env.LAYER_IMAGE_PROFILE && device==='pen';
    if(profile){await call('Profiler.enable');await call('Profiler.start');}
    await evaluate(`placementTest.frames=[];placementTest.frame=layerApp.app.frame.bind(layerApp.app);layerApp.app.frame=(...args)=>{const t=performance.now();try{return placementTest.frame(...args)}finally{placementTest.frames.push([args[0],t,performance.now()-t])}}`);
    try {
      const started=performance.now();let inputs=0,error;
      const delivered=[];
      await pointer('mousePressed',start,device);
      while(performance.now()-started<duration){
        const elapsed=performance.now()-started;
        // CDP touch acknowledgements can wait for a compositor frame. Keep the
        // injection clock independent, as a physical device's input clock is.
        delivered.push(pointer('mouseMoved',{x:start.x+40*Math.sin(elapsed/250),y:start.y+20*Math.cos(elapsed/310)},device).catch(e=>{error??=e;}));
        inputs++;await new Promise(resolve=>setTimeout(resolve,4));
      }
      await Promise.all(delivered);if(error)throw error;
      await pointer('mouseReleased',start,device);await settle();
      const result=await evaluate(`({host:placementTest.frames,stats:JSON.parse(JSON.stringify(layerApp.app.renderer_stats(),(_,v)=>typeof v==='bigint'?Number(v):v))})`);
      // The normal browser animation callback owns rendering throughout input.
      // Keep its full timeline; a rolling GPU history can contain older work.
      return {device,inputs,frames:result.host,frame_fields:['animation_ms','start_ms','cpu_frame_ms'],
        host_frame_ms:summary(result.host.map(f=>f[2])),
        callback_interval_ms:summary(result.host.slice(1).map((f,i)=>f[0]-result.host[i][0])),
        gpu_ms:summary(result.stats.gpu_samples),render_cpu_ms:summary(result.stats.samples),tracked_canvas_bytes:result.stats.resident_bytes,...await memory()};
    } finally {
      await evaluate('layerApp.app.frame=placementTest.frame;delete placementTest.frame');
      if(profile){
        const {profile}=await call('Profiler.stop');await writeFile(join(directory,`motion-${profileIndex++}.cpuprofile`),JSON.stringify(profile));await call('Profiler.disable');
      }
    }
  }
  try {
    await send({type:'customize',action:{type:'set_panel_visible',panel:'stats',visible:true}});
    const statsGroup=await evaluate("layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('stats')).id");
    await send({type:'customize',action:{type:'set_column_collapsed',group:statsGroup,collapsed:false}});
    await send({type:'select_panel_tab',group:statsGroup,panel:'stats'});
    await invoke('fit_canvas');
    await send({type:'object',action:{op:'expand',layer,expanded:true}});
    const rows=(await state()).layers.find(l=>l.id===layer).objects;
    for(let index=0;index<objects.length;index++)for(const size of ['fit','original']) {
      for(const row of rows)await send({type:'object',action:{op:'visibility',id:row.id,visible:row.label===objects[index].data.name}});
      const row=rows.find(r=>r.label===objects[index].data.name);
      await invoke('move');await send({type:'object',action:{op:'select',id:row.id,extend:false}});
      if(size==='original')await invoke('placement_original_size');
      const extent=packageObject(baseline,objects[index].data.image).data.extent;
      const entry={extent,size,translation:[],drawing:null};
      for(const device of ['mouse','touch','pen'])entry.translation.push(await motion(device,device==='pen'?'measure':'input'));
      console.log('Translation diagnostics',JSON.stringify({...entry,translation:entry.translation.map(({frames,...summary})=>summary)}));
      assert.ok(entry.translation.every(run=>run.host_frame_ms?.count>0),'Every pointer device moves the image');
      await invoke('add_layer');await invoke('pen');await send({type:'select_brush',id:1});
      await send({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'Srgb',rgba:[1,0,.7,.5]}}});
      entry.drawing=await motion('pen','measure');
      const painted=await save();assert.deepEqual(imageIdentity(painted),sources,'Drawing never changes image samples');
      assert.ok(packageResources(painted,'capy.raster-tile/1').length>packageResources(baseline,'capy.raster-tile/1').length,'Drawing records layer-local paint');
      await invoke('undo');const undone=await save();await invoke('redo');const redone=await save();
      assert.notDeepEqual(rasterIdentity(undone),rasterIdentity(redone),'Undo removes the drawing');assert.deepEqual(rasterIdentity(painted),rasterIdentity(redone));
      await invoke('undo');await invoke('undo');
      report.runs.push(entry);await writeFile(join(directory,'motion.json'),JSON.stringify(report,null,2));
      console.log(`Image ${extent[0]}×${extent[1]} at ${size} size: translation GPU ${JSON.stringify(entry.translation.at(-1).gpu_ms)}, drawing GPU ${JSON.stringify(entry.drawing.gpu_ms)}`);
    }
    for(const row of rows)await send({type:'object',action:{op:'visibility',id:row.id,visible:true}});
    if(objects.length>1){
      await invoke('add_layer');await invoke('pen');
      report.two_images=await motion('pen','measure');assert.deepEqual(imageIdentity(await save()),sources);
    }
    await writeFile(join(directory,'motion.json'),JSON.stringify(report,null,2));
  } finally {await writeFile(join(directory,'motion.json'),JSON.stringify(report,null,2));}
}
