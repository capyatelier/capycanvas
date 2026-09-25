import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";
import {dirname} from "node:path";

export async function benchRaster({evaluate,settle}) {
  await evaluate(`new Promise(resolve=>{function check(){if(layerApp.startupTimes.complete!==null)resolve();else setTimeout(check,25);}check();})`);
  if(process.argv.includes("--large-raster")) {
    await evaluate(`layerApp.dispatch({type:'invoke',command:'new_document'});`);
    await evaluate(`new Promise(resolve=>{function check(){if(document.querySelector('.document-dialog input'))resolve();else setTimeout(check,20);}check();})`);
    await evaluate(`[...document.querySelectorAll('.document-dialog input[type=number]')].slice(0,2).forEach((input,i)=>input.value=i?4000:6000);[...document.querySelectorAll('.document-dialog button')].find(button=>button.textContent==='Create').click();`);
    await evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(!layerApp.state().document_file.busy && layerApp.state().tabs[0].width===6000 && layerApp.app.brush_ready())resolve();else if(performance.now()-start>30000)reject(Error('Large canvas preparation failed'));else setTimeout(check,20);}check();})`);
  }
  // Native frame entry points and real WebGPU work; the only synthetic part is
  // pen delivery. Browser wall time includes the host's presentation encoding.
  const results=await evaluate(`(async()=>{
    const app=layerApp.app,delay=()=>new Promise(resolve=>setTimeout(resolve,0));
    const quantile=(values,p)=>values.slice().sort((a,b)=>a-b)[Math.min(values.length-1,Math.floor(values.length*p))];
    const results=[];let id=800;
    for(let run=0;run<3;run++){
      if(app.save_recovery)navigator.locks.request("capy-raster:benchmark-"+run,()=>new Promise(()=>{}));
      const cpu=[],up=[],turns=[];let save=null;
      for(let contact=0;contact<5;contact++){
        id++;const camera=app.camera(),view=camera.revision;
        const point=(phase,i)=>[id,phase,440+i*1.1,350+contact*50+Math.sin(i*.06)*20,.6,0,0,0,performance.now(),0,0];
        app.pen(point(1,0),view);app.frame(performance.now(),performance.now()+8.333);
        for(let i=1;i<=192;i++){
          const turn=performance.now();app.pen(point(2,i),view);
          const start=performance.now();app.frame(start,start+8.333);const elapsed=performance.now()-start;
          if(contact>0)cpu.push(elapsed);
          await delay();if(contact>0)turns.push(performance.now()-turn);
          if(contact===2&&i===20&&app.save_recovery){const start=performance.now();save=app.save_recovery('benchmark-'+run).then(()=>performance.now()-start);}
        }
        app.pen(point(3,192),view);const start=performance.now();app.frame(start,start+8.333);up.push(performance.now()-start);
        await new Promise(resolve=>setTimeout(resolve,30));
      }
      results.push({run,frames:cpu.length,cpu_p50:quantile(cpu,.5),cpu_p95:quantile(cpu,.95),cpu_p99:quantile(cpu,.99),cpu_max:Math.max(...cpu),up_p99:quantile(up,.99),turn_p99:quantile(turns,.99),save_ms:save?await save:null});
    }
    return results;
  })()`);
  console.log('Raster frame creation benchmark',JSON.stringify(results));
  const output=process.env.LAYER_RASTER_BENCH_OUTPUT||'artifacts/color-m1/web-frame-times.json';
  await mkdir(dirname(output),{recursive:true});await writeFile(output,JSON.stringify(results,null,2)+'\n');
  assert.ok(results.every(r=>r.frames===768));
  await settle();
}
