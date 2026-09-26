import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {cpus} from 'node:os';
import {join} from 'node:path';

const summary=values=>{
  if(!values.length)return null;
  const v=values.slice().sort((a,b)=>a-b),p=q=>v[Math.round((v.length-1)*q)];
  return {count:v.length,p50:p(.5),p95:p(.95),max:v.at(-1)};
};
export const sourceIdentity=m=>m.tiled_sources.images.map(image=>({...image,tiles:image.tiles.map(t=>{const {offset,...blob}=m.blobs[t.blob];return {...t,blob};})}));
export const placementSave=({evaluate,invoke,idle})=>async()=>{await invoke('save_document_as');await idle();return evaluate(`(()=>{const b=placementTest.saved;return JSON.parse(new TextDecoder().decode(b.slice(52,52+Number(new DataView(b.buffer,b.byteOffset).getBigUint64(12,true)))));})()`);};

// Hardware WebGPU execution and host frame timings. CDP supplies real browser
// pointer input; these numbers do not claim physical pen-to-photon latency.
export async function measurePlacedPhotos({call,evaluate,settle,invoke,save,baseline,loadingMs,hardware,readMemory}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/image-placement/web-motion';await mkdir(directory,{recursive:true});
  const report={...hardware??{cpu:cpus()[0]?.model,gpu:(await call('SystemInfo.getInfo',{},null)).gpu.devices},loading_ms:loadingMs,runs:[]};
  const sources=sourceIdentity(baseline),layers=baseline.document.layers.slice(0,sources.length);
  let profileIndex=0;
  const rasterIdentity=m=>m.rasters.map(r=>({...r,tiles:r.tiles.map(t=>({...t,blob:m.blobs[t.blob].digest}))}));
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
    for(let index=0;index<layers.length;index++)for(const factor of [1.1,1.2,2]) {
      for(let i=0;i<layers.length;i++)await send({type:'set_layer_visibility',id:layers[i].id,visible:i===index});
      await send({type:'layer',action:{op:'select',id:layers[index].id,mask:false}});
      await invoke('scale_rotate');
      const [w,h]=sources[index].extent,scale=Math.min(1,2000/w,1500/h)*factor;
      await send({type:'set_tool_setting',id:'transform_width',value:scale});
      const entry={extent:[w,h],factor,translation:[],drawing:null};
      for(const device of ['mouse','touch','pen'])entry.translation.push(await motion(device,device==='pen'?'measure':'input'));
      console.log('Translation diagnostics',JSON.stringify({...entry,translation:entry.translation.map(({frames,...summary})=>summary)}));
      assert.ok(entry.translation.every(run=>run.host_frame_ms?.count>0),'Every pointer device moves placement');
      await invoke('apply_transform');await invoke('pen');await send({type:'select_brush',id:1});
      await send({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'Srgb',rgba:[1,0,.7,.5]}}});
      entry.drawing=await motion('pen','measure');
      const painted=await save();assert.deepEqual(sourceIdentity(painted),sources,'Drawing never changes original source samples');
      assert.ok(painted.blobs.length>baseline.blobs.length,'Drawing records layer-local paint');
      await invoke('undo');const undone=await save();await invoke('redo');const redone=await save();
      assert.notDeepEqual(rasterIdentity(undone),rasterIdentity(redone),'Undo removes the drawing');assert.deepEqual(rasterIdentity(painted),rasterIdentity(redone));
      report.runs.push(entry);await writeFile(join(directory,'motion.json'),JSON.stringify(report,null,2));
      console.log(`Photo ${w}×${h} at ${factor}× fit: translation GPU ${JSON.stringify(entry.translation.at(-1).gpu_ms)}, drawing GPU ${JSON.stringify(entry.drawing.gpu_ms)}`);
    }
    // Keep both large sources visible for the final drawing qualification.
    if(layers.length>1){
      for(const layer of layers)await send({type:'set_layer_visibility',id:layer.id,visible:true});
      await send({type:'layer',action:{op:'select',id:layers[0].id,mask:false}});await invoke('pen');
      report.two_layers=await motion('pen','measure');assert.deepEqual(sourceIdentity(await save()),sources);
    }
    await writeFile(join(directory,'motion.json'),JSON.stringify(report,null,2));
  } finally {await writeFile(join(directory,'motion.json'),JSON.stringify(report,null,2));}
}
