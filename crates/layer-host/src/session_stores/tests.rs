use super::*;
use std::{collections::VecDeque, sync::atomic::{AtomicUsize, Ordering}};

fn heads(path: &std::path::Path) -> [Option<Vec<u8>>; 2] {
    ["head.0", "head.1"].map(|name| match std::fs::read(path.join(name)) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("{error}"),
    })
}

struct Fixture {
    stores: SessionStores,
    activity: CompactionActivity,
    queue: Arc<Mutex<VecDeque<StoreWork>>>,
    wakes: Arc<AtomicUsize>,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("capy-store-coordinator-{}", PortableId::random()));
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let queued = queue.clone();
        let wakes = Arc::new(AtomicUsize::new(0)); let wake = wakes.clone();
        let activity = CompactionActivity::new();
        let stores = SessionStores::with_opener(root.clone(), Arc::new(layer_core::package::session_store::NativeStoreOpener),
            Arc::new(move |work| queued.lock().unwrap().push_back(work)),
            Arc::new(move || { wake.fetch_add(1, Ordering::SeqCst); }), native_now_ms, activity.clone());
        Self {stores, activity, queue, wakes, root}
    }
    fn pop(&self) -> StoreWork { self.queue.lock().unwrap().pop_front().unwrap() }
    fn run(&self) { self.pop().run(); }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let pending = std::mem::take(&mut *self.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner)); drop(pending);
        { let mut state = self.stores.0.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner); state.drawings.clear(); state.aliases.clear(); }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
fn fixture_cleanup_survives_poisoned_locks() {
    let fixture=Fixture::new();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _queue=fixture.queue.lock().unwrap();
        let _state=fixture.stores.0.state.lock().unwrap();
        panic!("fixture lock failure");
    })).is_err());
    drop(fixture);
}
pub(super) fn capture() -> (SessionCapture, SessionStamp, RetainedTiles, Arc<layer_core::raster::TileBlob>) {
    let mut document = layer_core::Document::new(PortableId::random(), 256, 256,
        layer_core::DocumentNames {paint: "Ink".into(), paper: "Paper".into()});
    use layer_core::raster::{RasterPlane, RasterTile, RasterRevision, RasterData, TileKey, TileBlob};
    let tile = Arc::new(TileBlob::encode(RasterPlane::Color.descriptor(Default::default()),
        &[31, 63, 127, 255].repeat(256 * 256)).unwrap());
    *document.target_raster_mut(document.working.target.unwrap()).unwrap() = RasterRevision::backed(RasterData {
        tiles: [(TileKey {plane: RasterPlane::Color, coordinate: [0, 0]}, RasterTile::backed_shared(tile.clone()))].into(),
        ..Default::default()
    });
    let retained = layer_core::Editor::new(document.clone()).retained_tiles();
    let session = layer_ui::UiSession::new(crate::Renderer::default(), document, [256; 2], layer_ui::Platform::Gtk).unwrap();
    (session.capture_session().unwrap(), session.session_stamp(), retained, tile)
}

#[test]
fn cold_store_tiles_survive_color_job_worker_roundtrips() {
    use layer_core::{Document,DocumentNames,Editor,SourceTarget,authored::{Image,ImageObject,PaintBase},color::{ColorProfile,RgbSpace,SampleDepth,source::{SourceChannels,SourceImage,SourceInterpretation}},package::{session_store::SessionStore,transfer::PreparedTransfer},raster::{TileBlob,TILE_SIZE}};
    let fixture=Fixture::new();let cancel=AtomicBool::new(false);
    let mut original=Document::new(PortableId::random(),19,11,DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let interpretation=SourceInterpretation {channels:SourceChannels::Rgba,depth:SampleDepth::U8,profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false};
    let expected=[17,29,81,255].repeat((TILE_SIZE*TILE_SIZE) as usize);
    let tile=Arc::new(TileBlob::encode(interpretation.descriptor(),&expected).unwrap());
    let image=Image::new(Arc::new(SourceImage {extent:[19,11],resolution:None,tiles:[([0,0],tile)].into(),interpretation}));
    let SourceTarget::Paint(paint)=original.working.target.unwrap() else {panic!("paint")};
    original.artwork.paint.get_mut(paint).unwrap().base=Some(PaintBase::new(image.clone()));
    for index in 0..2 {let (_,edit)=original.create_object_layer_edit("Image",ImageObject::new(image.clone()),None,index).unwrap();original.apply(edit).unwrap();}
    let editor=Editor::new(original);let captured=editor.capture_session(editor.capture(0,Default::default()).unwrap()).unwrap();
    let mut store=pollster::block_on(SessionStore::open(&fixture.root.join("cold"))).unwrap();
    pollster::block_on(store.checkpoint(&captured,serde_json::json!({}).into(),&cancel)).unwrap();
    let original=pollster::block_on(store.load(Default::default(),&cancel)).unwrap().unwrap().editor.document().clone();
    let image=original.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.clone();
    let cold=image.tiles[&[0,0]].clone();assert_eq!(cold.resident_bytes(),0);assert!(cold.encoded_fingerprint().is_none());
    let roundtrip=|artwork| {
        let capture=Editor::new(Document::from_artwork(artwork).unwrap()).capture(0,Default::default()).unwrap();
        let transfer=PreparedTransfer::capture(&capture,&cancel).unwrap();
        let descriptor=serde_json::from_slice(&serde_json::to_vec(transfer.descriptor()).unwrap()).unwrap();
        let chunks=(0..transfer.payload_count()).map(|index|vec![Arc::from(transfer.read_chunk(index,0,transfer.payload_len(index).unwrap() as usize).unwrap())]).collect();
        pollster::block_on(PreparedTransfer::from_chunks(descriptor,chunks)).unwrap().adopt_verified(Default::default(),&cancel).unwrap().artwork.as_ref().clone()
    };
    let worker=Document::from_artwork(roundtrip(layer_color::color_job_artwork(&original.artwork))).unwrap();
    let converted=layer_color::prepare_document_color(&worker,layer_color::DocumentColorChange::Depth {depth:SampleDepth::U8,dither:Default::default()},16*1024*1024,||false).unwrap();
    let adopted=layer_color::adopt_color_job_artwork(&original,roundtrip(converted.document.artwork)).unwrap();
    for (_,_,object) in adopted.artwork.objects.iter() {assert!(object.image.same_owner(&image));assert!(Arc::ptr_eq(&object.image.tiles[&[0,0]],&cold));}
    let mut conflicting=original.artwork.clone();let mut source=(*image).clone();
    let encoded=TileBlob::encode(cold.descriptor,&[19,29,81,255].repeat((TILE_SIZE*TILE_SIZE) as usize)).unwrap().memory_compressed().unwrap().unwrap();
    source.tiles.insert([0,0],Arc::new(TileBlob::from_verified_resource(cold.resource_id(),cold.descriptor,encoded).unwrap()));
    let handle=conflicting.objects.iter().next().unwrap().0;conflicting.objects.get_mut(handle).unwrap().image=Image::with_id(image.id(),Arc::new(source));
    assert!(layer_color::adopt_color_job_artwork(&original,conflicting).is_err());
    assert_eq!(cold.resident_bytes(),0);assert_eq!(cold.decode_on_worker().unwrap(),expected);
}
fn named(capture: &SessionCapture, stamp: &SessionStamp, name: &str) -> (SessionCapture, SessionStamp) {
    let mut capture = capture.clone(); capture.state.unsaved_name = Some(name.into());
    let mut stamp = stamp.clone(); stamp.state = capture.state.clone();
    (capture, stamp)
}

struct GatedOpener {
    inner: layer_core::package::session_store::NativeStoreOpener,
    entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    resume: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}
impl StoreOpener for GatedOpener {
    fn storage_facts<'a>(&'a self, path: &'a std::path::Path) -> layer_core::package::store_io::FileFuture<'a, layer_core::package::storage_budget::StorageFacts> {
        self.inner.storage_facts(path)
    }
    fn open<'a>(&'a self, path: &'a std::path::Path, retirement: bool) -> layer_core::package::store_io::FileFuture<'a, Option<layer_core::package::session_store::SessionStore>> {
        if let Some(entered) = self.entered.lock().unwrap().take() { entered.send(()).unwrap(); }
        if let Some(resume) = self.resume.lock().unwrap().take() { resume.recv_timeout(std::time::Duration::from_secs(60)).unwrap(); }
        self.inner.open(path, retirement)
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn process_finish_waits_for_an_accepted_session_store_write_to_land() {
    let root = std::env::temp_dir().join(format!("capy-store-finish-{}", PortableId::random()));
    let (entered, started) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let opener = Arc::new(GatedOpener {inner: layer_core::package::session_store::NativeStoreOpener, entered: Mutex::new(Some(entered)), resume: Mutex::new(Some(resume))});
    let stores = SessionStores::with_opener(root.clone(), opener, Arc::new(native_worker), Arc::new(|| {}), native_now_ms, CompactionActivity::default());
    let binding = stores.bind(1, None).unwrap();
    let (saved, stamp, _, _) = capture();
    let reply = stores.checkpoint(1, saved, stamp, true).unwrap();
    started.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
    let (finished, done) = std::sync::mpsc::channel();
    let finishing = std::thread::spawn(move || {
        crate::finish_native_process();
        finished.send(()).unwrap();
    });
    let early = done.recv_timeout(std::time::Duration::from_millis(500)).is_ok();
    release.send(()).unwrap();
    done.recv_timeout(std::time::Duration::from_secs(60)).ok();
    finishing.join().unwrap();
    assert!(!early, "process finish returned before an accepted session-store write");
    assert!(matches!(reply.try_take(), Some(Ok(_))));
    assert!(heads(binding.path()).iter().any(Option::is_some));
    drop((binding, stores));
    let _ = std::fs::remove_dir_all(&root);
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn panicking_session_store_jobs_leave_the_queue_served_and_drainable() {
    let root = std::env::temp_dir().join(format!("capy-store-panics-{}", PortableId::random()));
    let stores = |wake: Arc<dyn Fn() + Send + Sync>| SessionStores::with_opener(root.clone(), Arc::new(layer_core::package::session_store::NativeStoreOpener), Arc::new(native_worker), wake, native_now_ms, CompactionActivity::default());
    let failing = [stores(Arc::new(|| panic!("session-store wake failed"))), stores(Arc::new(|| panic!("session-store wake failed")))];
    let healthy = stores(Arc::new(|| {}));
    for (index, store) in failing.iter().enumerate() {
        let (saved, stamp, _, _) = capture();
        store.bind(index as u64 + 1, None).unwrap();
        store.checkpoint(index as u64 + 1, saved, stamp, true).unwrap();
    }
    let (saved, stamp, _, _) = capture();
    let binding = healthy.bind(3, None).unwrap();
    let reply = healthy.checkpoint(3, saved, stamp, true).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let result = loop {
        if let Some(result) = reply.try_take() { break Some(result); }
        if std::time::Instant::now() > deadline { break None; }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert!(matches!(result, Some(Ok(_))), "a job queued after two panicking jobs never ran");
    assert!(heads(binding.path()).iter().any(Option::is_some));
    let (drained, done) = std::sync::mpsc::channel();
    std::thread::spawn(move || { drain_native_work(); drained.send(()).unwrap(); });
    assert!(done.recv_timeout(std::time::Duration::from_secs(20)).is_ok(), "draining never returned after panicking jobs");
    drop((binding, failing, healthy));
    let _ = std::fs::remove_dir_all(&root);
}
#[test]
fn failed_adoption_keeps_the_existing_binding() {
    let fixture = Fixture::new();
    let original = fixture.stores.bind(1, None).unwrap();
    assert!(fixture.stores.adopt(1, "../invalid").is_err());
    assert_eq!(fixture.stores.bind(1, None).unwrap().key(), original.key());
    assert_eq!(fixture.stores.bindings().len(), 1);
    let adopted = fixture.stores.adopt(1, "restored").unwrap();
    assert_eq!(adopted.key(), "restored");
    assert_eq!(fixture.stores.bindings()[0].1.key(), "restored");
}

#[test]
fn failed_restore_copy_retires_through_its_writer_after_membership_is_removed() {
    for registered in [false,true] {
        let fixture=Fixture::new();let (saved,stamp,_,_)=capture();
        let good=fixture.stores.bind(1,Some("good")).unwrap();
        let reply=fixture.stores.checkpoint(1,saved,stamp,true).unwrap();fixture.run();assert!(reply.try_take().unwrap().is_ok());
        let good_head=heads(good.path());
        let (saved,stamp,_,_)=capture();let bad=fixture.stores.bind(2,Some("failed")).unwrap();
        let reply=fixture.stores.checkpoint(2,saved,stamp,true).unwrap();fixture.run();assert!(reply.try_take().unwrap().is_ok());
        for generation in std::fs::read_dir(bad.path().join("generations")).unwrap() {std::fs::write(generation.unwrap().path(),b"broken checkpoint").unwrap();}
        let restore=fixture.stores.restore(2,ProjectLimits::default()).unwrap();fixture.run();assert!(restore.try_take().unwrap().is_err());
        if registered {
            assert!(pollster::block_on(layer_core::package::session_store::SessionStore::try_open(bad.path())).unwrap().is_none());
        }else {let mut state=fixture.stores.0.state.lock().unwrap();state.aliases.remove(&2);state.drawings.remove(bad.key());}
        let (key,path)=(bad.key().to_owned(),bad.path().to_path_buf());drop(bad);
        let prepare=fixture.stores.prepare_retirement(&key).unwrap();fixture.run();
        assert!(matches!(prepare.try_take(),Some(Ok(StoreResult::RetirementPrepared))));
        assert!(heads(&path).iter().any(Option::is_some));
        let manifest=layer_ui::SessionManifest::default().stage(vec![layer_ui::session_recovery::SessionDrawing {id:1,key:good.key().into()},layer_ui::session_recovery::SessionDrawing {id:2,key:key.clone()}],1).unwrap();
        manifest.remove(2).unwrap().publish(&fixture.root.join("session.json")).unwrap();
        let retire=fixture.stores.retire_copy(&key).unwrap();fixture.run();
        assert!(matches!(retire.try_take(),Some(Ok(StoreResult::Retired))));
        assert!(!heads(&path).iter().any(Option::is_some));
        assert_eq!(heads(good.path()),good_head);
        let restored=fixture.stores.restore(1,ProjectLimits::default()).unwrap();fixture.run();assert!(restored.try_take().unwrap().is_ok());
    }
}

#[test]
fn explicit_discard_prepares_a_corrupt_head_without_decoding_or_changing_another_copy() {
    use layer_core::package::session_store::{SessionStore, collect_unreferenced_stores};
    let fixture=Fixture::new();let cancel=AtomicBool::new(false);
    let (saved,stamp,_,_)=capture();let good=fixture.stores.bind(2,Some("good")).unwrap();
    let reply=fixture.stores.checkpoint(2,saved,stamp,true).unwrap();fixture.run();assert!(reply.try_take().unwrap().is_ok());
    let good_head=heads(good.path());
    let (saved,stamp,_,_)=capture();let bad=fixture.stores.bind(1,Some("corrupt")).unwrap();
    let reply=fixture.stores.checkpoint(1,saved,stamp,true).unwrap();fixture.run();assert!(reply.try_take().unwrap().is_ok());
    assert!(pollster::block_on(SessionStore::try_open(bad.path())).unwrap().is_none());
    let path=bad.path().to_path_buf();drop(bad);
    {let mut state=fixture.stores.0.state.lock().unwrap();state.aliases.remove(&1);state.drawings.remove("corrupt");}
    for name in ["head.0", "head.1"] {std::fs::write(path.join(name),b"invalid head").unwrap();}
    assert!(pollster::block_on(SessionStore::open(&path)).is_err());
    let reply=fixture.stores.prepare_retirement("corrupt").unwrap();fixture.run();
    assert!(matches!(reply.try_take(),Some(Ok(StoreResult::RetirementPrepared))));
    pollster::block_on(collect_unreferenced_stores(&fixture.root,&["corrupt".into()].into(),&cancel));
    assert_eq!(heads(&path),[Some(b"invalid head".to_vec()),Some(b"invalid head".to_vec())]);
    assert_eq!(std::fs::read_dir(path.join("generations")).unwrap().count(),1);
    let manifest=layer_ui::SessionManifest::default().stage(vec![layer_ui::session_recovery::SessionDrawing {id:1,key:"corrupt".into()}],1).unwrap();
    manifest.remove(1).unwrap().publish(&fixture.root.join("session.json")).unwrap();
    let reply=fixture.stores.retire_copy("corrupt").unwrap();fixture.run();
    assert!(matches!(reply.try_take(),Some(Ok(StoreResult::Retired))));
    assert!(!heads(&path).iter().any(Option::is_some));assert!(path.join(".retired").is_file());
    for directory in ["resources","generations","packs"] {assert!(!path.join(directory).exists());}
    assert!(pollster::block_on(SessionStore::open(&path)).is_err());
    assert_eq!(heads(good.path()),good_head);
}

#[test]
fn retirement_preserves_missing_unrecognized_and_locked_paths() {
    use layer_core::package::session_store::SessionStore;
    let fixture=Fixture::new();
    let prepared=fixture.stores.prepare_retirement("missing").unwrap();fixture.run();
    assert!(matches!(prepared.try_take(),Some(Ok(StoreResult::RetirementPrepared))));
    let retired=fixture.stores.retire_copy("missing").unwrap();fixture.run();
    assert!(matches!(retired.try_take(),Some(Ok(StoreResult::Retired))));
    assert!(!fixture.root.join("missing").exists());
    let unknown=fixture.root.join("unrecognized");std::fs::create_dir_all(&unknown).unwrap();
    std::fs::write(unknown.join("keep"),b"unrecognized bytes").unwrap();
    let prepared=fixture.stores.prepare_retirement("unrecognized").unwrap();fixture.run();
    assert!(matches!(prepared.try_take(),Some(Ok(StoreResult::RetirementPrepared))));
    let retired=fixture.stores.retire_copy("unrecognized").unwrap();fixture.run();
    assert!(matches!(retired.try_take(),Some(Ok(StoreResult::Retired))));
    assert_eq!(std::fs::read_dir(&unknown).unwrap().count(),1);
    assert_eq!(std::fs::read(unknown.join("keep")).unwrap(),b"unrecognized bytes");
    let locked=fixture.root.join("locked");let owner=pollster::block_on(SessionStore::open(&locked)).unwrap();
    let prepared=fixture.stores.prepare_retirement("locked").unwrap();fixture.run();
    assert!(matches!(prepared.try_take(),Some(Err(error)) if error=="Recovery drawing is already open"));
    let retired=fixture.stores.retire_copy("locked").unwrap();fixture.run();
    assert!(matches!(retired.try_take(),Some(Err(error)) if error=="Recovery drawing is already open"));
    assert!(!locked.join(".retiring").exists());assert!(!locked.join(".retired").exists());
    drop(owner);
    let prepared=fixture.stores.prepare_retirement("locked").unwrap();fixture.run();
    assert!(matches!(prepared.try_take(),Some(Ok(StoreResult::RetirementPrepared))));
    let retired=fixture.stores.retire_copy("locked").unwrap();fixture.run();
    assert!(matches!(retired.try_take(),Some(Ok(StoreResult::Retired))));
}

#[cfg(unix)]
#[test]
fn retirement_never_follows_root_or_nested_storage_symlinks() {
    use layer_core::package::session_store::SessionStore;
    use std::os::unix::fs::symlink;
    let fixture=Fixture::new();let outside=Fixture::new();
    std::fs::create_dir_all(&outside.root).unwrap();std::fs::write(outside.root.join("keep"),b"outside bytes").unwrap();
    for (index,name) in ["root","resources","generations","packs",".lock",".retiring",".retired"].into_iter().enumerate() {
        let key=format!("linked-{index}");let path=fixture.root.join(&key);
        if name=="root" {std::fs::create_dir_all(&fixture.root).unwrap();symlink(&outside.root,&path).unwrap();}
        else {
            drop(pollster::block_on(SessionStore::open(&path)).unwrap());let link=path.join(name);
            if link.is_dir() {std::fs::remove_dir(&link).unwrap();}else if link.exists() {std::fs::remove_file(&link).unwrap();}
            let target=if ["resources","generations","packs"].contains(&name) {outside.root.clone()}else {outside.root.join("keep")};
            symlink(target,link).unwrap();
        }
        let prepared=fixture.stores.prepare_retirement(&key).unwrap();fixture.run();assert!(prepared.try_take().is_some());
        let retired=fixture.stores.retire_copy(&key).unwrap();fixture.run();assert!(retired.try_take().is_some());
        assert!(std::fs::symlink_metadata(if name=="root" {path.clone()}else {path.join(name)}).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(outside.root.join("keep")).unwrap(),b"outside bytes");
        assert_eq!(std::fs::read_dir(&outside.root).unwrap().count(),1);
    }
}

#[test]
fn repeated_retirement_preserves_the_empty_terminal_marker() {
    use layer_core::package::session_store::collect_unreferenced_stores;
    let fixture=Fixture::new();let (saved,stamp,_,_)=capture();
    let binding=fixture.stores.bind(1,Some("closed")).unwrap();let path=binding.path().to_path_buf();
    let reply=fixture.stores.checkpoint(1,saved,stamp,true).unwrap();fixture.run();assert!(reply.try_take().unwrap().is_ok());
    let prepared=fixture.stores.prepare_retirement("closed").unwrap();fixture.run();assert!(prepared.try_take().unwrap().is_ok());
    let retired=fixture.stores.retire_copy("closed").unwrap();fixture.run();assert!(matches!(retired.try_take(),Some(Ok(StoreResult::Retired))));
    drop(binding);
    let marker=std::fs::metadata(path.join(".retired")).unwrap();let directory=std::fs::metadata(&path).unwrap();
    assert_eq!(marker.len(),0);assert!(!path.join(".retiring").exists());
    for _ in 0..8 {
        let prepared=fixture.stores.prepare_retirement("closed").unwrap();fixture.run();assert!(prepared.try_take().unwrap().is_ok());
        let retired=fixture.stores.retire_copy("closed").unwrap();fixture.run();assert!(retired.try_take().unwrap().is_ok());
        pollster::block_on(collect_unreferenced_stores(&fixture.root,&Default::default(),&AtomicBool::new(false)));
        let unchanged=std::fs::metadata(path.join(".retired")).unwrap();
        assert_eq!(marker.modified().unwrap(),unchanged.modified().unwrap());
        assert_eq!(directory.modified().unwrap(),std::fs::metadata(&path).unwrap().modified().unwrap());
        #[cfg(unix)] {use std::os::unix::fs::MetadataExt;assert_eq!(marker.ino(),unchanged.ino());}
        assert_eq!(std::fs::read_dir(&path).unwrap().count(),2);
        for directory in ["resources","generations","packs"] {assert!(!path.join(directory).exists());}
    }
}

#[test]
fn retired_registry_entries_release_the_writer_but_live_tiles_keep_their_pack() {
    let fixture = Fixture::new();
    let (capture, _, retained, tile) = capture();
    let expected = tile.decode_on_worker().unwrap();
    let binding = fixture.stores.bind(7, None).unwrap();
    let durable = fixture.stores.pressure_bound(&binding, retained.clone()).unwrap();
    fixture.run();
    assert!(matches!(durable.try_take(), Some(Ok(StoreResult::Durable(bytes))) if bytes > 0));
    let packs = std::fs::read_dir(binding.path().join("packs")).unwrap()
        .map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
    assert!(!packs.is_empty());
    let reply = fixture.stores.retire_bound(&binding).unwrap(); fixture.run();
    assert!(matches!(reply.try_take(), Some(Ok(StoreResult::Retired))));
    assert!(fixture.stores.bindings().is_empty());
    assert!(fixture.stores.0.state.lock().unwrap().drawings.is_empty());
    for path in &packs { assert!(path.exists()); }
    assert_eq!(tile.decode_on_worker().unwrap(), expected);
    drop((tile, retained, capture));
    pollster::block_on(binding.with_store(async |store| store.collect_garbage().await)).unwrap();
    for path in packs { assert!(!path.exists()); }
    assert_ne!(fixture.stores.bind(7, None).unwrap().key(), binding.key());
}

#[test]
fn dirty_observations_keep_the_first_deadline_and_clean_stamps_do_not_checkpoint() {
    let fixture = Fixture::new(); let (capture, first, _, _) = capture();
    let (second_capture, second) = named(&capture, &first, "Second");
    let interval = layer_ui::session_recovery::CHECKPOINT_INTERVAL_MS;
    assert!(!fixture.stores.observe(1, first.clone(), 100, false).unwrap());
    assert!(!fixture.stores.observe(1, second.clone(), 100 + interval / 2, false).unwrap());
    assert!(fixture.stores.observe(1, second.clone(), 100 + interval, false).unwrap());
    let reply = fixture.stores.checkpoint(1, second_capture, second.clone(), false).unwrap();
    fixture.run(); assert!(matches!(reply.try_take(), Some(Ok(StoreResult::Checkpoint(_)))));
    assert!(!fixture.stores.observe(1, second, 100 + 2 * interval, false).unwrap());
    assert!(fixture.stores.observe(1, first, 101 + 2 * interval, true).unwrap());
}

#[test]
fn checkpoint_due_import_force_and_interval_share_the_committed_batch_clock() {
    let fixture = Fixture::new(); let (capture, stamp, _, _) = capture();
    assert!(fixture.stores.checkpoint_due(1, stamp.clone(), false).unwrap());
    let reply = fixture.stores.import(1, capture.clone(), stamp.clone()).unwrap(); fixture.run();
    assert!(matches!(reply.try_take(), Some(Ok(StoreResult::Checkpoint(_)))));
    assert!(!fixture.stores.checkpoint_due(1, stamp.clone(), false).unwrap());
    assert!(!fixture.stores.checkpoint_due(1, stamp.clone(), true).unwrap());
    let (_, dirty) = named(&capture, &stamp, "Dirty");
    let binding = fixture.stores.bind(1, None).unwrap();
    {
        let mut state = fixture.stores.0.state.lock().unwrap();
        let drawing = state.drawings.get_mut(binding.key()).unwrap();
        assert!(drawing.committed_at.is_some());
        drawing.committed_at = Some(u64::MAX - SessionStores::checkpoint_interval_ms());
        drawing.due = None;
    }
    assert!(!fixture.stores.checkpoint_due(1, dirty.clone(), false).unwrap());
    assert!(fixture.stores.checkpoint_due(1, dirty.clone(), true).unwrap());
    {
        let mut state = fixture.stores.0.state.lock().unwrap();
        let drawing = state.drawings.get_mut(binding.key()).unwrap();
        drawing.committed_at = Some(10_000); drawing.due = None;
    }
    let deadline = 10_000 + SessionStores::checkpoint_interval_ms();
    assert!(!fixture.stores.observe(1, dirty.clone(), deadline - 1, false).unwrap());
    assert!(fixture.stores.observe(1, dirty, deadline, false).unwrap());
    assert!(!fixture.stores.observe(1, stamp, deadline, true).unwrap());
}

#[test]
fn returning_to_the_committed_stamp_supersedes_queued_optional_work() {
    let fixture = Fixture::new(); let (capture, stamp, _, _) = capture();
    let first = fixture.stores.import(1, capture.clone(), stamp.clone()).unwrap(); fixture.run();
    assert!(matches!(first.try_take(), Some(Ok(StoreResult::Checkpoint(_)))));
    let (edited, edited_stamp) = named(&capture, &stamp, "Undone edit");
    fixture.stores.observe(1, edited_stamp.clone(), 0, true).unwrap();
    let pending = fixture.stores.checkpoint(1, edited, edited_stamp, false).unwrap();
    assert!(!fixture.stores.checkpoint_due(1, stamp.clone(), false).unwrap());
    fixture.run();
    assert!(matches!(pending.try_take(), Some(Ok(StoreResult::Superseded))));
    assert!(fixture.stores.is_current(1, &stamp));
}

#[test]
fn restored_stamp_is_committed_at_ui_adoption() {
    for edit_after_restore in 0..4 {
        let fixture = Fixture::new(); let (capture, stamp, _, _) = capture();
        let binding = fixture.stores.bind(1, None).unwrap();
        pollster::block_on(binding.with_store(async |store| capture.checkpoint(store, &AtomicBool::new(false)).await)).unwrap();
        let head = heads(binding.path());
        assert!(!fixture.stores.is_current(1, &stamp));
        let reply = fixture.stores.restore_bound(&binding, ProjectLimits::default()).unwrap(); fixture.run();
        let Some(Ok(StoreResult::Restored {session: Some(restore), recovered_previous: false})) = reply.try_take()
            else {panic!("stored session must restore")};
        assert!(!fixture.stores.is_current(1, &stamp));
        let mut session = layer_ui::UiSession::from_project(crate::Renderer::default(), restore.document().clone(),
            None, [256; 2], layer_ui::Platform::Gtk).unwrap();
        let mut window=layer_ui::UiSession::from_project(crate::Renderer::default(),restore.document().clone(),
            None,[256;2],layer_ui::Platform::Gtk).unwrap();
        window.set_viewport([1706.6666,1018.6667],[2560,1528]).unwrap();
        let observed = restore.state.destination.clone();
        session.restore_session(restore, false, observed).unwrap();
        session.inherit_restored_window_state(&window).unwrap();
        session.set_viewport([1706.6666,1018.6667],[2560,1528]).unwrap();
        assert!(fixture.stores.0.state.lock().unwrap().drawings[binding.key()].committed.is_some());
        match edit_after_restore {
            1=>{session.dispatch(layer_ui::UiAction::Layer {action:layer_ui::LayerAction::New {group:false,clipped:false}}).unwrap();}
            2=>{session.gesture([100.,100.],[125.,130.],1.,0.).unwrap();}
            3=>{session.dispatch(layer_ui::UiAction::SetZoomLocked {locked:true}).unwrap();}
            _=>{}
        }
        let adopted = session.session_stamp();
        assert_eq!(fixture.stores.is_current(1, &adopted), edit_after_restore==0);
        assert!(!fixture.stores.checkpoint_due(1, adopted.clone(), false).unwrap());
        assert_eq!(fixture.stores.observe(1,adopted,(fixture.stores.0.clock)()+SessionStores::checkpoint_interval_ms(),false).unwrap(),edit_after_restore!=0);
        assert_eq!(heads(binding.path()), head);
        assert!(fixture.queue.lock().unwrap().is_empty());
    }
}

#[test]
fn optional_checkpoints_coalesce_to_the_latest_dirty_capture() {
    let fixture = Fixture::new(); let (capture, stamp, retained, _) = capture();
    let first = fixture.stores.pressure(1, retained).unwrap();
    let (older, older_stamp) = named(&capture, &stamp, "Older");
    let (latest, latest_stamp) = named(&capture, &stamp, "Latest");
    fixture.stores.observe(1, older_stamp.clone(), 0, true).unwrap();
    let older_reply = fixture.stores.checkpoint(1, older, older_stamp, false).unwrap();
    fixture.stores.observe(1, latest_stamp.clone(), 1, true).unwrap();
    let latest_reply = fixture.stores.checkpoint(1, latest, latest_stamp.clone(), false).unwrap();
    assert!(older_reply.try_take().is_none());
    assert_eq!(fixture.queue.lock().unwrap().len(), 1);
    fixture.run(); assert!(matches!(first.try_take(), Some(Ok(StoreResult::Durable(_)))));
    assert_eq!(fixture.queue.lock().unwrap().len(), 1);
    fixture.run();
    let Some(Ok(StoreResult::Checkpoint(older_generation))) = older_reply.try_take() else {panic!("older waiter must receive the durable batch")};
    let Some(Ok(StoreResult::Checkpoint(latest_generation))) = latest_reply.try_take() else {panic!("latest waiter must receive the durable batch")};
    assert_eq!(older_generation, latest_generation);
    assert!(!fixture.stores.observe(1, latest_stamp, 10_000, false).unwrap());
    let binding = fixture.stores.bind(1, None).unwrap();
    let name = pollster::block_on(binding.with_store(async |store| Ok(store.load(ProjectLimits::default(), &AtomicBool::new(false)).await
        .unwrap().unwrap().metadata.value["unsaved_name"].as_str().unwrap().to_owned()))).unwrap();
    assert_eq!(name, "Latest");
}

#[test]
fn pressure_and_forced_checkpoint_waiters_follow_the_latest_durable_batch() {
    let fixture = Fixture::new(); let (capture, stamp, retained, _) = capture();
    let first = fixture.stores.checkpoint(1, capture.clone(), stamp.clone(), true).unwrap();
    let pressure = fixture.stores.pressure(1, retained.clone()).unwrap();
    let merged_pressure = fixture.stores.pressure(1, retained).unwrap();
    let (second, second_stamp) = named(&capture, &stamp, "B");
    let second_reply = fixture.stores.checkpoint(1, second, second_stamp, true).unwrap();
    let (latest, latest_stamp) = named(&capture, &stamp, "C");
    let latest_reply = fixture.stores.checkpoint(1, latest, latest_stamp.clone(), true).unwrap();
    assert!(pressure.try_take().is_none()); assert!(merged_pressure.try_take().is_none());
    assert!(second_reply.try_take().is_none()); assert!(latest_reply.try_take().is_none());
    fixture.run(); assert!(matches!(first.try_take(), Some(Ok(StoreResult::Checkpoint(_)))));
    while !fixture.queue.lock().unwrap().is_empty() { fixture.run(); }
    assert!(pressure.try_take().unwrap().is_ok()); assert!(merged_pressure.try_take().unwrap().is_ok());
    let Some(Ok(StoreResult::Checkpoint(second_generation))) = second_reply.try_take() else {panic!("forced waiter must complete")};
    let Some(Ok(StoreResult::Checkpoint(latest_generation))) = latest_reply.try_take() else {panic!("latest forced waiter must complete")};
    assert_eq!(second_generation, latest_generation);
    assert!(fixture.stores.is_current(1, &latest_stamp));
    let binding = fixture.stores.bind(1, None).unwrap();
    let name = pollster::block_on(binding.with_store(async |store| Ok(store.load(ProjectLimits::default(), &AtomicBool::new(false)).await
        .unwrap().unwrap().metadata.value["unsaved_name"].as_str().unwrap().to_owned()))).unwrap();
    assert_eq!(name, "C");
    assert!(fixture.queue.lock().unwrap().is_empty());
}

#[test]
fn coalescing_an_optional_capture_preserves_the_forced_waiter() {
    let fixture = Fixture::new(); let (capture, stamp, retained, _) = capture();
    let first = fixture.stores.pressure(1, retained).unwrap();
    let (forced, forced_stamp) = named(&capture, &stamp, "Forced");
    let forced_reply = fixture.stores.checkpoint(1, forced, forced_stamp, true).unwrap();
    let (latest, latest_stamp) = named(&capture, &stamp, "Latest");
    let latest_reply = fixture.stores.checkpoint(1, latest, latest_stamp.clone(), false).unwrap();
    let (_, newer_observation) = named(&capture, &stamp, "Not captured yet");
    fixture.stores.observe(1, newer_observation, 0, true).unwrap();
    fixture.run(); assert!(first.try_take().unwrap().is_ok()); fixture.run();
    let Some(Ok(StoreResult::Checkpoint(forced_generation))) = forced_reply.try_take() else {panic!("forced waiter must remain durable")};
    let Some(Ok(StoreResult::Checkpoint(latest_generation))) = latest_reply.try_take() else {panic!("latest capture must inherit forced admission")};
    assert_eq!(forced_generation, latest_generation);
    assert!(fixture.stores.is_current(1, &latest_stamp));
}

#[test]
fn retirement_supersedes_queued_and_new_work_and_completes_its_reply() {
    let fixture = Fixture::new(); let (capture, stamp, retained, _) = capture();
    let binding = fixture.stores.bind(1, None).unwrap();
    let current = fixture.stores.checkpoint_bound(&binding, capture.clone(), stamp.clone(), true, None).unwrap();
    let pressure = fixture.stores.pressure_bound(&binding, retained.clone()).unwrap();
    let checkpoint = fixture.stores.checkpoint_bound(&binding, capture.clone(), stamp.clone(), true, None).unwrap();
    let retire = fixture.stores.retire_bound(&binding).unwrap();
    assert!(matches!(pressure.try_take(), Some(Ok(StoreResult::Superseded))));
    assert!(matches!(checkpoint.try_take(), Some(Ok(StoreResult::Superseded))));
    assert_eq!(fixture.queue.lock().unwrap().len(), 1);
    fixture.run();
    assert!(matches!(current.try_take(), Some(Ok(StoreResult::Checkpoint(_)))));
    assert_eq!(fixture.queue.lock().unwrap().len(), 1);
    assert!(retire.try_take().is_none());
    let rejected_checkpoint = fixture.stores.checkpoint_bound(&binding, capture, stamp, true, None).unwrap();
    let rejected_pressure = fixture.stores.pressure_bound(&binding, retained).unwrap();
    assert!(matches!(rejected_checkpoint.try_take(), Some(Ok(StoreResult::Superseded))));
    assert!(matches!(rejected_pressure.try_take(), Some(Ok(StoreResult::Superseded))));
    fixture.run();
    assert!(matches!(retire.try_take(), Some(Ok(StoreResult::Retired))));
    assert!(fixture.stores.bindings().is_empty());
    assert!(fixture.stores.0.state.lock().unwrap().drawings.is_empty());
    assert!(fixture.queue.lock().unwrap().is_empty());
}

#[test]
fn an_edit_observed_during_a_held_batch_is_due_on_the_next_poll() {
    let fixture = Fixture::new(); let (capture, stamp, _, _) = capture();
    let first = fixture.stores.import(1, capture.clone(), stamp.clone()).unwrap(); fixture.run();
    assert!(matches!(first.try_take(), Some(Ok(StoreResult::Checkpoint(_)))));
    let binding = fixture.stores.bind(1, None).unwrap();
    let (batch, batch_stamp) = named(&capture, &stamp, "Held batch");
    fixture.stores.observe(1, batch_stamp.clone(), 0, true).unwrap();
    let (entered, started) = std::sync::mpsc::channel(); let (resume, proceed) = std::sync::mpsc::channel();
    let reply = fixture.stores.checkpoint_bound(&binding, batch, batch_stamp, false,
        Some(Box::new(move |_| {entered.send(()).unwrap(); proceed.recv().unwrap(); Ok(())}))).unwrap();
    let work = fixture.pop(); let worker = std::thread::spawn(move || work.run());
    started.recv().unwrap();
    let (last_capture, last_stamp) = named(&capture, &stamp, "Last edit");
    assert!(!fixture.stores.observe(1, last_stamp.clone(), SessionStores::checkpoint_interval_ms(), false).unwrap());
    fixture.stores.0.state.lock().unwrap().drawings.get_mut(binding.key()).unwrap().due = Some((fixture.stores.0.clock)());
    resume.send(()).unwrap(); worker.join().unwrap();
    assert!(matches!(reply.try_take(), Some(Ok(StoreResult::Checkpoint(_)))));
    assert!(fixture.stores.checkpoint_due(1, last_stamp.clone(), false).unwrap());
    let last = fixture.stores.checkpoint(1, last_capture, last_stamp.clone(), false).unwrap(); fixture.run();
    assert!(matches!(last.try_take(), Some(Ok(StoreResult::Checkpoint(_)))));
    assert!(fixture.stores.is_current(1, &last_stamp));
}

#[test]
fn pressure_selects_the_oldest_on_its_worker_and_recounts_shared_tiles_before_stopping() {
    let fixture = Fixture::new();
    let (_old_capture, _, old, old_tile) = capture();
    let (_young_capture, _, young, young_tile) = capture();
    let budget = young.resident_bytes();
    assert_eq!(layer_core::raster_storage::resident_tile_bytes([&old, &old, &young]), old.resident_bytes() + budget);
    let reply = fixture.stores.trim(vec![(3, 30, young.clone()), (2, 20, old.clone()), (1, 10, old.clone())], budget);
    assert!(!fixture.root.exists());
    assert_eq!(old_tile.resident_bytes(), old.resident_bytes());
    assert!(fixture.stores.bindings().is_empty());
    fixture.run();
    assert_eq!(fixture.stores.bindings().iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![1]);
    assert!(old_tile.resident_bytes() > 0);
    fixture.run();
    assert_eq!(old_tile.resident_bytes(), 0);
    fixture.run();
    assert!(matches!(reply.try_take(), Some(Ok(StoreResult::Durable(_)))));
    assert_eq!(young_tile.resident_bytes(), budget);
    assert_eq!(layer_core::raster_storage::resident_tile_bytes([&old, &young]), budget);
    assert_eq!(fixture.stores.bindings().iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![1]);
    assert!(fixture.queue.lock().unwrap().is_empty());
}

#[test]
fn parked_budget_source_schedules_storage_without_an_owner_poll() {
    let fixture = Fixture::new();
    let mut documents = layer_ui::DocumentSessions::<()>::default();
    documents.budget.inactive_ram = 0;
    fixture.stores.watch_pressure(documents.pressure_source());
    while !fixture.queue.lock().unwrap().is_empty() { fixture.run(); }
    let (_capture, _, retained, tile) = capture();
    let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    let metadata=retained.metadata_bytes;
    documents.append((), retained.clone(), &localization);
    assert_eq!(documents.inactive_ram_budget(),0);
    let wait=documents.trim_wait().expect("zero inactive allowance must schedule storage");
    assert!(tile.resident_bytes() > 0);
    assert!(!fixture.queue.lock().unwrap().is_empty());
    while !fixture.queue.lock().unwrap().is_empty() { fixture.run(); }
    wait.wait_on_worker().expect("spilling inactive raster payloads must satisfy the zero allowance");
    assert_eq!(tile.resident_bytes(), 0);
    assert_eq!(fixture.stores.bindings().len(), 1);
    assert_eq!(retained.metadata_bytes,metadata,"durability preserves drawing metadata");
    let candidate=layer_core::Document::new(PortableId::random(),256,256,
        layer_core::DocumentNames {paint:"New".into(),paper:"Paper".into()});
    documents.admission(&RetainedTiles::default()).admit(&candidate).expect("a completed pressure trim must permit opening the next drawing");
}

#[test]
fn one_pressure_subscription_coalesces_new_roots_and_finishes_every_trim_waiter() {
    let fixture=Fixture::new();let localization=layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    let mut documents=layer_ui::DocumentSessions::<()>::default();documents.set_inactive_ram_budget(0);
    fixture.stores.watch_pressure(documents.pressure_source());
    let (_capture,_,first,first_tile)=capture();documents.append((),first,&localization);
    let first_wait=documents.trim_wait().unwrap();
    let (_capture,_,second,second_tile)=capture();documents.append((),second,&localization);
    let second_wait=documents.trim_wait().unwrap();
    assert_eq!(fixture.queue.lock().unwrap().len(),1);
    assert!(documents.trim_in_flight());
    while !fixture.queue.lock().unwrap().is_empty() {fixture.run();}
    first_wait.wait_on_worker().unwrap();second_wait.wait_on_worker().unwrap();
    assert!(!documents.trim_in_flight());assert!(documents.storage_error().is_none());
    assert_eq!(first_tile.resident_bytes()+second_tile.resident_bytes(),0);
}

#[test]
fn failed_subscription_finishes_trim_waiters_and_blocks_work_until_storage_is_cleared() {
    let fixture=Fixture::new();let localization=layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    let mut documents=layer_ui::DocumentSessions::<()>::default();documents.set_inactive_ram_budget(0);
    fixture.stores.watch_pressure(documents.pressure_source());
    let (_capture,_,retained,tile)=capture();documents.append((),retained,&localization);
    let wait=documents.trim_wait().unwrap();drop(fixture.pop());
    assert_eq!(wait.wait_on_worker().unwrap_err(),"Drawing storage worker stopped");
    assert!(documents.storage_error().is_some());assert!(!documents.trim_in_flight());
    assert!(fixture.queue.lock().unwrap().is_empty());assert!(tile.resident_bytes()>0);
    documents.storage_completed(Ok(()));let retry=documents.trim_wait().unwrap();
    while !fixture.queue.lock().unwrap().is_empty() {fixture.run();}
    retry.wait_on_worker().unwrap();assert!(documents.storage_error().is_none());assert_eq!(tile.resident_bytes(),0);
}

#[test]
fn a_committed_checkpoint_clears_storage_failure_and_finishes_parked_pressure() {
    let fixture=Fixture::new();let localization=layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    let mut documents=layer_ui::DocumentSessions::<()>::default();documents.set_inactive_ram_budget(0);
    let parked=documents.selected();fixture.stores.watch_pressure(documents.pressure_source());
    let (capture,stamp,retained,tile)=capture();documents.append((),retained,&localization);
    let failed=documents.trim_wait().unwrap();drop(fixture.pop());
    assert_eq!(failed.wait_on_worker().unwrap_err(),"Drawing storage worker stopped");
    assert!(documents.storage_error().is_some());assert!(tile.resident_bytes()>0);
    assert!(fixture.queue.lock().unwrap().is_empty());
    let checkpoint=fixture.stores.checkpoint(parked,capture,stamp,true).unwrap();fixture.run();
    assert!(matches!(checkpoint.try_take(),Some(Ok(StoreResult::Checkpoint(_)))));
    assert!(documents.storage_error().is_none());
    let retry=documents.trim_wait().expect("successful durable work resumes pressure accounting");
    while !fixture.queue.lock().unwrap().is_empty() {fixture.run();}
    retry.wait_on_worker().unwrap();assert!(!documents.trim_in_flight());
    assert_eq!(tile.resident_bytes(),0);assert_eq!(documents.resident_bytes(),0);
    assert!(documents.storage_error().is_none());
}

#[test]
fn stale_optional_work_skips_preparation_and_storage_but_forced_work_runs() {
    let fixture = Fixture::new(); let (capture, stamp, _, _) = capture();
    let binding = fixture.stores.bind(1, None).unwrap();
    let prepared = Arc::new(AtomicUsize::new(0)); let count = prepared.clone();
    fixture.stores.observe(1, stamp.clone(), 0, true).unwrap();
    let reply = fixture.stores.checkpoint_bound(&binding, capture.clone(), stamp.clone(), false,
        Some(Box::new(move |_| {count.fetch_add(1, Ordering::SeqCst); Ok(())}))).unwrap();
    let (_, latest) = named(&capture, &stamp, "Later");
    fixture.stores.observe(1, latest, 1, true).unwrap(); fixture.run();
    assert!(matches!(reply.try_take(), Some(Ok(StoreResult::Superseded))));
    assert_eq!(prepared.load(Ordering::SeqCst), 0); assert!(!binding.path().exists());
    let count = prepared.clone();
    let reply = fixture.stores.checkpoint_bound(&binding, capture, stamp, true,
        Some(Box::new(move |_| {count.fetch_add(1, Ordering::SeqCst); Ok(())}))).unwrap();
    fixture.run(); assert!(matches!(reply.try_take(), Some(Ok(StoreResult::Checkpoint(_)))));
    assert_eq!(prepared.load(Ordering::SeqCst), 1);
}

struct ReplyWake(AtomicUsize);
struct Photos(layer_core::color::DocumentColor);
impl layer_render::CanvasRenderer for Photos {
    type Error=std::io::Error;
    fn supports_tiled_sources(&self)->bool {true}
    fn document_color(&self)->layer_core::color::DocumentColor {self.0}
    fn resize_surface(&mut self,_width:u32,_height:u32)->Result<(),Self::Error> {Ok(())}
    fn prepare_asset(&mut self,_asset:&layer_core::AssetId,_image:layer_render::HostImage<'_>)->Result<(),Self::Error> {Ok(())}
    fn release_asset(&mut self,_asset:&layer_core::AssetId) {}
    fn submit(&mut self,_packet:layer_render::FramePacket<'_>)->Result<(),Self::Error> {Ok(())}
}
impl std::task::Wake for ReplyWake {
    fn wake(self: Arc<Self>) {self.0.fetch_add(1, Ordering::SeqCst);}
}
#[test]
fn session_attachment_leaves_foreign_outgoing_storage_and_pressure_untouched() {
    let outgoing=Fixture::new();let incoming=Fixture::new();
    let documents=layer_ui::DocumentSessions::<()>::default();let pressure=documents.pressure_source();
    let document=||layer_core::Document::new(PortableId::random(),32,32,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let blank=document();let session=layer_ui::UiSession::new(Photos(blank.composition().color),blank,[32,32],layer_ui::Platform::Android).unwrap();
    assert!(outgoing.stores.attach_session(1,&session,pressure.clone()).unwrap());
    let binding=session.asset_writer().binding().unwrap();
    let generation=outgoing.stores.0.pressure.lock().unwrap().as_ref().unwrap().1;
    assert!(!incoming.stores.attach_session(1,&session,pressure.clone()).unwrap());
    assert!(incoming.stores.bindings().is_empty());assert!(incoming.stores.0.pressure.lock().unwrap().is_none());
    assert!(pressure.is_current(generation));
    assert_eq!(session.asset_writer().binding().unwrap().path(),binding.path());
    assert!(incoming.stores.adopt_binding(1,binding).is_err());
    assert!(outgoing.queue.lock().unwrap().is_empty());assert!(incoming.queue.lock().unwrap().is_empty());
    assert!(!outgoing.root.exists() && !incoming.root.exists());
    let restored=document();let restored_binding=incoming.stores.bind(2,None).unwrap();
    restored.memory().bind_storage(restored_binding.clone()).unwrap();
    let adopted=layer_ui::UiSession::new(Photos(restored.composition().color),restored,[32,32],layer_ui::Platform::Android).unwrap();
    assert!(incoming.stores.attach_session(2,&adopted,pressure.clone()).unwrap());
    assert_eq!(adopted.asset_writer().binding().unwrap().path(),restored_binding.path());
    assert!(!pressure.is_current(generation));
    assert!(outgoing.stores.install_storage(2,&adopted).is_err());
    assert!(outgoing.queue.lock().unwrap().is_empty());assert!(incoming.queue.lock().unwrap().is_empty());
    assert!(!outgoing.root.exists() && !incoming.root.exists());
}

#[test]
fn imports_captured_before_storage_installation_keep_ram_and_use_one_stable_binding() {
    use std::{future::Future,task::{Context,Poll,Waker}};
    let fixture=Fixture::new();
    let source=layer_core::color::source::rgba8_source([32,32],|x,y|[x as u8,y as u8,127,255]);
    let mut document=layer_core::Document::new(PortableId::random(),32,32,layer_core::DocumentNames {paint:"Photo".into(),paper:"Paper".into()});
    let layer_core::SourceTarget::Paint(paint)=document.working.target.unwrap() else {unreachable!()};
    document.artwork.paint.get_mut(paint).unwrap().base=Some(layer_core::authored::PaintBase::new(source.clone().into()));
    let color=document.composition().color;
    let session=layer_ui::UiSession::new(Photos(color),document,[32,32],layer_ui::Platform::Android).unwrap();
    let captured=session.image_import_writer().unwrap();let fork=captured.fork().unwrap();
    assert!(captured.resolve().unwrap().is_none());assert!(fork.resolve().unwrap().is_none());
    let mut context=Context::from_waker(Waker::noop());
    let mut write=Box::pin(captured.store_document(session.engine().document()));
    assert!(matches!(write.as_mut().poll(&mut context),Poll::Ready(Ok(()))));drop(write);
    assert!(session.engine().document().memory().storage().is_none());
    assert!(source.tiles.values().all(|tile|tile.resident_bytes()>0));
    assert!(!fixture.root.exists());assert!(fixture.queue.lock().unwrap().is_empty());
    let documents=layer_ui::DocumentSessions::<()>::default();
    assert!(fixture.stores.attach_session(1,&session,documents.pressure_source()).unwrap());
    let binding=fixture.stores.bind(1,None).unwrap();
    assert!(!fixture.root.exists());assert!(fixture.queue.lock().unwrap().is_empty());
    let derived=fork.resolve().unwrap().unwrap().store;
    assert_ne!(derived.key(),binding.key());assert_eq!(derived.path().parent(),Some(fixture.root.as_path()));
    assert_eq!(fork.resolve().unwrap().unwrap().store.path(),derived.path());
    assert_eq!(captured.resolve().unwrap().unwrap().store.path(),binding.path());
    let mut write=Box::pin(captured.store_document(session.engine().document()));
    assert!(write.as_mut().poll(&mut context).is_pending());fixture.run();
    assert!(matches!(write.as_mut().poll(&mut context),Poll::Ready(Ok(()))));
    assert_eq!(session.engine().document().memory().storage().unwrap().path(),binding.path());
    assert!(source.tiles.values().all(|tile|tile.resident_bytes()==0));
}
#[test]
fn new_tabs_share_registration_or_bind_directly_without_changing_captured_identity() {
    for direct in [false,true] {
        let fixture=Fixture::new();
        let make=|| {let document=layer_core::Document::new(PortableId::random(),32,32,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
            layer_ui::UiSession::new(Photos(document.composition().color),document,[32,32],layer_ui::Platform::Android).unwrap()};
        let first=make();let incoming=make();
        incoming.bind_asset_writer(first.asset_writer().fork().unwrap()).unwrap();
        let captured=incoming.image_import_writer().unwrap();assert!(captured.resolve().unwrap().is_none());
        if direct {fixture.stores.install_storage(2,&incoming).unwrap();}
        let first_binding=fixture.stores.install_storage(1,&first).unwrap();
        let binding=captured.resolve().unwrap().unwrap().store;assert_ne!(binding.key(),first_binding.key());
        assert_eq!(binding.path().parent(),Some(fixture.root.as_path()));
        assert_eq!(fixture.stores.install_storage(2,&incoming).unwrap().path(),binding.path());
        assert_eq!(incoming.engine().document().memory().storage().unwrap().path(),binding.path());
        assert_eq!(captured.resolve().unwrap().unwrap().store.path(),binding.path());
        assert!(!fixture.root.exists());assert!(fixture.queue.lock().unwrap().is_empty());
    }
}
#[test]
fn opening_window_storage_attaches_active_and_parked_writers_without_changing_identity() {
    let fixture=Fixture::new();let mut host=crate::NativeHost::new(layer_ui::Platform::Android).unwrap();
    let mut window=crate::window::DocumentWindow::<Box<layer_ui::UiSession<crate::Renderer>>>::default();
    let inherited=host.session.asset_writer().fork().unwrap();
    let incoming=layer_ui::UiSession::new(crate::Renderer(None),layer_ui::new_drawing(32,32,host.session.localization()).unwrap(),[32,32],layer_ui::Platform::Android).unwrap();
    incoming.bind_asset_writer(inherited).unwrap();
    let outgoing=std::mem::replace(&mut host.session,Box::new(incoming));let retained=outgoing.retained_document_tiles();
    window.documents.append(outgoing,retained,host.session.localization());
    let captured=window.documents.order().iter().map(|id|window.session(&host,*id).unwrap().image_import_writer().unwrap()).collect::<Vec<_>>();
    for writer in &captured {assert!(writer.resolve().unwrap().is_none());}
    assert!(fixture.stores.attach_window(&window,&host).unwrap());
    let bindings=captured.iter().map(|writer| {
        let binding=writer.resolve().unwrap().unwrap().store;
        assert_eq!(binding.path().parent(),Some(fixture.root.as_path()));
        assert_eq!(writer.resolve().unwrap().unwrap().store.path(),binding.path());binding.path().to_owned()
    }).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(bindings.len(),2);
    assert_eq!(fixture.stores.bindings().len(),2);assert!(!fixture.root.exists());assert!(fixture.queue.lock().unwrap().is_empty());
}

#[test]
fn concurrent_writer_clones_keep_one_target_and_forks_keep_independent_targets() {
    let fixture=Fixture::new();let writer=fixture.stores.asset_writer(1).unwrap();
    let clones=std::thread::scope(|scope| {
        (0..8).map(|_| {let writer=writer.clone();scope.spawn(move ||writer.resolve().unwrap().unwrap().store.path().to_owned())})
            .collect::<Vec<_>>().into_iter().map(|thread|thread.join().unwrap()).collect::<Vec<_>>()
    });
    let binding=writer.binding().unwrap();assert!(clones.iter().all(|path|path==binding.path()));
    let forks=std::thread::scope(|scope| {
        (0..4).map(|_| {let writer=writer.clone();scope.spawn(move || {
            let fork=writer.fork().unwrap();let binding=fork.resolve().unwrap().unwrap().store;
            assert_eq!(fork.clone().resolve().unwrap().unwrap().store.path(),binding.path());binding.path().to_owned()
        })}).collect::<Vec<_>>().into_iter().map(|thread|thread.join().unwrap()).collect::<std::collections::BTreeSet<_>>()
    });
    assert_eq!(forks.len(),4);
    assert!(forks.iter().all(|path|path!=binding.path() && path.parent()==Some(fixture.root.as_path())));
    assert_eq!(writer.resolve().unwrap().unwrap().store.path(),binding.path());
    let document=layer_core::Document::new(PortableId::random(),32,32,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let original=writer.for_document(&document).unwrap();let shared=writer.for_document(&document.clone()).unwrap();
    let owned=original.resolve().unwrap().unwrap().store;assert_eq!(shared.resolve().unwrap().unwrap().store.path(),owned.path());
    let fresh=document.clone().with_memory(layer_core::DocumentMemory::new(layer_core::MemoryBudget::process(),None));
    assert_ne!(writer.for_document(&fresh).unwrap().resolve().unwrap().unwrap().store.path(),owned.path());
    assert!(!fixture.root.exists());assert!(fixture.queue.lock().unwrap().is_empty());
}

#[test]
fn concurrent_resolution_and_session_installation_choose_one_storage_identity() {
    let fixture=Fixture::new();let root=fixture.stores.asset_writer(1).unwrap();
    let document=layer_core::Document::new(PortableId::random(),32,32,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let session=Box::new(layer_ui::UiSession::new(Photos(document.composition().color),document,[32,32],layer_ui::Platform::Android).unwrap());
    session.bind_asset_writer(root.fork().unwrap()).unwrap();let captured=session.asset_writer();
    assert!(captured.binding().is_none());assert!(session.engine().document().memory().storage().is_none());
    let barrier=Arc::new(std::sync::Barrier::new(9));
    let ((session,binding),resolved)=std::thread::scope(|scope| {
        let readers=(0..8).map(|_| {let captured=captured.clone();let barrier=barrier.clone();scope.spawn(move || {
            barrier.wait();captured.resolve().unwrap().unwrap().store.path().to_owned()
        })}).collect::<Vec<_>>();
        let stores=fixture.stores.clone();let barrier=barrier.clone();
        let install=scope.spawn(move || {barrier.wait();let binding=stores.install_storage(2,&session).unwrap();(session,binding)});
        let resolved=readers.into_iter().map(|reader|reader.join().unwrap()).collect::<Vec<_>>();
        (install.join().unwrap(),resolved)
    });
    assert!(resolved.iter().all(|path|path==binding.path()));
    assert_eq!(captured.binding().unwrap().path(),binding.path());
    assert_eq!(session.engine().document().memory().storage().unwrap().path(),binding.path());
    assert_eq!(fixture.stores.bind(2,None).unwrap().path(),binding.path());
    assert_eq!(captured.resolve().unwrap().unwrap().store.path(),binding.path());
    assert!(!fixture.root.exists());assert!(fixture.queue.lock().unwrap().is_empty());
}

#[test]
fn startup_claim_retry_replaces_registration_without_reviving_failed_captures() {
    use std::{future::Future,task::{Context,Poll,Waker}};
    let fixture=Fixture::new();
    let mut document=layer_core::Document::new(PortableId::random(),32,32,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let source=layer_core::color::source::rgba8_source([4;2],|_,_|[12,34,56,255]);
    let layer_core::SourceTarget::Paint(paint)=document.working.target.unwrap() else {unreachable!()};
    document.artwork.paint.get_mut(paint).unwrap().base=Some(layer_core::authored::PaintBase::new(source.clone().into()));
    let session=layer_ui::UiSession::new(Photos(document.composition().color),document,[32,32],layer_ui::Platform::Android).unwrap();
    let captured=session.asset_writer();let fork=captured.fork().unwrap();
    assert!(captured.resolve().unwrap().is_none());assert!(fork.resolve().unwrap().is_none());
    session.storage_failed("Startup claim failed".into());
    assert!(matches!(captured.resolve(),Err(error) if error=="Startup claim failed"));
    assert!(matches!(fork.resolve(),Err(error) if error=="Startup claim failed"));
    assert!(!fixture.root.exists());assert!(fixture.queue.lock().unwrap().is_empty());
    let binding=fixture.stores.install_storage(1,&session).unwrap();let writer=session.asset_writer();
    assert_eq!(writer.resolve().unwrap().unwrap().store.path(),binding.path());
    assert_eq!(session.engine().document().memory().storage().unwrap().path(),binding.path());
    assert_eq!(fixture.stores.bind(1,None).unwrap().path(),binding.path());
    assert!(matches!(captured.resolve(),Err(error) if error=="Drawing storage owner closed"));
    assert!(matches!(fork.resolve(),Err(error) if error=="Drawing storage owner closed"));
    assert!(!fixture.root.exists());assert!(fixture.queue.lock().unwrap().is_empty());
    let mut writing=Box::pin(writer.store_document(session.engine().document()));let mut context=Context::from_waker(Waker::noop());
    assert!(writing.as_mut().poll(&mut context).is_pending());fixture.run();
    assert!(matches!(writing.as_mut().poll(&mut context),Poll::Ready(Ok(()))));
    assert_eq!(writer.resolve().unwrap().unwrap().store.path(),binding.path());
    for tile in source.tiles.values() {assert_eq!(tile.resident_bytes(),0);assert_eq!(&tile.decode_on_worker().unwrap()[..4],&[12,34,56,255]);}
}

#[test]
fn a_closed_coordinator_cannot_redirect_captured_writers_to_another_root() {
    let outgoing=Fixture::new();let incoming=Fixture::new();
    let make=|| {let document=layer_core::Document::new(PortableId::random(),32,32,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        layer_ui::UiSession::new(Photos(document.composition().color),document,[32,32],layer_ui::Platform::Android).unwrap()};
    let session=make();let original=outgoing.stores.install_storage(1,&session).unwrap();
    let captured=session.asset_writer();let fork=captured.fork().unwrap();
    let derived=fork.resolve().unwrap().unwrap().store;let before=captured.resolve().unwrap().unwrap();
    drop(outgoing);
    assert!(matches!(captured.resolve(),Err(error) if error=="Drawing storage owner closed"));
    assert!(matches!(fork.resolve(),Err(error) if error=="Drawing storage owner closed"));
    let (_,_,retained,_)=capture();
    assert!(matches!((before.write)(retained,None),Err(error) if error=="Drawing storage owner closed"));
    assert!(incoming.stores.install_storage(1,&session).is_err());
    let replacement=make();let current=incoming.stores.install_storage(1,&replacement).unwrap();
    assert_eq!(replacement.asset_writer().resolve().unwrap().unwrap().store.path(),current.path());
    assert_eq!(captured.binding().unwrap().path(),original.path());assert_eq!(fork.binding().unwrap().path(),derived.path());
    assert!(captured.resolve().is_err());assert!(fork.resolve().is_err());
    assert_ne!(current.path().parent(),original.path().parent());
    assert!(!incoming.root.exists());assert!(incoming.queue.lock().unwrap().is_empty());
}

#[test]
fn panicked_or_dropped_work_completes_replies_wakes_and_releases_busy_state() {
    use std::future::Future;
    for panic in [false, true] {
        let fixture = Fixture::new(); let (capture, stamp, retained, _) = capture();
        let binding = fixture.stores.bind(1, None).unwrap();
        let mut reply = fixture.stores.checkpoint_bound(&binding, capture, stamp, true,
            Some(Box::new(|_| panic!("fixture preparation failure")))).unwrap();
        let wake = Arc::new(ReplyWake(AtomicUsize::new(0))); let waker = Waker::from(wake.clone());
        assert!(std::pin::Pin::new(&mut reply).poll(&mut std::task::Context::from_waker(&waker)).is_pending());
        if panic {fixture.run();}else{drop(fixture.pop());}
        assert!(reply.try_take().unwrap().is_err());
        assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.wakes.load(Ordering::SeqCst), 1);
        let next = fixture.stores.pressure_bound(&binding, retained).unwrap();
        assert_eq!(fixture.queue.lock().unwrap().len(), 1); fixture.run();
        assert!(matches!(next.try_take(), Some(Ok(StoreResult::Durable(_)))));
        assert_eq!(fixture.wakes.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn an_import_prepared_before_binding_is_persisted_by_the_later_checkpoint() {
    let fixture=Fixture::new();
    let document=layer_core::Document::new(PortableId::random(),32,32,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let startup=layer_ui::UiSession::new(Photos(document.composition().color),document,[32,32],layer_ui::Platform::Mac).unwrap();
    let source=layer_core::color::source::rgba8_source([4;2],|_,_|[12,34,56,255]);
    let tile=source.tiles.values().next().unwrap().clone();
    let mut images=layer_ui::ImageImportBatch::new(Default::default(),layer_core::color::RgbSpace::ProPhoto,layer_ui::photo_decode_limits())
        .with_writer(startup.asset_writer());
    images.append("Photo".into(),(*source).clone(),false).unwrap();
    pollster::block_on(images.finish_storage()).unwrap();
    assert!(fixture.queue.lock().unwrap().is_empty());assert!(!fixture.root.exists());assert!(tile.resident_bytes()>0);
    let document=layer_ui::clipboard_document(images.take_sources(false).unwrap(),Default::default(),startup.localization()).unwrap();
    assert!(document.memory().storage().is_none());
    let session=layer_ui::UiSession::new(Photos(document.composition().color),document,[32,32],layer_ui::Platform::Mac).unwrap();
    let binding=fixture.stores.install_storage(1,&session).unwrap();
    let reply=fixture.stores.checkpoint(1,session.capture_session().unwrap(),session.session_stamp(),true).unwrap();fixture.run();
    assert!(matches!(reply.try_take(),Some(Ok(StoreResult::Checkpoint(_)))));
    assert_eq!(tile.resident_bytes(),0);assert_eq!(&tile.decode_on_worker().unwrap()[..4],&[12,34,56,255]);
    assert!(heads(binding.path()).iter().any(Option::is_some));
}

#[test]
fn imported_payload_ram_plateaus_with_one_decode_ahead_and_reuses_42_durable_images() {
    use std::{future::Future,pin::Pin,task::{Context,Poll,Waker}};
    const MIB:u64=1024*1024;
    let fixture=Fixture::new();
    let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {total_bytes:Some(MIB),available_bytes:Some(MIB),override_bytes:Some(MIB),..Default::default()});
    let limits=layer_color::photo::DecodeLimits::from_memory_budget(layer_color::photo::PhotoMemoryBudget::from_limit(MIB));
    let writer=fixture.stores.asset_writer(1).unwrap();let binding=writer.binding().unwrap();
    let mut images=layer_ui::ImageImportBatch::with_budget(Default::default(),layer_core::color::RgbSpace::ProPhoto,limits,budget.clone()).with_writer(writer);
    let mut tiles=Vec::new();let mut physical=0usize;
    for index in 0..42 {
        pollster::block_on(images.prepare_decode()).unwrap();
        let source=Arc::unwrap_or_clone(layer_core::color::source::rgba8_source([128,128],|x,y| {
            let value=x.wrapping_mul(0x9e3779b9).wrapping_add(y.wrapping_mul(0x85ebca6b)).wrapping_add(index*97);
            [value as u8,(value>>8) as u8,(value>>16) as u8,255]
        }));
        physical+=source.resident_bytes();tiles.extend(source.tiles.values().cloned());
        images.append(format!("Photo {index}"),source,false).unwrap();
        assert!(tiles.iter().filter(|tile|tile.resident_bytes()>0).count()<=2);
        assert!(budget.snapshot().reserved_bytes<MIB/2);
        let mut write=Box::pin(images.write_prepared());let mut context=Context::from_waker(Waker::noop());
        if index!=0 {assert!(write.as_mut().poll(&mut context).is_pending());fixture.run();}
        assert!(matches!(write.as_mut().poll(&mut context),Poll::Ready(Ok(()))));
        assert_eq!(fixture.queue.lock().unwrap().len(),1);
        assert_eq!(tiles.iter().filter(|tile|tile.resident_bytes()>0).count(),1);
    }
    assert!(physical>MIB as usize,"the complete batch exceeds its RAM allowance");
    let mut finish=Box::pin(images.finish_storage());let mut context=Context::from_waker(Waker::noop());
    assert!(finish.as_mut().poll(&mut context).is_pending());fixture.run();
    assert!(matches!(Pin::as_mut(&mut finish).poll(&mut context),Poll::Ready(Ok(()))));drop(finish);
    assert!(tiles.iter().all(|tile|tile.resident_bytes()==0));
    let packed=||std::fs::read_dir(binding.path().join("packs")).unwrap().map(|entry|entry.unwrap().metadata().unwrap().len()).sum::<u64>();
    let before=packed();assert_eq!(before,physical as u64);
    let document=layer_ui::clipboard_document(images.take_sources(false).unwrap(),Default::default(),&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    assert_eq!(document.memory().storage().unwrap().path(),binding.path());
    let color=document.composition().color;
    let session=layer_ui::UiSession::new(Photos(color),document,[256,256],layer_ui::Platform::Gtk).unwrap();
    let reply=fixture.stores.import(1,session.capture_session().unwrap(),session.session_stamp()).unwrap();fixture.run();
    assert!(matches!(reply.try_take(),Some(Ok(StoreResult::Checkpoint(_)))));
    assert_eq!(packed(),before,"checkpoint reuses all imported locations");
    assert_eq!(tiles[0].decode_on_worker().unwrap()[..4],[0,0,0,255]);
}

fn quota_opener(space:Arc<std::sync::atomic::AtomicU64>,quota:Arc<AtomicBool>,creates:Arc<AtomicUsize>)->Arc<dyn StoreOpener> {
    use layer_core::package::{store_io::{FileTransport,FileFuture,FileAccess,StoreFile,DirectoryEntry,NativeTransport},storage_budget::{StorageFacts,StorageBudget,STORAGE_UNAVAILABLE},session_store::{SessionStore,StoreOpener}};
    use std::sync::atomic::AtomicU64;
    struct Disk {inner:NativeTransport,space:Arc<AtomicU64>,quota:Arc<AtomicBool>,creates:Arc<AtomicUsize>,scope:String}
    impl FileTransport for Disk {
        fn storage_facts(&self)->FileFuture<'_,StorageFacts> {Box::pin(async {Ok(StorageFacts {scope:self.scope.clone(),available_bytes:Some(self.space.load(Ordering::SeqCst)),epoch:StorageBudget::process().sample_epoch()})})}
        fn open<'a>(&'a self,name:&'a str,access:FileAccess)->FileFuture<'a,Arc<dyn StoreFile>> {
            if access==FileAccess::Create && (name.starts_with("generations/") || name==".retiring") {
                self.creates.fetch_add(1,Ordering::SeqCst);
                if self.quota.load(Ordering::SeqCst) {return Box::pin(async {Err(STORAGE_UNAVAILABLE.into())});}
            }
            self.inner.open(name,access)
        }
        fn list<'a>(&'a self,name:&'a str)->FileFuture<'a,Vec<DirectoryEntry>> {self.inner.list(name)}
        fn remove<'a>(&'a self,name:&'a str)->FileFuture<'a,layer_core::package::store_io::Removal> {Box::pin(async move {
            let mut removed=self.inner.remove(name).await?;removed.scope=self.scope.clone();Ok(removed)
        })}
    }
    struct Opener(Arc<AtomicU64>,Arc<AtomicBool>,Arc<AtomicUsize>);
    impl StoreOpener for Opener {
        fn storage_facts<'a>(&'a self,path:&'a std::path::Path)->FileFuture<'a,StorageFacts> {Box::pin(async move {
            Ok(StorageFacts {scope:path.parent().unwrap().display().to_string(),available_bytes:Some(self.0.load(Ordering::SeqCst)),epoch:StorageBudget::process().sample_epoch()})
        })}
        fn open<'a>(&'a self,path:&'a std::path::Path,_retirement:bool)->FileFuture<'a,Option<SessionStore>> {Box::pin(async move {
            SessionStore::open_transport(path.into(),Arc::new(Disk {inner:NativeTransport::new(path.into()),space:self.0.clone(),quota:self.1.clone(),creates:self.2.clone(),scope:path.parent().unwrap().display().to_string()}),Arc::new(()),layer_core::tile_io::ReadCache::process()).await.map(Some)
        })}
    }
    Arc::new(Opener(space,quota,creates))
}

#[test]
fn refusal_epoch_preserves_sampling_cleanup_and_ignores_rollback_and_reservations() {
    use layer_core::package::{store_io::FileFuture,session_store::{SessionStore,StoreOpener},storage_budget::{StorageBudget,StorageFacts,StorageAdmission,STORAGE_UNAVAILABLE}};
    use std::{future::Future,sync::atomic::AtomicU64,task::{Context,Poll,Waker}};
    struct Opening {inner:Arc<dyn StoreOpener>,scope:String,available:Mutex<Option<u64>>,full:AtomicBool,hold:AtomicBool,wake:Mutex<Option<Waker>>,rollback:Mutex<Option<DrawingStore>>}
    impl StoreOpener for Opening {
        fn storage_facts<'a>(&'a self,_path:&'a std::path::Path)->FileFuture<'a,StorageFacts> {Box::pin(async {
            let epoch=StorageBudget::process().sample_epoch();
            std::future::poll_fn(|cx| {
                if !self.hold.load(Ordering::SeqCst) {return Poll::Ready(());}
                *self.wake.lock().unwrap()=Some(cx.waker().clone());Poll::Pending
            }).await;
            Ok(StorageFacts {scope:self.scope.clone(),available_bytes:*self.available.lock().unwrap(),epoch})
        })}
        fn open<'a>(&'a self,path:&'a std::path::Path,retirement:bool)->FileFuture<'a,Option<SessionStore>> {Box::pin(async move {
            if !self.full.load(Ordering::SeqCst) {return self.inner.open(path,retirement).await;}
            let rollback=self.rollback.lock().unwrap().take();
            if let Some(rollback)=rollback {rollback.with_retirement_store(async |store|store.retire().await).await?;}
            Err(STORAGE_UNAVAILABLE.into())
        })}
    }
    let mut fixture=Fixture::new();let space=Arc::new(AtomicU64::new(1<<30));let queued=fixture.queue.clone();
    let opener=Arc::new(Opening {inner:quota_opener(space.clone(),Arc::new(AtomicBool::new(false)),Arc::new(AtomicUsize::new(0))),scope:fixture.root.display().to_string(),available:Mutex::new(Some(1<<30)),full:AtomicBool::new(false),hold:AtomicBool::new(false),wake:Default::default(),rollback:Default::default()});
    fixture.stores=SessionStores::with_opener(fixture.root.clone(),opener.clone(),Arc::new(move |work|queued.lock().unwrap().push_back(work)),Arc::new(||{}),||0,fixture.activity.clone());
    for id in 2..=6 {let (saved,stamp,_,_)=capture();let reply=fixture.stores.checkpoint(id,saved,stamp,true).unwrap();fixture.run();assert!(reply.try_take().unwrap().is_ok());}
    *opener.rollback.lock().unwrap()=Some(fixture.stores.bind(2,None).unwrap());fixture.stores.release_binding(2);
    opener.full.store(true,Ordering::SeqCst);let (saved,stamp,_,_)=capture();
    let fail=||{let reply=fixture.stores.checkpoint(1,saved.clone(),stamp.clone(),true).unwrap();fixture.run();assert!(matches!(reply.try_take(),Some(Err(error)) if error==STORAGE_UNAVAILABLE));};
    fail();let binding=fixture.stores.bind(1,None).unwrap();
    let sample=|now|{assert!(!fixture.stores.observe(1,stamp.clone(),now,false).unwrap());fixture.run();fixture.stores.0.state.lock().unwrap().drawings[binding.key()].storage_error.is_none()};
    assert!(StorageBudget::process().removal(&opener.scope).0>0);assert!(!sample(2000),"rollback before refusal cannot grant another attempt");
    *opener.available.lock().unwrap()=Some(2<<30);space.store(2<<30,Ordering::SeqCst);assert!(sample(4000));
    opener.hold.store(true,Ordering::SeqCst);let reply=fixture.stores.checkpoint(1,saved.clone(),stamp.clone(),true).unwrap();
    let mut running=Box::pin(fixture.pop().run_async());let mut context=Context::from_waker(Waker::noop());
    assert!(running.as_mut().poll(&mut context).is_pending());
    let retired=fixture.stores.retire(3).unwrap();fixture.run();assert!(retired.try_take().unwrap().is_ok());
    opener.hold.store(false,Ordering::SeqCst);opener.wake.lock().unwrap().take().unwrap().wake();pollster::block_on(running);
    assert!(reply.try_take().unwrap().is_err());assert!(sample(6000),"cleanup during the facts sample must survive");
    fail();assert!(!sample(8000),"the same cleanup cannot grant another attempt");
    let retired=fixture.stores.retire(4).unwrap();fixture.run();assert!(retired.try_take().unwrap().is_ok());assert!(sample(10000));
    let reservation=match StorageBudget::process().admit(&opener.scope,64*1024*1024,0) {StorageAdmission::Granted(reservation)=>reservation,_=>panic!("expected reservation")};
    fail();drop(reservation);assert!(!sample(12000),"reservation release must not look like raw capacity growth");
    let mut now=14000;
    for (id,available) in [(5,Some(0)),(6,None)] {
        *opener.available.lock().unwrap()=available;assert!(!sample(now));now+=2000;
        let retired=fixture.stores.retire(id).unwrap();fixture.run();assert!(retired.try_take().unwrap().is_ok());
        assert!(sample(now),"cleanup is independent of zero or unknown estimates");now+=2000;
        assert!(!matches!(StorageBudget::process().admit(&opener.scope,1,0),StorageAdmission::Granted(_)));
        fail();assert!(!sample(now),"failed admission consumes cleanup evidence");now+=2000;
    }
    assert!(!binding.path().exists());
}

#[test]
fn failed_store_open_samples_without_creating_until_capacity_or_cleanup_changes() {
    use layer_core::package::{store_io::FileFuture,session_store::{SessionStore,StoreOpener},storage_budget::{StorageFacts,STORAGE_UNAVAILABLE}};
    use std::sync::atomic::AtomicU64;
    struct Opening {inner:Arc<dyn StoreOpener>,full:Arc<AtomicBool>,attempts:Arc<AtomicUsize>}
    impl StoreOpener for Opening {
        fn storage_facts<'a>(&'a self,path:&'a std::path::Path)->FileFuture<'a,StorageFacts> {self.inner.storage_facts(path)}
        fn open<'a>(&'a self,path:&'a std::path::Path,retirement:bool)->FileFuture<'a,Option<SessionStore>> {
            self.attempts.fetch_add(1,Ordering::SeqCst);
            if self.full.load(Ordering::SeqCst) {Box::pin(async {Err(STORAGE_UNAVAILABLE.into())})}else {self.inner.open(path,retirement)}
        }
    }
    let mut fixture=Fixture::new();let space=Arc::new(AtomicU64::new(1<<30));let full=Arc::new(AtomicBool::new(false));
    let creates=Arc::new(AtomicUsize::new(0));let attempts=Arc::new(AtomicUsize::new(0));let queued=fixture.queue.clone();
    fixture.stores=SessionStores::with_opener(fixture.root.clone(),Arc::new(Opening {inner:quota_opener(space.clone(),Arc::new(AtomicBool::new(false)),creates.clone()),full:full.clone(),attempts:attempts.clone()}),
        Arc::new(move |work|queued.lock().unwrap().push_back(work)),Arc::new(||{}),||0,fixture.activity.clone());
    for id in [2,3] {let (saved,stamp,_,_)=capture();let reply=fixture.stores.checkpoint(id,saved,stamp,true).unwrap();fixture.run();assert!(reply.try_take().unwrap().is_ok());}
    full.store(true,Ordering::SeqCst);let (saved,stamp,_,tile)=capture();
    let write=||fixture.stores.checkpoint(1,saved.clone(),stamp.clone(),true).unwrap();
    let first=write();fixture.run();assert!(matches!(first.try_take(),Some(Err(error)) if error==STORAGE_UNAVAILABLE));
    let binding=fixture.stores.bind(1,None).unwrap();let initial=attempts.load(Ordering::SeqCst);let initial_creates=creates.load(Ordering::SeqCst);
    for tick in 1..20 {assert!(!fixture.stores.observe(1,stamp.clone(),tick*2000,false).unwrap());fixture.run();}
    assert_eq!(attempts.load(Ordering::SeqCst),initial);assert_eq!(creates.load(Ordering::SeqCst),initial_creates);assert!(!binding.path().exists());
    space.store(2<<30,Ordering::SeqCst);assert!(!fixture.stores.observe(1,stamp.clone(),40000,false).unwrap());fixture.run();
    let retry=write();fixture.run();assert!(retry.try_take().unwrap().is_err());assert_eq!(attempts.load(Ordering::SeqCst),initial+1);
    for tick in 21..30 {assert!(!fixture.stores.observe(1,stamp.clone(),tick*2000,false).unwrap());fixture.run();}
    assert_eq!(attempts.load(Ordering::SeqCst),initial+1);assert!(!binding.path().exists());
    let retired=fixture.stores.retire(2).unwrap();fixture.run();assert!(retired.try_take().unwrap().is_ok());
    assert!(!fixture.stores.observe(1,stamp.clone(),60000,false).unwrap());fixture.run();
    let retry=write();fixture.run();assert!(retry.try_take().unwrap().is_err());assert_eq!(attempts.load(Ordering::SeqCst),initial+2);
    full.store(false,Ordering::SeqCst);assert!(!fixture.stores.observe(1,stamp.clone(),62000,false).unwrap());fixture.run();
    assert_eq!(attempts.load(Ordering::SeqCst),initial+2);assert_eq!(creates.load(Ordering::SeqCst),initial_creates);assert!(!binding.path().exists());
    let retired=fixture.stores.retire(3).unwrap();fixture.run();assert!(retired.try_take().unwrap().is_ok());
    assert!(!fixture.stores.observe(1,stamp.clone(),64000,false).unwrap());fixture.run();
    let retry=write();fixture.run();assert!(matches!(retry.try_take(),Some(Ok(StoreResult::Checkpoint(_)))));
    assert_eq!(attempts.load(Ordering::SeqCst),initial+3);assert_eq!(creates.load(Ordering::SeqCst),initial_creates+1);
    assert!(fixture.stores.is_current(1,&stamp));assert_eq!(tile.resident_bytes(),0);
}

#[test]
fn disk_refusal_preserves_each_write_kind_and_waits_for_changed_space_without_retrying_frames() {
    use layer_core::package::storage_budget::STORAGE_UNAVAILABLE;
    use std::sync::atomic::AtomicU64;
    for kind in ["import","pressure","checkpoint","legacy-import","prepare-retirement"] {
        let constant=kind=="legacy-import" || kind=="prepare-retirement";
        let mut fixture=Fixture::new();let space=Arc::new(AtomicU64::new(1<<30));let quota=Arc::new(AtomicBool::new(false));let creates=Arc::new(AtomicUsize::new(0));let queued=fixture.queue.clone();let wakes=fixture.wakes.clone();
        fixture.stores=SessionStores::with_opener(fixture.root.clone(),quota_opener(space.clone(),quota.clone(),creates.clone()),Arc::new(move |work|queued.lock().unwrap().push_back(work)),Arc::new(move ||{wakes.fetch_add(1,Ordering::SeqCst);}),||0,fixture.activity.clone());
        let (saved,stamp,_,_)=capture();let committed_stamp=stamp.clone();let baseline=fixture.stores.checkpoint(1,saved,stamp,true).unwrap();fixture.run();assert!(baseline.try_take().unwrap().is_ok());
        let binding=fixture.stores.bind(1,None).unwrap();let before=heads(binding.path());
        if constant {for id in [2,3] {
            let (saved,stamp,_,_)=capture();let reply=fixture.stores.checkpoint(id,saved,stamp,true).unwrap();fixture.run();assert!(reply.try_take().unwrap().is_ok());
        }}
        let legacy=if kind=="legacy-import" {
            let generation=std::fs::read_dir(binding.path().join("generations")).unwrap().next().unwrap().unwrap().path();
            let packed:serde_json::Value=serde_json::from_slice(&std::fs::read(&generation).unwrap()).unwrap();
            let metadata=packed["session"].clone();let mut resources=BTreeMap::new();
            for record in metadata["resources"].as_array().unwrap() {
                let pack=std::fs::read(binding.path().join(record["location"]["pack"].as_str().unwrap())).unwrap();
                let offset=record["location"]["offset"].as_str().unwrap().parse::<usize>().unwrap();
                let length=record["bytes"].as_str().unwrap().parse::<usize>().unwrap();
                let bytes:Arc<[u8]>=pack[offset..offset+length].into();
                resources.insert(record["id"].as_str().unwrap().parse().unwrap(),layer_core::package::ImmutableBacking::new(Arc::new(bytes)).unwrap());
            }
            let logical=fixture.stores.import_legacy(binding.key(),Some(PortableId::random()),serde_json::to_vec(&metadata).unwrap().into(),resources.clone()).unwrap();fixture.run();
            assert!(matches!(logical.try_take(),Some(Ok(StoreResult::Imported(Err(layer_core::package::session_store::ImportFailure::Changed))))));
            Some((generation.file_stem().unwrap().to_str().unwrap().parse::<PortableId>().unwrap(),Arc::<[u8]>::from(serde_json::to_vec(&metadata).unwrap()),resources))
        }else {None};
        let (saved,stamp,retained,tile)=capture();let bytes=tile.resident_bytes();
        if constant {quota.store(true,Ordering::SeqCst);}else {space.store(0,Ordering::SeqCst);}
        let reply=match kind {"prepare-retirement"=>fixture.stores.prepare_retirement(binding.key()),"legacy-import"=>{let (expected,metadata,resources)=legacy.as_ref().unwrap();fixture.stores.import_legacy(binding.key(),Some(*expected),metadata.clone(),resources.clone())},"import"=>fixture.stores.import(1,saved.clone(),stamp.clone()),"pressure"=>fixture.stores.pressure(1,retained),_=>fixture.stores.checkpoint(1,saved.clone(),stamp.clone(),false)}.unwrap();
        fixture.run();match reply.try_take().unwrap() {
            Err(error)|Ok(StoreResult::Imported(Err(layer_core::package::session_store::ImportFailure::Io(error))))=>assert_eq!(error,STORAGE_UNAVAILABLE),_=>panic!("write must fail for unavailable storage"),
        }
        assert_eq!(heads(binding.path()),before);assert_eq!(tile.resident_bytes(),bytes);
        assert_eq!(fixture.stores.checkpoint_due(1,stamp.clone(),true).unwrap_err(),STORAGE_UNAVAILABLE);
        assert!(!fixture.stores.checkpoint_due(1,committed_stamp,true).unwrap());
        assert!(fixture.queue.lock().unwrap().is_empty());
        let attempts=creates.load(Ordering::SeqCst);
        let wakes=fixture.wakes.load(Ordering::SeqCst);
        for now in 0..1000 {assert!(!fixture.stores.observe(1,stamp.clone(),now,false).unwrap());}
        assert!(fixture.queue.lock().unwrap().is_empty());assert_eq!(fixture.wakes.load(Ordering::SeqCst),wakes);
        assert!(!fixture.stores.observe(1,stamp.clone(),2000,false).unwrap());fixture.run();
        assert!(fixture.stores.0.state.lock().unwrap().drawings[binding.key()].storage_error.is_some());
        assert_eq!(fixture.wakes.load(Ordering::SeqCst),wakes,"unchanged full-disk facts do not wake a frame");
        if constant {
            let retired=fixture.stores.retire(2).unwrap();fixture.run();assert!(retired.try_take().unwrap().is_ok());
            assert!(!fixture.stores.observe(1,stamp.clone(),4000,false).unwrap());fixture.run();
            assert!(fixture.stores.observe(1,stamp.clone(),4001,false).unwrap());
            let retry=if let Some((expected,metadata,resources))=&legacy {fixture.stores.import_legacy(binding.key(),Some(*expected),metadata.clone(),resources.clone()).unwrap()}
                else {fixture.stores.prepare_retirement(binding.key()).unwrap()};fixture.run();
            assert!(matches!(retry.try_take(),Some(Err(error)) if error==STORAGE_UNAVAILABLE));
            assert_eq!(creates.load(Ordering::SeqCst),attempts+1);assert_eq!(heads(binding.path()),before);
            for tick in 3..20 {
                assert!(!fixture.stores.observe(1,stamp.clone(),tick*2000,false).unwrap());fixture.run();
                assert!(fixture.stores.0.state.lock().unwrap().drawings[binding.key()].storage_error.is_some());
            }
            assert_eq!(creates.load(Ordering::SeqCst),attempts+1,"the same removal permits only one write attempt");
            quota.store(false,Ordering::SeqCst);
            let retired=fixture.stores.retire(3).unwrap();fixture.run();assert!(retired.try_take().unwrap().is_ok());
        }else {space.store(1<<30,Ordering::SeqCst);}
        assert!(!fixture.stores.observe(1,stamp.clone(),40000,false).unwrap());fixture.run();
        assert!(fixture.stores.observe(1,stamp.clone(),40001,false).unwrap());
        let retry=if let Some((expected,metadata,resources))=legacy {fixture.stores.import_legacy(binding.key(),Some(expected),metadata,resources).unwrap()}
            else if kind=="prepare-retirement" {fixture.stores.prepare_retirement(binding.key()).unwrap()}
            else {fixture.stores.checkpoint(1,saved,stamp,true).unwrap()};
        fixture.run();assert!(matches!(retry.try_take(),Some(Ok(StoreResult::Checkpoint(_)|StoreResult::Imported(Ok(_))|StoreResult::RetirementPrepared))));
        if kind=="prepare-retirement" {assert_eq!(heads(binding.path()),before);assert!(binding.path().join(".retiring").is_file());}
        else {if kind!="legacy-import" {assert_eq!(tile.resident_bytes(),0);}assert_ne!(heads(binding.path()),before);}
    }
}

#[test]
fn compaction_yields_its_queue_position_to_restore_pressure_and_checkpoint() {
    let (capture,stamp,_,_)=capture();let mut pending=Pending::default();
    let request=|work|Request {work,replies:Vec::new()};
    assert!(pending.push(request(Work::Compact)).is_empty());
    assert!(pending.push(request(Work::Checkpoint {capture,stamp,forced:false,prepare:None})).is_empty());
    assert!(pending.push(request(Work::Pressure(Vec::new()))).is_empty());
    assert!(pending.push(request(Work::Restore(ProjectLimits::default()))).is_empty());
    assert!(matches!(pending.take().unwrap().work,Work::Restore(_)));
    assert!(matches!(pending.take().unwrap().work,Work::Pressure(_)));
    assert!(matches!(pending.take().unwrap().work,Work::Checkpoint {..}));
    assert!(matches!(pending.take().unwrap().work,Work::Compact));assert!(pending.take().is_none());
    pending.push(request(Work::Compact));
    assert!(matches!(pending.push(request(Work::Retire)).pop().unwrap().work,Work::Compact));
    assert!(matches!(pending.take().unwrap().work,Work::Retire));assert!(pending.take().is_none());
}

fn package_session(fixture:&Fixture)->layer_ui::UiSession<Photos> {
    use layer_core::package::{codec::{PreparedPackage,OpenOutcome},transport};
    use std::future::Future;
    let (source,_,_,_)=capture();let cancel=AtomicBool::new(false);
    let prepared=PreparedPackage::prepare(source.artwork(),None,&cancel).unwrap();
    let mut bytes=Vec::new();prepared.write(&mut bytes,&cancel).unwrap();
    let backing=transport::spool(&mut std::io::Cursor::new(bytes),&std::env::temp_dir(),u64::MAX,&cancel).unwrap();
    let OpenOutcome::Candidate {artwork,memory,..}=layer_core::package::codec::open(backing,ProjectLimits::default(),&cancel).unwrap() else {panic!("editable package")};
    let mut document=layer_core::Document::from_artwork(artwork).unwrap().with_memory(memory);
    let writer=fixture.stores.asset_writer(1).unwrap();
    let mut preparing=Box::pin(writer.prepare_document(&mut document));
    assert!(matches!(preparing.as_mut().poll(&mut std::task::Context::from_waker(Waker::noop())),Poll::Ready(Ok(()))));drop(preparing);
    assert!(fixture.queue.lock().unwrap().is_empty());assert!(!fixture.root.exists());
    let session=layer_ui::UiSession::new(Photos(document.composition().color),document,[256;2],layer_ui::Platform::Gtk).unwrap();
    fixture.stores.install_storage(1,&session).unwrap();session
}

fn queue_package_copy(fixture:&Fixture,stamp:&SessionStamp) {
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
    while fixture.queue.lock().unwrap().is_empty() && std::time::Instant::now()<deadline {
        assert!(!fixture.stores.checkpoint_due(1,stamp.clone(),false).unwrap());
        if fixture.queue.lock().unwrap().is_empty() {std::thread::sleep(std::time::Duration::from_millis(10));}
    }
    assert_eq!(fixture.queue.lock().unwrap().len(),1);
}

#[test]
fn package_open_does_not_queue_a_copy_or_checkpoint_before_the_first_picture() {
    let fixture=Fixture::new();let session=package_session(&fixture);
    assert!(session.engine().document().memory().package_pending());
    assert!(session.engine().document().memory().package_inventory().is_none());
    assert!(!fixture.stores.checkpoint_due(1,session.session_stamp(),false).unwrap());
    assert!(!fixture.stores.copy_package(fixture.stores.bind(1,None).unwrap().key()));
    assert!(fixture.queue.lock().unwrap().is_empty());assert!(!fixture.root.exists());
}

#[test]
fn save_over_waits_for_existing_package_copy_without_publishing_a_head() {
    let fixture=Fixture::new();let session=package_session(&fixture);let memory=session.engine().document().memory();memory.presented();
    let binding=fixture.stores.bind(1,None).unwrap();let before=heads(binding.path());
    let reply=fixture.stores.finish_package(1).unwrap();
    assert!(reply.try_take().is_none());assert!(memory.package_pending());assert_eq!(heads(binding.path()),before);
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
    while memory.package_pending() && std::time::Instant::now()<deadline {
        fixture.stores.maintain().unwrap();
        if !fixture.queue.lock().unwrap().is_empty() {fixture.run();}else{std::thread::yield_now();}
    }
    assert!(!memory.package_pending());assert!(matches!(reply.try_take(),Some(Ok(StoreResult::Durable(_)))));
    assert_eq!(heads(binding.path()),before);
    let inventory=session.retained_document_tiles();
    for tile in pollster::block_on(inventory.await_blobs()).unwrap() {assert!(tile.memory_compressed().unwrap().is_none());assert!(!pollster::block_on(tile.read_compressed()).unwrap().is_empty());}
}

#[test]
fn save_over_copy_failure_keeps_package_owners_and_the_head_and_does_not_retry() {
    use layer_core::package::{session_store::{SessionStore,StoreOpener,NativeStoreOpener},store_io::FileFuture,storage_budget::STORAGE_UNAVAILABLE};
    struct Opening(&'static str);
    impl StoreOpener for Opening {
        fn storage_facts<'a>(&'a self,path:&'a std::path::Path)->FileFuture<'a,layer_core::package::storage_budget::StorageFacts> {NativeStoreOpener.storage_facts(path)}
        fn open<'a>(&'a self,_:&'a std::path::Path,_:bool)->FileFuture<'a,Option<SessionStore>> {Box::pin(async {Err(self.0.into())})}
    }
    for error in [STORAGE_UNAVAILABLE,"Storage IO failed"] {
        let mut fixture=Fixture::new();let queue=fixture.queue.clone();
        fixture.stores=SessionStores::with_opener(fixture.root.clone(),Arc::new(Opening(error)),Arc::new(move |work|queue.lock().unwrap().push_back(work)),Arc::new(||{}),||0,fixture.activity.clone());
        let session=package_session(&fixture);let memory=session.engine().document().memory();memory.presented();
        let binding=fixture.stores.bind(1,None).unwrap();let before=heads(binding.path());let reply=fixture.stores.finish_package(1).unwrap();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
        while fixture.queue.lock().unwrap().is_empty() && std::time::Instant::now()<deadline {fixture.stores.maintain().unwrap();std::thread::yield_now();}
        fixture.run();assert!(matches!(reply.try_take(),Some(Err(reason)) if reason==error));
        assert_eq!(heads(binding.path()),before);assert!(fixture.queue.lock().unwrap().is_empty());
        assert!(matches!(fixture.stores.finish_package(1),Err(reason) if reason==error));
        for tile in pollster::block_on(session.retained_document_tiles().await_blobs()).unwrap() {assert!(!pollster::block_on(tile.read_compressed()).unwrap().is_empty());}
    }
}

#[test]
fn save_over_waiter_fails_when_the_existing_copy_worker_is_dropped() {
    let fixture=Fixture::new();let session=package_session(&fixture);session.engine().document().memory().presented();
    let reply=fixture.stores.finish_package(1).unwrap();
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
    while fixture.queue.lock().unwrap().is_empty() && std::time::Instant::now()<deadline {fixture.stores.maintain().unwrap();std::thread::yield_now();}
    drop(fixture.pop());assert!(matches!(reply.try_take(),Some(Err(error)) if error=="Drawing storage worker stopped"));
}

#[test]
fn continuous_input_cannot_starve_package_durability_before_a_host_captures() {
    use layer_core::package::session_store::CompactionActivity;
    use std::sync::atomic::AtomicU64;
    static NOW:AtomicU64=AtomicU64::new(0);
    fn now()->u64 {NOW.load(Ordering::SeqCst)}
    NOW.store(0,Ordering::SeqCst);
    let mut fixture=Fixture::new();let queue=fixture.queue.clone();
    fixture.stores=SessionStores::with_opener(fixture.root.clone(),Arc::new(layer_core::package::session_store::NativeStoreOpener),
        Arc::new(move |work|queue.lock().unwrap().push_back(work)),Arc::new(||{}),now,fixture.activity.clone());
    let session=package_session(&fixture);let memory=session.engine().document().memory();memory.presented();
    let binding=fixture.stores.bind(1,None).unwrap();let bound=5*SessionStores::checkpoint_interval_ms();
    for elapsed in (0..=bound).step_by(1000) {
        NOW.store(elapsed,Ordering::SeqCst);fixture.activity.touch();
        let mut stamp=session.session_stamp();stamp.state.unsaved_name=Some(elapsed.to_string());
        assert!(!fixture.stores.checkpoint_due(1,stamp,false).unwrap());
        assert!(fixture.queue.lock().unwrap().is_empty());
        assert!(fixture.stores.0.state.lock().unwrap().drawings[binding.key()].pending.checkpoint.is_none());
    }
    struct OtherInput {
        stop: std::sync::mpsc::Sender<()>,
        worker: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for OtherInput {
        fn drop(&mut self) {
            let _ = self.stop.send(());
            if let Some(worker) = self.worker.take() { let _ = worker.join(); }
        }
    }
    let (stop, stopped) = std::sync::mpsc::channel();
    let _other_input = OtherInput {
        stop,
        worker: Some(std::thread::spawn(move || {
            while matches!(stopped.recv_timeout(std::time::Duration::from_millis(20)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)) {
                CompactionActivity::interrupt();
            }
        })),
    };
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
    while memory.package_pending() && std::time::Instant::now()<deadline {
        assert!(!fixture.stores.checkpoint_due(1,session.session_stamp(),false).unwrap());
        if fixture.queue.lock().unwrap().is_empty() {std::thread::sleep(std::time::Duration::from_millis(10));}else {fixture.run();}
    }
    assert!(fixture.queue.lock().unwrap().is_empty());assert!(!memory.package_pending());
    assert!(fixture.stores.0.state.lock().unwrap().drawings[binding.key()].deferred_since.is_none());
    let stamp=session.session_stamp();assert!(fixture.stores.checkpoint_due(1,stamp.clone(),false).unwrap());
    let reply=fixture.stores.checkpoint(1,session.capture_session().unwrap(),stamp.clone(),true).unwrap();fixture.run();
    assert!(matches!(reply.try_take(),Some(Ok(StoreResult::Checkpoint(_)))));assert!(fixture.stores.is_current(1,&stamp));
}

#[test]
fn forced_checkpoint_is_due_immediately_while_a_package_copy_is_pending() {
    use std::sync::atomic::AtomicU64;
    static NOW:AtomicU64=AtomicU64::new(0);
    fn now()->u64 {NOW.load(Ordering::SeqCst)}
    NOW.store(0,Ordering::SeqCst);
    let mut fixture=Fixture::new();let queue=fixture.queue.clone();
    fixture.stores=SessionStores::with_opener(fixture.root.clone(),Arc::new(layer_core::package::session_store::NativeStoreOpener),
        Arc::new(move |work|queue.lock().unwrap().push_back(work)),Arc::new(||{}),now,fixture.activity.clone());
    let session=package_session(&fixture);session.engine().document().memory().presented();
    fixture.activity.touch();assert!(!fixture.stores.checkpoint_due(1,session.session_stamp(),false).unwrap());
    NOW.store(5*SessionStores::checkpoint_interval_ms(),Ordering::SeqCst);
    queue_package_copy(&fixture,&session.session_stamp());
    assert!(session.engine().document().memory().package_pending());
    assert!(fixture.stores.checkpoint_due(1,session.session_stamp(),true).unwrap());
    let reply=fixture.stores.checkpoint(1,session.capture_session().unwrap(),session.session_stamp(),true).unwrap();
    fixture.run();fixture.run();assert!(matches!(reply.try_take(),Some(Ok(StoreResult::Checkpoint(_)))));
    assert!(!session.engine().document().memory().package_pending());
}
