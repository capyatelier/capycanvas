import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {launchChrome} from '../../tools/cdp.mjs';

const root=fileURLToPath(new URL('./',import.meta.url));
const pkg=process.env.CAPY_WEB_TEST_PKG?resolve(process.env.CAPY_WEB_TEST_PKG):resolve(root,'pkg');
const fixture=fileURLToPath(new URL('../../crates/layer-core/src/package/codec/fixtures/deflate-resources.capy',import.meta.url));
const server=createServer(async(request,response)=> {
  try {
    const pathname=new URL(request.url,'http://private.test').pathname;
    if(pathname==='/'){response.setHeader('Content-Type','text/html');response.end('<!doctype html><link rel="icon" href="data:,"><title>Leased package test</title>');return;}
    const path=pathname==='/fixture.capy'?fixture:pathname.startsWith('/pkg/')?resolve(pkg,pathname.slice(5)):resolve(root,pathname.slice(1));
    if(path!==fixture&&!path.startsWith(`${pkg}/`)&&!path.startsWith(root))throw Error('Invalid test resource');
    response.setHeader('Content-Type',path.endsWith('.wasm')?'application/wasm':path.endsWith('.js')?'text/javascript':'application/octet-stream');
    response.end(await readFile(path));
  }catch(error){response.writeHead(404);response.end(String(error));}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));let browser;
try {
  browser=await launchChrome(['--headless=new','--disable-gpu']);await browser.attachPage();await browser.call('Runtime.enable');await browser.call('Page.enable');
  const loaded=browser.once('Page.loadEventFired');await browser.call('Page.navigate',{url:`http://127.0.0.1:${server.address().port}/`});await loaded;
  const result=await browser.evaluate(`(async()=> {
    const [{createStoreWorker},{createRasterWorker},wasm]=await Promise.all([import('/store-worker-client.js'),import('/raster-worker-client.js'),import('/pkg/layer_web.js')]);
    const module=await WebAssembly.compileStreaming(fetch('/pkg/layer_web_bg.wasm'));
    const NativeWorker=Worker,workers=[];
    window.Worker=class extends NativeWorker {constructor(url,options){super(url,options);this.kind=String(url);workers.push(this);}terminate(){this.ended=true;super.terminate();}};
    const storage=createStoreWorker(()=>{}),files=createRasterWorker(module,()=>{},storage);
    const file=new File([await(await fetch('/fixture.capy')).blob()],'leased.capy');
    const registered=await storage({operation:'register_blob',file});
    const opened=await files({source_key:registered.key,operation:'read',metadata:JSON.stringify({dimension:16384,photo_policy:{},names:{paint:'Ink',paper:'Paper'},intent:'Open'}),buffers:[],file});
    if(opened.package)throw Error('Fixture unexpectedly preserved: '+JSON.stringify(opened.package));
    const envelope=JSON.parse(opened.metadata),references=envelope.transfer.backings;
    const tiles=Object.entries(envelope.transfer.resources).filter(([,resource])=>resource.kind==='tile_range');
    if(tiles.length!==3)throw Error('Expected all three fixture tiles as references');
    if(Object.values(envelope.transfer.resources).some(resource=>resource.kind==='tile'))throw Error('Eager tile payload');
    const parser=workers.find(worker=>worker.kind.endsWith('/raster-worker.js'));
    if(!parser?.ended)throw Error('Parse worker still running');
    const leases=await storage({operation:'adopt_sources',metadata:{ticket:envelope.offer,references}});
    await storage({operation:'release_source',metadata:{lease:registered.lease}});
    const [id,range]=tiles.find(([id])=>envelope.transfer.manifest.resources.find(record=>record.id===id).data.channels==='rgba');
    const data=envelope.transfer.manifest.resources.find(record=>record.id===id).data;
    const descriptor={channels:4,bits_per_channel:16,encoding:'Profile',alpha:'Straight'};
    if(data.depth!=='u16'||data.alpha!=='straight'||!range.filtered)throw Error('Unexpected fixture tile descriptor');
    const response=await storage({operation:'read_source',metadata:{lease:leases[range.backing],offset:range.offset,length:range.original.bytes}});
    const normalized=await files({operation:'package-normalize',metadata:JSON.stringify([descriptor,range.original.crc32]),buffers:response.buffers});
    const pixels=await files({operation:'image-decode',metadata:JSON.stringify({Raster:{descriptor,space:'DisplayP3',native_channels:true}}),buffers:[normalized]});
    for(const lease of leases)await storage({operation:'release_source',metadata:{lease}});
    let expired=false;
    try{await storage({operation:'offer_sources',metadata:{references}});}catch(error){expired=/no longer available/.test(error.message);}
    return {tiles:tiles.length,eager_tile_chunks:0,parser_ended:parser.ended,decoded_bytes:pixels.byteLength,storage_workers:workers.filter(worker=>worker.kind.endsWith('/store-worker.js')).length,expired};
  })()`);
  assert.equal(result.tiles,3);assert.equal(result.eager_tile_chunks,0);assert.equal(result.parser_ended,true);
  assert.equal(result.decoded_bytes,256*256*8);assert.equal(result.storage_workers,1);assert.equal(result.expired,true);
  assert.deepEqual(browser.errors,[]);
  console.log('PASS actual package parser terminated; unseen filtered tile read and decoded through the single storage owner',JSON.stringify(result));
}finally{if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));}
