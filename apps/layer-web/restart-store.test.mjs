import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {launchChrome} from '../../tools/cdp.mjs';

const modules=new Map(await Promise.all([
  ['/released-writer.js','./fixtures/1.0.15/restart-store.js'],
  ['/legacy-reader.js','./legacy-restart-store.js'],
].map(async([route,path])=>[route,await readFile(new URL(path,import.meta.url))])));
const server=createServer((request,response)=>{
  const source=modules.get(request.url);
  response.setHeader('Content-Type',source?'text/javascript':'text/html');
  response.end(source??'<!doctype html><title>Private legacy session storage test</title>');
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
let browser;
try {
  browser=await launchChrome(['--headless=new','--disable-gpu']);
  await browser.attachPage();await browser.call('Runtime.enable');await browser.call('Page.enable');
  const loaded=browser.once('Page.loadEventFired');
  await browser.call('Page.navigate',{url:`http://127.0.0.1:${server.address().port}/`});await loaded;
  await browser.evaluate(`(async()=>{
    window.createReader=(await import('/legacy-reader.js')).createLegacyRestartReader;
    const released=await import('/released-writer.js');window.verify=released.verifyRestartCheckpoint;
    window.writer=released.createRestartStore({name:'released-session'});
    window.readDatabase=async()=>{
      const opening=indexedDB.open('released-session');
      const database=await new Promise((resolve,reject)=>{opening.onsuccess=()=>resolve(opening.result);opening.onerror=()=>reject(opening.error);});
      try {
        return await new Promise((resolve,reject)=>{
          const transaction=database.transaction(['sessions','resources','windows'],'readonly'),result={version:database.version};
          for(const name of ['sessions','resources','windows']) {
            const store=transaction.objectStore(name),keys=store.getAllKeys(),values=store.getAll();
            keys.onsuccess=()=>{(result[name]??={}).keys=keys.result;};values.onsuccess=()=>{(result[name]??={}).values=values.result;};
          }
          transaction.oncomplete=()=>resolve(result);transaction.onabort=()=>reject(transaction.error);
        });
      }finally{database.close();}
    };
    window.unchanged=(before,after)=>before.version===after.version&&['sessions','windows'].every(name=>JSON.stringify(before[name])===JSON.stringify(after[name]))
      &&JSON.stringify(before.resources.keys)===JSON.stringify(after.resources.keys)
      &&before.resources.values.length===after.resources.values.length
      &&before.resources.values.every((bytes,index)=>bytes.length===after.resources.values[index].length&&bytes.every((byte,offset)=>byte===after.resources.values[index][offset]));
  })()`);
  assert.deepEqual(await browser.evaluate(`(async()=>{
    const absent=window.absentReader=createReader({indexedDB,name:'absent-released-session'}),before=await indexedDB.databases();
    const observations=[await absent.windows(),await absent.manifest('missing')===undefined,await absent.snapshot('missing')===undefined];
    return {before:before.some(db=>db.name==='absent-released-session'),after:(await indexedDB.databases()).some(db=>db.name==='absent-released-session'),observations};
  })()`),{before:false,after:false,observations:[[],true,true]},'An absent legacy database probe cannot create a database');
  assert.deepEqual(await browser.evaluate(`(async()=>{
    const {createRestartStore}=await import('/released-writer.js'),writer=createRestartStore({name:'absent-released-session'});
    const manifest={generation:1,drawings:[],active:0,clean_exit:false,restoring:[],blocked:[]};
    await writer.publishManifest('late-window',manifest);
    return {windows:await absentReader.windows(),manifest:await absentReader.manifest('late-window'),version:(await indexedDB.databases()).find(database=>database.name==='absent-released-session').version};
  })()`),{windows:['late-window'],manifest:{generation:1,drawings:[],active:0,clean_exit:false,restoring:[],blocked:[]},version:1},'The same reader observes a released database created after its absent probe');
  await browser.evaluate(`(async()=>{
    window.BLOCK=4*1024*1024;
    window.block=index=>Uint8Array.from({length:index===2?17:BLOCK},(_,offset)=>(offset*31+index*19)&255);
    window.ids=['large-0','large-1','large-2','gap-0'];
    await writer.publish('drawing',{generation:1,resources:['large-0'],metadata:'{"camera":1}'},[{id:'large-0',bytes:block(0)}]);
    await writer.publish('drawing',{generation:2,resources:ids,metadata:'{"camera":2}'},[{id:'large-1',bytes:block(1)},{id:'large-2',bytes:block(2)},{id:'gap-0',bytes:block(0)}],[],1);
    await writer.publishManifest('window',{generation:1,drawings:[{id:1,key:'drawing'}],active:1,clean_exit:false,restoring:[{id:1,generation:1}],blocked:[]});
    window.beforeUpgrade=await readDatabase();window.reader=createReader({indexedDB,name:'released-session'});
    window.modes=[];window.transaction=IDBDatabase.prototype.transaction;
    IDBDatabase.prototype.transaction=function(stores,mode,...options){modes.push(mode??'readonly');return transaction.call(this,stores,mode,...options);};
  })()`);
  assert.deepEqual(await browser.evaluate('reader.windows()'),['window']);
  assert.deepEqual(await browser.evaluate("reader.manifest('window')"),{generation:1,drawings:[{id:1,key:'drawing'}],active:1,clean_exit:false,restoring:[{id:1,generation:1}],blocked:[]});
  assert.deepEqual(await browser.evaluate(`(async()=>{
    const snapshot=await reader.snapshot('drawing');await verify(snapshot.current);await verify(snapshot.previous);
    return {current:snapshot.current.generation,previous:snapshot.previous.generation,metadata:[snapshot.current.metadata,snapshot.previous.metadata],resources:snapshot.current.resources,handles:snapshot.handles};
  })()`),{current:2,previous:1,metadata:['{"camera":2}','{"camera":1}'],resources:['large-0','large-1','large-2','gap-0'],handles:[]});
  for(const [offset,length] of [[0,1],[0,4*1024*1024],[4*1024*1024-3,9],[4*1024*1024,4*1024*1024],[0,8*1024*1024],[8*1024*1024-2,19],[8*1024*1024,17],[4*1024*1024,0]]) {
    assert.equal(await browser.evaluate(`(async()=>{const bytes=await reader.range('drawing','large',${offset},${length});return bytes.length===${length}&&bytes.every((byte,index)=>{const position=${offset}+index;return byte===(((position%BLOCK)*31+Math.floor(position/BLOCK)*19)&255);});})()`),true,`Exact released resource range at ${offset}+${length}`);
  }
  assert.equal(await browser.evaluate("reader.range('drawing','gap',BLOCK-1,2).then(()=>false,error=>error.message==='A saved session resource is missing')"),true,'A missing next block rejects the entire requested range');
  for(const [offset,length] of [[-1,1],[0,8*1024*1024+1],[0,0.5],[Number.MAX_SAFE_INTEGER,1]]) {
    assert.equal(await browser.evaluate(`reader.range('drawing','large',${offset},${length}).then(()=>false,error=>error.message==='Invalid saved session range')`),true,'Range validation rejects unsafe or oversized reads');
  }
  assert.equal(await browser.evaluate(`(async()=>{
    const snapshot=await reader.snapshot('drawing');snapshot.current.metadata='changed returned copy';
    const bytes=await reader.range('drawing','large',0,1);bytes[0]^=255;
    return (await reader.snapshot('drawing')).current.metadata==='{"camera":2}'&&(await reader.range('drawing','large',0,1))[0]===0;
  })()`),true,'Returned metadata and range buffers are copies');
  assert.deepEqual(await browser.evaluate(`(async()=>{
    const after=await readDatabase();IDBDatabase.prototype.transaction=transaction;
    return {unchanged:unchanged(beforeUpgrade,after),version:after.version,readOnly:modes.length>0&&modes.every(mode=>mode==='readonly')};
  })()`),{unchanged:true,version:1,readOnly:true},'Upgrade reads preserve every released record and byte in version 1 readonly transactions');
  await browser.evaluate(`(async()=>{
    window.writer=(await import('/released-writer.js')).createRestartStore({name:'released-session'});
    await writer.publish('drawing',{generation:3,resources:[...ids,'edited-0'],metadata:'{"camera":3,"downgrade":true}'},[{id:'edited-0',bytes:Uint8Array.of(9,8,7)}],[],2);
    await writer.publishManifest('window',{generation:2,drawings:[{id:1,key:'drawing'}],active:1,clean_exit:true,restoring:[],blocked:[]});
    window.afterDowngrade=await readDatabase();window.reader=createReader({indexedDB,name:'released-session'});
    modes=[];IDBDatabase.prototype.transaction=function(stores,mode,...options){modes.push(mode??'readonly');return transaction.call(this,stores,mode,...options);};
  })()`);
  assert.deepEqual(await browser.evaluate(`(async()=>{
    const snapshot=await reader.snapshot('drawing');await verify(snapshot.current);await verify(snapshot.previous);
    const manifest=await reader.manifest('window');
    return {generation:snapshot.current.generation,previous:snapshot.previous.generation,metadata:snapshot.current.metadata,windowGeneration:manifest.generation,cleanExit:manifest.clean_exit};
  })()`),{generation:3,previous:2,metadata:'{"camera":3,"downgrade":true}',windowGeneration:2,cleanExit:true},'The released writer can edit after upgrade reads and the new reader observes its next generation');
  assert.deepEqual(await browser.evaluate("(async()=>Array.from(await reader.range('drawing','edited',0,3)))()"),[9,8,7],'A newly opened released writer can publish edited resource bytes');
  assert.equal(await browser.evaluate(`(async()=>{
    const bytes=await reader.range('drawing','large',BLOCK-1,2),after=await readDatabase();IDBDatabase.prototype.transaction=transaction;
    return unchanged(afterDowngrade,after)&&after.version===1&&modes.length>0&&modes.every(mode=>mode==='readonly')&&bytes[0]===225&&bytes[1]===19;
  })()`),true,'Post-downgrade reads preserve all stored values and exact cross-block bytes');
  assert.deepEqual(browser.errors,[]);
  console.log('PASS legacy restart reader: released 1.0.15 writer, readonly copies, current/previous metadata, 4 MiB ranges, missing blocks, downgrade edits and absent database');
} finally {
  if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));
}
