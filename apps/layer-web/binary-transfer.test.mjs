import assert from 'node:assert/strict';

// Run with a generated fixture from `cargo run -p layer-core --release
// --example binary_transfer -- <served path>`. Exercises real worker heaps and
// transferable buffers, without modifying the open document or any artwork.
export async function checkBinaryTransfer({evaluate}, fixture) {
  assert.ok(fixture, 'Set LAYER_BINARY_FIXTURE_URL to the generated .capy fixture');
  const result = await evaluate(`(async()=>{
    const wasm=await import('./pkg/layer_web.js');
    const worker=new Worker('./raster-worker.js',{type:'module'});
    let next=0;const pending=new Map();
    worker.onmessage=({data})=>{const p=pending.get(data.id);pending.delete(data.id);data.error?p.reject(Error(data.error)):p.resolve(data.result);};
    worker.onerror=e=>{for(const p of pending.values())p.reject(Error(e.message));pending.clear();};
    const request=(operation,metadata,buffers)=>new Promise((resolve,reject)=>{
      const id=++next;pending.set(id,{resolve,reject});
      worker.postMessage({id,request:{operation,metadata,buffers}},buffers.map(b=>b.buffer));
    });
    const options=JSON.stringify({dimension:16384,photo_policy:{},name:'Binary fixture',intent:'Open'});
    try {
      const bytes=new Uint8Array(await(await fetch(${JSON.stringify(fixture)})).arrayBuffer());
      const start=performance.now();
      const wire=await request('read',options,[bytes]);
      const read=performance.now()-start;
      const meta=JSON.parse(wire.metadata);
      const payload=wire.buffers.reduce((n,b)=>n+b.length,0);
      let pixel=0;
      for(const index of meta.selections.pixels[0].chunks) {
        for(const value of wire.buffers[index]) {
          const expected=pixel<9504*6336 ? (Math.floor((pixel%9504)/37)+Math.floor(Math.floor(pixel/9504)/19))&255 : 0;
          if(value!==expected)throw Error('Coverage mismatch at '+pixel);
          pixel++;
        }
      }
      const hash=async bytes=>Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes))).join(',');
      const begin=performance.now();
      // Exercise unpack in the editor's actual Wasm heap, then use the archive
      // writer to verify its output. Production archive writing is worker-only.
      const saved=await wasm.raster_worker_write(wire.metadata,wire.buffers);
      const main=performance.now()-begin;
      const expectedHash=await hash(saved);
      const again=await request('read',options,[saved]);
      const reopened=JSON.parse(again.metadata);
      const finish=performance.now();
      const final=await request('write',again.metadata,again.buffers);
      const write=performance.now()-finish;
      const exact=expectedHash===await hash(final);
      return {read_ms:read,main_roundtrip_ms:main,worker_write_ms:write,exact_archive:exact,
        metadata_bytes:wire.metadata.length,payload_bytes:payload,archive_bytes:final.length,
        selections:meta.selections,profiles:meta.profiles,proof:meta.proof,originals:meta.originals,
        same_index:JSON.stringify(meta)===JSON.stringify(reopened)};
    } finally {worker.terminate();}
  })()`);
  assert.equal(result.same_index, true, 'Lossless worker/main/worker round trip');
  assert.equal(result.exact_archive, true, 'All mask and ICC bytes survive reopening exactly');
  assert.equal(result.selections.pixels.length, 1, 'Shared 61 MP coverage transfers once');
  assert.equal(result.selections.bindings.length, 3, 'Current, saved selection and layer mask survive');
  assert.equal(result.profiles.length, 1);
  assert.equal(result.proof.profile.Embedded, 0);
  assert.equal(result.originals[0].interpretation.profile.Embedded, 0, 'Proof and original share one ICC buffer');
  assert.ok(result.metadata_bytes < 1024 * 1024, 'No image-sized JSON arrays');
  delete result.selections;delete result.profiles;delete result.proof;delete result.originals;
  console.log('PASS binary 61 MP selection + 2 MiB ICC transfer', result);
  return result;
}
