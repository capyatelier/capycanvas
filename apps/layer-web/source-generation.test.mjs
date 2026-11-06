import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {launchChrome} from '../../tools/cdp.mjs';

const modules=new Map(await Promise.all(['store-worker-client.js','store-worker.js','store-sources.js','legacy-restart-store.js','file-handles.js'].map(async path=>['/'+path,await readFile(new URL(path,import.meta.url))])));
const server=createServer((request,response)=>{const source=modules.get(request.url);response.setHeader('Content-Type',source?'text/javascript':'text/html');response.end(source??'<!doctype html><link rel="icon" href="data:,"><title>Private source generation test</title>');});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));let browser;
try {
  browser=await launchChrome(['--headless=new','--disable-gpu']);await browser.attachPage();await browser.call('Runtime.enable');await browser.call('Page.enable');
  const loaded=browser.once('Page.loadEventFired');await browser.call('Page.navigate',{url:`http://127.0.0.1:${server.address().port}/`});await loaded;
  const result=await browser.evaluate(`(async()=> {
    const {createStoreWorker}=await import('/store-worker-client.js'),storage=createStoreWorker(()=>{});
    const root=await navigator.storage.getDirectory(),name='source-generation-'+crypto.randomUUID(),directory=await root.getDirectoryHandle(name,{create:true});
    const registered=[],leases=[];
    try {
      const handle=await directory.getFileHandle('drawing.capy',{create:true});
      let writer=await handle.createWritable();await writer.write(new Uint8Array(128*1024).fill(0x5a));await writer.close();
      const original=await handle.getFile();
      for(const file of [original,original.slice(),new Blob([original])])registered.push(await storage({operation:'register_blob',file}));
      let openedDirectory=false;try {await root.getDirectoryHandle('capy-session-store');openedDirectory=true;}catch(error){if(error.name!=='NotFoundError')throw error;}
      const references=registered.map((source,index)=>({key:source.key,generation:'original-generation-'+index,length:original.size}));
      const ticket=await storage({operation:'offer_sources',metadata:{references}});
      leases.push(...await storage({operation:'adopt_sources',metadata:{ticket,references}}));
      const bound=(await storage({operation:'read_source',metadata:{lease:leases[0],offset:0,length:8}})).buffers[0];
      const store='generation-test/pack';await storage({operation:'open',metadata:{name:store,access:'create'}});
      await storage({operation:'write_at',metadata:{name:store,offset:0},buffers:[bound]});await storage({operation:'flush',metadata:{name:store}});
      const storedReference={key:'store:'+store,generation:'stored',length:8};
      const storedTicket=await storage({operation:'offer_sources',metadata:{references:[storedReference]}});
      const [stored]=await storage({operation:'adopt_sources',metadata:{ticket:storedTicket,references:[storedReference]}});leases.push(stored);
      writer=await handle.createWritable();await writer.write(new Uint8Array(128*1024).fill(0xa5));await writer.close();
      const lost=[];for(let index=0;index<3;index++)try {
        await storage({operation:'read_source',metadata:{lease:leases[index],offset:64*1024,length:8}});lost.push(null);
      }catch(error){lost.push({message:error.message,key:registered[index].key});}
      await directory.removeEntry('drawing.capy');
      const durable=[...(await storage({operation:'read_source',metadata:{lease:stored,offset:0,length:8}})).buffers[0]];
      for(const lease of leases.splice(0))await storage({operation:'release_source',metadata:{lease}});
      for(const source of registered.splice(0))await storage({operation:'release_source',metadata:{lease:source.lease}});
      await storage({operation:'close_tree',metadata:{name:'generation-test'}});await storage({operation:'remove',metadata:{name:store}});
      return {openedDirectory,lost,durable};
    }finally {
      for(const lease of leases)await storage({operation:'release_source',metadata:{lease}});
      for(const source of registered)await storage({operation:'release_source',metadata:{lease:source.lease}});
      await root.removeEntry(name,{recursive:true});
    }
  })()`);
  assert.equal(result.openedDirectory,false,'registering File generations performs no OPFS copy');
  for(const error of result.lost){assert.ok(error);assert.match(error.message,/source.*lost/i);assert.ok(error.message.includes(error.key));}
  assert.deepEqual(result.durable,Array(8).fill(0x5a));assert.deepEqual(browser.errors,[]);
  console.log('PASS external replacement fails unread source ranges explicitly; bound Store bytes survive',JSON.stringify(result));
}finally{if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));}
