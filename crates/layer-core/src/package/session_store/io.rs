use super::*;
use crate::package::store_io::{FileAccess,FileTransport,StoreFile};

pub(super) async fn read_optional(io:&dyn FileTransport,name:&str,limit:u64)->Result<Option<Vec<u8>>,String> {
    let (directory,leaf)=name.rsplit_once('/').unwrap_or(("",name));
    let entries=io.list(directory).await?;
    let Some(entry)=entries.iter().find(|entry|entry.name==leaf) else {return Ok(None);};
    if entry.directory {return Err("Drawing store member is a directory".into());}
    let file=io.open(name,FileAccess::Read).await?;read_file(&*file,limit).await.map(Some)
}
pub(super) async fn read_file(file:&dyn StoreFile,limit:u64)->Result<Vec<u8>,String> {
    let length=file.len().await?;
    if length>limit {return Err("Drawing store member exceeds admission".into());}
    let length=usize::try_from(length).map_err(|_|"Drawing store member exceeds address space")?;
    let mut bytes=Vec::new();bytes.try_reserve_exact(length).map_err(|_|"Drawing store read allocation failed")?;
    while bytes.len()<length {
        let count=(length-bytes.len()).min(MAX_RANGE_BYTES);
        let chunk=file.read_at(bytes.len() as u64,count).await?;
        if chunk.len()!=count {return Err("Incomplete drawing store read".into());}
        bytes.extend_from_slice(&chunk);
    }Ok(bytes)
}
pub(super) async fn create(io:&dyn FileTransport,name:&str,bytes:&[u8],cancelled:&AtomicBool)->Result<(),String> {
    check_cancelled(cancelled)?;let file=io.open(name,FileAccess::Create).await?;
    for (index,chunk) in bytes.chunks(MAX_RANGE_BYTES).enumerate() {
        check_cancelled(cancelled)?;file.write_at((index*MAX_RANGE_BYTES) as u64,chunk).await?;
    }
    file.flush().await?;check_cancelled(cancelled)
}
pub(super) async fn marker(io:&dyn FileTransport,name:&str,expected:&[u8])->Result<bool,String> {
    let Some(bytes)=read_optional(io,name,16).await? else {return Ok(false);};
    if bytes!=expected {return Err("Invalid drawing retirement marker".into());}Ok(true)
}
pub(super) async fn clean(io:&dyn FileTransport,directory:&str,retained:&BTreeSet<PortableId>,suffix:&str)->Result<(),String> {
    for entry in io.list(directory).await? {
        if entry.directory {continue;}
        if entry.name.starts_with(".capy-save-") {
            io.remove(&if directory.is_empty() {entry.name}else {format!("{directory}/{}",entry.name)}).await?;continue;
        }
        let Some(name)=entry.name.strip_suffix(suffix) else {continue;};
        let Ok(id)=name.parse() else {continue;};
        if !retained.contains(&id) {io.remove(&if directory.is_empty() {entry.name}else {format!("{directory}/{}",entry.name)}).await?;}
    }Ok(())
}
