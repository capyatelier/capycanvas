use super::{ByteRange, ByteSource, ImmutableBacking, RangeState, MAX_RANGE_BYTES,
    resources::Payload, session::{OpenSession, PreparedSession}};
use crate::{PortableId, ProjectLimits};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write}, path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicBool, Ordering}}};

#[derive(Debug)]
pub struct AtomicReplaceError { pub published: bool, pub error: String }
impl From<String> for AtomicReplaceError {
    fn from(error: String) -> Self { Self { published: false, error } }
}
impl From<&str> for AtomicReplaceError {
    fn from(error: &str) -> Self { error.to_string().into() }
}

pub struct OwnerLock { _file: File, readers: Mutex<Vec<Weak<ResourceFile>>> }
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
        Ok(Some(Self { _file: file, readers: Mutex::new(Vec::new()) }))
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
fn parse_head(bytes: &[u8]) -> Result<Head, String> {
    let head: Head = serde_json::from_value(super::parse_json(bytes, 1024)?).map_err(|e| e.to_string())?;
    if std::iter::once(&head.current).chain(head.previous.as_ref()).any(|generation|
        generation.sha256.len() != 64 || !generation.sha256.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        || head.previous.as_ref().is_some_and(|previous| previous.id == head.current.id) {
        return Err("Invalid recovery metadata checksum reference".into());
    }
    Ok(head)
}

pub struct SessionStore {
    path: PathBuf,
    _owner: Arc<OwnerLock>,
    _writer: Arc<()>,
    head: Option<Head>,
    loaded: Option<Generation>,
    verified: BTreeMap<PortableId, (u64, u32, [u8; 32])>,
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
    pub fn try_open(path: &Path) -> Result<Option<Self>, String> {
        create_directory(path)?;
        let path = &fs::canonicalize(path).map_err(|e| e.to_string())?;
        let Some((owner, writer)) = claim_store(path)? else { return Ok(None) };
        if retired_store(path)? { return Err("This drawing's session copy was retired".into()); }
        retiring_store(path)?;
        sync_directory(path)?;
        create_directory(&path.join("resources"))?;
        create_directory(&path.join("generations"))?;
        let head = read_optional(&path.join("head.json"), 1024)?.map(|bytes| parse_head(&bytes)).transpose()?;
        let mut store = Self { path: path.into(), _owner: owner, _writer: writer, head, loaded: None,
            verified: BTreeMap::new() };
        match store.resource_roots() {
            Ok(roots) => store.collect_roots(roots)?,
            Err(_) => {
                clean_temporaries(&store.path)?;
                clean_temporaries(&store.path.join("generations"))?;
                clean_temporaries(&store.path.join("resources"))?;
            }
        }
        Ok(Some(store))
    }
    pub fn path(&self) -> &Path { &self.path }
    pub fn generation(&self) -> Option<PortableId> { self.head.as_ref().map(|head| head.current.id) }
    pub fn loaded_generation(&self) -> Option<PortableId> { self.loaded.as_ref().map(|loaded| loaded.id) }
    pub fn recovered_previous(&self) -> bool { self.loaded_generation().is_some_and(|loaded| Some(loaded) != self.generation()) }

    pub fn commit(&mut self, prepared: &PreparedSession, cancelled: &AtomicBool) -> Result<PortableId, String> {
        self.commit_using(prepared, cancelled, AtomicPublication)
    }
    fn commit_using(&mut self, prepared: &PreparedSession, cancelled: &AtomicBool, mut publication: impl Publication) -> Result<PortableId, String> {
        check_cancelled(cancelled)?;
        self.validate_layout()?;
        if retired_store(&self.path)? { return Err("This drawing's session copy was retired".into()); }
        self.refresh_head()?;
        let index = resource_index(prepared.metadata(), ProjectLimits::default())?;
        let entries: BTreeMap<_, _> = prepared.resources().entries.iter().map(|entry| (entry.payload.id(), entry)).collect();
        if entries.len() != index.records.len() { return Err("Recovery resource inventory disagrees with metadata".into()); }
        for record in &index.records {
            check_cancelled(cancelled)?;
            let entry = entries.get(&record.id).ok_or("Missing recovery resource payload")?;
            if entry.bytes != record.length || entry.crc != record.crc || resource_descriptor(&entry.record)? != record.descriptor {
                return Err("Recovery resource integrity disagrees with metadata".into());
            }
            let path = self.resource_path(record.id);
            if let Some(verified) = self.verified.get(&record.id) {
                if *verified != (record.length, record.crc, record.descriptor) { return Err("Conflicting immutable recovery resource descriptor".into()); }
                continue;
            }
            if metadata_optional(&path)?.is_some() {
                let file = File::open(&path).map_err(|e| e.to_string())?;
                verify_file(&file, record, cancelled)?;
                verify_payload(file, &entry.payload, record.length, cancelled)?;
            } else {
                publication.publish(&path, |file| {
                    let mut hasher = crc32fast::Hasher::new();
                    match &entry.payload {
                        Payload::Opaque(resource) => {
                            let mut offset = 0;
                            while offset < record.length {
                                check_cancelled(cancelled)?;
                                let count = (record.length - offset).min(MAX_RANGE_BYTES as u64) as usize;
                                let bytes = resource.read_chunk(offset, count, cancelled)?;
                                hasher.update(&bytes);
                                file.write_all(&bytes).map_err(|e| e.to_string())?;
                                offset += count as u64;
                            }
                        }
                        payload => {
                            let bytes = payload.encoded()?.bytes;
                            if bytes.len() as u64 != record.length { return Err("Recovery payload length changed".into()); }
                            for chunk in bytes.chunks(MAX_RANGE_BYTES) {
                                check_cancelled(cancelled)?;
                                hasher.update(chunk);
                                file.write_all(chunk).map_err(|e| e.to_string())?;
                            }
                        }
                    }
                    if hasher.finalize() != record.crc { return Err("Recovery resource checksum failed".into()); }
                    Ok(())
                }, || check_cancelled(cancelled))?;
            }
            self.verified.insert(record.id, (record.length, record.crc, record.descriptor));
        }
        let generation = PortableId::random();
        publication.publish(&self.generation_path(generation), |file| {
            file.write_all(prepared.metadata()).map_err(|e| e.to_string())
        }, || check_cancelled(cancelled))?;
        let reference = Generation::new(generation, prepared.metadata());
        let head = Head { current: reference.clone(),
            previous: self.loaded.clone().or_else(|| self.head.as_ref().map(|head| head.current.clone())) };
        let bytes = serde_json::to_vec(&head).map_err(|e| e.to_string())?;
        publication.publish(&self.path.join("head.json"), |file| file.write_all(&bytes).map_err(|e| e.to_string()),
            || check_cancelled(cancelled))?;
        self.head = Some(head);
        self.loaded = Some(reference);
        self.collect_garbage()?;
        Ok(generation)
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
                Err(error) => { if failure.is_none() { failure = Some(error); } }
            }
        }
        Err(failure.unwrap_or_else(|| "Recovery checkpoint unavailable".into()))
    }

    fn generation_metadata(&self, generation: &Generation, limit: u64) -> Result<Vec<u8>, String> {
        let metadata = read_bounded(&self.generation_path(generation.id), limit)?;
        if Generation::new(generation.id, &metadata) != *generation { return Err("Recovery metadata checksum failed".into()); }
        Ok(metadata)
    }
    fn load_generation(&mut self, generation: &Generation, limits: ProjectLimits, cancelled: &AtomicBool) -> Result<OpenSession, String> {
        let metadata = self.generation_metadata(generation, limits.metadata_bytes)?;
        let index = resource_index(&metadata, limits)?;
        let mut files = Vec::with_capacity(index.ranges.len());
        let mut verified = BTreeMap::new();
        for record in &index.ranges {
            check_cancelled(cancelled)?;
            let file = File::open(self.resource_path(record.id)).map_err(|e| e.to_string())?;
            verify_file(&file, record, cancelled)?;
            verified.insert(record.id, (record.length, record.crc, record.descriptor));
            let owner = Arc::new(ResourceFile { id: record.id, path: self.resource_path(record.id),
                _owner: self._owner.clone(), length: record.length });
            self._owner.readers.lock().map_err(|_| "Recovery reader lock failed")?.push(Arc::downgrade(&owner));
            files.push((record.offset, owner));
        }
        let backing = ImmutableBacking::new(Arc::new(ResourcePack { length: index.length, files })).map_err(String::from)?;
        let session = super::session::open_parts(&metadata, backing, limits, cancelled)?;
        if verified.iter().any(|(id, value)| self.verified.get(id).is_some_and(|previous| previous != value)) {
            return Err("Conflicting immutable recovery resource descriptor".into());
        }
        self.verified.extend(verified);
        Ok(session)
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
        self.collect_garbage()
    }
    pub fn prepare_retirement(&mut self) -> Result<(), String> {
        mark_retirement_intent(&self.path)
    }

    pub fn collect_garbage(&mut self) -> Result<(), String> {
        self.validate_layout()?;
        self.refresh_head()?;
        sync_directory(&self.path)?;
        self.collect_roots(self.resource_roots()?)
    }
    fn resource_roots(&self) -> Result<(BTreeSet<PortableId>, BTreeSet<PortableId>), String> {
        let mut generations = BTreeSet::new();
        let mut resources = BTreeSet::new();
        if let Some(head) = &self.head {
            for generation in std::iter::once(&head.current).chain(head.previous.as_ref()) {
                generations.insert(generation.id);
                let metadata = self.generation_metadata(generation, ProjectLimits::default().metadata_bytes)?;
                resources.extend(resource_index(&metadata, ProjectLimits::default())?.records.into_iter().map(|record| record.id));
            }
        }
        Ok((generations, resources))
    }
    fn collect_roots(&mut self, (generations, mut resources): (BTreeSet<PortableId>, BTreeSet<PortableId>)) -> Result<(), String> {
        self._owner.readers.lock().map_err(|_| "Recovery reader lock failed")?.retain(|reader| {
            if let Some(owner) = reader.upgrade() { resources.insert(owner.id); true } else { false }
        });
        let terminal = retired_store(&self.path)?;
        for (name, retained, suffix) in [("generations", &generations, ".json"), ("resources", &resources, ".bin")] {
            let path = self.path.join(name);
            if metadata_optional(&path)?.is_some() {
                clean_directory(&path, retained, suffix)?;
                if terminal && retained.is_empty() { fs::remove_dir(&path).map_err(|e| e.to_string())?; }
            } else if !terminal { return Err("Missing drawing session directory".into()); }
        }
        clean_temporaries(&self.path)?;
        self.verified.retain(|id, _| resources.contains(id));
        Ok(())
    }
    fn resource_path(&self, id: PortableId) -> PathBuf { self.path.join("resources").join(format!("{id}.bin")) }
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
            head: None, loaded: None, verified: BTreeMap::new() };
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
    fn publish(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String>;
}
struct AtomicPublication;
impl Publication for AtomicPublication {
    fn publish(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        atomic_publish(path, write, ready)
    }
}

struct ResourceRecord { id: PortableId, offset: u64, length: u64, crc: u32, descriptor: [u8; 32] }
struct ResourceIndex { records: Vec<ResourceRecord>, ranges: Vec<ResourceRecord>, length: u64 }
fn resource_index(metadata: &[u8], limits: ProjectLimits) -> Result<ResourceIndex, String> {
    if metadata.len() as u64 > limits.metadata_bytes { return Err("Recovery metadata exceeds admission limit".into()); }
    let value = super::parse_json(metadata, limits.metadata_bytes.min(usize::MAX as u64) as usize)?;
    let records = value["resources"].as_array().ok_or("Missing recovery resource inventory")?;
    if records.len() > 262_144 { return Err("Recovery resource count exceeds admission limit".into()); }
    let mut ids = BTreeSet::new();
    let mut ranges = BTreeMap::<u64, (u64, u32, PortableId, [u8; 32])>::new();
    let mut output = Vec::with_capacity(records.len());
    for record in records {
        let id: PortableId = record["id"].as_str().ok_or("Invalid recovery resource identity")?.parse().map_err(str::to_string)?;
        if !ids.insert(id) { return Err("Duplicate recovery resource identity".into()); }
        if record["location"]["pack"] != "data/tiles-1.bin" { return Err("Invalid recovery resource pack".into()); }
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
        if let Some((previous_length, previous_crc, _, previous_descriptor)) = ranges.get(&offset) {
            if *previous_length != length || *previous_crc != crc || previous_descriptor != &descriptor { return Err("Conflicting recovery resource aliases".into()); }
        } else { ranges.insert(offset, (length, crc, id, descriptor)); }
        output.push(ResourceRecord { id, offset, length, crc, descriptor });
    }
    let mut length = 0u64;
    let mut physical = Vec::with_capacity(ranges.len());
    for (offset, (count, crc, id, descriptor)) in ranges {
        if offset != length { return Err("Recovery resource pack has overlapping or missing ranges".into()); }
        length = length.checked_add(count).ok_or("Recovery resource pack overflow")?;
        let budget = limits.raster_bytes.saturating_add(limits.asset_bytes).saturating_add(512 * 1024 * 1024).saturating_mul(2);
        if length > budget { return Err("Recovery resource pack exceeds admission limit".into()); }
        physical.push(ResourceRecord { id, offset, length: count, crc, descriptor });
    }
    if output.iter().any(|record| record.offset.checked_add(record.length).is_none_or(|end| end > length)) {
        return Err("Recovery resource range exceeds pack".into());
    }
    Ok(ResourceIndex { records: output, ranges: physical, length })
}
fn resource_descriptor(record: &serde_json::Value) -> Result<[u8; 32], String> {
    let mut descriptor = record.clone();
    let fields = descriptor.as_object_mut().ok_or("Invalid recovery resource descriptor")?;
    fields.remove("id"); fields.remove("location");
    Ok(Sha256::digest(serde_json::to_vec(&descriptor).map_err(|e| e.to_string())?).into())
}

struct ResourceFile { id: PortableId, path: PathBuf, _owner: Arc<OwnerLock>, length: u64 }
struct ResourcePack { length: u64, files: Vec<(u64, Arc<ResourceFile>)> }
impl ByteSource for ResourcePack {
    fn byte_len(&self) -> u64 { self.length }
    fn resident_bytes(&self) -> usize { 0 }
    fn poll(&self, offset: u64, length: usize) -> Result<RangeState, String> {
        if length > MAX_RANGE_BYTES || offset.checked_add(length as u64).is_none_or(|end| end > self.length) { return Err("Recovery read exceeds backing".into()); }
        let mut bytes = vec![0; length];
        let mut position = offset;
        let mut written = 0;
        let mut index = self.files.partition_point(|(start, _)| *start <= offset).saturating_sub(1);
        while written < length {
            let (start, owner) = self.files.get(index).ok_or("Incomplete recovery backing")?;
            let local = position.checked_sub(*start).ok_or("Recovery backing range overflow")?;
            let count = (owner.length.saturating_sub(local)).min((length - written) as u64) as usize;
            if count > 0 {
                let mut file = File::open(&owner.path).map_err(|e| e.to_string())?;
                file.seek(SeekFrom::Start(local)).and_then(|_| file.read_exact(&mut bytes[written..written + count])).map_err(|e| e.to_string())?;
                written += count; position += count as u64;
            }
            index += 1;
        }
        Ok(RangeState::Ready(ByteRange::new(bytes.into(), 0..length)?))
    }
}

fn verify_file(file: &File, record: &ResourceRecord, cancelled: &AtomicBool) -> Result<(), String> {
    if file.metadata().map_err(|e| e.to_string())?.len() != record.length { return Err("Recovery resource length changed".into()); }
    let mut file = file.try_clone().map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut buffer = [0; 64 * 1024];
    let mut remaining = record.length;
    let mut checksum = crc32fast::Hasher::new();
    while remaining != 0 {
        check_cancelled(cancelled)?;
        let count = remaining.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..count]).map_err(|e| e.to_string())?;
        checksum.update(&buffer[..count]); remaining -= count as u64;
    }
    if checksum.finalize() != record.crc { return Err("Recovery resource checksum failed".into()); }
    Ok(())
}
fn verify_payload(mut file: File, payload: &Payload, length: u64, cancelled: &AtomicBool) -> Result<(), String> {
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let encoded = match payload { Payload::Opaque(_) => None, payload => Some(payload.encoded()?.bytes) };
    let mut buffer = [0; 64 * 1024];
    let mut offset = 0;
    while offset < length {
        check_cancelled(cancelled)?;
        let count = (length - offset).min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..count]).map_err(|e| e.to_string())?;
        let same = match payload {
            Payload::Opaque(resource) => *resource.read_chunk(offset, count, cancelled)? == buffer[..count],
            _ => encoded.as_ref().and_then(|bytes| bytes.get(offset as usize..offset as usize + count)) == Some(&buffer[..count]),
        };
        if !same { return Err("Conflicting immutable recovery resource bytes".into()); }
        offset += count as u64;
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
    fn capture(marker: u8) -> PreparedSession {
        let mut document = Document::new(PortableId::random(), 256, 256, DocumentNames { paint: "ink".into(), paper: "paper".into() });
        let descriptor = RasterPlane::Color.descriptor(Default::default());
        let tile = RasterTile::backed(TileBlob::encode(descriptor, &vec![marker; 256 * 256 * 4]).unwrap());
        *document.target_raster_mut(document.working.target.unwrap()).unwrap() = RasterRevision::backed(RasterData {
            tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }, tile)].into(), ..Default::default()
        });
        let editor = Editor::new(document);
        let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
        PreparedSession::prepare(&capture, json!({"marker": marker}), &AtomicBool::new(false)).unwrap()
    }
    fn count(path: &Path) -> usize { match fs::read_dir(path) { Ok(entries) => entries.count(), Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0, Err(error) => panic!("{error}") } }
    fn resource_ids(prepared: &PreparedSession) -> BTreeSet<PortableId> { prepared.resources().entries.iter().map(|entry| entry.payload.id()).collect() }
    fn tile_resource(prepared: &PreparedSession) -> &super::super::resources::PreparedResource {
        prepared.resources().entries.iter().find(|entry| matches!(entry.payload, Payload::Tile(_))).unwrap()
    }
    fn marker(store: &mut SessionStore) -> u8 {
        store.load(ProjectLimits::default(), &AtomicBool::new(false)).unwrap().unwrap().metadata.value["marker"].as_u64().unwrap() as u8
    }

    struct Interrupted { operation: usize, after: bool, count: usize }
    impl Publication for Interrupted {
        fn publish(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
            self.count += 1;
            let interrupt = self.count == self.operation;
            if interrupt && !self.after {
                return atomic_publish(path, |file| { write(file)?; Err("Interrupted write".into()) }, ready);
            }
            atomic_publish(path, write, ready)?;
            if interrupt { return Err("Interrupted publication".into()); }
            Ok(())
        }
    }

    #[test]
    fn every_write_interruption_restores_a_complete_checkpoint() {
        let old = capture(11);
        let new = capture(22);
        let old_ids = resource_ids(&old); let new_ids = resource_ids(&new);
        let operations = new_ids.difference(&old_ids).count() + 2;
        for after in [false, true] {
            for operation in 1..=operations {
                let directory = Directory::new();
                let mut store = SessionStore::open(&directory.0).unwrap();
                store.commit(&old, &AtomicBool::new(false)).unwrap();
                let failure = store.commit_using(&new, &AtomicBool::new(false), Interrupted { operation, after, count: 0 });
                assert!(failure.is_err());
                drop(store);
                let mut restored = SessionStore::open(&directory.0).unwrap();
                assert_eq!(marker(&mut restored), if after && operation == operations { 22 } else { 11 });
                assert_eq!(count(&directory.0.join("generations")), if after && operation == operations { 2 } else { 1 });
                assert_eq!(count(&directory.0.join("resources")), if after && operation == operations { old_ids.union(&new_ids).count() } else { old_ids.len() });
            }
        }
    }

    #[test]
    fn process_death_mid_write_keeps_the_original_and_cleans_temporary_files() {
        if let Some(directory) = std::env::var_os("CAPY_SESSION_CRASH_DIRECTORY") {
            let path = PathBuf::from(directory);
            let store = SessionStore::open(&path).unwrap();
            let target = match std::env::var("CAPY_SESSION_CRASH_STAGE").unwrap().as_str() {
                "resources" => store.resource_path(PortableId::random()),
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
            store.commit(&original, &AtomicBool::new(false)).unwrap();
            drop(store);
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "package::session_store::tests::process_death_mid_write_keeps_the_original_and_cleans_temporary_files", "--test-threads=1"])
                .env("CAPY_SESSION_CRASH_DIRECTORY", &directory.0)
                .env("CAPY_SESSION_CRASH_STAGE", stage)
                .status().unwrap();
            assert_eq!(status.code(), Some(87));
            let mut store = SessionStore::open(&directory.0).unwrap();
            assert_eq!(marker(&mut store), 7);
            assert_eq!(count(&directory.0.join("resources")), original.resources().entries.len());
            assert_eq!(count(&directory.0.join("generations")), 1);
            assert_eq!(count(&directory.0), 4);
        }
    }

    #[test]
    fn publication_error_after_head_does_not_collect_the_new_checkpoint() {
        let directory = Directory::new();
        let mut store = SessionStore::open(&directory.0).unwrap();
        let old = capture(1);
        store.commit(&old, &AtomicBool::new(false)).unwrap();
        let new = capture(2);
        assert!(store.commit_using(&new, &AtomicBool::new(false), Interrupted { operation: resource_ids(&new).difference(&resource_ids(&old)).count() + 2, after: true, count: 0 }).is_err());
        store.collect_garbage().unwrap();
        assert_eq!(marker(&mut store), 2);
        let third = capture(3);
        store.commit(&third, &AtomicBool::new(false)).unwrap();
        assert_eq!(marker(&mut store), 3);
        assert_eq!(count(&directory.0.join("generations")), 2);
        assert_eq!(count(&directory.0.join("resources")), resource_ids(&new).union(&resource_ids(&third)).count());
    }

    #[test]
    fn repeated_changes_keep_only_current_previous_and_live_readers() {
        let root = Directory::new(); let directory = Directory(root.0.join("drawing"));
        let mut store = SessionStore::open(&directory.0).unwrap();
        let original = capture(1);
        store.commit(&original, &AtomicBool::new(false)).unwrap();
        let resource = tile_resource(&original);
        let live = Arc::new(ResourceFile { id: resource.payload.id(), path: store.resource_path(resource.payload.id()), length: resource.bytes, _owner: store._owner.clone() });
        store._owner.readers.lock().unwrap().push(Arc::downgrade(&live));
        for value in 2..20 {
            store.commit(&capture(value), &AtomicBool::new(false)).unwrap();
            assert!(count(&directory.0.join("generations")) <= 2);
            assert!(count(&directory.0.join("resources")) <= original.resources().entries.len() + 2);
        }
        let pack = ResourcePack { length: live.length, files: vec![(0, live.clone())] };
        assert!(matches!(pack.poll(0, live.length as usize).unwrap(), RangeState::Ready(_)));
        store.retire().unwrap();
        assert_eq!(count(&directory.0.join("resources")), 1);
        drop(store);
        assert!(OwnerLock::claim(&directory.0.join(".lock")).unwrap().is_none());
        assert!(SessionStore::open(&directory.0).is_err());
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &AtomicBool::new(false)).unwrap();
        assert_eq!(count(&directory.0.join("resources")), 1);
        drop(pack); drop(live);
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &AtomicBool::new(false)).unwrap();
        assert_eq!(count(&directory.0.join("resources")), 0);
        assert_eq!(count(&directory.0.join("generations")), 0);
        assert!(directory.0.join(".lock").exists());
    }

    #[test]
    fn corrupt_current_falls_back_then_replacement_collects_corrupt_files() {
        for corruption in 0..3 {
            let directory = Directory::new();
            let mut store = SessionStore::open(&directory.0).unwrap();
            store.commit(&capture(1), &AtomicBool::new(false)).unwrap();
            let second = capture(2);
            let generation = store.commit(&second, &AtomicBool::new(false)).unwrap();
            match corruption {
                0 => fs::write(store.resource_path(tile_resource(&second).payload.id()), b"broken").unwrap(),
                1 => fs::write(store.generation_path(generation), b"broken").unwrap(),
                _ => {
                    let mut metadata: serde_json::Value = serde_json::from_slice(second.metadata()).unwrap();
                    let resource = metadata["resources"].as_array_mut().unwrap().iter_mut().find(|record| record["id"] == json!(tile_resource(&second).payload.id())).unwrap();
                    resource["encoding"] = "unsupported".into();
                    let metadata = serde_json::to_vec(&metadata).unwrap();
                    fs::write(store.generation_path(generation), &metadata).unwrap();
                    let mut head = store.head.clone().unwrap();head.current=Generation::new(generation,&metadata);
                    fs::write(store.path.join("head.json"), serde_json::to_vec(&head).unwrap()).unwrap();
                }
            }
            drop(store);
            let mut store = SessionStore::open(&directory.0).unwrap();
            assert_eq!(marker(&mut store), 1);
            store.commit(&capture(3), &AtomicBool::new(false)).unwrap();
            assert_eq!(marker(&mut store), 3);
            assert_eq!(count(&directory.0.join("generations")), 2);
            assert_eq!(count(&directory.0.join("resources")), second.resources().entries.len() + 1);
        }
    }

    #[test]
    fn reuse_verifies_existing_immutable_bytes_and_does_not_rewrite() {
        let directory = Directory::new();
        let capture = capture(9);
        let mut store = SessionStore::open(&directory.0).unwrap();
        store.commit(&capture, &AtomicBool::new(false)).unwrap();
        let path = store.resource_path(tile_resource(&capture).payload.id());
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        store.commit(&capture, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        drop(store);
        let mut store = SessionStore::open(&directory.0).unwrap();
        fs::write(&path, b"corrupt").unwrap();
        let head = fs::read(directory.0.join("head.json")).unwrap();
        assert!(store.commit(&capture, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(directory.0.join("head.json")).unwrap(), head);
        assert!(store.load(ProjectLimits::default(), &AtomicBool::new(false)).is_err());
    }

    #[test]
    fn ownership_and_cancelled_retirement_preserve_the_only_copy() {
        let directory = Directory::new();
        let mut store = SessionStore::open(&directory.0).unwrap();
        assert!(SessionStore::open(&directory.0).is_err());
        let saved = capture(5);
        store.commit(&saved, &AtomicBool::new(false)).unwrap();
        assert!(store.commit(&capture(6), &AtomicBool::new(true)).is_err());
        assert_eq!(marker(&mut store), 5);
        let file = directory.0.join("generations/.capy-save-interrupted");
        fs::write(&file, b"partial").unwrap();
        store.collect_garbage().unwrap();
        assert!(!file.exists());
        drop(store);
        let store = SessionStore::open(&directory.0).unwrap();
        assert_eq!(count(&directory.0.join("resources")), saved.resources().entries.len());
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
                1 => value["resources"][0]["bytes"] = "18446744073709551615".into(),
                2 => value["resources"][0]["id"] = "../elsewhere".into(),
                3 => { let record = value["resources"][0].clone(); value["resources"].as_array_mut().unwrap().push(record); },
                _ => value["resources"][0]["crc32"] = "FFFFFFFF".into(),
            }
            assert!(resource_index(&serde_json::to_vec(&value).unwrap(), ProjectLimits::default()).is_err());
        }
    }

    #[test]
    fn orphan_sweep_preserves_membership_writers_and_reader_pins() {
        let root = Directory::new();
        let cancel = AtomicBool::new(false);
        let mut kept = SessionStore::open(&root.0.join("kept")).unwrap();
        kept.commit(&capture(1), &cancel).unwrap();
        let mut live = SessionStore::open(&root.0.join("writer")).unwrap();
        live.commit(&capture(2), &cancel).unwrap();
        let mut orphan = SessionStore::open(&root.0.join("orphan")).unwrap();
        let prepared = capture(3);
        orphan.commit(&prepared, &cancel).unwrap();
        let tile = tile_resource(&prepared);
        let reader = Arc::new(ResourceFile { id: tile.payload.id(), path: orphan.resource_path(tile.payload.id()), length: tile.bytes, _owner: orphan._owner.clone() });
        orphan._owner.readers.lock().unwrap().push(Arc::downgrade(&reader));
        orphan.retire().unwrap();
        let mut unpublished = SessionStore::open(&root.0.join("unpublished")).unwrap();
        unpublished.commit(&capture(4), &cancel).unwrap();
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
        assert_eq!(count(&root.0.join("orphan/resources")), 1);
        assert!(reader.path.is_file());
        assert_eq!(fs::read(unrecognized.join("keep")).unwrap(), b"user bytes");
        assert!(root.0.join("unpublished/head.json").is_file());
        drop(reader); drop(live);
        collect_unreferenced_stores(&root.0, &reachable, &cancel).unwrap();
        assert_eq!(count(&root.0.join("orphan/resources")), 0);
        assert!(root.0.join("writer/head.json").is_file());
        assert!(root.0.join("orphan/.lock").is_file());
    }

    #[test]
    fn durable_retirement_prevents_reopening_before_head_is_removed() {
        let root = Directory::new();
        let path = root.0.join("closed");
        let mut store = SessionStore::open(&path).unwrap();
        store.commit(&capture(1), &AtomicBool::new(false)).unwrap();
        atomic_replace(&path.join(".retired"), b"").unwrap();
        assert!(path.join("head.json").is_file());
        assert!(store.commit(&capture(2), &AtomicBool::new(false)).is_err());
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
        store.commit(&capture(1), &cancel).unwrap();
        store.prepare_retirement().unwrap();
        drop(store);
        collect_unreferenced_stores(&root.0, &["closing".into()].into(), &cancel).unwrap();
        let mut store = SessionStore::open(&path).unwrap();
        assert_eq!(marker(&mut store), 1);
        store.commit(&capture(2), &cancel).unwrap();
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
        store.commit(&capture(8), &cancel).unwrap();
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
            store.commit(&capture(2), &cancel).unwrap();
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
        let previous = store.commit(&capture(1), &cancel).unwrap();
        let prepared = capture(2);
        let current = store.commit(&prepared, &cancel).unwrap();
        let mut metadata: serde_json::Value = serde_json::from_slice(prepared.metadata()).unwrap();
        metadata["metadata"]["marker"] = 3.into();
        let damaged = serde_json::to_vec(&metadata).unwrap();
        assert_eq!(damaged.len(), prepared.metadata().len());
        assert_ne!(damaged, prepared.metadata());
        fs::write(store.generation_path(current), damaged).unwrap();
        assert!(store.collect_garbage().is_err());
        assert_eq!(count(&directory.0.join("generations")), 2);
        drop(store);
        let mut store = SessionStore::open(&directory.0).unwrap();
        assert_eq!(marker(&mut store), 1);
        assert_eq!(store.loaded_generation(), Some(previous));
        assert!(store.recovered_previous());
        store.commit(&capture(4), &cancel).unwrap();
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
        store.commit(&capture(2), &cancel).unwrap();
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
            store.commit(&capture(1), &cancel).unwrap();
            store.prepare_retirement().unwrap();
            let target = outside.0.join(format!("{}.{}", PortableId::random(), if name == "resources" { "bin" } else { "json" }));
            fs::write(&target, b"outside user bytes").unwrap();
            let retained = path.join(format!("original-{name}"));
            fs::rename(path.join(name), &retained).unwrap();
            symlink(&outside.0, path.join(name)).unwrap();
            assert!(store.commit(&capture(2), &cancel).is_err());
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
        let generation = store.commit(&prepared, &AtomicBool::new(false)).unwrap();
        let permissions = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0)).unwrap();
        let failed = store.collect_garbage();
        fs::set_permissions(&path, permissions).unwrap();
        assert!(failed.is_err());
        assert_eq!(store.generation(), Some(generation));
        drop(store);
        fs::rename(path.join("head.json"), path.join("saved-head")).unwrap();
        symlink("head.json", path.join("head.json")).unwrap();
        assert!(SessionStore::open(&path).is_err());
        assert_eq!(count(&path.join("resources")), prepared.resources().entries.len());
        assert_eq!(count(&path.join("generations")), 1);
        fs::remove_file(path.join("head.json")).unwrap();
        fs::rename(path.join("saved-head"), path.join("head.json")).unwrap();
        let mut store = SessionStore::open(&path).unwrap();
        assert_eq!(marker(&mut store), 7);
    }
}
