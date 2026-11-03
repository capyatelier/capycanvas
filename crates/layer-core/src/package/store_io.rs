use super::MAX_RANGE_BYTES;
use crate::tile_io::RangeReader;
use std::{future::Future, pin::Pin, sync::Arc};

pub type FileFuture<'a,T> = Pin<Box<dyn Future<Output=Result<T,String>>+'a>>;

#[derive(Clone,Copy,PartialEq,Eq)]
pub enum FileAccess {Read,Write,Create}
pub struct DirectoryEntry {pub name:String,pub directory:bool}

pub trait StoreFile: RangeReader {
    fn len(&self)->FileFuture<'_,u64>;
    fn read_at(&self,offset:u64,length:usize)->FileFuture<'_,Arc<[u8]>>;
    fn write_at<'a>(&'a self,offset:u64,bytes:&'a [u8])->FileFuture<'a,()>;
    fn flush(&self)->FileFuture<'_,()>;
}
pub trait FileTransport: Send+Sync {
    fn open<'a>(&'a self,name:&'a str,access:FileAccess)->FileFuture<'a,Arc<dyn StoreFile>>;
    fn list<'a>(&'a self,directory:&'a str)->FileFuture<'a,Vec<DirectoryEntry>>;
    fn remove<'a>(&'a self,name:&'a str)->FileFuture<'a,()>;
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
    struct NativeFile {file:File,path:PathBuf,new_name:AtomicBool,writable:bool}
    impl NativeFile {
        fn read(&self,offset:u64,length:usize)->Result<Arc<[u8]>,String> {
            check_range(offset,length)?;
            let mut bytes=read_buffer(length);read_exact_at(&self.file,offset,Arc::get_mut(&mut bytes).unwrap())?;Ok(bytes)
        }
    }
    impl RangeReader for NativeFile {
        fn read(&self,offset:u64,length:usize,crc:Option<u32>,completion:ReadCompletion) {
            completion.complete(self.read(offset,length).and_then(|bytes|verify_crc(&bytes,crc).map(|()|bytes)));
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
                    let count=match result {Err(error) if error.kind()==std::io::ErrorKind::Interrupted=>continue,result=>result.map_err(|error|error.to_string())?};
                    if count==0 {return Err("Incomplete drawing store write".into());}
                    offset+=count as u64;bytes=&bytes[count..];
                }Ok(())
            })
        }
        fn flush(&self)->FileFuture<'_,()> {
            Box::pin(async {
                super::super::session_store::sync_file(&self.file).map_err(|error|error.to_string())?;
                if self.new_name.load(Ordering::Acquire) {
                    super::super::session_store::sync_directory(self.path.parent().unwrap())?;
                    self.new_name.store(false,Ordering::Release);
                }Ok(())
            })
        }
    }
    impl FileTransport for NativeTransport {
        fn open<'a>(&'a self,name:&'a str,access:FileAccess)->FileFuture<'a,Arc<dyn StoreFile>> {
            Box::pin(async move {
                let path=self.member(name)?;
                if access==FileAccess::Create {
                    let parent=path.parent().unwrap();
                    super::super::session_store::create_directory(parent)?;
                    if let Some(ancestor)=parent.parent() {super::super::session_store::sync_directory(ancestor)?;}
                }
                let mut options=OpenOptions::new();options.read(true).write(access!=FileAccess::Read).create_new(access==FileAccess::Create);
                #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
                let file=options.open(&path).map_err(|error|error.to_string())?;
                Ok(Arc::new(NativeFile {file,path,new_name:AtomicBool::new(access==FileAccess::Create),writable:access!=FileAccess::Read}) as Arc<dyn StoreFile>)
            })
        }
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
        fn remove<'a>(&'a self,name:&'a str)->FileFuture<'a,()> {
            Box::pin(async move {
                let path=self.member(name)?;
                let result=match fs::symlink_metadata(&path) {Ok(metadata) if metadata.is_dir()=>fs::remove_dir(&path),Ok(_)=>fs::remove_file(&path),Err(error)=>Err(error)};
                match result {Ok(())=>super::super::session_store::sync_directory(path.parent().unwrap()),Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(()),Err(error)=>Err(error.to_string())}
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
        drop((file,other));ready(io.remove("packs/test.bin")).unwrap();ready(io.remove("packs/test.bin")).unwrap();
        assert!(ready(io.list("packs")).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn member_names_never_escape_the_drawing() {
        for name in ["","/packs/tile","packs//tile","packs/../tile","./tile","packs/./tile","C:/tile","packs\\tile","tile\0"] {assert!(check_member(name).is_err(),"{name:?}");}
        for name in ["head.0","head.1","packs/tile.bin","generations/checkpoint.json"] {check_member(name).unwrap();}
    }
}
