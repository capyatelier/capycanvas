import test from 'node:test';
import assert from 'node:assert/strict';
import {createSourceRegistry} from './store-sources.js';

function fixture() {
  const bytes=Uint8Array.of(11,23,37,41),handle={getSize:()=>bytes.length,read(target,{at}){target.set(bytes.subarray(at,at+target.length));return target.length;}};
  const registry=createSourceRegistry({file:async name=>{assert.equal(name,'drawing/pack');return handle;},parts:name=>{assert.equal(name,'drawing/pack');}});
  const call=(operation,metadata={},owner='parser',extra={})=>registry.execute({operation,metadata,...extra},owner);
  return {registry,call,bytes,reference:{key:'store:drawing/pack',generation:'generation',length:bytes.length}};
}

test('registering an incoming Blob does no copy or storage admission',async()=>{
  const registry=createSourceRegistry({file:()=>assert.fail('No Store open'),parts(){}});
  const file=new Blob(['unread package']);
  file.arrayBuffer=()=>assert.fail('Registration must not read the File');
  file.slice=()=>assert.fail('Registration must not copy File chunks');
  const registered=await registry.execute({operation:'register_blob',file},'page');
  assert.equal(registered.file,file);
  assert.equal('snapshot_bytes' in registered,false);
  await registry.execute({operation:'release_source',metadata:{lease:registered.lease}},'page');
});

for(const failure of ['NotReadableError','short'])test(`an unread Blob range fails explicitly on source loss: ${failure}`,async()=>{
  const {registry,call}=fixture(),file=new Blob(['original bytes']);
  const registered=await call('register_blob',{},'page',{file});
  file.slice=()=>({async arrayBuffer(){if(failure==='NotReadableError')throw new DOMException('File changed','NotReadableError');return new ArrayBuffer(1);}});
  const accepted=registry.accept({operation:'read_source',metadata:{lease:registered.lease,offset:0,length:4}},'page');
  await assert.rejects(registry.read(accepted),error=>/source.*lost/i.test(error.message)&&error.message.includes(registered.key));
  accepted.release();await call('release_source',{lease:registered.lease},'page');
});

test('a claimed file offer survives the parsing connection and reads an unseen range',async()=>{
  const {registry,call}=fixture(),blob=new Blob([Uint8Array.of(1,2,3,5,8,13)]);
  blob.arrayBuffer=()=>assert.fail('No whole-Blob read');
  const registered=await call('register_blob',{},'page',{file:blob});
  const reference={key:registered.key,generation:'immutable-generation',length:blob.size};
  const ticket=await call('offer_sources',{references:[reference]});
  await call('claim_sources',{ticket},'page');registry.disconnect('parser');
  const [lease]=await call('adopt_sources',{ticket,references:[reference]},'page');
  await call('release_source',{lease:registered.lease},'page');
  const accepted=registry.accept({operation:'read_source',metadata:{lease,offset:4,length:2}},'page');
  await call('release_source',{lease},'page');
  assert.deepEqual(Array.from((await registry.read(accepted)).buffers[0]),[8,13]);accepted.release();
  await assert.rejects(call('offer_sources',{references:[reference]},'page'),/no longer available/);
});

test('last release waits for accepted reads and lets retirement finish without blocking the queue',async()=>{
  const {registry,call,reference}=fixture(),ticket=await call('offer_sources',{references:[reference]});
  const [lease]=await call('adopt_sources',{ticket,references:[reference]},'page');
  const accepted=registry.accept({operation:'read_source',metadata:{lease,offset:1,length:2}},'page');
  const retirement=registry.after('drawing',true);assert.equal(registry.held('drawing',true),true);
  await call('release_source',{lease},'page');assert.equal(registry.held('drawing/pack'),true);
  assert.deepEqual(Array.from((await registry.read(accepted)).buffers[0]),[23,37]);accepted.release();
  assert.equal(registry.held('drawing',true),false);await retirement.deferred;
  assert.throws(()=>registry.accept({operation:'read_source',metadata:{lease,offset:0,length:1}},'page'),/no longer available/);
});

test('adoption checks the offered generation, extent and key before consuming it',async()=>{
  const {registry,call,reference}=fixture(),ticket=await call('offer_sources',{references:[reference]});
  for(const changed of [{generation:'replacement'},{key:'store:another/pack'},{length:3}]) {
    await assert.rejects(call('adopt_sources',{ticket,references:[{...reference,...changed}]},'page'),/identity mismatch/);
    assert.equal(registry.held('drawing/pack'),true);
  }
  const [lease]=await call('adopt_sources',{ticket,references:[reference]},'page');
  for(const range of [{offset:3,length:2},{offset:Number.MAX_SAFE_INTEGER,length:2},{offset:0,length:8*1024*1024+1}]) {
    assert.throws(()=>registry.accept({operation:'read_source',metadata:{lease,...range}},'page'),/exceeds immutable range/);
  }
  registry.disconnect('page');assert.equal(registry.held('drawing/pack'),false);
});

test('cancellation releases the entire offer and cannot release another connection',async()=>{
  const {registry,call,reference}=fixture(),ticket=await call('offer_sources',{references:[reference,reference]});
  await assert.rejects(call('cancel_sources',{ticket},'page'),/another connection/);
  await call('cancel_sources',{ticket});assert.equal(registry.held('drawing/pack'),false);
  await call('cancel_sources',{ticket});
  const second=await call('offer_sources',{references:[reference]});registry.disconnect('parser');
  assert.equal(registry.held('drawing/pack'),false);
  await assert.rejects(call('adopt_sources',{ticket:second,references:[reference]},'page'),/identity mismatch/);
});

test('partial offer failure releases earlier roots and missing Blob generations fail explicitly',async()=>{
  const {registry,call,reference}=fixture();
  await assert.rejects(call('offer_sources',{references:[reference,{key:'blob:missing',length:3}]}),/no longer available/);
  assert.equal(registry.held('drawing/pack'),false);
  await assert.rejects(call('offer_sources',{references:[{...reference,length:5}]}),/exceeds immutable extent/);
  assert.equal(registry.held('drawing/pack'),false);
});


test('disconnect during an asynchronous open cannot strand a new offer',async()=>{
  let connected=true;const opened=Promise.withResolvers();
  const registry=createSourceRegistry({file:()=>opened.promise,parts(){},connected:()=>connected});
  const offer=registry.execute({operation:'offer_sources',metadata:{references:[{key:'store:drawing/pack',length:4}]}},'parser');
  connected=false;registry.disconnect('parser');
  opened.resolve({getSize:()=>4});await assert.rejects(offer,/connection closed/);
  assert.equal(registry.held('drawing/pack'),false);
});


test('cancelling an accepted job retires unadopted results and rejects a late result offer',async()=>{
  const {registry,call,reference}=fixture(),parent=await call('offer_sources',{references:[reference]},'page');
  const [input]=await call('adopt_sources',{ticket:parent,references:[reference]},'worker');
  const child=await call('offer_sources',{references:[reference],parent},'worker');
  await call('claim_sources',{ticket:child},'page');
  await call('cancel_sources',{ticket:parent},'page');
  await assert.rejects(call('adopt_sources',{ticket:child,references:[reference]},'page'),/identity mismatch/);
  await assert.rejects(call('offer_sources',{references:[reference],parent},'worker'),/operation has ended/);
  await call('release_source',{lease:input},'worker');assert.equal(registry.held('drawing/pack'),false);
});

test('cancelling a Blob open releases an unadopted parser result',async()=>{
  const {call}=fixture(),registered=await call('register_blob',{},'page',{file:new Blob(['unread tile'])});
  const reference={key:registered.key,generation:'original',length:registered.length};
  const ticket=await call('offer_sources',{references:[reference]},'parser');
  await call('claim_sources',{ticket},'page');await call('release_source',{lease:registered.lease},'page');
  await assert.rejects(call('adopt_sources',{ticket,references:[reference]},'page'),/identity mismatch/);
  await assert.rejects(call('offer_sources',{references:[reference]},'parser'),/no longer available/);
});
