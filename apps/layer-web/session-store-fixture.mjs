export function installSessionStoreFixture(evaluate) {
  return evaluate(sessionStoreFixtureSource);
}
export const sessionStoreFixtureSource=`(${install.toString()})()`;

function install() {
  const decoder=new TextDecoder();
  const table=Uint32Array.from({length:256},(_,index)=>{
    let crc=index;for(let bit=0;bit<8;bit++)crc=(crc>>>1)^((crc&1)?0xedb88320:0);return crc>>>0;
  });
  function head(bytes,slot) {
    if(bytes.length!==2048||decoder.decode(bytes.subarray(0,8))!=='CAPYSH01')return null;
    const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),sequence=Number(view.getBigUint64(8,true)),length=view.getUint32(16,true);
    if(!Number.isSafeInteger(sequence)||sequence===0||sequence%2!==slot||length>2024)return null;
    let crc=0xffffffff;
    for(const [begin,end] of [[8,20],[24,bytes.length]])for(let at=begin;at<end;at++)crc=(crc>>>8)^table[(crc^bytes[at])&255];
    if(((crc^0xffffffff)>>>0)!==view.getUint32(20,true))return null;
    return {sequence,...JSON.parse(decoder.decode(bytes.subarray(24,24+length)))};
  }
  async function directory(path,create=false) {
    let root=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-session-store',{create});
    for(const part of path.split('/').filter(Boolean))root=await root.getDirectoryHandle(part,{create});return root;
  }
  async function bytes(root,path) {
    const names=path.split('/'),leaf=names.pop();for(const name of names)root=await root.getDirectoryHandle(name);
    return new Uint8Array(await(await(await root.getFileHandle(leaf)).getFile()).arrayBuffer());
  }
  async function files(key,resourcesOnly=false) {
    let root;try{root=await directory(`drawings/${key}`);}catch(error){if(error.name==='NotFoundError')return {};throw error;}
    const result={};
    async function visit(dir,prefix='') {
      const entries=[];for await(const entry of dir.entries())entries.push(entry);entries.sort(([a],[b])=>a<b?-1:a>b?1:0);
      for(const [name,entry] of entries) {
        const path=prefix+name;
        if(entry.kind==='directory')await visit(entry,path+'/');
        else if(!resourcesOnly||path.startsWith('packs/')||path.startsWith('resources/'))result[path]=Array.from(await bytes(root,path));
      }
    }
    await visit(root);return result;
  }
  async function index(path) {
    let root;try{root=await directory(path);}catch(error){if(error.name==='NotFoundError')return null;throw error;}
    const heads=[];
    for(const slot of [0,1])try{const value=head(await bytes(root,`head.${slot}`),slot);if(value)heads.push(value);}catch(error){if(error.name!=='NotFoundError')throw error;}
    heads.sort((a,b)=>b.sequence-a.sequence);if(!heads.length)return null;
    async function generation(reference) {
      if(!reference)return null;
      const data=await bytes(root,`generations/${reference.id}.json`),digest=new Uint8Array(await crypto.subtle.digest('SHA-256',data));
      const sha256=Array.from(digest,byte=>byte.toString(16).padStart(2,'0')).join('');
      if(sha256!==reference.sha256)throw Error('Checkpoint metadata checksum failed');
      const packed=JSON.parse(decoder.decode(data));return {...reference,metadata:packed.session??packed};
    }
    return {sequence:heads[0].sequence,current:await generation(heads[0].current),previous:await generation(heads[0].previous)};
  }
  const snapshot=key=>index(`drawings/${key}`);
  async function drawing(id) {
    const key=layerApp.app.session_binding(BigInt(id),null);return {key,record:await snapshot(key)};
  }
  async function windows() {
    const result=[];
    for(const key of await layerApp.app.session_windows()) {
      const store=await layerApp.app.open_session_window(key);if(!store)throw Error('Test window storage is busy');
      try{result.push({key,manifest:JSON.parse(JSON.stringify(await store.read(),(_,value)=>typeof value==='bigint'?Number(value):value))});}finally{store.free();}
    }
    return result;
  }
  async function copy(source,target) {
    if(source===target)throw Error('Test drawing copies need distinct owners');
    const input=await files(source),destination=await directory(`drawings/${target}`,true);
    for await(const _entry of destination.entries())throw Error('Test drawing destination already exists');
    for(const [path,data] of Object.entries(input)) {
      const names=path.split('/'),leaf=names.pop();let root=destination;
      for(const name of names)root=await root.getDirectoryHandle(name,{create:true});
      const stream=await(await root.getFileHandle(leaf,{create:true})).createWritable();
      try{await stream.write(new Uint8Array(data));await stream.close();}catch(error){await stream.abort();throw error;}
    }
  }
  async function checkpoint() {
    await layerApp.documents.autosave();
    for(const tab of layerApp.app.document_tabs(0).tabs)if(!layerApp.app.session_current(tab.id))await layerApp.app.checkpoint_session(tab.id,true);
    await layerApp.documents.autosave();
  }
  window.sessionStoreFixture={snapshot,index,drawing,windows,files,copy,checkpoint};
}
