import init, * as wasm from "./pkg/layer_web.js";
const ready = init();
let pending = Promise.resolve();
self.onmessage = ({data}) => { pending = pending.then(() => execute(data)); };
async function execute({id,request}) {
  try {
    await ready;
    let result;
    switch(request.operation) {
      case "encode": result = wasm.raster_worker_encode(request.metadata,request.buffers[0]); break;
      case "read": result = await wasm.raster_worker_read(Number(request.metadata),request.buffers[0]); break;
      case "png": result = wasm.raster_worker_png(request.metadata,request.buffers); break;
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
    self.postMessage({id,result},result instanceof Uint8Array ? [result.buffer] : (result?.buffers || []).map(bytes=>bytes.buffer));
  } catch(error) { self.postMessage({id,error:String(error)}); }
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
