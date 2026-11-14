use super::tests::{Wait,Paths,open_store,control,Control,Fault,read_head};
use super::*;
use crate::{Document, DocumentNames, Editor, Edit, raster::*, raster_storage::RetainedTiles};
use std::{fs::{self,OpenOptions},io::Write,sync::mpsc};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("capy-live-store-{}", PortableId::random()));
        fs::create_dir_all(&path).unwrap(); Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}
fn blob(marker: u8) -> Arc<TileBlob> {
    Arc::new(TileBlob::encode(RasterPlane::Color.descriptor(Default::default()), &vec![marker; 256 * 256 * 4]).unwrap())
}
fn revision(blob: Arc<TileBlob>) -> RasterRevision {
    RasterRevision::backed(RasterData { tiles: [(TileKey {plane: RasterPlane::Color, coordinate: [0, 0]},
        RasterTile::backed_shared(blob))].into(), ..Default::default() })
}
fn document_editor(blob: Arc<TileBlob>) -> Editor {
    let mut document = Document::new(PortableId::random(), 256, 256,
        DocumentNames {paint: "Ink".into(), paper: "Paper".into()});
    *document.target_raster_mut(document.working.target.unwrap()).unwrap() = revision(blob);
    Editor::new(document)
}
fn captured_editor(editor: &Editor) -> super::super::session::EditorCapture {
    editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap()
}
fn tiles(blob: Arc<TileBlob>) -> RetainedTiles { RetainedTiles {sources: vec![blob], ..Default::default()} }
fn pack_lengths(store: &SessionStore) -> BTreeMap<PathBuf, u64> {
    fs::read_dir(store.path.join("packs")).unwrap().map(|entry| {
        let entry = entry.unwrap(); (entry.path(), entry.metadata().unwrap().len())
    }).collect()
}

#[test]
fn transport_reopen_preserves_unheaded_history_and_capture_pack_leases() {
    use super::super::store_io::NativeTransport;
    let directory=Directory::new();let cancel=AtomicBool::new(false);
    let lease=Arc::new(OwnerLock::claim(&directory.0.join(".lock")).unwrap().unwrap());
    let transport=Arc::new(NativeTransport::new(directory.0.clone()));
    let reads=crate::tile_io::ReadCache::new(crate::MemoryBudget::default());
    let first=blob(37);let expected=first.decode_on_worker().unwrap();
    let mut editor=document_editor(first.clone());let target=editor.document().working.target.unwrap();
    editor.perform(Edit::SetRaster {target,revision:revision(blob(71))}).unwrap();
    let capture=captured_editor(&editor);let retained=editor.retained_tiles();
    let mut store=SessionStore::open_transport(directory.0.clone(),transport.clone(),lease.clone(),reads.clone()).wait().unwrap();
    assert!(store.make_durable(&retained,&cancel).wait().unwrap()>0);
    let packs=pack_lengths(&store);assert!(!packs.is_empty());assert!(store.generation().is_none());
    drop(store);
    let mut store=SessionStore::open_transport(directory.0.clone(),transport,lease,reads).wait().unwrap();
    assert_eq!(pack_lengths(&store),packs);assert_eq!(first.decode_on_worker().unwrap(),expected);
    drop((first,retained,editor));store.collect_garbage().wait().unwrap();assert_eq!(pack_lengths(&store),packs);
    drop(capture);store.collect_garbage().wait().unwrap();assert!(pack_lengths(&store).is_empty());
}

#[test]
fn transport_startup_collection_preserves_writers_locations_and_unclaimed_heads() {
    use super::super::store_io::NativeTransport;
    let directory=Directory::new();let cancel=AtomicBool::new(false);
    let io=Arc::new(NativeTransport::new(directory.0.clone()));let lease=Arc::new(());
    let tile=blob(29);let encoded=tile.compressed_on_worker().unwrap().into_arc();let retained=tiles(tile.clone());
    let mut store=SessionStore::open_transport(directory.0.clone(),io.clone(),lease.clone(),crate::tile_io::ReadCache::new(crate::MemoryBudget::default())).wait().unwrap();
    store.make_durable(&retained,&cancel).wait().unwrap();let packs=pack_lengths(&store);
    let location=tile.compressed.location().unwrap().unwrap();drop((tile,retained));
    assert!(!directory.0.join(".lock").exists());
    assert!(!SessionStore::collect_unreferenced_transport(directory.0.clone(),io.clone(),lease.clone()).wait());
    assert_eq!(pack_lengths(&store),packs);assert!(!directory.0.join(".retired").exists());drop(store);
    assert!(SessionStore::collect_unreferenced_transport(directory.0.clone(),io.clone(),lease.clone()).wait());
    for path in packs.keys() {assert!(path.is_file());}
    assert_eq!(&*location.read().wait().unwrap(),&*encoded);drop(location);
    assert!(SessionStore::collect_unreferenced_transport(directory.0.clone(),io.clone(),lease.clone()).wait());
    for path in packs.keys() {assert!(!path.exists());}
    assert_eq!(fs::read(directory.0.join(".retired")).unwrap(),b"");
    assert!(!SessionStore::collect_unreferenced_transport(directory.0.clone(),io,lease).wait());
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(),1);

    let headed=Directory::new();let io=Arc::new(NativeTransport::new(headed.0.clone()));let lease=Arc::new(());
    let mut store=SessionStore::open_transport(headed.0.clone(),io.clone(),lease.clone(),crate::tile_io::ReadCache::new(crate::MemoryBudget::default())).wait().unwrap();
    store.checkpoint(&captured_editor(&document_editor(blob(83))),serde_json::json!({}).into(),&cancel).wait().unwrap();
    let head=read_head(&headed.0).unwrap();let packs=pack_lengths(&store);drop(store);
    assert!(!SessionStore::collect_unreferenced_transport(headed.0.clone(),io,lease).wait());
    assert_eq!(read_head(&headed.0).unwrap(),head);assert!(!headed.0.join(".retired").exists());
    for (path,length) in packs {assert_eq!(fs::metadata(path).unwrap().len(),length);}
    assert!(!headed.0.join(".lock").exists());
}

#[test]
fn unheaded_pressure_tiles_follow_history_and_capture_lifetimes() {
    let directory = Directory::new(); let cancel = AtomicBool::new(false);
    let first = blob(31); let expected = first.decode_on_worker().unwrap();
    let mut editor = document_editor(first.clone());
    let target = editor.document().working.target.unwrap();
    editor.perform(Edit::SetRaster {target, revision: revision(blob(79))}).unwrap();
    let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
    let retained = editor.retained_tiles();
    let mut store = SessionStore::open(&directory.0).wait().unwrap();
    assert!(store.make_durable(&retained, &cancel).wait().unwrap() > 0);
    let pressure_packs = pack_lengths(&store);
    assert!(!pressure_packs.is_empty());
    for path in pressure_packs.keys() { OpenOptions::new().write(true).open(path).unwrap().set_len(PACK_BYTES).unwrap(); }
    store.checkpoint(&captured_editor(&document_editor(blob(141))), serde_json::json!({}).into(), &cancel).wait().unwrap();
    store.collect_garbage().wait().unwrap();
    for path in pressure_packs.keys() { assert!(path.exists()); }
    assert_eq!(first.decode_on_worker().unwrap(), expected);
    drop((first, retained, editor));
    store.collect_garbage().wait().unwrap();
    for path in pressure_packs.keys() { assert!(path.exists(), "capture retains history locations"); }
    drop(capture);
    store.collect_garbage().wait().unwrap();
    for path in pressure_packs.keys() { assert!(!path.exists(), "unreferenced pressure pack is collectible"); }
}

#[test]
fn reopening_collects_unheaded_pressure_packs_without_live_roots() {
    let directory = Directory::new(); let cancel = AtomicBool::new(false);
    let mut store = SessionStore::open(&directory.0).wait().unwrap();
    let retained = tiles(blob(43));
    store.make_durable(&retained, &cancel).wait().unwrap();
    let packs = pack_lengths(&store); assert!(!packs.is_empty());
    drop((retained, store));
    let store = SessionStore::open(&directory.0).wait().unwrap();
    assert!(store.generation().is_none());
    assert!(pack_lengths(&store).is_empty());
}

#[test]
fn pressure_then_checkpoint_keeps_one_physical_tile_copy() {
    let directory = Directory::new(); let cancel = AtomicBool::new(false);
    let tile = blob(57); let editor = document_editor(tile.clone());
    let retained = editor.retained_tiles();
    let mut store = SessionStore::open(&directory.0).wait().unwrap();
    assert_eq!(store.make_durable(&retained, &cancel).wait().unwrap(), tile.compressed_len());
    let before = pack_lengths(&store);
    store.checkpoint(&captured_editor(&editor), serde_json::json!({}).into(), &cancel).wait().unwrap();
    assert_eq!(pack_lengths(&store), before);
    assert_eq!(store.make_durable(&retained, &cancel).wait().unwrap(), 0);
}

#[test]
fn pressure_alias_only_checkpoint_reuses_the_shared_durable_location() {
    let directory = Directory::new(); let cancel = AtomicBool::new(false);
    let original = blob(73);
    let alias = Arc::new(original.alias(PortableId::random()));
    assert_ne!(original.resource_id(), alias.resource_id());
    let retained = RetainedTiles {sources: vec![original.clone(), alias.clone()], ..Default::default()};
    let mut store = SessionStore::open(&directory.0).wait().unwrap();
    assert_eq!(store.make_durable(&retained, &cancel).wait().unwrap(), original.compressed_len());
    let before = pack_lengths(&store);
    let editor = document_editor(alias.clone());
    let checkpoint = captured_editor(&editor);
    store.checkpoint(&checkpoint, serde_json::json!({}).into(), &cancel).wait().unwrap();
    assert_eq!(pack_lengths(&store), before);
    let bytes=store.generation_metadata(&store.head.as_ref().unwrap().current,ProjectLimits::default().metadata_bytes).wait().unwrap();
    let stored=StoredGeneration::parse(&bytes,ProjectLimits::default()).unwrap_or_else(|error|panic!("{}",error.message()));let ids=stored.locations.unwrap();
    assert!(ids.contains_key(&alias.resource_id()));assert!(!ids.contains_key(&original.resource_id()));
    drop((checkpoint, editor, retained, alias, original, store));
    let mut store = SessionStore::open(&directory.0).wait().unwrap();
    let opened = store.load(ProjectLimits::default(), &cancel).wait().unwrap().unwrap();
    let blobs = opened.editor.retained_tiles().blobs().unwrap();
    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs[0].decode_on_worker().unwrap(), vec![73; 256 * 256 * 4]);
    assert_eq!(pack_lengths(&store), before);
}

#[test]
fn failed_pressure_write_or_sync_keeps_original_ram_and_last_good_head() {
    for sync in [false, true] {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let seed = document_editor(blob(63)); let retained = seed.retained_tiles();
        let mut store = open_store(&directory.0).wait().unwrap();
        store.make_durable(&retained, &cancel).wait().unwrap();
        store.checkpoint(&captured_editor(&seed), serde_json::json!({}).into(), &cancel).wait().unwrap();
        let head = read_head(&directory.0).unwrap();
        let tile = blob(91); let bytes = tile.compressed_on_worker().unwrap().into_arc(); let expected = tile.decode_on_worker().unwrap();
        assert!({let state=control(&store);state.lock().unwrap().fault=Some(if sync {Fault::Sync("Injected data sync failure")}else {Fault::Partial(b"partial pressure write")});let result=store.make_durable(&tiles(tile.clone()), &cancel).wait();*state.lock().unwrap()=Control::default();result}.is_err());
        assert_eq!(tile.resident_bytes(), bytes.len());
        assert!(Arc::ptr_eq(&tile.compressed_on_worker().unwrap().into_arc(), &bytes));
        assert_eq!(tile.decode_on_worker().unwrap(), expected);
        assert_eq!(read_head(&directory.0).unwrap(), head);
        assert!(store.load(ProjectLimits::default(), &cancel).wait().unwrap().is_some());
    }
}

#[test]
fn blocked_pressure_sync_does_not_hold_tile_storage_lock() {
    let directory = Directory::new(); let cancel = AtomicBool::new(false);
    let seed = tiles(blob(117));
    let mut store = open_store(&directory.0).wait().unwrap();
    store.make_durable(&seed, &cancel).wait().unwrap();
    let tile = blob(131); let bytes = tile.compressed_on_worker().unwrap().into_arc(); let expected = tile.decode_on_worker().unwrap();
    let retained = tiles(tile.clone());
    let (entered, writing) = mpsc::channel(); let (resume, proceed) = mpsc::channel();
    let state=control(&store);state.lock().unwrap().fault=Some(Fault::Block {entered,resume:proceed});
    let writer=std::thread::spawn(move || store.make_durable(&retained,&cancel).wait());
    writing.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    let (done, query) = mpsc::channel();
    let queried = tile.clone();
    let reader = std::thread::spawn(move || done.send((queried.resident_bytes(), queried.decode_on_worker())).unwrap());
    let result = query.recv_timeout(std::time::Duration::from_secs(5));
    resume.send(()).unwrap();
    let released = writer.join().unwrap().unwrap(); reader.join().unwrap();
    let (resident, pixels) = result.expect("RAM queries must complete while durable write is blocked");
    assert_eq!(resident, bytes.len()); assert_eq!(pixels.unwrap(), expected);
    assert_eq!(released, bytes.len());
    assert_eq!(tile.decode_on_worker().unwrap(), expected);
}

#[test]
fn thousands_of_durable_tiles_share_one_file_and_read_source_per_pack() {
    let directory = Directory::new(); let cancel = AtomicBool::new(false);
    let descriptor = RasterPlane::Color.descriptor(Default::default());
    let mut pixels = vec![255; 256 * 256 * 4];
    let mut blobs = Vec::with_capacity(4096);
    for index in 0..4096u32 {
        pixels[..3].copy_from_slice(&index.to_le_bytes()[..3]);
        blobs.push(Arc::new(TileBlob::encode(descriptor, &pixels).unwrap()));
    }
    let alias = Arc::new(blobs[0].alias(PortableId::random()));
    let retained = RetainedTiles {sources: blobs.clone(), ..Default::default()};
    let mut store = SessionStore::open(&directory.0).wait().unwrap();
    let expected = blobs.iter().map(|blob| blob.compressed_len()).sum::<usize>();
    assert_eq!(store.make_durable(&retained, &cancel).wait().unwrap(), expected);
    let physical = pack_lengths(&store);
    assert!(!physical.is_empty());
    let packs=store._owner.packs.lock().unwrap().values().filter_map(Weak::upgrade).collect::<Vec<_>>();
    assert_eq!(packs.len(),physical.len());
    let file_owners=packs.iter().map(|pack|Arc::as_ptr(&pack.file)).collect::<std::collections::HashSet<_>>();
    assert_eq!(file_owners.len(),physical.len());
    for blob in &blobs {
        let location=blob.compressed.location().unwrap().unwrap();
        assert!(packs.iter().any(|pack|Arc::ptr_eq(pack,&location.range().pack)));
    }
    let original = blobs[0].compressed.location().unwrap().unwrap();
    let aliased = alias.compressed.location().unwrap().unwrap();
    assert!(Arc::ptr_eq(&original, &aliased));
    assert!(Arc::ptr_eq(&original.range().pack, &aliased.range().pack));
    assert_eq!(original.range().offset, aliased.range().offset);
    assert_eq!(retained.resident_bytes(), 0);
}

#[test]
fn a_cold_durable_tile_polls_pending_and_decodes_on_its_worker() {
    let directory=Directory::new();let cancel=AtomicBool::new(false);
    let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {override_bytes:Some(1<<30),..Default::default()});
    let cache=crate::tile_io::ReadCache::new(budget);cache.set_scheduler(Arc::new(|_|{}));
    let tile=blob(73);let expected=tile.decode_on_worker().unwrap();
    let mut store=SessionStore::open_with_cache(&directory.0,cache.clone()).wait().unwrap();
    store.make_durable(&tiles(tile.clone()),&cancel).wait().unwrap();
    assert!(matches!(tile.poll_compressed().unwrap(),RangeState::Pending));
    std::thread::spawn(move ||cache.drive()).join().unwrap();
    let RangeState::Ready(bytes)=tile.poll_compressed().unwrap() else {panic!("completed worker range")};
    assert_eq!(bytes.len(),tile.compressed_len());
    assert_eq!(tile.decode_on_worker().unwrap(),expected);
}

#[test]
fn blocking_store_reads_complete_without_import_headroom() {
    let directory=Directory::new();let cancel=AtomicBool::new(false);
    let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {override_bytes:Some(1<<30),..Default::default()});
    let cache=crate::tile_io::ReadCache::new(budget.clone());
    let tile=blob(83);let expected=tile.compressed_on_worker().unwrap().into_arc();
    let mut store=SessionStore::open_with_cache(&directory.0,cache).wait().unwrap();
    store.make_durable(&tiles(tile.clone()),&cancel).wait().unwrap();
    budget.flush_reports();
    let mut pressure=Some(budget.admit_import(budget.import_headroom_bytes()).unwrap());
    assert_eq!(budget.import_headroom_bytes(),0);
    assert!(matches!(tile.poll_compressed().unwrap(),RangeState::Pending));
    let (send,receive)=mpsc::channel();let reading=tile.clone();
    let worker=std::thread::spawn(move ||send.send(reading.compressed_on_worker()).unwrap());
    let result=receive.recv_timeout(std::time::Duration::from_secs(5));
    if result.is_err() {drop(pressure.take());}
    worker.join().unwrap();
    let bytes=result.expect("worker read waited for import headroom").unwrap();
    assert_eq!(&*bytes,&*expected);
    assert!(matches!(tile.poll_compressed().unwrap(),RangeState::Ready(_)));
    budget.flush_reports();
    assert_eq!(budget.import_headroom_bytes(),0);
    assert!(budget.snapshot().ram_pinned_bytes>=bytes.len() as u64);
    assert!(pressure.as_ref().unwrap().active());
}

struct CountedReads {
    cache:crate::tile_io::ReadCache,
    calls:Arc<std::sync::atomic::AtomicUsize>,
}
impl CountedReads {
    fn new()->Self {
        let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {override_bytes:Some(1<<30),..Default::default()});
        let cache=crate::tile_io::ReadCache::new(budget);
        Self {cache,calls:Default::default()}
    }
    fn calls(&self)->usize {self.calls.load(Ordering::Acquire)}
    fn open(&self,path:&Path)->SessionStore {
        let calls=self.calls.clone();
        crate::package::store_io::test_support::open_observed(path,self.cache.clone(),Arc::new(move |_,_|{calls.fetch_add(1,Ordering::Relaxed);})).wait().unwrap()
    }
}
#[test]
fn clean_checkpoints_reuse_the_index_without_reading_cold_tiles_or_recomputing_their_crc() {
    let directory=Directory::new();let reads=CountedReads::new();let cancel=AtomicBool::new(false);
    let tile=blob(97);let editor=document_editor(tile.clone());
    let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
    let mut store=reads.open(&directory.0);
    store.checkpoint(&capture,serde_json::json!({"camera":0}).into(),&cancel).wait().unwrap();
    let physical=pack_lengths(&store);let location=tile.compressed.location().unwrap().unwrap();
    assert_eq!(tile.resident_bytes(),0);assert_eq!(reads.calls(),0);
    for camera in 1..4 {
        store.checkpoint(&capture,serde_json::json!({"camera":camera}).into(),&cancel).wait().unwrap();
        assert_eq!(pack_lengths(&store),physical);
        assert!(Arc::ptr_eq(&tile.compressed.location().unwrap().unwrap(),&location));
        assert_eq!(reads.calls(),0,"a cold tile CRC requires a payload read");
    }
    assert_eq!(tile.decode_on_worker().unwrap(),vec![97;256*256*4]);
    assert!(reads.calls()>0,"the counting reader must observe actual tile use");
}

#[test]
fn checkpoint_reuse_preserves_captured_effect_phases_and_ignores_runtime_elapsed() {
    use crate::{EffectInstance, authored::{EffectApplication, EvaluationContext, Occurrence, OccurrenceContent}};
    let directory=Directory::new();let cancel=AtomicBool::new(false);
    let mut document=Document::new(PortableId::random(),256,256,DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let program=crate::bundled_effect_catalog().get("motion_blur").unwrap().program();
    let values=EffectInstance::new(program.clone()).values;
    let effect=document.artwork.effects.insert(PortableId::random(),EffectApplication::new(program,values,[256;2])).unwrap();
    let occurrence=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(effect),"Frozen motion")).unwrap();
    let stack=document.composition().result;document.artwork.stacks.get_mut(stack).unwrap().entries.insert(0,occurrence);
    let editor=Editor::new(Document::from_artwork(document.artwork).unwrap());
    let base=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
    let mut store=SessionStore::open(&directory.0).wait().unwrap();
    let mut previous=None;
    for (index,(elapsed,phase)) in [(1.25,2.5),(3.75,2.5),(3.75,7.5),(3.75,7.5)].into_iter().enumerate() {
        let context=EvaluationContext {elapsed,phases:vec![(effect,phase)].into()};
        let mut capture=base.clone();
        let artwork=Arc::make_mut(&mut capture.artwork_mut().artwork);
        artwork.outputs.get_mut(artwork.default_output).unwrap().context=context.clone();
        let key=CheckpointKey::of(&capture);
        assert_eq!(key.checkpoint,CheckpointKey::of(&base).checkpoint);
        assert_eq!(key.context,Some(EvaluationContext {elapsed:0.,phases:context.phases.clone()}));
        if let Some(previous)=&previous {assert_eq!(key==*previous,index!=2);}
        previous=Some(key);
        store.checkpoint(&capture,serde_json::json!({"camera":index}).into(),&cancel).wait().unwrap();
        let restored=store.load(ProjectLimits::default(),&cancel).wait().unwrap().unwrap();
        assert_eq!(restored.editor.document().output().context,EvaluationContext {elapsed:0.,phases:context.phases});
        assert_eq!(restored.metadata.value["camera"],serde_json::json!(index));
    }
}

#[test]
fn checkpoint_reuse_invalidates_changed_metadata_profiles_and_rejects_missing_output() {
    use crate::color::{ColorProfile, RgbSpace};
    let directory=Directory::new();let cancel=AtomicBool::new(false);
    let editor=document_editor(blob(41));
    let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
    let mut store=SessionStore::open(&directory.0).wait().unwrap();
    for (index,space) in [RgbSpace::Srgb,RgbSpace::DisplayP3,RgbSpace::DisplayP3].into_iter().enumerate() {
        let profiles=vec![ColorProfile::Builtin(space)];
        store.checkpoint(&capture,super::super::session::SessionMetadata {value:serde_json::json!({"camera":index}),profiles:profiles.clone()},&cancel).wait().unwrap();
        let restored=store.load(ProjectLimits::default(),&cancel).wait().unwrap().unwrap();
        assert_eq!(restored.metadata.profiles,profiles);
        assert_eq!(restored.metadata.value["camera"],serde_json::json!(index));
    }
    let head=read_head(&directory.0).unwrap();
    let mut invalid=capture.clone();
    Arc::make_mut(&mut invalid.artwork_mut().artwork).default_output=crate::authored::OutputHandle::from_index(u32::MAX);
    assert!(CheckpointKey::of(&invalid).context.is_none());
    assert!(store.checkpoint(&invalid,serde_json::json!({}).into(),&cancel).wait().is_err());
    assert_eq!(read_head(&directory.0).unwrap(),head);
}
#[test]
fn stored_tile_identity_checks_worker_payload_without_reading_the_cold_owner() {
    let directory=Directory::new();let reads=CountedReads::new();let cancel=AtomicBool::new(false);
    let source=blob(83);let bytes=source.memory_compressed().unwrap().unwrap();
    let editor=document_editor(source);let capture=captured_editor(&editor);
    let mut store=reads.open(&directory.0);
    store.checkpoint(&capture,serde_json::json!({}).into(),&cancel).wait().unwrap();
    let cold=store.load(ProjectLimits::default(),&cancel).wait().unwrap().unwrap().editor.retained_tiles().blobs().unwrap().pop().unwrap();
    let identity=cold.content_identity();assert!(identity.is_some());assert!(cold.encoded_fingerprint().is_none());
    let worker=TileBlob::from_verified_resource_with_content_identity(cold.resource_id(),cold.descriptor,bytes.clone(),identity).unwrap();
    assert!(cold.same_content(&worker));assert!(cold.same_content(&worker.alias(PortableId::random())));
    let mut different=bytes.to_vec();different[0]^=1;
    assert!(TileBlob::from_verified_resource_with_content_identity(cold.resource_id(),cold.descriptor,different.into(),identity).is_err());
    assert!(TileBlob::from_verified_resource_with_content_identity(cold.resource_id(),crate::color::PixelDescriptor::COVERAGE8,bytes.clone(),identity).is_err());
    let unknown=TileBlob::from_verified_resource(cold.resource_id(),cold.descriptor,bytes).unwrap();assert!(!cold.same_content(&unknown));
    assert_eq!(cold.resident_bytes(),0);assert_eq!(reads.calls(),0);
}

#[test]
fn restore_binds_cold_tiles_and_defers_checksum_verification_until_the_first_read() {
    let directory=Directory::new();let reads=CountedReads::new();let cancel=AtomicBool::new(false);
    let editor=document_editor(blob(113));
    let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
    let mut store=reads.open(&directory.0);
    store.checkpoint(&capture,serde_json::json!({}).into(),&cancel).wait().unwrap();
    let tile=store.load(ProjectLimits::default(),&cancel).wait().unwrap().unwrap().editor.retained_tiles().blobs().unwrap().pop().unwrap();
    assert_eq!(tile.resident_bytes(),0);assert_eq!(reads.calls(),0);
    let location=tile.compressed.location().unwrap().unwrap();
    let mut file=OpenOptions::new().write(true).open(store.pack_path(location.range().pack.id)).unwrap();
    use std::io::{Seek,SeekFrom};let byte=location.range().pack.file.read_at(location.range().offset,1).wait().unwrap();
    file.seek(SeekFrom::Start(location.range().offset)).unwrap();file.write_all(&[byte[0]^0xff]).unwrap();file.sync_all().unwrap();
    assert!(tile.decode_on_worker().unwrap_err().contains("checksum"));
    assert!(reads.calls()>0);
    assert_eq!(store.load(ProjectLimits::default(),&cancel).wait().unwrap().unwrap().editor.document().composition().size,[256;2]);
}
