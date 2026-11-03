import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import {createStoreWorker} from './store-worker-client.js';
import {createLegacyRestartReader} from './legacy-restart-store.js';
import {createFileHandleStore} from './file-handles.js';

const request=(operation,metadata={},buffers=[])=>({operation,metadata,buffers});
const deferred=()=>Promise.withResolvers();

function fixture() {
  const originalWorker=globalThis.Worker,originalNavigator=Object.getOwnPropertyDescriptor(globalThis,'navigator');
  const workers=[],locks=new Map(),releases=[],events=[];
  Object.defineProperty(globalThis,'navigator',{configurable:true,value:{locks:{request(name,options,callback) {
    assert.equal(options.ifAvailable,true);
    if(locks.has(name))return Promise.resolve().then(()=>callback(null));
    const token={},completion=deferred();locks.set(name,token);events.push(['claim',name]);
    Promise.resolve().then(()=>callback(token)).then(()=>{
      releases.push(()=>{assert.equal(locks.get(name),token);locks.delete(name);events.push(['release',name]);completion.resolve();});
    },completion.reject);
    return completion.promise;
  }}}});
  globalThis.Worker=class {
    constructor(){this.messages=[];this.transfers=[];this.terminated=false;workers.push(this);}
    postMessage(message,transfer){assert.equal(this.terminated,false);if(this.postError)throw this.postError;this.messages.push(message);this.transfers.push(transfer);}
    terminate(){this.terminated=true;events.push(['terminate']);}
    reply(message,result,error){this.onmessage({data:{id:message.id,result,error}});}
    error(message='worker failed'){this.onerror({preventDefault(){},message});}
  };
  const run=createStoreWorker(()=>events.push(['wake']));
  return {run,workers,locks,releases,events,
    async flush(){for(let i=0;i<8;i++)await Promise.resolve();},
    releaseLocks(){for(const release of releases.splice(0))release();},
    close(){globalThis.Worker=originalWorker;if(originalNavigator)Object.defineProperty(globalThis,'navigator',originalNavigator);else delete globalThis.navigator;},
  };
}
async function withFixture(body){const f=fixture();try{await body(f);}finally{f.close();}}
function channels(tasks) {
  return class {constructor(){this.port1={};this.port2={postMessage:()=>tasks.push(()=>this.port1.onmessage({data:null}))};}};
}

test('store RPC replies bind to their request and transfer payload buffers without a Wasm module',()=>withFixture(async f=>{
  const bytes=Uint8Array.of(1,2,3),first=f.run(request('write_at',{name:'drawing/pack'},[bytes]));
  const second=f.run(request('read_at',{name:'drawing/pack',offset:0,length:3})),worker=f.workers[0];
  assert.equal(Object.hasOwn(worker.messages[0],'module'),false);assert.equal(Object.hasOwn(worker.messages[1],'module'),false);
  assert.deepEqual(worker.transfers,[[bytes.buffer],[]]);
  worker.reply(worker.messages[1],{buffers:[bytes]});assert.deepEqual(await second,{buffers:[bytes]});
  worker.reply(worker.messages[0],'written');assert.equal(await first,'written');
  worker.reply({id:999},'unknown');worker.reply(worker.messages[1],'duplicate');
  assert.deepEqual(f.events,[['wake']]);assert.equal(worker.terminated,false);
}));

test('a store operation error rejects only its own RPC and wakes a failed read',()=>withFixture(async f=>{
  const first=f.run(request('read_at')),second=f.run(request('flush')),worker=f.workers[0];
  const rejected=assert.rejects(first,{message:'Incomplete drawing store read'});
  worker.reply(worker.messages[0],undefined,'Incomplete drawing store read');await rejected;
  worker.reply(worker.messages[1],'flushed');assert.equal(await second,'flushed');
  assert.deepEqual(f.events,[['wake']]);assert.equal(worker.terminated,false);
}));

for(const event of ['error','messageerror'])test(`store ${event} rejects every outstanding RPC and future request`,()=>withFixture(async f=>{
  const first=f.run(request('read_at')),second=f.run(request('flush')),worker=f.workers[0],errors=[];
  const results=[first,second].map(promise=>promise.catch(error=>{errors.push(error);return 'rejected';}));
  if(event==='error')worker.error();else worker.onmessageerror();
  assert.deepEqual(await Promise.all(results),['rejected','rejected']);assert.equal(errors[0],errors[1]);
  await assert.rejects(f.run(request('list')),error=>error===errors[0]);
  assert.equal(worker.terminated,true);assert.equal(worker.messages.length,2);assert.deepEqual(f.events,[['terminate'],['wake']]);
}));

test('a postMessage error leaves another outstanding store RPC intact',()=>withFixture(async f=>{
  const first=f.run(request('list')),worker=f.workers[0],error=Error('transfer rejected');worker.postError=error;
  await assert.rejects(f.run(request('write_at')),result=>result===error);
  worker.postError=undefined;worker.reply(worker.messages[0],['head.1']);assert.deepEqual(await first,['head.1']);
  assert.equal(worker.terminated,false);
}));

test('release waits for close_tree before returning its drawing WebLock',()=>withFixture(async f=>{
  assert.equal(await f.run(request('claim',{name:'drawing'})),'drawing');
  const worker=f.workers[0],release=f.run(request('release',{name:'drawing'}));
  assert.equal(worker.messages.at(-1).request.operation,'close_tree');
  await f.flush();assert.equal(f.locks.size,1);assert.equal(f.releases.length,0);
  worker.reply(worker.messages.at(-1));await f.flush();
  assert.equal(f.releases.length,1);f.releaseLocks();await release;assert.equal(f.locks.size,0);
}));

test('immediate reclaim waits for the previous WebLock release to complete',()=>withFixture(async f=>{
  await f.run(request('claim',{name:'drawing'}));const worker=f.workers[0];
  const release=f.run(request('release',{name:'drawing'}));
  const reclaim=f.run(request('claim',{name:'drawing'}));
  const outcome=reclaim.then(value=>({value}),error=>({error}));
  worker.reply(worker.messages.at(-1));await f.flush();
  assert.equal(f.releases.length,1);f.releaseLocks();await release;await f.flush();
  const result=await outcome;assert.equal(result.error,undefined);assert.equal(result.value,'drawing');assert.equal(f.locks.size,1);
  const cleanup=f.run(request('release',{name:'drawing'}));worker.reply(worker.messages.at(-1));await f.flush();f.releaseLocks();await cleanup;
}));

test('a failed close_tree terminates the transport and keeps its drawing locked',()=>withFixture(async f=>{
  await f.run(request('claim',{name:'drawing'}));const worker=f.workers[0];
  const release=f.run(request('release',{name:'drawing'}));
  const rejected=assert.rejects(release,{message:'Drawing handle close failed'});
  worker.reply(worker.messages.at(-1),undefined,'Drawing handle close failed');await rejected;await f.flush();
  assert.equal(worker.terminated,true);
  assert.equal(f.locks.size,1);assert.equal(f.releases.length,0);
  await assert.rejects(f.run(request('claim',{name:'drawing'})),{message:'Drawing handle close failed'});
  assert.equal(f.events.filter(([event])=>event==='release').length,0);
}));

test('busy drawing ownership is reported without acquiring or releasing the foreign lock',()=>withFixture(async f=>{
  const foreign={};f.locks.set('capy-session-store:busy',foreign);
  assert.equal(await f.run(request('claim',{name:'busy'})),null);
  await f.run(request('release',{name:'busy'}));assert.equal(f.locks.get('capy-session-store:busy'),foreign);
  assert.equal(f.workers[0].messages.length,0);
}));

test('released drawing and window claims preserve the released lock names and need no OPFS close',()=>withFixture(async f=>{
  for(const legacy of ['drawing','window'])assert.equal(await f.run(request('claim',{name:'released',legacy})),'released');
  assert.deepEqual([...f.locks.keys()],['capy-session:drawing:released','capy-session:window:released']);
  for(const legacy of ['drawing','window']) {
    const release=f.run(request('release',{name:'released',legacy}));await f.flush();f.releaseLocks();await release;
  }
  assert.equal(f.locks.size,0);assert.equal(f.workers[0].messages.length,0);
}));

test('worker legacy RPCs use the real read-only reader without opening OPFS files',async()=>{
  const modes=[],manifest={generation:7,drawings:[{id:1,key:'drawing'}]},snapshot={current:{generation:2,metadata:'{}'},previous:null};
  const rows={windows:new Map([['window',manifest]]),sessions:new Map([['drawing',snapshot]]),resources:new Map([[JSON.stringify(['drawing','tile-0']),Uint8Array.of(3,5,7)]])};
  const indexedDB={open(name){
    assert.equal(name,'capy-session-restart');const opening={};
    queueMicrotask(()=>{opening.result={version:1,objectStoreNames:{contains:name=>name in rows},close(){},transaction(name,mode){
      modes.push(mode);const tx={objectStore(){return {
        get(key){const read={};queueMicrotask(()=>{read.result=rows[name].get(Array.isArray(key)?JSON.stringify(key):key);read.onsuccess();tx.oncomplete();});return read;},
        getAllKeys(){const read={};queueMicrotask(()=>{read.result=[...rows[name].keys()];read.onsuccess();tx.oncomplete();});return read;},
      };}};return tx;
    }};opening.onsuccess();});return opening;
  }};
  const timers=[],replies=[],scope={navigator:{storage:{getDirectory(){assert.fail('Legacy reads must not open OPFS');}}},
    createLegacyRestartReader:options=>createLegacyRestartReader({...options,indexedDB}),createFileHandleStore,
    MessageChannel:channels(timers),self:{postMessage:(message,transfers)=>replies.push({message,transfers})},Uint8Array,Number};
  runInNewContext(readFileSync(new URL('./store-worker.js',import.meta.url),'utf8').replace(/^import .*;\n/gm,''),scope);
  async function run(operation,metadata={}){scope.self.onmessage({data:{id:replies.length+1,request:request(operation,metadata)}});await timers.shift()();const reply=replies.at(-1);assert.equal(reply.message.error,undefined);return reply;}
  assert.deepEqual((await run('legacy-windows')).message.result,['window']);
  assert.deepEqual((await run('legacy-manifest',{key:'window'})).message.result,manifest);
  assert.deepEqual((await run('legacy-snapshot',{key:'drawing'})).message.result,snapshot);
  const ranged=await run('legacy-range',{key:'drawing',resource:'tile',offset:1,length:2});
  assert.deepEqual(Array.from(ranged.message.result.buffers[0]),[5,7]);assert.equal(ranged.transfers.length,1);assert.equal(ranged.transfers[0],ranged.message.result.buffers[0].buffer);
  assert.deepEqual(modes,['readonly','readonly','readonly','readonly']);
});

test('worker close_tree closes only the released drawing handles before its acknowledgement',async()=>{
  const source=readFileSync(new URL('./store-worker.js',import.meta.url),'utf8').replace(/^import .*;\n/gm,'');
  const timers=[],replies=[],closed=[];
  const directory={async getDirectoryHandle(){return this;},async getFileHandle(name){
    return {async createSyncAccessHandle(){let released=false;return {close(){released=true;closed.push(name);},getSize(){assert.equal(released,false);return 12;}};}};
  }};
  const scope={navigator:{storage:{async getDirectory(){return directory;}}},MessageChannel:channels(timers),
    self:{postMessage(message){replies.push({message,closed:[...closed]});}},Uint8Array,Number,createLegacyRestartReader,createFileHandleStore};
  runInNewContext(source,scope);
  async function run(id,operation,name) {scope.self.onmessage({data:{id,request:request(operation,{name,access:'read'})}});await timers.shift()();return replies.at(-1);}
  await run(1,'open','drawing/head.0');await run(2,'open','drawing/pack');await run(3,'open','drawing-other/head.1');
  const reply=await run(4,'close_tree','drawing');
  assert.equal(reply.message.id,4);assert.equal(reply.message.error,undefined);
  assert.deepEqual(reply.closed,['head.0','pack']);
  const stillOpen=await run(5,'len','drawing-other/head.1');assert.equal(stillOpen.message.result,12);
});

test('queued source reads precede writes and each channel task completes one bounded operation',async()=>{
  const tasks=[],replies=[],pixels=Uint8Array.of(1,2,3),handle={
    read(target,{at}){target.set(pixels.subarray(at,at+target.length));return target.length;},
    write(source,{at}){pixels.set(source,at);return source.length;},close(){},
  };
  const directory={async getDirectoryHandle(){return this;},async getFileHandle(){return {async createSyncAccessHandle(){return handle;}};}};
  const scope={navigator:{storage:{async getDirectory(){return directory;}}},MessageChannel:channels(tasks),
    self:{postMessage:message=>replies.push(message)},Uint8Array,Number,createLegacyRestartReader,createFileHandleStore};
  runInNewContext(readFileSync(new URL('./store-worker.js',import.meta.url),'utf8').replace(/^import .*;\n/gm,''),scope);
  const enqueue=(id,operation,metadata,buffers=[])=>scope.self.onmessage({data:{id,request:request(operation,metadata,buffers)}});
  enqueue(0,'open',{name:'drawing/page',access:'read'});await tasks.shift()();replies.length=0;
  enqueue(1,'write_at',{name:'drawing/page',offset:0},[Uint8Array.of(9)]);
  enqueue(2,'read_at',{name:'drawing/page',offset:0,length:3});
  enqueue(3,'write_at',{name:'drawing/page',offset:1},[Uint8Array.of(8)]);
  enqueue(4,'read_at',{name:'drawing/page',offset:0,length:2});
  assert.equal(tasks.length,1);
  for(let completed=1;completed<=4;completed++){await tasks.shift()();assert.equal(replies.length,completed);}
  assert.deepEqual(replies.map(reply=>reply.id),[2,4,1,3]);assert.ok(replies.every(reply=>reply.error===undefined));
  assert.deepEqual(Array.from(replies[0].result.buffers[0]),[1,2,3]);assert.deepEqual(Array.from(replies[1].result.buffers[0]),[1,2]);
  assert.deepEqual(Array.from(pixels),[9,8,3]);assert.equal(tasks.length,0);
});

function destinationFixture() {
  const rows=new Map(),queued=[],waiting=[];let opens=0;
  const indexedDB={open(name,version){
    assert.equal(name,'private-destinations');assert.equal(version,1);opens++;const request={};
    queueMicrotask(()=>{request.result={transaction(name,mode){
      assert.equal(name,'handles');const draft=new Map(rows),reads=[];
      const tx={mode,aborted:false,objectStore(){return {
        put(handle,uri){draft.set(uri,structuredClone(handle));},
        get(uri){const read={};reads.push([read,uri]);return read;},
      };},abort(){this.aborted=true;queueMicrotask(()=>tx.onabort());},complete(){
        assert.equal(this.aborted,false);
        if(mode==='readwrite'){rows.clear();for(const [key,value] of draft)rows.set(key,value);}
        for(const [read,key] of reads)read.onsuccess({target:{result:structuredClone(rows.get(key))}});tx.oncomplete();
      }};
      if(waiting.length)waiting.shift()(tx);else queued.push(tx);return tx;
    }};request.onsuccess();});return request;
  }};
  return {store:createFileHandleStore({indexedDB,name:'private-destinations'}),rows,opens:()=>opens,
    transaction:()=>queued.length?Promise.resolve(queued.shift()):new Promise(resolve=>waiting.push(resolve))};
}

test('destination handles publish on transaction completion and reopen through a readonly transaction',async()=>{
  const f=destinationFixture();await f.store.remember([]);assert.equal(f.opens(),0);
  const handle={kind:'file',name:'painting.capy'},writing=f.store.remember([{uri:'browser:painting',handle}]);
  let completed=false;writing.then(()=>{completed=true;});
  const write=await f.transaction();assert.equal(write.mode,'readwrite');assert.equal(completed,false);assert.equal(f.rows.size,0);
  write.complete();await writing;handle.name='changed caller copy';
  const reading=f.store.read('browser:painting'),read=await f.transaction();assert.equal(read.mode,'readonly');read.complete();
  assert.deepEqual(await reading,{kind:'file',name:'painting.capy'});assert.equal(f.opens(),1);
});

test('invalid destination batches abort without partially publishing handles and storage failure rejects',async()=>{
  const f=destinationFixture(),valid={uri:'browser:painting',handle:{kind:'file',name:'painting.capy'}};
  const invalid=assert.rejects(f.store.remember([valid,{uri:'file:foreign',handle:{kind:'file'}}]),{message:'Invalid file destination'});
  const rejected=await f.transaction();await invalid;assert.equal(rejected.aborted,true);assert.equal(f.rows.size,0);
  const error=Error('Destination storage full'),failure=assert.rejects(f.store.remember([valid]),result=>result===error);
  const write=await f.transaction();write.error=error;write.abort();await failure;assert.equal(f.rows.size,0);
});
