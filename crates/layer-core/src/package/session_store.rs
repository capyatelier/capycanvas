use super::{ByteRange, ByteSource, ImmutableBacking, RangeState, MAX_RANGE_BYTES,
    resources::Payload, session::OpenSession};
use crate::{PortableId, ProjectLimits};
use crate::tile_io::{ReadCache, ReadSource, RangeReader, ReadCompletion, read_buffer, read_exact_at};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Write}, path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicBool, AtomicU64, Ordering}}};

#[derive(Debug)]
pub struct AtomicReplaceError { pub published: bool, pub error: String }
impl From<String> for AtomicReplaceError {
    fn from(error: String) -> Self { Self { published: false, error } }
}
impl From<&str> for AtomicReplaceError {
    fn from(error: &str) -> Self { error.to_string().into() }
}

pub struct OwnerLock { _file:File, packs:Mutex<BTreeMap<PortableId,Weak<Pack>>> }
impl OwnerLock {
    pub fn claim(path: &Path) -> Result<Option<Self>, String> {
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        let file = options.open(path).map_err(|e| e.to_string())?;
        #[cfg(target_os = "android")]
        let owned = {
            use std::os::fd::AsRawFd;
            unsafe extern "C" { fn flock(fd: i32, operation: i32) -> i32; }
            loop {
                if unsafe { flock(file.as_raw_fd(), 2 | 4) } == 0 { break true; }
                let error = std::io::Error::last_os_error();
                match error.kind() {
                    std::io::ErrorKind::Interrupted => continue,
                    std::io::ErrorKind::WouldBlock => break false,
                    _ => return Err(error.to_string()),
                }
            }
        };
        #[cfg(not(target_os = "android"))]
        let owned = match file.try_lock() {
            Ok(()) => true,
            Err(fs::TryLockError::WouldBlock) => false,
            Err(fs::TryLockError::Error(error)) => return Err(error.to_string()),
        };
        if !owned {return Ok(None);}
        Ok(Some(Self { _file:file,packs:Default::default() }))
    }
}
impl Drop for OwnerLock {
    fn drop(&mut self) {
        #[cfg(target_os="android")]
        {
            use std::os::fd::AsRawFd;
            unsafe extern "C" {fn flock(fd:i32,operation:i32)->i32;}
            loop {
                if unsafe {flock(self._file.as_raw_fd(),8)}==0 || std::io::Error::last_os_error().kind()!=std::io::ErrorKind::Interrupted {break;}
            }
        }
        #[cfg(not(target_os="android"))]
        loop {match self._file.unlock() {Err(error) if error.kind()==std::io::ErrorKind::Interrupted=>continue,_=>break}}
    }
}

pub struct SessionLease { _owner: OwnerLock }
pub fn packed_sessions(root: &Path) -> Result<PathBuf, String> {
    let packed = root.join("packed");
    if metadata_optional(&packed.join(".lock"))?.is_some() || metadata_optional(&packed.join("owner.lock"))?.is_some() {
        return Err("The packed session directory conflicts with an existing session; its files were kept".into());
    }
    create_directory(&packed)?;
    Ok(packed)
}
fn relocate_session<Owner>(source: &Path, destination: &Path, _owner: Owner) -> Result<(), String> {
    let parent = source.parent().ok_or("Session has no parent directory")?;
    let target = destination.parent().ok_or("Session has no destination directory")?;
    if metadata_optional(destination)?.is_some() { return Err("The saved session exists in both versions; both copies were kept".into()); }
    if !metadata_optional(source)?.is_some_and(|metadata| metadata.is_dir()) { return Err("Invalid session source directory".into()); }
    let mut stores = Vec::new();
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() { continue; }
        let lock = entry.path().join(".lock");
        let Some(metadata) = metadata_optional(&lock)? else { continue; };
        if !metadata.is_file() { return Err("Invalid recovery drawing lock".into()); }
        let owner = OwnerLock::claim(&lock)?.ok_or("Recovery drawing is already open")?;
        stores.push(owner);
    }
    #[cfg(windows)] drop((stores, _owner));
    fs::rename(source, destination).map_err(|e| e.to_string())?;
    sync_directory(target)?;
    sync_directory(parent)
}
pub fn import_session_directories(root: &Path, manifest: &str, lock: &str) -> Result<PathBuf, String> {
    import_session_directories_with_owner(root, manifest, lock, OwnerLock::claim)
}
pub fn import_session_directories_with_owner<Owner>(root: &Path, manifest: &str, lock: &str, claim: impl FnMut(&Path) -> Result<Option<Owner>, String>) -> Result<PathBuf, String> {
    import_with_owner_using(root, manifest, lock, claim, |_| Ok(()))
}
#[cfg(test)]
fn import_sessions_using(root: &Path, manifest: &str, lock: &str, boundary: impl FnMut(&str) -> Result<(), String>) -> Result<PathBuf, String> {
    import_with_owner_using(root, manifest, lock, OwnerLock::claim, boundary)
}
fn import_with_owner_using<Owner>(root: &Path, manifest: &str, lock: &str, mut claim: impl FnMut(&Path) -> Result<Option<Owner>, String>, mut boundary: impl FnMut(&str) -> Result<(), String>) -> Result<PathBuf, String> {
    if !matches!((manifest, lock), ("session.json", "owner.lock") | ("window.json", ".lock") | (".json", ".lock")) {
        return Err("Invalid native session layout".into());
    }
    let packed = packed_sessions(root)?;
    if manifest == ".json" {
        let Some(_source_gate) = OwnerLock::claim(&root.join(".gc.lock"))? else { return Err("Session cleanup is already running".into()); };
        let Some(_packed_gate) = OwnerLock::claim(&packed.join(".gc.lock"))? else { return Err("Session cleanup is already running".into()); };
        relocate_flat_sessions(root, &packed, &mut boundary)?;
        return Ok(packed);
    }
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let source = entry.path();
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir()
            || !metadata_optional(&source.join(manifest))?.is_some_and(|metadata| metadata.is_file()) { continue; }
        let Some(owner) = claim(&source.join(lock))? else { continue; };
        let destination = packed.join(entry.file_name());
        relocate_session(&source, &destination, owner)?;
    }
    Ok(packed)
}
fn relocate_flat_sessions(root: &Path, packed: &Path, boundary: &mut impl FnMut(&str) -> Result<(), String>) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?; let source = entry.path();
        if source.extension().is_none_or(|extension| extension != "json") || !entry.file_type().map_err(|e| e.to_string())?.is_file() { continue; }
        let Some(_owner) = OwnerLock::claim(&source.with_extension("lock"))? else { continue; };
        let destination = packed.join(entry.file_name());
        if metadata_optional(&destination)?.is_some() { return Err("The saved session exists in both versions; both copies were kept".into()); }
        let bytes = read_bounded(&source, 1024 * 1024)?;
        let value = super::parse_json(&bytes, 1024 * 1024)?;
        let drawings = value["drawings"].as_array().ok_or("Invalid saved drawing list")?;
        if drawings.len() > 1024 { return Err("Too many saved drawings".into()); }
        let mut keys = BTreeSet::new(); let mut moves = Vec::new(); let mut owners = Vec::new(); let mut busy = false;
        for drawing in drawings {
            let key = drawing["key"].as_str().ok_or("Invalid saved drawing key")?;
            if key.is_empty() || key.len() > 128 || !key.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') || !keys.insert(key) {
                return Err("Invalid saved drawing key".into());
            }
            let from = root.join(key); let to = packed.join(key);
            let original = metadata_optional(&from)?; let moved = metadata_optional(&to)?;
            if original.as_ref().is_some_and(|metadata| !metadata.is_dir()) || moved.as_ref().is_some_and(|metadata| !metadata.is_dir()) { return Err("Invalid saved drawing directory".into()); }
            if original.is_some() && moved.is_some() && metadata_optional(&from.join("head.json"))?.is_some() {
                return Err("The saved drawing exists in both versions; both copies were kept".into());
            }
            for path in [original.as_ref().map(|_| &from), moved.as_ref().map(|_| &to)].into_iter().flatten() {
                let Some(owner) = OwnerLock::claim(&path.join(".lock"))? else { busy = true; break; };
                owners.push(owner);
            }
            if busy { break; }
            if original.is_some() && moved.is_none() { moves.push((from, to)); }
        }
        if busy { continue; }
        #[cfg(windows)] drop(owners);
        for (from, to) in moves { relocate_session(&from, &to, ())?; boundary("store")?; }
        relocate_entry(&source, &destination)?; boundary("manifest")?;
        relocate_entry(&source.with_extension("lock"), &destination.with_extension("lock"))?; boundary("lock")?;
    }
    for entry in fs::read_dir(packed).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?; let destination = entry.path();
        if destination.extension().is_none_or(|extension| extension != "json") || !entry.file_type().map_err(|e| e.to_string())?.is_file() { continue; }
        let source = root.join(entry.file_name()); let lock = source.with_extension("lock");
        if metadata_optional(&source)?.is_some() || metadata_optional(&lock)?.is_none() { continue; }
        let Some(_owner) = OwnerLock::claim(&lock)? else { continue; };
        let target = destination.with_extension("lock");
        let _target = if metadata_optional(&target)?.is_some() { OwnerLock::claim(&target)? } else { None };
        if metadata_optional(&target)?.is_some() && _target.is_none() { continue; }
        relocate_entry(&lock, &target)?; boundary("lock")?;
    }
    Ok(())
}
fn relocate_entry(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|e| e.to_string())?;
    sync_directory(destination.parent().ok_or("Session has no destination directory")?)?;
    sync_directory(source.parent().ok_or("Session has no parent directory")?)
}
pub fn preserve_directory(path: &Path) -> Result<PathBuf, String> {
    let parent = path.parent().ok_or("Session has no parent directory")?;
    let preserved = parent.join(format!("preserved-{}", PortableId::random()));
    fs::rename(path, &preserved).map_err(|e| e.to_string())?;
    sync_directory(parent)?;
    Ok(preserved)
}
pub fn restore_error(root: &Path) -> Result<Option<String>, String> {
    let file = match File::open(root.join("restore-error.txt")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut text = String::new();
    file.take(65_536).read_to_string(&mut text).map_err(|e| e.to_string())?;
    Ok(Some(text))
}
pub fn set_restore_error(root: &Path, error: Option<&str>) -> Result<(), String> {
    let path = root.join("restore-error.txt");
    match error {
        Some(error) => atomic_replace(&path, error.as_bytes()),
        None => remove_file(&path),
    }
}
impl SessionLease {
    pub fn claim(root: &Path) -> Result<Option<Self>, String> {
        create_directory(root)?;
        let owner = OwnerLock::claim(&root.join(".lock"))?;
        sync_directory(root)?;
        Ok(owner.map(|owner| Self { _owner: owner }))
    }
}

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

const PACK_BYTES: u64 = 64 * 1024 * 1024;
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
use record::StoredRecord;
struct IndexedResource {record:StoredRecord,location:Arc<StoreLocation>,owner:Option<Weak<dyn Send+Sync>>}
#[derive(Clone)]
struct StoreInput {
    id:PortableId,record:StoredRecord,payload:Payload,length:u64,crc:u32,descriptor:[u8;32],owner:Weak<dyn Send+Sync>,
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
struct DrawingStoreInner {key:String,path:PathBuf,writer:Mutex<Option<SessionStore>>}
impl DrawingStore {
    pub fn new(root:&Path,key:Option<&str>)->Result<Self,String> {
        let key=key.map(str::to_owned).unwrap_or_else(||PortableId::random().to_string());
        if key.is_empty() || key.len()>128 || !key.bytes().all(|byte|byte.is_ascii_alphanumeric() || byte==b'-' || byte==b'_') {return Err("Invalid session drawing key".into());}
        Ok(Self(Arc::new(DrawingStoreInner {path:root.join(&key),key,writer:Mutex::new(None)})))
    }
    pub fn key(&self)->&str {&self.0.key}
    pub fn path(&self)->&Path {&self.0.path}
    pub fn on_worker<T>(&self,work:impl FnOnce(&mut SessionStore)->Result<T,String>)->Result<T,String> {
        let mut writer=self.0.writer.lock().map_err(|_|"Drawing writer failed")?;
        if writer.is_none() {*writer=Some(SessionStore::open(&self.0.path)?);}
        work(writer.as_mut().unwrap())
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
    _owner: Arc<OwnerLock>,
    _writer: Arc<()>,
    head: Option<Head>,
    loaded: Option<Generation>,
    index:BTreeMap<PortableId,IndexedResource>,
    by_owner:BTreeMap<(u8,usize),PortableId>,
    append:Option<(PortableId,u64)>,
    programs: BTreeMap<PortableId, super::TrustedPrograms>,
    reads: crate::tile_io::ReadCache,
    generation_roots:BTreeMap<PortableId,(Generation,GenerationRoots)>,
    published_capture:Option<PublishedCapture>,
}
#[derive(Clone,Default)]
struct GenerationRoots {resources:BTreeSet<PortableId>,packs:BTreeSet<PortableId>,identities:BTreeSet<PortableId>}
struct StoreRoots {
    generations: BTreeSet<PortableId>,
    resources: BTreeSet<PortableId>,
    packs: BTreeSet<PortableId>,
    identities: BTreeSet<PortableId>,
}
struct Ownership { owner: Weak<OwnerLock>, writer: Weak<()> }
type StoreClaim = (Arc<OwnerLock>, Arc<()>);
fn claim_store(path: &Path) -> Result<Option<StoreClaim>, String> {
    static OWNERS: OnceLock<Mutex<BTreeMap<PathBuf, Ownership>>> = OnceLock::new();
    let mut owners = OWNERS.get_or_init(Default::default).lock().map_err(|_| "Recovery ownership lock failed")?;
    owners.retain(|_, ownership| ownership.owner.strong_count() != 0);
    if owners.get(path).is_some_and(|ownership| ownership.writer.strong_count() != 0) {
        return Ok(None);
    }
    let owner = match owners.get(path).and_then(|ownership| ownership.owner.upgrade()) {
        Some(owner) => owner,
        None => match OwnerLock::claim(&path.join(".lock"))? { Some(owner) => Arc::new(owner), None => return Ok(None) },
    };
    let writer = Arc::new(());
    owners.insert(path.into(), Ownership { owner: Arc::downgrade(&owner), writer: Arc::downgrade(&writer) });
    Ok(Some((owner, writer)))
}
impl SessionStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::try_open(path)?.ok_or_else(|| "Recovery drawing is already open".into())
    }
    pub fn open_with_cache(path: &Path, reads:crate::tile_io::ReadCache) -> Result<Self,String> {
        Self::try_open_with_cache(path,reads)?.ok_or_else(||"Recovery drawing is already open".into())
    }
    pub fn try_open(path: &Path) -> Result<Option<Self>, String> {
        Self::try_open_with_cache(path,crate::tile_io::ReadCache::process())
    }
    fn try_open_with_cache(path:&Path,reads:crate::tile_io::ReadCache)->Result<Option<Self>,String> {
        create_directory(path)?;
        let path = &fs::canonicalize(path).map_err(|e| e.to_string())?;
        let Some((owner, writer)) = claim_store(path)? else { return Ok(None) };
        if retired_store(path)? { return Err("This drawing's session copy was retired".into()); }
        retiring_store(path)?;
        sync_directory(path)?;
        create_directory(&path.join("resources"))?;
        create_directory(&path.join("generations"))?;
        create_directory(&path.join("packs"))?;
        let head = read_optional(&path.join("head.json"), 1024)?.map(|bytes| parse_head(&bytes)).transpose()?;
        let mut store = Self { path: path.into(), _owner: owner, _writer: writer, head, loaded: None,
            index:BTreeMap::new(),by_owner:BTreeMap::new(),append:None,programs:BTreeMap::new(),reads,generation_roots:BTreeMap::new(),published_capture:None };
        if let Some(head)=store.head.clone() {
            for generation in std::iter::once(&head.current).chain(head.previous.as_ref()) {
                if let Ok(bytes)=store.generation_metadata(generation,ProjectLimits::default().metadata_bytes)
                    && let Ok(stored)=StoredGeneration::parse(&bytes,ProjectLimits::default()) {
                    store.recover_append(&stored.extents);break;
                }
            }
        }
        match store.resource_roots() {
            Ok(roots) => store.collect_roots(roots)?,
            Err(_) => {
                clean_temporaries(&store.path)?;
                clean_temporaries(&store.path.join("generations"))?;
                clean_temporaries(&store.path.join("resources"))?;
                clean_temporaries(&store.path.join("packs"))?;
            }
        }
        Ok(Some(store))
    }
    pub fn path(&self) -> &Path { &self.path }
    pub fn generation(&self) -> Option<PortableId> { self.head.as_ref().map(|head| head.current.id) }
    pub fn loaded_generation(&self) -> Option<PortableId> { self.loaded.as_ref().map(|loaded| loaded.id) }
    pub fn recovered_previous(&self) -> bool { self.loaded_generation().is_some_and(|loaded| Some(loaded) != self.generation()) }
    pub fn checkpoint(&mut self,capture:&super::session::EditorCapture,metadata:super::session::SessionMetadata,cancelled:&AtomicBool)->Result<PortableId,String> {
        self.checkpoint_using(capture,metadata,cancelled,AtomicPublication)
    }
    fn checkpoint_using(&mut self,capture:&super::session::EditorCapture,metadata:super::session::SessionMetadata,cancelled:&AtomicBool,mut publication:impl Publication)->Result<PortableId,String> {
        let memory=capture.artwork().memory.as_ref().map(|memory|memory.weak());
        let key=CheckpointKey::of(capture);let profiles=metadata.profiles.clone();
        let result=(|| {
        capture.validate_identity()?;self.refresh_head()?;
        if let Some(published)=&self.published_capture && published.key==key && self.loaded_generation()==Some(published.generation) && self.generation()==Some(published.generation)
            && same_profiles(&published.profiles,&metadata.profiles) {
            let generation=self.head.as_ref().unwrap().current.clone();
            let bytes=self.generation_metadata(&generation,ProjectLimits::default().metadata_bytes)?;
            let metadata=update_session_metadata(&bytes,&metadata.value)?;
            let roots=self.generation_roots.get(&generation.id).ok_or("Missing published generation roots")?.1.clone();
            let programs=self.programs.get(&generation.id).cloned().unwrap_or_default();
            return self.publish_packed(metadata,roots,&programs,cancelled,&mut publication);
        }
        let draft=super::session::SessionDraft::capture(capture,metadata,cancelled)?;
        let mut inputs=draft.inventory.entries.values().map(|entry|StoreInput::entry(entry,cancelled)).collect::<Result<Vec<_>,_>>()?;
        for opaque in &draft.opaque {inputs.push(StoreInput::opaque(opaque.clone())?);}
        let order=draft.spatial_order();
        inputs.sort_by_key(|input|order.get(&input.id).copied().unwrap_or((usize::MAX,0,0)));
        let ids=self.store_inputs(inputs,cancelled,&mut publication)?;
        let extents=self.pack_extents(&ids)?;
        let records=ids.iter().map(|id|&self.index[id]).collect::<Vec<_>>();
        let programs=draft.trusted_programs.clone();
        let metadata=draft.packed_metadata(&records,&extents)?;
        let roots=GenerationRoots {identities:ids.into_iter().collect(),packs:extents.into_keys().collect(),..Default::default()};
        self.publish_packed(metadata,roots,&programs,cancelled,&mut publication)
        })();
        if let Ok(generation)=result {self.published_capture=Some(PublishedCapture {key,profiles,generation});}
        if let Some(owner)=memory.and_then(|owner|owner.upgrade()) {owner.refresh();}
        result
    }

    fn resource_values(&self,ids:&[PortableId])->Result<Vec<serde_json::Value>,String> {
        ids.iter().map(|id|serde_json::to_value(&self.index[id]).map_err(|error|error.to_string())).collect()
    }
    fn pack_extents(&self,ids:&[PortableId])->Result<BTreeMap<PortableId,u64>,String> {
        let mut extents=BTreeMap::new();
        for id in ids {
            let location=&self.index.get(id).ok_or("Missing durable resource index")?.location;let pack=location.pack.id;
            let extent=location.pack.extent.load(Ordering::Acquire);
            if !extents.contains_key(&pack) {
                if location.pack.file.metadata().map_err(|error|error.to_string())?.len()<extent {return Err("Recovery pack is shorter than its committed extent".into());}
                extents.insert(pack,extent);
            }
        }Ok(extents)
    }
    fn publish_packed(&mut self,metadata:Vec<u8>,roots:GenerationRoots,programs:&super::TrustedPrograms,cancelled:&AtomicBool,publication:&mut impl Publication)->Result<PortableId,String> {
        check_cancelled(cancelled)?;self.validate_layout()?;self.refresh_head()?;
        if retired_store(&self.path)? {return Err("This drawing's session copy was retired".into());}
        if metadata.len() as u64>ProjectLimits::default().metadata_bytes {return Err("Packed recovery metadata exceeds admission limit".into());}
        let generation=PortableId::random();
        publication.publish(&self.generation_path(generation),|file|file.write_all(&metadata).map_err(|error|error.to_string()),||check_cancelled(cancelled)).map_err(|error|error.error)?;
        let reference=Generation::new(generation,&metadata);
        let head=Head {current:reference.clone(),previous:self.loaded.clone().or_else(||self.head.as_ref().map(|head|head.current.clone()))};
        let bytes=serde_json::to_vec(&head).map_err(|error|error.to_string())?;
        let published=publication.publish(&self.path.join("head.json"),|file|file.write_all(&bytes).map_err(|error|error.to_string()),||check_cancelled(cancelled));
        if published.as_ref().is_err_and(|error|!error.published) {return Err(published.unwrap_err().error);}
        self.generation_roots.insert(generation,(reference.clone(),roots));
        self.head=Some(head);self.loaded=Some(reference);self.programs.insert(generation,programs.clone());
        let previous=self.head.as_ref().and_then(|head|head.previous.as_ref()).map(|previous|previous.id);
        self.programs.retain(|id,_|*id==generation || Some(*id)==previous);
        published.map_err(|error|error.error)?;self.collect_garbage()?;Ok(generation)
    }
    fn store_inputs(&mut self,inputs:Vec<StoreInput>,cancelled:&AtomicBool,publication:&mut impl Publication)->Result<Vec<PortableId>,String> {
        self.validate_layout()?;
        if retired_store(&self.path)? {return Err("This drawing's session copy was retired".into());}
        let mut records=Vec::with_capacity(inputs.len());
        let mut pending=Vec::new();let mut bytes=0u64;
        for input in inputs {
            check_cancelled(cancelled)?;
            if bytes>0 && bytes+input.length>PACK_BYTES {self.append_inputs(std::mem::take(&mut pending),cancelled,publication)?;bytes=0;}
            let id=input.id;
            if let Some(location)=self.existing(&input)? {self.remember(input,location)?;}
            else {bytes=bytes.saturating_add(input.length);pending.push(input);}
            records.push(id);
        }
        if !pending.is_empty() {self.append_inputs(pending,cancelled,publication)?;}
        Ok(records)
    }
    fn existing(&self,input:&StoreInput)->Result<Option<Arc<StoreLocation>>,String> {
        if let Payload::Tile(tile)=&input.payload && let Some(location)=tile.compressed.location()? && Arc::ptr_eq(&location.pack.owner,&self._owner) {
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
    fn append_inputs(&mut self,inputs:Vec<StoreInput>,cancelled:&AtomicBool,publication:&mut impl Publication)->Result<(),String> {
        let mut first=0;
        while first<inputs.len() {
            let target=if let Some((id,extent))=self.append {
                match fs::metadata(self.pack_path(id)) {
                    Ok(metadata)=>{
                        if metadata.len()<extent {return Err("Recovery pack is shorter than its committed extent".into());}
                        metadata.len().checked_add(inputs[first].length.max(1)).filter(|end|*end<=PACK_BYTES).map(|_|(id,metadata.len()))
                    },
                    Err(error) if error.kind()==std::io::ErrorKind::NotFound=>{
                        if self.resource_roots()?.packs.contains(&id) {return Err("Committed recovery pack is missing".into());}None
                    },
                    Err(error)=>return Err(error.to_string()),
                }
            }else {None};
            let (pack,start)=target.unwrap_or_else(||(PortableId::random(),0));
            let mut length=0u64;let mut last=first;
            let mut aliases=BTreeMap::<(u8,usize),(u64,[u8;32])>::new();let mut offsets=Vec::new();
            while last<inputs.len() {
                let input=&inputs[last];let key=input.owner_key();
                let alias=aliases.get(&key).copied();
                let offset=if let Some((offset,descriptor))=alias {
                    if descriptor!=input.descriptor {return Err("Conflicting aliased resource descriptors".into());}offset
                }else {
                    if last>first && start.saturating_add(length).saturating_add(input.length)>PACK_BYTES {break;}
                    let offset=start+length;length=length.checked_add(input.length.max(1)).ok_or("Recovery pack length overflow")?;
                    aliases.insert(key,(offset,input.descriptor));offset
                };
                offsets.push((offset,alias.is_none()));last+=1;
            }
            let write=|file:&mut BufWriter<File>| {
                for (input,&(_,physical)) in inputs[first..last].iter().zip(&offsets) {
                    if !physical {continue;}
                    let record=ResourceRecord {id:input.id,offset:0,length:input.length,crc:input.crc,descriptor:input.descriptor};
                    write_payload(file,&input.payload,&record,cancelled)?;
                    if input.length==0 {file.write_all(&[0]).map_err(|error|error.to_string())?;}
                }Ok(())
            };
            if target.is_some() {publication.append(&self.pack_path(pack),write,||check_cancelled(cancelled))?;}
            else {publication.publish(&self.pack_path(pack),write,||check_cancelled(cancelled)).map_err(|error|error.error)?;}
            let extent=start.checked_add(length).ok_or("Recovery pack extent overflow")?;
            let owner=self.pack(pack,extent)?;let source=owner.source(&self.reads);
            let mut shared=BTreeMap::new();
            for (input,&(offset,_)) in inputs[first..last].iter().zip(&offsets) {
                let location=shared.entry(offset).or_insert_with(||Arc::new(StoreLocation {pack:owner.clone(),source:source.clone(),offset,length:input.length,crc:input.crc,descriptor:input.descriptor})).clone();
                self.remember(input.clone(),location)?;
            }
            self.append=Some((pack,extent));first=last;
        }Ok(())
    }
    fn remember(&mut self,input:StoreInput,location:Arc<StoreLocation>)->Result<(),String> {
        if let Payload::Tile(tile)=&input.payload && let Some(memory)=tile.compressed.memory()? {tile.compressed.bind(&memory,location.clone())?;}
        self.by_owner.insert(input.owner_key(),input.id);
        self.index.insert(input.id,IndexedResource {record:input.record,location,owner:Some(input.owner)});Ok(())
    }
    pub fn make_durable(&mut self,tiles:&crate::raster_storage::RetainedTiles,cancelled:&AtomicBool)->Result<usize,String> {
        self.make_durable_using(tiles,cancelled,AtomicPublication)
    }
    fn make_durable_using(&mut self,tiles:&crate::raster_storage::RetainedTiles,cancelled:&AtomicBool,mut publication:impl Publication)->Result<usize,String> {
        let before=tiles.resident_bytes();
        let inputs=tiles.blobs()?.into_iter().filter(|blob|blob.resident_bytes()>0).map(|blob| {
            let entry=super::resources::ResourceEntry {kind:"capy.raster-tile/1",data:super::resources::encode_descriptor(blob.descriptor)?,raw_encoding:"capy.lz4-tile/1",payload:Payload::Tile(blob)};
            StoreInput::entry(&entry,cancelled)
        }).collect::<Result<Vec<_>,String>>()?;
        let result=self.store_inputs(inputs,cancelled,&mut publication);
        let released=before.saturating_sub(tiles.resident_bytes());
        if released>0 && let Some(owner)=tiles.owner.as_ref().and_then(Weak::upgrade) {owner.refresh();}
        result.map(|_|released)
    }
    fn recover_append(&mut self,extents:&BTreeMap<PortableId,u64>) {
        self.append=extents.iter().find(|(_,extent)|**extent<PACK_BYTES).map(|(&id,&extent)|(id,extent));
    }
    fn pack(&mut self,id:PortableId,extent:u64)->Result<Arc<Pack>,String> {
        let mut packs=self._owner.packs.lock().map_err(|_|"Recovery pack lock failed")?;
        let pack=match packs.get(&id).and_then(Weak::upgrade) {
            Some(pack)=>pack,
            None=>{let pack=Arc::new(Pack {id,file:Arc::new(File::open(self.pack_path(id)).map_err(|error|error.to_string())?),owner:self._owner.clone(),source:Default::default(),extent:AtomicU64::new(extent)});packs.insert(id,Arc::downgrade(&pack));pack},
        };
        if pack.file.metadata().map_err(|error|error.to_string())?.len()<extent {return Err("Recovery pack is shorter than its committed extent".into());}
        pack.extent.fetch_max(extent,Ordering::Release);Ok(pack)
    }

    pub fn load(&mut self, limits: ProjectLimits, cancelled: &AtomicBool) -> Result<Option<OpenSession>, String> {
        check_cancelled(cancelled)?;
        self.validate_layout()?;
        if retired_store(&self.path)? { return Err("This drawing was closed and its session copy was retired".into()); }
        self.refresh_head()?;
        let Some(head) = self.head.clone() else { return Ok(None) };
        let mut failure = None;
        for generation in std::iter::once(head.current).chain(head.previous) {
            check_cancelled(cancelled)?;
            match self.load_generation(&generation, limits, cancelled) {
                Ok(session) => { self.loaded = Some(generation); return Ok(Some(session)); }
                Err(super::session::OpenFailure::Unsupported(message)) => return Err(message),
                Err(super::session::OpenFailure::MemoryBudgetExceeded) => return Err(crate::MEMORY_BUDGET_EXCEEDED.into()),
                Err(error) => { if failure.is_none() { failure = Some(error.message()); } }
            }
        }
        Err(failure.unwrap_or_else(|| "Recovery checkpoint unavailable".into()))
    }

    fn generation_metadata(&self, generation: &Generation, limit: u64) -> Result<Vec<u8>, String> {
        let metadata = read_bounded(&self.generation_path(generation.id), limit)?;
        if Generation::new(generation.id, &metadata) != *generation { return Err("Recovery metadata checksum failed".into()); }
        Ok(metadata)
    }
    fn load_generation(&mut self,generation:&Generation,limits:ProjectLimits,cancelled:&AtomicBool)->Result<OpenSession,super::session::OpenFailure> {
        let metadata=self.generation_metadata(generation,limits.metadata_bytes)?;
        let mut stored=StoredGeneration::parse(&metadata,limits)?;
        let index=resource_index(&stored.metadata,limits)?;
        if stored.locations.is_none() {
            let value:serde_json::Value=serde_json::from_slice(&stored.metadata).map_err(|error|error.to_string())?;
            let values=value["resources"].as_array().ok_or("Missing legacy resource records")?;
            let mut inputs=Vec::new();
            for (record,value) in index.records.iter().zip(values) {
                check_cancelled(cancelled)?;
                let file=Arc::new(File::open(self.resource_path(record.id)).map_err(|error|error.to_string())?);
                if file.metadata().map_err(|error|error.to_string())?.len()!=record.length {return Err("Recovery resource length changed".into());}
                let source=ReadSource::file(file,record.length);
                let backing=ImmutableBacking::new(Arc::new(source))?;
                let resource=Arc::new(crate::authored::OpaqueResource::from_record(record.id,value,backing,0,record.length,record.crc)?);
                inputs.push(StoreInput::opaque(resource)?);
            }
            let records=self.store_inputs(inputs,cancelled,&mut AtomicPublication)?;
            let mut value=value;value["resources"]=self.resource_values(&records)?.into();
            stored.metadata=serde_json::to_vec(&value).map_err(|error|error.to_string())?;
            let mut locations=BTreeMap::new();
            for record in value["resources"].as_array().unwrap() {
                let id:PortableId=record["id"].as_str().unwrap().parse().map_err(str::to_string)?;
                let location=&self.index[&id].location;
                stored.extents.insert(location.pack.id,location.pack.extent.load(Ordering::Acquire));
                locations.insert(id,PackRange {pack:location.pack.id,offset:location.offset});
            }
            stored.locations=Some(locations);
        }
        let mut locations=BTreeMap::new();
        let mut packs=BTreeMap::new();
        for (&id,&extent) in &stored.extents {packs.insert(id,self.pack(id,extent)?);}
        let value:serde_json::Value=serde_json::from_slice(&stored.metadata).map_err(|error|error.to_string())?;
        let records=resource_index(&stored.metadata,limits)?.records;
        for (record,value) in records.iter().zip(value["resources"].as_array().unwrap()) {
            check_cancelled(cancelled)?;
            let range=stored.locations.as_ref().unwrap().get(&record.id).ok_or("Missing recovery pack location")?;
            let pack=packs[&range.pack].clone();
            let location=Arc::new(StoreLocation {source:pack.source(&self.reads),pack,offset:range.offset,length:record.length,crc:record.crc,descriptor:record.descriptor});
            let stored_record=StoredRecord::parse(value.clone())?;
            self.index.entry(record.id).or_insert_with(||IndexedResource {record:stored_record,location:location.clone(),owner:None});
            locations.insert(record.id,location);
        }
        self.recover_append(&stored.extents);
        let resources=StoreResources {locations,extents:stored.extents};
        let programs=self.programs.get(&generation.id).cloned().unwrap_or_default();
        super::session::open_stored_parts(&stored.metadata,&resources,limits,cancelled,&programs)
    }

    pub fn retire(&mut self) -> Result<(), String> {
        self.validate_layout()?;
        retiring_store(&self.path)?;
        if !retired_store(&self.path)? { atomic_replace(&self.path.join(".retired"), b"")?; }
        sync_directory(&self.path)?;
        remove_file(&self.path.join(".retiring"))?;
        remove_file(&self.path.join("head.json"))?;
        sync_directory(&self.path)?;
        self.head = None;
        self.loaded = None;
        self.programs.clear();
        self.collect_garbage()
    }
    pub fn prepare_retirement(&mut self) -> Result<(), String> {
        mark_retirement_intent(&self.path)
    }

    pub fn collect_garbage(&mut self) -> Result<(), String> {
        self.validate_layout()?;
        self.refresh_head()?;
        sync_directory(&self.path)?;
        {let roots=self.resource_roots()?;self.collect_roots(roots)}
    }
    fn resource_roots(&mut self)->Result<StoreRoots,String> {
        let mut roots=StoreRoots {generations:BTreeSet::new(),resources:BTreeSet::new(),packs:BTreeSet::new(),identities:BTreeSet::new()};
        if let Some(head)=self.head.clone() {
            for generation in std::iter::once(head.current).chain(head.previous) {
                roots.generations.insert(generation.id);
                let metadata=self.generation_metadata(&generation,ProjectLimits::default().metadata_bytes)?;
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
    fn collect_roots(&mut self, roots: StoreRoots) -> Result<(), String> {
        let StoreRoots {generations,resources,mut packs,identities}=roots;
        self.index.retain(|id,entry|identities.contains(id) || entry.owner.as_ref().is_some_and(|owner|owner.strong_count()!=0));
        self.by_owner.retain(|_,id|self.index.contains_key(id));
        self._owner.packs.lock().map_err(|_|"Recovery pack lock failed")?.retain(|id,weak| {
            if weak.strong_count()==0 {false}else {packs.insert(*id);true}
        });
        let terminal = retired_store(&self.path)?;
        for (name, retained, suffix) in [("generations", &generations, ".json"), ("resources", &resources, ".bin"), ("packs", &packs, ".bin")] {
            let path = self.path.join(name);
            if metadata_optional(&path)?.is_some() {
                clean_directory(&path, retained, suffix)?;
                if terminal && retained.is_empty() { fs::remove_dir(&path).map_err(|e| e.to_string())?; }
            } else if !terminal && name != "packs" { return Err("Missing drawing session directory".into()); }
        }
        clean_temporaries(&self.path)?;
        Ok(())
    }
    fn resource_path(&self, id: PortableId) -> PathBuf { self.path.join("resources").join(format!("{id}.bin")) }
    fn pack_path(&self, id: PortableId) -> PathBuf { self.path.join("packs").join(format!("{id}.bin")) }
    fn validate_layout(&self) -> Result<(), String> {
        if store_layout_with(&self.path, retired_store(&self.path)?)? { Ok(()) } else { Err("Invalid drawing session directory".into()) }
    }
    fn generation_path(&self, id: PortableId) -> PathBuf { self.path.join("generations").join(format!("{id}.json")) }
    fn refresh_head(&mut self) -> Result<(), String> {
        let path = self.path.join("head.json");
        let head = read_optional(&path, 1024)?.map(|bytes| parse_head(&bytes)).transpose()?;
        if head != self.head { self.loaded = None; self.head = head; }
        Ok(())
    }
}

pub fn prepare_store_retirement(path: &Path) -> Result<(), String> {
    if metadata_optional(path)?.is_none() { return Ok(()); }
    if retired_store(path)? {
        return if store_layout_with(path, true)? { Ok(()) } else { Err("Invalid drawing session directory".into()) };
    }
    if !store_layout(path)? { return Err("Invalid drawing session directory".into()); }
    let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let Some(_claim) = claim_store(&path)? else { return Err("Recovery drawing is already open".into()) };
    mark_retirement_intent(&path)
}
fn mark_retirement_intent(path: &Path) -> Result<(), String> {
    if retired_store(path)? { return Err("This drawing's session copy was retired".into()); }
    if retiring_store(path)? { return Ok(()); }
    atomic_replace(&path.join(".retiring"), b"retiring\n")
}
fn store_layout(path: &Path) -> Result<bool, String> {
    store_layout_with(path, false)
}
fn store_layout_with(path: &Path, terminal: bool) -> Result<bool, String> {
    Ok(metadata_optional(path)?.is_some_and(|metadata| metadata.is_dir())
        && metadata_optional(&path.join(".lock"))?.is_some_and(|metadata| metadata.is_file())
        && metadata_optional(&path.join("resources"))?.map_or(terminal, |metadata| metadata.is_dir())
        && metadata_optional(&path.join("packs"))?.is_none_or(|metadata| metadata.is_dir())
        && metadata_optional(&path.join("generations"))?.map_or(terminal, |metadata| metadata.is_dir()))
}
fn compact_retired_store(path: &Path) -> Result<bool, String> {
    for entry in fs::read_dir(path).map_err(|e|e.to_string())? {
        if !matches!(entry.map_err(|e|e.to_string())?.file_name().to_str(), Some(".lock"|".retired")) { return Ok(false); }
    }
    Ok(true)
}
pub fn collect_unreferenced_stores(root: &Path, reachable: &BTreeSet<String>, cancelled: &AtomicBool) -> Result<usize, String> {
    let mut retired = 0;
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        check_cancelled(cancelled)?;
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() { continue; }
        let Some(key) = entry.file_name().to_str().map(str::to_owned) else { continue };
        let path = entry.path();
        if reachable.contains(&key) || !metadata_optional(&path.join(".lock"))?.is_some_and(|metadata|metadata.is_file()) { continue; }
        let terminal = retired_store(&path)?;
        let retiring = retiring_store(&path)?;
        if !store_layout_with(&path, terminal)? { continue; }
        let head = metadata_optional(&path.join("head.json"))?;
        if terminal && head.is_none() && !retiring && metadata_optional(&path.join("resources"))?.is_none()
            && metadata_optional(&path.join("generations"))?.is_none()
            && compact_retired_store(&path)? { continue; }
        let path = fs::canonicalize(&path).map_err(|e| e.to_string())?;
        let Some((owner, writer)) = claim_store(&path)? else { continue };
        if head.is_some() && !terminal && !retiring { continue; }
        let mut store = SessionStore { path, _owner: owner, _writer: writer,
            head:None,loaded:None,index:BTreeMap::new(),by_owner:BTreeMap::new(),append:None,programs:BTreeMap::new(),reads:crate::tile_io::ReadCache::process(),generation_roots:BTreeMap::new(),published_capture:None };
        store.retire()?;
        retired += 1;
    }
    Ok(retired)
}
fn retired_store(path: &Path) -> Result<bool, String> {
    let marker = path.join(".retired");
    let Some(bytes) = read_optional(&marker, 16)? else { return Ok(false) };
    if !bytes.is_empty() { return Err("Invalid drawing retirement marker".into()); }
    Ok(true)
}
fn retiring_store(path: &Path) -> Result<bool, String> {
    let Some(bytes) = read_optional(&path.join(".retiring"), 16)? else { return Ok(false) };
    if bytes != b"retiring\n" { return Err("Invalid drawing retirement intent".into()); }
    Ok(true)
}

trait Publication {
    fn publish(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), AtomicReplaceError>;
    fn append(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        append_pack(path, write, ready)
    }
}
fn append_pack(path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    append_using(path, write, ready, |file| sync_file(file).map_err(|e| e.to_string()))
}
fn append_using(path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>, sync: impl FnOnce(&File) -> Result<(), String>) -> Result<(), String> {
    if !metadata_optional(path)?.is_some_and(|metadata| metadata.is_file()) { return Err("Invalid recovery pack file".into()); }
    let mut file = BufWriter::new(OpenOptions::new().append(true).open(path).map_err(|e| e.to_string())?);
    write(&mut file)?;
    file.flush().map_err(|e| e.to_string())?;
    sync(file.get_ref())?;
    ready()
}
pub(crate) fn sync_file(file: &File) -> std::io::Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        use std::os::fd::AsRawFd;
        unsafe extern "C" { fn fcntl(fd: i32, command: i32, ...) -> i32; }
        const F_FULLFSYNC: i32 = 51;
        if unsafe { fcntl(file.as_raw_fd(), F_FULLFSYNC) } == -1 { return Err(std::io::Error::last_os_error()); }
        Ok(())
    }
    #[cfg(all(unix, not(target_vendor = "apple")))]
    { file.sync_data() }
    #[cfg(windows)]
    { file.sync_all() }
}
struct AtomicPublication;
impl Publication for AtomicPublication {
    fn publish(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), AtomicReplaceError> {
        atomic_publish_checked(path, write, ready)
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
    file:Arc<File>,
    owner:Arc<OwnerLock>,
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
    fn read(&self,offset:u64,length:usize,crc:Option<u32>,completion:ReadCompletion) {
        let mut bytes=read_buffer(length);
        let result=read_exact_at(&self.file,offset,Arc::get_mut(&mut bytes).unwrap())
            .and_then(|_|crate::tile_io::verify_crc(&bytes,crc)).map(|_|bytes);
        completion.complete(result);
    }
}
pub(crate) struct StoreLocation {
    pack:Arc<Pack>,
    source:ReadSource,
    offset:u64,
    pub(crate) length:u64,
    pub(crate) crc:u32,
    descriptor:[u8;32],
}
impl StoreLocation {
    pub(crate) fn poll(&self)->Result<RangeState,String> {self.source.poll_checked(self.offset,self.length as usize,self.crc)}
    pub(crate) fn read_on_worker(&self)->Result<ByteRange,String> {self.source.read_checked_on_worker(self.offset,self.length as usize,self.crc)}
    pub(crate) fn backing(self:&Arc<Self>)->Result<ImmutableBacking,String> {ImmutableBacking::new(self.clone()).map_err(str::to_string)}
    pub(crate) fn read_all_on_worker(&self)->Result<Arc<[u8]>,String> {
        let length=usize::try_from(self.length).map_err(|_|"Stored resource exceeds address space")?;
        let mut bytes=Vec::new();bytes.try_reserve_exact(length).map_err(|_|"Stored resource allocation failed")?;
        let mut checksum=crc32fast::Hasher::new();
        while bytes.len()<length {
            let count=(length-bytes.len()).min(MAX_RANGE_BYTES);
            let chunk=self.source.wait_on_worker(self.offset+bytes.len() as u64,count)?;
            checksum.update(&chunk);bytes.extend_from_slice(&chunk);
        }
        if checksum.finalize()!=self.crc {return Err("Recovery resource checksum failed".into());}
        Ok(bytes.into())
    }
}
impl ByteSource for StoreLocation {
    fn byte_len(&self)->u64 {self.length}
    fn resident_bytes(&self)->usize {0}
    fn poll(&self,offset:u64,length:usize)->Result<RangeState,String> {
        if offset.checked_add(length as u64).is_none_or(|end|end>self.length) {return Err("Stored resource range exceeds extent".into());}
        self.source.poll(self.offset+offset,length)
    }
    fn poll_on_worker(&self,offset:u64,length:usize)->Result<RangeState,String> {
        if offset.checked_add(length as u64).is_none_or(|end|end>self.length) {return Err("Stored resource range exceeds extent".into());}
        self.source.wait_on_worker(self.offset+offset,length).map(RangeState::Ready)
    }
}
pub(crate) struct StoreResources {
    pub(crate) locations:BTreeMap<PortableId,Arc<StoreLocation>>,
    extents:BTreeMap<PortableId,u64>,
}
impl StoreResources {
    pub(crate) fn manifest_records(&self,records:&mut [serde_json::Value])->Result<(),String> {
        let packs=self.extents.keys().enumerate().map(|(index,&pack)|(pack,format!("data/tiles-{}.bin",index+1))).collect::<BTreeMap<_,_>>();
        for record in records {
            let id:PortableId=record["id"].as_str().ok_or("Missing stored resource identity")?.parse().map_err(str::to_string)?;
            let location=self.locations.get(&id).ok_or("Missing stored resource location")?;
            record["location"]["pack"]=packs[&location.pack.id].clone().into();
        }Ok(())
    }
    pub(crate) fn directory(&self)->super::archive::Directory {
        super::archive::Directory {members:self.extents.values().enumerate().map(|(index,&length)|super::archive::Member {
            name:format!("data/tiles-{}.bin",index+1),offset:0,length,compressed_length:None,crc32:0,
        }).collect(),length:self.extents.values().copied().sum()}
    }
}
fn write_payload(file: &mut impl Write, payload: &Payload, record: &ResourceRecord, cancelled: &AtomicBool) -> Result<(), String> {
    if let Payload::Tile(tile)=payload {
        let integrity=tile.compressed.integrity(cancelled)?;
        if integrity.bytes!=record.length || integrity.crc32!=record.crc {return Err("Recovery tile integrity changed".into());}
        let bytes=tile.compressed_on_worker()?;
        for chunk in bytes.chunks(MAX_RANGE_BYTES) {
            check_cancelled(cancelled)?;
            file.write_all(chunk).map_err(|error|error.to_string())?;
        }
        return Ok(());
    }
    if let Payload::Opaque(resource)=payload {
        let mut checksum=crc32fast::Hasher::new();let mut offset=0;
        while offset<record.length {
            check_cancelled(cancelled)?;let count=(record.length-offset).min(MAX_RANGE_BYTES as u64) as usize;
            let bytes=resource.read_chunk(offset,count,cancelled)?;checksum.update(&bytes);
            file.write_all(&bytes).map_err(|error|error.to_string())?;offset+=count as u64;
        }
        if checksum.finalize()!=record.crc {return Err("Recovery resource checksum failed".into());}
    }else {
        let (_,integrity)=payload.prepared(cancelled)?;
        if integrity.bytes!=record.length || integrity.crc32!=record.crc {return Err("Recovery resource integrity changed".into());}
        let bytes=payload.encoded()?.bytes;
        for chunk in bytes.chunks(MAX_RANGE_BYTES) {check_cancelled(cancelled)?;file.write_all(chunk).map_err(|error|error.to_string())?;}
    }

    Ok(())
}
pub fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    read_file_bounded(file, limit)
}
fn read_optional(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let Some(metadata) = metadata_optional(path)? else { return Ok(None) };
    if !metadata.is_file() { return Err("Invalid recovery metadata file".into()); }
    match File::open(path) {
        Ok(file) => read_file_bounded(file, limit).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
fn metadata_optional(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
fn read_file_bounded(file: File, limit: u64) -> Result<Vec<u8>, String> {
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if length > limit || length > usize::MAX as u64 { return Err("Recovery metadata exceeds admission limit".into()); }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit { return Err("Recovery metadata exceeds admission limit".into()); }
    Ok(bytes)
}
fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) { Err("Recovery operation cancelled".into()) } else { Ok(()) }
}

pub fn create_directory(path: &Path) -> Result<(), String> {
    if metadata_optional(path)?.is_some_and(|metadata| metadata.is_dir()) { return Ok(()); }
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).ok_or("Invalid recovery directory")?;
    create_directory(parent)?;
    #[cfg(unix)]
    let mut builder = fs::DirBuilder::new();
    #[cfg(windows)]
    let builder = fs::DirBuilder::new();
    #[cfg(unix)] { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
    match builder.create(path) {
        Ok(()) => {},
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && metadata_optional(path)?.is_some_and(|metadata| metadata.is_dir()) => {},
        Err(error) => return Err(error.to_string()),
    }
    sync_directory(parent)
}
pub fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)] { File::open(path).and_then(|file| file.sync_all()).map_err(|e| e.to_string()) }
    #[cfg(windows)] { let _ = path; Ok(()) }
}
pub fn atomic_publish(path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    atomic_publish_checked(path, write, ready).map_err(|error| error.error)
}
fn atomic_publish_checked(path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), AtomicReplaceError> {
    #[cfg(unix)] { crate::atomic_file::atomic_write_outcome(path, write, ready) }
    #[cfg(windows)] {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" { fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32; }
        let parent = path.parent().ok_or("Invalid recovery destination")?;
        let parent = fs::canonicalize(if parent.as_os_str().is_empty() { Path::new(".") } else { parent }).map_err(|error| error.to_string())?;
        let destination = parent.join(path.file_name().ok_or("Invalid recovery destination")?);
        let temporary = parent.join(format!(".capy-save-{}", PortableId::random()));
        struct Cleanup(PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }
        let cleanup = Cleanup(temporary);
        let mut file = BufWriter::new(OpenOptions::new().create_new(true).write(true).open(&cleanup.0).map_err(|e| e.to_string())?);
        write(&mut file)?;
        file.flush().and_then(|_| file.get_ref().sync_all()).map_err(|e| e.to_string())?;
        drop(file);
        ready()?;
        let from: Vec<u16> = cleanup.0.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 { return Err(std::io::Error::last_os_error().to_string().into()); }
        Ok(())
    }
}
pub fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    atomic_replace_checked(path, bytes).map_err(|error| error.error)
}
pub fn atomic_replace_checked(path: &Path, bytes: &[u8]) -> Result<(), AtomicReplaceError> {
    let parent = path.parent().ok_or("Invalid recovery destination")?;
    create_directory(parent)?;
    if let Some(ancestor) = parent.parent().filter(|path| !path.as_os_str().is_empty()) { sync_directory(ancestor)?; }
    atomic_publish_checked(path, |file| file.write_all(bytes).map_err(|e| e.to_string()), || Ok(()))
}
fn remove_file(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) { Ok(()) => Ok(()), Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()), Err(error) => Err(error.to_string()) }
}
fn clean_directory(path: &Path, retained: &BTreeSet<PortableId>, suffix: &str) -> Result<(), String> {
    if !metadata_optional(path)?.is_some_and(|metadata| metadata.is_dir()) { return Err("Invalid recovery cleanup directory".into()); }
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(".capy-save-") || name.strip_suffix(suffix).and_then(|id| id.parse::<PortableId>().ok()).is_some_and(|id| !retained.contains(&id)) {
            remove_file(&entry.path())?;
        }
    }
    sync_directory(path)
}
fn clean_temporaries(path: &Path) -> Result<(), String> { clean_directory(path, &BTreeSet::new(), ".unused") }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, DocumentNames, Editor, raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey}};
    use serde_json::json;
    use super::super::session::{EditorCapture, PreparedSession, SessionMetadata};
    struct Captured {editor:EditorCapture,metadata:SessionMetadata,archive:PreparedSession}
    impl std::ops::Deref for Captured {type Target=PreparedSession;fn deref(&self)->&PreparedSession {&self.archive}}
    fn captured(editor:&EditorCapture,metadata:impl Into<SessionMetadata>)->Captured {
        let metadata=metadata.into();let archive=PreparedSession::prepare(editor,metadata.clone(),&AtomicBool::new(false)).unwrap();
        Captured {editor:editor.clone(),metadata,archive}
    }
    fn checkpoint(store:&mut SessionStore,capture:&Captured,cancel:&AtomicBool)->Result<PortableId,String> {
        store.checkpoint(&capture.editor,capture.metadata.clone(),cancel)
    }
    fn checkpoint_using(store:&mut SessionStore,capture:&Captured,cancel:&AtomicBool,publication:impl Publication)->Result<PortableId,String> {
        store.checkpoint_using(&capture.editor,capture.metadata.clone(),cancel,publication)
    }

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("capy-session-store-test-{}", PortableId::random())))
        }
    }
    impl Drop for Directory { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    #[cfg(windows)]
    #[test]
    fn restore_diagnostics_publish_and_clear_in_long_directories() {
        use std::os::windows::ffi::OsStrExt;
        let directory = Directory::new();
        let mut root = directory.0.clone();
        while root.as_os_str().encode_wide().count() < 280 { root = root.join("saved-drawings"); }
        fs::create_dir_all(&root).unwrap();
        for cause in ["first restore failure", "second restore failure"] {
            set_restore_error(&root, Some(cause)).unwrap();
            assert_eq!(restore_error(&root).unwrap().as_deref(), Some(cause));
            assert_eq!(count(&root), 1);
        }
        let error = atomic_publish_checked(&root.join("restore-error.txt"),
            |file| file.write_all(b"uncommitted failure").map_err(|error| error.to_string()),
            || Err("Cancelled".into())).unwrap_err();
        assert!(!error.published);
        assert_eq!(restore_error(&root).unwrap().as_deref(), Some("second restore failure"));
        assert_eq!(count(&root), 1);
        set_restore_error(&root, None).unwrap();
        assert_eq!(restore_error(&root).unwrap(), None);
        assert_eq!(count(&root), 0);
    }
    fn capture(marker: u8) -> Captured {
        let mut document = Document::new(PortableId::random(), 256, 256, DocumentNames { paint: "ink".into(), paper: "paper".into() });
        let descriptor = RasterPlane::Color.descriptor(Default::default());
        let tile = RasterTile::backed(TileBlob::encode(descriptor, &vec![marker; 256 * 256 * 4]).unwrap());
        *document.target_raster_mut(document.working.target.unwrap()).unwrap() = RasterRevision::backed(RasterData {
            tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }, tile)].into(), ..Default::default()
        });
        let editor = Editor::new(document);
        let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
        captured(&capture, json!({"marker": marker}))
    }
    fn copy_fixture(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap(); fs::copy(source.join("head.json"), destination.join("head.json")).unwrap();
        for name in ["generations", "resources"] {
            fs::create_dir_all(destination.join(name)).unwrap();
            for entry in fs::read_dir(source.join(name)).unwrap() {
                let entry = entry.unwrap(); fs::copy(entry.path(), destination.join(name).join(entry.file_name())).unwrap();
            }
        }
    }
    fn session_history(path: &Path) -> serde_json::Value {
        let cancel = AtomicBool::new(false); let mut store = SessionStore::open(path).unwrap();
        let mut opened = store.load(ProjectLimits::default(), &cancel).unwrap().unwrap();
        assert_eq!(store.loaded_generation(), store.generation());
        let snapshot = |editor: &Editor| {
            let capture = editor.capture_session(editor.capture(11, editor.document().output().context.clone()).unwrap()).unwrap();
            let prepared = PreparedSession::prepare(&capture, opened.metadata.clone(), &cancel).unwrap();
            let mut metadata: serde_json::Value = serde_json::from_slice(prepared.metadata()).unwrap(); metadata["checkpoint"]["owner"] = 0.into();
            let resources = prepared.resources().entries.iter().map(|entry| (entry.payload.id().to_string(), Sha256::digest(&entry.payload.encoded().unwrap().bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>())).collect::<BTreeMap<_,_>>();
            json!({"metadata":metadata,"resources":resources})
        };
        let current = snapshot(&opened.editor); let mut history = Vec::new();
        while opened.editor.can_undo() { opened.editor.undo().unwrap(); history.push(snapshot(&opened.editor)); }
        while opened.editor.can_redo() { opened.editor.redo().unwrap(); history.push(snapshot(&opened.editor)); }
        assert!(!history.is_empty()); json!({"current":current,"history":history})
    }
    #[test]
    fn flat_session_moves_resume_at_every_store_manifest_and_lock_boundary() {
        for interrupt in 1..=4 {
            let directory = Directory::new(); let root = &directory.0;
            let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
            copy_fixture(&crate_root.join("tests/compat/1.0.15/session"), &root.join("drawing-a"));
            copy_fixture(&crate_root.join("src/package/codec/fixtures/v1.0.14-paint-session"), &root.join("drawing-b"));
            let expected = [session_history(&root.join("drawing-a")), session_history(&root.join("drawing-b"))];
            let manifest = serde_json::to_vec(&json!({"generation":7,"drawings":[{"id":1,"key":"drawing-a"},{"id":2,"key":"drawing-b"}],"active":2,"clean_exit":false,"restoring":[],"blocked":[],"future_ui":{"keep":42}})).unwrap();
            fs::write(root.join("window-test.json"), &manifest).unwrap();
            let mut count = 0;
            assert!(import_sessions_using(root, ".json", ".lock", |_| {
                assert!(OwnerLock::claim(&root.join(".gc.lock")).unwrap().is_none());
                assert!(OwnerLock::claim(&root.join("packed/.gc.lock")).unwrap().is_none());
                count += 1; if count == interrupt { Err("Interrupted move".into()) } else { Ok(()) }
            }).is_err());
            let packed = import_session_directories(root, ".json", ".lock").unwrap();
            import_session_directories(root, ".json", ".lock").unwrap();
            assert!(!root.join("window-test.json").exists()); assert!(!root.join("window-test.lock").exists());
            assert!(packed.join("window-test.lock").is_file()); assert_eq!(fs::read(packed.join("window-test.json")).unwrap(), manifest);
            for (index, key) in ["drawing-a", "drawing-b"].into_iter().enumerate() {
                assert!(!root.join(key).exists()); assert_eq!(session_history(&packed.join(key)), expected[index]);
            }
            collect_unreferenced_stores(root, &BTreeSet::new(), &AtomicBool::new(false)).unwrap();
            assert!(!packed.join(".lock").exists()); assert!(packed.join("drawing-a/head.json").is_file()); assert!(packed.join("drawing-b/head.json").is_file());
        }
    }
    fn many_tiles(count: u32) -> Captured {
        let mut document = Document::new(PortableId::random(), 50 * 256, count.div_ceil(50) * 256,
            DocumentNames { paint: "ink".into(), paper: "paper".into() });
        let descriptor = RasterPlane::Color.descriptor(Default::default());
        let tiles = (0..count).map(|index| {
            let mut bytes = vec![37; 256 * 256 * 4];
            bytes[..4].copy_from_slice(&index.to_le_bytes());
            let tile = RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap());
            (TileKey { plane: RasterPlane::Color, coordinate: [index % 50, index / 50] }, tile)
        }).collect();
        *document.target_raster_mut(document.working.target.unwrap()).unwrap() = RasterRevision::backed(RasterData { tiles, ..Default::default() });
        let editor = Editor::new(document);
        let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
        captured(&capture, json!({"marker": 37}))
    }
    struct Operations<'a>(&'a mut Vec<String>);
    impl Publication for Operations<'_> {
        fn publish(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), AtomicReplaceError> {
            self.0.push(path.parent().unwrap().file_name().unwrap().to_str().unwrap().into());
            atomic_publish_checked(path, write, ready)
        }
        fn append(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
            self.0.push("append".into()); append_pack(path, write, ready)
        }
    }
    #[test]
    fn two_thousand_tile_checkpoint_syncs_packs_and_reuses_existing_payloads() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let prepared = many_tiles(2_000);
        let mut store = SessionStore::open(&directory.0).unwrap();
        let mut operations = Vec::new();
        checkpoint_using(&mut store, &prepared, &cancel, Operations(&mut operations)).unwrap();
        assert_eq!(operations.len(), 3);
        assert_eq!(&operations[..2], &["packs", "generations"]);
        assert_eq!(count(&directory.0.join("resources")), 0);
        let bytes = store.generation_metadata(&store.head.as_ref().unwrap().current, ProjectLimits::default().metadata_bytes).unwrap();
        let stored = StoredGeneration::parse(&bytes, ProjectLimits::default()).unwrap_or_else(|e| panic!("{}", e.message()));
        let locations = stored.locations.unwrap();
        let mut spatial=super::super::session::SessionDraft::capture(&prepared.editor,prepared.metadata.clone(),&cancel).unwrap().spatial_order().into_iter().map(|(id,(_,y,x))|(id,[x,y])).collect::<Vec<_>>();spatial.sort_by_key(|(_,coordinate)|[coordinate[1],coordinate[0]]);
        assert_eq!(spatial.len(), 2_000);
        assert!(spatial.windows(2).all(|pair| locations[&pair[0].0].offset < locations[&pair[1].0].offset));
        let pack = resource_owner(&store, tile_resource(&prepared).payload.id(), tile_resource(&prepared).bytes);
        let bytes = fs::read(&pack.path).unwrap();
        operations.clear();
        checkpoint_using(&mut store, &prepared, &cancel, Operations(&mut operations)).unwrap();
        assert_eq!(operations.len(), 2);
        assert_eq!(fs::read(&pack.path).unwrap(), bytes);
        operations.clear();
        checkpoint_using(&mut store, &capture(42), &cancel, Operations(&mut operations)).unwrap();
        assert_eq!(&operations[..2], &["append", "generations"]);
        assert_eq!(count(&directory.0.join("packs")), 1);
        assert_eq!(marker(&mut store), 42);
    }
    #[test]
    fn a_truncated_committed_pack_is_never_reused_for_append() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let mut store = SessionStore::open(&directory.0).unwrap();
        let first = capture(1); checkpoint(&mut store, &first, &cancel).unwrap();
        let pack = resource_owner(&store, tile_resource(&first).payload.id(), tile_resource(&first).bytes);
        let head = fs::read(directory.0.join("head.json")).unwrap();
        OpenOptions::new().write(true).open(&pack.path).unwrap().set_len(0).unwrap();
        assert_eq!(checkpoint(&mut store, &capture(2), &cancel).unwrap_err(), "Recovery pack is shorter than its committed extent");
        assert_eq!(fs::metadata(&pack.path).unwrap().len(), 0);
        assert_eq!(fs::read(directory.0.join("head.json")).unwrap(), head);
    }
    #[test]
    fn failed_partial_append_keeps_last_good_extent_and_skips_its_tail() {
        struct FullDisk;
        impl Publication for FullDisk {
            fn publish(&mut self, _: &Path, _: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, _: impl FnOnce() -> Result<(), String>) -> Result<(), AtomicReplaceError> {
                panic!("The incremental checkpoint must append its existing pack")
            }
            fn append(&mut self, path: &Path, _: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, _: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
                let mut file = OpenOptions::new().append(true).open(path).unwrap();
                file.write_all(b"partial append").unwrap(); file.sync_data().unwrap();
                Err(std::io::Error::from(std::io::ErrorKind::StorageFull).to_string())
            }
        }
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let original = capture(1); let second = capture(2);
        let mut store = SessionStore::open(&directory.0).unwrap(); checkpoint(&mut store, &original, &cancel).unwrap();
        let owner = resource_owner(&store, tile_resource(&original).payload.id(), tile_resource(&original).bytes);
        let committed = fs::metadata(&owner.path).unwrap().len();
        let head = fs::read(directory.0.join("head.json")).unwrap();
        assert!(checkpoint_using(&mut store, &second, &cancel, FullDisk).is_err());
        let tail = fs::metadata(&owner.path).unwrap().len(); assert!(tail > committed);
        assert_eq!(fs::read(directory.0.join("head.json")).unwrap(), head);
        assert_eq!(marker(&mut store), 1);
        drop(store);
        let mut store = SessionStore::open(&directory.0).unwrap();
        assert_eq!(fs::metadata(&owner.path).unwrap().len(), tail);
        checkpoint(&mut store, &second, &cancel).unwrap();
        let next = resource_owner(&store, tile_resource(&second).payload.id(), tile_resource(&second).bytes);
        assert_eq!(owner.path, next.path); assert!(next.offset >= tail);
        assert_eq!(marker(&mut store), 2);
        let bytes = fs::read(&owner.path).unwrap(); assert_eq!(&bytes[committed as usize..tail as usize], b"partial append");
    }
    #[test]
    fn failed_append_sync_preserves_the_last_good_checkpoint() {
        struct FailedSync;
        impl Publication for FailedSync {
            fn publish(&mut self, _: &Path, _: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, _: impl FnOnce() -> Result<(), String>) -> Result<(), AtomicReplaceError> {
                panic!("The incremental checkpoint must append")
            }
            fn append(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
                append_using(path, write, ready, |_| Err("Pack data sync failed".into()))
            }
        }
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let mut store = SessionStore::open(&directory.0).unwrap(); checkpoint(&mut store, &capture(1), &cancel).unwrap();
        let head = fs::read(directory.0.join("head.json")).unwrap();
        assert_eq!(checkpoint_using(&mut store, &capture(2), &cancel, FailedSync).unwrap_err(), "Pack data sync failed");
        assert_eq!(fs::read(directory.0.join("head.json")).unwrap(), head); drop(store);
        let mut store = SessionStore::open(&directory.0).unwrap(); assert_eq!(marker(&mut store), 1);
        checkpoint(&mut store, &capture(3), &cancel).unwrap(); assert_eq!(marker(&mut store), 3);
    }
    #[test]
    fn repeated_failed_rollovers_collect_only_unpublished_whole_packs() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false); let first = capture(1); let second = capture(2);
        let mut store = SessionStore::open(&directory.0).unwrap(); checkpoint(&mut store, &first, &cancel).unwrap();
        let owner = resource_owner(&store, tile_resource(&first).payload.id(), tile_resource(&first).bytes);
        OpenOptions::new().write(true).open(&owner.path).unwrap().set_len(PACK_BYTES).unwrap();
        for _ in 0..10 {
            assert!(checkpoint_using(&mut store, &second, &cancel, Interrupted { operation: 2, after: false, count: 0 }).is_err());
            assert_eq!(count(&directory.0.join("packs")), 2); assert_eq!(marker(&mut store), 1);
        }
        checkpoint(&mut store, &second, &cancel).unwrap(); assert_eq!(count(&directory.0.join("packs")), 2); assert_eq!(marker(&mut store), 2);
    }
    #[test]
    fn a_resource_retained_only_by_previous_is_reused_without_append() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false); let first = capture(1);
        let mut store = SessionStore::open(&directory.0).unwrap(); checkpoint(&mut store, &first, &cancel).unwrap();
        let owner = resource_owner(&store, tile_resource(&first).payload.id(), tile_resource(&first).bytes);
        checkpoint(&mut store, &capture(2), &cancel).unwrap(); let length = fs::metadata(&owner.path).unwrap().len();
        let mut operations = Vec::new(); checkpoint_using(&mut store, &first, &cancel, Operations(&mut operations)).unwrap();
        assert_eq!(operations.len(), 2); assert_eq!(fs::metadata(&owner.path).unwrap().len(), length); assert_eq!(marker(&mut store), 1);
    }
    #[cfg(unix)]
    #[test]
    fn verified_camera_checkpoint_does_not_open_or_read_payload_packs() {
        use std::os::unix::fs::PermissionsExt;
        let directory = Directory::new(); let cancel = AtomicBool::new(false); let prepared = capture(1);
        let mut store = SessionStore::open(&directory.0).unwrap(); checkpoint(&mut store, &prepared, &cancel).unwrap();
        let owner = resource_owner(&store, tile_resource(&prepared).payload.id(), tile_resource(&prepared).bytes);
        let permissions = fs::metadata(&owner.path).unwrap().permissions();
        fs::set_permissions(&owner.path, fs::Permissions::from_mode(0)).unwrap();
        assert!(File::open(&owner.path).is_err());
        let mut operations = Vec::new(); let result = checkpoint_using(&mut store, &prepared, &cancel, Operations(&mut operations));
        fs::set_permissions(&owner.path, permissions).unwrap();
        result.unwrap(); assert_eq!(operations.len(), 2); assert_eq!(marker(&mut store), 1);
    }
    #[test]
    fn rollover_keeps_previous_pack_and_live_ranges() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let original = capture(1); let second = capture(2);
        let mut store = SessionStore::open(&directory.0).unwrap(); checkpoint(&mut store, &original, &cancel).unwrap();
        let owner = resource_owner(&store, tile_resource(&original).payload.id(), tile_resource(&original).bytes);
        OpenOptions::new().write(true).open(&owner.path).unwrap().set_len(PACK_BYTES).unwrap();
        checkpoint(&mut store, &second, &cancel).unwrap();
        let next = resource_owner(&store, tile_resource(&second).payload.id(), tile_resource(&second).bytes);
        assert_ne!(owner.path, next.path); assert_eq!(count(&directory.0.join("packs")), 2);
        let pack=owner.location.backing().unwrap();
        let RangeState::Ready(bytes)=pack.read_on_worker(0,owner.length as usize).map(RangeState::Ready).unwrap() else {panic!("worker read must complete")};
        assert_eq!(bytes.len(),owner.length as usize);
        let next_path = next.path.clone();
        drop(next); drop(second);
        store.retire().unwrap(); assert!(owner.path.is_file()); assert!(!next_path.exists());
    }
    #[test]
    fn corrupt_pack_extents_ranges_and_aliases_are_rejected() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let mut store = SessionStore::open(&directory.0).unwrap();
        let generation = checkpoint(&mut store, &capture(1), &cancel).unwrap();
        let bytes = fs::read(store.generation_path(generation)).unwrap();
        let base: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for corruption in 0..5 {
            let mut value = base.clone();
            let pack = parse_pack(value["session"]["resources"][0]["location"]["pack"].as_str().unwrap()).unwrap().to_string();
            match corruption {
                0 => value["session"]["resources"][0]["location"]["offset"] = u64::MAX.to_string().into(),
                1 => value["extents"][&pack] = 0.into(),
                2 => { value["extents"].as_object_mut().unwrap().remove(&pack); },
                3 => { value["session"]["resources"][0]["location"].as_object_mut().unwrap().remove("pack"); },
                _ => { value["extents"][PortableId::random().to_string()] = 1.into(); },
            }
            assert!(StoredGeneration::parse(&serde_json::to_vec(&value).unwrap(), ProjectLimits::default()).is_err());
        }
    }
    #[cfg(windows)]
    #[test]
    fn competing_owner_at_directory_rename_preserves_the_original_session() {
        struct Reclaim { owner: Option<OwnerLock>, path: PathBuf, competing: Arc<Mutex<Option<OwnerLock>>> }
        impl Drop for Reclaim {
            fn drop(&mut self) {
                drop(self.owner.take());
                *self.competing.lock().unwrap() = Some(OwnerLock::claim(&self.path).unwrap().unwrap());
            }
        }
        let root = Directory::new(); let source = root.0.join("scene"); create_directory(&source).unwrap();
        fs::write(source.join("window.json"), b"original membership").unwrap();
        let mut store = SessionStore::open(&source.join("drawing")).unwrap();
        checkpoint(&mut store, &capture(1), &AtomicBool::new(false)).unwrap(); drop(store);
        let head = fs::read(source.join("drawing/head.json")).unwrap();
        let competing = Arc::new(Mutex::new(None));
        assert!(import_session_directories_with_owner(&root.0, "window.json", ".lock", |path| {
            Ok(OwnerLock::claim(path)?.map(|owner| Reclaim { owner: Some(owner), path: path.into(), competing: competing.clone() }))
        }).is_err());
        assert!(competing.lock().unwrap().is_some());
        assert!(OwnerLock::claim(&source.join(".lock")).unwrap().is_none());
        assert!(!root.0.join("packed/scene").exists());
        assert_eq!(fs::read(source.join("window.json")).unwrap(), b"original membership");
        assert_eq!(fs::read(source.join("drawing/head.json")).unwrap(), head);
    }
    #[test]
    fn native_namespace_import_preserves_live_and_downgrade_sessions() {
        let root = Directory::new(); create_directory(&root.0).unwrap();
        let old = root.0.join("scene"); create_directory(&old).unwrap();
        fs::write(old.join("window.json"), b"original membership").unwrap();
        let first = capture(1);
        let mut store = SessionStore::open(&old.join("drawing")).unwrap(); checkpoint(&mut store, &first, &AtomicBool::new(false)).unwrap();
        let live = OwnerLock::claim(&old.join(".lock")).unwrap().unwrap();
        let mut callbacks = 0;
        import_session_directories_with_owner(&root.0, "window.json", ".lock", |path| {
            assert_eq!(path, old.join(".lock")); callbacks += 1; Ok(None::<OwnerLock>)
        }).unwrap();
        assert_eq!(callbacks, 1); assert!(OwnerLock::claim(&old.join(".lock")).unwrap().is_none());
        let packed = import_session_directories(&root.0, "window.json", ".lock").unwrap();
        assert!(old.is_dir()); assert!(!packed.join("scene").exists()); drop(live);
        assert_eq!(import_session_directories(&root.0, "window.json", ".lock").unwrap_err(), "Recovery drawing is already open");
        let reader = resource_owner(&store, tile_resource(&first).payload.id(), tile_resource(&first).bytes); drop(store); drop(first);
        assert_eq!(import_session_directories(&root.0, "window.json", ".lock").unwrap_err(), "Recovery drawing is already open"); drop(reader);
        import_session_directories(&root.0, "window.json", ".lock").unwrap();
        assert_eq!(fs::read(packed.join("scene/window.json")).unwrap(), b"original membership");
        assert!(!old.exists());
        create_directory(&old).unwrap(); fs::write(old.join("window.json"), b"downgrade latest membership").unwrap();
        assert_eq!(import_session_directories(&root.0, "window.json", ".lock").unwrap_err(), "The saved session exists in both versions; both copies were kept");
        assert_eq!(fs::read(packed.join("scene/window.json")).unwrap(), b"original membership");
        assert_eq!(fs::read(old.join("window.json")).unwrap(), b"downgrade latest membership");
        fs::remove_file(old.join("window.json")).unwrap(); fs::remove_file(old.join(".lock")).unwrap(); fs::remove_dir(&old).unwrap();
        let upgraded = packed.join("scene/drawing");
        let mut upgraded_store = SessionStore::open(&upgraded).unwrap();
        let mut opened = upgraded_store.load(ProjectLimits::default(), &AtomicBool::new(false)).unwrap().unwrap();
        let target = opened.editor.document().working.target.unwrap();
        let descriptor = RasterPlane::Color.descriptor(Default::default());
        let tile = RasterTile::backed(TileBlob::encode(descriptor, &vec![42; 256 * 256 * 4]).unwrap());
        opened.editor.perform(crate::Edit::SetRaster { target, revision: RasterRevision::backed(RasterData {
            tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }, tile)].into(), ..Default::default()
        }) }).unwrap();
        let capture = opened.editor.capture_session(opened.editor.capture(11, Default::default()).unwrap()).unwrap();
        checkpoint(&mut upgraded_store, &captured(&capture, json!({"marker":42})), &AtomicBool::new(false)).unwrap();
        drop(opened); drop(upgraded_store);
        let newest = session_history(&upgraded);
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &AtomicBool::new(false)).unwrap();
        assert_eq!(session_history(&upgraded), newest);
        let downgrade = root.0.join("new-legacy-window"); create_directory(&downgrade).unwrap();
        fs::write(downgrade.join("window.json"), b"fresh downgrade membership").unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat/1.0.15/session");
        copy_fixture(&fixture, &downgrade.join("drawing"));
        let legacy = session_history(&downgrade.join("drawing"));
        import_session_directories(&root.0, "window.json", ".lock").unwrap();
        assert_eq!(session_history(&upgraded), newest);
        assert_eq!(session_history(&packed.join("new-legacy-window/drawing")), legacy);
        assert!(packed.join("new-legacy-window/window.json").is_file());
    }
    #[test]
    fn packed_checkpoints_preserve_all_sample_depths_and_auxiliary_planes() {
        use crate::color::SampleDepth;
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
            let directory = Directory::new(); let cancel = AtomicBool::new(false);
            let mut document = Document::new(PortableId::random(), 256, 256, DocumentNames { paint: "ink".into(), paper: "paper".into() });
            document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = depth;
            let color = document.composition().color;
            let tiles = [RasterPlane::Color, RasterPlane::WatercolorWetness].into_iter().map(|plane| {
                let descriptor = plane.descriptor(color);
                let sample = if plane != RasterPlane::Color { vec![0x34; descriptor.bits_per_channel as usize / 8] }
                    else { match depth { SampleDepth::U8 => vec![42], SampleDepth::U16 => vec![0x34, 0x12],
                        SampleDepth::F16 => half::f16::from_f32(0.25).to_le_bytes().to_vec(), SampleDepth::F32 => 0.25f32.to_le_bytes().to_vec() } };
                let bytes = sample.repeat(descriptor.byte_len([256; 2]).unwrap() / sample.len());
                (TileKey { plane, coordinate: [0, 0] }, RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()))
            }).collect();
            *document.target_raster_mut(document.working.target.unwrap()).unwrap() = RasterRevision::backed(RasterData {
                tiles, watercolor: Some(crate::raster::RasterWatercolor { wet_edge: 0.25, burnt_edge: 0.5, edge_width: 2. }),
            });
            let editor = Editor::new(document);
            let expected = editor.document().target_raster(editor.document().working.target.unwrap()).unwrap().wait_data().unwrap();
            let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
            let prepared = captured(&capture, json!({}));
            let mut store = SessionStore::open(&directory.0).unwrap(); checkpoint(&mut store, &prepared, &cancel).unwrap(); drop(store);
            let mut store = SessionStore::open(&directory.0).unwrap();
            let restored = store.load(ProjectLimits::default(), &cancel).unwrap().unwrap();
            let document = restored.editor.document(); assert_eq!(document.composition().color.depth, depth);
            let actual = document.target_raster(document.working.target.unwrap()).unwrap().wait_data().unwrap();
            assert_eq!(actual.watercolor, expected.watercolor); assert_eq!(actual.tiles.len(), expected.tiles.len());
            for (key, tile) in &expected.tiles {
                let a = actual.tiles[key].wait_backing().unwrap(); let b = tile.wait_backing().unwrap();
                assert_eq!(a.descriptor, b.descriptor); assert_eq!(a.resource_id(), b.resource_id()); assert_eq!(a.decode_on_worker().unwrap(), b.decode_on_worker().unwrap());
            }
        }
    }
    #[test]
    fn opaque_resources_preserve_empty_ranges_and_seal_oversized_packs() {
        for length in [0, PACK_BYTES + 1_024] {
            struct Repeated(u64);
            impl ByteSource for Repeated {
                fn byte_len(&self) -> u64 { self.0 }
                fn poll(&self, offset: u64, length: usize) -> Result<RangeState, String> {
                    if offset.checked_add(length as u64).is_none_or(|end| end > self.0) { return Err("Repeat range exceeds source".into()); }
                    Ok(RangeState::Ready(ByteRange::new(vec![37; length].into(), 0..length)?))
                }
            }
            let directory = Directory::new(); let cancel = AtomicBool::new(false);
            let backing = ImmutableBacking::new(Arc::new(Repeated(length))).unwrap();
            let mut checksum = crc32fast::Hasher::new(); let chunk = vec![37; 1024];
            for _ in 0..length / 1024 { checksum.update(&chunk); }
            let id = PortableId::random();
            let opaque = Arc::new(crate::authored::OpaqueResource { id, kind: "future.attachment/1".into(), data: json!({}), encoding: "future.codec/1".into(),
                extra_fields: Default::default(), backing, offset: 0, length, crc32: checksum.finalize() });
            let mut document = Document::new(PortableId::random(), 256, 256, DocumentNames { paint: "ink".into(), paper: "paper".into() });
            document.artwork.extensions = Arc::new(crate::authored::Extensions { resources: [(id, opaque)].into(), ..Default::default() });
            let editor = Editor::new(document);
            let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
            let prepared = captured(&capture, json!({}));
            let mut store = SessionStore::open(&directory.0).unwrap(); checkpoint(&mut store, &prepared, &cancel).unwrap();
            let owner = resource_owner(&store, id, length); assert_eq!(owner.offset, 0); assert_eq!(fs::metadata(&owner.path).unwrap().len(), length.max(1));
            if length == 0 {
                let path = owner.path.clone();
                drop((owner, prepared, capture, editor, store));
                let missing = path.with_extension("held"); fs::rename(&path, &missing).unwrap();
                let mut store = SessionStore::open(&directory.0).unwrap();
                let failed = store.load(ProjectLimits::default(), &cancel).is_err();
                fs::rename(missing, &path).unwrap(); assert!(failed);
                let restored = store.load(ProjectLimits::default(), &cancel).unwrap().unwrap();
                assert_eq!(restored.editor.document().artwork.extensions.resources[&id].length, 0);
                checkpoint(&mut store, &self::capture(2), &cancel).unwrap();
                continue;
            }
            let restored = store.load(ProjectLimits::default(), &cancel).unwrap().unwrap();
            let resource = &restored.editor.document().artwork.extensions.resources[&id];
            if length != 0 { assert_eq!(&*resource.read_chunk(length - 1024, 1024, &cancel).unwrap(), chunk.as_slice()); }
            checkpoint(&mut store, &self::capture(2), &cancel).unwrap();
            if length > PACK_BYTES { assert_eq!(fs::metadata(&owner.path).unwrap().len(), length); }
        }
    }
    #[test]
    fn v1_0_14_paint_session_preserves_resources_history_and_identities() {
        let fixture=Path::new(env!("CARGO_MANIFEST_DIR")).join("src/package/codec/fixtures/v1.0.14-paint-session");
        let directory=Directory::new();let cancel=AtomicBool::new(false);
        copy_fixture(&fixture, &directory.0);
        let head:Head=serde_json::from_slice(&fs::read(directory.0.join("head.json")).unwrap()).unwrap();
        let metadata:serde_json::Value=serde_json::from_slice(&fs::read(directory.0.join("generations").join(format!("{}.json",head.current.id))).unwrap()).unwrap();
        assert!(metadata["layout"].get("vectors").is_none());
        let mut store=SessionStore::open(&directory.0).unwrap();
        let mut restored=store.load(ProjectLimits::default(),&cancel).unwrap().unwrap();
        assert_eq!(store.loaded_generation(),Some(head.current.id));assert!(!store.recovered_previous());
        let document=restored.editor.document();
        assert_eq!(document.artwork.id.to_string(),"cb36dabee8e7459b89932a0dc0ebe7a7");
        assert_eq!(document.composition().size,[1024,1024]);assert!(document.artwork.vectors.is_empty());
        assert_eq!(document.artwork.occurrences.iter().map(|(h,id,o)|(h.index(),id.to_string(),o.name.as_ref())).collect::<Vec<_>>(),vec![
            (0,"be8351c98c2540a48d9307ab0108bebb".into(),"Upgrade source"),
            (1,"a9335685506d4224bee788264243dff8".into(),"Paper")]);
        let paint=match document.working.target.unwrap() {crate::SourceTarget::Paint(handle)=>handle,_=>panic!("Expected editable Paint layer")};
        assert_eq!(paint.index(),0);
        assert_eq!(document.artwork.paint.id(paint).unwrap().to_string(),"c7640099209e4fee9080371060ec828a");
        let image=document.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.clone();
        assert_eq!(image.id().to_string(),"983fa80a28bd4dea8fa20d29713b49fa");assert_eq!(image.extent,[1024,1024]);
        let resource_ids=fs::read_dir(fixture.join("resources")).unwrap().map(|entry|entry.unwrap().path().file_stem().unwrap().to_str().unwrap().parse::<PortableId>().unwrap()).collect::<BTreeSet<_>>();
        assert_eq!(image.tiles.values().map(|tile|tile.resource_id()).collect::<BTreeSet<_>>(),resource_ids);assert_eq!(resource_ids.len(),16);
        let samples=|image:&crate::Image| {
            let mut rows=image.rows();let mut row=vec![0;image.row_bytes()];
            rows.read(0,&mut row).unwrap();assert_eq!(&row[..3],&[118,118,118]);
            rows.read(512,&mut row).unwrap();assert_eq!(&row[512*3..513*3],&[246,245,244]);
        };
        samples(&image);
        let artwork=document.artwork.clone();let working=document.working.clone();
        assert_eq!(restored.editor.checkpoint(),1);assert!(restored.editor.can_undo());assert!(!restored.editor.can_redo());
        restored.editor.undo().unwrap();assert_eq!(restored.editor.checkpoint(),0);assert!(!restored.editor.can_undo());assert!(restored.editor.can_redo());
        restored.editor.redo().unwrap();assert_eq!(restored.editor.checkpoint(),1);assert_eq!(restored.editor.document().artwork,artwork);
        assert_eq!(restored.editor.document().working,working);
        assert_eq!(restored.metadata.value["saved_checkpoint"],1);
        assert_eq!(restored.metadata.value["location"]["uri"],"fixtures/v1.0.14/Upgrade drawing.capy");
        let metadata=restored.metadata.value.clone();
        let capture=restored.editor.capture_session(restored.editor.capture(2,Default::default()).unwrap()).unwrap();
        let prepared=captured(&capture, restored.metadata);
        let current:serde_json::Value=serde_json::from_slice(prepared.metadata()).unwrap();assert_eq!(current["layout"]["vectors"],json!([]));
        let generation=checkpoint(&mut store, &prepared,&cancel).unwrap();assert_ne!(generation,head.current.id);drop(store);
        let mut reopened=SessionStore::open(&directory.0).unwrap();
        let mut restored=reopened.load(ProjectLimits::default(),&cancel).unwrap().unwrap();
        assert_eq!(reopened.loaded_generation(),Some(generation));assert_eq!(restored.metadata.value,metadata);
        assert_eq!(restored.editor.document().artwork,artwork);assert_eq!(restored.editor.document().working,working);
        samples(&restored.editor.document().artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image);
        restored.editor.undo().unwrap();assert_eq!(restored.editor.checkpoint(),0);
        restored.editor.redo().unwrap();assert_eq!(restored.editor.checkpoint(),1);assert_eq!(restored.editor.document().artwork,artwork);
    }
    #[test]
    fn custom_authority_is_local_to_committed_generations_and_survives_fallback() {
        let root=Directory::new();let cancel=AtomicBool::new(false);
        let mut editor=Editor::new(Document::new(PortableId::random(),19,11,DocumentNames {paint:"ink".into(),paper:"paper".into()}));
        let handle=editor.document().artwork.effects.iter().next().unwrap().0;
        let program=crate::effect_catalog::custom_program("solid_color");
        let values=crate::EffectInstance::new(program.clone()).values;
        let application=crate::EffectApplication::new(program,values,editor.document().composition().size);
        editor.perform(crate::Edit::Effect(crate::RecordChange::replace(&editor.document().artwork.effects,handle,Some(application)).unwrap())).unwrap();
        let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
        let prepared=captured(&capture, json!({"marker":42}));
        let mut store=SessionStore::open(&root.0).unwrap();
        let first=checkpoint(&mut store, &prepared,&cancel).unwrap();
        assert!(store.load(ProjectLimits::default(),&cancel).unwrap().is_some());
        let authority=store.programs.keys().copied().collect::<Vec<_>>();
        assert!(checkpoint_using(&mut store, &prepared,&cancel,Interrupted {operation:1,after:false,count:0}).is_err());
        assert_eq!(store.programs.keys().copied().collect::<Vec<_>>(),authority);
        let second=checkpoint(&mut store, &prepared,&cancel).unwrap();
        let first_bytes=fs::read(store.generation_path(first)).unwrap();
        let second_bytes=fs::read(store.generation_path(second)).unwrap();
        fs::write(store.generation_path(second),b"corrupt metadata").unwrap();
        assert!(store.load(ProjectLimits::default(),&cancel).unwrap().is_some());
        assert_eq!(store.loaded_generation(),Some(first));
        fs::write(store.generation_path(second),&second_bytes).unwrap();
        drop(store);
        let mut reopened=SessionStore::open(&root.0).unwrap();
        assert!(reopened.load(ProjectLimits::default(),&cancel).err().unwrap().contains("not approved"));
        assert_eq!(fs::read(reopened.generation_path(first)).unwrap(),first_bytes);
        assert_eq!(fs::read(reopened.generation_path(second)).unwrap(),second_bytes);
    }
    #[test]
    fn unapproved_generation_never_silently_restores_an_older_builtin_checkpoint() {
        let root=Directory::new();let cancel=AtomicBool::new(false);
        let mut editor=Editor::new(Document::new(PortableId::random(),19,11,DocumentNames {paint:"ink".into(),paper:"paper".into()}));
        let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
        let builtin=captured(&capture, json!({"marker":1}));
        let mut store=SessionStore::open(&root.0).unwrap();checkpoint(&mut store, &builtin,&cancel).unwrap();
        let handle=editor.document().artwork.effects.iter().next().unwrap().0;
        let program=crate::effect_catalog::custom_program("solid_color");let values=crate::EffectInstance::new(program.clone()).values;
        let application=crate::EffectApplication::new(program,values,editor.document().composition().size);
        editor.perform(crate::Edit::Effect(crate::RecordChange::replace(&editor.document().artwork.effects,handle,Some(application)).unwrap())).unwrap();
        let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
        let custom=captured(&capture, json!({"marker":2}));
        let generation=checkpoint(&mut store, &custom,&cancel).unwrap();
        let metadata=fs::read(store.generation_path(generation)).unwrap();
        let head=fs::read(root.0.join("head.json")).unwrap();drop(store);
        let mut store=SessionStore::open(&root.0).unwrap();
        assert!(store.load(ProjectLimits::default(),&cancel).err().unwrap().contains("not approved"));
        assert_eq!(store.loaded_generation(),None);assert_eq!(store.generation(),Some(generation));
        assert_eq!(fs::read(store.generation_path(generation)).unwrap(),metadata);
        assert_eq!(fs::read(root.0.join("head.json")).unwrap(),head);
    }
    fn count(path: &Path) -> usize { match fs::read_dir(path) { Ok(entries) => entries.count(), Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0, Err(error) => panic!("{error}") } }
    struct ReaderLease {location:Arc<StoreLocation>,path:PathBuf}
    impl std::ops::Deref for ReaderLease {type Target=StoreLocation;fn deref(&self)->&Self::Target {&self.location}}
    fn resource_owner(store:&SessionStore,id:PortableId,length:u64)->ReaderLease {
        let location=store.index[&id].location.clone();assert_eq!(location.length,length);
        ReaderLease {path:store.pack_path(location.pack.id),location}
    }
    fn tile_resource(prepared: &PreparedSession) -> &super::super::resources::PreparedResource {
        prepared.resources().entries.iter().find(|entry| matches!(entry.payload, Payload::Tile(_))).unwrap()
    }
    fn marker(store: &mut SessionStore) -> u8 {
        store.load(ProjectLimits::default(), &AtomicBool::new(false)).unwrap().unwrap().metadata.value["marker"].as_u64().unwrap() as u8
    }

    struct Interrupted { operation: usize, after: bool, count: usize }
    impl Publication for Interrupted {
        fn append(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
            self.count += 1;
            let interrupt = self.count == self.operation;
            append_pack(path, |file| {
                write(file)?;
                if interrupt && !self.after { return Err("Interrupted append".into()); }
                Ok(())
            }, ready)?;
            if interrupt { return Err("Interrupted append sync".into()); }
            Ok(())
        }
        fn publish(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), AtomicReplaceError> {
            self.count += 1;
            let interrupt = self.count == self.operation;
            if interrupt && !self.after {
                return atomic_publish_checked(path, |file| { write(file)?; Err("Interrupted write".into()) }, ready);
            }
            atomic_publish_checked(path, write, ready)?;
            if interrupt { return Err(AtomicReplaceError { published: true, error: "Interrupted publication".into() }); }
            Ok(())
        }
    }

    #[test]
    fn every_write_interruption_restores_a_complete_checkpoint() {
        let old = capture(11);
        let new = capture(22);
        let operations = 3;
        for after in [false, true] {
            for operation in 1..=operations {
                let directory = Directory::new();
                let mut store = SessionStore::open(&directory.0).unwrap();
                checkpoint(&mut store, &old, &AtomicBool::new(false)).unwrap();
                let failure = checkpoint_using(&mut store, &new, &AtomicBool::new(false), Interrupted { operation, after, count: 0 });
                assert!(failure.is_err());
                drop(store);
                let mut restored = SessionStore::open(&directory.0).unwrap();
                assert_eq!(marker(&mut restored), if after && operation == operations { 22 } else { 11 });
                assert_eq!(count(&directory.0.join("generations")), if after && operation == operations { 2 } else { 1 });
                assert_eq!(count(&directory.0.join("packs")), 1);
            }
        }
    }

    #[test]
    fn process_death_mid_write_keeps_the_original_and_cleans_temporary_files() {
        if let Some(directory) = std::env::var_os("CAPY_SESSION_CRASH_DIRECTORY") {
            let path = PathBuf::from(directory);
            let store = SessionStore::open(&path).unwrap();
            let target = match std::env::var("CAPY_SESSION_CRASH_STAGE").unwrap().as_str() {
                "resources" => store.pack_path(PortableId::random()),
                "generations" => store.generation_path(PortableId::random()),
                _ => path.join("head.json"),
            };
            let _ = atomic_publish(&target, |file| {
                file.write_all(b"partial write").unwrap();
                file.flush().unwrap();
                file.get_ref().sync_all().unwrap();
                std::process::exit(87);
            }, || Ok(()));
            panic!("Crash worker returned");
        }
        for stage in ["resources", "generations", "head"] {
            let directory = Directory::new();
            let mut store = SessionStore::open(&directory.0).unwrap();
            let original = capture(7);
            checkpoint(&mut store, &original, &AtomicBool::new(false)).unwrap();
            drop(store); drop(original);
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "package::session_store::tests::process_death_mid_write_keeps_the_original_and_cleans_temporary_files", "--test-threads=1"])
                .env("CAPY_SESSION_CRASH_DIRECTORY", &directory.0)
                .env("CAPY_SESSION_CRASH_STAGE", stage)
                .status().unwrap();
            assert_eq!(status.code(), Some(87));
            let mut store = SessionStore::open(&directory.0).unwrap();
            assert_eq!(marker(&mut store), 7);
            assert_eq!(count(&directory.0.join("packs")), 1);
            assert_eq!(count(&directory.0.join("generations")), 1);
            assert_eq!(count(&directory.0), 5);
        }
    }

    #[test]
    fn publication_error_after_head_does_not_collect_the_new_checkpoint() {
        let directory = Directory::new();
        let mut store = SessionStore::open(&directory.0).unwrap();
        let old = capture(1);
        let previous = checkpoint(&mut store, &old, &AtomicBool::new(false)).unwrap();
        let new = capture(2);
        assert!(checkpoint_using(&mut store, &new, &AtomicBool::new(false), Interrupted { operation: 3, after: true, count: 0 }).is_err());
        assert_ne!(store.generation(), Some(previous)); assert_eq!(store.loaded_generation(), store.generation());
        assert!(store.programs.contains_key(&store.generation().unwrap()));
        store.collect_garbage().unwrap();
        assert_eq!(marker(&mut store), 2);
        let third = capture(3);
        checkpoint(&mut store, &third, &AtomicBool::new(false)).unwrap();
        assert_eq!(marker(&mut store), 3);
        assert_eq!(count(&directory.0.join("generations")), 2);
        assert_eq!(count(&directory.0.join("packs")), 1);
    }

    #[test]
    fn repeated_changes_keep_only_current_previous_and_live_readers() {
        let root = Directory::new(); let directory = Directory(root.0.join("drawing"));
        let mut store = SessionStore::open(&directory.0).unwrap();
        let original = capture(1);
        checkpoint(&mut store, &original, &AtomicBool::new(false)).unwrap();
        let resource = tile_resource(&original);
        let live = resource_owner(&store, resource.payload.id(), resource.bytes);
        for value in 2..20 {
            checkpoint(&mut store, &capture(value), &AtomicBool::new(false)).unwrap();
            assert!(count(&directory.0.join("generations")) <= 2);
            assert!(count(&directory.0.join("packs")) <= 3);
        }
        let pack=live.location.backing().unwrap();
        let RangeState::Ready(bytes)=pack.read_on_worker(0,live.length as usize).map(RangeState::Ready).unwrap() else {panic!("worker read must complete")};
        assert_eq!(bytes.len(),live.length as usize);
        store.retire().unwrap();
        assert_eq!(count(&directory.0.join("packs")), 1);
        drop(store);
        assert!(OwnerLock::claim(&directory.0.join(".lock")).unwrap().is_none());
        assert!(SessionStore::open(&directory.0).is_err());
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &AtomicBool::new(false)).unwrap();
        assert_eq!(count(&directory.0.join("packs")), 1);
        drop(bytes); drop(pack); drop(live); drop(original);
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &AtomicBool::new(false)).unwrap();
        assert_eq!(count(&directory.0.join("packs")), 0);
        assert_eq!(count(&directory.0.join("generations")), 0);
        assert!(directory.0.join(".lock").exists());
    }

    #[test]
    fn corrupt_current_falls_back_and_unsupported_current_stays_preserved() {
        for corruption in 0..4 {
            let directory = Directory::new();
            let mut store = SessionStore::open(&directory.0).unwrap();
            let previous = checkpoint(&mut store, &capture(1), &AtomicBool::new(false)).unwrap();
            let second = capture(2);
            let generation = checkpoint(&mut store, &second, &AtomicBool::new(false)).unwrap();
            match corruption {
                0 => {
                    let owner = resource_owner(&store, tile_resource(&second).payload.id(), tile_resource(&second).bytes);
                    let mut file = OpenOptions::new().write(true).open(&owner.path).unwrap();
                    use std::io::{Seek, SeekFrom};
                    file.seek(SeekFrom::Start(owner.offset)).unwrap(); file.write_all(b"broken").unwrap();
                }
                1 => fs::write(store.generation_path(generation), b"broken").unwrap(),
                3 => {
                    let path = store.generation_path(generation);
                    let metadata = fs::read(&path).unwrap();
                    fs::write(path,&metadata[..metadata.len()/2]).unwrap();
                }
                _ => {
                    let mut metadata: serde_json::Value = serde_json::from_slice(&fs::read(store.generation_path(generation)).unwrap()).unwrap();
                    let resource = metadata["session"]["resources"].as_array_mut().unwrap().iter_mut().find(|record| record["id"] == json!(tile_resource(&second).payload.id())).unwrap();
                    resource["encoding"] = "unsupported".into();
                    let metadata = serde_json::to_vec(&metadata).unwrap();
                    fs::write(store.generation_path(generation), &metadata).unwrap();
                    let mut head = store.head.clone().unwrap();head.current=Generation::new(generation,&metadata);
                    fs::write(store.path.join("head.json"), serde_json::to_vec(&head).unwrap()).unwrap();
                }
            }
            let metadata = fs::read(store.generation_path(generation)).unwrap();
            let head = fs::read(store.path.join("head.json")).unwrap();
            drop(store);
            let mut store = SessionStore::open(&directory.0).unwrap();
            if corruption==0 {
                let opened = store.load(ProjectLimits::default(), &AtomicBool::new(false)).unwrap().unwrap();
                assert_eq!(store.loaded_generation(), Some(generation));
                assert_eq!(store.generation(), Some(generation));
                let blobs = opened.editor.retained_tiles().blobs().unwrap();
                assert_eq!(blobs.len(), 1);
                assert_eq!(blobs[0].decode_on_worker().unwrap_err(), "Recovery resource checksum failed");
                assert_eq!(fs::read(store.generation_path(generation)).unwrap(), metadata);
                assert_eq!(fs::read(store.path.join("head.json")).unwrap(), head);
                continue;
            }
            if corruption==2 {
                assert_eq!(store.load(ProjectLimits::default(),&AtomicBool::new(false)).err().unwrap(),"Unsupported session artwork");
                assert_eq!(store.loaded_generation(),None);assert_eq!(store.generation(),Some(generation));
                assert_eq!(fs::read(store.generation_path(generation)).unwrap(),metadata);
                assert_eq!(fs::read(store.path.join("head.json")).unwrap(),head);
                continue;
            }
            assert_eq!(marker(&mut store), 1);
            assert_eq!(store.loaded_generation(),Some(previous));
            assert_eq!(fs::read(store.generation_path(generation)).unwrap(),metadata);
            assert_eq!(fs::read(store.path.join("head.json")).unwrap(),head);
            checkpoint(&mut store, &capture(3), &AtomicBool::new(false)).unwrap();
            assert_eq!(marker(&mut store), 3);
            assert_eq!(count(&directory.0.join("generations")), 2);
            assert_eq!(count(&directory.0.join("packs")), 1);
        }
    }

    #[test]
    fn future_session_schema_is_terminal_before_reading_its_resource_inventory() {
        let directory=Directory::new();let cancel=AtomicBool::new(false);
        let mut store=SessionStore::open(&directory.0).unwrap();checkpoint(&mut store, &capture(1),&cancel).unwrap();
        let generation=checkpoint(&mut store, &capture(2),&cancel).unwrap();
        let metadata=serde_json::to_vec(&json!({"format":"capy.session","version":2,"future_field":[]})).unwrap();
        fs::write(store.generation_path(generation),&metadata).unwrap();
        let mut head=store.head.clone().unwrap();head.current=Generation::new(generation,&metadata);
        let head=serde_json::to_vec(&head).unwrap();fs::write(store.path.join("head.json"),&head).unwrap();drop(store);
        let mut store=SessionStore::open(&directory.0).unwrap();
        assert_eq!(store.load(ProjectLimits::default(),&cancel).err().unwrap(),"Unsupported session format");
        assert_eq!(store.loaded_generation(),None);assert_eq!(store.generation(),Some(generation));
        assert_eq!(fs::read(store.generation_path(generation)).unwrap(),metadata);
        assert_eq!(fs::read(store.path.join("head.json")).unwrap(),head);
    }
    #[test]
    fn verified_reuse_does_not_reread_and_corruption_fails_on_first_read() {
        let directory = Directory::new();
        let capture = capture(9);
        let mut store = SessionStore::open(&directory.0).unwrap();
        checkpoint(&mut store, &capture, &AtomicBool::new(false)).unwrap();
        let path = resource_owner(&store, tile_resource(&capture).payload.id(), tile_resource(&capture).bytes).path.clone();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        checkpoint(&mut store, &capture, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        drop(store);
        let mut store = SessionStore::open(&directory.0).unwrap();
        let mut bytes = fs::read(&path).unwrap(); bytes[0] ^= 1; fs::write(&path, &bytes).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        checkpoint(&mut store, &capture, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let head = fs::read(directory.0.join("head.json")).unwrap();
        let opened = store.load(ProjectLimits::default(), &AtomicBool::new(false)).unwrap().unwrap();
        let blobs = opened.editor.retained_tiles().blobs().unwrap();
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0].decode_on_worker().unwrap_err(), "Recovery resource checksum failed");
        assert_eq!(fs::read(directory.0.join("head.json")).unwrap(), head);
    }

    #[test]
    fn ownership_and_cancelled_retirement_preserve_the_only_copy() {
        let directory = Directory::new();
        let mut store = SessionStore::open(&directory.0).unwrap();
        assert!(SessionStore::open(&directory.0).is_err());
        let saved = capture(5);
        checkpoint(&mut store, &saved, &AtomicBool::new(false)).unwrap();
        assert!(checkpoint(&mut store, &capture(6), &AtomicBool::new(true)).is_err());
        assert_eq!(marker(&mut store), 5);
        let file = directory.0.join("generations/.capy-save-interrupted");
        fs::write(&file, b"partial").unwrap();
        store.collect_garbage().unwrap();
        assert!(!file.exists());
        drop(store);
        let store = SessionStore::open(&directory.0).unwrap();
        assert_eq!(count(&directory.0.join("packs")), 1);
        drop(store);
    }

    #[test]
    fn duplicated_file_handle_does_not_extend_session_owner_lifetime() {
        let directory=Directory::new();create_directory(&directory.0).unwrap();let path=directory.0.join(".lock");
        let owner=OwnerLock::claim(&path).unwrap().unwrap();let inherited=owner._file.try_clone().unwrap();
        assert!(OwnerLock::claim(&path).unwrap().is_none());assert!(OwnerLock::claim(&path).unwrap().is_none());drop(owner);
        let replacement=OwnerLock::claim(&path).unwrap().expect("A duplicated file handle must not keep a retired session owner locked");
        drop(replacement);drop(inherited);
    }

    #[test]
    fn malformed_resource_ranges_are_rejected_before_opening_files() {
        let prepared = capture(1);
        let base: serde_json::Value = serde_json::from_slice(prepared.metadata()).unwrap();
        for corruption in 0..5 {
            let mut value = base.clone();
            match corruption {
                0 => value["resources"][0]["location"]["offset"] = "1".into(),
                1 => {
                    value["resources"][0]["bytes"] = u64::MAX.to_string().into();
                    let mut next=value["resources"][0].clone();
                    next["id"]=PortableId::random().to_string().into();
                    next["location"]["offset"]=u64::MAX.to_string().into();
                    next["bytes"]="1".into();
                    value["resources"].as_array_mut().unwrap().push(next);
                },
                2 => value["resources"][0]["id"] = "../elsewhere".into(),
                3 => { let record = value["resources"][0].clone(); value["resources"].as_array_mut().unwrap().push(record); },
                _ => value["resources"][0]["crc32"] = "FFFFFFFF".into(),
            }
            let error=resource_index(&serde_json::to_vec(&value).unwrap(), ProjectLimits::default()).err().expect("Malformed recovery range").message();
            if corruption==1 {assert_eq!(error,"Recovery resource range overflow");}
        }
    }

    #[test]
    fn orphan_sweep_preserves_membership_writers_and_reader_pins() {
        let root = Directory::new();
        let cancel = AtomicBool::new(false);
        let mut kept = SessionStore::open(&root.0.join("kept")).unwrap();
        checkpoint(&mut kept, &capture(1), &cancel).unwrap();
        let mut live = SessionStore::open(&root.0.join("writer")).unwrap();
        checkpoint(&mut live, &capture(2), &cancel).unwrap();
        let mut orphan = SessionStore::open(&root.0.join("orphan")).unwrap();
        let prepared = capture(3);
        checkpoint(&mut orphan, &prepared, &cancel).unwrap();
        let tile = tile_resource(&prepared);
        let reader = resource_owner(&orphan, tile.payload.id(), tile.bytes);
        orphan.retire().unwrap();
        let mut unpublished = SessionStore::open(&root.0.join("unpublished")).unwrap();
        checkpoint(&mut unpublished, &capture(4), &cancel).unwrap();
        drop(unpublished);
        let unrecognized = root.0.join("unrecognized");
        fs::create_dir(&unrecognized).unwrap(); fs::write(unrecognized.join("keep"), b"user bytes").unwrap();
        fs::write(orphan.path.join("head.json"), b"invalid").unwrap();
        drop(orphan); drop(kept);
        let reachable = ["kept".into()].into();
        assert!(collect_unreferenced_stores(&root.0, &reachable, &AtomicBool::new(true)).is_err());
        collect_unreferenced_stores(&root.0, &reachable, &cancel).unwrap();
        assert!(root.0.join("kept/head.json").is_file());
        assert!(root.0.join("writer/head.json").is_file());
        assert!(!root.0.join("orphan/head.json").exists());
        assert_eq!(count(&root.0.join("orphan/packs")), 1);
        assert!(reader.path.is_file());
        assert_eq!(fs::read(unrecognized.join("keep")).unwrap(), b"user bytes");
        assert!(root.0.join("unpublished/head.json").is_file());
        drop(reader); drop(live); drop(prepared);
        collect_unreferenced_stores(&root.0, &reachable, &cancel).unwrap();
        assert_eq!(count(&root.0.join("orphan/packs")), 0);
        assert!(root.0.join("writer/head.json").is_file());
        assert!(root.0.join("orphan/.lock").is_file());
    }

    #[test]
    fn durable_retirement_prevents_reopening_before_head_is_removed() {
        let root = Directory::new();
        let path = root.0.join("closed");
        let mut store = SessionStore::open(&path).unwrap();
        checkpoint(&mut store, &capture(1), &AtomicBool::new(false)).unwrap();
        atomic_replace(&path.join(".retired"), b"").unwrap();
        assert!(path.join("head.json").is_file());
        assert!(checkpoint(&mut store, &capture(2), &AtomicBool::new(false)).is_err());
        assert_eq!(count(&path.join("generations")), 1);
        drop(store);
        assert!(SessionStore::open(&path).is_err());
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &AtomicBool::new(false)).unwrap();
        assert!(!path.join("head.json").exists());
        assert_eq!(count(&path.join("resources")), 0);
        assert_eq!(count(&path.join("generations")), 0);
        assert!(SessionStore::open(&path).is_err());
        assert!(path.join(".retired").exists());
        assert_eq!(count(&path.join("generations")), 0);
    }

    #[test]
    fn close_intent_is_harmless_while_indexed_and_retires_after_membership_removal() {
        let root = Directory::new(); let path = root.0.join("closing");
        let cancel = AtomicBool::new(false);
        let mut store = SessionStore::open(&path).unwrap();
        checkpoint(&mut store, &capture(1), &cancel).unwrap();
        store.prepare_retirement().unwrap();
        drop(store);
        collect_unreferenced_stores(&root.0, &["closing".into()].into(), &cancel).unwrap();
        let mut store = SessionStore::open(&path).unwrap();
        assert_eq!(marker(&mut store), 1);
        checkpoint(&mut store, &capture(2), &cancel).unwrap();
        assert_eq!(marker(&mut store), 2);
        assert!(!path.join(".retired").exists());
        drop(store);
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &cancel).unwrap();
        assert_eq!(count(&path.join("resources")), 0);
        assert_eq!(count(&path.join("generations")), 0);
        assert!(SessionStore::open(&path).is_err());
    }

    #[test]
    fn explicit_discard_can_prepare_a_corrupt_store_without_decoding_it() {
        let root = Directory::new(); let path = root.0.join("corrupt");
        let cancel = AtomicBool::new(false);
        let mut store = SessionStore::open(&path).unwrap();
        checkpoint(&mut store, &capture(8), &cancel).unwrap();
        assert!(prepare_store_retirement(&path).is_err());
        drop(store);
        atomic_replace(&path.join("head.json"), b"invalid head").unwrap();
        assert!(SessionStore::open(&path).is_err());
        prepare_store_retirement(&path).unwrap();
        collect_unreferenced_stores(&root.0, &["corrupt".into()].into(), &cancel).unwrap();
        assert_eq!(read_bounded(&path.join("head.json"), 1024).unwrap(), b"invalid head");
        assert_eq!(count(&path.join("generations")), 1);
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &cancel).unwrap();
        assert_eq!(count(&path.join("generations")), 0);
        assert_eq!(count(&path.join("resources")), 0);
        assert!(retired_store(&path).unwrap());
        assert!(prepare_store_retirement(&root.0.join("unrecognized")).is_ok());
        assert!(!root.0.join("unrecognized").exists());
        create_directory(&root.0.join("unrecognized")).unwrap();
        assert!(prepare_store_retirement(&root.0.join("unrecognized")).is_err());
    }

    #[test]
    fn malformed_retirement_markers_stop_collection_even_without_a_head() {
        let root = Directory::new(); let cancel = AtomicBool::new(false);
        for marker in [".retired", ".retiring"] {
            let path = root.0.join(marker.trim_start_matches('.'));
            let mut store = SessionStore::open(&path).unwrap();
            checkpoint(&mut store, &capture(2), &cancel).unwrap();
            drop(store);
            remove_file(&path.join("head.json")).unwrap();
            atomic_replace(&path.join(marker), b"malformed").unwrap();
            assert!(SessionStore::open(&path).is_err());
            assert_eq!(count(&path.join("generations")), 1);
            assert!(collect_unreferenced_stores(&root.0, &BTreeSet::new(), &cancel).is_err());
            assert_eq!(count(&path.join("generations")), 1);
            assert_eq!(read_bounded(&path.join(marker), 16).unwrap(), b"malformed");
            fs::remove_dir_all(path).unwrap();
        }
    }

    #[test]
    fn valid_same_length_metadata_corruption_falls_back_before_decode_or_collection() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let mut store = SessionStore::open(&directory.0).unwrap();
        let previous = checkpoint(&mut store, &capture(1), &cancel).unwrap();
        let prepared = capture(2);
        let current = checkpoint(&mut store, &prepared, &cancel).unwrap();
        let original = fs::read(store.generation_path(current)).unwrap();
        let mut metadata: serde_json::Value = serde_json::from_slice(&original).unwrap();
        metadata["session"]["metadata"]["marker"] = 3.into();
        let damaged = serde_json::to_vec(&metadata).unwrap();
        assert_eq!(damaged.len(), original.len());
        assert_ne!(damaged, original);
        fs::write(store.generation_path(current), damaged).unwrap();
        assert!(store.collect_garbage().is_err());
        assert_eq!(count(&directory.0.join("generations")), 2);
        drop(store);
        let mut store = SessionStore::open(&directory.0).unwrap();
        assert_eq!(marker(&mut store), 1);
        assert_eq!(store.loaded_generation(), Some(previous));
        assert!(store.recovered_previous());
        checkpoint(&mut store, &capture(4), &cancel).unwrap();
        assert_eq!(store.head.as_ref().unwrap().previous.as_ref().unwrap().id, previous);
        assert_eq!(marker(&mut store), 4);
        assert_eq!(count(&directory.0.join("generations")), 2);
        let mut head: serde_json::Value = serde_json::from_slice(&read_bounded(&directory.0.join("head.json"), 1024).unwrap()).unwrap();
        head["current"]["sha256"] = "invalid".into();
        atomic_replace(&directory.0.join("head.json"), &serde_json::to_vec(&head).unwrap()).unwrap();
        assert!(store.load(ProjectLimits::default(), &cancel).is_err());
        assert!(store.collect_garbage().is_err());
        drop(store);
        assert!(SessionStore::open(&directory.0).is_err());
        assert_eq!(count(&directory.0.join("generations")), 2);
    }

    #[test]
    fn repeated_retirement_preserves_the_empty_terminal_marker() {
        let root = Directory::new(); let path = root.0.join("closed"); let cancel = AtomicBool::new(false);
        let mut store = SessionStore::open(&path).unwrap();
        checkpoint(&mut store, &capture(2), &cancel).unwrap();
        store.prepare_retirement().unwrap();
        store.retire().unwrap();
        let marker = fs::metadata(path.join(".retired")).unwrap();
        let directory = fs::metadata(&path).unwrap();
        assert_eq!(marker.len(), 0);
        assert!(!path.join(".retiring").exists());
        prepare_store_retirement(&path).unwrap();
        drop(store);
        for _ in 0..8 {
            collect_unreferenced_stores(&root.0, &BTreeSet::new(), &cancel).unwrap();
            let unchanged = fs::metadata(path.join(".retired")).unwrap();
            assert_eq!(marker.modified().unwrap(), unchanged.modified().unwrap());
            assert_eq!(directory.modified().unwrap(), fs::metadata(&path).unwrap().modified().unwrap());
            #[cfg(unix)] { use std::os::unix::fs::MetadataExt; assert_eq!(marker.ino(), unchanged.ino()); }
            assert_eq!(count(&path.join("resources")), 0);
            assert_eq!(count(&path.join("generations")), 0);
            assert_eq!(count(&path), 2);
        }
    }

    #[cfg(unix)]
    #[test]
    fn nested_directory_symlinks_never_write_or_remove_the_target() {
        use std::os::unix::fs::symlink;
        let root = Directory::new(); let outside = Directory::new(); create_directory(&outside.0).unwrap();
        let cancel = AtomicBool::new(false);
        for name in ["resources", "generations"] {
            let path = root.0.join(name); let mut store = SessionStore::open(&path).unwrap();
            checkpoint(&mut store, &capture(1), &cancel).unwrap();
            store.prepare_retirement().unwrap();
            let target = outside.0.join(format!("{}.{}", PortableId::random(), if name == "resources" { "bin" } else { "json" }));
            fs::write(&target, b"outside user bytes").unwrap();
            let retained = path.join(format!("original-{name}"));
            fs::rename(path.join(name), &retained).unwrap();
            symlink(&outside.0, path.join(name)).unwrap();
            assert!(checkpoint(&mut store, &capture(2), &cancel).is_err());
            assert!(store.load(ProjectLimits::default(), &cancel).is_err());
            assert!(store.collect_garbage().is_err());
            assert!(store.retire().is_err());
            assert!(clean_directory(&path.join(name), &BTreeSet::new(), if name == "resources" { ".bin" } else { ".json" }).is_err());
            drop(store);
            assert!(SessionStore::open(&path).is_err());
            assert_eq!(collect_unreferenced_stores(&root.0, &BTreeSet::new(), &cancel).unwrap(), 0);
            assert_eq!(fs::read(&target).unwrap(), b"outside user bytes");
            assert!(path.join("head.json").is_file());
            fs::remove_file(path.join(name)).unwrap();
            fs::rename(retained, path.join(name)).unwrap();
            let mut store = SessionStore::open(&path).unwrap();
            assert_eq!(marker(&mut store), 1);
            store.retire().unwrap();
            drop(store);
            assert_eq!(fs::read(&target).unwrap(), b"outside user bytes");
            fs::remove_dir_all(path).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn unknown_head_read_error_is_never_treated_as_an_absent_checkpoint() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = Directory::new(); let path = root.0.join("drawing");
        let mut store = SessionStore::open(&path).unwrap();
        let prepared = capture(7);
        let generation = checkpoint(&mut store, &prepared, &AtomicBool::new(false)).unwrap();
        let permissions = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
        let failed = store.collect_garbage();
        fs::set_permissions(&path, permissions).unwrap();
        assert!(failed.is_err());
        assert_eq!(store.generation(), Some(generation));
        drop(store);
        fs::rename(path.join("head.json"), path.join("saved-head")).unwrap();
        symlink("head.json", path.join("head.json")).unwrap();
        assert!(SessionStore::open(&path).is_err());
        assert_eq!(count(&path.join("packs")), 1);
        assert_eq!(count(&path.join("generations")), 1);
        fs::remove_file(path.join("head.json")).unwrap();
        fs::rename(path.join("saved-head"), path.join("head.json")).unwrap();
        let mut store = SessionStore::open(&path).unwrap();
        assert_eq!(marker(&mut store), 7);
    }
}

#[cfg(test)]
mod live_tests;
