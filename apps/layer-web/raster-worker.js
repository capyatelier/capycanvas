import init, * as wasm from "./pkg/layer_web.js";
const ready = init();
let pending = Promise.resolve();
self.onmessage = ({data}) => { pending = pending.then(() => execute(data)); };
async function execute({id,request}) {
  let instance;
  const retire=()=>outputs.size===0 && (instance?.memory.buffer.byteLength || 0)>256*1024*1024;
  try {
    instance=await ready;
    let result;
    switch(request.operation) {
      case "encode": result = wasm.raster_worker_encode(request.metadata,request.buffers[0]); break;
      case "profile-library": result=await navigator.locks.request("capy-profile-library",()=>profileLibrary(JSON.parse(request.metadata),request.buffers[0]));break;
      case "export-presets": result=await navigator.locks.request("capy-export-presets",async()=>{
        const bytes=await colorPreferences("readonly",store=>store.get("export-presets"));
        const prepared=wasm.raster_worker_export_presets(request.metadata,bytes??new Uint8Array());
        if(prepared.bytes)await colorPreferences("readwrite",store=>store.put(prepared.bytes,"export-presets"));
        return prepared.view;
      });break;
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
        if(metadata.preview||metadata.flatten) {
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
      case "output-close": await closeOutput(request.metadata); result=true;break;
      case "recover-list": result = await recovery("readonly", store=>store.getAllKeys()); break;
      case "recover-get": result = await recovery("readonly", store=>store.get(request.metadata)); break;
      case "recover-delete": await recovery("readwrite", store=>store.delete(request.metadata)); result=true; break;
      case "recover-write": {
        const {key,project}=JSON.parse(request.metadata);
        const bytes=await wasm.raster_worker_write(project,request.buffers);
        await recovery("readwrite",store=>store.put(bytes,key)); result=true; break;
      }
      case "write": result = await wasm.raster_worker_write(request.metadata,request.buffers); break;
      default: throw new Error("Unknown raster worker operation");
    }
    self.postMessage({id,result,retire:retire()},result instanceof Uint8Array ? [result.buffer] : (result?.buffers || []).map(bytes=>bytes.buffer));
  } catch(error) { self.postMessage({id,error:String(error),retire:retire()}); }
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
function outputJob(token) { const job=outputs.get(token);if(!job)throw new Error("Output job is no longer available");return job; }
async function closeOutput(token) {
  const job=outputs.get(token);if(!job)return;
  outputs.delete(token);try{job.raw?.close();await job.root.removeEntry(token,{recursive:true});}finally{job.release();}
}

// One transaction replaces the previous complete checkpoint. Quota, worker or
// tab failure before commit leaves that checkpoint intact. No file handles or
// undo history are persisted; recovery remains an unsaved document.
async function recovery(mode, operation) {
  const database=await new Promise((resolve,reject)=>{
    const request=indexedDB.open("capy-raster-recovery",1);
    request.onupgradeneeded=()=>request.result.createObjectStore("projects");
    request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);
  });
  try { return await new Promise((resolve,reject)=>{
    const transaction=database.transaction("projects",mode,{durability:"strict"});
    const request=operation(transaction.objectStore("projects"));
    transaction.oncomplete=()=>resolve(request.result);
    transaction.onabort=()=>reject(transaction.error || request.error || new Error("Recovery transaction aborted"));
    transaction.onerror=()=>{};
  }); } finally { database.close(); }
}

async function colorPreferences(mode,operation,storeName="values") {
  const database=await new Promise((resolve,reject)=>{
    const request=indexedDB.open("capy-color-preferences",2);
    request.onupgradeneeded=()=>{for(const name of ["values","profiles"])if(!request.result.objectStoreNames.contains(name))request.result.createObjectStore(name);};
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
    if(!data)throw new Error("Profile is no longer in the library");
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
    try{if(!entry.issue)data=await read(entry.id);}catch(e){error=String(e);}
    entries.push(policy({type:"inspect",entry,error},data));
  }
  return entries.sort((a,b)=>a.name.localeCompare(b.name)||a.id.localeCompare(b.id));
}
