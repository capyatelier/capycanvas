use super::*;
use crate::package::storage_budget::storage_error;
use std::{fs::{self,File,OpenOptions},io::{BufWriter,Read,Write}};

#[derive(Debug)]
pub struct AtomicReplaceError { pub published: bool, pub error: String }
impl From<String> for AtomicReplaceError {
    fn from(error: String) -> Self { Self { published: false, error } }
}
impl From<&str> for AtomicReplaceError {
    fn from(error: &str) -> Self { error.to_string().into() }
}

pub struct OwnerLock { pub(super) _file:File }
impl OwnerLock {
    pub fn claim(path: &Path) -> Result<Option<Self>, String> {
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        let file = options.open(path).map_err(storage_error)?;
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
        Ok(Some(Self { _file:file }))
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
pub(super) fn relocate_session<Owner>(source: &Path, destination: &Path, _owner: Owner) -> Result<(), String> {
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
pub(super) fn import_sessions_using(root: &Path, manifest: &str, lock: &str, boundary: impl FnMut(&str) -> Result<(), String>) -> Result<PathBuf, String> {
    import_with_owner_using(root, manifest, lock, OwnerLock::claim, boundary)
}
pub(super) fn import_with_owner_using<Owner>(root: &Path, manifest: &str, lock: &str, mut claim: impl FnMut(&Path) -> Result<Option<Owner>, String>, mut boundary: impl FnMut(&str) -> Result<(), String>) -> Result<PathBuf, String> {
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
pub(super) fn relocate_flat_sessions(root: &Path, packed: &Path, boundary: &mut impl FnMut(&str) -> Result<(), String>) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?; let source = entry.path();
        if source.extension().is_none_or(|extension| extension != "json") || !entry.file_type().map_err(|e| e.to_string())?.is_file() { continue; }
        let Some(_owner) = OwnerLock::claim(&source.with_extension("lock"))? else { continue; };
        let destination = packed.join(entry.file_name());
        if metadata_optional(&destination)?.is_some() { return Err("The saved session exists in both versions; both copies were kept".into()); }
        let bytes = read_bounded(&source, 1024 * 1024)?;
        let value = super::super::parse_json(&bytes, 1024 * 1024)?;
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
pub(super) fn relocate_entry(source: &Path, destination: &Path) -> Result<(), String> {
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
        Err(error) => return Err(storage_error(error)),
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


pub(super) fn claim_store(path:&Path)->Result<Option<StoreClaim>,String> {
    claim_store_with(path,||match OwnerLock::claim(&path.join(".lock"))? {
        Some(owner)=>Ok(Some((Arc::new(super::super::store_io::NativeTransport::new(path.into())),Arc::new(owner)))),
        None=>Ok(None),
    })
}

impl SessionStore {
    pub async fn open(path: &Path) -> Result<Self, String> {
        Self::try_open(path).await?.ok_or_else(|| "Recovery drawing is already open".into())
    }
    pub async fn open_with_cache(path: &Path, reads:crate::tile_io::ReadCache) -> Result<Self,String> {
        Self::try_open_with_cache(path,reads).await?.ok_or_else(||"Recovery drawing is already open".into())
    }
    pub async fn try_open(path: &Path) -> Result<Option<Self>, String> {
        Self::try_open_with_cache(path,crate::tile_io::ReadCache::process()).await
    }
    async fn try_open_with_cache(path:&Path,reads:crate::tile_io::ReadCache)->Result<Option<Self>,String> {
        create_directory(path)?;
        let path = &fs::canonicalize(path).map_err(|e| e.to_string())?;
        let Some((owner, writer)) = claim_store(path)? else { return Ok(None) };
        if retired_store(path)? { return Err("This drawing's session copy was retired".into()); }
        retiring_store(path)?;
        sync_directory(path)?;
        create_directory(&path.join("resources"))?;
        create_directory(&path.join("generations"))?;
        create_directory(&path.join("packs"))?;
        Self::initialize(path.into(),owner,writer,reads).await.map(Some)
    }
}

pub(super) fn store_layout(path: &Path) -> Result<bool, String> {
    store_layout_with(path, false)
}
pub(super) fn store_layout_with(path: &Path, terminal: bool) -> Result<bool, String> {
    Ok(metadata_optional(path)?.is_some_and(|metadata| metadata.is_dir())
        && metadata_optional(&path.join(".lock"))?.is_some_and(|metadata| metadata.is_file())
        && metadata_optional(&path.join("resources"))?.map_or(terminal, |metadata| metadata.is_dir())
        && metadata_optional(&path.join("packs"))?.is_none_or(|metadata| metadata.is_dir())
        && metadata_optional(&path.join("generations"))?.map_or(terminal, |metadata| metadata.is_dir()))
}
pub async fn collect_unreferenced_stores(root: &Path, reachable: &BTreeSet<String>, cancelled: &AtomicBool) -> Result<usize, String> {
    let mut retired = 0;
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        check_cancelled(cancelled)?;
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() { continue; }
        let Some(key) = entry.file_name().to_str().map(str::to_owned) else { continue };
        let path = entry.path();
        if reachable.contains(&key) || !metadata_optional(&path.join(".lock"))?.is_some_and(|metadata|metadata.is_file()) { continue; }
        let terminal=retired_store(&path)?;
        if !store_layout_with(&path,terminal)? {continue;}
        let path=fs::canonicalize(&path).map_err(|error|error.to_string())?;
        let Some((owner,writer))=claim_store(&path)? else {continue;};
        if SessionStore::collect_unreferenced_claim(path,owner,writer).await? {retired+=1;}

    }
    Ok(retired)
}
pub(super) fn retired_store(path: &Path) -> Result<bool, String> {
    let marker = path.join(".retired");
    let Some(bytes) = read_optional(&marker, 16)? else { return Ok(false) };
    if !bytes.is_empty() { return Err("Invalid drawing retirement marker".into()); }
    Ok(true)
}
pub(super) fn retiring_store(path: &Path) -> Result<bool, String> {
    let Some(bytes) = read_optional(&path.join(".retiring"), 16)? else { return Ok(false) };
    if bytes != b"retiring\n" { return Err("Invalid drawing retirement intent".into()); }
    Ok(true)
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


pub fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    read_file_bounded(file, limit)
}
pub(super) fn read_optional(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let Some(metadata) = metadata_optional(path)? else { return Ok(None) };
    if !metadata.is_file() { return Err("Invalid recovery metadata file".into()); }
    match File::open(path) {
        Ok(file) => read_file_bounded(file, limit).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
pub(super) fn metadata_optional(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
pub(super) fn read_file_bounded(file: File, limit: u64) -> Result<Vec<u8>, String> {
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if length > limit || length > usize::MAX as u64 { return Err("Recovery metadata exceeds admission limit".into()); }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit { return Err("Recovery metadata exceeds admission limit".into()); }
    Ok(bytes)
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
        Err(error) => return Err(storage_error(error)),
    }
    sync_directory(parent)
}
pub fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)] { File::open(path).and_then(|file| file.sync_all()).map_err(storage_error) }
    #[cfg(windows)] { let _ = path; Ok(()) }
}
pub fn atomic_publish(path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    atomic_publish_checked(path, write, ready).map_err(|error| error.error)
}
pub(super) fn atomic_publish_checked(path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), AtomicReplaceError> {
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
pub(super) fn remove_file(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) { Ok(()) => Ok(()), Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()), Err(error) => Err(error.to_string()) }
}

pub struct NativeStoreOpener;
impl StoreOpener for NativeStoreOpener {
    fn storage_facts<'a>(&'a self,path:&'a Path)->super::super::store_io::FileFuture<'a,super::super::storage_budget::StorageFacts> {
        Box::pin(async move {
            let transport=super::super::store_io::NativeTransport::new(path.into());
            super::super::store_io::FileTransport::storage_facts(&transport).await
        })
    }
    fn open<'a>(&'a self,path:&'a Path,retirement:bool)->super::super::store_io::FileFuture<'a,Option<SessionStore>> {
        Box::pin(async move {
            if !retirement {return SessionStore::open(path).await.map(Some);}
            if metadata_optional(path)?.is_none() {return Ok(None);}
            if retired_store(path)? {
                return if store_layout_with(path,true)? {Ok(None)}else {Err("Invalid drawing session directory".into())};
            }
            if !store_layout(path)? {return Err("Invalid drawing session directory".into());}
            let path=fs::canonicalize(path).map_err(|error|error.to_string())?;
            let Some((owner,claim))=claim_store(&path)? else {return Err("Recovery drawing is already open".into())};
            Ok(Some(SessionStore::for_retirement(path,owner,claim)))
        })
    }
}
