// Analyze bounded host timing and DXGI statistics, never label it photon latency.
import fs from 'node:fs';
import path from 'node:path';
export function distribution(values) {
  const v=values.filter(Number.isFinite).sort((a,b)=>a-b);
  if(!v.length)return null;
  const p=q=>v[Math.floor(q*(v.length-1))];
  return {count:v.length,mean_ms:v.reduce((a,b)=>a+b,0)/v.length,p50_ms:p(.5),p95_ms:p(.95),p99_ms:p(.99),max_ms:v.at(-1)};
}
function csv(file){const rows=fs.readFileSync(file,'utf8').trim().split(/\r?\n/);const keys=rows.shift().split(',');return rows.filter(Boolean).map(line=>Object.fromEntries(line.split(',').map((v,i)=>[keys[i],Number(v)])));}
export function analyze(directory){
 const meta=JSON.parse(fs.readFileSync(path.join(directory,'capture.json'),'utf8').replace(/^\uFEFF/,''));
 const prefix=path.join(directory,`latency-${meta.surface.window_id}`);
 if(JSON.parse(fs.readFileSync(prefix+'-status.json','utf8')).overflow)throw Error('Trace overflow; reject this run');
 const inputs=csv(prefix+'-input.csv'),consumed=csv(prefix+'-consumed.csv'),frames=csv(prefix+'-frames.csv');
 if(!inputs.length||!consumed.length)throw Error('No pen input reached the render owner');
 if(inputs.length!==consumed.length||new Set(consumed.map(p=>p.sequence)).size!==inputs.length)throw Error('Incomplete pen consumption; reject this run');
 const bySequence=new Map(inputs.map(p=>[p.sequence,p])),byFrame=new Map(frames.map(f=>[f.frame,f]));
 const actual=frames.filter(f=>f.stats_error===0&&f.displayed_present>0&&f.sync_qpc>0);
 const samples=new Map();for(const f of actual)samples.set(f.sync_refresh,f.sync_qpc);
 const sync=[...samples].sort((a,b)=>a[0]-b[0]);
 const periods=sync.slice(1).map(([r,t],i)=>(t-sync[i][1])*1000/meta.qpc_frequency/(r-sync[i][0]));
 const period=distribution(periods)?.p50_ms;
 const validPeriod=period>4&&period<50;
 const displayed=new Map();
 if(validPeriod)for(const f of actual){
  const time=f.sync_qpc*1000/meta.qpc_frequency+(f.present_refresh-f.sync_refresh)*period;
  // Keep the earliest report for each presented frame; never substitute a
  // later frame for an unobserved/dropped one or assume every submit displays.
  if(!displayed.has(f.displayed_present))displayed.set(f.displayed_present,time);
 }
 const delivery=[],queue=[],submit=[],display=[],latest=new Map();let unmatched=0;
 for(const c of consumed){const p=bySequence.get(c.sequence),f=byFrame.get(c.frame);if(!p||!f){unmatched++;continue;}
  delivery.push((p.arrival_ns-p.sample_ns)/1e6);queue.push((f.render_start_ns-p.arrival_ns)/1e6);submit.push((f.render_end_ns-p.sample_ns)/1e6);
  if(!latest.has(c.frame)||latest.get(c.frame).sample_ns<p.sample_ns)latest.set(c.frame,p);
  const shown=displayed.get(f.last_present);if(shown!==undefined)display.push(shown-p.sample_ns/1e6);
 }
 if(unmatched)throw Error('Consumed pen input has no completed host frame');
 if(delivery.some(v=>v<-.1)||queue.some(v=>v<0)||display.some(v=>v<-.1))throw Error('Inconsistent timestamp domains or presentation mapping');
 const active=frames.filter(f=>latest.has(f.frame));
 return {scope:'OS-injected pen to DXGI-reported presentation, excluding physical digitizer, scanout position and panel response',
  process_id:meta.process_id,diameter:meta.diameter,present_mode:meta.surface.present_mode,inputs:inputs.length,consumed:consumed.length,unmatched,
  stats_errors:[...new Set(frames.map(f=>f.stats_error))],valid_stats:actual.length,observed_displayed_frames:displayed.size,
  refresh_period_ms:validPeriod?period:null,delivery_ms:distribution(delivery),owner_queue_ms:distribution(queue),input_to_frame_return_ms:distribution(submit),
  host_frame_ms:distribution(active.map(f=>(f.render_end_ns-f.render_start_ns)/1e6)),
  acquire_wait_ms:distribution(active.map(f=>(f.acquired_ns-f.acquire_start_ns)/1e6)),
  input_to_dxgi_display_ms:distribution(display),
  newest_input_to_dxgi_display_ms:distribution(active.flatMap(f=>{const t=displayed.get(f.last_present);return t===undefined?[]:[t-latest.get(f.frame).sample_ns/1e6];})),
  display_matched_inputs:display.length};
}
if(process.argv[1]&&path.resolve(process.argv[1])===path.resolve(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/,'$1'))){
 const result=analyze(process.argv[2]);fs.writeFileSync(path.join(process.argv[2],'summary.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result,null,2));
}
