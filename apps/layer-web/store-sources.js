export function createSourceRegistry({file,parts,connected=()=>true}) {
  const sources=new Map(),leases=new Map(),offers=new Map(),waiters=[];
  function drop(source) {
    if(--source.references===0) {sources.delete(source.key);flush();}
  }
  function held(name,tree=false) {
    return [...sources.values()].some(source=>source.name&&(tree?source.name.startsWith(`${name}/`):source.name===name));
  }
  function flush() {
    for(let index=waiters.length-1;index>=0;index--) {
      const waiter=waiters[index];if(held(waiter.name,waiter.tree))continue;
      waiters.splice(index,1);waiter.resolve();
    }
  }
  async function source(reference) {
    const {key,length}=reference;
    if(typeof key!=='string'||!Number.isSafeInteger(length)||length<0)throw Error('Invalid backing reference');
    let source=sources.get(key);
    if(!source) {
      if(!key.startsWith('store:'))throw Error('File generation is no longer available');
      const name=key.slice(6);parts(name);const handle=await file(name);
      if(length>handle.getSize())throw Error('Backing reference exceeds immutable extent');
      source={key,name,handle,references:0};sources.set(key,source);
    }
    if(length>(source.blob?.size??source.handle.getSize()))throw Error('Backing reference exceeds immutable extent');
    return source;
  }
  function lease(source,reference,owner,id=crypto.randomUUID()) {
    if(typeof id!=='string'||!id||id.length>64||leases.has(id))throw Error('Invalid backing lease identity');source.references++;leases.set(id,{source,reference,owner});return id;
  }
  async function retire(ticket) {
    const offer=offers.get(ticket);if(!offer)return;
    offers.delete(ticket);if(offer.parent)offers.get(offer.parent)?.children.delete(ticket);
    await Promise.all([...offer.children].map(retire).concat(offer.entries.map(entry=>drop(entry.source))));
  }
  async function release(id,owner) {
    const entry=leases.get(id);if(!entry)return;
    if(entry.owner!==owner)throw Error('Backing lease belongs to another connection');
    leases.delete(id);
    const retiring=[];
    if(entry.registration)for(const [ticket,offer]of offers)if(offer.references.some(reference=>reference.key===entry.source.key))retiring.push(retire(ticket));
    retiring.push(drop(entry.source));await Promise.all(retiring);
  }
  async function cancel(ticket,owner) {
    const offer=offers.get(ticket);if(!offer)return;
    if(offer.owner!==owner)throw Error('Backing offer belongs to another connection');
    await retire(ticket);
  }
  return {
    held,
    after(name,tree=false) {return {deferred:held(name,tree)?new Promise(resolve=>waiters.push({name,tree,resolve})):Promise.resolve()};},
    accept(request,owner) {
      if(request.operation!=='read_source')return null;
      const {lease:id,offset,length}=request.metadata??{},entry=leases.get(id);
      if(!entry||entry.owner!==owner)throw Error('Backing lease is no longer available');
      if(!Number.isSafeInteger(offset)||offset<0||!Number.isSafeInteger(length)||length<0||length>8*1024*1024||!Number.isSafeInteger(offset+length)||offset+length>entry.reference.length)throw Error('Backing read exceeds immutable range');
      entry.source.references++;
      return {source:entry.source,offset,length,release:()=>drop(entry.source)};
    },
    async read(accepted) {
      const {source,offset,length}=accepted;
      let bytes;
      if(source.blob) {
        try {
          bytes=new Uint8Array(await source.blob.slice(offset,offset+length).arrayBuffer());
          if(bytes.byteLength!==length)throw Error('Incomplete Blob range');
        }catch(error){throw Error(`Source ${source.key} was lost: ${error.message??error}`,{cause:error});}
      }else {
        bytes=new Uint8Array(length);
        if(source.handle.read(bytes,{at:offset})!==length)throw Error('Incomplete backing range');
      }
      return {buffers:[bytes]};
    },
    disconnect(owner) {
      for(const [ticket,offer]of offers)if(offer.owner===owner)cancel(ticket,owner).catch(()=>{});
      for(const [id,entry]of leases)if(entry.owner===owner)release(id,owner).catch(()=>{});
    },
    async execute(request,owner) {
      if(!connected(owner))throw Error('Drawing storage connection closed');
      const meta=request.metadata??{};
      switch(request.operation) {
        case 'register_blob': {
          if(!(request.file instanceof Blob))throw Error('Invalid file generation');
          const id=meta.lease??crypto.randomUUID();
          if(typeof id!=='string'||!id||id.length>64||leases.has(id)||sources.has(`blob:${id}`))throw Error('Invalid file generation identity');
          const key=`blob:${id}`,reference={key,length:request.file.size};
          const source={key,blob:request.file,references:0};sources.set(key,source);
          lease(source,reference,owner,id);leases.get(id).registration=true;return {...reference,lease:id,file:request.file};
        }
        case 'release_source': await release(meta.lease,owner);return;
        case 'offer_sources': {
          if(!Array.isArray(meta.references))throw Error('Invalid backing offer');
          const parent=meta.parent?offers.get(meta.parent):null;
          if(meta.parent&&!parent?.adopted)throw Error('Backing operation has ended');
          const ticket=meta.ticket??crypto.randomUUID();
          if(typeof ticket!=='string'||!ticket||ticket.length>64||offers.has(ticket))throw Error('Invalid backing offer identity');
          const entries=[];
          try {for(const reference of meta.references) {const value=await source(reference);value.references++;entries.push({source:value,reference});if(!connected(owner))throw Error('Drawing storage connection closed');}}
          catch(error){for(const entry of entries)drop(entry.source);throw error;}
          if(meta.parent&&!offers.has(meta.parent)){for(const entry of entries)drop(entry.source);throw Error('Backing operation has ended');}
          offers.set(ticket,{entries,references:meta.references,owner,parent:meta.parent,children:new Set(),adopted:false});parent?.children.add(ticket);return ticket;
        }
        case 'claim_sources': {
          const offer=offers.get(meta.ticket);if(!offer)throw Error('Backing offer is no longer available');offer.owner=owner;return;
        }
        case 'cancel_sources': await cancel(meta.ticket,owner);return;
        case 'adopt_sources': {
          const offer=offers.get(meta.ticket);
          if(!offer||offer.adopted||!Array.isArray(meta.references)||offer.references.length!==meta.references.length||offer.references.some((reference,index)=>['key','generation','length'].some(field=>reference[field]!==meta.references[index][field])))throw Error('Backing offer identity mismatch');
          if(meta.leases&&(!Array.isArray(meta.leases)||meta.leases.length!==offer.entries.length||new Set(meta.leases).size!==meta.leases.length||meta.leases.some(id=>typeof id!=='string'||!id||id.length>64||leases.has(id))))throw Error('Invalid backing lease identities');
          const result=offer.entries.map((entry,index)=>lease(entry.source,entry.reference,owner,meta.leases?.[index]));
          for(const entry of offer.entries)drop(entry.source);offer.entries=[];offer.adopted=true;return result;
        }
        default: throw Error('Unknown backing operation');
      }
    },
  };
}
