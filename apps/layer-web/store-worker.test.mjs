import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {launchChrome} from '../../tools/cdp.mjs';

const modules=new Map(await Promise.all(['store-worker-client.js','store-worker.js','legacy-restart-store.js','file-handles.js']
  .map(async path=>['/'+path,await readFile(new URL(path,import.meta.url))])));
const requests=[];
const server=createServer((request,response)=>{
  requests.push(request.url);const source=modules.get(request.url);
  response.setHeader('Content-Type',source?'text/javascript':'text/html');
  response.end(source??'<!doctype html><link rel="icon" href="data:,"><title>Private storage worker test</title>');
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));let browser;
try {
  browser=await launchChrome(['--headless=new','--disable-gpu']);await browser.attachPage();await browser.call('Runtime.enable');await browser.call('Page.enable');
  const loaded=browser.once('Page.loadEventFired');await browser.call('Page.navigate',{url:`http://127.0.0.1:${server.address().port}/`});await loaded;
  const result=await browser.evaluate(`(async()=>{
    const {createStoreWorker}=await import('/store-worker-client.js'),run=createStoreWorker(()=>{});
    const call=(operation,metadata={},buffers=[])=>run({operation,metadata,buffers});
    const facts=await call('estimate',{epoch:17});
    if(facts.epoch!==17||facts.scope!==location.origin||!Number.isSafeInteger(facts.available_bytes)||facts.available_bytes<0)throw Error('Invalid browser quota facts');
    const name='scheduler/checksum',expected=new TextEncoder().encode('123456789'),crc=0xcbf43926;
    await call('claim',{name:'scheduler'});await call('open',{name,access:'create'});
    await call('write_at',{name,offset:0},[expected]);await call('flush',{name});
    const read=()=>call('read_at',{name,offset:0,length:9,crc});
    const cold=await call('copy_read_at',{name,offset:0,length:9});if(new TextDecoder().decode(cold.buffers[0])!=='123456789')throw Error('Background copy changed bytes');
    const first=await read();if(new TextDecoder().decode(first.buffers[0])!=='123456789')throw Error('Correct CRC changed bytes');
    const refusal=async request=>{try{await call('read_at',{name,offset:0,length:9,...request});return false;}catch(error){return error.message==='Recovery resource checksum failed';}};
    const mismatch=await refusal({crc:(crc+1)>>>0}),strict=await refusal({crc:String(crc)});
    await call('write_at',{name,offset:0},[Uint8Array.of(0)]);const corrupt=await refusal({crc});
    await call('write_at',{name,offset:0},[Uint8Array.of(49)]);await call('flush',{name});
    const begin=performance.now(),reads=await Promise.all(Array.from({length:100},read)),elapsed=performance.now()-begin;
    const exact=reads.every(result=>new TextDecoder().decode(result.buffers[0])==='123456789');
    const opened=await call('remove',{name});
    const closedName='scheduler/closed';await call('open',{name:closedName,access:'create'});
    await call('write_at',{name:closedName,offset:0},[Uint8Array.of(1,2,3)]);await call('close',{name:closedName});
    const closed=await call('remove',{name:closedName}),absent=await call('remove',{name:'scheduler/absent'});
    const directory=await(await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-session-store')).getDirectoryHandle('scheduler');
    await directory.getDirectoryHandle('empty',{create:true});const empty=await call('remove',{name:'scheduler/empty'});
    const remaining=await call('list',{name:'scheduler'});
    await call('release',{name:'scheduler'});
    return {reads:reads.length,bytes:reads.reduce((sum,result)=>sum+result.buffers[0].byteLength,0),elapsed_ms:elapsed,exact,mismatch,strict,corrupt,quota_epoch:facts.epoch,origin:location.origin,removals:[opened,closed,absent,empty],remaining};
  })()`);
  assert.equal(result.reads,100);assert.equal(result.bytes,900);assert.equal(result.exact,true);
  assert.equal(result.mismatch,true,'A different CRC is rejected');assert.equal(result.strict,true,'CRC comparison cannot coerce an incorrect type');
  assert.equal(result.corrupt,true,'Changed file bytes fail the original CRC');
  assert.equal(result.quota_epoch,17);
  assert.deepEqual(result.removals.map(value=>value.scope),Array(4).fill(result.origin),'Removal evidence stays in the actual origin');
  assert.deepEqual(result.removals.map(value=>value.bytes),[9,3,0,0],'Open and closed files report actual bytes; absent files and empty directories report zero');
  assert.deepEqual(result.remaining,[],'Removal closes handles and deletes the actual OPFS entries');
  assert.ok(result.elapsed_ms<250,`100 queued small reads took ${result.elapsed_ms.toFixed(2)} ms; expected less than 250 ms`);
  assert.equal(requests.some(path=>path.includes('/pkg/')||path.endsWith('.wasm')),false,'Storage CRC verification loads no app Wasm module');
  assert.deepEqual(browser.errors,[]);
  console.log('PASS real OPFS worker: 100 queued reads, strict/corrupt CRC, no Wasm requests',JSON.stringify(result));
}finally{if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));}
