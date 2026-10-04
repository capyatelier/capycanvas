import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {launchChrome} from '../../tools/cdp.mjs';

const source=await readFile(new URL('./restart-store.js',import.meta.url));
const server=createServer((request,response)=>{
  response.setHeader('Content-Type',request.url==='/restart-store.js'?'text/javascript':'text/html');
  response.end(request.url==='/restart-store.js'?source:'<!doctype html><title>Private session storage test</title>');
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const browser=await launchChrome(['--headless=new']);
try {
  await browser.attachPage();await browser.call('Runtime.enable');await browser.call('Page.enable');
  await browser.call('Page.navigate',{url:`http://127.0.0.1:${server.address().port}/`});
  await browser.evaluate(`(async()=>{window.store=(await import('/restart-store.js')).createRestartStore();})()`);
  const publish=(generation,ids,payloads)=>browser.evaluate(`store.publish('owned-session',{generation:${generation},resources:${JSON.stringify(ids)},metadata:'{"value":${generation}}'},${payloads},[],${generation-1})`);
  await publish(1,['a'],"[{id:'a',bytes:new Uint8Array([1,2,3])}]");
  await publish(2,['a','b'],"[{id:'b',bytes:new Uint8Array([4,5])}]");
  assert.deepEqual(await browser.evaluate("store.list()"),['owned-session']);
  assert.deepEqual(await browser.evaluate("(async()=>{const record=await store.read('owned-session');for(const checkpoint of [record.current,record.previous]){await (await import('/restart-store.js')).verifyRestartCheckpoint(checkpoint);delete checkpoint.metadata;delete checkpoint.metadata_sha256;}return record;})()"),{
    current:{generation:2,resources:['a','b']},previous:{generation:1,resources:['a']},handles:[],
  });
  assert.deepEqual(await browser.evaluate("(async()=>(await store.resources('owned-session',['a','b'])).map(bytes=>Array.from(bytes)))()"),[[1,2,3],[4,5]]);
  assert.equal(await browser.evaluate("store.publish('owned-session',{generation:3,resources:['a'],metadata:'{}'},[{id:'a',bytes:new Uint8Array([9,9,9])}]).then(()=>false,()=>true)"),true,'Conflicting immutable IDs cannot overwrite either complete generation');
  assert.deepEqual(await browser.evaluate("(async()=>(await store.resources('owned-session',['a'])).map(bytes=>Array.from(bytes)))()"),[[1,2,3]]);
  assert.equal(await browser.evaluate(`(()=>{const original=IDBObjectStore.prototype.put;IDBObjectStore.prototype.put=function(value,key){if(this.name==='sessions')throw new DOMException('Injected quota failure','QuotaExceededError');return original.call(this,value,key);};return store.publish('owned-session',{generation:3,resources:['c'],metadata:'{}'},[{id:'c',bytes:new Uint8Array([9])}]).then(()=>false,()=>true).finally(()=>IDBObjectStore.prototype.put=original);})()`),true);
  assert.equal(await browser.evaluate("(async()=>(await store.read('owned-session')).current.generation)()"),2,'Quota failure preserves the last published generation');
  assert.equal(await browser.evaluate("store.resources('owned-session',['c']).then(()=>false,()=>true)"),true,'A failed publication rolls back inserted resources');
  await browser.evaluate("store.publishManifest('unfinished-window',{generation:1,drawings:[{id:1,key:'owned-session'}],active:1,clean_exit:false,restoring:[{id:1,generation:1}],blocked:[]})");
  const loaded=browser.once('Page.loadEventFired');await browser.call('Page.reload');await loaded;
  await browser.evaluate("(async()=>{window.store=(await import('/restart-store.js')).createRestartStore();})()");
  assert.equal(await browser.evaluate("(async()=>(await store.manifest('unfinished-window')).restoring.length)()"),1,'Unfinished restore survives a browser restart');
  const transfer="store.transferManifest('unfinished-window',{generation:3,drawings:[],active:0,clean_exit:false,restoring:[],blocked:[]},'restored-window',{generation:1,drawings:[{id:4,key:'owned-session'}],active:4,clean_exit:false,restoring:[],blocked:[]},'owned-session',1,0)";
  assert.equal(await browser.evaluate(transfer.replace(",'owned-session',1,0)",",'owned-session',0,0)")+".then(()=>false,()=>true)"),true,'An ownership transfer cannot publish against an outdated source receipt');
  assert.equal(await browser.evaluate(`(()=>{const original=IDBObjectStore.prototype.put;IDBObjectStore.prototype.put=function(value,key){if(this.name==='windows'&&key==='restored-window')throw new DOMException('Injected handoff failure','QuotaExceededError');return original.call(this,value,key);};return ${transfer}.then(()=>false,()=>true).finally(()=>IDBObjectStore.prototype.put=original);})()`),true);
  assert.deepEqual(await browser.evaluate("store.windows()"),['unfinished-window'],'Failed ownership transfer retains the original authoritative membership');
  assert.equal(await browser.evaluate("(async()=>(await store.read('owned-session')).current.generation)()"),2,'Failed ownership transfer preserves the sole complete drawing');
  await browser.evaluate(transfer);
  await browser.evaluate(transfer);
  assert.deepEqual(await browser.evaluate("store.windows()"),['restored-window'],'Successful ownership transfer publishes exactly one membership');
  assert.equal(await browser.evaluate("(async()=>(await store.read('owned-session')).current.generation)()"),2,'Ownership transfer reuses the immutable drawing without copying it');
  await browser.evaluate("store.removeManifest('restored-window')");
  await publish(3,['b'],"[]");
  assert.deepEqual(await browser.evaluate("(async()=>(await store.resources('owned-session',['a','b'])).map(bytes=>Array.from(bytes)))()"),[[1,2,3],[4,5]],'Prior checkpoint retains every required resource');
  await publish(4,['b'],"[]");
  assert.equal(await browser.evaluate("store.resources('owned-session',['a']).then(()=>false,()=>true)"),true,'Cleanup retires only resources absent from both complete generations');
  assert.equal(await browser.evaluate("store.publish('owned-session',{generation:3,resources:['a'],metadata:'{}'},[{id:'a',bytes:new Uint8Array([7])}]).then(()=>false,()=>true)"),true,'Older drawing completion cannot replace newer work');
  await browser.evaluate("store.publishManifest('window',{generation:1,drawings:[{id:1,key:'owned-session'}],active:1,clean_exit:false,restoring:[],blocked:[]})");
  assert.deepEqual(await browser.evaluate("store.windows()"),['window']);
  await browser.evaluate("store.publishManifest('window',{generation:2,drawings:[],active:0,clean_exit:false,restoring:[],blocked:[]},['owned-session'])");
  await browser.evaluate("store.publishManifest('window',{generation:2,drawings:[],active:0,clean_exit:false,restoring:[],blocked:[]},['owned-session'])");
  assert.equal(await browser.evaluate("store.publishManifest('window',{generation:2,drawings:[{id:9,key:'owned-session'}],active:9,clean_exit:false,restoring:[],blocked:[]}).then(()=>false,()=>true)"),true,'Equal-generation acknowledgement retries require identical membership');
  assert.equal(await browser.evaluate("store.publishManifest('window',{generation:1,drawings:[{id:1,key:'owned-session'}],active:1,clean_exit:false,restoring:[],blocked:[]}).then(()=>false,()=>true)"),true,'Older membership completion cannot resurrect a closed drawing');
  assert.deepEqual(await browser.evaluate("store.manifest('window')"),{generation:2,drawings:[],active:0,clean_exit:false,restoring:[],blocked:[]});
  assert.deepEqual(await browser.evaluate("store.list()"),[]);
  assert.equal(await browser.evaluate("store.resources('owned-session',['b']).then(()=>false,()=>true)"),true);
  await browser.evaluate("store.removeManifest('window')");
  await browser.evaluate(`store.publish('metadata-session',{generation:1,resources:[],metadata:'{"camera":1}'},[])`);
  await browser.evaluate(`store.publish('metadata-session',{generation:2,resources:[],metadata:'{"camera":2}'},[],[],1)`);
  await browser.evaluate(`(async()=>{const opening=indexedDB.open('capy-session-restart'),database=await new Promise((resolve,reject)=>{opening.onsuccess=()=>resolve(opening.result);opening.onerror=()=>reject(opening.error);});try{await new Promise((resolve,reject)=>{const transaction=database.transaction('sessions','readwrite'),sessions=transaction.objectStore('sessions');transaction.oncomplete=resolve;transaction.onabort=()=>reject(transaction.error);const request=sessions.get('metadata-session');request.onsuccess=()=>{const record=request.result;record.current.metadata='{"camera":3}';sessions.put(record,'metadata-session');};});}finally{database.close();}})()`);
  assert.deepEqual(await browser.evaluate("(async()=>{const record=await store.read('metadata-session'),{verifyRestartCheckpoint}=await import('/restart-store.js');return Promise.all([record.current,record.previous].map(checkpoint=>verifyRestartCheckpoint(checkpoint).then(()=>true,()=>false)));})()"),[false,true],'A valid JSON scalar bit flip rejects the current checkpoint and retains a verified previous snapshot');
  assert.equal(await browser.evaluate(`store.publish('metadata-session',{generation:3,resources:[],metadata:'{"camera":4}'},[],[],99).then(()=>false,()=>true)`),true,'A missing verified base cannot publish a replacement');
  await browser.evaluate(`store.publish('metadata-session',{generation:3,resources:[],metadata:'{"camera":4}'},[],[],1)`);
  assert.equal(await browser.evaluate("(async()=>{const record=await store.read('metadata-session');await (await import('/restart-store.js')).verifyRestartCheckpoint(record.previous);return record.previous.generation;})()"),1,'Fallback publication retains the verified loaded generation instead of the damaged current head');
  await browser.evaluate(`(async()=>{const opening=indexedDB.open('capy-session-restart'),database=await new Promise(resolve=>{opening.onsuccess=()=>resolve(opening.result);});try{await new Promise((resolve,reject)=>{const transaction=database.transaction('sessions','readwrite'),sessions=transaction.objectStore('sessions');transaction.oncomplete=resolve;transaction.onabort=()=>reject(transaction.error);const request=sessions.get('metadata-session');request.onsuccess=()=>{const record=request.result;record.current.metadata='{"camera":5}';sessions.put(record,'metadata-session');};});}finally{database.close();}})()`);
  assert.deepEqual(await browser.evaluate("(async()=>{const record=await store.read('metadata-session'),{verifyRestartCheckpoint}=await import('/restart-store.js');return Promise.all([record.current,record.previous].map(checkpoint=>verifyRestartCheckpoint(checkpoint).then(()=>true,()=>false)));})()"),[false,true],'A second damaged head still has the independently verified fallback');
  await browser.evaluate("store.remove('metadata-session')");
  await browser.evaluate("store.removeManifest('unfinished-window')");
  assert.deepEqual(await browser.evaluate("store.windows()"),[]);
  assert.deepEqual(browser.errors,[]);
  console.log('PASS atomic session storage: rollback, previous generation, resource cleanup, restart and restore attempts');
} finally {
  await browser.close();await new Promise(resolve=>server.close(resolve));
}
