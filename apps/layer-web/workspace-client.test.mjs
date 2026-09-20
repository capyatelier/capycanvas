import assert from 'node:assert/strict';
import {test} from 'node:test';
import {createWorkspaceClient} from './workspace-store.js';

test('storage completion wakes the controller after replies, errors and reconnect', async () => {
  const previous=globalThis.Worker, workers=[];
  class Worker {
    requests=[];
    constructor() { workers.push(this); }
    postMessage(request) { this.requests.push(request); }
    terminate() { this.terminated=true; }
    reply(value, error) {
      const {id}=this.requests.shift();
      this.onmessage({data:{id,response:value,error}});
    }
  }
  globalThis.Worker=Worker;
  let wakes=0;
  const client=createWorkspaceClient('workspace-worker.js',{onSettled:()=>{wakes++;}});
  const execute=type=>client.execute(JSON.stringify({type}));
  try {
    const first=execute('reopen');
    assert.equal(wakes,0);
    workers[0].reply('opened');
    assert.equal(wakes,0,'Never reenter Wasm inside the worker message handler');
    assert.equal(await first,'opened'); assert.equal(wakes,1);

    const rejected=execute('claim');
    workers[0].reply(null,'owned_elsewhere');
    await assert.rejects(rejected,error=>error==='owned_elsewhere');
    assert.equal(wakes,2,'Rejected operations must advance recovery state');

    const reads=[execute('list'),execute('binding')];
    workers[0].onerror({message:'lost worker'});
    assert.equal(workers[0].terminated,true);
    const results=await Promise.allSettled(reads);
    assert.ok(results.every(r=>r.status==='rejected'&&JSON.parse(r.reason).kind==='unavailable'));
    assert.equal(wakes,4,'Worker failure wakes all outstanding operations');
    await assert.rejects(execute('list'),error=>JSON.parse(error).kind==='unavailable');
    assert.equal(wakes,5);
    assert.equal(workers.length,1,'Only explicit reopen reconnects a failed worker');

    const reopened=execute('reopen');
    assert.equal(workers.length,2);
    workers[1].reply('reconnected');
    assert.equal(await reopened,'reconnected'); assert.equal(wakes,6);
    const next=execute('list');workers[1].reply('items');
    assert.equal(await next,'items'); assert.equal(wakes,7);
  } finally { globalThis.Worker=previous; }
});

test('preloaded storage receives its compiled module and reuses it after reconnect', async () => {
  const previous=globalThis.Worker, workers=[];
  class Worker {
    requests=[];
    constructor() { workers.push(this); }
    postMessage(request) { this.requests.push(request); }
    terminate() {}
  }
  globalThis.Worker=Worker;
  const module=new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0]));
  try {
    const client=createWorkspaceClient('workspace-worker.js',{preload:true});
    assert.equal(workers.length,1,'Load storage modules while Wasm compiles');
    assert.deepEqual(workers[0].requests,[]);
    client.initialize(module);
    assert.deepEqual(workers[0].requests,[{module}]);
    const failed=client.execute('{"type":"list"}');
    workers[0].onerror({message:'worker lost'});
    await assert.rejects(failed);
    const reopened=client.execute('{"type":"reopen"}');
    assert.equal(workers.length,2);
    const [init,request]=workers[1].requests;
    assert.equal(init.module,module,'Reconnect retains the compiled application');
    workers[1].onmessage({data:{id:request.id,response:'ready'}});
    assert.equal(await reopened,'ready');
  } finally { globalThis.Worker=previous; }
});
