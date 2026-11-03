const BLOCK=4*1024*1024,MAX_RANGE_BYTES=8*1024*1024;
export function createLegacyRestartReader({indexedDB=globalThis.indexedDB,name='capy-session-restart'}={}) {
  let opening;
  function open() {
    return opening??=new Promise((resolve,reject)=>{
      let absent=false;
      const request=indexedDB.open(name);
      request.onupgradeneeded=()=>{absent=true;request.transaction.abort();};
      request.onerror=()=>absent?resolve(null):reject(request.error);
      request.onblocked=()=>reject(Error('Session storage is unavailable'));
      request.onsuccess=()=>{
        const database=request.result;
        if(database.version!==1||!['sessions','resources','windows'].every(store=>database.objectStoreNames.contains(store))) {
          database.close();reject(Error('Unsupported saved session storage'));return;
        }
        database.onversionchange=()=>{database.close();opening=null;};database.onclose=()=>{opening=null;};resolve(database);
      };
    }).then(database=>{if(database===null)opening=null;return database;}).catch(error=>{opening=null;throw error;});
  }
  async function read(store,key,all=false) {
    const database=await open();if(!database)return all?[]:undefined;
    return new Promise((resolve,reject)=>{
      const tx=database.transaction(store,'readonly'),object=tx.objectStore(store);
      const request=all?object.getAllKeys():object.get(key);let result;
      request.onsuccess=()=>{result=request.result;};tx.oncomplete=()=>resolve(result);
      tx.onerror=tx.onabort=()=>reject(tx.error||Error('Saved session read failed'));
    });
  }
  return {
    windows:()=>read('windows',null,true),
    manifest:key=>read('windows',key),
    snapshot:key=>read('sessions',key),
    async range(key,resource,offset,length) {
      if(typeof key!=='string'||typeof resource!=='string'||!Number.isSafeInteger(offset)||offset<0||!Number.isSafeInteger(length)||length<0||length>MAX_RANGE_BYTES||!Number.isSafeInteger(offset+length))throw Error('Invalid saved session range');
      const result=new Uint8Array(length);
      for(let cursor=0;cursor<length;) {
        const position=offset+cursor,part=Math.floor(position/BLOCK),inside=position%BLOCK;
        const bytes=await read('resources',[key,`${resource}-${part}`]);
        if(!(bytes instanceof Uint8Array)||bytes.length>BLOCK||inside>=bytes.length)throw Error('A saved session resource is missing');
        const count=Math.min(length-cursor,bytes.length-inside);result.set(bytes.subarray(inside,inside+count),cursor);cursor+=count;
      }
      return result;
    },
  };
}
