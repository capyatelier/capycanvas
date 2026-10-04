import test from 'node:test';
import assert from 'node:assert/strict';
import {createRasterWorker} from './raster-worker-client.js';

function fixture() {
  const original=Object.fromEntries(['Worker','setTimeout','clearTimeout','setInterval','clearInterval'].map(k=>[k,globalThis[k]]));
  let now=0,next=0;const timers=new Map(),workers=[];
  const schedule=(callback,delay,interval=false)=>{const id=++next;timers.set(id,{callback,at:now+delay,interval:interval?delay:0});return id;};
  globalThis.setTimeout=(f,d)=>schedule(f,d);globalThis.setInterval=(f,d)=>schedule(f,d,true);
  globalThis.clearTimeout=globalThis.clearInterval=id=>timers.delete(id);
  globalThis.Worker=class {
    constructor(){this.messages=[];this.terminated=false;workers.push(this);}
    postMessage(message){assert.equal(this.terminated,false);if(this.postError)throw this.postError;this.messages.push(message);}
    terminate(){this.terminated=true;}
    reply(result,extra={}){this.onmessage({data:{id:this.messages.at(-1).id,result,...extra}});}
    error(){this.onerror({preventDefault(){},message:'worker failed'});}
  };
  const run=createRasterWorker();
  return {run,workers,async flush(){for(let i=0;i<4;i++)await Promise.resolve();},
    async advance(delay){const end=now+delay;while(true){const ready=[...timers].filter(([,t])=>t.at<=end).sort((a,b)=>a[1].at-b[1].at)[0];if(!ready)break;const [id,t]=ready;now=t.at;if(!t.interval)timers.delete(id);t.callback();if(t.interval&&timers.has(id))t.at+=t.interval;await this.flush();}now=end;},
    close(){for(const [k,v]of Object.entries(original))globalThis[k]=v;}};
}
async function withFixture(body){const f=fixture();try{await body(f);}finally{f.close();}}
const request=(operation='snapshot',extra={})=>({operation,metadata:'{}',buffers:[],...extra});

test('successful idle analysis worker is reused and expires after five seconds',()=>withFixture(async f=>{
  const first=f.run(request());f.workers[0].reply('first');assert.equal(await first,'first');assert.equal(f.workers[0].terminated,false);
  await f.advance(4999);const second=f.run(request());assert.equal(f.workers.length,1);f.workers[0].reply('second');assert.equal(await second,'second');
  await f.advance(5000);assert.equal(f.workers[0].terminated,true);
  const third=f.run(request());assert.equal(f.workers.length,2);f.workers[1].reply('third');assert.equal(await third,'third');
}));

test('concurrent analysis jobs have separate ownership and retain at most one idle worker',()=>withFixture(async f=>{
  const first=f.run(request()),second=f.run(request());assert.equal(f.workers.length,2);
  f.workers[1].reply('second');assert.equal(await second,'second');assert.equal(f.workers[0].terminated,false);
  f.workers[0].reply('first');assert.equal(await first,'first');assert.equal(f.workers.filter(w=>!w.terminated).length,1);
}));

test('analysis cancellation retires only its worker and ignores late replies',()=>withFixture(async f=>{
  let cancelled=false;const first=f.run(request('snapshot',{cancelled:()=>cancelled}));const rejected=assert.rejects(first,{name:'AbortError'});
  const second=f.run(request());cancelled=true;await f.advance(50);await rejected;assert.equal(f.workers[0].terminated,true);assert.equal(f.workers[1].terminated,false);
  f.workers[0].reply('late');f.workers[1].reply('second');assert.equal(await second,'second');
  const third=f.run(request());assert.equal(f.workers.length,2);f.workers[1].reply('third');assert.equal(await third,'third');
}));

test('failed oversized and timed out analysis workers are never reused',()=>withFixture(async f=>{
  for(const reason of ['reply','crash','oversized','timeout']) {
    const index=f.workers.length;const pending=f.run(request());const worker=f.workers.at(-1);
    const result=reason==='oversized'?pending:assert.rejects(pending);
    if(reason==='reply')worker.reply(undefined,{error:'bad result'});else if(reason==='crash')worker.error();else if(reason==='oversized')worker.reply('large',{retire:true});else await f.advance(180000);
    await result;assert.equal(worker.terminated,true,reason);
    const next=f.run(request());assert.equal(f.workers.length,index+2,reason);f.workers.at(-1).reply('small',{retire:true});await next;
  }
}));

test('analysis cancellation leaves an independent output job usable',()=>withFixture(async f=>{
  const output=f.run(request('output-begin'));f.workers[0].reply('owned-output');assert.equal(await output,'owned-output');
  let cancelled=false;const analysis=f.run(request('snapshot',{cancelled:()=>cancelled}));const rejected=assert.rejects(analysis,{name:'AbortError'});cancelled=true;await f.advance(50);await rejected;assert.equal(f.workers[0].terminated,false);
  const encoded=f.run(request('output-encode',{metadata:JSON.stringify({token:'owned-output'})}));assert.equal(f.workers.length,2);assert.equal(f.workers[0].messages.at(-1).request.operation,'output-encode');f.workers[0].reply('bytes');assert.equal(await encoded,'bytes');
  const closed=f.run(request('output-close',{metadata:'owned-output'}));f.workers[0].reply(true);assert.equal(await closed,true);assert.equal(f.workers[0].terminated,true);
}));

test('failed postMessage preserves its error and never retires another active job',()=>withFixture(async f=>{
  const warm=f.run(request());f.workers[0].reply('warm');await warm;
  const error=new Error('transfer rejected');f.workers[0].postError=error;
  await assert.rejects(f.run(request()),e=>e===error);assert.equal(f.workers[0].terminated,true);
  const active=f.run(request());const failed=f.run(request());const rejection=assert.rejects(failed,{message:'worker failed'});
  f.workers[2].error();await rejection;assert.equal(f.workers[1].terminated,false);
  f.workers[1].reply('active');assert.equal(await active,'active');
}));

test('already cancelled analysis sends nothing and preserves a parallel output owner',()=>withFixture(async f=>{
  const output=f.run(request('output-begin'));f.workers[0].reply('output');await output;
  await assert.rejects(f.run(request('snapshot',{cancelled:()=>true})),{name:'AbortError'});
  assert.equal(f.workers[1].messages.length,0);assert.equal(f.workers[1].terminated,true);
  assert.equal(f.workers[0].terminated,false);
  const encoded=f.run(request('output-encode',{metadata:JSON.stringify({token:'output'})}));
  f.workers[0].reply('encoded');assert.equal(await encoded,'encoded');
  const next=f.run(request());assert.equal(f.workers.length,3);f.workers[2].reply('next');assert.equal(await next,'next');
}));


test('manual save retains its output owner until publication completes',()=>withFixture(async f=>{
  const pending=f.run(request('write')),worker=f.workers[0];worker.reply({token:'archive',blob:new Blob(['exact archive'])});
  const output=await pending;assert.equal(worker.terminated,false);assert.equal(await output.blob.text(),'exact archive');
  const closed=f.run(request('output-close',{metadata:output.token}));worker.reply(true);assert.equal(await closed,true);
  assert.deepEqual(worker.messages.map(m=>m.request.operation),['write','output-close']);assert.equal(worker.terminated,true);
}));

test('separate save outputs retain independent owners',()=>withFixture(async f=>{
  const first=f.run(request('write')),a=f.workers[0];a.reply({token:'first',blob:new Blob(['first'])});await first;
  const second=f.run(request('write')),b=f.workers[1];b.reply({token:'second',blob:new Blob(['second'])});await second;
  await assert.rejects(f.run(request('write')),/Finish the current output/);
  const closeFirst=f.run(request('output-close',{metadata:'first'}));a.reply(true);await closeFirst;
  assert.equal(a.terminated,true);assert.equal(b.terminated,false);
  const closeSecond=f.run(request('output-close',{metadata:'second'}));b.reply(true);await closeSecond;assert.equal(b.terminated,true);
}));

test('failed saves release archive workers and checkpoints reuse their cache owner',()=>withFixture(async f=>{
  const pending=f.run(request('write'));const rejected=assert.rejects(pending,/Publication failed/);
  f.workers[0].reply(undefined,{error:'Publication failed'});await rejected;assert.equal(f.workers[0].terminated,true);
  const recovery=f.run(request('restart-write'));f.workers[1].reply(true);assert.equal(await recovery,true);assert.equal(f.workers[1].terminated,false);
  const begin=f.run(request('restart-begin'));f.workers[1].reply([]);assert.deepEqual(await begin,[]);assert.equal(f.workers.length,2);
}));
