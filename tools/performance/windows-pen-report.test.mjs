import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {analyze} from './windows-pen-report.mjs';
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'capy-pen-report-'));t.after(()=>{assert.ok(path.resolve(dir).startsWith(path.resolve(os.tmpdir())+path.sep));assert.ok(path.basename(dir).startsWith('capy-pen-report-'));fs.rmSync(dir,{recursive:true,force:true});});
 fs.writeFileSync(path.join(dir,'capture.json'),JSON.stringify({process_id:1,diameter:18,qpc_frequency:1000,surface:{window_id:9,present_mode:'Fifo'}}));
 const prefix=path.join(dir,'latency-9');
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
