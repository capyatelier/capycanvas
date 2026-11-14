use super::{ByteRange,ImmutableBacking,RangeState};
use crate::{authored::EncodedIntegrity,color::PixelDescriptor,tile_io::{RangeReader,ReadCache,ReadCompletion,ReadSource}};
use std::{collections::BTreeMap,sync::{Arc,Mutex,OnceLock,Weak}};

pub fn normalize_filtered_tile(descriptor:PixelDescriptor,bytes:&[u8],crc:u32)->Result<Arc<[u8]>,String> {
    crate::tile_io::verify_crc(bytes,Some(crc))?;
    let scratch=descriptor.byte_len([super::RASTER_TILE_SIZE;2]).ok_or("Invalid package tile descriptor")? as u64*4;
    let _admission=crate::MemoryBudget::process().admit_import(scratch).ok_or(crate::MEMORY_BUDGET_EXCEEDED)?;
    crate::raster::TileBlob::from_filtered_deflate(crate::PortableId::random(),descriptor,bytes)?.memory_compressed()?.ok_or_else(||"Missing normalized package tile".into())
}
struct Reader {backing:ImmutableBacking,ranges:Arc<Mutex<BTreeMap<(u64,usize),Arc<PackageIdentity>>>>}
impl RangeReader for Reader {
    fn allocation_bytes(&self,offset:u64,length:usize)->usize {
        let input=self.backing.read_source().map_or(length,|source|source.allocation_bytes(offset,length));
        let filtered=self.ranges.lock().unwrap().get(&(offset,length)).is_some_and(|range|range.filtered);
        if filtered {input.saturating_add(*self.result_bounds(offset,length).end())}else{input}
    }
    fn result_bounds(&self,offset:u64,length:usize)->std::ops::RangeInclusive<usize> {
        match self.ranges.lock().unwrap().get(&(offset,length)) {
            Some(range) if range.filtered=>1..=crate::raster::TileBlob::max_compressed_len(range.descriptor).unwrap(),
            _=>length..=length,
        }
    }
    fn read(&self,offset:u64,length:usize,_crc:Option<u32>,completion:ReadCompletion) {
        let range=self.ranges.lock().unwrap().get(&(offset,length)).cloned();let backing=self.backing.clone();
        let filtered=range.as_ref().filter(|range|range.filtered).map(|range|(range.original.crc32,range.descriptor));
        let source=self.backing.read_source();let prepared=source.is_some()&&filtered.is_some();
        let completion=completion.map(Box::new(move |bytes|bytes.and_then(|bytes| {
            let range=range.ok_or("Missing package tile descriptor")?;
            let bytes=if prepared {bytes}else if range.filtered {ByteRange::whole(normalize_filtered_tile(range.descriptor,&bytes,range.original.crc32)?)}
                else {crate::tile_io::verify_crc(&bytes,Some(range.original.crc32))?;bytes};
            let integrity=EncodedIntegrity {bytes:bytes.len() as u64,crc32:crc32fast::hash(&bytes)};
            if range.canonical.get_or_init(||integrity)!=&integrity {return Err("Package normalization changed immutable bytes".into());}
            Ok(bytes)
        }).map_err(|error|backing.fail(error))));
        if let Some(source)=source {
            match filtered {Some((crc,descriptor))=>source.read_filtered(offset,length,crc,descriptor,completion),None=>source.read_direct(offset,length,None,completion)}return;
        }
        #[cfg(not(target_arch="wasm32"))]
        let bytes=pollster::block_on(self.backing.wait(offset,length)).map(ByteRange::into_arc);
        #[cfg(target_arch="wasm32")]
        let bytes=self.backing.read_on_worker(offset,length).map(|bytes|Arc::from(&*bytes));
        completion.complete(bytes);
    }
}
#[derive(Clone)]
pub struct PackageLease(Arc<Lease>);
#[derive(Default)]
pub(crate) struct WeakPackageLease(Weak<Lease>);
impl WeakPackageLease {pub(crate) fn upgrade(&self)->Option<PackageLease> {self.0.upgrade().map(PackageLease)}}
struct Lease {generation:crate::PortableId,backing:ImmutableBacking,reads:ReadSource,ranges:Arc<Mutex<BTreeMap<(u64,usize),Arc<PackageIdentity>>>>}
impl PackageLease {
    pub fn new(backing:ImmutableBacking)->Self {backing.package_lease()}
    #[cfg(test)]
    pub(crate) fn read_source(&self)->&ReadSource {&self.0.reads}
    pub(crate) fn downgrade(&self)->WeakPackageLease {WeakPackageLease(Arc::downgrade(&self.0))}
    pub(crate) fn create(backing:ImmutableBacking,generation:crate::PortableId)->Self {
        let ranges=Arc::new(Mutex::new(BTreeMap::new()));
        let reads=ReadCache::process().source(backing.byte_len(),Arc::new(Reader {backing:backing.clone(),ranges:ranges.clone()}));
        Self(Arc::new(Lease {generation,backing,reads,ranges}))
    }
    pub(crate) fn range(&self,offset:u64,length:u64,crc:u32,descriptor:PixelDescriptor,filtered:bool)->Result<Arc<PackageRange>,String> {
        if length==0 || length>super::MAX_RANGE_BYTES as u64 || offset.checked_add(length).is_none_or(|end|end>self.0.backing.byte_len()) {return Err("Package tile exceeds immutable range".into());}
        let mut ranges=self.0.ranges.lock().unwrap();
        let identity=ranges.entry((offset,length as usize)).or_insert_with(|| {
            let canonical=OnceLock::new();let original=EncodedIntegrity {bytes:length,crc32:crc};if !filtered {canonical.set(original).unwrap();}
            Arc::new(PackageIdentity {generation:self.0.generation,offset,original,descriptor,filtered,canonical,identity:OnceLock::new(),backing:(self.0.backing.identity(),self.0.backing.resident_bytes(),self.0.backing.weak_owner())})
        }).clone();
        if identity.original.crc32!=crc || identity.descriptor!=descriptor || identity.filtered!=filtered {return Err("Conflicting immutable package range descriptor".into());}
        Ok(Arc::new(PackageRange {lease:self.clone(),identity}))
    }
}
pub(crate) struct PackageIdentity {generation:crate::PortableId,offset:u64,original:EncodedIntegrity,descriptor:PixelDescriptor,filtered:bool,canonical:OnceLock<EncodedIntegrity>,identity:OnceLock<crate::raster::TileContentIdentity>,backing:(u64,usize,Weak<dyn Send+Sync>)}
impl PackageIdentity {
    pub(crate) fn backing_owner(&self)->(u64,usize) {(self.backing.0,if self.backing.2.strong_count()>0 {self.backing.1}else {0})}
    pub(crate) fn integrity(&self)->Option<EncodedIntegrity> {self.canonical.get().copied()}
    pub(crate) fn length(&self)->usize {self.integrity().unwrap_or(self.original).bytes as usize}
    pub(crate) fn identity(&self,id:crate::PortableId)->Result<crate::raster::TileContentIdentity,String> {
        if let Some(identity)=self.identity.get() {return Ok(*identity);}
        let integrity=self.integrity().ok_or("Package normalization must finish before transfer")?;
        let descriptor=super::session_store::tile_fingerprint(id,self.descriptor,integrity.bytes,integrity.crc32)?;
        Ok(*self.identity.get_or_init(||crate::raster::TileContentIdentity::Package {generation:self.generation,offset:self.offset,bytes:integrity.bytes,crc32:integrity.crc32,descriptor}))
    }
}
pub(crate) struct TransferRange {pub backing:ImmutableBacking,pub offset:u64,pub original:EncodedIntegrity,pub filtered:bool,pub canonical:Option<EncodedIntegrity>}
pub(crate) struct PackageRange {lease:PackageLease,pub(crate) identity:Arc<PackageIdentity>}
impl PackageRange {
    pub(crate) fn transfer_range(&self)->TransferRange {
        TransferRange {backing:self.lease.0.backing.clone(),offset:self.identity.offset,original:self.identity.original,filtered:self.identity.filtered,canonical:self.identity.integrity()}
    }
    pub(crate) fn seed_integrity(&self,integrity:EncodedIntegrity)->Result<(),String> {
        if self.identity.canonical.get_or_init(||integrity)!=&integrity {return Err("Conflicting package normalization integrity".into());}Ok(())
    }
    pub(crate) fn backing_owner(&self)->(u64,usize) {self.identity.backing_owner()}
    pub(crate) fn healthy(&self)->Result<(),String> {self.lease.0.backing.healthy()}
    pub(crate) fn fail(&self,error:String)->String {self.lease.0.backing.fail(error)}
    pub(crate) fn poll(&self)->Result<RangeState,String> {
        self.healthy()?;self.lease.0.reads.poll(self.identity.offset,self.identity.original.bytes as usize).map_err(|error|self.fail(error))
    }
    pub(crate) async fn read(&self)->Result<ByteRange,String> {
        self.read_interest()?.await.map_err(|error|self.fail(error))
    }
    pub(crate) fn read_interest(&self)->Result<crate::tile_io::ReadWait,String> {self.healthy()?;self.lease.0.reads.wait(self.identity.offset,self.identity.original.bytes as usize)}
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn read_on_worker(&self)->Result<ByteRange,String> {
        self.healthy()?;self.lease.0.reads.wait_on_worker(self.identity.offset,self.identity.original.bytes as usize).map_err(|error|self.fail(error))
    }
}
