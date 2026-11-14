    pub(super) trait Wait:std::future::Future+Sized {fn wait(self)->Self::Output {pollster::block_on(self)}}
    impl<F:std::future::Future> Wait for F {}
    pub(super) trait Paths {
        fn generation_path(&self,id:PortableId)->PathBuf;
        fn pack_path(&self,id:PortableId)->PathBuf;
    }
    impl Paths for SessionStore {
        fn generation_path(&self,id:PortableId)->PathBuf {self.path.join("generations").join(format!("{id}.json"))}
        fn pack_path(&self,id:PortableId)->PathBuf {self.path.join("packs").join(format!("{id}.bin"))}
    }
    use super::super::store_io::{FileTransport,StoreFile,FileFuture,FileAccess,DirectoryEntry,NativeTransport};
    pub(super) enum Fault {
        Interrupt {operation:usize,after:bool},
        Partial(&'static [u8]),
        Sync(&'static str),
        Block {entered:std::sync::mpsc::Sender<()>,resume:std::sync::mpsc::Receiver<()>},
        Exit(&'static str),
        HeadWrite(usize),
        StorageFull,
        Remove,
    }
    #[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
    pub(super) struct IoCounts {pub opens:u64,pub lists:u64,pub reads:u64,pub facts:u64}
    #[derive(Default)]
    pub(super) struct Control {pub fault:Option<Fault>,pub(super) io:IoCounts,pub(super) space:Option<Option<u64>>,pub(super) count:usize,pub(super) operations:Vec<String>,hold_reads:bool,reads:std::collections::VecDeque<(Arc<dyn StoreFile>,u64,usize,Option<u32>,ReadCompletion)>}
    fn controls()->&'static Mutex<BTreeMap<PathBuf,Weak<Transport>>> {
        static CONTROLS:OnceLock<Mutex<BTreeMap<PathBuf,Weak<Transport>>>>=OnceLock::new();CONTROLS.get_or_init(Default::default)
    }
    pub(super) fn control(store:&SessionStore)->Arc<Mutex<Control>> {controls().lock().unwrap()[&store.path].upgrade().unwrap().control.clone()}
    struct Transport {inner:Arc<dyn FileTransport>,control:Arc<Mutex<Control>>,lease:Arc<dyn Send+Sync>}
    struct TransportFile {inner:Arc<dyn StoreFile>,name:String,control:Arc<Mutex<Control>>}
    impl RangeReader for TransportFile {
        fn transfer_key(&self)->Option<String> {Some(format!("test-store:{}",self.name))}
        fn allocation_bytes(&self,offset:u64,length:usize)->usize {self.inner.allocation_bytes(offset,length)}
        fn read(&self,offset:u64,length:usize,crc:Option<u32>,completion:ReadCompletion) {
            let mut state=self.control.lock().unwrap();
            if state.hold_reads {state.reads.push_back((self.inner.clone(),offset,length,crc,completion));return;}
            drop(state);self.inner.read(offset,length,crc,completion)
        }
    }
    impl StoreFile for TransportFile {
        fn len(&self)->FileFuture<'_,u64> {self.inner.len()}
        fn read_at(&self,offset:u64,length:usize)->FileFuture<'_,Arc<[u8]>> {self.control.lock().unwrap().io.reads+=1;self.inner.read_at(offset,length)}
        fn write_at<'a>(&'a self,offset:u64,bytes:&'a [u8])->FileFuture<'a,()> {
            Box::pin(async move {
                if matches!(self.control.lock().unwrap().fault,Some(Fault::StorageFull)) {return Err(crate::package::storage_budget::STORAGE_UNAVAILABLE.into());}
                let head_write=if self.name.starts_with("head.") {match self.control.lock().unwrap().fault {Some(Fault::HeadWrite(length))=>Some(length),_=>None}}else {None};
                if let Some(length)=head_write {if length>0 {self.inner.write_at(offset,&bytes[..length.min(bytes.len())]).await?;}return Err("Injected head write failed".into());}
                let interrupted={let state=self.control.lock().unwrap();matches!(state.fault,Some(Fault::Interrupt {operation,after:false}) if operation==state.count+1)};
                if interrupted {self.inner.write_at(offset,&bytes[..bytes.len().min(7)]).await?;return Err("Interrupted write".into());}
                let exit=match self.control.lock().unwrap().fault {Some(Fault::Exit(stage))=>self.name.starts_with(stage),_=>false};
                if exit {self.inner.write_at(offset,&bytes[..bytes.len().min(7)]).await?;self.inner.flush().await?;std::process::exit(87);}
                let partial=if self.name.starts_with("packs/") {match self.control.lock().unwrap().fault {Some(Fault::Partial(bytes))=>Some(bytes),_=>None}}else {None};
                if let Some(partial)=partial {self.inner.write_at(offset,partial).await?;self.inner.flush().await?;return Err(std::io::Error::from(std::io::ErrorKind::StorageFull).to_string());}
                self.inner.write_at(offset,bytes).await
            })
        }
        fn flush(&self)->FileFuture<'_,()> {
            Box::pin(async move {
                let (interrupt,after,block,error)={
                    let mut state=self.control.lock().unwrap();state.count+=1;
                    state.operations.push(if self.name.starts_with("packs/") {"append"}else if self.name.starts_with("generations/") {"generations"}else {"head"}.into());
                    match state.fault.as_ref() {
                        Some(Fault::Interrupt {operation,after})=>(state.count==*operation,*after,None,None),
                        Some(Fault::Sync(error)) if self.name.starts_with("packs/")=>(false,false,None,Some(*error)),
                        Some(Fault::Block {..}) if self.name.starts_with("packs/")=>{
                            let Some(Fault::Block {entered,resume})=state.fault.take() else {unreachable!()};(false,false,Some((entered,resume)),None)
                        },_ =>(false,false,None,None),
                    }
                };
                if let Some((entered,resume))=block {entered.send(()).unwrap();resume.recv().unwrap();}
                if let Some(error)=error {return Err(error.into());}
                if interrupt && !after {return Err("Interrupted write".into());}
                self.inner.flush().await?;
                if interrupt {return Err("Interrupted flush".into());}Ok(())
            })
        }
    }
    impl FileTransport for Transport {
        fn storage_facts(&self)->FileFuture<'_,crate::package::storage_budget::StorageFacts> {Box::pin(async {
            self.control.lock().unwrap().io.facts+=1;
            let mut facts=self.inner.storage_facts().await?;
            if let Some(space)=self.control.lock().unwrap().space {facts.scope=format!("test:{:p}",Arc::as_ptr(&self.control));facts.available_bytes=space;}
            Ok(facts)
        })}
        fn open<'a>(&'a self,name:&'a str,access:FileAccess)->FileFuture<'a,Arc<dyn StoreFile>> {Box::pin(async move {
            self.control.lock().unwrap().io.opens+=1;
            Ok(Arc::new(TransportFile {inner:self.inner.open(name,access).await?,name:name.into(),control:self.control.clone()}) as Arc<dyn StoreFile>)
        })}
        fn list<'a>(&'a self,name:&'a str)->FileFuture<'a,Vec<DirectoryEntry>> {self.control.lock().unwrap().io.lists+=1;self.inner.list(name)}
        fn remove<'a>(&'a self,name:&'a str)->FileFuture<'a,super::super::store_io::Removal> {Box::pin(async move {
            if matches!(self.control.lock().unwrap().fault,Some(Fault::Remove)) {return Err("Interrupted remove".into());}
            let mut removed=self.inner.remove(name).await?;
            if self.control.lock().unwrap().space.is_some() {removed.scope=format!("test:{:p}",Arc::as_ptr(&self.control));}Ok(removed)
        })}
    }
    pub(super) async fn open_store(path:&Path)->Result<SessionStore,String> {
        create_directory(path)?;let path=fs::canonicalize(path).map_err(|error|error.to_string())?;
        for name in ["resources","generations","packs"] {create_directory(&path.join(name))?;}
        let existing=controls().lock().unwrap().get(&path).and_then(Weak::upgrade);
        let transport=match existing {
            Some(transport)=>transport,
            None=>{
                let lease=Arc::new(OwnerLock::claim(&path.join(".lock"))?.ok_or("Recovery drawing is already open")?);
                let transport=Arc::new(Transport {inner:Arc::new(NativeTransport::new(path.clone())),control:Default::default(),lease});
                controls().lock().unwrap().insert(path.clone(),Arc::downgrade(&transport));transport
            },
        };
        SessionStore::open_transport(path,transport.clone(),transport.lease.clone(),ReadCache::process()).await
    }
    fn fault_checkpoint(store:&mut SessionStore,capture:&Captured,cancel:&AtomicBool,fault:Fault)->Result<PortableId,String> {
        let state=control(store);*state.lock().unwrap()=Control {fault:Some(fault),..Default::default()};
        let result=checkpoint(store,capture,cancel);*state.lock().unwrap()=Control::default();result
    }
    fn recorded_checkpoint(store:&mut SessionStore,capture:&Captured,cancel:&AtomicBool,operations:&mut Vec<String>)->Result<PortableId,String> {
        let state=control(store);*state.lock().unwrap()=Control::default();let result=checkpoint(store,capture,cancel);
        operations.extend(std::mem::take(&mut state.lock().unwrap().operations));result
    }
    pub(super) fn read_head(path:&Path)->Result<Vec<u8>,String> {
        let io=NativeTransport::new(path.into());
        match head::read(&io).wait()? {
            Some(published)=>serde_json::to_vec(&published.head).map_err(|error|error.to_string()),
            None=>fs::read(path.join("head.json")).map_err(|error|error.to_string()),
        }
    }
    fn write_head(path:&Path,bytes:&[u8])->Result<(),String> {
        let io=NativeTransport::new(path.into());let sequence=head::read(&io).wait()?.map_or(0,|published|published.sequence);
        match parse_head(bytes) {
            Ok(head)=>head::publish(&io,sequence,&head).wait().map(|_|()),
            Err(_)=>{
                for name in ["head.0","head.1"] {
                    if !path.join(name).exists() {continue;}
                    let mut slot=fs::read(path.join(name)).map_err(|error|error.to_string())?;
                    slot[16..20].copy_from_slice(&(bytes.len() as u32).to_le_bytes());slot[24..].fill(0);slot[24..24+bytes.len()].copy_from_slice(bytes);
                    let mut hash=crc32fast::Hasher::new();hash.update(&slot[8..20]);hash.update(&slot[24..]);slot[20..24].copy_from_slice(&hash.finalize().to_le_bytes());
                    let file=io.open(name,FileAccess::Write).wait()?;file.write_at(0,&slot).wait()?;file.flush().wait()?;
                }Ok(())
            },
        }
    }
    fn head_path(path:&Path)->PathBuf {
        ["head.0","head.1"].into_iter().filter_map(|name| {
            let bytes=fs::read(path.join(name)).ok()?;
            Some((u64::from_le_bytes(bytes.get(8..16)?.try_into().ok()?),path.join(name)))
        }).max_by_key(|(sequence,_)|*sequence).map_or_else(||path.join("head.json"),|(_,path)|path)
    }
    use super::*;
    use std::{fs::{self,OpenOptions},io::Write,sync::OnceLock};
    use super::native::import_sessions_using;
    #[cfg(windows)]
    use super::native::atomic_publish_checked;
    use crate::{Document, DocumentNames, Editor, raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey}};
    use serde_json::json;
    use super::super::session::{EditorCapture,SessionMetadata};
    pub(crate) struct SavedSession {store:std::cell::RefCell<Option<SessionStore>>,session:Vec<u8>,_directory:Directory}
    impl SavedSession {
        pub(crate) fn new(capture:&EditorCapture,metadata:impl Into<SessionMetadata>,cancel:&AtomicBool)->Result<Self,String> {
            let directory=Directory::new();let mut store=SessionStore::open(&directory.0).wait()?;
            let id=store.checkpoint(capture,metadata.into(),cancel).wait()?;
            let bytes=store.generation_metadata(&store.head.as_ref().unwrap().current,ProjectLimits::default().metadata_bytes).wait()?;
            let session=StoredGeneration::parse(&bytes,ProjectLimits::default()).map_err(|error|error.message())?.metadata;
            assert_eq!(store.generation(),Some(id));Ok(Self {store:std::cell::RefCell::new(Some(store)),session,_directory:directory})
        }
        pub(crate) fn metadata(&self)->&[u8] {&self.session}
        pub(crate) fn close(&self) {self.store.borrow_mut().take();}
        pub(crate) fn load(&self)->Result<OpenSession,String> {
            let mut store=self.store.borrow_mut();if store.is_none() {*store=Some(SessionStore::open(&self._directory.0).wait()?);}
            store.as_mut().unwrap().load(ProjectLimits::default(),&AtomicBool::new(false)).wait()?.ok_or("Missing test checkpoint".into())
        }
        pub(crate) fn load_metadata(&self,value:serde_json::Value)->Result<OpenSession,String> {
            let mut borrowed=self.store.borrow_mut();let store=borrowed.as_mut().unwrap();let reference=store.head.as_ref().unwrap().current.clone();
            let mut packed:serde_json::Value=serde_json::from_slice(&fs::read(store.generation_path(reference.id)).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
            packed["session"]=value;let bytes=serde_json::to_vec(&packed).map_err(|error|error.to_string())?;
            fs::write(store.generation_path(reference.id),&bytes).map_err(|error|error.to_string())?;
            let mut current=store.head.clone().unwrap();current.current=Generation::new(reference.id,&bytes);
            head::publish(&*store._owner.io,store.sequence,&current).wait()?;
            store.load(ProjectLimits::default(),&AtomicBool::new(false)).wait()?.ok_or("Missing test checkpoint".into())
        }
        pub(crate) fn resource_digests(&self)->BTreeMap<String,String> {
            self.store.borrow().as_ref().unwrap().index.iter().map(|(id,entry)| {
                let bytes=entry.location.read_all().wait().unwrap();(id.to_string(),Sha256::digest(&bytes).iter().map(|byte|format!("{byte:02x}")).collect())
            }).collect()
        }
        pub(crate) fn resource_count(&self)->usize {self.store.borrow().as_ref().unwrap().index.len()}
        pub(crate) fn corrupt_payload(&self,missing:bool) {
            let borrowed=self.store.borrow();let store=borrowed.as_ref().unwrap();let location=&store.index.values().next().unwrap().location;let path=store.pack_path(location.range().pack.id);
            if missing {fs::remove_file(path).unwrap();}else {let mut bytes=fs::read(&path).unwrap();bytes[location.range().offset as usize]^=1;fs::write(path,bytes).unwrap();}
        }
    }
    struct Captured {editor:EditorCapture,metadata:SessionMetadata,tiles:Vec<Arc<TileBlob>>}
    fn captured(editor:&EditorCapture,metadata:impl Into<SessionMetadata>)->Captured {
        Captured {editor:editor.clone(),metadata:metadata.into(),tiles:editor.retained_tiles().blobs().unwrap()}
    }
    fn checkpoint(store:&mut SessionStore,capture:&Captured,cancel:&AtomicBool)->Result<PortableId,String> {
        store.checkpoint(&capture.editor,capture.metadata.clone(),cancel).wait()
    }
    fn interrupted_checkpoint(store:&mut SessionStore,capture:&Captured,cancel:&AtomicBool,interruption:Interrupted)->Result<PortableId,String> {
        let control=control(store);*control.lock().unwrap()=Control {fault:Some(Fault::Interrupt {operation:interruption.operation,after:interruption.after}),..Default::default()};
        let result=checkpoint(store,capture,cancel);*control.lock().unwrap()=Control::default();result
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
    #[test]
    fn full_disk_keeps_pressure_and_checkpoint_bytes_in_ram_without_publishing() {
        use crate::package::storage_budget::STORAGE_UNAVAILABLE;
        for authoritative in [false,true] {for operation in ["pressure","checkpoint"] {
            let directory=Directory::new();let cancel=AtomicBool::new(false);let mut store=open_store(&directory.0).wait().unwrap();
            let old=capture(5);let head=checkpoint(&mut store,&old,&cancel).unwrap();
            let incoming=capture(9);let before=incoming.tiles.iter().map(|tile|tile.resident_bytes()).sum::<usize>();assert!(before>0);
            let state=control(&store);*state.lock().unwrap()=Control {space:Some(Some(if authoritative {1<<30}else {0})),fault:authoritative.then_some(Fault::StorageFull),..Default::default()};
            let result=match operation {
                "pressure"=>store.make_durable(&crate::raster_storage::RetainedTiles {sources:incoming.tiles.clone(),..Default::default()},&cancel).wait().map(|_|()),
                _=>checkpoint(&mut store,&incoming,&cancel).map(|_|()),
            };
            assert_eq!(result.unwrap_err(),STORAGE_UNAVAILABLE,"{operation}; authoritative={authoritative}");
            assert_eq!(store.generation(),Some(head));assert_eq!(head::read(&*store._owner.io).wait().unwrap().unwrap().head.current.id,head);
            assert_eq!(incoming.tiles.iter().map(|tile|tile.resident_bytes()).sum::<usize>(),before);
            if !authoritative {assert!(state.lock().unwrap().operations.is_empty());}
            *state.lock().unwrap()=Control::default();
            assert!(checkpoint(&mut store,&incoming,&cancel).is_ok());
        }}
    }
    #[test]
    fn async_stored_tile_read_retains_completed_bytes_until_its_lease_is_consumed() {
        use std::{future::Future,task::{Context,Poll,Waker}};
        let directory=Directory::new();let cancel=AtomicBool::new(false);
        let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {total_bytes:Some(1<<30),available_bytes:Some(1<<30),override_bytes:Some(1<<30),..Default::default()});
        let cache=ReadCache::new(budget.clone());cache.set_scheduler(Arc::new(|_|{}));
        let tile=Arc::new(TileBlob::encode(RasterPlane::Color.descriptor(Default::default()),&vec![53;256*256*4]).unwrap());
        let expected=tile.compressed_on_worker().unwrap().into_arc();
        let mut store=open_store(&directory.0).wait().unwrap();store.reads=cache.clone();
        store.make_durable(&crate::raster_storage::RetainedTiles {sources:vec![tile.clone()],..Default::default()},&cancel).wait().unwrap();
        assert_eq!(tile.resident_bytes(),0);let location=tile.compressed.location().unwrap().unwrap();
        let drain=budget.admit_import(budget.import_headroom_bytes()).unwrap();budget.flush_reports();assert_eq!(cache.limit_bytes(),0);
        let physical=budget.snapshot().ram_physical_bytes;let control=control(&store);control.lock().unwrap().hold_reads=true;
        let mut read=Box::pin(tile.read_compressed());let mut context=Context::from_waker(Waker::noop());
        assert!(read.as_mut().poll(&mut context).is_pending());cache.drive();
        let (file,offset,length,crc,completion)=control.lock().unwrap().reads.pop_front().expect("one leased tile read");
        let retained=file.allocation_bytes(offset,length) as u64;
        assert_eq!(crc,Some(location.crc));file.read(offset,length,crc,completion);
        location.range().source.evict();cache.drive();budget.flush_reports();
        assert_eq!(budget.snapshot().ram_physical_bytes,physical+retained);
        let Poll::Ready(Ok(bytes))=read.as_mut().poll(&mut context) else {panic!("completed tile read retains its result")};drop(read);
        assert_eq!(&*bytes,&*expected);assert_eq!(crc32fast::hash(&bytes),location.crc);assert_eq!(bytes.retained_bytes() as u64,retained);
        location.range().source.evict();budget.flush_reports();assert_eq!(budget.snapshot().ram_physical_bytes,physical+retained);
        drop(bytes);location.range().source.evict();budget.flush_reports();assert_eq!(cache.limit_bytes(),0);
        cache.drive();budget.flush_reports();
        assert_eq!(budget.snapshot().ram_physical_bytes,physical);assert!(control.lock().unwrap().reads.is_empty());drop(drain);
    }
    #[test]
    fn a_session_open_reads_its_resources_only_into_room_another_owner_has_released() {
        let directory=Directory::new();let cancel=AtomicBool::new(false);
        let mut document=Document::new(PortableId::random(),64,64,DocumentNames {paint:"Paint".into(),paper:"Paper".into()});
        document.artwork.metadata=Arc::new(crate::PhotoMetadata {xmp:Some(crate::authored::Resource::from(vec![7;64*1024])),..Default::default()});
        let editor=Editor::new(document);
        let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
        SessionStore::open(&directory.0).wait().unwrap().checkpoint(&capture,json!({}).into(),&cancel).wait().unwrap();
        let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {override_bytes:Some(1<<30),..Default::default()});
        let limit=budget.ram_limit_bytes();
        let cache=budget.register_owner(crate::MemoryOwnerKind::Other,crate::MemoryClass::Warm,None);
        cache.report(crate::MemoryReport {ram_bytes:limit,demand_ram_bytes:limit,..Default::default()});
        let (send,receive)=std::sync::mpsc::channel();
        let worker={let path=directory.0.clone();let budget=budget.clone();std::thread::spawn(move || {
            let mut store=SessionStore::open(&path).wait().unwrap();
            send.send(store.load_with_budget(ProjectLimits::default(),&AtomicBool::new(false),budget).wait().map(|session|session.is_some())).unwrap();
        })};
        assert!(receive.recv_timeout(std::time::Duration::from_millis(500)).is_err(),"the open read its resources into room another owner still holds");
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
        let opened=loop {
            cache.report(crate::MemoryReport {ram_bytes:cache.grant().ram_bytes,demand_ram_bytes:limit,..Default::default()});
            let snapshot=budget.snapshot();
            assert!(snapshot.ram_physical_bytes+snapshot.reserved_bytes<=limit,"physical {} reserved {} limit {limit}",snapshot.ram_physical_bytes,snapshot.reserved_bytes);
            match receive.recv_timeout(std::time::Duration::from_millis(20)) {Ok(opened)=>break opened,Err(_)=>assert!(std::time::Instant::now()<deadline,"the cache's releases never let the open finish")}
        };
        assert_eq!(opened,Ok(true));
        worker.join().unwrap();
    }
    #[test]
    fn oversized_known_resource_is_rejected_before_payload_reads() {
        struct CountedFile {inner:Arc<dyn StoreFile>,reads:Arc<AtomicU64>}
        impl RangeReader for CountedFile {
            fn allocation_bytes(&self,offset:u64,length:usize)->usize {self.inner.allocation_bytes(offset,length)}
            fn read(&self,offset:u64,length:usize,crc:Option<u32>,completion:ReadCompletion) {
                self.reads.fetch_add(1,Ordering::Relaxed);self.inner.read(offset,length,crc,completion)
            }
        }
        impl StoreFile for CountedFile {
            fn len(&self)->FileFuture<'_,u64> {self.inner.len()}
            fn read_at(&self,offset:u64,length:usize)->FileFuture<'_,Arc<[u8]>> {self.reads.fetch_add(1,Ordering::Relaxed);self.inner.read_at(offset,length)}
            fn write_at<'a>(&'a self,offset:u64,bytes:&'a [u8])->FileFuture<'a,()> {self.inner.write_at(offset,bytes)}
            fn flush(&self)->FileFuture<'_,()> {self.inner.flush()}
        }
        let directory=Directory::new();let store=SessionStore::open(&directory.0).wait().unwrap();
        let length=4*1024*1024+1;let id=PortableId::random();let pack_id=PortableId::random();
        let file=store._owner.io.open(&format!("packs/{pack_id}.bin"),FileAccess::Create).wait().unwrap();
        file.write_at(length-1,&[0]).wait().unwrap();file.flush().wait().unwrap();
        let reads=Arc::new(AtomicU64::new(0));let cache=ReadCache::new(crate::MemoryBudget::default());
        let pack=Arc::new(Pack {id:pack_id,file:Arc::new(CountedFile {inner:file,reads:reads.clone()}),owner:store._owner.clone(),source:Default::default(),extent:AtomicU64::new(length)});
        let location=Arc::new(StoreLocation::new(pack.clone(),pack.source(&cache),0,length,0,[0;32]));
        let mut resources=StoreResources {locations:[(id,location)].into(),..Default::default()};
        let metadata=json!({"resources":[{"id":id,"type":"capy.wgsl/1","encoding":"utf8","bytes":length.to_string(),"crc32":"00000000","data":{},"location":{"pack":"data/tiles-1.bin","offset":"0"}}]});
        assert!(resources.prepare(&metadata,&crate::MemoryBudget::default()).wait().is_err());
        assert_eq!(reads.load(Ordering::Relaxed),0,"encoded bounds precede transport payload reads");
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
        let cancel = AtomicBool::new(false); let mut store = SessionStore::open(path).wait().unwrap();
        let mut opened = store.load(ProjectLimits::default(), &cancel).wait().unwrap().unwrap();
        assert_eq!(store.loaded_generation(), store.generation());
        restored_history(&mut opened)
    }
    fn restored_history(opened:&mut OpenSession)->serde_json::Value {
        let cancel=AtomicBool::new(false);
        let snapshot = |editor: &Editor| {
            let capture = editor.capture_session(editor.capture(11, editor.document().output().context.clone()).unwrap()).unwrap();
            let prepared = SavedSession::new(&capture, opened.metadata.clone(), &cancel).unwrap();
            let mut metadata: serde_json::Value = serde_json::from_slice(prepared.metadata()).unwrap(); metadata["checkpoint"]["owner"] = 0.into();
            let resources=prepared.resource_digests();
            for record in metadata["resources"].as_array_mut().unwrap() {record.as_object_mut().unwrap().remove("location");}
            json!({"metadata":metadata,"resources":resources})
        };
        let current = snapshot(&opened.editor); let mut history = Vec::new();
        while opened.editor.can_undo() { opened.editor.undo().unwrap(); history.push(snapshot(&opened.editor)); }
        while opened.editor.can_redo() { opened.editor.redo().unwrap(); history.push(snapshot(&opened.editor)); }
        assert!(!history.is_empty()); json!({"current":current,"history":history})
    }
    #[test]
    fn released_generation_import_preserves_history_aliases_and_fallback_without_changing_inputs() {
        fn input_bytes(root:&Path,owner_lock:Option<&Path>)->BTreeMap<PathBuf,Vec<u8>> {
            let mut bytes=BTreeMap::new();
            for entry in fs::read_dir(root).unwrap() {
                let path=entry.unwrap().path();
                if Some(path.as_path())==owner_lock {continue;}
                if path.is_dir() {for (name,value) in input_bytes(&path,owner_lock) {bytes.insert(path.file_name().map(PathBuf::from).unwrap().join(name),value);}}
                else {bytes.insert(path.file_name().map(PathBuf::from).unwrap(),fs::read(path).unwrap());}
            }bytes
        }
        let fixture=Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat/1.0.15/session");
        let original=input_bytes(&fixture,None);let head:Head=serde_json::from_slice(&fs::read(fixture.join("head.json")).unwrap()).unwrap();
        let metadata=fs::read(fixture.join("generations").join(format!("{}.json",head.current.id))).unwrap();
        let value:serde_json::Value=serde_json::from_slice(&metadata).unwrap();
        let mut ranges=BTreeMap::<(String,u64,u64),ImmutableBacking>::new();let mut resources=BTreeMap::new();
        let mut aliases=BTreeMap::<u64,Vec<PortableId>>::new();
        for record in value["resources"].as_array().unwrap() {
            let id:PortableId=record["id"].as_str().unwrap().parse().unwrap();
            let bytes=fs::read(fixture.join("resources").join(format!("{id}.bin"))).unwrap();
            let key=(record["location"]["pack"].as_str().unwrap().to_owned(),super::super::manifest::decimal_u64(&record["location"]["offset"]).unwrap(),super::super::manifest::decimal_u64(&record["bytes"]).unwrap());
            let backing=ranges.entry(key).or_insert_with(||ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes.clone()))).unwrap()).clone();
            assert_eq!(&*backing.read_on_worker(0,bytes.len()).unwrap(),bytes);
            aliases.entry(backing.identity()).or_default().push(id);resources.insert(id,backing);
        }
        assert!(aliases.values().any(|ids|ids.len()>1));
        let directory=Directory::new();let baseline=Directory::new();copy_fixture(&fixture,&baseline.0);
        let expected=session_history(&baseline.0);let cancel=AtomicBool::new(false);
        let mut store=SessionStore::open(&directory.0).wait().unwrap();
        let mut previous=value.clone();previous["metadata"]["imported_previous"]=true.into();
        let previous=store.import_generation(None,&serde_json::to_vec(&previous).unwrap(),resources.clone(),&cancel).wait().unwrap();
        let mut opened=store.load(ProjectLimits::default(),&cancel).wait().unwrap().unwrap();
        let expected_previous=restored_history(&mut opened);drop(opened);
        let lengths=fs::read_dir(directory.0.join("packs")).unwrap().map(|entry|entry.unwrap().metadata().unwrap().len()).sum::<u64>();
        assert_eq!(lengths,ranges.keys().map(|(_,_,length)|(*length).max(1)).sum::<u64>());
        let current=store.import_generation(Some(previous),&metadata,resources.clone(),&cancel).wait().unwrap();
        assert_eq!(fs::read_dir(directory.0.join("packs")).unwrap().map(|entry|entry.unwrap().metadata().unwrap().len()).sum::<u64>(),lengths);
        for ids in aliases.values().filter(|ids|ids.len()>1) {
            let first=&store.index[&ids[0]].location;
            for id in &ids[1..] {let alias=&store.index[id].location;assert_eq!(first.range().pack.id,alias.range().pack.id);assert_eq!(first.range().offset,alias.range().offset);}
        }
        let mut opened=store.load(ProjectLimits::default(),&cancel).wait().unwrap().unwrap();assert_eq!(restored_history(&mut opened),expected);drop(opened);
        let owner_lock=directory.0.join(".lock");assert!(OwnerLock::claim(&owner_lock).unwrap().is_none());
        let before=input_bytes(&directory.0,Some(&owner_lock));
        assert!(matches!(store.import_generation(Some(previous),&metadata,resources.clone(),&cancel).wait(),Err(ImportFailure::Changed)));
        assert_eq!(store.generation(),Some(current));assert_eq!(input_bytes(&directory.0,Some(&owner_lock)),before);assert!(OwnerLock::claim(&owner_lock).unwrap().is_none());
        fs::write(store.generation_path(current),b"incomplete imported generation").unwrap();
        let mut opened=store.load(ProjectLimits::default(),&cancel).wait().unwrap().unwrap();assert_eq!(store.loaded_generation(),Some(previous));assert_eq!(restored_history(&mut opened),expected_previous);drop(opened);
        let bad=Directory::new();let mut bad_store=SessionStore::open(&bad.0).wait().unwrap();
        let (&id,backing)=resources.first_key_value().unwrap();let mut bytes=backing.read_on_worker(0,backing.byte_len() as usize).unwrap().to_vec();bytes[0]^=1;
        resources.insert(id,ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap());
        assert!(bad_store.import_generation(None,&metadata,resources,&cancel).wait().unwrap_err().message().contains("checksum"));
        assert!(bad_store.generation().is_none());assert!(head::read(&*bad_store._owner.io).wait().unwrap().is_none());
        assert_eq!(input_bytes(&fixture,None),original);
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
            collect_unreferenced_stores(root, &BTreeSet::new(), &AtomicBool::new(false)).wait();
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
    #[test]
    fn two_thousand_tile_checkpoint_syncs_packs_and_reuses_existing_payloads() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let prepared = many_tiles(2_000);
        let mut store = open_store(&directory.0).wait().unwrap();
        let mut operations = Vec::new();
        recorded_checkpoint(&mut store, &prepared, &cancel, &mut operations).unwrap();
        assert_eq!(operations.len(), 3);
        assert_eq!(&operations[..2], &["append", "generations"]);
        assert_eq!(count(&directory.0.join("resources")), 0);
        let bytes = store.generation_metadata(&store.head.as_ref().unwrap().current, ProjectLimits::default().metadata_bytes).wait().unwrap();
        let stored = StoredGeneration::parse(&bytes, ProjectLimits::default()).unwrap_or_else(|e| panic!("{}", e.message()));
        let locations = stored.locations.unwrap();
        let mut spatial=super::super::session::SessionDraft::capture(&prepared.editor,prepared.metadata.clone(),&cancel).unwrap().spatial_order().into_iter().map(|(id,(_,y,x))|(id,[x,y])).collect::<Vec<_>>();spatial.sort_by_key(|(_,coordinate)|[coordinate[1],coordinate[0]]);
        assert_eq!(spatial.len(), 2_000);
        assert!(spatial.windows(2).all(|pair| locations[&pair[0].0].offset < locations[&pair[1].0].offset));
        let pack = resource_owner(&store, tile_resource(&prepared).payload.resource_id(), tile_resource(&prepared).bytes);
        let bytes = fs::read(&pack.path).unwrap();
        operations.clear();
        recorded_checkpoint(&mut store, &prepared, &cancel, &mut operations).unwrap();
        assert_eq!(operations.len(), 2);
        assert_eq!(fs::read(&pack.path).unwrap(), bytes);
        operations.clear();
        recorded_checkpoint(&mut store, &capture(42), &cancel, &mut operations).unwrap();
        assert_eq!(&operations[..2], &["append", "generations"]);
        assert_eq!(count(&directory.0.join("packs")), 1);
        assert_eq!(marker(&mut store), 42);
    }
    #[test]
    fn a_truncated_committed_pack_is_never_reused_for_append() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let mut store = SessionStore::open(&directory.0).wait().unwrap();
        let first = capture(1); checkpoint(&mut store, &first, &cancel).unwrap();
        let pack = resource_owner(&store, tile_resource(&first).payload.resource_id(), tile_resource(&first).bytes);
        let head = read_head(&directory.0).unwrap();
        OpenOptions::new().write(true).open(&pack.path).unwrap().set_len(0).unwrap();
        assert_eq!(checkpoint(&mut store, &capture(2), &cancel).unwrap_err(), "Recovery pack is shorter than its committed extent");
        assert_eq!(fs::metadata(&pack.path).unwrap().len(), 0);
        assert_eq!(read_head(&directory.0).unwrap(), head);
    }
    #[test]
    fn failed_partial_append_keeps_last_good_extent_and_skips_its_tail() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let original = capture(1); let second = capture(2);
        let mut store = open_store(&directory.0).wait().unwrap(); checkpoint(&mut store, &original, &cancel).unwrap();
        let owner = resource_owner(&store, tile_resource(&original).payload.resource_id(), tile_resource(&original).bytes);
        let committed = fs::metadata(&owner.path).unwrap().len();
        let head = read_head(&directory.0).unwrap();
        assert!(fault_checkpoint(&mut store,&second,&cancel,Fault::Partial(b"partial append")).is_err());
        let tail = fs::metadata(&owner.path).unwrap().len(); assert!(tail > committed);
        assert_eq!(read_head(&directory.0).unwrap(), head);
        assert_eq!(marker(&mut store), 1);
        drop(store);
        let mut store = open_store(&directory.0).wait().unwrap();
        assert_eq!(fs::metadata(&owner.path).unwrap().len(), tail);
        checkpoint(&mut store, &second, &cancel).unwrap();
        let next = resource_owner(&store, tile_resource(&second).payload.resource_id(), tile_resource(&second).bytes);
        assert_eq!(owner.path, next.path); assert!(next.range().offset >= tail);
        assert_eq!(marker(&mut store), 2);
        let bytes = fs::read(&owner.path).unwrap(); assert_eq!(&bytes[committed as usize..tail as usize], b"partial append");
    }
    #[test]
    fn failed_append_sync_preserves_the_last_good_checkpoint() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let mut store = open_store(&directory.0).wait().unwrap(); checkpoint(&mut store, &capture(1), &cancel).unwrap();
        let head = read_head(&directory.0).unwrap();
        assert_eq!(fault_checkpoint(&mut store,&capture(2),&cancel,Fault::Sync("Pack data sync failed")).unwrap_err(), "Pack data sync failed");
        assert_eq!(read_head(&directory.0).unwrap(), head); drop(store);
        let mut store = open_store(&directory.0).wait().unwrap(); assert_eq!(marker(&mut store), 1);
        checkpoint(&mut store, &capture(3), &cancel).unwrap(); assert_eq!(marker(&mut store), 3);
    }
    #[test]
    fn repeated_failed_rollovers_collect_only_unpublished_whole_packs() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false); let first = capture(1); let second = capture(2);
        let mut store = open_store(&directory.0).wait().unwrap(); checkpoint(&mut store, &first, &cancel).unwrap();
        let owner = resource_owner(&store, tile_resource(&first).payload.resource_id(), tile_resource(&first).bytes);
        OpenOptions::new().write(true).open(&owner.path).unwrap().set_len(PACK_BYTES).unwrap();
        for _ in 0..10 {
            assert!(interrupted_checkpoint(&mut store, &second, &cancel, Interrupted { operation: 2, after: false }).is_err());
            assert_eq!(count(&directory.0.join("packs")), 2); assert_eq!(marker(&mut store), 1);
        }
        checkpoint(&mut store, &second, &cancel).unwrap(); assert_eq!(count(&directory.0.join("packs")), 2); assert_eq!(marker(&mut store), 2);
    }
    #[test]
    fn a_resource_retained_only_by_previous_is_reused_without_append() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false); let first = capture(1);
        let mut store = open_store(&directory.0).wait().unwrap(); checkpoint(&mut store, &first, &cancel).unwrap();
        let owner = resource_owner(&store, tile_resource(&first).payload.resource_id(), tile_resource(&first).bytes);
        checkpoint(&mut store, &capture(2), &cancel).unwrap(); let length = fs::metadata(&owner.path).unwrap().len();
        let mut operations = Vec::new(); recorded_checkpoint(&mut store, &first, &cancel, &mut operations).unwrap();
        assert_eq!(operations.len(), 2); assert_eq!(fs::metadata(&owner.path).unwrap().len(), length); assert_eq!(marker(&mut store), 1);
    }
    #[cfg(unix)]
    #[test]
    fn verified_camera_checkpoint_does_not_open_or_read_payload_packs() {
        use std::os::unix::fs::PermissionsExt;
        let directory = Directory::new(); let cancel = AtomicBool::new(false); let prepared = capture(1);
        let mut store = open_store(&directory.0).wait().unwrap(); checkpoint(&mut store, &prepared, &cancel).unwrap();
        let owner = resource_owner(&store, tile_resource(&prepared).payload.resource_id(), tile_resource(&prepared).bytes);
        let permissions = fs::metadata(&owner.path).unwrap().permissions();
        fs::set_permissions(&owner.path, fs::Permissions::from_mode(0o000)).unwrap();
        assert!(fs::File::open(&owner.path).is_err());
        let mut operations = Vec::new(); let result = recorded_checkpoint(&mut store, &prepared, &cancel, &mut operations);
        fs::set_permissions(&owner.path, permissions).unwrap();
        result.unwrap(); assert_eq!(operations.len(), 2); assert_eq!(marker(&mut store), 1);
    }
    #[test]
    fn shared_owner_crossing_a_partly_filled_pack_is_written_once() {
        fn input(resource:crate::authored::Resource<[u8]>)->StoreInput {
            StoreInput::entry(&super::super::resources::ResourceEntry {kind:"capy.test/1",data:serde_json::json!({}),raw_encoding:"capy.test/1",payload:Payload::Encoded(resource)},&AtomicBool::new(false)).unwrap()
        }
        let directory=Directory::new();let cancel=AtomicBool::new(false);let mut store=SessionStore::open(&directory.0).wait().unwrap();
        store.store_inputs(vec![input(vec![1].into())],&cancel).wait().unwrap().commit().unwrap();
        let pack=store.append.unwrap().0;let path=store.pack_path(pack);
        OpenOptions::new().write(true).open(&path).unwrap().set_len(PACK_BYTES-16).unwrap();
        let owner:crate::authored::Resource<[u8]>=vec![17;8].into();let alias=owner.alias(PortableId::random());
        let first=owner.id();let second=alias.id();let separate:crate::authored::Resource<[u8]>=vec![29;16].into();let separate_id=separate.id();
        store.store_inputs(vec![input(owner),input(separate),input(alias)],&cancel).wait().unwrap().commit().unwrap();
        let first=&store.index[&first].location;let second=&store.index[&second].location;
        assert!(Arc::ptr_eq(first,second));assert_eq!(first.range().pack.id,pack);
        assert_eq!(&*first.read().wait().unwrap(),&[17;8]);assert_eq!(&*second.read().wait().unwrap(),&[17;8]);
        assert_eq!(&*store.index[&separate_id].location.read().wait().unwrap(),&[29;16]);
        let bytes=fs::read_dir(directory.0.join("packs")).unwrap().map(|entry|entry.unwrap().metadata().unwrap().len()).sum::<u64>();
        assert_eq!(bytes,PACK_BYTES-16+24);assert_eq!(count(&directory.0.join("packs")),2);
    }

    #[test]
    fn rollover_keeps_previous_pack_and_live_ranges() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let original = capture(1); let second = capture(2);
        let mut store = SessionStore::open(&directory.0).wait().unwrap(); checkpoint(&mut store, &original, &cancel).unwrap();
        let owner = resource_owner(&store, tile_resource(&original).payload.resource_id(), tile_resource(&original).bytes);
        OpenOptions::new().write(true).open(&owner.path).unwrap().set_len(PACK_BYTES).unwrap();
        checkpoint(&mut store, &second, &cancel).unwrap();
        let next = resource_owner(&store, tile_resource(&second).payload.resource_id(), tile_resource(&second).bytes);
        assert_ne!(owner.path, next.path); assert_eq!(count(&directory.0.join("packs")), 2);
        let pack=owner.location.backing().unwrap();
        let RangeState::Ready(bytes)=pack.read_on_worker(0,owner.length as usize).map(RangeState::Ready).unwrap() else {panic!("worker read must complete")};
        assert_eq!(bytes.len(),owner.length as usize);
        let next_path = next.path.clone();
        drop(next); drop(second);
        store.retire().wait().unwrap(); assert!(owner.path.is_file()); assert!(!next_path.exists());
    }
    #[test]
    fn corrupt_pack_extents_ranges_and_aliases_are_rejected() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let mut store = SessionStore::open(&directory.0).wait().unwrap();
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
        let mut store = SessionStore::open(&source.join("drawing")).wait().unwrap();
        checkpoint(&mut store, &capture(1), &AtomicBool::new(false)).unwrap(); drop(store);
        let head = read_head(&source.join("drawing")).unwrap();
        let competing = Arc::new(Mutex::new(None));
        assert!(import_session_directories_with_owner(&root.0, "window.json", ".lock", |path| {
            Ok(OwnerLock::claim(path)?.map(|owner| Reclaim { owner: Some(owner), path: path.into(), competing: competing.clone() }))
        }).is_err());
        assert!(competing.lock().unwrap().is_some());
        assert!(OwnerLock::claim(&source.join(".lock")).unwrap().is_none());
        assert!(!root.0.join("packed/scene").exists());
        assert_eq!(fs::read(source.join("window.json")).unwrap(), b"original membership");
        assert_eq!(read_head(&source.join("drawing")).unwrap(), head);
    }
    #[test]
    fn native_namespace_import_preserves_live_and_downgrade_sessions() {
        let root = Directory::new(); create_directory(&root.0).unwrap();
        let old = root.0.join("scene"); create_directory(&old).unwrap();
        fs::write(old.join("window.json"), b"original membership").unwrap();
        let first = capture(1);
        let mut store = SessionStore::open(&old.join("drawing")).wait().unwrap(); checkpoint(&mut store, &first, &AtomicBool::new(false)).unwrap();
        let live = OwnerLock::claim(&old.join(".lock")).unwrap().unwrap();
        let mut callbacks = 0;
        import_session_directories_with_owner(&root.0, "window.json", ".lock", |path| {
            assert_eq!(path, old.join(".lock")); callbacks += 1; Ok(None::<OwnerLock>)
        }).unwrap();
        assert_eq!(callbacks, 1); assert!(OwnerLock::claim(&old.join(".lock")).unwrap().is_none());
        let packed = import_session_directories(&root.0, "window.json", ".lock").unwrap();
        assert!(old.is_dir()); assert!(!packed.join("scene").exists()); drop(live);
        import_session_directories(&root.0, "window.json", ".lock").unwrap(); assert!(old.is_dir());
        let reader = resource_owner(&store, tile_resource(&first).payload.resource_id(), tile_resource(&first).bytes); drop(store); drop(first);
        import_session_directories(&root.0, "window.json", ".lock").unwrap(); assert!(old.is_dir()); drop(reader);
        import_session_directories(&root.0, "window.json", ".lock").unwrap();
        assert_eq!(fs::read(packed.join("scene/window.json")).unwrap(), b"original membership");
        assert!(!old.exists());
        create_directory(&old).unwrap(); fs::write(old.join("window.json"), b"downgrade latest membership").unwrap();
        import_session_directories(&root.0, "window.json", ".lock").unwrap();
        assert_eq!(fs::read(packed.join("scene/window.json")).unwrap(), b"original membership");
        assert!(!old.exists());
        let renamed = fs::read_dir(&packed).unwrap().map(|entry| entry.unwrap().path()).find(|path| fs::read(path.join("window.json")).is_ok_and(|bytes| bytes == b"downgrade latest membership")).unwrap();
        fs::remove_dir_all(renamed).unwrap();
        let upgraded = packed.join("scene/drawing");
        let mut upgraded_store = SessionStore::open(&upgraded).wait().unwrap();
        let mut opened = upgraded_store.load(ProjectLimits::default(), &AtomicBool::new(false)).wait().unwrap().unwrap();
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
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &AtomicBool::new(false)).wait();
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
            let mut store = SessionStore::open(&directory.0).wait().unwrap(); checkpoint(&mut store, &prepared, &cancel).unwrap(); drop(store);
            let mut store = SessionStore::open(&directory.0).wait().unwrap();
            let restored = store.load(ProjectLimits::default(), &cancel).wait().unwrap().unwrap();
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
            let mut store = SessionStore::open(&directory.0).wait().unwrap(); checkpoint(&mut store, &prepared, &cancel).unwrap();
            let owner = resource_owner(&store, id, length); assert_eq!(owner.range().offset, 0); assert_eq!(fs::metadata(&owner.path).unwrap().len(), length.max(1));
            if length == 0 {
                let path = owner.path.clone();
                drop((owner, prepared, capture, editor, store));
                let missing = path.with_extension("held"); fs::rename(&path, &missing).unwrap();
                let mut store = SessionStore::open(&directory.0).wait().unwrap();
                let failed = store.load(ProjectLimits::default(), &cancel).wait().is_err();
                fs::rename(missing, &path).unwrap(); assert!(failed);
                let restored = store.load(ProjectLimits::default(), &cancel).wait().unwrap().unwrap();
                assert_eq!(restored.editor.document().artwork.extensions.resources[&id].length, 0);
                checkpoint(&mut store, &self::capture(2), &cancel).unwrap();
                continue;
            }
            let restored = store.load(ProjectLimits::default(), &cancel).wait().unwrap().unwrap();
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
        let head:Head=serde_json::from_slice(&read_head(&directory.0).unwrap()).unwrap();
        let metadata:serde_json::Value=serde_json::from_slice(&fs::read(directory.0.join("generations").join(format!("{}.json",head.current.id))).unwrap()).unwrap();
        assert!(metadata["layout"].get("vectors").is_none());
        let mut store=SessionStore::open(&directory.0).wait().unwrap();
        let mut restored=store.load(ProjectLimits::default(),&cancel).wait().unwrap().unwrap();
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
        let current:serde_json::Value=serde_json::from_slice(SavedSession::new(&prepared.editor,prepared.metadata.clone(),&AtomicBool::new(false)).unwrap().metadata()).unwrap();assert_eq!(current["layout"]["vectors"],json!([]));
        let generation=checkpoint(&mut store, &prepared,&cancel).unwrap();assert_ne!(generation,head.current.id);drop(store);
        let mut reopened=SessionStore::open(&directory.0).wait().unwrap();
        let mut restored=reopened.load(ProjectLimits::default(),&cancel).wait().unwrap().unwrap();
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
        let mut store=open_store(&root.0).wait().unwrap();
        let first=checkpoint(&mut store, &prepared,&cancel).unwrap();
        assert!(store.load(ProjectLimits::default(),&cancel).wait().unwrap().is_some());
        let authority=store.programs.keys().copied().collect::<Vec<_>>();
        assert!(interrupted_checkpoint(&mut store, &prepared,&cancel,Interrupted {operation:1,after:false}).is_err());
        assert_eq!(store.programs.keys().copied().collect::<Vec<_>>(),authority);
        let second=checkpoint(&mut store, &prepared,&cancel).unwrap();
        let first_bytes=fs::read(store.generation_path(first)).unwrap();
        let second_bytes=fs::read(store.generation_path(second)).unwrap();
        fs::write(store.generation_path(second),b"corrupt metadata").unwrap();
        assert!(store.load(ProjectLimits::default(),&cancel).wait().unwrap().is_some());
        assert_eq!(store.loaded_generation(),Some(first));
        fs::write(store.generation_path(second),&second_bytes).unwrap();
        drop(store);
        let mut reopened=open_store(&root.0).wait().unwrap();
        assert!(reopened.load(ProjectLimits::default(),&cancel).wait().err().unwrap().contains("not approved"));
        assert_eq!(fs::read(reopened.generation_path(first)).unwrap(),first_bytes);
        assert_eq!(fs::read(reopened.generation_path(second)).unwrap(),second_bytes);
    }
    #[test]
    fn unapproved_generation_never_silently_restores_an_older_builtin_checkpoint() {
        let root=Directory::new();let cancel=AtomicBool::new(false);
        let mut editor=Editor::new(Document::new(PortableId::random(),19,11,DocumentNames {paint:"ink".into(),paper:"paper".into()}));
        let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
        let builtin=captured(&capture, json!({"marker":1}));
        let mut store=SessionStore::open(&root.0).wait().unwrap();checkpoint(&mut store, &builtin,&cancel).unwrap();
        let handle=editor.document().artwork.effects.iter().next().unwrap().0;
        let program=crate::effect_catalog::custom_program("solid_color");let values=crate::EffectInstance::new(program.clone()).values;
        let application=crate::EffectApplication::new(program,values,editor.document().composition().size);
        editor.perform(crate::Edit::Effect(crate::RecordChange::replace(&editor.document().artwork.effects,handle,Some(application)).unwrap())).unwrap();
        let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
        let custom=captured(&capture, json!({"marker":2}));
        let generation=checkpoint(&mut store, &custom,&cancel).unwrap();
        let metadata=fs::read(store.generation_path(generation)).unwrap();
        let head=read_head(&root.0).unwrap();drop(store);
        let mut store=SessionStore::open(&root.0).wait().unwrap();
        assert!(store.load(ProjectLimits::default(),&cancel).wait().err().unwrap().contains("not approved"));
        assert_eq!(store.loaded_generation(),None);assert_eq!(store.generation(),Some(generation));
        assert_eq!(fs::read(store.generation_path(generation)).unwrap(),metadata);
        assert_eq!(read_head(&root.0).unwrap(),head);
    }
    fn count(path: &Path) -> usize { match fs::read_dir(path) { Ok(entries) => entries.count(), Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0, Err(error) => panic!("{error}") } }
    struct ReaderLease {location:Arc<StoreLocation>,path:PathBuf}
    impl std::ops::Deref for ReaderLease {type Target=StoreLocation;fn deref(&self)->&Self::Target {&self.location}}
    fn resource_owner(store:&SessionStore,id:PortableId,length:u64)->ReaderLease {
        let location=store.index[&id].location.clone();assert_eq!(location.length,length);
        ReaderLease {path:store.pack_path(location.range().pack.id),location}
    }
    struct TileResource<'a> {payload:&'a Arc<TileBlob>,bytes:u64}
    fn tile_resource(capture:&Captured)->TileResource<'_> {
        let payload=capture.tiles.first().unwrap();TileResource {payload,bytes:payload.compressed_len() as u64}
    }
    fn marker(store: &mut SessionStore) -> u8 {
        store.load(ProjectLimits::default(), &AtomicBool::new(false)).wait().unwrap().unwrap().metadata.value["marker"].as_u64().unwrap() as u8
    }

    struct Interrupted { operation: usize, after: bool }

    #[test]
    fn failed_first_head_write_retries_without_copying_durable_resources() {
        for length in [0,32] {
            let directory=Directory::new();let cancel=AtomicBool::new(false);let capture=capture(17);
            let mut store=open_store(&directory.0).wait().unwrap();
            assert_eq!(fault_checkpoint(&mut store,&capture,&cancel,Fault::HeadWrite(length)).unwrap_err(),"Injected head write failed");
            assert!(store.generation().is_none());assert!(!directory.0.join("head.1").exists());
            let packs=fs::read_dir(directory.0.join("packs")).unwrap().map(|entry| {let entry=entry.unwrap();(entry.path(),entry.metadata().unwrap().len())}).collect::<BTreeMap<_,_>>();
            assert_eq!(packs.len(),1);
            checkpoint(&mut store,&capture,&cancel).unwrap();
            let after=fs::read_dir(directory.0.join("packs")).unwrap().map(|entry| {let entry=entry.unwrap();(entry.path(),entry.metadata().unwrap().len())}).collect::<BTreeMap<_,_>>();
            assert_eq!(after,packs);assert_eq!(marker(&mut store),17);
            assert_eq!(count(&directory.0.join("generations")),1);
        }
    }

    #[test]
    fn lost_head_acknowledgement_preserves_the_last_acknowledged_previous_generation() {
        let directory=Directory::new();let cancel=AtomicBool::new(false);
        let mut store=open_store(&directory.0).wait().unwrap();let first=capture(1);
        let acknowledged=checkpoint(&mut store,&first,&cancel).unwrap();
        assert!(interrupted_checkpoint(&mut store,&capture(2),&cancel,Interrupted {operation:3,after:true}).is_err());
        let unacknowledged=store.generation().unwrap();assert_ne!(unacknowledged,acknowledged);
        assert_eq!(store.loaded_generation(),Some(acknowledged));
        let visible=head::read(&*store._owner.io).wait().unwrap().unwrap();assert_eq!(visible.head.current.id,unacknowledged);
        let committed=checkpoint(&mut store,&capture(3),&cancel).unwrap();
        assert_eq!(store.head.as_ref().unwrap().previous.as_ref().unwrap().id,acknowledged);
        assert_eq!(store.loaded_generation(),Some(committed));assert_eq!(marker(&mut store),3);
        assert!(store.generation_path(acknowledged).is_file());assert!(!store.generation_path(unacknowledged).exists());
    }

    #[test]
    fn interrupted_existing_head_overwrite_keeps_both_published_slot_files() {
        for length in [0,32] {
            let directory=Directory::new();let cancel=AtomicBool::new(false);
            let mut store=open_store(&directory.0).wait().unwrap();checkpoint(&mut store,&capture(1),&cancel).unwrap();
            let acknowledged=checkpoint(&mut store,&capture(2),&cancel).unwrap();
            let old=fs::read(directory.0.join("head.1")).unwrap();let current=fs::read(directory.0.join("head.0")).unwrap();
            assert!(fault_checkpoint(&mut store,&capture(3),&cancel,Fault::HeadWrite(length)).is_err());
            assert!(directory.0.join("head.1").is_file());assert_eq!(fs::read(directory.0.join("head.0")).unwrap(),current);
            if length==0 {assert_eq!(fs::read(directory.0.join("head.1")).unwrap(),old);}
            assert_eq!(store.generation(),Some(acknowledged));assert_eq!(marker(&mut store),2);
            checkpoint(&mut store,&capture(3),&cancel).unwrap();assert_eq!(marker(&mut store),3);
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
                let mut store = open_store(&directory.0).wait().unwrap();
                checkpoint(&mut store, &old, &AtomicBool::new(false)).unwrap();
                let failure = interrupted_checkpoint(&mut store, &new, &AtomicBool::new(false), Interrupted { operation, after });
                assert!(failure.is_err());
                drop(store);
                let mut restored = open_store(&directory.0).wait().unwrap();
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
            let store = open_store(&path).wait().unwrap();
            let mut store=store;
            let stage=match std::env::var("CAPY_SESSION_CRASH_STAGE").unwrap().as_str() {"resources"=>"packs/","generations"=>"generations/",_=>"head."};
            control(&store).lock().unwrap().fault=Some(Fault::Exit(stage));
            checkpoint(&mut store,&capture(8),&AtomicBool::new(false)).unwrap();
            panic!("Crash worker returned");
        }
        for stage in ["resources", "generations", "head"] {
            let directory = Directory::new();
            let mut store = open_store(&directory.0).wait().unwrap();
            let original = capture(7);
            checkpoint(&mut store, &original, &AtomicBool::new(false)).unwrap();
            drop(store); drop(original);
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "package::session_store::tests::process_death_mid_write_keeps_the_original_and_cleans_temporary_files", "--test-threads=1"])
                .env("CAPY_SESSION_CRASH_DIRECTORY", &directory.0)
                .env("CAPY_SESSION_CRASH_STAGE", stage)
                .status().unwrap();
            assert_eq!(status.code(), Some(87));
            let mut store = open_store(&directory.0).wait().unwrap();
            assert_eq!(marker(&mut store), 7);
            assert_eq!(count(&directory.0.join("packs")), 1);
            assert_eq!(count(&directory.0.join("generations")), 1);
            assert_eq!(count(&directory.0), if stage=="head" {6}else {5});
        }
    }

    #[test]
    fn publication_error_after_head_does_not_collect_the_new_checkpoint() {
        let directory = Directory::new();
        let mut store = open_store(&directory.0).wait().unwrap();
        let old = capture(1);
        let previous = checkpoint(&mut store, &old, &AtomicBool::new(false)).unwrap();
        let new = capture(2);
        assert!(interrupted_checkpoint(&mut store, &new, &AtomicBool::new(false), Interrupted { operation: 3, after: true }).is_err());
        assert_ne!(store.generation(), Some(previous)); assert_eq!(store.loaded_generation(),Some(previous));
        assert!(store.programs.contains_key(&store.generation().unwrap()));
        store.collect_garbage().wait().unwrap();
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
        let mut store = SessionStore::open(&directory.0).wait().unwrap();
        let original = capture(1);
        checkpoint(&mut store, &original, &AtomicBool::new(false)).unwrap();
        let resource = tile_resource(&original);
        let live = resource_owner(&store, resource.payload.resource_id(), resource.bytes);
        for value in 2..20 {
            checkpoint(&mut store, &capture(value), &AtomicBool::new(false)).unwrap();
            assert!(count(&directory.0.join("generations")) <= 2);
            assert!(count(&directory.0.join("packs")) <= 3);
        }
        let pack=live.location.backing().unwrap();
        let RangeState::Ready(bytes)=pack.read_on_worker(0,live.length as usize).map(RangeState::Ready).unwrap() else {panic!("worker read must complete")};
        assert_eq!(bytes.len(),live.length as usize);
        store.retire().wait().unwrap();
        assert_eq!(count(&directory.0.join("packs")), 1);
        drop(store);
        assert!(OwnerLock::claim(&directory.0.join(".lock")).unwrap().is_none());
        assert!(SessionStore::open(&directory.0).wait().is_err());
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &AtomicBool::new(false)).wait();
        assert_eq!(count(&directory.0.join("packs")), 1);
        drop(bytes); drop(pack); drop(live); drop(original);
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &AtomicBool::new(false)).wait();
        assert_eq!(count(&directory.0.join("packs")), 0);
        assert_eq!(count(&directory.0.join("generations")), 0);
        assert!(directory.0.join(".lock").exists());
    }

    #[test]
    fn corrupt_current_falls_back_and_unsupported_current_stays_preserved() {
        for corruption in 0..4 {
            let directory = Directory::new();
            let mut store = SessionStore::open(&directory.0).wait().unwrap();
            let previous = checkpoint(&mut store, &capture(1), &AtomicBool::new(false)).unwrap();
            let second = capture(2);
            let generation = checkpoint(&mut store, &second, &AtomicBool::new(false)).unwrap();
            match corruption {
                0 => {
                    let owner = resource_owner(&store, tile_resource(&second).payload.resource_id(), tile_resource(&second).bytes);
                    let mut file = OpenOptions::new().write(true).open(&owner.path).unwrap();
                    use std::io::{Seek, SeekFrom};
                    file.seek(SeekFrom::Start(owner.range().offset)).unwrap(); file.write_all(b"broken").unwrap();
                }
                1 => fs::write(store.generation_path(generation), b"broken").unwrap(),
                3 => {
                    let path = store.generation_path(generation);
                    let metadata = fs::read(&path).unwrap();
                    fs::write(path,&metadata[..metadata.len()/2]).unwrap();
                }
                _ => {
                    let mut metadata: serde_json::Value = serde_json::from_slice(&fs::read(store.generation_path(generation)).unwrap()).unwrap();
                    let resource = metadata["session"]["resources"].as_array_mut().unwrap().iter_mut().find(|record| record["id"] == json!(tile_resource(&second).payload.resource_id())).unwrap();
                    resource["encoding"] = "unsupported".into();
                    let metadata = serde_json::to_vec(&metadata).unwrap();
                    fs::write(store.generation_path(generation), &metadata).unwrap();
                    let mut head = store.head.clone().unwrap();head.current=Generation::new(generation,&metadata);
                    write_head(&store.path,&serde_json::to_vec(&head).unwrap()).unwrap();
                }
            }
            let metadata = fs::read(store.generation_path(generation)).unwrap();
            let head = read_head(&store.path).unwrap();
            drop(store);
            let mut store = SessionStore::open(&directory.0).wait().unwrap();
            if corruption==0 {
                let opened = store.load(ProjectLimits::default(), &AtomicBool::new(false)).wait().unwrap().unwrap();
                assert_eq!(store.loaded_generation(), Some(generation));
                assert_eq!(store.generation(), Some(generation));
                let blobs = opened.editor.retained_tiles().blobs().unwrap();
                assert_eq!(blobs.len(), 1);
                assert_eq!(blobs[0].decode_on_worker().unwrap_err(), "Recovery resource checksum failed");
                assert_eq!(fs::read(store.generation_path(generation)).unwrap(), metadata);
                assert_eq!(read_head(&store.path).unwrap(), head);
                continue;
            }
            if corruption==2 {
                assert_eq!(store.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().err().unwrap(),"Unsupported session artwork");
                assert_eq!(store.loaded_generation(),None);assert_eq!(store.generation(),Some(generation));
                assert_eq!(fs::read(store.generation_path(generation)).unwrap(),metadata);
                assert_eq!(read_head(&store.path).unwrap(),head);
                continue;
            }
            assert_eq!(marker(&mut store), 1);
            assert_eq!(store.loaded_generation(),Some(previous));
            assert_eq!(fs::read(store.generation_path(generation)).unwrap(),metadata);
            assert_eq!(read_head(&store.path).unwrap(),head);
            checkpoint(&mut store, &capture(3), &AtomicBool::new(false)).unwrap();
            assert_eq!(marker(&mut store), 3);
            assert_eq!(count(&directory.0.join("generations")), 2);
            assert_eq!(count(&directory.0.join("packs")), 1);
        }
    }

    #[test]
    fn future_session_schema_is_terminal_before_reading_its_resource_inventory() {
        let directory=Directory::new();let cancel=AtomicBool::new(false);
        let mut store=SessionStore::open(&directory.0).wait().unwrap();checkpoint(&mut store, &capture(1),&cancel).unwrap();
        let generation=checkpoint(&mut store, &capture(2),&cancel).unwrap();
        let metadata=serde_json::to_vec(&json!({"format":"capy.session","version":2,"future_field":[]})).unwrap();
        fs::write(store.generation_path(generation),&metadata).unwrap();
        let mut head=store.head.clone().unwrap();head.current=Generation::new(generation,&metadata);
        let head=serde_json::to_vec(&head).unwrap();write_head(&store.path,&head).unwrap();drop(store);
        let mut store=SessionStore::open(&directory.0).wait().unwrap();
        assert_eq!(store.load(ProjectLimits::default(),&cancel).wait().err().unwrap(),"Unsupported session format");
        assert_eq!(store.loaded_generation(),None);assert_eq!(store.generation(),Some(generation));
        assert_eq!(fs::read(store.generation_path(generation)).unwrap(),metadata);
        assert_eq!(read_head(&store.path).unwrap(),head);
    }
    #[test]
    fn verified_reuse_does_not_reread_and_corruption_fails_on_first_read() {
        let directory = Directory::new();
        let capture = capture(9);
        let mut store = SessionStore::open(&directory.0).wait().unwrap();
        checkpoint(&mut store, &capture, &AtomicBool::new(false)).unwrap();
        let path = resource_owner(&store, tile_resource(&capture).payload.resource_id(), tile_resource(&capture).bytes).path.clone();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        checkpoint(&mut store, &capture, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        drop(store);
        let mut store = SessionStore::open(&directory.0).wait().unwrap();
        let mut bytes = fs::read(&path).unwrap(); bytes[0] ^= 1; fs::write(&path, &bytes).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        checkpoint(&mut store, &capture, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let head = read_head(&directory.0).unwrap();
        let opened = store.load(ProjectLimits::default(), &AtomicBool::new(false)).wait().unwrap().unwrap();
        let blobs = opened.editor.retained_tiles().blobs().unwrap();
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0].decode_on_worker().unwrap_err(), "Recovery resource checksum failed");
        assert_eq!(read_head(&directory.0).unwrap(), head);
    }

    #[test]
    fn ownership_and_cancelled_retirement_preserve_the_only_copy() {
        let directory = Directory::new();
        let mut store = SessionStore::open(&directory.0).wait().unwrap();
        assert!(SessionStore::open(&directory.0).wait().is_err());
        let saved = capture(5);
        checkpoint(&mut store, &saved, &AtomicBool::new(false)).unwrap();
        assert!(checkpoint(&mut store, &capture(6), &AtomicBool::new(true)).is_err());
        assert_eq!(marker(&mut store), 5);
        let file = directory.0.join("generations/.capy-save-interrupted");
        fs::write(&file, b"partial").unwrap();
        store.collect_garbage().wait().unwrap();
        assert!(!file.exists());
        drop(store);
        let store = SessionStore::open(&directory.0).wait().unwrap();
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
        let base: serde_json::Value = serde_json::from_slice(SavedSession::new(&prepared.editor,prepared.metadata.clone(),&AtomicBool::new(false)).unwrap().metadata()).unwrap();
        for corruption in 0..5 {
            let mut value = base.clone();
            match corruption {
                0 => {let mut next=value["resources"][0].clone();next["id"]=PortableId::random().to_string().into();next["location"]["offset"]="1".into();value["resources"].as_array_mut().unwrap().push(next);},
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
        let mut kept = SessionStore::open(&root.0.join("kept")).wait().unwrap();
        checkpoint(&mut kept, &capture(1), &cancel).unwrap();
        let mut live = SessionStore::open(&root.0.join("writer")).wait().unwrap();
        checkpoint(&mut live, &capture(2), &cancel).unwrap();
        let mut orphan = SessionStore::open(&root.0.join("orphan")).wait().unwrap();
        let prepared = capture(3);
        checkpoint(&mut orphan, &prepared, &cancel).unwrap();
        let tile = tile_resource(&prepared);
        let reader = resource_owner(&orphan, tile.payload.resource_id(), tile.bytes);
        orphan.retire().wait().unwrap();
        let mut unpublished = SessionStore::open(&root.0.join("unpublished")).wait().unwrap();
        checkpoint(&mut unpublished, &capture(4), &cancel).unwrap();
        drop(unpublished);
        let unrecognized = root.0.join("unrecognized");
        fs::create_dir(&unrecognized).unwrap(); fs::write(unrecognized.join("keep"), b"user bytes").unwrap();
        fs::write(orphan.path.join("head.json"), b"invalid").unwrap();
        drop(orphan); drop(kept);
        let reachable = ["kept".into()].into();
        assert_eq!(collect_unreferenced_stores(&root.0, &reachable, &AtomicBool::new(true)).wait(), 0);
        collect_unreferenced_stores(&root.0, &reachable, &cancel).wait();
        assert!(head_path(&root.0.join("kept")).is_file());
        assert!(head_path(&root.0.join("writer")).is_file());
        assert!(!root.0.join("orphan/head.json").exists());
        assert_eq!(count(&root.0.join("orphan/packs")), 1);
        assert!(reader.path.is_file());
        assert_eq!(fs::read(unrecognized.join("keep")).unwrap(), b"user bytes");
        assert!(head_path(&root.0.join("unpublished")).is_file());
        drop(reader); drop(live); drop(prepared);
        collect_unreferenced_stores(&root.0, &reachable, &cancel).wait();
        assert_eq!(count(&root.0.join("orphan/packs")), 0);
        assert!(head_path(&root.0.join("writer")).is_file());
        assert!(root.0.join("orphan/.lock").is_file());
    }

    #[test]
    fn durable_retirement_prevents_reopening_before_head_is_removed() {
        let root = Directory::new();
        let path = root.0.join("closed");
        let mut store = SessionStore::open(&path).wait().unwrap();
        checkpoint(&mut store, &capture(1), &AtomicBool::new(false)).unwrap();
        atomic_replace(&path.join(".retired"), b"").unwrap();
        assert!(head_path(&path).is_file());
        assert!(checkpoint(&mut store, &capture(2), &AtomicBool::new(false)).is_err());
        assert_eq!(count(&path.join("generations")), 1);
        drop(store);
        assert!(SessionStore::open(&path).wait().is_err());
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &AtomicBool::new(false)).wait();
        assert!(!path.join("head.json").exists());
        assert_eq!(count(&path.join("resources")), 0);
        assert_eq!(count(&path.join("generations")), 0);
        assert!(SessionStore::open(&path).wait().is_err());
        assert!(path.join(".retired").exists());
        assert_eq!(count(&path.join("generations")), 0);
    }

    #[test]
    fn close_intent_is_harmless_while_indexed_and_retires_after_membership_removal() {
        let root = Directory::new(); let path = root.0.join("closing");
        let cancel = AtomicBool::new(false);
        let mut store = SessionStore::open(&path).wait().unwrap();
        checkpoint(&mut store, &capture(1), &cancel).unwrap();
        store.prepare_retirement().wait().unwrap();
        drop(store);
        collect_unreferenced_stores(&root.0, &["closing".into()].into(), &cancel).wait();
        let mut store = SessionStore::open(&path).wait().unwrap();
        assert_eq!(marker(&mut store), 1);
        checkpoint(&mut store, &capture(2), &cancel).unwrap();
        assert_eq!(marker(&mut store), 2);
        assert!(!path.join(".retired").exists());
        drop(store);
        collect_unreferenced_stores(&root.0, &BTreeSet::new(), &cancel).wait();
        assert_eq!(count(&path.join("resources")), 0);
        assert_eq!(count(&path.join("generations")), 0);
        assert!(SessionStore::open(&path).wait().is_err());
    }



    #[test]
    fn torn_or_foreign_retirement_markers_still_retire_an_unreferenced_store() {
        let root = Directory::new(); let cancel = AtomicBool::new(false);
        for (marker, bytes) in [(".retired", &b"malformed"[..]), (".retiring", &b""[..])] {
            let path = root.0.join(marker.trim_start_matches('.'));
            let mut store = SessionStore::open(&path).wait().unwrap();
            checkpoint(&mut store, &capture(2), &cancel).unwrap();
            drop(store);
            atomic_replace(&path.join(marker), bytes).unwrap();
            assert_eq!(collect_unreferenced_stores(&root.0, &BTreeSet::new(), &cancel).wait(), 1);
            assert!(!head_path(&path).exists() && !path.join("generations").exists());
            assert!(SessionStore::open(&path).wait().is_err());
            assert_eq!(collect_unreferenced_stores(&root.0, &BTreeSet::new(), &cancel).wait(), 0);
            fs::remove_dir_all(path).unwrap();
        }
    }

    #[test]
    fn valid_same_length_metadata_corruption_falls_back_before_decode_or_collection() {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let mut store = SessionStore::open(&directory.0).wait().unwrap();
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
        assert!(store.collect_garbage().wait().is_err());
        assert_eq!(count(&directory.0.join("generations")), 2);
        drop(store);
        let mut store = SessionStore::open(&directory.0).wait().unwrap();
        assert_eq!(marker(&mut store), 1);
        assert_eq!(store.loaded_generation(), Some(previous));
        assert!(store.recovered_previous());
        checkpoint(&mut store, &capture(4), &cancel).unwrap();
        assert_eq!(store.head.as_ref().unwrap().previous.as_ref().unwrap().id, previous);
        assert_eq!(marker(&mut store), 4);
        assert_eq!(count(&directory.0.join("generations")), 2);
        let mut head: serde_json::Value = serde_json::from_slice(&read_head(&directory.0).unwrap()).unwrap();
        head["current"]["sha256"] = "invalid".into();
        write_head(&directory.0,&serde_json::to_vec(&head).unwrap()).unwrap();
        assert!(store.load(ProjectLimits::default(), &cancel).wait().is_err());
        assert!(store.collect_garbage().wait().is_err());
        drop(store);
        assert!(SessionStore::open(&directory.0).wait().is_err());
        assert_eq!(count(&directory.0.join("generations")), 2);
    }



    #[cfg(unix)]
    #[test]
    fn nested_directory_symlinks_never_write_or_remove_the_target() {
        use std::os::unix::fs::symlink;
        let root = Directory::new(); let outside = Directory::new(); create_directory(&outside.0).unwrap();
        let cancel = AtomicBool::new(false);
        for name in ["resources", "generations"] {
            let path = root.0.join(name); let mut store = SessionStore::open(&path).wait().unwrap();
            checkpoint(&mut store, &capture(1), &cancel).unwrap();
            store.prepare_retirement().wait().unwrap();
            let target = outside.0.join(format!("{}.{}", PortableId::random(), if name == "resources" { "bin" } else { "json" }));
            fs::write(&target, b"outside user bytes").unwrap();
            let retained = path.join(format!("original-{name}"));
            fs::rename(path.join(name), &retained).unwrap();
            symlink(&outside.0, path.join(name)).unwrap();
            assert!(checkpoint(&mut store, &capture(2), &cancel).is_err());
            assert!(store.load(ProjectLimits::default(), &cancel).wait().is_err());
            assert!(store.collect_garbage().wait().is_err());
            assert!(store.retire().wait().is_err());
            assert!(NativeTransport::new(path.clone()).list(name).wait().is_err());
            drop(store);
            assert!(SessionStore::open(&path).wait().is_err());
            assert_eq!(collect_unreferenced_stores(&root.0, &BTreeSet::new(), &cancel).wait(), 0);
            assert_eq!(fs::read(&target).unwrap(), b"outside user bytes");
            assert!(head_path(&path).is_file());
            fs::remove_file(path.join(name)).unwrap();
            fs::rename(retained, path.join(name)).unwrap();
            let mut store = SessionStore::open(&path).wait().unwrap();
            assert_eq!(marker(&mut store), 1);
            store.retire().wait().unwrap();
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
        let mut store = SessionStore::open(&path).wait().unwrap();
        let prepared = capture(7);
        let generation = checkpoint(&mut store, &prepared, &AtomicBool::new(false)).unwrap();
        let permissions = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
        let failed = store.collect_garbage().wait();
        fs::set_permissions(&path, permissions).unwrap();
        assert!(failed.is_err());
        assert_eq!(store.generation(), Some(generation));
        drop(store);
        let current_head=head_path(&path);
        fs::rename(&current_head,path.join("saved-head")).unwrap();
        symlink(current_head.file_name().unwrap(),&current_head).unwrap();
        assert!(SessionStore::open(&path).wait().is_err());
        assert_eq!(count(&path.join("packs")),1);assert_eq!(count(&path.join("generations")),1);
        fs::remove_file(&current_head).unwrap();fs::rename(path.join("saved-head"),&current_head).unwrap();
        let mut store = SessionStore::open(&path).wait().unwrap();
        assert_eq!(marker(&mut store), 7);
    }

    #[test]
    fn late_package_decode_failure_refuses_reuse_and_checkpoint_without_rotating_the_head() {
        let directory=Directory::new();let mut store=SessionStore::open(&directory.0).wait().unwrap();let cancel=AtomicBool::new(false);
        let mut document=Document::new(PortableId::random(),256,256,DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let descriptor=RasterPlane::Color.descriptor(Default::default());let encoded:Arc<[u8]>=Arc::from([1,2,3]);
        let backing=ImmutableBacking::new(Arc::new(encoded.clone())).unwrap();let lease=super::super::lease::PackageLease::new(backing);
        let range=lease.range(0,encoded.len() as u64,crc32fast::hash(&encoded),descriptor,false).unwrap();
        let blob=Arc::new(TileBlob::from_package_range(PortableId::random(),descriptor,range).unwrap());
        *document.target_raster_mut(document.working.target.unwrap()).unwrap()=RasterRevision::backed(RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0,0]},RasterTile::backed_shared(blob.clone()))].into(),..Default::default()});
        let editor=Editor::new(document);let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
        let first=store.checkpoint(&capture,serde_json::Value::Null.into(),&cancel).wait().unwrap();let sequence=store.sequence;
        let input=StoreInput::entry(&super::super::resources::ResourceEntry {kind:"capy.raster-tile/1",data:super::super::resources::encode_descriptor(descriptor).unwrap(),raw_encoding:"capy.lz4-tile/1",payload:Payload::Tile(blob.clone())},&cancel).unwrap();
        assert!(blob.decode_on_worker().is_err());
        assert!(store.store_inputs(vec![input],&cancel).wait().is_err());
        assert!(store.checkpoint(&capture,serde_json::json!({"camera":"later"}).into(),&cancel).wait().is_err());
        assert_eq!(store.generation(),Some(first));assert_eq!(store.sequence,sequence);
        assert!(super::super::codec::PreparedPackage::prepare(capture.artwork(),None,&cancel).is_err());
        assert!(blob.read_compressed().wait().is_err());
    }
