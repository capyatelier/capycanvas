use super::{ByteRange, ByteSource, ImmutableBacking, RangeState, MAX_RANGE_BYTES,
    resources::Payload, session::OpenSession};
use crate::{PortableId, ProjectLimits};
use crate::tile_io::{ReadCache, ReadSource, RangeReader, ReadCompletion};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicBool, AtomicU64, Ordering}}};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation { id: PortableId, sha256: String }
impl Generation {
    fn new(id: PortableId, metadata: &[u8]) -> Self {
        let hex = b"0123456789abcdef";
        Self { id, sha256: Sha256::digest(metadata).iter().flat_map(|byte| [hex[(byte >> 4) as usize] as char, hex[(byte & 15) as usize] as char]).collect() }
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Head { current: Generation, previous: Option<Generation> }

#[cfg(not(target_arch="wasm32"))]
mod native;
#[cfg(not(target_arch="wasm32"))]
pub use native::*;

pub(super) const PACK_BYTES: u64 = 64 * 1024 * 1024;
mod head;
mod io;
mod metadata;
pub use metadata::MetadataStore;
pub async fn store_retired(io:&dyn super::store_io::FileTransport)->Result<bool,String> {io::marker(io,".retired").await}
const RESOURCE_CHECKSUM_FAILED:&str="Recovery resource checksum failed";
#[derive(Clone,Debug)]
pub enum ImportFailure {Invalid(String),Unsupported(String),MemoryBudgetExceeded,Changed,Io(String)}
impl ImportFailure {
    pub fn message(self)->String {match self {Self::Invalid(error)|Self::Unsupported(error)|Self::Io(error)=>error,Self::MemoryBudgetExceeded=>crate::MEMORY_BUDGET_EXCEEDED.into(),Self::Changed=>"The saved drawing changed during import".into()}}
}
impl From<super::session::OpenFailure> for ImportFailure {
    fn from(error:super::session::OpenFailure)->Self {match error {
        super::session::OpenFailure::Invalid(error)=>Self::Invalid(error),super::session::OpenFailure::Unsupported(error)=>Self::Unsupported(error),super::session::OpenFailure::MemoryBudgetExceeded|super::session::OpenFailure::Uncovered=>Self::MemoryBudgetExceeded,
    }}
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackRange { pack: PortableId, offset: u64 }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackedGeneration {
    format: String,
    version: u32,
    session: serde_json::Value,
    extents: BTreeMap<PortableId, u64>,
}
struct StoredGeneration {
    metadata: Vec<u8>,
    locations: Option<BTreeMap<PortableId, PackRange>>,
    extents: BTreeMap<PortableId, u64>,
}
impl StoredGeneration {
    fn parse(bytes: &[u8], limits: ProjectLimits) -> Result<Self, super::session::OpenFailure> {
        let value = super::parse_json(bytes, limits.metadata_bytes.min(usize::MAX as u64) as usize)?;
        if value["format"] != "capy.session.packed" {
            return Ok(Self { metadata: bytes.into(), locations: None, extents: BTreeMap::new() });
        }
        if value["version"] != 2 {return Err(super::session::OpenFailure::Unsupported("Unsupported packed recovery format".into()));}
        let packed:PackedGeneration=serde_json::from_value(value).map_err(|error|error.to_string())?;
        let metadata=serde_json::to_vec(&packed.session).map_err(|error|error.to_string())?;
        let index=resource_index(&metadata,limits)?;
        let mut locations=BTreeMap::new();let mut referenced=BTreeSet::new();
        for (record,value) in index.records.iter().zip(packed.session["resources"].as_array().unwrap()) {
            let name=value["location"]["pack"].as_str().ok_or("Missing recovery pack identity")?;
            let pack=parse_pack(name)?;
            let extent=*packed.extents.get(&pack).ok_or("Missing recovery pack extent")?;
            if extent==0 || record.offset.checked_add(record.length).is_none_or(|end|end>extent) {return Err("Recovery range exceeds committed pack extent".into());}
            locations.insert(record.id,PackRange {pack,offset:record.offset});referenced.insert(pack);
        }
        if referenced.len()!=packed.extents.len() {return Err("Unreferenced recovery pack extent".into());}

        Ok(Self {metadata,locations:Some(locations),extents:packed.extents})
    }
}
fn parse_head(bytes: &[u8]) -> Result<Head, String> {
    let head: Head = serde_json::from_value(super::parse_json(bytes, 1024)?).map_err(|e| e.to_string())?;
    if std::iter::once(&head.current).chain(head.previous.as_ref()).any(|generation|
        generation.sha256.len() != 64 || !generation.sha256.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        || head.previous.as_ref().is_some_and(|previous| previous.id == head.current.id) {
        return Err("Invalid recovery metadata checksum reference".into());
    }
    Ok(head)
}

fn parse_pack(name:&str)->Result<PortableId,String> {
    name.strip_prefix("packs/").and_then(|name|name.strip_suffix(".bin")).ok_or("Invalid recovery pack identity")?.parse().map_err(str::to_string)
}
fn same_profiles(a:&[crate::color::ColorProfile],b:&[crate::color::ColorProfile])->bool {
    a.len()==b.len() && a.iter().zip(b).all(|(a,b)|match (a,b) {
        (crate::color::ColorProfile::Builtin(a),crate::color::ColorProfile::Builtin(b))=>a==b,
        (crate::color::ColorProfile::Icc(a),crate::color::ColorProfile::Icc(b))=>a.same_owner(b),_=>false,
    })
}
fn update_session_metadata(bytes:&[u8],metadata:&serde_json::Value)->Result<Vec<u8>,String> {
    use serde_json::value::RawValue;
    #[derive(Deserialize)]
    struct Packed<'a> {#[serde(borrow)]format:&'a str,version:u32,#[serde(borrow)]session:&'a RawValue,#[serde(borrow)]extents:&'a RawValue}
    #[derive(Serialize)]
    struct Updated<'a> {format:&'a str,version:u32,session:BTreeMap<&'a str,&'a RawValue>,extents:&'a RawValue}
    let packed:Packed<'_>=serde_json::from_slice(bytes).map_err(|error|error.to_string())?;
    let mut session:BTreeMap<&str,&RawValue>=serde_json::from_str(packed.session.get()).map_err(|error|error.to_string())?;
    let metadata=serde_json::value::to_raw_value(metadata).map_err(|error|error.to_string())?;
    session.insert("metadata",&metadata);
    serde_json::to_vec(&Updated {format:packed.format,version:packed.version,session,extents:packed.extents}).map_err(|error|error.to_string())
}
mod record;
mod compaction;
pub use compaction::{CompactionActivity,CompactionStep};
use record::StoredRecord;
struct IndexedResource {record:StoredRecord,location:Arc<StoreLocation>,owner:Option<Weak<dyn Send+Sync>>}
#[derive(Clone)]
struct StoreInput {
    id:PortableId,record:StoredRecord,payload:Payload,length:u64,crc:u32,descriptor:[u8;32],owner:Weak<dyn Send+Sync>,
}
pub struct PackageCopy {tiles:Vec<Arc<crate::raster::TileBlob>>,next:usize}
impl PackageCopy {
    pub async fn new(inventory:&crate::raster_storage::RetainedTiles)->Result<Self,String> {
        Ok(Self {tiles:inventory.await_blobs().await?.into_iter().filter(|tile|tile.has_package_origin()).collect(),next:0})
    }
    pub fn remaining(&self)->usize {self.tiles.len()-self.next}
    pub fn complete(&self)->bool {self.next==self.tiles.len()}
}
struct StoredInputs {
    ids:Vec<PortableId>,
    bindings:Vec<(Arc<crate::raster::TileBlob>,Arc<StoreLocation>)>,
    reservation:Option<super::storage_budget::StorageReservation>,
}
impl StoredInputs {
    fn commit(self)->Result<(),String> {
        for (tile,location) in self.bindings {
            tile.compressed.healthy()?;
            tile.compressed.bind(location)?;
        }
        Ok(())
    }
}
impl StoreInput {
    fn entry(entry:&super::resources::ResourceEntry,cancelled:&AtomicBool)->Result<Self,String> {
        if let Payload::Tile(tile)=&entry.payload {
            let integrity=tile.compressed.integrity(cancelled)?;
            let record=StoredRecord::tile(tile.resource_id(),tile.descriptor)?;
            let descriptor=match tile.compressed.location()? {Some(location)=>location.descriptor,None=>record.fingerprint(integrity.bytes,integrity.crc32)?};
            return Ok(Self {id:tile.resource_id(),record,payload:entry.payload.clone(),length:integrity.bytes,crc:integrity.crc32,descriptor,owner:entry.payload.weak_owner()});
        }
        let (record,integrity)=entry.store_record(cancelled)?;Self::new(record,entry.payload.clone(),integrity.bytes,integrity.crc32)
    }
    fn opaque(resource:Arc<crate::authored::OpaqueResource>)->Result<Self,String> {
        Self::new(resource.record(serde_json::Value::Null),Payload::Opaque(resource.clone()),resource.length,resource.crc32)
    }
    fn new(record:serde_json::Value,payload:Payload,length:u64,crc:u32)->Result<Self,String> {
        let descriptor=resource_descriptor(&record)?;let record=StoredRecord::parse(record)?;let owner=payload.weak_owner();
        Ok(Self {id:payload.id(),record,payload,length,crc,descriptor,owner})
    }
    fn owner_key(&self)->(u8,usize) {
        let kind=match self.payload {Payload::Tile(_)=>0,Payload::Bytes(_)=>1,Payload::Code(_)=>2,Payload::Encoded(_)=>3,Payload::Vector(_)=>4,Payload::Opaque(_)=>5};
        (kind,self.owner.as_ptr() as *const () as usize)
    }
}
#[derive(Clone)]
pub struct DrawingStore(Arc<DrawingStoreInner>);
pub trait StoreOpener:Send+Sync {
    fn storage_facts<'a>(&'a self,path:&'a Path)->super::store_io::FileFuture<'a,super::storage_budget::StorageFacts>;
    fn open<'a>(&'a self,path:&'a Path,retirement:bool)->super::store_io::FileFuture<'a,Option<SessionStore>>;
}
pub fn check_store_key(key:&str)->Result<(),String> {
    if key.is_empty() || key.len()>128 || !key.bytes().all(|byte|byte.is_ascii_alphanumeric() || byte==b'-' || byte==b'_') {Err("Invalid session drawing key".into())}else {Ok(())}
}
struct DrawingStoreInner {key:String,path:PathBuf,writer:Mutex<WriterSlot>,opener:Arc<dyn StoreOpener>}
#[derive(Default)]
struct WriterSlot {store:Option<SessionStore>,busy:bool,waiting:std::collections::VecDeque<(Arc<()>,std::task::Waker)>}
struct WriterLease<'a> {slot:&'a Mutex<WriterSlot>,store:Option<SessionStore>}
struct WriterAcquire<'a> {slot:&'a Mutex<WriterSlot>,token:Option<Arc<()>>}
impl<'a> std::future::Future for WriterAcquire<'a> {
    type Output=Result<WriterLease<'a>,String>;
    fn poll(mut self:std::pin::Pin<&mut Self>,cx:&mut std::task::Context<'_>)->std::task::Poll<Self::Output> {
        let mut state=match self.slot.lock() {Ok(state)=>state,Err(_)=>return std::task::Poll::Ready(Err("Drawing writer failed".into()))};
        let ready=!state.busy && match &self.token {
            Some(token)=>state.waiting.front().is_some_and(|(first,_)|Arc::ptr_eq(first,token)),
            None=>state.waiting.is_empty(),
        };
        if ready {
            if self.token.take().is_some() {state.waiting.pop_front();}
            state.busy=true;
            return std::task::Poll::Ready(Ok(WriterLease {slot:self.slot,store:state.store.take()}));
        }
        if let Some(token)=&self.token {
            let (_,wake)=state.waiting.iter_mut().find(|(current,_)|Arc::ptr_eq(current,token)).unwrap();
            wake.clone_from(cx.waker());
        }else {
            let token=Arc::new(());state.waiting.push_back((token.clone(),cx.waker().clone()));self.token=Some(token);
        }
        std::task::Poll::Pending
    }
}
impl Drop for WriterAcquire<'_> {
    fn drop(&mut self) {
        let Some(token)=self.token.take() else {return};
        let wake={let mut state=self.slot.lock().unwrap_or_else(|error|error.into_inner());
            state.waiting.retain(|(current,_)|!Arc::ptr_eq(current,&token));
            if state.busy {None}else {state.waiting.front().map(|(_,wake)|wake.clone())}};
        if let Some(wake)=wake {wake.wake();}
    }
}
impl<'a> WriterLease<'a> {
    async fn take(slot:&'a Mutex<WriterSlot>)->Result<Self,String> {WriterAcquire {slot,token:None}.await}
}
impl Drop for WriterLease<'_> {
    fn drop(&mut self) {
        let wake={let mut slot=self.slot.lock().unwrap_or_else(|error|error.into_inner());
            slot.store=self.store.take();slot.busy=false;slot.waiting.front().map(|(_,wake)|wake.clone())};
        if let Some(wake)=wake {wake.wake();}
    }
}
impl DrawingStore {
    #[cfg(not(target_arch="wasm32"))]
    pub fn new(root:&Path,key:Option<&str>)->Result<Self,String> {Self::with_opener(root,key,Arc::new(NativeStoreOpener))}
    pub fn with_opener(root:&Path,key:Option<&str>,opener:Arc<dyn StoreOpener>)->Result<Self,String> {
        let key=key.map(str::to_owned).unwrap_or_else(||PortableId::random().to_string());
        check_store_key(&key)?;
        Ok(Self(Arc::new(DrawingStoreInner {path:root.join(&key),key,writer:Default::default(),opener})))
    }
    pub fn key(&self)->&str {&self.0.key}
    pub fn path(&self)->&Path {&self.0.path}
    pub async fn with_store<T>(&self,work:impl AsyncFnOnce(&mut SessionStore)->Result<T,String>)->Result<T,String> {
        let mut writer=WriterLease::take(&self.0.writer).await?;
        if writer.store.is_none() {
            writer.store=Some(self.0.opener.open(&self.0.path,false).await?.ok_or("Drawing store is unavailable")?);
        }
        work(writer.store.as_mut().unwrap()).await
    }
    pub async fn storage_facts(&self)->Result<super::storage_budget::StorageFacts,String> {
        let writer=WriterLease::take(&self.0.writer).await?;
        let facts=match writer.store.as_ref() {Some(store)=>store._owner.io.storage_facts().await?,None=>self.0.opener.storage_facts(&self.0.path).await?};
        super::storage_budget::StorageBudget::process().publish(facts.clone());Ok(facts)
    }
    pub async fn with_retirement_store<T>(&self,work:impl AsyncFnOnce(&mut SessionStore)->Result<T,String>)->Result<Option<T>,String> {
        let mut writer=WriterLease::take(&self.0.writer).await?;
        if writer.store.is_none() {
            writer.store=self.0.opener.open(&self.0.path,true).await?;
        }
        if writer.store.is_none() {return Ok(None);}
        work(writer.store.as_mut().unwrap()).await.map(Some)
    }
}

#[derive(PartialEq)]
struct CheckpointKey {checkpoint:crate::authored::CaptureCheckpoint,context:Option<crate::authored::EvaluationContext>,next:u64,stroke:u64,undo:Vec<u64>,redo:Vec<u64>}
impl CheckpointKey {
    fn of(capture:&super::session::EditorCapture)->Self {
        let artwork=&capture.artwork.artwork;
        let context=artwork.outputs.get(artwork.default_output).map(|output| {let mut context=output.context.clone();context.elapsed=0.;context});
        Self {checkpoint:capture.captured_checkpoint,context,next:capture.next_checkpoint,stroke:capture.document.next_stroke_id,undo:capture.undo.iter().map(|(_,id)|*id).collect(),redo:capture.redo.iter().map(|(_,id)|*id).collect()}
    }
}
struct PublishedCapture {key:CheckpointKey,profiles:Vec<crate::color::ColorProfile>,generation:PortableId}
pub struct SessionStore {
    path: PathBuf,
    _owner: Arc<StoreOwner>,
    _writer: Arc<()>,
    head: Option<Head>,
    sequence:u64,
    loaded: Option<Generation>,
    index:BTreeMap<PortableId,IndexedResource>,
    by_owner:BTreeMap<(u8,usize),PortableId>,
    append:Option<(PortableId,u64)>,
    programs: BTreeMap<PortableId, super::TrustedPrograms>,
    reads: crate::tile_io::ReadCache,
    generation_roots:BTreeMap<PortableId,(Generation,GenerationRoots)>,
    published_capture:Option<PublishedCapture>,
    compaction:Option<compaction::Copy>,
    compaction_refusal:Option<super::storage_budget::StorageRefusal>,
    compaction_fallback:Option<Head>,
    storage_scope:Option<String>,
}
#[derive(Clone,Default)]
struct GenerationRoots {resources:BTreeSet<PortableId>,packs:BTreeSet<PortableId>,identities:BTreeSet<PortableId>}
struct StoreRoots {
    generations: BTreeSet<PortableId>,
    resources: BTreeSet<PortableId>,
    packs: BTreeSet<PortableId>,
    identities: BTreeSet<PortableId>,
}
struct StoreOwner {
    _lease:Arc<dyn Send+Sync>,
    io:Arc<dyn super::store_io::FileTransport>,
    packs:Mutex<BTreeMap<PortableId,Weak<Pack>>>,
}
struct Ownership {owner:Weak<StoreOwner>,writer:Weak<()>}
type StoreClaim=(Arc<StoreOwner>,Arc<()>);
type TransportClaim=(Arc<dyn super::store_io::FileTransport>,Arc<dyn Send+Sync>);
fn claim_store_with(path:&Path,claim:impl FnOnce()->Result<Option<TransportClaim>,String>)->Result<Option<StoreClaim>,String> {
    static OWNERS:OnceLock<Mutex<BTreeMap<PathBuf,Ownership>>>=OnceLock::new();
    let mut owners=OWNERS.get_or_init(Default::default).lock().map_err(|_|"Recovery ownership lock failed")?;
    owners.retain(|_,ownership|ownership.owner.strong_count()!=0);
    if owners.get(path).is_some_and(|ownership|ownership.writer.strong_count()!=0) {return Ok(None);}
    let owner=match owners.get(path).and_then(|ownership|ownership.owner.upgrade()) {
        Some(owner)=>owner,
        None=>match claim()? {
            Some((io,lease))=>Arc::new(StoreOwner {_lease:lease,io,packs:Default::default()}),
            None=>return Ok(None),
        },
    };
    let writer=Arc::new(());owners.insert(path.into(),Ownership {owner:Arc::downgrade(&owner),writer:Arc::downgrade(&writer)});
    Ok(Some((owner,writer)))
}
impl SessionStore {
    fn for_retirement(path:PathBuf,owner:Arc<StoreOwner>,writer:Arc<()>)->Self {
        Self {path,_owner:owner,_writer:writer,head:None,sequence:0,loaded:None,index:BTreeMap::new(),by_owner:BTreeMap::new(),append:None,
            programs:BTreeMap::new(),reads:ReadCache::process(),generation_roots:BTreeMap::new(),published_capture:None,compaction:None,compaction_refusal:None,compaction_fallback:None,storage_scope:None}
    }
    pub async fn retirement_transport(path:PathBuf,transport:Arc<dyn super::store_io::FileTransport>,lease:Arc<dyn Send+Sync>)->Result<Option<Self>,String> {
        let (owner,writer)=claim_store_with(&path,||Ok(Some((transport,lease))))?.ok_or("Recovery drawing is already open")?;
        let store=Self::for_retirement(path,owner,writer);
        Ok((!io::marker(&*store._owner.io,".retired").await? && store.validate_layout().await.is_ok()).then_some(store))
    }
    pub async fn collect_unreferenced_transport(path:PathBuf,transport:Arc<dyn super::store_io::FileTransport>,lease:Arc<dyn Send+Sync>)->bool {
        let Ok(Some((owner,writer)))=claim_store_with(&path,||Ok(Some((transport,lease)))) else {return false;};
        Self::collect_unreferenced_claim(path,owner,writer).await.unwrap_or(false)
    }
    async fn collect_unreferenced_claim(path:PathBuf,owner:Arc<StoreOwner>,writer:Arc<()>)->Result<bool,String> {
        let entries=owner.io.list("").await?;
        let terminal=io::marker(&*owner.io,".retired").await?;
        let retiring=io::marker(&*owner.io,".retiring").await?;
        if entries.iter().any(|entry|matches!(entry.name.as_str(),"head.0"|"head.1"|"head.json")) && !terminal && !retiring {return Ok(false);}
        if entries.is_empty() || terminal && entries.iter().all(|entry|matches!(entry.name.as_str(),".retired"|".lock")) {return Ok(false);}
        let mut store=Self::for_retirement(path,owner,writer);store.validate_layout().await?;
        store.retire().await?;Ok(true)
    }
    pub async fn open_transport(path:PathBuf,transport:Arc<dyn super::store_io::FileTransport>,lease:Arc<dyn Send+Sync>,reads:ReadCache)->Result<Self,String> {
        let (owner,writer)=claim_store_with(&path,||Ok(Some((transport,lease))))?.ok_or("Recovery drawing is already open")?;
        Self::initialize(path,owner,writer,reads).await
    }
    async fn initialize(path:PathBuf,owner:Arc<StoreOwner>,writer:Arc<()>,reads:ReadCache)->Result<Self,String> {
        if io::marker(&*owner.io,".retired").await? {return Err("This drawing's session copy was retired".into());}
        let published=head::read(&*owner.io).await?;
        let (head,sequence)=match published {
            Some(published)=>(Some(published.head),published.sequence),
            None=>(io::read_optional(&*owner.io,"head.json",1024).await?.map(|bytes|parse_head(&bytes)).transpose()?,0),
        };
        let mut store=Self {path,_owner:owner,_writer:writer,head,sequence,loaded:None,
            index:BTreeMap::new(),by_owner:BTreeMap::new(),append:None,programs:BTreeMap::new(),reads,generation_roots:BTreeMap::new(),published_capture:None,compaction:None,compaction_refusal:None,compaction_fallback:None,storage_scope:None};
        if let Ok(facts)=store._owner.io.storage_facts().await {
            store.storage_scope=Some(facts.scope.clone());super::storage_budget::StorageBudget::process().publish(facts);
        }
        if let Some(head)=&store.head {
            for generation in std::iter::once(&head.current).chain(head.previous.as_ref()) {
                if let Ok(bytes)=store.generation_metadata(generation,ProjectLimits::default().metadata_bytes).await
                    && let Ok(stored)=StoredGeneration::parse(&bytes,ProjectLimits::default()) {
                    store.recover_append(&stored.extents);break;
                }
            }
        }
        match store.resource_roots().await {
            Ok(roots)=>store.collect_roots(roots).await?,
            Err(_)=>{
                for directory in ["","generations","resources","packs"] {io::clean(&*store._owner.io,directory,&BTreeSet::new(),".unused").await?;}
            }
        }
        Ok(store)
    }
    pub fn path(&self) -> &Path { &self.path }
    pub fn generation(&self) -> Option<PortableId> { self.head.as_ref().map(|head| head.current.id) }
    pub fn loaded_generation(&self) -> Option<PortableId> { self.loaded.as_ref().map(|loaded| loaded.id) }
    pub fn recovered_previous(&self) -> bool { self.loaded_generation().is_some_and(|loaded| Some(loaded) != self.generation()) }
    pub async fn checkpoint(&mut self,capture:&super::session::EditorCapture,metadata:super::session::SessionMetadata,cancelled:&AtomicBool)->Result<PortableId,String> {
        let memory=capture.artwork().memory.as_ref().map(|memory|memory.weak());
        let key=CheckpointKey::of(capture);let profiles=metadata.profiles.clone();
        let result=async {
        capture.validate_identity()?;let tiles=capture.retained_tiles();tiles.check_integrity()?;self.refresh_head().await?;
        if let Some(published)=&self.published_capture && published.key==key && self.loaded_generation()==Some(published.generation) && self.generation()==Some(published.generation)
            && same_profiles(&published.profiles,&metadata.profiles) {
            let generation=self.head.as_ref().unwrap().current.clone();
            let bytes=self.generation_metadata(&generation,ProjectLimits::default().metadata_bytes).await?;
            let metadata=update_session_metadata(&bytes,&metadata.value)?;
            let roots=self.generation_roots.get(&generation.id).ok_or("Missing published generation roots")?.1.clone();
            let programs=self.programs.get(&generation.id).cloned().unwrap_or_default();
            tiles.check_integrity()?;return self.publish_packed(metadata,roots,&programs,cancelled).await;
        }
        for blob in capture.retained_tiles().await_blobs().await? {if blob.compressed.needs_preparation() {blob.read_compressed().await?;}}
        let draft=super::session::SessionDraft::capture(capture,metadata,cancelled)?;
        let mut inputs=draft.inventory.entries.values().map(|entry|StoreInput::entry(entry,cancelled)).collect::<Result<Vec<_>,_>>()?;
        for opaque in &draft.opaque {inputs.push(StoreInput::opaque(opaque.clone())?);}
        let order=draft.spatial_order();
        inputs.sort_by_key(|input|order.get(&input.id).copied().unwrap_or((usize::MAX,0,0)));
        let stored=self.store_inputs(inputs,cancelled).await?;
        let extents=self.pack_extents(&stored.ids).await?;
        let records=stored.ids.iter().map(|id|&self.index[id]).collect::<Vec<_>>();
        let programs=draft.trusted_programs.clone();
        let metadata=draft.packed_metadata(&records,&extents)?;
        let roots=GenerationRoots {identities:stored.ids.iter().copied().collect(),packs:extents.into_keys().collect(),..Default::default()};
        tiles.check_integrity()?;
        let generation=self.publish_packed(metadata,roots,&programs,cancelled).await?;
        stored.commit()?;Ok(generation)
        }.await;
        if let Ok(generation)=result {self.published_capture=Some(PublishedCapture {key,profiles,generation});}
        if let Some(owner)=memory.and_then(|owner|owner.upgrade()) {owner.refresh();}
        result
    }

    fn resource_values(&self,ids:&[PortableId])->Result<Vec<serde_json::Value>,String> {
        ids.iter().map(|id|serde_json::to_value(&self.index[id]).map_err(|error|error.to_string())).collect()
    }
    async fn pack_extents(&self,ids:&[PortableId])->Result<BTreeMap<PortableId,u64>,String> {
        let mut extents=BTreeMap::new();
        for id in ids {
            let location=self.index.get(id).ok_or("Missing durable resource index")?.location.range();let pack=location.pack.id;
            let extent=location.pack.extent.load(Ordering::Acquire);
            if !extents.contains_key(&pack) {
                if location.pack.file.len().await?<extent {return Err("Recovery pack is shorter than its committed extent".into());}
                extents.insert(pack,extent);
            }
        }Ok(extents)
    }
    async fn publish_packed(&mut self,metadata:Vec<u8>,roots:GenerationRoots,programs:&super::TrustedPrograms,cancelled:&AtomicBool)->Result<PortableId,String> {
        check_cancelled(cancelled)?;self.validate_layout().await?;self.refresh_head().await?;
        if io::marker(&*self._owner.io,".retired").await? {return Err("This drawing's session copy was retired".into());}
        if metadata.len() as u64>ProjectLimits::default().metadata_bytes {return Err("Packed recovery metadata exceeds admission limit".into());}
        let mut reservation=super::storage_budget::reserve(&*self._owner.io,metadata.len() as u64+head::SLOT_BYTES as u64).await?;
        let generation=PortableId::random();
        let created=io::create(&*self._owner.io,&format!("generations/{generation}.json"),&metadata,cancelled).await;
        reservation.written(metadata.len() as u64)?;created?;
        let reference=Generation::new(generation,&metadata);
        let head=Head {current:reference.clone(),previous:self.loaded.clone().or_else(||self.head.as_ref().map(|head|head.current.clone()))};
        check_cancelled(cancelled)?;
        let published=head::publish(&*self._owner.io,self.sequence,&head).await;
        reservation.written(head::SLOT_BYTES as u64)?;
        let sequence=match &published {
            Ok(sequence)=>*sequence,
            Err(error)=>match head::read(&*self._owner.io).await {
                Ok(Some(current)) if current.sequence>self.sequence && current.head==head=>current.sequence,
                _=>return Err(error.clone()),
            },
        };
        self.generation_roots.insert(generation,(reference.clone(),roots));
        self.head=Some(head);self.sequence=sequence;
        if published.is_ok() {self.loaded=Some(reference);self.compaction_fallback=None;}
        self.programs.insert(generation,programs.clone());
        let previous=self.head.as_ref().and_then(|head|head.previous.as_ref()).map(|previous|previous.id);
        self.programs.retain(|id,_|*id==generation || Some(*id)==previous);
        published?;
        self.collect_garbage().await?;Ok(generation)
    }
    async fn store_inputs(&mut self,inputs:Vec<StoreInput>,cancelled:&AtomicBool)->Result<StoredInputs,String> {
        self.seal_compaction_output();
        self.validate_layout().await?;
        if io::marker(&*self._owner.io,".retired").await? {return Err("This drawing's session copy was retired".into());}
        let mut owners=BTreeSet::new();let mut bytes=0u64;
        for input in &inputs {
            if self.existing(input)?.is_none() && owners.insert(input.owner_key()) {bytes=bytes.checked_add(input.length.max(1)).ok_or("Drawing storage size overflow")?;}
        }
        let reservation=if bytes==0 {None}else {Some(super::storage_budget::reserve(&*self._owner.io,bytes).await?)};
        let mut stored=StoredInputs {ids:Vec::with_capacity(inputs.len()),bindings:Vec::new(),reservation};
        let mut pending=Vec::new();let mut bytes=0u64;
        for input in inputs {
            check_cancelled(cancelled)?;
            if bytes>0 && bytes+input.length>PACK_BYTES {self.append_inputs(std::mem::take(&mut pending),cancelled,&mut stored).await?;bytes=0;}
            let id=input.id;
            if let Some(location)=self.existing(&input)? {self.remember(input,location,&mut stored)?;}
            else {bytes=bytes.saturating_add(input.length);pending.push(input);}
            stored.ids.push(id);
        }
        if !pending.is_empty() {self.append_inputs(pending,cancelled,&mut stored).await?;}
        Ok(stored)
    }
    fn existing(&self,input:&StoreInput)->Result<Option<Arc<StoreLocation>>,String> {
        if let Payload::Tile(tile)=&input.payload {tile.compressed.healthy()?;}
        if let Payload::Tile(tile)=&input.payload && let Some(location)=tile.compressed.location()? && Arc::ptr_eq(&location.range().pack.owner,&self._owner) {
            if location.length!=input.length || location.crc!=input.crc || location.descriptor!=input.descriptor {return Err("Conflicting immutable recovery resource descriptor".into());}
            return Ok(Some(location));
        }
        if let Some(entry)=self.index.get(&input.id) {
            if entry.location.descriptor!=input.descriptor {return Err("Conflicting immutable recovery resource descriptor".into());}
            if entry.owner.as_ref().is_none_or(|owner|std::sync::Weak::ptr_eq(owner,&input.owner)) {return Ok(Some(entry.location.clone()));}
            if entry.owner.as_ref().is_some_and(|owner|owner.strong_count()!=0) {return Err("Conflicting immutable recovery resource owner".into());}
        }
        let key=input.owner_key();
        if let Some(entry)=self.by_owner.get(&key).and_then(|id|self.index.get(id))
            && entry.owner.as_ref().is_some_and(|owner|owner.strong_count()!=0 && std::sync::Weak::ptr_eq(owner,&input.owner)) {
            if entry.location.descriptor!=input.descriptor {return Err("Conflicting aliased resource descriptors".into());}
            return Ok(Some(entry.location.clone()));
        }
        Ok(None)
    }
    async fn append_inputs(&mut self,inputs:Vec<StoreInput>,cancelled:&AtomicBool,stored:&mut StoredInputs)->Result<(),String> {
        use super::store_io::FileAccess;
        let mut first=0;let mut aliases=BTreeMap::<(u8,usize),(PortableId,u64,[u8;32])>::new();
        while first<inputs.len() {
            if aliases.contains_key(&inputs[first].owner_key()) {
                let location=self.existing(&inputs[first])?.ok_or("Missing aliased recovery location")?;
                self.remember(inputs[first].clone(),location,stored)?;first+=1;continue;
            }
            let target=if let Some((id,extent))=self.append {
                let file=self._owner.io.open(&format!("packs/{id}.bin"),FileAccess::Read).await?;
                let length=file.len().await?;
                if length<extent {return Err("Recovery pack is shorter than its committed extent".into());}
                length.checked_add(inputs[first].length.max(1)).filter(|end|*end<=PACK_BYTES).map(|_|(id,length))
            }else {None};
            let (pack,start)=target.unwrap_or_else(||(PortableId::random(),0));
            let mut length=0u64;let mut last=first;
            let mut offsets=Vec::new();
            while last<inputs.len() {
                let input=&inputs[last];let key=input.owner_key();let alias=aliases.get(&key).copied();
                let (input_pack,offset)=if let Some((pack,offset,descriptor))=alias {
                    if descriptor!=input.descriptor {return Err("Conflicting aliased resource descriptors".into());}(pack,offset)
                }else {
                    if last>first && start.saturating_add(length).saturating_add(input.length)>PACK_BYTES {break;}
                    let offset=start+length;length=length.checked_add(input.length.max(1)).ok_or("Recovery pack length overflow")?;
                    aliases.insert(key,(pack,offset,input.descriptor));(pack,offset)
                };
                offsets.push((input_pack,offset,alias.is_none()));last+=1;
            }
            let file=self._owner.io.open(&format!("packs/{pack}.bin"),if target.is_some() {FileAccess::Write}else {FileAccess::Create}).await?;
            let written=async {for (input,&(_,offset,physical)) in inputs[first..last].iter().zip(&offsets) {
                if !physical {continue;}
                write_payload(&*file,offset,input,cancelled).await?;
                if input.length==0 {file.write_at(offset,&[0]).await?;}
            }file.flush().await?;check_cancelled(cancelled)}.await;
            let growth=file.len().await.unwrap_or(start.saturating_add(length)).saturating_sub(start).min(length);
            stored.reservation.as_mut().unwrap().written(growth)?;written?;
            let extent=start.checked_add(length).ok_or("Recovery pack extent overflow")?;
            let owner=self.pack(pack,extent).await?;let source=owner.source(&self.reads);let mut shared=BTreeMap::new();
            for (input,&(input_pack,offset,_)) in inputs[first..last].iter().zip(&offsets) {
                let location=if input_pack==pack {shared.entry(offset).or_insert_with(||Arc::new(StoreLocation::new(owner.clone(),source.clone(),offset,input.length,input.crc,input.descriptor))).clone()}
                    else {self.existing(input)?.ok_or("Missing aliased recovery location")?};
                self.remember(input.clone(),location,stored)?;
            }
            self.append=Some((pack,extent));first=last;
        }Ok(())
    }
    fn remember(&mut self,input:StoreInput,location:Arc<StoreLocation>,stored:&mut StoredInputs)->Result<(),String> {
        if let Payload::Tile(tile)=&input.payload && tile.compressed.needs_store()? {stored.bindings.push((tile.clone(),location.clone()));}
        self.by_owner.insert(input.owner_key(),input.id);
        self.index.insert(input.id,IndexedResource {record:input.record,location,owner:Some(input.owner)});Ok(())
    }
    pub async fn copy_package_step(&mut self,copy:&mut PackageCopy,cancelled:&AtomicBool,checkpoint:bool,activity:&CompactionActivity)->Result<(),String> {
        let epoch=activity.revision();
        let Some(_permit)=activity.chunk(checkpoint) else {return Ok(());};
        self.copy_package_ready(copy,cancelled,activity,epoch).await
    }
    async fn copy_package_ready(&mut self,copy:&mut PackageCopy,cancelled:&AtomicBool,activity:&CompactionActivity,epoch:u64)->Result<(),String> {
        let mut bytes=0;let mut next=copy.next;let mut inputs=Vec::new();
        while let Some(blob)=copy.tiles.get(next) {
            if activity.revision()!=epoch {break;}
            if !blob.compressed.needs_store()? {next+=1;continue;}
            let bound=if blob.compressed.needs_preparation() {crate::raster::TileBlob::max_compressed_len(blob.descriptor).ok_or("Invalid package tile descriptor")?}
                else {blob.compressed_len()} as u64;
            if bytes>0 && bytes+bound>compaction::CHUNK_BYTES {break;}
            if blob.compressed.needs_preparation() {blob.read_compressed().await?;}
            let entry=super::resources::ResourceEntry {kind:"capy.raster-tile/1",data:super::resources::encode_descriptor(blob.descriptor)?,raw_encoding:"capy.lz4-tile/1",payload:Payload::Tile(blob.clone())};
            inputs.push(StoreInput::entry(&entry,cancelled)?);bytes+=bound;next+=1;
        }
        if activity.revision()!=epoch {return Ok(());}
        if !inputs.is_empty() {self.store_inputs(inputs,cancelled).await?.commit()?;}
        copy.next=next;Ok(())
    }
    pub async fn make_durable(&mut self,tiles:&crate::raster_storage::RetainedTiles,cancelled:&AtomicBool)->Result<usize,String> {
        let before=tiles.resident_bytes();
        let mut inputs=Vec::new();
        for blob in tiles.await_blobs().await? {
            if !blob.compressed.needs_pressure_store()? {continue;}
            if blob.compressed.needs_preparation() {blob.read_compressed().await?;}
            let entry=super::resources::ResourceEntry {kind:"capy.raster-tile/1",data:super::resources::encode_descriptor(blob.descriptor)?,raw_encoding:"capy.lz4-tile/1",payload:Payload::Tile(blob)};
            inputs.push(StoreInput::entry(&entry,cancelled)?);
        }
        let result=match self.store_inputs(inputs,cancelled).await {Ok(stored)=>stored.commit(),Err(error)=>Err(error)};
        let released=before.saturating_sub(tiles.resident_bytes());
        if released>0 && let Some(owner)=tiles.owner.as_ref().and_then(Weak::upgrade) {owner.refresh();}
        result.map(|_|released)
    }
    pub async fn import_generation(&mut self,expected:Option<PortableId>,metadata:&[u8],resources:BTreeMap<PortableId,ImmutableBacking>,cancelled:&AtomicBool)->Result<PortableId,ImportFailure> {
        self.refresh_head().await.map_err(ImportFailure::Io)?;
        if self.head.as_ref().map(|head|head.current.id)!=expected {return Err(ImportFailure::Changed);}
        let limits=ProjectLimits::for_import();
        if self.index.is_empty() && let Some(head)=self.head.clone() {
            let mut failure=None;let mut loaded=false;
            for generation in std::iter::once(head.current).chain(head.previous) {
                match self.indexed_generation(&generation,limits,cancelled).await {
                    Ok(_)=>{self.loaded=Some(generation);loaded=true;break;},Err(error)=>failure=Some(error),
                }
            }
            if !loaded {return Err(failure.unwrap().into());}
        }
        let index=resource_index(metadata,limits)?;
        let mut metadata=super::parse_json(metadata,limits.metadata_bytes.min(usize::MAX as u64) as usize).map_err(|error|ImportFailure::from(super::session::OpenFailure::from(error)))?;
        let records=metadata["resources"].as_array().ok_or_else(||ImportFailure::Invalid("Missing stored resource records".into()))?;
        if index.records.len()!=resources.len() {return Err(ImportFailure::Invalid("Incomplete imported session resources".into()));}
        let mut inputs=Vec::with_capacity(index.records.len());
        for (resource,record) in index.records.into_iter().zip(records) {
            let backing=resources.get(&resource.id).ok_or_else(||ImportFailure::Invalid("Missing imported resource".into()))?.clone();
            if backing.byte_len()!=resource.length {return Err(ImportFailure::Invalid("Imported resource length disagrees with its descriptor".into()));}
            let owner=backing.weak_owner();
            let opaque=Arc::new(crate::authored::OpaqueResource::from_record(resource.id,record,backing,0,resource.length,resource.crc).map_err(ImportFailure::Invalid)?);
            let mut input=StoreInput::opaque(opaque).map_err(ImportFailure::Invalid)?;input.owner=owner;inputs.push(input);
        }
        let stored=self.store_inputs(inputs,cancelled).await.map_err(|error|if error==RESOURCE_CHECKSUM_FAILED {ImportFailure::Invalid(error)}else {ImportFailure::Io(error)})?;
        let extents=self.pack_extents(&stored.ids).await.map_err(ImportFailure::Io)?;
        metadata["resources"]=serde_json::to_value(self.resource_values(&stored.ids).map_err(ImportFailure::Invalid)?).map_err(|error|ImportFailure::Invalid(error.to_string()))?;
        let metadata=serde_json::to_vec(&PackedGeneration {format:"capy.session.packed".into(),version:2,session:metadata,extents:extents.clone()}).map_err(|error|ImportFailure::Invalid(error.to_string()))?;
        let roots=GenerationRoots {identities:stored.ids.iter().copied().collect(),packs:extents.into_keys().collect(),..Default::default()};
        let generation=self.publish_packed(metadata,roots,&super::TrustedPrograms::default(),cancelled).await.map_err(ImportFailure::Io)?;
        stored.commit().map_err(ImportFailure::Io)?;Ok(generation)
    }
    fn recover_append(&mut self,extents:&BTreeMap<PortableId,u64>) {
        self.append=extents.iter().find(|(_,extent)|**extent<PACK_BYTES).map(|(&id,&extent)|(id,extent));
    }
    async fn pack(&mut self,id:PortableId,extent:u64)->Result<Arc<Pack>,String> {
        let existing=self._owner.packs.lock().map_err(|_|"Recovery pack lock failed")?.get(&id).and_then(Weak::upgrade);
        let pack=if let Some(pack)=existing {pack}else {
            let file=self._owner.io.open(&format!("packs/{id}.bin"),super::store_io::FileAccess::Read).await?;
            let pack=Arc::new(Pack {id,file,owner:self._owner.clone(),source:Default::default(),extent:AtomicU64::new(extent)});
            self._owner.packs.lock().map_err(|_|"Recovery pack lock failed")?.insert(id,Arc::downgrade(&pack));pack
        };
        let length=pack.file.len().await?;
        if length<extent {return Err("Recovery pack is shorter than its committed extent".into());}
        pack.extent.fetch_max(length,Ordering::Release);Ok(pack)
    }

    pub async fn load(&mut self, limits: ProjectLimits, cancelled: &AtomicBool) -> Result<Option<OpenSession>, String> {
        self.load_with_budget(limits,cancelled,crate::MemoryBudget::process()).await
    }
    pub async fn load_with_budget(&mut self,limits:ProjectLimits,cancelled:&AtomicBool,budget:crate::MemoryBudget)->Result<Option<OpenSession>,String> {
        check_cancelled(cancelled)?;
        self.validate_layout().await?;
        if io::marker(&*self._owner.io,".retired").await? { return Err("This drawing was closed and its session copy was retired".into()); }
        self.refresh_head().await?;
        let Some(head) = self.head.clone() else { return Ok(None) };
        let mut failure = None;
        for generation in std::iter::once(head.current).chain(head.previous) {
            check_cancelled(cancelled)?;
            match self.load_generation(&generation, limits, cancelled,budget.clone()).await {
                Ok(session) => { self.loaded = Some(generation); return Ok(Some(session)); }
                Err(super::session::OpenFailure::Unsupported(message)) => return Err(message),
                Err(super::session::OpenFailure::MemoryBudgetExceeded) => return Err(crate::MEMORY_BUDGET_EXCEEDED.into()),
                Err(error) => { if failure.is_none() { failure = Some(error.message()); } }
            }
        }
        Err(failure.unwrap_or_else(|| "Recovery checkpoint unavailable".into()))
    }

    async fn generation_metadata(&self,generation:&Generation,limit:u64)->Result<Vec<u8>,String> {
        let bytes=io::read_optional(&*self._owner.io,&format!("generations/{}.json",generation.id),limit).await?.ok_or("Recovery generation is missing")?;
        if Generation::new(generation.id,&bytes)!=*generation {return Err("Recovery generation checksum failed".into());}Ok(bytes)
    }
    async fn indexed_generation(&mut self,generation:&Generation,limits:ProjectLimits,cancelled:&AtomicBool)->Result<(StoredGeneration,StoreResources,serde_json::Value),super::session::OpenFailure> {
        let metadata=self.generation_metadata(generation,limits.metadata_bytes).await?;
        let mut stored=StoredGeneration::parse(&metadata,limits)?;
        let index=resource_index(&stored.metadata,limits)?;
        if stored.locations.is_none() {
            let value:serde_json::Value=serde_json::from_slice(&stored.metadata).map_err(|error|error.to_string())?;
            let values=value["resources"].as_array().ok_or("Missing legacy resource records")?;
            let mut inputs=Vec::new();
            for (record,value) in index.records.iter().zip(values) {
                check_cancelled(cancelled)?;
                let file=self._owner.io.open(&format!("resources/{}.bin",record.id),super::store_io::FileAccess::Read).await?;
                if file.len().await?!=record.length {return Err("Recovery resource length changed".into());}
                let source=self.reads.source(record.length,file);
                let backing=ImmutableBacking::new(Arc::new(source))?;
                let resource=Arc::new(crate::authored::OpaqueResource::from_record(record.id,value,backing,0,record.length,record.crc)?);
                inputs.push(StoreInput::opaque(resource)?);
            }
            let records=self.store_inputs(inputs,cancelled).await?;
            let mut value=value;value["resources"]=self.resource_values(&records.ids)?.into();records.commit()?;
            stored.metadata=serde_json::to_vec(&value).map_err(|error|error.to_string())?;
            let mut locations=BTreeMap::new();
            for record in value["resources"].as_array().unwrap() {
                let id:PortableId=record["id"].as_str().unwrap().parse().map_err(str::to_string)?;
                let location=self.index[&id].location.range();
                stored.extents.insert(location.pack.id,location.pack.extent.load(Ordering::Acquire));
                locations.insert(id,PackRange {pack:location.pack.id,offset:location.offset});
            }
            stored.locations=Some(locations);
        }
        let (resources,value)=self.index_generation_parts(&stored,cancelled).await?;
        self.recover_append(&stored.extents);
        Ok((stored,resources,value))
    }
    async fn index_generation_parts(&mut self,stored:&StoredGeneration,cancelled:&AtomicBool)->Result<(StoreResources,serde_json::Value),super::session::OpenFailure> {
        let limits=ProjectLimits::default();
        let mut locations=BTreeMap::new();let mut extents=BTreeMap::new();
        let mut packs=BTreeMap::new();
        for (&id,&extent) in &stored.extents {packs.insert(id,self.pack(id,extent).await?);}
        let value:serde_json::Value=serde_json::from_slice(&stored.metadata).map_err(|error|error.to_string())?;
        let records=resource_index(&stored.metadata,limits)?.records;
        for (record,value) in records.iter().zip(value["resources"].as_array().unwrap()) {
            check_cancelled(cancelled)?;
            let range=stored.locations.as_ref().unwrap().get(&record.id).ok_or("Missing recovery pack location")?;
            let pack=packs[&range.pack].clone();
            let location=self.index.get(&record.id).filter(|entry|entry.location.descriptor==record.descriptor).map(|entry|entry.location.clone())
                .unwrap_or_else(||Arc::new(StoreLocation::new(pack.clone(),pack.source(&self.reads),range.offset,record.length,record.crc,record.descriptor)));
            let stored_record=StoredRecord::parse(value.clone())?;
            self.index.entry(record.id).or_insert_with(||IndexedResource {record:stored_record,location:location.clone(),owner:None});
            let range=location.range();extents.insert(range.pack.id,range.pack.extent.load(Ordering::Acquire));
            locations.insert(record.id,location);
        }
        let resources=StoreResources {locations,extents,..Default::default()};
        Ok((resources,value))
    }
    async fn load_generation(&mut self,generation:&Generation,limits:ProjectLimits,cancelled:&AtomicBool,budget:crate::MemoryBudget)->Result<OpenSession,super::session::OpenFailure> {
        let (stored,mut resources,value)=self.indexed_generation(generation,limits,cancelled).await?;
        resources.prepare(&value,&budget).await?;
        let programs=self.programs.get(&generation.id).cloned().unwrap_or_default();
        let session=loop {
            match super::session::open_stored_parts(&stored.metadata,&resources,limits,cancelled,&programs,budget.clone()) {
                Err(super::session::OpenFailure::Uncovered)=>if let Some(admission)=&resources.admission {admission.covering().await;},
                session=>break session?,
            }
        };
        resources.verify_pending().await?;Ok(session)
    }

    pub async fn retire(&mut self)->Result<(),String> {
        self.validate_layout().await?;
        if !io::marker(&*self._owner.io,".retired").await? {io::create(&*self._owner.io,".retired",b"",&AtomicBool::new(false)).await?;}
        for name in [".retiring","head.0","head.1","head.json"] {super::storage_budget::remove(&*self._owner.io,name).await?;}
        self.head=None;self.loaded=None;self.programs.clear();self.compaction=None;self.compaction_fallback=None;self.compaction_refusal=None;self.collect_garbage().await
    }
    pub async fn prepare_retirement(&mut self)->Result<(),String> {
        if io::marker(&*self._owner.io,".retired").await? {return Err("This drawing's session copy was retired".into());}
        if !io::marker(&*self._owner.io,".retiring").await? {io::create(&*self._owner.io,".retiring",b"retiring\n",&AtomicBool::new(false)).await?;}
        Ok(())
    }
    pub async fn collect_garbage(&mut self) -> Result<(), String> {
        self.validate_layout().await?;
        self.refresh_head().await?;
        let roots=self.resource_roots().await?;self.collect_roots(roots).await
    }
    async fn resource_roots(&mut self)->Result<StoreRoots,String> {
        let mut roots=StoreRoots {generations:BTreeSet::new(),resources:BTreeSet::new(),packs:BTreeSet::new(),identities:BTreeSet::new()};
        for head in self.head.iter().chain(self.compaction_fallback.iter()).cloned() {
            for generation in std::iter::once(head.current).chain(head.previous) {
                if !roots.generations.insert(generation.id) {continue;}
                let metadata=self.generation_metadata(&generation,ProjectLimits::default().metadata_bytes).await?;
                if self.generation_roots.get(&generation.id).is_none_or(|(known,_)|*known!=generation) {
                    let stored=StoredGeneration::parse(&metadata,ProjectLimits::default()).map_err(|error|error.message())?;
                    let mut current=GenerationRoots::default();
                    if let Some(locations)=stored.locations {current.identities.extend(locations.into_keys());current.packs.extend(stored.extents.into_keys());}
                    else {current.resources.extend(resource_index(&stored.metadata,ProjectLimits::default())?.records.into_iter().map(|record|record.id));current.identities.extend(&current.resources);}
                    self.generation_roots.insert(generation.id,(generation.clone(),current));
                }
                let current=&self.generation_roots[&generation.id].1;
                roots.resources.extend(&current.resources);roots.packs.extend(&current.packs);roots.identities.extend(&current.identities);
            }
        }
        self.generation_roots.retain(|id,_|roots.generations.contains(id));Ok(roots)
    }
    fn retain_index(&mut self,identities:&BTreeSet<PortableId>) {
        let mut references=BTreeMap::new();
        for entry in self.index.values() {*references.entry(Arc::as_ptr(&entry.location)).or_insert(0usize)+=1;}
        self.index.retain(|id,entry|identities.contains(id) || entry.owner.as_ref().is_some_and(|owner|owner.strong_count()!=0)
            || Arc::strong_count(&entry.location)>references[&Arc::as_ptr(&entry.location)]);
        self.by_owner.retain(|_,id|self.index.contains_key(id));
    }
    async fn collect_roots(&mut self,roots:StoreRoots)->Result<(),String> {
        let StoreRoots {generations,resources,mut packs,identities}=roots;
        self.retain_index(&identities);
        for (&id,weak) in self._owner.packs.lock().map_err(|_|"Recovery pack lock failed")?.iter() {
            if weak.strong_count()!=0 {packs.insert(id);}
        }
        if self.append.is_some_and(|(id,_)|!packs.contains(&id)) {self.append=None;}
        let terminal=io::marker(&*self._owner.io,".retired").await?;
        for (name,retained,suffix) in [("generations",&generations,".json"),("resources",&resources,".bin"),("packs",&packs,".bin")] {
            io::clean(&*self._owner.io,name,retained,suffix).await?;
            if terminal && retained.is_empty() && self._owner.io.list(name).await?.is_empty() {super::storage_budget::remove(&*self._owner.io,name).await?;}
        }
        io::clean(&*self._owner.io,"",&BTreeSet::new(),".unused").await?;
        self._owner.packs.lock().map_err(|_|"Recovery pack lock failed")?.retain(|_,pack|pack.strong_count()!=0);Ok(())
    }
    async fn validate_layout(&self)->Result<(),String> {
        for entry in self._owner.io.list("").await? {
            if ["resources","packs","generations"].contains(&entry.name.as_str()) && !entry.directory {return Err("Invalid drawing session directory".into());}
            if ["head.0","head.1","head.json",".retired",".retiring",".lock"].contains(&entry.name.as_str()) && entry.directory {return Err("Invalid drawing session member".into());}
        }Ok(())
    }
    async fn refresh_head(&mut self)->Result<(),String> {
        let (head,sequence)=match head::read(&*self._owner.io).await? {
            Some(published)=>(Some(published.head),published.sequence),
            None=>(io::read_optional(&*self._owner.io,"head.json",1024).await?.map(|bytes|parse_head(&bytes)).transpose()?,0),
        };
        if head!=self.head {self.loaded=None;self.head=head;}self.sequence=sequence;Ok(())
    }
}

#[derive(Clone)]
struct ResourceRecord { id: PortableId, offset: u64, length: u64, crc: u32, descriptor: [u8; 32] }
struct ResourceIndex {records:Vec<ResourceRecord>}
fn resource_index(metadata: &[u8], limits: ProjectLimits) -> Result<ResourceIndex, super::session::OpenFailure> {
    if metadata.len() as u64 > limits.metadata_bytes { return Err("Recovery metadata exceeds admission limit".into()); }
    let value = super::parse_json(metadata, limits.metadata_bytes.min(usize::MAX as u64) as usize)?;
    super::session::validate_format(&value)?;
    let records = value["resources"].as_array().ok_or("Missing recovery resource inventory")?;
    if records.len() > 262_144 { return Err("Recovery resource count exceeds admission limit".into()); }
    let mut ids = BTreeSet::new();
    let mut ranges = BTreeMap::<(String,u64), (u64, u32, PortableId, [u8; 32])>::new();
    let mut legacy=true;
    let mut output = Vec::with_capacity(records.len());
    for record in records {
        let id: PortableId = record["id"].as_str().ok_or("Invalid recovery resource identity")?.parse().map_err(str::to_string)?;
        if !ids.insert(id) { return Err("Duplicate recovery resource identity".into()); }
        let pack=record["location"]["pack"].as_str().ok_or("Invalid recovery resource pack")?;
        if pack!="data/tiles-1.bin" {parse_pack(pack)?;legacy=false;}
        let offset = super::manifest::decimal_u64(&record["location"]["offset"])?;
        let length = super::manifest::decimal_u64(&record["bytes"])?;
        let checksum = record["crc32"].as_str().ok_or("Invalid recovery checksum")?;
        if checksum.len() != 8 || !checksum.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return Err("Invalid recovery checksum".into()); }
        let crc = u32::from_str_radix(checksum, 16).map_err(|e| e.to_string())?;
        let descriptor = resource_descriptor(record)?;
        if length == 0 {
            if crc != 0 { return Err("Invalid empty recovery resource checksum".into()); }
            output.push(ResourceRecord { id, offset, length, crc, descriptor });
            continue;
        }
        if let Some((previous_length, previous_crc, _, previous_descriptor)) = ranges.get(&(pack.to_owned(),offset)) {
            if *previous_length != length || *previous_crc != crc || previous_descriptor != &descriptor { return Err("Conflicting recovery resource aliases".into()); }
        } else { ranges.insert((pack.to_owned(),offset), (length, crc, id, descriptor)); }
        output.push(ResourceRecord { id, offset, length, crc, descriptor });
    }
    let mut length = 0u64;
    let mut ends=BTreeMap::new();
    for ((pack,offset),(count,_,_,_)) in ranges {
        let end=ends.entry(pack).or_insert(0u64);
        if offset<*end || (legacy && offset!=*end) {return Err("Recovery resource pack has overlapping or missing ranges".into());}
        *end=offset.checked_add(count).ok_or("Recovery resource range overflow")?;
        length = length.checked_add(count).ok_or("Recovery resource pack overflow")?;
        let budget = limits.raster_bytes.saturating_add(limits.asset_bytes).saturating_add(512 * 1024 * 1024).saturating_mul(2);
        if length > budget { return Err("Recovery resource pack exceeds admission limit".into()); }
    }
    if legacy && output.iter().any(|record| record.offset.checked_add(record.length).is_none_or(|end| end > length)) {
        return Err("Recovery resource range exceeds pack".into());
    }
    Ok(ResourceIndex {records:output})
}

fn resource_descriptor(record: &serde_json::Value) -> Result<[u8; 32], String> {
    let mut descriptor = record.clone();
    let fields = descriptor.as_object_mut().ok_or("Invalid recovery resource descriptor")?;
    fields.remove("id"); fields.remove("location");
    Ok(Sha256::digest(serde_json::to_vec(&descriptor).map_err(|e| e.to_string())?).into())
}

struct Pack {
    id:PortableId,
    file:Arc<dyn super::store_io::StoreFile>,
    owner:Arc<StoreOwner>,
    source:Mutex<crate::tile_io::WeakReadSource>,
    extent:AtomicU64,
}
impl Pack {
    fn source(self:&Arc<Self>,cache:&ReadCache)->ReadSource {
        let mut current=self.source.lock().unwrap();
        let source=current.upgrade().unwrap_or_else(||cache.source(self.extent.load(Ordering::Acquire),self.clone()));
        source.extend_to(self.extent.load(Ordering::Acquire));*current=source.downgrade();source
    }
}
impl RangeReader for Pack {
    fn transfer_key(&self)->Option<String> {self.file.transfer_key()}
    fn allocation_bytes(&self,offset:u64,length:usize)->usize {self.file.allocation_bytes(offset,length)}
    fn read(&self,offset:u64,length:usize,crc:Option<u32>,completion:ReadCompletion) {self.file.read(offset,length,crc,completion);}
}
struct StoreRange {pack:Arc<Pack>,source:ReadSource,offset:u64}
pub(crate) fn tile_fingerprint(id:PortableId,descriptor:crate::color::PixelDescriptor,length:u64,crc:u32)->Result<[u8;32],String> {StoredRecord::tile(id,descriptor)?.fingerprint(length,crc)}
pub(crate) fn validate_tile_identity(id:PortableId,descriptor:crate::color::PixelDescriptor,bytes:&[u8],length:u64,crc:u32,digest:[u8;32])->Result<(),String> {
    if bytes.len() as u64!=length || crc32fast::hash(bytes)!=crc || tile_fingerprint(id,descriptor,length,crc)?!=digest {
        return Err("Conflicting immutable stored tile identity".into());
    }Ok(())
}
pub(crate) struct StoreLocation {
    range:Mutex<Arc<StoreRange>>,
    pub(crate) length:u64,
    pub(crate) crc:u32,
    descriptor:[u8;32],
}
impl StoreLocation {
    pub(crate) fn transfer_range(&self)->Result<Option<(ImmutableBacking,u64,crate::authored::EncodedIntegrity)>,String> {
        let range=self.range();
        if range.pack.file.transfer_key().is_none() {return Ok(None);}
        let backing=ImmutableBacking::with_generation(Arc::new(range.source.clone()),range.pack.id)?;
        Ok(Some((backing,range.offset,crate::authored::EncodedIntegrity {bytes:self.length,crc32:self.crc})))
    }
    pub(crate) fn content_identity(&self)->crate::raster::TileContentIdentity {
        crate::raster::TileContentIdentity::Stored {bytes:self.length,crc32:self.crc,descriptor:self.descriptor}
    }
    fn new(pack:Arc<Pack>,source:ReadSource,offset:u64,length:u64,crc:u32,descriptor:[u8;32])->Self {
        Self {range:Mutex::new(Arc::new(StoreRange {pack,source,offset})),length,crc,descriptor}
    }
    fn range(&self)->Arc<StoreRange> {self.range.lock().unwrap().clone()}
    async fn read_all(&self)->Result<Arc<[u8]>,String> {
        let range=self.range();
        let length=usize::try_from(self.length).map_err(|_|"Stored resource exceeds address space")?;
        let mut bytes=Vec::new();bytes.try_reserve_exact(length).map_err(|_|"Stored resource allocation failed")?;
        let mut checksum=crc32fast::Hasher::new();
        while bytes.len()<length {
            let count=(length-bytes.len()).min(MAX_RANGE_BYTES);
            let chunk=range.source.wait(range.offset+bytes.len() as u64,count)?.await?;
            checksum.update(&chunk);bytes.extend_from_slice(&chunk);
        }
        if checksum.finalize()!=self.crc {return Err("Recovery resource checksum failed".into());}Ok(bytes.into())
    }
    async fn verify(&self)->Result<(),String> {
        let range=self.range();let mut offset=0;let mut checksum=crc32fast::Hasher::new();
        while offset<self.length {
            let count=(self.length-offset).min(MAX_RANGE_BYTES as u64) as usize;
            let bytes=range.source.wait(range.offset+offset,count)?.await?;checksum.update(&bytes);offset+=count as u64;
        }
        if checksum.finalize()!=self.crc {return Err("Recovery resource checksum failed".into());}Ok(())
    }
    pub(crate) fn read_interest(&self)->Result<crate::tile_io::ReadWait,String> {let range=self.range();range.source.wait_checked(range.offset,self.length as usize,self.crc)}
    pub(crate) async fn read(&self)->Result<ByteRange,String> {self.read_interest()?.await}
    pub(crate) fn poll(&self)->Result<RangeState,String> {let range=self.range();range.source.poll_checked(range.offset,self.length as usize,self.crc)}
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn read_on_worker(&self)->Result<ByteRange,String> {let range=self.range();range.source.read_checked_on_worker(range.offset,self.length as usize,self.crc)}
    pub(crate) fn backing(self:&Arc<Self>)->Result<ImmutableBacking,String> {ImmutableBacking::new(self.clone()).map_err(str::to_string)}
}
impl ByteSource for StoreLocation {
    fn byte_len(&self)->u64 {self.length}
    fn resident_bytes(&self)->usize {0}
    fn poll(&self,offset:u64,length:usize)->Result<RangeState,String> {
        if offset.checked_add(length as u64).is_none_or(|end|end>self.length) {return Err("Stored resource range exceeds extent".into());}
        let range=self.range();range.source.poll(range.offset+offset,length)
    }
    fn wait(&self,offset:u64,length:usize)->super::store_io::FileFuture<'_,ByteRange> {
        Box::pin(async move {
            if offset.checked_add(length as u64).is_none_or(|end|end>self.length) {return Err("Stored resource range exceeds extent".into());}
            let range=self.range();range.source.wait(range.offset+offset,length)?.await
        })
    }
    #[cfg(not(target_arch="wasm32"))]
    fn poll_on_worker(&self,offset:u64,length:usize)->Result<RangeState,String> {
        if offset.checked_add(length as u64).is_none_or(|end|end>self.length) {return Err("Stored resource range exceeds extent".into());}
        let range=self.range();range.source.wait_on_worker(range.offset+offset,length).map(RangeState::Ready)
    }
}
#[derive(Default)]
pub(crate) struct StoreResources {
    pub(crate) locations:BTreeMap<PortableId,Arc<StoreLocation>>,
    extents:BTreeMap<PortableId,u64>,
    pub(crate) loaded:BTreeMap<PortableId,Arc<[u8]>>,
    pub(crate) admission:Option<crate::MemoryReservation>,
    opaque:Mutex<BTreeSet<PortableId>>,
}
impl StoreResources {
    pub(crate) fn verify_opaque(&self,id:PortableId) {if !self.loaded.contains_key(&id) {self.opaque.lock().unwrap().insert(id);}}
    async fn prepare(&mut self,metadata:&serde_json::Value,budget:&crate::MemoryBudget)->Result<(),super::session::OpenFailure> {
        let mut pending=Vec::new();let mut total=0u64;
        for record in metadata["resources"].as_array().ok_or("Missing stored resource records")? {
            if record["type"]=="capy.raster-tile/1" && record["encoding"]=="capy.lz4-tile/1" {continue;}
            if super::resources::stored_decode_bound(record)?.is_none() {continue;}
            let id:PortableId=record["id"].as_str().ok_or("Missing stored resource identity")?.parse().map_err(str::to_string)?;
            let location=self.locations.get(&id).ok_or("Missing stored resource location")?;
            total=total.checked_add(location.length).ok_or("Stored resource size overflow")?;pending.push(id);
        }
        let admission=budget.admit_import(total).ok_or(super::session::OpenFailure::MemoryBudgetExceeded)?;
        admission.covering().await;self.admission=Some(admission);
        for id in pending {
            let bytes=self.locations.get(&id).ok_or("Missing stored resource location")?.read_all().await?;
            self.loaded.insert(id,bytes);
        }Ok(())
    }
    async fn verify_pending(&self)->Result<(),String> {
        let ids=std::mem::take(&mut *self.opaque.lock().unwrap());
        for id in ids {self.locations.get(&id).ok_or("Missing opaque resource location")?.verify().await?;}Ok(())
    }
    pub(crate) fn manifest_records(&self,records:&mut [serde_json::Value])->Result<(),String> {
        let packs=self.extents.keys().enumerate().map(|(index,&pack)|(pack,format!("data/tiles-{}.bin",index+1))).collect::<BTreeMap<_,_>>();
        for record in records {
            let id:PortableId=record["id"].as_str().ok_or("Missing stored resource identity")?.parse().map_err(str::to_string)?;
            let location=self.locations.get(&id).ok_or("Missing stored resource location")?;
            let range=location.range();record["location"]["pack"]=packs[&range.pack.id].clone().into();
            record["location"]["offset"]=range.offset.to_string().into();
        }Ok(())
    }
    pub(crate) fn directory(&self)->super::archive::Directory {
        super::archive::Directory {members:self.extents.values().enumerate().map(|(index,&length)|super::archive::Member {
            name:format!("data/tiles-{}.bin",index+1),offset:0,length,compressed_length:None,crc32:0,
        }).collect(),length:self.extents.values().copied().sum()}
    }
}
async fn write_payload(file:&dyn super::store_io::StoreFile,offset:u64,input:&StoreInput,cancelled:&AtomicBool)->Result<(),String> {
    check_cancelled(cancelled)?;
    if let Payload::Tile(tile)=&input.payload {
        let integrity=tile.compressed.integrity(cancelled)?;
        if integrity.bytes!=input.length || integrity.crc32!=input.crc {return Err("Recovery tile integrity changed".into());}
        let bytes=tile.read_compressed().await?;
        for (index,chunk) in bytes.chunks(MAX_RANGE_BYTES).enumerate() {check_cancelled(cancelled)?;file.write_at(offset+(index*MAX_RANGE_BYTES) as u64,chunk).await?;}
    }else if let Payload::Opaque(resource)=&input.payload {
        let mut checksum=crc32fast::Hasher::new();let mut position=0;
        while position<input.length {
            check_cancelled(cancelled)?;let count=(input.length-position).min(MAX_RANGE_BYTES as u64) as usize;
            let bytes=resource.read_chunk_async(position,count,cancelled).await.map_err(|error|error.to_string())?;
            checksum.update(&bytes);file.write_at(offset+position,&bytes).await?;position+=count as u64;
        }
        if checksum.finalize()!=input.crc {return Err(RESOURCE_CHECKSUM_FAILED.into());}
    }else {
        let (_,integrity)=input.payload.prepared(cancelled)?;
        if integrity.bytes!=input.length || integrity.crc32!=input.crc {return Err("Recovery resource integrity changed".into());}
        let bytes=input.payload.encoded()?.bytes;
        for (index,chunk) in bytes.chunks(MAX_RANGE_BYTES).enumerate() {check_cancelled(cancelled)?;file.write_at(offset+(index*MAX_RANGE_BYTES) as u64,chunk).await?;}
    }Ok(())
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) { Err("Recovery operation cancelled".into()) } else { Ok(()) }
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod live_tests;

#[cfg(test)]
mod writer_tests;

#[cfg(all(test,not(target_arch="wasm32")))]
mod compaction_tests;
