import assert from 'node:assert/strict';
import {packageManifest,packageEvidence} from './package-fixture.test.mjs';

export async function checkBinaryTransfer({evaluate}, fixture) {
  assert.ok(fixture, 'Set LAYER_BINARY_FIXTURE_URL to the generated .capy fixture');
  const result = await evaluate(`(async()=>{
    const wasm=await import('./pkg/layer_web.js');
    const worker=new Worker('./raster-worker.js',{type:'module'});
    let next=0;const pending=new Map();const outputs=new Set();
    worker.onmessage=({data})=>{const p=pending.get(data.id);pending.delete(data.id);data.error?p.reject(Error(data.error)):p.resolve(data.result);};
    worker.onerror=e=>{for(const p of pending.values())p.reject(Error(e.message));pending.clear();};
    const request=(operation,metadata,buffers=[])=>new Promise((resolve,reject)=>{
      const id=++next;pending.set(id,{resolve,reject});
      worker.postMessage({id,request:{operation,metadata,buffers}},buffers.map(b=>b.buffer));
    });
    const options=JSON.stringify({dimension:16384,photo_policy:{},names:{paint:'Binary fixture',paper:'Paper'},intent:'Open'});
    const check=condition=>{if(!condition)throw Error('Invalid typed transfer fixture');};
    let expectedProfile;
    const inspect=wire=>{
      if(wire.package)throw Error('Binary fixture must admit editable artwork: '+wire.package.disposition+': '+wire.package.reason);
      check(typeof wire.metadata==='string' && Array.isArray(wire.buffers));
      const envelope=JSON.parse(wire.metadata),transfer=envelope.transfer;
      const starts=[];let at=0;
      for(const count of envelope.chunks){starts.push(at);at+=count;}
      check(at===wire.buffers.length && envelope.chunks.length===transfer.lengths.length);
      let largest=0,total=0;
      wire.buffers.forEach(bytes=>{check(bytes.byteLength<=4*1024*1024);largest=Math.max(largest,bytes.byteLength);total+=bytes.byteLength;});
      const payload=index=>wire.buffers.slice(starts[index],starts[index]+envelope.chunks[index]);
      transfer.lengths.forEach((length,index)=>check(payload(index).reduce((n,b)=>n+b.byteLength,0)===Number(length)));
      check(transfer.selections.length===1 && transfer.working_selection===null);
      const selection=transfer.selections[0];
      check(selection.extent[0]===9504 && selection.extent[1]===6336 && selection.bytes);
      let pixel=0;
      for(const bytes of payload(selection.payload))for(const value of bytes){
        const expected=(Math.floor((pixel%9504)/37)+Math.floor(Math.floor(pixel/9504)/19))&255;
        if(value!==expected)throw Error('Coverage mismatch at '+pixel);pixel++;
      }
      check(pixel===9504*6336);
      const objects=transfer.manifest.objects;
      const saved=objects.find(o=>o.type==='capy.selection/1');
      const coverage=objects.find(o=>o.type==='capy.coverage-source/1');
      check(saved && coverage && JSON.stringify(saved.data.shape.pixels.chunks)===JSON.stringify(coverage.data.initial.shape.pixels.chunks));
      check(JSON.stringify(selection.chunks)===JSON.stringify(saved.data.shape.pixels.chunks.map(c=>c.ref)));
      const profiles=transfer.manifest.resources.filter(r=>r.type==='capy.icc/1');check(profiles.length===1);
      const profile=profiles[0].id;
      const proof=objects.find(o=>o.type==='capy.output/1').data.proof;
      const original=objects.find(o=>o.type==='capy.paint-source/1').data.original;
      check(proof.profile.resource.ref===profile && original.interpretation.profile.resource.ref===profile);
      const resource=transfer.resources[profile];check(resource.kind==='bytes');
      let code=0;
      for(const bytes of payload(resource.payload))for(const value of bytes){if(value!==expectedProfile[code])throw Error('ICC mismatch at '+code);code++;}
      check(code===2*1024*1024 && code===expectedProfile.length);
      return {envelope,metadata:wire.metadata.length,payload:total,largest,decoded:pixel,profiles:profiles.length,bindings:2};
    };
    const hash=async blob=>Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',await blob.arrayBuffer()))).join(',');
    try {
      const download=async url=>{const response=await fetch(url);if(!response.ok)throw Error('Fixture download failed: '+url+': '+response.status);return new Uint8Array(await response.arrayBuffer());};
      const bytes=await download(${JSON.stringify(fixture)});expectedProfile=await download(${JSON.stringify(fixture + '.icc')});
      check(expectedProfile.length===2*1024*1024 && new DataView(expectedProfile.buffer).getUint32(0)===expectedProfile.length && new TextDecoder().decode(expectedProfile.subarray(36,40))==='acsp');
      const originalManifest=(${packageManifest.toString()})(bytes),originalEvidence=await (${packageEvidence.toString()})(bytes,originalManifest);
      const start=performance.now();const wire=await request('read',options,[bytes]);const read=performance.now()-start;
      const first=inspect(wire);const parts=[];let offset=0,largestWrite=0;
      const begin=performance.now();
      await wasm.raster_worker_write(wire.metadata,wire.buffers,(at,bytes)=>{
        check(at===offset && bytes.byteLength<=4*1024*1024);parts.push(bytes.slice());offset+=bytes.byteLength;
        largestWrite=Math.max(largestWrite,bytes.byteLength);return bytes.byteLength;
      });
      const main=performance.now()-begin;
      const saved=new Blob(parts),expectedHash=await hash(saved),savedBytes=new Uint8Array(await saved.arrayBuffer());
      const savedManifest=(${packageManifest.toString()})(savedBytes),savedEvidence=await (${packageEvidence.toString()})(savedBytes,savedManifest);
      const identity=manifest=>({objects:manifest.objects,resources:manifest.resources.map(({location,...resource})=>resource)});
      const exactOriginal=JSON.stringify(identity(savedManifest))===JSON.stringify(identity(originalManifest))&&JSON.stringify(savedEvidence)===JSON.stringify(originalEvidence);
      const again=await request('read',options,[new Uint8Array(await saved.arrayBuffer())]);const second=inspect(again);
      const finish=performance.now();const final=await request('write',again.metadata,again.buffers);outputs.add(final.token);
      const write=performance.now()-finish;const exact=expectedHash===await hash(final.blob);
      await request('output-close',final.token);outputs.delete(final.token);
      return {read_ms:read,main_roundtrip_ms:main,worker_write_ms:write,exact_archive:exact,exact_original:exactOriginal,
        metadata_bytes:first.metadata,payload_bytes:first.payload,archive_bytes:final.blob.size,
        largest_payload_chunk:first.largest,largest_archive_chunk:largestWrite,decoded_selection_bytes:first.decoded,
        selections:first.envelope.transfer.selections.length,selection_bindings:first.bindings,profiles:first.profiles,
        same_index:JSON.stringify(first.envelope)===JSON.stringify(second.envelope)};
    } finally {
      for(const token of outputs)await request('output-close',token).catch(()=>{});
      worker.terminate();
    }
  })()`);
  assert.equal(result.same_index, true, 'Lossless typed worker/main/worker round trip');
  assert.equal(result.exact_archive, true, 'All mask and ICC bytes survive reopening exactly');
  assert.equal(result.exact_original, true, 'Original authored records and every immutable resource identity, descriptor and byte survive the first round trip');
  assert.equal(result.selections, 1, 'Shared 61 MP authored coverage transfers once');
  assert.equal(result.selection_bindings, 2, 'Saved selection and layer mask share one payload');
  assert.equal(result.profiles, 1, 'Proof and original share one ICC resource');
  assert.equal(result.decoded_selection_bytes, 9504 * 6336);
  assert.ok(result.metadata_bytes < 1024 * 1024, 'No image-sized JSON arrays');
  assert.ok(result.payload_bytes < 90 * 1024 * 1024, 'Decoded coverage transfers once within the fixture memory bound');
  assert.ok(result.largest_payload_chunk <= 4 * 1024 * 1024);
  assert.ok(result.largest_archive_chunk <= 4 * 1024 * 1024, 'Archive writes use bounded JavaScript chunks');
  console.log('PASS binary 61 MP selection + 2 MiB ICC transfer', result);
  return result;
}
