export function createDocumentRecovery({app,call,message,restore,settled=()=>{},canOffer=()=>true,handles=()=>[],order=()=>{},failed=async()=>'later',reserve=()=>{}}) {
  const owners=new Map(),leases=new Map(),pending=new Map(),windowKey=crypto.randomUUID();
  let initialization,job,manifest=null,restoring=false,storageKept=false,keepingStorage;
  const json=value=>JSON.stringify(value,(_,item)=>{
    if(typeof item!=='bigint')return item;
    if(JSON.rawJSON)return JSON.rawJSON(String(item));
    const number=Number(item);if(!Number.isSafeInteger(number))throw new Error('Session identity exceeds browser JSON precision');return number;
  });
  const tabs=()=>app.document_tabs(0);
  const irreplaceable=tab=>tab.uri==null&&tab.modified;
  function keepStorage() {
    keepingStorage??=(async()=>{
      if(!navigator.storage?.persist)return;
      storageKept=await navigator.storage.persisted()||await navigator.storage.persist();
    })().catch(()=>{});
  }
  const storage=(operation,metadata='',buffers=[])=>call({operation:`restart-store-${operation}`,metadata:typeof metadata==='string'?metadata:json(metadata),buffers});
  const fail=error=>{const detail=String(error);message(()=>app.document_recovery_unavailable(detail));};
  const update=(state,event)=>app.session_manifest_update(state?json(state):'',event);
  const stamp=id=>json(app.session_stamp_for(BigInt(id)));
  const generation=value=>{const number=Number(value);if(!Number.isSafeInteger(number)||number<0)throw new Error('Session generation exceeds browser JSON precision');return number;};
  async function lease(key,available=false,kind='drawing') {
    const name=`${kind}:${key}`;
    if(leases.has(name))return true;
    return new Promise((resolve,reject)=>navigator.locks.request(`capy-session:${name}`,available?{ifAvailable:true}:{},async lock=>{
      if(!lock){resolve(false);return;}
      await new Promise(done=>{leases.set(name,done);resolve(true);});
    }).catch(reject));
  }
  function release(key,kind='drawing'){const name=`${kind}:${key}`;leases.get(name)?.();leases.delete(name);}
  async function ensure(id=tabs().selected) {
    const key=String(id);
    if(!owners.has(key)) {
      const owner={id:key,key:crypto.randomUUID(),generation:0,base:0,durable:null};
      owners.set(key,owner);await lease(owner.key);
    }
    return owners.get(key);
  }
  async function move(owner) {
    const origin=pending.get(owner.id);
    await captureOne(owner.id);
    if(origin.transfer) {
      await storage('transfer-manifest',origin.transfer);
      manifest=origin.transfer.destinationManifest;pending.delete(owner.id);
      if(!restoring)release(origin.key,'window');
      return origin.transfer.sourceManifest;
    }
    let saved=await storage('manifest',origin.key);
    const sourceGeneration=saved.generation,destinationGeneration=manifest?.generation??0;
    if(saved.restoring.some(attempt=>String(attempt.id)===String(origin.id)))saved=update(saved,{type:'finish_restore',attempt:origin.attempt,success:true});
    const sourceManifest=update(saved,{type:'remove',id:origin.id}),current=tabs();
    for(const tab of current.tabs)await ensure(tab.id);
    const drawings=current.tabs.map(tab=>({id:tab.id,key:owners.get(String(tab.id)).key}));
    const destinationManifest=update(manifest,{type:'reconcile',drawings,active:current.selected,clean_exit:false});
    origin.transfer={source:origin.key,sourceManifest,destination:windowKey,destinationManifest,key:owner.key,sourceGeneration,destinationGeneration};
    await storage('transfer-manifest',origin.transfer);
    manifest=destinationManifest;pending.delete(owner.id);
    if(!restoring)release(origin.key,'window');
    return sourceManifest;
  }
  async function publish(clean_exit=false,stage=false) {
    const current=tabs();
    for(const tab of current.tabs)await ensure(tab.id);
    const drawings=current.tabs.map(tab=>({id:tab.id,key:owners.get(String(tab.id)).key}));
    const next=update(manifest,stage?{type:'stage',drawings,active:current.selected}:{type:'reconcile',drawings,active:current.selected,clean_exit});
    if(manifest&&json({...next,generation:0})===json({...manifest,generation:0}))return;
    await storage('publish-manifest',{key:windowKey,manifest:next});manifest=next;
    await call({operation:'restart-retain',metadata:json(drawings.map(drawing=>drawing.key)),buffers:[]});
  }
  async function captureOne(id) {
    const owner=await ensure(id),observed=stamp(id);
    if(owner.durable===observed)return;
    if(!owner.generation&&!manifest?.drawings.some(drawing=>drawing.key===owner.key))await publish(false,true);
    const capture=app.capture_tab_session(BigInt(id));
    try {
      const stored=await storage('read',owner.key);
      const next=generation(Math.max(owner.generation,generation(stored?.current.generation??0))+1);
      const base=[stored?.current,stored?.previous].find(checkpoint=>checkpoint&&generation(checkpoint.generation)===owner.base);
      await capture.write(owner.key,BigInt(next),BigInt(owner.base),base?.resources??[],handles(id));
      owner.generation=next;owner.base=next;owner.durable=observed;
      if(tabs().tabs.some(tab=>String(tab.id)===String(id)&&irreplaceable(tab)))keepStorage();
    } finally {capture.free();}
  }
  function autosave(clean_exit=false) {
    if(restoring)return Promise.resolve();
    if(job)return job;
    job=(async()=>{
      await lease(windowKey,false,'window');
      for(const id of [...pending.keys()])await move(owners.get(id));
      for(const tab of tabs().tabs)await captureOne(tab.id);
      await publish(clean_exit);
      await settled();
    })().finally(()=>job=null);
    return job;
  }
  async function capture(){await autosave();}
  async function retire(id=tabs().selected,closed=false) {
    if(!closed)return capture();
    if(job)await job;
    if(pending.has(String(id)))await move(owners.get(String(id)));
    const owner=await ensure(id);
    if(manifest?.drawings.some(drawing=>String(drawing.id)===String(id))) {
      const next=update(manifest,{type:'remove',id});
      await storage('publish-manifest',{key:windowKey,manifest:next,closed:[owner.key]});manifest=next;
    } else await storage('remove',owner.key);
    owners.delete(String(id));release(owner.key);
  }
  async function initialize() {
    restoring=true;
    let initial;const restoredOrder=[];
    try {
      await lease(windowKey,false,'window');
      while(!canOffer())await new Promise(resolve=>setTimeout(resolve,50));
      initial=app.session_stamp_for(tabs().selected);
      for(const origin of await storage('list')) {
        if(origin===windowKey||!await lease(origin,true,'window'))continue;
        let saved;
        try {
          saved=await storage('manifest',origin);
          if(!saved)continue;
          const interrupted=update(saved,{type:'interrupted'});
          await storage('publish-manifest',{key:origin,manifest:interrupted});saved=interrupted;
          reserve(saved.drawings.map(drawing=>drawing.id));
          const entries=[...saved.drawings].sort((a,b)=>Number(String(b.id)===String(saved.active))-Number(String(a.id)===String(saved.active)));
          const sourceOrder=[...saved.drawings],restored=new Map();
          for(const drawing of entries) {
            if(!await lease(drawing.key,true))continue;
            let detail;
            for(;;) {
            while(!canOffer())await new Promise(resolve=>setTimeout(resolve,50));
            if(saved.blocked.some(id=>String(id)===String(drawing.id))) {
              const decision=await failed(detail);
              if(decision==='later')break;
              if(decision==='discard') {
                const removed=update(saved,{type:'remove',id:drawing.id});
                await storage('publish-manifest',{key:origin,manifest:removed,closed:[drawing.key]});saved=removed;break;
              }
              const retried=update(saved,{type:'retry_restore',id:drawing.id});
              await storage('publish-manifest',{key:origin,manifest:retried});saved=retried;
              continue;
            }
            let success=false,attempt;
            try {
              if(!saved.restoring.some(attempt=>String(attempt.id)===String(drawing.id))) {
                const begun=update(saved,{type:'begin_restore',id:drawing.id});
                await storage('publish-manifest',{key:origin,manifest:begun});saved=begun;
              }
              attempt=saved.restoring.find(ticket=>String(ticket.id)===String(drawing.id));
              const snapshots=await storage('read',drawing.key);
              let failure,restoredId,base;
              for(const checkpoint of [snapshots?.current,snapshots?.previous].filter(Boolean)) {
                try {
                  const {buffers}=await storage('resources',{key:drawing.key,ids:checkpoint.resources});
                  const id=await restore(checkpoint,buffers,!saved.clean_exit||checkpoint===snapshots.previous,initial,snapshots.handles??[],String(drawing.id)===String(saved.active),drawing.id);
                  restoredId=id;base=generation(checkpoint.generation);break;
                } catch(error) {failure=error;}
              }
              if(restoredId==null)throw failure??new Error('The saved drawing is missing');
              const owner={id:String(restoredId),key:drawing.key,generation:generation(snapshots.current.generation),base,durable:null};
              const previous=owners.get(owner.id);if(previous&&previous.key!==owner.key)release(previous.key);
              owners.set(owner.id,owner);pending.set(owner.id,{key:origin,id:drawing.id,attempt});
              saved=await move(owner);restored.set(String(drawing.id),restoredId);success=true;
            } catch(error) {if(pending.size)throw error;detail=String(error);fail(error);}
            finally {
              if(!pending.size&&saved.restoring.some(attempt=>String(attempt.id)===String(drawing.id))){const completed=update(saved,{type:'finish_restore',attempt,success});await storage('publish-manifest',{key:origin,manifest:completed});saved=completed;}
            }
            if(success)break;
            }
            if(!owners.has(String(restored.get(String(drawing.id)))))release(drawing.key);
          }
          restoredOrder.push(...sourceOrder.flatMap(drawing=>restored.has(String(drawing.id))?[restored.get(String(drawing.id))]:[]));
          release(origin,'window');
        } catch(error){fail(error);if(pending.size)throw error;release(origin,'window');}
      }
      const restoredIds=new Set(restoredOrder.map(String));
      order([...tabs().tabs.filter(tab=>!restoredIds.has(String(tab.id))).map(tab=>tab.id),...restoredOrder]);
      for(const tab of tabs().tabs)await captureOne(tab.id);
      await publish();

    } finally {restoring=false;await settled();}
  }
  const protectedSession=()=>{
    if(restoring||job)return false;
    try {
      const current=tabs();
      return manifest&&json(current.tabs.map(tab=>String(tab.id)))===json(manifest.drawings.map(drawing=>String(drawing.id)))
        &&String(current.selected)===String(manifest.active)&&current.tabs.every(tab=>owners.get(String(tab.id))?.durable===stamp(tab.id)&&(storageKept||!irreplaceable(tab)));
    } catch{return false;}
  };
  setInterval(()=>{if(!restoring&&canOffer())autosave().catch(fail);},app.session_checkpoint_interval());
  document.addEventListener('visibilitychange',()=>{if(document.hidden&&!restoring)autosave().catch(fail);});
  window.addEventListener('pagehide',()=>{if(!restoring)autosave(true).catch(fail);});
  window.addEventListener('beforeunload',event=>{if(!protectedSession()){event.preventDefault();event.returnValue='';}});
  return {ensure,capture,retire,autosave,start:()=>initialization??=initialize(),protected:protectedSession};
}
