// One compression worker and one file worker keep archive I/O independent of ink.
// GPU objects remain in the owning Wasm instance; only bounded byte blocks move.
export function createRasterWorker() {
  const owners = new Map();
  let next = 0;
  const outputs=new Map(),reads=new Set();
  function owner(kind) {
    if (owners.has(kind)) return owners.get(kind);
    const worker = new Worker(new URL("./raster-worker.js", import.meta.url), {type:"module"});
    const pending = new Map();
    let closed=false;
    let idleTimer;
    const fail = error => {
      closed=true;
      for (const job of pending.values()) { clearTimeout(job.timer); job.reject(error); }
      clearTimeout(idleTimer); pending.clear(); worker.terminate(); owners.delete(kind);
    };
    worker.onmessage = ({data}) => {
      const job = pending.get(data.id);
      if (!job) return;
      pending.delete(data.id); clearTimeout(job.timer);
      if (data.error) job.reject(new Error(data.error)); else job.resolve(data.result);
      // Wasm heaps cannot shrink. Release an oversized idle file arena after
      // its transferred result is owned by the editor and no OPFS job is live.
      if(kind==="files" && data.retire && !pending.size)idleTimer=setTimeout(()=>{
        if(!pending.size){worker.terminate();owners.delete(kind);}
      },5000);
    };
    worker.onerror = event => { event.preventDefault(); fail(new Error(event.message || "Raster worker stopped")); };
    worker.onmessageerror = () => fail(new Error("Invalid raster worker response"));
    const state = {worker,pending,fail,get closed(){return closed;},active(){clearTimeout(idleTimer);}}; owners.set(kind,state); return state;
  }
  function send(state,request,cancelled) {
    return new Promise((resolve,reject)=>{
      const id=++next;
      state.active();
      const timer=setTimeout(()=>state.fail(new Error("Raster worker timed out")),request.operation==="encode"?30000:180000);
      const poll=cancelled?setInterval(()=>{if(cancelled())state.fail(new DOMException("Image operation cancelled","AbortError"));},50):null;
      const finish=callback=>value=>{clearInterval(poll);callback(value);};
      state.pending.set(id,{resolve:finish(resolve),reject:finish(reject),timer});
      try{state.worker.postMessage({id,request},(request.buffers||[]).map(bytes=>bytes.buffer));}
      catch(error){clearInterval(poll);clearTimeout(timer);state.pending.delete(id);reject(error);}
    });
  }
  return async ({cancelled,...request})=>{
    const op=request.operation;
    if(op==='cancel-read'){for(const state of reads)state.fail(new DOMException('Opening cancelled','AbortError'));reads.clear();return true;}
    if(op==='write'||op==='recover-write'){const state=owner(`archive:${++next}`);try{return await send(state,request,cancelled);}finally{state.fail(new DOMException('Archive finished','AbortError'));}}
    if(op==='read'){
      const state=owner(`read:${++next}`);reads.add(state);
      try{return await send(state,request,cancelled);}finally{reads.delete(state);state.fail(new DOMException('Reading finished','AbortError'));}
    }
    if(op==='output-begin'){
      if(outputs.size>=2)throw Error('Finish the current output before starting another');
      const state=owner(`output:${++next}`);
      try{const token=await send(state,request,cancelled);outputs.set(token,state);return token;}
      catch(e){state.fail(e);throw e;}
    }
    if(op.startsWith('output-')){
      const metadata=op==='output-encode'?JSON.parse(request.metadata):null;
      const token=metadata?.token??request.metadata,state=outputs.get(token);
      if(!state||state.closed){outputs.delete(token);if(op==='output-close')return send(owner('files'),{operation:'output-discard',metadata:token,buffers:[]});throw Error('Output job is no longer available');}
      try{return await send(state,request,cancelled);}
      finally{
        if(op==='output-close'||(op==='output-encode'&&(cancelled?.()||metadata.preview||metadata.flatten))){outputs.delete(token);state.fail(new DOMException('Output finished','AbortError'));}
      }
    }
    if(op==='tone'){const state=owner(`analysis:${++next}`);try{return await send(state,request,cancelled);}finally{state.fail(new DOMException('Analysis finished','AbortError'));}}
    return send(owner(op==='encode'?'codec':'files'),request,cancelled);
  };
}
