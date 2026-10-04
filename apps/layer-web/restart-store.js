async function metadataDigest(metadata) {
  if(typeof metadata!=="string")throw new Error("Missing drawing checkpoint metadata");
  const digest=await crypto.subtle.digest("SHA-256",new TextEncoder().encode(metadata));
  return Array.from(new Uint8Array(digest),byte=>byte.toString(16).padStart(2,"0")).join("");
}
export async function verifyRestartCheckpoint(checkpoint) {
  if(!/^[0-9a-f]{64}$/.test(checkpoint.metadata_sha256??"")||await metadataDigest(checkpoint.metadata)!==checkpoint.metadata_sha256)
    throw new Error("Drawing checkpoint metadata is damaged");
}
export function createRestartStore({indexedDB=globalThis.indexedDB,name="capy-session-restart"}={}) {
  let opening;
  const open=()=>opening??=new Promise((resolve,reject)=>{
    const request=indexedDB.open(name,1);
    request.onupgradeneeded=()=>{
      request.result.createObjectStore("sessions");
      request.result.createObjectStore("resources");
      request.result.createObjectStore("windows");
    };
    request.onerror=()=>reject(request.error);
    request.onblocked=()=>reject(new Error("Session storage is unavailable"));
    request.onsuccess=()=>{
      const database=request.result;
      database.onversionchange=()=>{database.close();opening=null;};
      database.onclose=()=>{opening=null;};
      resolve(database);
    };
  }).catch(error=>{opening=null;throw error;});
  async function transaction(mode,run) {
    const database=await open();
    return new Promise((resolve,reject)=>{
      let result,failure;
      const tx=database.transaction(["sessions","resources","windows"],mode,{durability:"strict"});
      tx.oncomplete=()=>resolve(result);
      tx.onabort=()=>reject(failure??tx.error??new Error("Session storage was interrupted"));
      tx.onerror=()=>{};
      const guard=callback=>event=>{try{callback(event.target.result);}catch(error){failure=error;tx.abort();}};
      try{run(tx.objectStore("sessions"),tx.objectStore("resources"),guard,value=>result=value,tx.objectStore("windows"));}
      catch(error){failure=error;tx.abort();}
    });
  }
  const read=key=>transaction("readonly",(sessions,resources,guard,done)=>{
    sessions.get(key).onsuccess=guard(record=>{
      done(record);
    });
  });
  return {
    list:()=>transaction("readonly",(sessions,resources,guard,done)=>{sessions.getAllKeys().onsuccess=guard(done);}),
    read:key=>read(key),
    resources:(key,ids)=>transaction("readonly",(sessions,resources,guard,done)=>{
      const result=new Array(ids.length);let pending=ids.length;
      if(!pending){done(result);return;}
      ids.forEach((id,index)=>{resources.get([key,id]).onsuccess=guard(value=>{
        if(value===undefined)throw new Error("A session resource is missing");
        result[index]=value;if(!--pending)done(result);
      });});
    }),
    publish:async(key,snapshot,payloads,handles=[],baseGeneration=0)=>{
      const checkpoint={...snapshot,resources:[...snapshot.resources]};
      checkpoint.metadata_sha256=await metadataDigest(checkpoint.metadata);
      return transaction("readwrite",(sessions,resources,guard,done)=>{
      sessions.get(key).onsuccess=guard(record=>{
        if(record&&BigInt(checkpoint.generation)<=BigInt(record.current.generation))throw new Error("A newer drawing checkpoint is already stored");
        const previous=BigInt(baseGeneration)===0n?null:[record?.current,record?.previous].find(base=>base&&BigInt(base.generation)===BigInt(baseGeneration));
        if(BigInt(baseGeneration)!==0n&&!previous)throw new Error("The verified drawing checkpoint is missing");
        for(const {id,bytes} of payloads)resources.get([key,id]).onsuccess=guard(existing=>{
          if(existing===undefined){resources.put(bytes,[key,id]);return;}
          if(!(existing instanceof Uint8Array)||!(bytes instanceof Uint8Array)||existing.length!==bytes.length||existing.some((value,index)=>value!==bytes[index]))
            throw new Error("An immutable session resource changed");
        });
        sessions.put({current:checkpoint,previous,handles},key);
        const retained=new Set([...checkpoint.resources,...(previous?.resources??[])]);
        const cursor=resources.openKeyCursor(IDBKeyRange.bound([key],[key,[]]));
        cursor.onsuccess=guard(entry=>{
          if(!entry){done(true);return;}
          if(!retained.has(entry.key[1]))resources.delete(entry.key);
          entry.continue();
        });
      });
      });
    },
    remove:key=>transaction("readwrite",(sessions,resources,guard,done)=>{
      sessions.delete(key);
      resources.delete(IDBKeyRange.bound([key],[key,[]]));done(true);
    }),
    windows:()=>transaction("readonly",(sessions,resources,guard,done,windows)=>{windows.getAllKeys().onsuccess=guard(done);}),
    manifest:key=>transaction("readonly",(sessions,resources,guard,done,windows)=>{windows.get(key).onsuccess=guard(done);}),
    publishManifest:(key,manifest,closed=[])=>transaction("readwrite",(sessions,resources,guard,done,windows)=>{
      windows.get(key).onsuccess=guard(previous=>{
        if(previous&&JSON.stringify(previous)===JSON.stringify(manifest)){done(true);return;}
        if(previous&&BigInt(manifest.generation)<=BigInt(previous.generation))throw new Error("A newer session checkpoint is already stored");
        windows.put(manifest,key);
        for(const drawing of closed){sessions.delete(drawing);resources.delete(IDBKeyRange.bound([drawing],[drawing,[]]));}
        done(true);
      });
    }),
    transferManifest:(source,sourceManifest,destination,destinationManifest,key,sourceGeneration,destinationGeneration)=>transaction("readwrite",(sessions,resources,guard,done,windows)=>{
      windows.get(source).onsuccess=guard(priorSource=>{
        windows.get(destination).onsuccess=guard(priorDestination=>{
          if(JSON.stringify(priorDestination)===JSON.stringify(destinationManifest)
            &&(sourceManifest.drawings.length?JSON.stringify(priorSource)===JSON.stringify(sourceManifest):!priorSource)){done(true);return;}
          if(!priorSource||BigInt(priorSource.generation)!==BigInt(sourceGeneration)||BigInt(priorDestination?.generation??0)!==BigInt(destinationGeneration)
            ||BigInt(sourceManifest.generation)<=BigInt(priorSource.generation)||BigInt(destinationManifest.generation)<=BigInt(destinationGeneration))
            throw new Error("A newer session checkpoint is already stored");
          if(!priorSource.drawings.some(drawing=>drawing.key===key)||sourceManifest.drawings.some(drawing=>drawing.key===key)||!destinationManifest.drawings.some(drawing=>drawing.key===key))
            throw new Error("The restored drawing ownership changed");
          if(sourceManifest.drawings.length)windows.put(sourceManifest,source);else windows.delete(source);
          windows.put(destinationManifest,destination);done(true);
        });
      });
    }),
    removeManifest:key=>transaction("readwrite",(sessions,resources,guard,done,windows)=>{windows.delete(key);done(true);}),
  };
}
