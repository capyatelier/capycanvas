import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {analyze} from './windows-pen-report.mjs';
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'capy-pen-report-'));t.after(()=>{assert.ok(path.resolve(dir).startsWith(path.resolve(os.tmpdir())+path.sep));assert.ok(path.basename(dir).startsWith('capy-pen-report-'));fs.rmSync(dir,{recursive:true,force:true});});
 fs.writeFileSync(path.join(dir,'capture.json'),JSON.stringify({process_id:1,diameter:18,qpc_frequency:1000,surface:{window_id:9,present_mode:'Fifo'}}));
 const prefix=path.join(dir,'latency-1-9');
 fs.writeFileSync(prefix+'-status.json','{"overflow":false}');
 fs.writeFileSync(prefix+'-input.csv','sequence,sample_ns,arrival_ns,phase\n1,95000000,96000000,1\n2,105000000,106000000,2\n');
 fs.writeFileSync(prefix+'-consumed.csv','sequence,frame\n1,1\n2,2\n');
 fs.writeFileSync(prefix+'-frames.csv','frame,acquire_start_ns,acquired_ns,render_start_ns,render_end_ns,last_present,displayed_present,present_refresh,sync_refresh,sync_qpc,stats_error\n1,97000000,99000000,100000000,105000000,1,0,0,0,0,-1\n2,107000000,109000000,110000000,115000000,2,1,11,11,110,0\n3,117000000,119000000,120000000,125000000,3,2,12,12,120,0\n');
 return {dir,prefix};
}
test('correlates input with its own displayed present ID, including delayed statistics',t=>{
 const {dir}=fixture(t),r=analyze(dir);
 assert.equal(r.input_to_dxgi_display_ms.p50_ms,15);
 assert.equal(r.input_to_dxgi_display_ms.count,2);
 assert.equal(r.delivery_ms.p50_ms,1);
 assert.equal(r.owner_queue_ms.p50_ms,4);
 assert.equal(r.host_frame_ms.p50_ms,5);
 assert.equal(r.refresh_period_ms,10);
});
test('does not substitute later displayed frames for missing presents',t=>{
 const {dir,prefix}=fixture(t);
 fs.writeFileSync(prefix+'-frames.csv',fs.readFileSync(prefix+'-frames.csv','utf8').replace('3,2,12,12,120,0','3,3,12,12,120,0'));
 assert.equal(analyze(dir).display_matched_inputs,1);
});
test('rejects overflow and mismatched input clock domains',t=>{
 const {dir,prefix}=fixture(t);
 fs.writeFileSync(prefix+'-status.json','{"overflow":true}');assert.throws(()=>analyze(dir),/overflow/);
 fs.writeFileSync(prefix+'-status.json','{"overflow":false}');
 fs.writeFileSync(prefix+'-input.csv',fs.readFileSync(prefix+'-input.csv','utf8').replace('95000000','950000000'));
 assert.throws(()=>analyze(dir),/timestamp/);
});
test('rejects a renderer that stopped consuming pen input',t=>{
 const {dir,prefix}=fixture(t);
 fs.writeFileSync(prefix+'-consumed.csv','sequence,frame\n1,1\n');
 assert.throws(()=>analyze(dir),/Incomplete pen consumption/);
});

test('uses observation bounds instead of refresh-start timestamps for immediate flips',t=>{
 const {dir}=fixture(t);
 const meta=JSON.parse(fs.readFileSync(path.join(dir,'capture.json'),'utf8'));
 meta.surface.present_mode='Immediate';fs.writeFileSync(path.join(dir,'capture.json'),JSON.stringify(meta));
 const r=analyze(dir);
 assert.equal(r.input_to_dxgi_display_ms,null);
 assert.equal(r.refresh_timestamp_applicable,false);
 assert.equal(r.input_to_presentation_observation_bound_ms.p50_ms,22);
 assert.equal(r.observation_matched_inputs,1); // final query has no subsequent timestamp
});

test('rejects consumed input without a new present',t=>{
 const {dir,prefix}=fixture(t);
 fs.writeFileSync(prefix+'-frames.csv',fs.readFileSync(prefix+'-frames.csv','utf8').replace('115000000,2,1','115000000,1,1'));
 assert.throws(()=>analyze(dir),/no new present/);
});

test('uses recorded injection QPC despite quantized Windows pointer timestamps',t=>{
 const {dir,prefix}=fixture(t);
 fs.writeFileSync(path.join(dir,'injected.csv'),'index,qpc_before,qpc_after,x,y\n0,95,95.2,1,1\n1,105,105.2,2,2\n');
 fs.writeFileSync(prefix+'-input.csv',fs.readFileSync(prefix+'-input.csv','utf8').replace('95000000','96150000'));
 const r=analyze(dir);
 assert.equal(r.delivery_ms.p50_ms,1);
 assert.equal(r.input_to_dxgi_display_ms.p50_ms,15);
 assert.match(r.timestamp_origin,/QPC/);
 assert.equal(r.pointer_timestamp_offset_ms.max,1.15);
});

test('rejects missing injections and mismatched injection-to-pointer correspondence',t=>{
 const {dir}=fixture(t),file=path.join(dir,'injected.csv');
 fs.writeFileSync(file,'index,qpc_before,qpc_after\n0,95,95.2\n');
 assert.throws(()=>analyze(dir),/count mismatch/);
 fs.writeFileSync(file,'index,qpc_before,qpc_after\n0,95,95.2\n1,99,99.2\n');
 assert.throws(()=>analyze(dir),/correspondence mismatch/);
});

function navigationFixture(t, {netZero = false, missingPresent = false, secondsByContact = [1,6,6,6]} = {}) {
 const f=fixture(t),meta=JSON.parse(fs.readFileSync(path.join(f.dir,'capture.json'),'utf8'));
 meta.rate_hz=2;meta.surface.present_mode='Immediate';meta.navigation={mode:'hand',ui_trace_enabled:false,contacts:[]};
 const inputs=[],injected=[],consumed=[],frames=[];
 let index=0,frame=0,present=0,time=95;
 for(let contact=0;contact<secondsByContact.length;contact++){
  const seconds=secondsByContact[contact],count=seconds*meta.rate_hz+1,first=index;
  meta.navigation.contacts.push({first_index:first,count,seconds,measured:contact>0});
  for(let i=0;i<count;i++,index++){
   const at=time+i*500,x=netZero&&contact===1&&i<=3?(i===1?1:0):Math.min(i,count-2);
   injected.push(`${index},${at},${at+.2},${x},0`);
   inputs.push(`${index+1},${at*1e6},${(at+1)*1e6},${i===0?1:i===count-1?3:2}`);
   if(netZero&&contact===1&&i===2){
    consumed.push(`${index+1},${frame}`);
    frames[frames.length-1]=`${frame},${(at+2)*1e6},${(at+3)*1e6},${(at+4)*1e6},${(at+5)*1e6},${present},0,0,0,0,-1`;
   }else{
    frame++;
    if(i>0&&i<count-1&&!(missingPresent&&contact===1&&i===3))present++;
    consumed.push(`${index+1},${frame}`);
    frames.push(`${frame},${(at+2)*1e6},${(at+3)*1e6},${(at+4)*1e6},${(at+5)*1e6},${present},0,0,0,0,-1`);
   }
  }
  time+=seconds*1000+50000;
 }
 fs.writeFileSync(path.join(f.dir,'capture.json'),JSON.stringify(meta));
 fs.writeFileSync(path.join(f.dir,'injected.csv'),'index,qpc_before,qpc_after,x,y\n'+injected.join('\n')+'\n');
 fs.writeFileSync(f.prefix+'-input.csv','sequence,sample_ns,arrival_ns,phase\n'+inputs.join('\n')+'\n');
 fs.writeFileSync(f.prefix+'-consumed.csv','sequence,frame\n'+consumed.join('\n')+'\n');
 fs.writeFileSync(f.prefix+'-frames.csv','frame,acquire_start_ns,acquired_ns,render_start_ns,render_end_ns,last_present,displayed_present,present_refresh,sync_refresh,sync_qpc,stats_error\n'+frames.join('\n')+'\n');
 return f;
}
test('navigation keeps no-op Down/Up observations without relaxing paint submission guards',t=>{
 const {dir}=navigationFixture(t),r=analyze(dir);
 assert.deepEqual(r.navigation.no_new_present_inputs,{down:4,move:0,up:4});
 assert.equal(r.navigation.true_presented_fps,null);assert.equal(r.navigation.target_met,null);
 assert.equal(r.navigation.contacts[0].distinct_submitted_frames,0);
 const file=path.join(dir,'capture.json'),meta=JSON.parse(fs.readFileSync(file,'utf8'));delete meta.navigation;
 fs.writeFileSync(file,JSON.stringify(meta));assert.throws(()=>analyze(dir),/no new present/);
});
test('navigation excludes duplicate and net-zero input frames without discarding their counts',t=>{
 const {dir}=navigationFixture(t,{netZero:true}),r=analyze(dir);
 assert.equal(r.inputs,42);assert.equal(r.consumed,42);
 assert.equal(r.navigation.contacts[1].distinct_submitted_frames,8);
 assert.equal(r.navigation.contacts[2].distinct_submitted_frames,11);
});
test('navigation preserves no-submission Move evidence and gaps between submitted frames',t=>{
 const {dir}=navigationFixture(t,{missingPresent:true}),r=analyze(dir);
 assert.equal(r.navigation.no_new_present_inputs.move,1);
 assert.equal(r.navigation.contacts[1].distinct_submitted_frames,10);
 assert.equal(r.navigation.contacts[1].frame_intervals_ms.max_ms,1000);
});
test('navigation never joins timing intervals across priming or separated contacts',t=>{
 const {dir}=navigationFixture(t),r=analyze(dir);
 assert.equal(r.navigation.renderer_frame_intervals_ms.count,30);
 assert.equal(r.navigation.renderer_frame_intervals_ms.max_ms,500);
 assert.equal(r.navigation.renderer_frames_per_second,2);
 assert.equal(r.navigation.contacts[1].actual_seconds,6);
});
test('navigation still rejects lost consumption and missing injected samples',t=>{
 const {dir,prefix}=navigationFixture(t),file=prefix+'-consumed.csv',original=fs.readFileSync(file,'utf8');
 fs.writeFileSync(file,original.replace('2,2\n',''));assert.throws(()=>analyze(dir),/Incomplete pen consumption/);
 fs.writeFileSync(file,original);
 const injected=path.join(dir,'injected.csv');fs.writeFileSync(injected,fs.readFileSync(injected,'utf8').split('\n').slice(0,-2).join('\n')+'\n');
 assert.throws(()=>analyze(dir),/count mismatch/);
});

test('navigation rejects reordered consumption even when all input IDs are present',t=>{
 const {dir,prefix}=navigationFixture(t),file=prefix+'-consumed.csv';
 fs.writeFileSync(file,fs.readFileSync(file,'utf8').replace('1,1\n2,2\n','2,2\n1,1\n'));
 assert.throws(()=>analyze(dir),/consumption order/);
});

test('Zoom ignores vertical-only injected movement rather than reporting it as zoom frames',t=>{
 const {dir}=navigationFixture(t),metaFile=path.join(dir,'capture.json'),meta=JSON.parse(fs.readFileSync(metaFile,'utf8'));
 meta.navigation.mode='zoom';fs.writeFileSync(metaFile,JSON.stringify(meta));
 const file=path.join(dir,'injected.csv'),lines=fs.readFileSync(file,'utf8').trim().split('\n');
 fs.writeFileSync(file,lines.map((line,i)=>{if(!i)return line;const parts=line.split(',');parts[4]=parts[3];parts[3]='0';return parts.join(',');}).join('\n')+'\n');
 assert.throws(()=>analyze(dir),/Insufficient distinct submitted/);
});

test('navigation rejects enabled, missing or non-boolean UI-trace metadata',t=>{
 const {dir}=navigationFixture(t),file=path.join(dir,'capture.json'),meta=JSON.parse(fs.readFileSync(file,'utf8'));
 for(const value of [true,undefined,'false']){
  meta.navigation.ui_trace_enabled=value;fs.writeFileSync(file,JSON.stringify(meta));
  assert.throws(()=>analyze(dir),/explicit disabled UI trace metadata/);
 }
});
test('navigation rejects a completed but partial two-motion capture',t=>{
 const {dir}=navigationFixture(t,{secondsByContact:[1,6,6]});
 assert.throws(()=>analyze(dir),/Incomplete navigation series/);
});
test('navigation rejects a short measured contact or invalid priming contact',t=>{
 for(const secondsByContact of [[1,4,6,6],[.5,6,6,6]]){
  const {dir}=navigationFixture(t,{secondsByContact});
  assert.throws(()=>analyze(dir),/Incomplete navigation series/);
 }
 const {dir}=navigationFixture(t),file=path.join(dir,'capture.json'),meta=JSON.parse(fs.readFileSync(file,'utf8'));
 meta.navigation.contacts[0].measured=true;fs.writeFileSync(file,JSON.stringify(meta));
 assert.throws(()=>analyze(dir),/Incomplete navigation series/);
});

function objectFixture(t, mode='move') {
 const f=navigationFixture(t),file=path.join(f.dir,'capture.json'),meta=JSON.parse(fs.readFileSync(file,'utf8'));
 meta.object_motion={...meta.navigation,mode,preflight_sha256:'A'.repeat(64)};delete meta.navigation;
 meta.object_motion.contacts=meta.object_motion.contacts.map(c=>({...c,artwork_changed:true,package_validated:true,restored:true}));
 fs.writeFileSync(file,JSON.stringify(meta));return {...f,file,meta};
}
test('all four Object workloads reuse strict contact timing and preserve inference limits',t=>{
 for(const mode of ['move','scale','rotate','placement']){
  const {dir}=objectFixture(t,mode),r=analyze(dir);
  assert.equal(r.object_motion.mode,mode);assert.equal(r.navigation,undefined);
  assert.equal(r.object_motion.contacts.length,4);assert.equal(r.object_motion.renderer_frame_intervals_ms.count,30);
  assert.equal(r.object_motion.true_presented_fps,null);assert.equal(r.object_motion.target_met,null);
  assert.match(r.object_motion.motion_inference,/per-frame object affine/);
  assert.deepEqual(r.object_motion.no_new_present_inputs,{down:4,move:0,up:4});
 }
});
test('Object motion requires every priming and measured contact to prove its artwork and restoration',t=>{
 const {dir,file,meta}=objectFixture(t);
 for(const field of ['artwork_changed','package_validated','restored'])for(const index of [0,1,2,3]){
  const m=structuredClone(meta);m.object_motion.contacts[index][field]=false;fs.writeFileSync(file,JSON.stringify(m));
  assert.throws(()=>analyze(dir),/lacks artwork, package or restoration/);
 }
});
test('Object motion rejects unknown or mixed workloads and missing preflight identity',t=>{
 const {dir,file,meta}=objectFixture(t);
 for(const change of [m=>m.object_motion.mode='hand',m=>delete m.object_motion.preflight_sha256,m=>m.navigation=m.object_motion]){
  const m=structuredClone(meta);change(m);fs.writeFileSync(file,JSON.stringify(m));
  assert.throws(()=>analyze(dir),/qualification|Ambiguous/);
 }
});
test('Object motion rejects short contacts and enabled UI tracing',t=>{
 const {dir,file,meta}=objectFixture(t);
 for(const change of [m=>m.object_motion.contacts[1].seconds=4,m=>m.object_motion.ui_trace_enabled=true]){
  const m=structuredClone(meta);change(m);fs.writeFileSync(file,JSON.stringify(m));
  assert.throws(()=>analyze(dir),/Incomplete navigation series|disabled UI trace/);
 }
});
test('Object motion rejects stationary injected coordinates and lost consumption',t=>{
 const {dir,prefix}=objectFixture(t),file=path.join(dir,'injected.csv'),original=fs.readFileSync(file,'utf8');
 fs.writeFileSync(file,original.trim().split('\n').map((l,i)=>i?l.split(',').slice(0,3).join(',')+',0,0':l).join('\n')+'\n');
 assert.throws(()=>analyze(dir),/Insufficient distinct submitted/);fs.writeFileSync(file,original);
 const consumed=prefix+'-consumed.csv';fs.writeFileSync(consumed,fs.readFileSync(consumed,'utf8').replace('2,2\n',''));
 assert.throws(()=>analyze(dir),/Incomplete pen consumption/);
});
