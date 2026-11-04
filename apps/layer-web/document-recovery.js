export function createDocumentRecovery({app,message,restore,remember=async()=>{},settled=()=>{},canOffer=()=>true,handles=()=>[],order=()=>{},failed=async()=>'later',reserve=()=>{}}) {
  let initialization,job,pending,windowStore,manifest=null,restoring=false,storageKept=false,keepingStorage;
  const tabs=()=>app.document_tabs(0),same=(a,b)=>String(a)===String(b);
  const irreplaceable=tab=>tab.uri==null&&tab.modified;
  const fail=error=>message(()=>app.document_recovery_unavailable(String(error)));
  const json=value=>JSON.stringify(value,(_,item)=>{
    if(typeof item!=='bigint')return item;
    const number=Number(item);if(!Number.isSafeInteger(number))throw Error('Session identity exceeds browser JSON precision');return number;
  });
  const update=(state,event)=>app.session_manifest_update(state?json(state):'',event);
  const ensure=id=>({id,key:app.session_binding(BigInt(id??tabs().selected),null)});
  const drawings=()=>tabs().tabs.map(tab=>({id:tab.id,key:ensure(tab.id).key}));
  function keepStorage() {
    return keepingStorage??=(async()=>{
      if(!navigator.storage?.persist)return;
      storageKept=await navigator.storage.persisted()||await navigator.storage.persist();
    })().catch(()=>{});
  }
  async function current() {
    windowStore??=await app.open_session_window(null);
    if(!windowStore)throw Error('Session storage is already open');
    return windowStore;
  }
  async function publish(clean_exit=false,stage=false) {
    const store=await current(),currentTabs=tabs();
    const next=update(manifest,stage?{type:'stage',drawings:drawings(),active:currentTabs.selected}:
      {type:'reconcile',drawings:drawings(),active:currentTabs.selected,clean_exit});
    if(manifest&&json({...next,generation:0})===json({...manifest,generation:0}))return;
    await store.publish(next);manifest=next;
  }
  async function captureOne(id,force=false) {
    const owner=ensure(id);
    if(!app.session_current(BigInt(id))) {
      if(!manifest?.drawings.some(drawing=>drawing.key===owner.key))await publish(false,true);
      await remember(handles(id));
      await app.checkpoint_session(BigInt(id),force);
    }
    if(tabs().tabs.some(tab=>same(tab.id,id)&&irreplaceable(tab)))await keepStorage();
  }
  function autosave(clean_exit=false,force=clean_exit) {
    if(restoring)return Promise.resolve();
    if(job){
      if(force||clean_exit){pending??={force:false,clean_exit:false};pending.force||=force;pending.clean_exit||=clean_exit;}
      return job;
    }
    job=(async()=>{
      try{
        await current();let request={force,clean_exit},failure;
        do{
          for(const tab of tabs().tabs)try{await captureOne(tab.id,request.force);}catch(error){failure??=error;}
          await publish(request.clean_exit);await settled();app.session_maintenance();request=pending;pending=null;
        }while(request);
        if(failure)throw failure;
      }finally{job=null;pending=null;}
    })();
    return job;
  }
  async function retire(id=tabs().selected,closed=false) {
    if(!closed)return autosave();
    if(job)await job;
    const owner=ensure(id);await app.prepare_session_retirement(owner.key);
    if(manifest?.drawings.some(drawing=>same(drawing.id,id))) {
      const next=update(manifest,{type:'remove',id});await (await current()).publish(next);manifest=next;
    }
    await app.retire_session_copy(owner.key);
  }
  async function initialize() {
    restoring=true;const restoredOrder=[];
    try {
      const own=await current();
      while(!canOffer())await new Promise(resolve=>setTimeout(resolve,50));
      const initial=app.session_stamp_for(tabs().selected);
      try{for(const warning of await app.import_legacy_sessions())fail(warning);}catch(error){fail(error);}
      for(const originKey of await app.session_windows()) {
        if(originKey===own.key)continue;
        const origin=await app.open_session_window(originKey);if(!origin)continue;
        try {
          let saved=await origin.read();if(!saved)continue;
          saved=update(saved,{type:'interrupted'});await origin.publish(saved);
          reserve(saved.drawings.map(drawing=>drawing.id));
          const sourceOrder=[...saved.drawings],restored=new Map();
          const entries=[...sourceOrder].sort((a,b)=>Number(same(b.id,saved.active))-Number(same(a.id,saved.active)));
          for(const drawing of entries) {
            let detail;
            for(;;) {
              while(!canOffer())await new Promise(resolve=>setTimeout(resolve,50));
              const existing=app.session_existing_binding(drawing.key);
              if(existing!=null) {
                await publish();const removed=update(saved,{type:'remove',id:drawing.id});await origin.publish(removed);saved=removed;
                restored.set(String(drawing.id),existing);break;
              }
              if(saved.blocked.some(id=>same(id,drawing.id))) {
                const decision=await failed(detail);
                if(decision==='discard') {
                  await app.prepare_session_retirement(drawing.key);
                  const removed=update(saved,{type:'remove',id:drawing.id});await origin.publish(removed);saved=removed;
                  await app.retire_session_copy(drawing.key);break;
                }
                if(decision!=='retry')break;
                const retried=update(saved,{type:'retry_restore',id:drawing.id});await origin.publish(retried);saved=retried;continue;
              }
              let success=false,attempt,adopted=false;
              try {
                if(!saved.restoring.some(ticket=>same(ticket.id,drawing.id))) {
                  const begun=update(saved,{type:'begin_restore',id:drawing.id});await origin.publish(begun);saved=begun;
                }
                attempt=saved.restoring.find(ticket=>same(ticket.id,drawing.id));
                const id=await restore(drawing.key,!saved.clean_exit,initial,same(drawing.id,saved.active),drawing.id);
                if(id==null) {
                  const finished=update(saved,{type:'finish_restore',attempt,success:true});await origin.publish(finished);saved=finished;break;
                }
                adopted=true;saved=await origin.read();await publish();
                const completed=update(saved,{type:'finish_restore',attempt,success:true});
                const removed=update(completed,{type:'remove',id:drawing.id});await origin.publish(removed);saved=removed;
                restored.set(String(drawing.id),id);success=true;
              }catch(error) {
                detail=String(error);fail(error);if(adopted)throw error;
                if(attempt&&saved.restoring.some(ticket=>same(ticket.id,drawing.id))) {
                  const completed=update(saved,{type:'finish_restore',attempt,success:false});await origin.publish(completed);saved=completed;
                }else throw error;
              }
              if(success)break;
            }
          }
          restoredOrder.push(...sourceOrder.flatMap(drawing=>restored.has(String(drawing.id))?[restored.get(String(drawing.id))]:[]));
        }catch(error){fail(error);}finally{origin.free();}
      }
      const ids=new Set(restoredOrder.map(String));
      order([...tabs().tabs.filter(tab=>!ids.has(String(tab.id))).map(tab=>tab.id),...restoredOrder]);
      for(const tab of tabs().tabs)await captureOne(tab.id);
      await publish();
    }finally{restoring=false;await settled();}
  }
  const protectedSession=()=>{
    if(restoring||job)return false;
    try {
      const currentTabs=tabs();
      return manifest&&json(currentTabs.tabs.map(tab=>String(tab.id)))===json(manifest.drawings.map(drawing=>String(drawing.id)))
        &&same(currentTabs.selected,manifest.active)&&currentTabs.tabs.every(tab=>app.session_current(BigInt(tab.id))&&(storageKept||!irreplaceable(tab)));
    }catch{return false;}
  };
  setInterval(()=>{if(!restoring&&canOffer())autosave().catch(fail);},app.session_checkpoint_interval());
  document.addEventListener('visibilitychange',()=>{app.session_background(document.hidden);if(document.hidden&&!restoring)autosave(false,true).catch(fail);});
  window.addEventListener('pagehide',()=>{if(!restoring)autosave(true,true).catch(fail);});
  window.addEventListener('beforeunload',event=>{if(!protectedSession()){event.preventDefault();event.returnValue='';}});
  return {ensure,capture:()=>autosave(),retire,autosave,start:()=>initialization??=initialize(),protected:protectedSession};
}
