import init, * as wasm from "./pkg/layer_web.js";
import {createRestartStore,verifyRestartCheckpoint} from "./restart-store.js";
const restart=createRestartStore();
let ready;
let pending = Promise.resolve();
self.onmessage = ({data}) => { pending = pending.then(() => execute(data)); };
async function execute({id,request}) {
  let instance;
  const retire=()=>outputs.size===0 && (instance?.memory.buffer.byteLength || 0)>256*1024*1024;
  try {
    // These operations only touch IndexedDB. Recovery discovery must not
    // download/instantiate the image codec before listing a few keys.
    if (!request.operation.startsWith('restart-store-'))
      instance=await (ready ??= init());
    let result;
    switch(request.operation) {
      case "fingerprint": {
        const fingerprint=new wasm.WebFileFingerprint(),reader=request.file.stream().getReader();
        try {for(;;){const {done,value}=await reader.read();if(done)break;fingerprint.update(value);}result=fingerprint.finish();}
        finally {fingerprint.free();reader.releaseLock();}
        break;
      }
      case "restart-store-list": result=await restart.windows();break;
      case "restart-store-manifest": result=await restart.manifest(request.metadata);break;
      case "restart-store-publish-manifest": {const {key,manifest,closed}=JSON.parse(request.metadata);result=await restart.publishManifest(key,manifest,closed);break;}
      case "restart-store-transfer-manifest": {const {source,sourceManifest,destination,destinationManifest,key,sourceGeneration,destinationGeneration}=JSON.parse(request.metadata);result=await restart.transferManifest(source,sourceManifest,destination,destinationManifest,key,sourceGeneration,destinationGeneration);break;}
      case "restart-store-remove-manifest": result=await restart.removeManifest(request.metadata);break;
      case "restart-store-read": result=await restart.read(request.metadata);break;
      case "restart-store-resources": {const {key,ids}=JSON.parse(request.metadata);result={buffers:await restart.resources(key,ids)};break;}
      case "restart-store-publish": {const {key,checkpoint,ids,base_generation}=JSON.parse(request.metadata);result=await restart.publish(key,checkpoint,ids.map((id,index)=>({id,bytes:request.buffers[index]})),request.handles,base_generation);break;}
      case "restart-store-remove": result=await restart.remove(request.metadata);break;
      case "restart-begin": result=wasm.raster_worker_restart_begin(request.metadata);break;
      case "restart-retain": wasm.raster_worker_restart_retain(request.metadata);result=true;break;
      case "restart-open": {
        await verifyRestartCheckpoint(JSON.parse(request.metadata));
        result=await wasm.raster_worker_restart_open(request.metadata,request.buffers);break;
      }
      case "restart-write": {
        const prepared=await wasm.raster_worker_restart_write(request.metadata,request.buffers);
        const {key,checkpoint,ids,base_generation}=JSON.parse(prepared.metadata);
        result=await restart.publish(key,checkpoint,ids.map((id,index)=>({id,bytes:prepared.buffers[index]})),request.handles,base_generation);
        break;
      }
      case "color-field": result = wasm.raster_worker_color_field(request.metadata); break;
      case "encode": result = wasm.raster_worker_encode(request.metadata,request.buffers[0]); break;
      case "lookup": result = wasm.raster_worker_lookup(request.metadata,request.buffers[0]); break;
      case "profile-library": result=await navigator.locks.request("capy-profile-library",()=>profileLibrary(JSON.parse(request.metadata),request.buffers[0]));break;
      case "export-presets": result=await navigator.locks.request("capy-export-presets",async()=>{
        const bytes=await colorPreferences("readonly",store=>store.get("export-presets"));
        const prepared=wasm.raster_worker_export_presets(request.metadata,bytes instanceof Uint8Array?bytes:new Uint8Array());
        if(prepared.bytes)await colorPreferences("readwrite",store=>store.put(prepared.bytes,"export-presets"));
        return prepared.view;
      });break;
      case "palette-file": { const [metadata,bytes]=wasm.raster_worker_palette_file(request.metadata,request.buffers[0]); result={metadata,bytes}; break; }
      case "snapshot": result = await wasm.raster_worker_snapshot(request.metadata,request.buffers); break;
      case "properties": result = wasm.raster_worker_properties(request.metadata); break;
      case "source-profile": result = wasm.raster_worker_source_profile(request.metadata); break;
      case "source-rasterize": result = await wasm.raster_worker_source_rasterize(request.metadata,request.buffers); break;
      case "color-convert": result = await wasm.raster_worker_color(request.metadata,request.buffers); break;
      case "read": result = await wasm.raster_worker_read(request.metadata,request.buffers[0]); break;
      case "output-begin": result = await beginOutput(); break;
      case "output-band": {
        const job=outputJob(request.metadata),bytes=request.buffers[0];
        if(bytes.length>32*1024*1024 || job.offset+bytes.length>4*1024*1024*1024)throw new Error("Output capture exceeds its file budget");
        if(job.raw.write(bytes,{at:job.offset})!==bytes.length)throw new Error("Incomplete output capture write");
        job.offset+=bytes.length;result=true;break;
      }
      case "output-encode": {
        const metadata=JSON.parse(request.metadata),job=outputJob(metadata.token);
        if(!metadata.original && job.offset!==metadata.extent[0]*metadata.extent[1]*16)throw new Error("Incomplete output capture");
        if(metadata.preview||metadata.flatten||metadata.clip) {
          try {result=await wasm.raster_worker_output(request.metadata,request.buffers,(offset,size)=>{
            const bytes=new Uint8Array(size);if(job.raw.read(bytes,{at:offset})!==size)throw new Error("Incomplete output row");return bytes;
          },()=>{throw new Error("Prepared output cannot write an image file");});}finally{await closeOutput(metadata.token);}
          break;
        }
        const handle=await job.directory.getFileHandle("image",{create:true}),output=await handle.createSyncAccessHandle();
        let statistics;
        try {
          statistics=await wasm.raster_worker_output(request.metadata,request.buffers,(offset,size)=>{
            const bytes=new Uint8Array(size);if(job.raw.read(bytes,{at:offset})!==size)throw new Error("Incomplete output row");return bytes;
          },(offset,bytes)=>output.write(bytes,{at:offset}));
          output.flush();
        } finally { output.close(); }
        job.raw.close();job.raw=null;await job.directory.removeEntry("capture");
        result={token:metadata.token,blob:await handle.getFile(),statistics};break;
      }
      case "output-discard": {
        const root=await(await navigator.storage.getDirectory()).getDirectoryHandle("capy-output",{create:true});
        await navigator.locks.request(`capy-output:${request.metadata}`,()=>root.removeEntry(request.metadata,{recursive:true}).catch(e=>{if(e.name!=="NotFoundError")throw e;}));result=true;break;
      }
      case "output-close": await closeOutput(request.metadata); result=true;break;
      case "write": { const output=await writePackage(request.metadata,request.buffers); result=output; break; }
      default: throw new Error("Unknown raster worker operation");
    }
    self.postMessage({id,result,retire:retire()},result instanceof Uint8Array ? [result.buffer] : [...(result?.bytes instanceof Uint8Array ? [result.bytes] : result?.waveform_counts ? [result.waveform_counts] : result?.buffers || []),...(result?.preview instanceof Uint8Array ? [result.preview] : [])].map(bytes=>bytes.buffer));
  } catch(error) { self.postMessage({id,error:String(error),color_feature_error:error?.color_feature_error,retire:retire()}); }
}

const outputs=new Map();
let outputRoot;
async function outputDirectory() {
  if(!outputRoot)outputRoot=(async()=>{
    const root=await(await navigator.storage.getDirectory()).getDirectoryHandle("capy-output",{create:true});
    // A worker/tab crash can leave temporary files. Live jobs hold a Web Lock,
    // so another tab's active delivery is never removed by this cleanup.
    for await(const name of root.keys())await navigator.locks.request(`capy-output:${name}`,{ifAvailable:true},async lock=>{
      if(lock)await root.removeEntry(name,{recursive:true});
    });
    return root;
  })();
  return outputRoot;
}
async function beginOutput() {
  if(outputs.size>=2)throw new Error("Finish the current output before starting another");
  const root=await outputDirectory(),token=crypto.randomUUID();let release;
  await new Promise((ready,reject)=>navigator.locks.request(`capy-output:${token}`,()=>{ready();return new Promise(r=>release=r)}).catch(reject));
  try {
    const directory=await root.getDirectoryHandle(token,{create:true}),handle=await directory.getFileHandle("capture",{create:true});
    const raw=await handle.createSyncAccessHandle();outputs.set(token,{root,directory,raw,offset:0,release});return token;
  } catch(error) { release();await root.removeEntry(token,{recursive:true}).catch(()=>{});throw error; }
}
async function writePackage(metadata,buffers) {
  const token=await beginOutput(),job=outputJob(token);
  try {
    const fingerprint=await wasm.raster_worker_write(metadata,buffers,(offset,bytes)=>job.raw.write(bytes,{at:offset}));
    job.raw.flush();job.raw.close();job.raw=null;
    const handle=await job.directory.getFileHandle("capture");
    return {token,blob:await handle.getFile(),fingerprint};
  } catch(error) { await closeOutput(token);throw error; }
}
function outputJob(token) { const job=outputs.get(token);if(!job)throw new Error("Output job is no longer available");return job; }
async function closeOutput(token) {
  const job=outputs.get(token);if(!job)return;
  outputs.delete(token);try{job.raw?.close();await job.root.removeEntry(token,{recursive:true});}finally{job.release();}
}

async function colorPreferences(mode,operation,storeName="values") {
  const database=await new Promise((resolve,reject)=>{
    const request=indexedDB.open("capy-color-preferences",2);
    request.onupgradeneeded=()=>{
      const database=request.result;
      for(const name of [...database.objectStoreNames])database.deleteObjectStore(name);
      for(const name of ["values","profiles"])database.createObjectStore(name);
    };
    request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);
  });
  try{return await new Promise((resolve,reject)=>{
    const transaction=database.transaction(storeName,mode,{durability:"strict"}),request=operation(transaction.objectStore(storeName));
    transaction.oncomplete=()=>resolve(request.result);transaction.onabort=()=>reject(transaction.error||request.error||new Error("Color preferences were not saved"));transaction.onerror=()=>{};
  });}finally{database.close();}
}

async function profileLibrary({operation,id},bytes) {
  const store=(mode,op)=>colorPreferences(mode,op,"profiles");
  const policy=(action,data=new Uint8Array())=>wasm.raster_worker_profile_library(JSON.stringify(action),data);
  const profile=({name,channels,profile})=>({name,channels,profile});
  const hidden=policy({type:"visibility",hidden:await colorPreferences("readonly",s=>s.get("profile-menu-hidden"))??[]});
  if(operation==="show"||operation==="hide"||operation==="remove"){
    const next=policy({type:"visibility",hidden,id,visible:operation!=="hide"});
    await colorPreferences("readwrite",s=>s.put(next,"profile-menu-hidden"));
    if(operation!=="remove")return true;
  }
  const inventory=async()=>{
    const entries=[];
    for(const id of await store("readonly",s=>s.getAllKeys())) {
      const data=await store("readonly",s=>s.get(id));entries.push({id,bytes:data?.length??0});
    }
    return policy({type:"inventory",entries});
  };
  const read=async id=>{
    const key=policy({type:"remove",id}).id;
    const data=await store("readonly",s=>s.get(key));
    if(!data)throw {color_feature_error:"ProfileMissing"};
    return data;
  };
  if(operation==="get")return profile(policy({type:"get",id},await read(id)));
  if(operation==="remove"){
    const key=policy({type:"remove",id}).id;
    await store("readwrite",s=>s.delete(key));return true;
  }
  if(operation==="import"){
    const entry=policy({type:"import",entries:await inventory()},bytes);
    await store("readwrite",s=>s.put(bytes,entry.id));return profile(entry);
  }
  if(operation!=="list")throw new Error("Unknown profile library action");
  const entries=[];
  for(const entry of await inventory()){
    let data=new Uint8Array(),error=null;
    try{if(!entry.issue)data=await read(entry.id);}catch(e){error=e?.color_feature_error??{Diagnostic:String(e)};}
    entries.push({...policy({type:"inspect",entry,error},data),visible:!hidden.includes(entry.id)});
  }
  return entries.sort((a,b)=>a.name.localeCompare(b.name)||a.id.localeCompare(b.id));
}
