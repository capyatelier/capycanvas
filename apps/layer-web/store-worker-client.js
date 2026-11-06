export function createPortTransport(port,wake=()=>{},onFailure=()=>{},offerParent=()=>null) {
  const pending=new Map();let next=0,failed;
  function fail(error) {if(failed)return;failed=error;for(const job of pending.values())job.reject(failed);pending.clear();port.close();wake();onFailure(error);}
  port.onmessageerror=()=>fail(Error('Invalid drawing storage response'));
  port.onmessage=({data})=> {
    if(data.fatal){fail(Error(data.fatal));return;}
    const job=pending.get(data.id);if(!job)return;pending.delete(data.id);
    if(data.error!==undefined){const error=Error(data.error);if(data.errorName)error.name=data.errorName;job.reject(error);}else job.resolve(data.result);
    if(job.operation==='read_source'||job.operation==='read_at')wake();
  };
  port.start();
  return request=> {
    if(request.operation==='offer_sources'&&offerParent())request={...request,metadata:{...request.metadata,parent:offerParent()}};
    if(failed)return Promise.reject(failed);
    return new Promise((resolve,reject)=> {
      const id=++next;pending.set(id,{resolve,reject,operation:request.operation});
      try {port.postMessage({id,request},(request.buffers??[]).map(bytes=>bytes.buffer));}
      catch(error){pending.delete(id);reject(error);}
    });
  };
}

export function createStoreWorker(wake) {
  const worker=new Worker(new URL('./store-worker.js',import.meta.url),{type:'module'});
  const pending=new Map(),claims=new Map(),connections=new Map();
  let next=0,failed;
  function fail(error) {
    failed??=error;
    for(const job of pending.values())job.reject(failed);
    pending.clear();for(const connection of connections.values())connection(error);connections.clear();worker.terminate();wake();
  }
  worker.onerror=event=>{event.preventDefault();fail(Error(event.message||'Drawing storage worker stopped'));};
  worker.onmessageerror=()=>fail(Error('Invalid drawing storage response'));
  worker.onmessage=({data})=>{
    const job=pending.get(data.id);if(!job)return;
    pending.delete(data.id);
    if(data.error!==undefined){const error=Error(data.error);if(data.errorName)error.name=data.errorName;job.reject(error);}else job.resolve(data.result);
    if(job.operation==='read_source'||job.operation==='read_at')wake();
  };
  function send(request) {
    if(failed)return Promise.reject(failed);
    return new Promise((resolve,reject)=>{
      const id=++next;pending.set(id,{resolve,reject,operation:request.operation});
      try {worker.postMessage({id,request},(request.buffers??[]).map(bytes=>bytes.buffer));}
      catch(error){pending.delete(id);reject(error);}
    });
  }
  async function claim(name,legacy) {
    if(failed)throw failed;
    if(legacy!=null&&!['drawing','window'].includes(legacy))throw Error('Invalid session ownership scope');
    const identity=legacy==null?name:`${legacy}:${name}`;
    const previous=claims.get(identity);
    if(previous) {
      if(!previous.retired)throw Error('Drawing storage is already open');
      await previous.retired;
    }
    return new Promise((resolve,reject)=>{
      let release;
      const held=new Promise(done=>{release=done;});
      const state={release};
      state.finished=navigator.locks.request(legacy==null?`capy-session-store:${name}`:`capy-session:${identity}`,{ifAvailable:true},async lock=>{
        if(!lock){resolve(null);return;}
        claims.set(identity,state);resolve(name);await held;
      });
      state.finished.catch(reject);
    });
  }
  async function release(name,legacy) {
    const identity=legacy==null?name:`${legacy}:${name}`;
    const held=claims.get(identity);if(!held)return;
    held.retired??=(async()=>{
      try {if(legacy==null)await send({operation:'close_tree',metadata:{name}});}
      catch(error){fail(error);throw error;}
      held.release();await held.finished;claims.delete(identity);
    })();
    await held.retired;
  }
  const transport=request=>request.operation==='claim'?claim(request.metadata.name,request.metadata.legacy):
    request.operation==='release'?release(request.metadata.name,request.metadata.legacy):send(request);
  transport.connect=onFailure=> {
    if(failed)throw failed;
    const id=++next,channel=new MessageChannel();connections.set(id,onFailure);
    try {worker.postMessage({connect:{id,port:channel.port1}},[channel.port1]);}
    catch(error){connections.delete(id);channel.port1.close();channel.port2.close();throw error;}
    return {port:channel.port2,close(){channel.port2.close();if(!connections.delete(id))return;if(!failed)worker.postMessage({disconnect:id});}};
  };
  return transport;
}
