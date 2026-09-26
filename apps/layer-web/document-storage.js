// Immutable live tile chunks, separate from recovery archives and cancellable
// import/export workers. Async OPFS APIs keep the browser event loop responsive.
export function createDocumentStorage() {
  const owner=crypto.randomUUID(),lockName=id=>`capy-live-tiles:${id}`;
  let ready,lease,serial=0,reading=0;const waiters=[];
  async function directory(){return ready??=initialize().catch(error=>{ready=null;throw error;});}
  async function initialize(){
    const root=await (await navigator.storage.getDirectory()).getDirectoryHandle('capy-live-tiles',{create:true});
    // A crashed page loses its lock. Never touch chunks belonging to a live
    // window; recovery files use an entirely separate store.
    for await(const [id,entry] of root.entries())if(entry.kind==='directory'&&id!==owner){
      await navigator.locks.request(lockName(id),{ifAvailable:true},async lock=>{if(lock)await root.removeEntry(id,{recursive:true});});
    }
    await (lease??=new Promise((resolve,reject)=>navigator.locks.request(lockName(owner),()=>{resolve();return new Promise(()=>{});}).catch(reject)));
    return root.getDirectoryHandle(owner,{create:true});
  }
  return async function transport({operation,metadata,buffers}){
    const dir=await directory();
    if(operation==='tab-write'){
      const bytes=buffers[0];if(!bytes?.byteLength||bytes.byteLength>8*1024*1024)throw Error('Invalid drawing cache chunk.');
      const key=String(++serial),file=await dir.getFileHandle(key,{create:true});let stream;
      try{stream=await file.createWritable();await stream.write(bytes);await stream.close();return key;}
      catch(error){try{await stream?.abort();}catch{}try{await dir.removeEntry(key);}catch{}throw Error(`Cannot park drawing in browser storage: ${error.message||error}`);}
    }
    if(!/^\d+$/.test(metadata))throw Error('Invalid drawing cache owner.');
    if(operation==='tab-release'){try{await dir.removeEntry(metadata);}catch(error){if(error.name!=='NotFoundError')throw error;}return;}
    if(operation==='tab-read'){
      if(reading>=2)await new Promise(resolve=>waiters.push(resolve));else reading++;
      try{const file=await(await dir.getFileHandle(metadata)).getFile();if(file.size>8*1024*1024)throw Error('Oversized drawing cache chunk.');return new Uint8Array(await file.arrayBuffer());}
      finally{const next=waiters.shift();if(next)next();else reading--;}
    }
    throw Error('Unknown drawing cache operation.');
  };
}
