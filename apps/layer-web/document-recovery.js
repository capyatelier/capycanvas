// A lease and Rust recovery policy per drawing; one timer for the window.
// Each accepted work ticket captures its immutable project before any await.
export function createDocumentRecovery({app,call,dialog,element,button,message,restore,settled=()=>{},canOffer=()=>true}) {
  const owners=new Map(),heldOrigins=new Map();let initialization,jobs=Promise.resolve();
  const lockName=key=>`capy-raster:${key}`,keyId=id=>String(id);
  const current=()=>app.document_tabs(0).selected;
  const transport=(op,key='')=>call({operation:`recover-${op}`,metadata:key,buffers:[]});
  function releaseOrigin(key){heldOrigins.get(key)?.();heldOrigins.delete(key);}
  function event(owner,event){const result=app.recovery_update(owner.policy,event);owner.policy=result.state;for(const key of result.update.release)releaseOrigin(key);return result.update;}
  function execute(owner,first){
    if(!first)return owner.jobs;
    // Freeze at scheduling, not when older storage work eventually completes.
    const freeze=work=>work?.kind.type==='capture'?app.capture_tab_recovery(BigInt(owner.id)):null;
    let frozen;
    try{frozen=freeze(first);}catch(error){event(owner,{type:'complete',token:first.token,success:false});message(`Recovery capture failed: ${error}`);return owner.jobs;}
    const run=async()=>{
      let work=first,capture=frozen;
      while(work){let success=false;
        try{
          if(work.kind.type==='capture'){await capture.write(owner.key);success=true;}
          else if(work.kind.type==='retire'){await transport('delete',owner.key);success=true;}
          else if(work.kind.type==='retire_origin'){
            const key=work.kind.key;
            const remove=async()=>{await transport('delete',key);success=true;};
            if(heldOrigins.has(key))await remove();
            else await navigator.locks.request(lockName(key),{ifAvailable:true},async lock=>{if(lock)await remove();});
          }
        }catch(error){message(`Recovery operation failed: ${error}`);}
        finally{capture?.free();capture=null;}
        work=event(owner,{type:'complete',token:work.token,success}).work;
        if(work){try{capture=freeze(work);}catch(error){event(owner,{type:'complete',token:work.token,success:false});message(`Recovery capture failed: ${error}`);break;}}
      }
    };
    owner.jobs=jobs=jobs.then(run,run);return owner.jobs;
  }
  function ensure(id=current()){
    const key=keyId(id);if(owners.has(key))return owners.get(key).ready;
    const owner={id:key,key:crypto.randomUUID(),policy:'',jobs:Promise.resolve(),release:null,ready:null};owners.set(key,owner);
    owner.ready=new Promise((resolve,reject)=>{
      navigator.locks.request(lockName(owner.key),async()=>{resolve(owner);await new Promise(done=>owner.release=done);}).catch(reject);
    });
    return owner.ready;
  }
  async function capture(id=current()){
    const owner=await ensure(id);
    const update=event(owner,{type:'observe',document:app.recovery_document_for(BigInt(owner.id)),owned:true});
    await execute(owner,update.work);
  }
  async function retire(id=current(),closed=false){
    const owner=await ensure(id);
    if(closed)event(owner,{type:'close'});
    await execute(owner,event(owner,{type:'retire',discard_origin:true}).work);
    await owner.jobs;
    if(closed){owner.release?.();owners.delete(keyId(id));}
  }
  async function autosave(){try{for(const tab of app.document_tabs(0).tabs)await capture(tab.id);}finally{await settled();}}
  async function initialize(){
    await ensure();
    for(const key of await transport('list')){
      if([...owners.values()].some(o=>o.key===key))continue;
      while(!canOffer())await new Promise(resolve=>setTimeout(resolve,50));
      const release=await new Promise((resolve,reject)=>navigator.locks.request(lockName(key),{ifAvailable:true},async lock=>{
        if(!lock){resolve(null);return;}
        await new Promise(done=>resolve(done));
      }).catch(reject));
      if(!release)continue;
      heldOrigins.set(key,release);let adopted=false;
        try{
          const decision=await dialog('Recover drawing?',(form,finish)=>{
            form.append(element('p','','An unsaved drawing from a closed window is available.'));
            const footer=element('footer');footer.append(button('Keep for Later',()=>finish(null)),button('Discard',()=>finish('discard')),button('Recover',()=>finish('recover'),'suggested-action'));form.append(footer);
          });
          if(decision==='discard')await transport('delete',key);
          else if(decision==='recover'){
            const bytes=await transport('get',key);
            await restore(bytes);
            const owner=await ensure();
            // Shared policy retains the origin until this drawing's new owned
            // recovery checkpoint has become durable.
            adopted=true;await execute(owner,event(owner,{type:'adopted',key}).work);
            await capture(owner.id);
          }
        }catch(error){message(`Recovery operation failed: ${error}`);}
        finally{if(!adopted)releaseOrigin(key);}
    }
  }
  // Discovery is a startup task; the interval only checkpoints open drawings.
  setInterval(()=>autosave().catch(e=>message(`Recovery unavailable: ${e}`)),15000);
  function start(){return initialization??=initialize();}
  document.addEventListener('visibilitychange',()=>{if(document.hidden)autosave().catch(e=>message(`Recovery unavailable: ${e}`));});
  window.addEventListener('beforeunload',e=>{if(app.document_tabs(0).tabs.some(t=>t.modified)){e.preventDefault();e.returnValue='';}});
  return{ensure,capture,retire,autosave,start};
}
