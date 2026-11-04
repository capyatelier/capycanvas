import {createLegacyRestartReader} from './legacy-restart-store.js';
import {createFileHandleStore} from './file-handles.js';

const MAX_RANGE_BYTES=8*1024*1024;
const reads=[],writes=[],files=new Map();
const legacy=createLegacyRestartReader();
const destinations=createFileHandleStore();
let root,scheduled=false,active=false;
const channel=new MessageChannel();
channel.port1.onmessage=()=>drain();
const crcTable=Uint32Array.from({length:256},(_,index)=>{
  let crc=index;for(let bit=0;bit<8;bit++)crc=(crc>>>1)^((crc&1)?0xedb88320:0);return crc>>>0;
});
function verify(bytes,expected) {
  let crc=0xffffffff;
  for(const byte of bytes)crc=(crc>>>8)^crcTable[(crc^byte)&255];
  if(((crc^0xffffffff)>>>0)!==expected)throw Error('Recovery resource checksum failed');
}

function parts(name,empty=false) {
  if(empty&&name==='')return [];
  if(typeof name!=='string'||!name||name.length>1024||/[\\\0:]/.test(name))throw Error('Invalid drawing store member');
  const result=name.split('/');
  if(result.some(part=>!part||part==='.'||part==='..'))throw Error('Invalid drawing store member');
  return result;
}
async function directory(name,create=false) {
  let current=await (root??=navigator.storage.getDirectory().then(root=>root.getDirectoryHandle('capy-session-store',{create:true})));
  for(const part of parts(name,true))current=await current.getDirectoryHandle(part,{create});
  return current;
}
async function parent(name,create=false) {
  const names=parts(name),leaf=names.pop();
  return [await directory(names.join('/'),create),leaf];
}
async function file(name) {
  parts(name);let handle=files.get(name);
  if(!handle) {const [dir,leaf]=await parent(name);handle=await (await dir.getFileHandle(leaf)).createSyncAccessHandle();files.set(name,handle);}
  return handle;
}
function close(name) {const handle=files.get(name);if(handle){files.delete(name);handle.close();}}
function range(offset,length) {
  if(!Number.isSafeInteger(offset)||offset<0||!Number.isSafeInteger(length)||length<0||length>MAX_RANGE_BYTES||!Number.isSafeInteger(offset+length))throw Error('Drawing store range exceeds limit');
}
async function execute({operation,metadata:meta={},buffers=[],handles=[]}) {
  switch(operation) {
    case 'estimate': {
      const {quota,usage}=await navigator.storage.estimate();
      return {epoch:meta.epoch,scope:self.location.origin,available_bytes:Number.isSafeInteger(quota)&&Number.isSafeInteger(usage)&&quota>=usage?quota-usage:null};
    }
    case 'legacy-windows': return legacy.windows();
    case 'legacy-manifest': return legacy.manifest(meta.key);
    case 'legacy-snapshot': return legacy.snapshot(meta.key);
    case 'legacy-range': return {buffers:[await legacy.range(meta.key,meta.resource,meta.offset,meta.length)]};
    case 'remember-handles': return destinations.remember(handles);
    case 'open': {
      const {name,access}=meta;parts(name);
      if(!['read','write','create'].includes(access))throw Error('Invalid drawing store access');
      if(files.has(name)) {if(access==='create')throw Error('Drawing store member already exists');return name;}
      const [dir,leaf]=await parent(name,access==='create');
      if(access==='create') {
        try {await dir.getFileHandle(leaf);throw Error('Drawing store member already exists');}
        catch(error) {if(error.name!=='NotFoundError')throw error;}
      }
      const entry=await dir.getFileHandle(leaf,{create:access==='create'}),handle=await entry.createSyncAccessHandle();
      files.set(name,handle);return name;
    }
    case 'close': close(meta.name);return;
    case 'close_tree': {
      parts(meta.name);
      for(const name of files.keys())if(name.startsWith(`${meta.name}/`))close(name);
      return;
    }
    case 'len': return (await file(meta.name)).getSize();
    case 'copy_read_at':
    case 'read_at': {
      range(meta.offset,meta.length);const bytes=new Uint8Array(meta.length);
      if((await file(meta.name)).read(bytes,{at:meta.offset})!==bytes.length)throw Error('Incomplete drawing store read');
      if(meta.crc!==undefined&&meta.crc!==null)verify(bytes,meta.crc);
      return {buffers:[bytes]};
    }
    case 'write_at': {
      const bytes=buffers[0];range(meta.offset,bytes?.byteLength);
      if((await file(meta.name)).write(bytes,{at:meta.offset})!==bytes.byteLength)throw Error('Incomplete drawing store write');return;
    }
    case 'flush': (await file(meta.name)).flush();return;
    case 'list': {
      let dir;try {dir=await directory(meta.name??'');}catch(error){if(error.name==='NotFoundError')return [];throw error;}
      const entries=[];for await(const [name,entry] of dir.entries())entries.push({name,directory:entry.kind==='directory'});
      return entries.sort((a,b)=>a.name<b.name?-1:a.name>b.name?1:0);
    }
    case 'remove': {
      const opened=files.get(meta.name);let bytes=opened?.getSize()??0;close(meta.name);
      try {
        const [dir,leaf]=await parent(meta.name);
        if(!opened)try {bytes=(await (await dir.getFileHandle(leaf)).getFile()).size;}
        catch(error){if(error.name!=='TypeMismatchError')throw error;}
        await dir.removeEntry(leaf);return {scope:self.location.origin,bytes};
      } catch(error){if(error.name!=='NotFoundError')throw error;return {scope:self.location.origin,bytes:0};}
    }
    default: throw Error('Unknown drawing store operation');
  }
}
function schedule() {
  if(scheduled||active||(!reads.length&&!writes.length))return;
  scheduled=true;channel.port2.postMessage(null);
}
async function drain() {
  scheduled=false;if(active)return;
  const item=reads.shift()??writes.shift();if(!item)return;active=true;
  try {
    const result=await execute(item.request),transfers=(result?.buffers??[]).map(bytes=>bytes.buffer);
    self.postMessage({id:item.id,result},transfers);
  }catch(error){self.postMessage({id:item.id,error:String(error.message??error),errorName:error.name});}
  finally {active=false;schedule();}
}
self.onmessage=({data})=> {
  const operation=data.request?.operation;
  const priority=operation==='read_at'||operation==='len'||operation?.startsWith('legacy-')||(operation==='open'&&data.request.metadata?.access==='read');
  (priority?reads:writes).push(data);schedule();
};
