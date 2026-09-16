// One compression worker and one file worker keep archive I/O independent of ink.
// GPU objects remain in the owning Wasm instance; only bounded byte blocks move.
export function createRasterWorker() {
  const owners = new Map();
  let next = 0;
  function owner(kind) {
    if (owners.has(kind)) return owners.get(kind);
    const worker = new Worker(new URL("./raster-worker.js", import.meta.url), {type:"module"});
    const pending = new Map();
    const fail = error => {
      for (const job of pending.values()) { clearTimeout(job.timer); job.reject(error); }
      pending.clear(); worker.terminate(); owners.delete(kind);
    };
    worker.onmessage = ({data}) => {
      const job = pending.get(data.id);
      if (!job) return;
      pending.delete(data.id); clearTimeout(job.timer);
      if (data.error) job.reject(new Error(data.error)); else job.resolve(data.result);
    };
    worker.onerror = event => { event.preventDefault(); fail(new Error(event.message || "Raster worker stopped")); };
    worker.onmessageerror = () => fail(new Error("Invalid raster worker response"));
    const state = {worker,pending,fail}; owners.set(kind,state); return state;
  }
  return request => new Promise((resolve,reject) => {
    const state = owner(request.operation === "encode" ? "codec" : "files"), id = ++next;
    // Full-resolution file codecs can outlast an interactive compression block.
    const timeout=request.operation === "encode" ? 30000 : 180000;
    const timer = setTimeout(() => state.fail(new Error("Raster worker timed out")), timeout);
    state.pending.set(id,{resolve,reject,timer});
    const transfers = (request.buffers || []).map(bytes => bytes.buffer);
    try { state.worker.postMessage({id,request},transfers); }
    catch(error) { clearTimeout(timer); state.pending.delete(id); reject(error); }
  });
}
