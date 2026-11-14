use super::MAX_RANGE_BYTES;
use crate::tile_io::RangeReader;
use std::{future::Future, pin::Pin, sync::Arc};

#[cfg(all(not(target_arch="wasm32"),any(test,feature="test-support")))]
pub mod test_support;

pub type FileFuture<'a,T> = Pin<Box<dyn Future<Output=Result<T,String>>+'a>>;

#[derive(Clone,Copy,PartialEq,Eq)]
pub enum FileAccess {Read,Write,Create}
pub struct DirectoryEntry {pub name:String,pub directory:bool}

pub trait StoreFile: RangeReader {
    fn len(&self)->FileFuture<'_,u64>;
    fn read_at(&self,offset:u64,length:usize)->FileFuture<'_,Arc<[u8]>>;
    fn write_at<'a>(&'a self,offset:u64,bytes:&'a [u8])->FileFuture<'a,()>;
    fn flush(&self)->FileFuture<'_,()>;
    fn read_cold_at(&self,offset:u64,length:usize)->FileFuture<'_,Arc<[u8]>> {self.read_at(offset,length)}
    fn discard_cold(&self,_offset:u64,_length:u64)->FileFuture<'_,()> {Box::pin(async {Ok(())})}
}
#[derive(Clone,Debug,serde::Deserialize,PartialEq,Eq)]
pub struct Removal {pub scope:String,pub bytes:u64}

pub trait FileTransport: Send+Sync {
    fn storage_facts(&self)->FileFuture<'_,super::storage_budget::StorageFacts>;
    fn open<'a>(&'a self,name:&'a str,access:FileAccess)->FileFuture<'a,Arc<dyn StoreFile>>;
    fn open_copy<'a>(&'a self,name:&'a str,access:FileAccess)->FileFuture<'a,Arc<dyn StoreFile>> {self.open(name,access)}
    fn list<'a>(&'a self,directory:&'a str)->FileFuture<'a,Vec<DirectoryEntry>>;
    fn remove<'a>(&'a self,name:&'a str)->FileFuture<'a,Removal>;
}

pub fn check_member(name:&str)->Result<(),String> {
    if name.is_empty() || name.len()>1024 || name.contains(['\\','\0',':']) || name.split('/').any(|part|part.is_empty() || part=="." || part=="..") {
        return Err("Invalid drawing store member".into());
    }
    Ok(())
}
pub fn check_range(offset:u64,length:usize)->Result<(),String> {
    if length>MAX_RANGE_BYTES || offset.checked_add(length as u64).is_none() {Err("Drawing store range exceeds limit".into())}else {Ok(())}
}

#[cfg(not(target_arch="wasm32"))]
mod native {
    use super::*;
    use super::super::storage_budget::storage_error;
    use crate::tile_io::{read_buffer,read_exact_at,verify_crc,ReadCompletion};
    use std::{fs::{self,File,OpenOptions},path::PathBuf,sync::atomic::{AtomicBool,Ordering}};

    pub struct NativeTransport {root:PathBuf}
    impl NativeTransport {
        pub fn new(root:PathBuf)->Self {Self {root}}
        fn member(&self,name:&str)->Result<PathBuf,String> {
            check_member(name)?;
            let mut path=self.root.clone();
            for part in name.split('/') {
                path.push(part);
                match fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.file_type().is_symlink()=>return Err("Drawing store member is a symbolic link".into()),
                    Ok(_)=>{},Err(error) if error.kind()==std::io::ErrorKind::NotFound=>{},Err(error)=>return Err(error.to_string()),
                }
            }Ok(path)
        }
    }
    impl NativeTransport {
        fn open_file<'a>(&'a self,name:&'a str,access:FileAccess,cold:bool)->FileFuture<'a,Arc<dyn StoreFile>> {
            Box::pin(async move {
                let path=self.member(name)?;
                if access==FileAccess::Create {
                    let parent=path.parent().unwrap();
                    super::super::session_store::create_directory(parent)?;
                    if let Some(ancestor)=parent.parent() {super::super::session_store::sync_directory(ancestor)?;}
                }
                let mut options=OpenOptions::new();options.read(true).write(access!=FileAccess::Read).create_new(access==FileAccess::Create);
                #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
                #[cfg(windows)] {use std::os::windows::fs::OpenOptionsExt;if cold {options.custom_flags(0x08000000);}}
                let file=options.open(&path).map_err(storage_error)?;
                #[cfg(any(target_os="macos",target_os="ios"))]
                if cold {use std::os::fd::AsRawFd;unsafe {libc::fcntl(file.as_raw_fd(),libc::F_NOCACHE,1);}}
                let _=cold;
                Ok(Arc::new(NativeFile {file,path,new_name:AtomicBool::new(access==FileAccess::Create),writable:access!=FileAccess::Read}) as Arc<dyn StoreFile>)
            })
        }
    }
    fn existing_path(mut path:&std::path::Path)->Result<&std::path::Path,String> {
        loop {match std::fs::metadata(path) {
            Ok(_)=>return Ok(path),Err(error) if error.kind()==std::io::ErrorKind::NotFound=>{path=path.parent().ok_or("Drawing storage volume is unavailable")?;},
            Err(error)=>return Err(storage_error(error)),
        }}
    }
    fn storage_scope(path:&std::path::Path)->Result<String,String> {
        let path=existing_path(path)?;
        #[cfg(unix)]
        {use std::os::unix::fs::MetadataExt;Ok(std::fs::metadata(path).map_err(storage_error)?.dev().to_string())}
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::{core::PCWSTR,Win32::Storage::FileSystem::GetVolumePathNameW};
            let path=path.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();let mut volume=vec![0u16;32768];
            unsafe {GetVolumePathNameW(PCWSTR(path.as_ptr()),&mut volume).map_err(|error|error.to_string())?;}
            let end=volume.iter().position(|unit|*unit==0).ok_or("Invalid drawing storage volume")?;
            String::from_utf16(&volume[..end]).map_err(|_|"Invalid drawing storage volume".into())
        }
    }
    fn storage_facts(path:&std::path::Path)->Result<super::super::storage_budget::StorageFacts,String> {
        use super::super::storage_budget::{StorageBudget,StorageFacts};
        let epoch=StorageBudget::process().sample_epoch();let path=existing_path(path)?;let scope=storage_scope(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let path=std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_|"Invalid drawing storage path")?;
            let mut facts=std::mem::MaybeUninit::<libc::statvfs>::uninit();
            if unsafe {libc::statvfs(path.as_ptr(),facts.as_mut_ptr())}!=0 {return Err(storage_error(std::io::Error::last_os_error()));}
            let facts=unsafe {facts.assume_init()};
            Ok(StorageFacts {scope,available_bytes:(facts.f_bavail as u64).checked_mul(facts.f_frsize as u64),epoch})
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::{core::PCWSTR,Win32::Storage::FileSystem::GetDiskFreeSpaceExW};
            let path=path.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();let mut available=0u64;
            unsafe {GetDiskFreeSpaceExW(PCWSTR(path.as_ptr()),Some(&mut available),None,None).map_err(|error|error.to_string())?;}
            Ok(StorageFacts {scope,available_bytes:Some(available),epoch})
        }
    }
    struct NativeFile {file:File,path:PathBuf,new_name:AtomicBool,writable:bool}
    impl NativeFile {
        fn read(&self,offset:u64,length:usize)->Result<Arc<[u8]>,String> {
            check_range(offset,length)?;
            let mut bytes=read_buffer(length);read_exact_at(&self.file,offset,Arc::get_mut(&mut bytes).unwrap())?;Ok(bytes)
        }
    }
    impl RangeReader for NativeFile {
        fn allocation_bytes(&self,_:u64,length:usize)->usize {crate::tile_io::buffer_class(length)}
        fn read(&self,offset:u64,length:usize,crc:Option<u32>,mut completion:ReadCompletion) {
            let mut bytes=completion.buffer(length);
            let result=check_range(offset,length).and_then(|_|read_exact_at(&self.file,offset,&mut Arc::get_mut(&mut bytes).unwrap()[..length]))
                .and_then(|_|verify_crc(&bytes[..length],crc)).and_then(|_|super::super::ByteRange::new(bytes,0..length).map_err(str::to_string));
            completion.complete_range(result);
        }
    }
    impl StoreFile for NativeFile {
        fn len(&self)->FileFuture<'_,u64> {Box::pin(async {self.file.metadata().map(|metadata|metadata.len()).map_err(|error|error.to_string())})}
        fn read_at(&self,offset:u64,length:usize)->FileFuture<'_,Arc<[u8]>> {Box::pin(async move {self.read(offset,length)})}
        fn write_at<'a>(&'a self,mut offset:u64,mut bytes:&'a [u8])->FileFuture<'a,()> {
            Box::pin(async move {
                check_range(offset,bytes.len())?;
                if !self.writable {return Err("Drawing store file is read only".into());}
                while !bytes.is_empty() {
                    #[cfg(unix)]
                    let result={use std::os::unix::fs::FileExt;self.file.write_at(bytes,offset)};
                    #[cfg(windows)]
                    let result={use std::os::windows::fs::FileExt;self.file.seek_write(bytes,offset)};
                    let count=match result {Err(error) if error.kind()==std::io::ErrorKind::Interrupted=>continue,result=>result.map_err(storage_error)?};
                    if count==0 {return Err("Incomplete drawing store write".into());}
                    offset+=count as u64;bytes=&bytes[count..];
                }Ok(())
            })
        }
        #[cfg(any(target_os="linux",target_os="android"))]
        fn discard_cold(&self,offset:u64,length:u64)->FileFuture<'_,()> {
            Box::pin(async move {
                use std::os::fd::AsRawFd;
                unsafe {libc::posix_fadvise(self.file.as_raw_fd(),offset as libc::off_t,length as libc::off_t,libc::POSIX_FADV_DONTNEED);}
                Ok(())
            })
        }
        fn flush(&self)->FileFuture<'_,()> {
            Box::pin(async {
                super::super::session_store::sync_file(&self.file).map_err(storage_error)?;
                if self.new_name.load(Ordering::Acquire) {
                    super::super::session_store::sync_directory(self.path.parent().unwrap())?;
                    self.new_name.store(false,Ordering::Release);
                }Ok(())
            })
        }
    }
    impl FileTransport for NativeTransport {
        fn storage_facts(&self)->FileFuture<'_,super::super::storage_budget::StorageFacts> {Box::pin(async {storage_facts(&self.root)})}
        fn open<'a>(&'a self,name:&'a str,access:FileAccess)->FileFuture<'a,Arc<dyn StoreFile>> {self.open_file(name,access,false)}
        fn open_copy<'a>(&'a self,name:&'a str,access:FileAccess)->FileFuture<'a,Arc<dyn StoreFile>> {self.open_file(name,access,true)}
        fn list<'a>(&'a self,directory:&'a str)->FileFuture<'a,Vec<DirectoryEntry>> {
            Box::pin(async move {
                let path=if directory.is_empty() {self.root.clone()}else {self.member(directory)?};
                let mut entries=Vec::new();
                match fs::read_dir(path) {
                    Ok(directory)=>for entry in directory {
                        let entry=entry.map_err(|error|error.to_string())?;let kind=entry.file_type().map_err(|error|error.to_string())?;
                        if !kind.is_file() && !kind.is_dir() {return Err("Invalid drawing store entry".into());}
                        entries.push(DirectoryEntry {name:entry.file_name().into_string().map_err(|_|"Invalid drawing store name")?,directory:kind.is_dir()});
                    },
                    Err(error) if error.kind()==std::io::ErrorKind::NotFound=>{},Err(error)=>return Err(error.to_string()),
                }
                entries.sort_by(|a,b|a.name.cmp(&b.name));Ok(entries)
            })
        }
        fn remove<'a>(&'a self,name:&'a str)->FileFuture<'a,Removal> {
            Box::pin(async move {
                let path=self.member(name)?;let scope=storage_scope(&path)?;
                let (result,bytes)=match fs::symlink_metadata(&path) {Ok(metadata) if metadata.is_dir()=>(fs::remove_dir(&path),0),Ok(metadata)=>(fs::remove_file(&path),metadata.len()),Err(error)=>(Err(error),0)};
                match result {Ok(())=>{super::super::session_store::sync_directory(path.parent().unwrap())?;Ok(Removal {scope,bytes})},Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(Removal {scope,bytes:0}),Err(error)=>Err(error.to_string())}
            })
        }
    }
}
#[cfg(not(target_arch="wasm32"))]
pub use native::NativeTransport;

#[cfg(all(test,not(target_arch="wasm32")))]
mod tests {
    use super::*;
    use std::task::{Context,Poll,Waker};
    fn ready<T>(mut future:FileFuture<'_,T>)->Result<T,String> {
        match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {Poll::Ready(result)=>result,Poll::Pending=>panic!("Native file operation did not finish on its worker")}
    }
    #[test]
    fn native_ranges_preserve_offsets_and_refuse_unbounded_operations() {
        let root=std::env::temp_dir().join(format!("capy-store-io-{}",crate::PortableId::random()));
        let io=NativeTransport::new(root.clone());
        let file=ready(io.open("packs/test.bin",FileAccess::Create)).unwrap();
        ready(file.write_at(0,b"abcdef")).unwrap();ready(file.write_at(2,b"XY")).unwrap();ready(file.flush()).unwrap();
        assert_eq!(ready(file.len()).unwrap(),6);
        assert_eq!(&*ready(file.read_at(1,4)).unwrap(),b"bXYe");
        let other=ready(io.open("packs/test.bin",FileAccess::Read)).unwrap();
        assert_eq!(&*ready(other.read_at(0,2)).unwrap(),b"ab");
        assert!(ready(other.write_at(0,b"no")).is_err());
        assert!(ready(file.read_at(0,MAX_RANGE_BYTES+1)).is_err());
        assert!(ready(file.write_at(u64::MAX,b"no")).is_err());
        assert!(ready(io.open("packs/test.bin",FileAccess::Create)).is_err());
        assert_eq!(ready(io.list("packs")).unwrap().into_iter().map(|entry|entry.name).collect::<Vec<_>>(),["test.bin"]);
        drop((file,other));assert_eq!(ready(io.remove("packs/test.bin")).unwrap().bytes,6);assert_eq!(ready(io.remove("packs/test.bin")).unwrap().bytes,0);
        assert_eq!(ready(io.remove("packs")).unwrap().bytes,0);
        assert!(ready(io.list("packs")).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn member_names_never_escape_the_drawing() {
        for name in ["","/packs/tile","packs//tile","packs/../tile","./tile","packs/./tile","C:/tile","packs\\tile","tile\0"] {assert!(check_member(name).is_err(),"{name:?}");}
        for name in ["head.0","head.1","packs/tile.bin","generations/checkpoint.json"] {check_member(name).unwrap();}
    }
}
