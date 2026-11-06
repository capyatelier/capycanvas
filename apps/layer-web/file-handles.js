export function createFileHandleStore({indexedDB=globalThis.indexedDB,name='capy-file-handles'}={}) {
  let opening;
  const open=()=>opening??=new Promise((resolve,reject)=>{
    const request=indexedDB.open(name,1);
    request.onupgradeneeded=()=>request.result.createObjectStore('handles');
    request.onerror=()=>reject(request.error);
    request.onblocked=()=>reject(Error('File destination storage is unavailable'));
    request.onsuccess=()=>{
      const database=request.result;database.onversionchange=()=>{database.close();opening=null;};resolve(database);
    };
  }).catch(error=>{opening=null;throw error;});
  async function transaction(mode,work) {
    const database=await open();
    return new Promise((resolve,reject)=>{
      const tx=database.transaction('handles',mode);let result;
      tx.oncomplete=()=>resolve(result);tx.onerror=tx.onabort=()=>reject(tx.error||Error('Cannot store file destination'));
      try {work(tx.objectStore('handles'),value=>{result=value;});}catch(error){tx.abort();reject(error);}
    });
  }
  return {
    async remember(handles) {
      if(!handles.length)return;
      await transaction('readwrite',store=>{
        for(const {uri,handle} of handles) {
          if(typeof uri!=='string'||!uri.startsWith('browser:')||handle?.kind!=='file')throw Error('Invalid file destination');
          store.put(handle,uri);
        }
      });
    },
    read:uri=>transaction('readonly',(store,done)=>{store.get(uri).onsuccess=event=>done(event.target.result);}),
  };
}

export async function writeDocumentFile(handle,bytes,sources,copy) {
  const stream=await handle.createWritable();
  try {
    await stream.write(bytes);
    for(const source of sources)if(await handle.isSameEntry(source.handle))await copy(source.id);
    await stream.close();
  }
  catch(error){try{await stream.abort();}catch{}throw error;}
}
