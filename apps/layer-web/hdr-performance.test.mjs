import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
const exec=promisify(execFile);
const summary=values=>{const a=values.toSorted((a,b)=>a-b),p=q=>a[Math.floor((a.length-1)*q)];return a.length?{count:a.length,p50:p(.5),p95:p(.95),p99:p(.99),max:a.at(-1)}:null;};

// Real host input/frame/worker measurements; never call frame from this test.
// Process PSS includes Chrome workers and the GPU process where observable.
export async function measureHdr({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/color-m4-web-android/performance';await mkdir(directory,{recursive:true});
  const report={browser:await evaluate('navigator.userAgent'),runs:[],errors:[]};
  const persist=()=>writeFile(`${directory}/performance.json`,JSON.stringify(report,null,2)+'\n');
  const wait=c=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){if(${c})resolve(true);else if(performance.now()-start>150000)reject(Error(${JSON.stringify(c)}+': '+document.body.innerText.slice(-1000)));else setTimeout(poll,30)}poll()})`);
  const invoke=async command=>{await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);};
  const memory=async()=>{
    try {
      if(process.env.LAYER_DEVICE_SERIAL){const {stdout}=await exec(process.env.ADB||'adb',['-s',process.env.LAYER_DEVICE_SERIAL,'shell','dumpsys','meminfo']);const section=stdout.split('Total PSS by process:')[1]?.split('Total PSS by OOM adjustment:')[0]??'';const detail=[...section.matchAll(/([\d,]+)K:\s+(com\.android\.chrome[^\s]*) \(pid (\d+)/g)].map(m=>({id:Number(m[3]),type:m[2],pss_bytes:Number(m[1].replaceAll(',',''))*1024}));return{pss_bytes:detail.reduce((s,p)=>s+p.pss_bytes,0)||null,detail,source:'ADB all-process PSS; includes all Chrome tabs, isolated renderers and GPU',raw:section};}
      const {processInfo}=await call('SystemInfo.getProcessInfo',{},null);let pss=0,observed=0;const detail=[];
      for(const p of processInfo){try{const m=(await readFile(`/proc/${p.id}/smaps_rollup`,'utf8')).match(/^Pss:\s+(\d+)/m);if(m){const bytes=Number(m[1])*1024;pss+=bytes;observed++;detail.push({id:p.id,type:p.type,pss_bytes:bytes});}}catch{}}
      return{pss_bytes:pss||null,observed,processes:processInfo.length,detail,source:'Chrome SystemInfo PIDs /proc smaps_rollup'};
    }catch(e){return{unavailable:String(e)}}
  };
  await wait('layerApp.startupTimes.complete!==null&&layerApp.app.brush_ready()');
  await evaluate(`window.hdrPerf={originalOpen:showOpenFilePicker,originalSave:showSaveFilePicker,gaps:[],frames:[],inputs:[],last:performance.now()};
    hdrPerf.timer=setInterval(()=>{const n=performance.now();hdrPerf.gaps.push(n-hdrPerf.last);hdrPerf.last=n},16);
    hdrPerf.dismiss=setInterval(()=>[...document.querySelectorAll('dialog[open] button')].find(b=>['Keep for Later','Discard Changes'].includes(b.textContent))?.click(),30);
    window.showOpenFilePicker=async()=>[{name:hdrPerf.name,async getFile(){return new File([await(await fetch('/pkg/'+hdrPerf.name)).blob()],hdrPerf.name)}}];
    window.showSaveFilePicker=async()=>({name:'benchmark.capy',async createWritable(){return{async write(bytes){hdrPerf.savedBytes=bytes.byteLength??bytes.size},async close(){},async abort(){}}}});
    hdrPerf.frame=layerApp.app.frame.bind(layerApp.app);layerApp.app.frame=(...args)=>{const t=performance.now();try{return hdrPerf.frame(...args)}finally{hdrPerf.frames.push([args[0],t,performance.now()-t])}};
    hdrPerf.pointer=layerApp.app.input.bind(layerApp.app);layerApp.app.input=(...args)=>{const t=performance.now();try{return hdrPerf.pointer(...args)}finally{hdrPerf.inputs.push(performance.now()-t)}};`);
  const reset=()=>evaluate('hdrPerf.gaps=[];hdrPerf.frames=[];hdrPerf.inputs=[];hdrPerf.last=performance.now()');
  const diagnostics=()=>evaluate(`({gaps:hdrPerf.gaps,frames:hdrPerf.frames,inputs:hdrPerf.inputs,stats:JSON.parse(JSON.stringify(layerApp.app.renderer_stats(),(_,v)=>typeof v==='bigint'?Number(v):v))})`);
  const read=async()=>{const d=await diagnostics();return{heartbeat_ms:summary(d.gaps),submission_ms:summary(d.inputs),host_frame_ms:summary(d.frames.map(v=>v[2])),callback_ms:summary(d.frames.slice(1).map((v,i)=>v[0]-d.frames[i][0])),render_cpu_ms:summary(d.stats.samples),gpu_ms:summary(d.stats.gpu_samples),renderer_bytes:d.stats.resident_bytes,frames:d.frames,...await memory()};};
  async function motion(device){
    const p=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]*.5)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]*.5)*r.height/c.viewport[1]}})()`);
    const pointer=(type,x,y)=>device==='touch'?call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:21,x,y}]}):call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],x,y,button:'left',buttons:type==='up'?0:1,pointerType:device,force:type==='up'?0:.65});
    const toneBefore=await evaluate("JSON.parse(JSON.stringify(layerApp.app.tone_status(),(_,v)=>typeof v==='bigint'?Number(v):v))");
    await reset();await pointer('down',p.x,p.y);const start=performance.now(),pending=[];let failure;
    while(performance.now()-start<3000){const t=performance.now()-start;pending.push(pointer('move',p.x+40*Math.sin(t/250),p.y+20*Math.cos(t/310)).catch(e=>failure??=e));await new Promise(r=>setTimeout(r,8));}
    await Promise.all(pending);if(failure)throw failure;
    const held=await evaluate("JSON.parse(JSON.stringify(layerApp.app.tone_status(),(_,v)=>typeof v==='bigint'?Number(v):v))");
    if(toneBefore.hdr){assert.equal(held.retained,true);assert.equal(held.publications,toneBefore.publications);}
    await pointer('up',p.x,p.y);const released=performance.now();await settle();const drawing=await read();
    if(toneBefore.hdr)await wait('layerApp.app.tone_status().ready||layerApp.app.tone_status().error');
    const toneAfter=await evaluate("JSON.parse(JSON.stringify(layerApp.app.tone_status(),(_,v)=>typeof v==='bigint'?Number(v):v))");
    assert.equal(toneAfter.error??null,null);
    return{device,...drawing,guide_before:toneBefore,guide_during:held,guide_after:toneAfter,pen_up_guide_ms:performance.now()-released};
  }
  try {
    for(const name of (process.env.LAYER_HDR_WORKLOADS||'sparse4k,hdr24.png,hdr45.png,hdr60.png').split(',')){
      const entry={name,memory:[],interactions:[]};report.runs.push(entry);await persist();
      let sampling=false;const samples=setInterval(async()=>{if(sampling)return;sampling=true;try{entry.memory.push({at:Date.now(),...await memory()})}finally{sampling=false}},1000);
      try {
        await reset();if(process.env.LAYER_HDR_PROFILE){await call("Profiler.enable");await call("Profiler.start");}const start=performance.now();
        if(['sparse4k','sdr4k'].includes(name)){
          await invoke('new_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Bit depth"]')`);
          await evaluate(`(()=>{const d=document.querySelector('dialog[open]'),depth=d.querySelector('[aria-label="Bit depth"]');depth.value=${JSON.stringify(name==='sdr4k'?'U8':'F16')};depth.dispatchEvent(new Event('change'));for(const[label,v]of[['Width',3840],['Height',2160]])[...d.querySelectorAll('input')].find(n=>n.getAttribute('aria-label')?.startsWith(label)).value=v;[...d.querySelectorAll('button')].find(b=>b.textContent==='Create').click()})()`);
        }else{await evaluate(`hdrPerf.name=${JSON.stringify(name)}`);await invoke('open_document');}
        await wait('!layerApp.state().document_file.busy&&layerApp.app.brush_ready()');entry.open_ms=performance.now()-start;
        await wait('!layerApp.app.tone_status||!layerApp.app.tone_status().hdr||layerApp.app.tone_status().ready||layerApp.app.tone_status().error');entry.ready_ms=performance.now()-start;entry.tone=await evaluate(`JSON.parse(JSON.stringify(layerApp.app.tone_status?.()??{hdr:false},(_,v)=>typeof v==='bigint'?Number(v):v))`);assert.equal(entry.tone.error??null,null);entry.cold=await read();if(process.env.LAYER_HDR_PROFILE){const {profile}=await call("Profiler.stop");await writeFile(`${directory}/open-${name}.cpuprofile`,JSON.stringify(profile));await call("Profiler.disable");}
        if(process.env.LAYER_HDR_DIAGNOSTICS)await evaluate(`(()=>{layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'stats',visible:true}});layerApp.dispatch({type:'move_panel',panel:'stats',viewport:[innerWidth,innerHeight],target:{kind:'float',position:[innerWidth-310,90]}});})()`);
        await invoke('fit_canvas');await invoke('pen');await evaluate(`layerApp.dispatch({type:'select_brush',id:1});layerApp.dispatch({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'Srgb',rgba:[${name==='sdr4k'?.8:1.8},.3,.1,1]}}});`);
        for(let i=0;i<3;i++){entry.interactions.push(await motion(i===1?'touch':'pen'));await persist();}
        await wait('!layerApp.app.tone_status||!layerApp.app.tone_status().needed');
        // Actual in-progress histogram cancellation; heartbeat includes setup/readback.
        await reset();entry.histogram_cancel=await evaluate(`(async()=>{const c=layerApp.app.capture_control();const promise=layerApp.app.histogram(c);await new Promise(r=>setTimeout(r,40));const t=performance.now();c.cancel();try{await promise;return{completed_before_cancel:true}}catch(e){return{ms:performance.now()-t,error:String(e)}}finally{c.free()}})()`);entry.cancel_work=await read();
        // Save while a separate bounded analysis is running; immutable snapshots
        // and worker ownership must preserve the still-editable master.
        await reset();const saved=performance.now();await evaluate(`hdrPerf.c=layerApp.app.capture_control();hdrPerf.inspection=layerApp.app.histogram(hdrPerf.c).then(()=>null,e=>String(e)).finally(()=>hdrPerf.c.free())`);await invoke('save_document_as');await wait('!layerApp.state().document_file.busy');entry.save_ms=performance.now()-saved;entry.concurrent_histogram_error=await evaluate('hdrPerf.inspection');entry.saved_bytes=await evaluate('hdrPerf.savedBytes');entry.concurrent=await read();
        // Cancel actual visible Open and output-preview work, then verify the
        // existing saved document remains the active editable master.
        const epoch=await evaluate('Number(layerApp.state().document_file.epoch)');
        if(!['sparse4k','sdr4k'].includes(name)){
          await invoke('open_document');await wait(`!!document.querySelector('.file-progress button')`);
          await evaluate('new Promise(r=>setTimeout(r,40))');const t=performance.now();
          await evaluate(`document.querySelector('.file-progress button').click()`);await wait('!layerApp.state().document_file.busy');
          entry.open_cancel_ms=performance.now()-t;assert.equal(await evaluate('Number(layerApp.state().document_file.epoch)'),epoch);
        }
        await invoke('export_document');await wait(`!![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Preview Output'&&!b.disabled)`);
        await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Preview Output').click()`);
        await wait(`document.querySelector('dialog[open]').textContent.includes('Preparing complete output comparison')`);
        await evaluate('new Promise(r=>setTimeout(r,40))');const cancelStart=performance.now();
        await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Cancel').click()`);
        await wait('!layerApp.state().document_file.busy');entry.export_cancel_ms=performance.now()-cancelStart;
        assert.equal(await evaluate('Number(layerApp.state().document_file.epoch)'),epoch);
        assert.equal(await evaluate('layerApp.state().host_error??null'),null);
        console.log('HDR workload',name,JSON.stringify({open_ms:entry.open_ms,ready_ms:entry.ready_ms,submission:entry.interactions.map(x=>x.submission_ms),memory_peak:Math.max(...entry.memory.map(x=>x.pss_bytes||0)),cancel:entry.histogram_cancel}));
      }catch(e){entry.error=String(e);report.errors.push(`${name}: ${e}`);console.error(entry.error);break;}
      finally{clearInterval(samples);await persist();}
    }
  }finally{await persist();await evaluate('clearInterval(hdrPerf.timer);clearInterval(hdrPerf.dismiss);layerApp.app.frame=hdrPerf.frame;layerApp.app.input=hdrPerf.pointer;window.showOpenFilePicker=hdrPerf.originalOpen;window.showSaveFilePicker=hdrPerf.originalSave');}
  assert.deepEqual(report.errors,[]);
}
