use super::*;
use std::{collections::VecDeque, sync::atomic::{AtomicUsize, Ordering}};

struct Fixture {
    stores: SessionStores,
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
        let stores = SessionStores::new(root.clone(), Arc::new(move |work| queued.lock().unwrap().push_back(work)),
            Arc::new(move || { wake.fetch_add(1, Ordering::SeqCst); }));
        Self {stores, queue, wakes, root}
    }
    fn pop(&self) -> StoreWork { self.queue.lock().unwrap().pop_front().unwrap() }
    fn run(&self) { self.pop().run(); }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let pending = std::mem::take(&mut *self.queue.lock().unwrap()); drop(pending);
        { let mut state = self.stores.0.state.lock().unwrap(); state.drawings.clear(); state.aliases.clear(); }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn capture() -> (SessionCapture, SessionStamp, RetainedTiles, Arc<layer_core::raster::TileBlob>) {
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
fn named(capture: &SessionCapture, stamp: &SessionStamp, name: &str) -> (SessionCapture, SessionStamp) {
    let mut capture = capture.clone(); capture.state.unsaved_name = Some(name.into());
    let mut stamp = stamp.clone(); stamp.state = capture.state.clone();
    (capture, stamp)
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
        let good_head=std::fs::read(good.path().join("head.json")).unwrap();
        let (saved,stamp,_,_)=capture();let bad=fixture.stores.bind(2,Some("failed")).unwrap();
        let reply=fixture.stores.checkpoint(2,saved,stamp,true).unwrap();fixture.run();assert!(reply.try_take().unwrap().is_ok());
        for generation in std::fs::read_dir(bad.path().join("generations")).unwrap() {std::fs::write(generation.unwrap().path(),b"broken checkpoint").unwrap();}
        let restore=fixture.stores.restore(2,ProjectLimits::default()).unwrap();fixture.run();assert!(restore.try_take().unwrap().is_err());
        if registered {
            assert!(layer_core::package::session_store::SessionStore::try_open(bad.path()).unwrap().is_none());
        }else {let mut state=fixture.stores.0.state.lock().unwrap();state.aliases.remove(&2);state.drawings.remove(bad.key());}
        let (key,path)=(bad.key().to_owned(),bad.path().to_path_buf());drop(bad);
        let prepare=fixture.stores.prepare_retirement(&key).unwrap();fixture.run();
        assert!(matches!(prepare.try_take(),Some(Ok(StoreResult::RetirementPrepared))));
        assert!(path.join("head.json").exists());
        let manifest=layer_ui::SessionManifest::default().stage(vec![layer_ui::session_recovery::SessionDrawing {id:1,key:good.key().into()},layer_ui::session_recovery::SessionDrawing {id:2,key:key.clone()}],1).unwrap();
        manifest.remove(2).unwrap().publish(&fixture.root.join("session.json")).unwrap();
        let retire=fixture.stores.retire_copy(&key).unwrap();fixture.run();
        assert!(matches!(retire.try_take(),Some(Ok(StoreResult::Retired))));
        assert!(!path.join("head.json").exists());
        assert_eq!(std::fs::read(good.path().join("head.json")).unwrap(),good_head);
        let restored=fixture.stores.restore(1,ProjectLimits::default()).unwrap();fixture.run();assert!(restored.try_take().unwrap().is_ok());
    }
}

#[test]
fn explicit_discard_prepares_a_corrupt_head_without_decoding_or_changing_another_copy() {
    use layer_core::package::session_store::{SessionStore, collect_unreferenced_stores};
    let fixture=Fixture::new();let cancel=AtomicBool::new(false);
    let (saved,stamp,_,_)=capture();let good=fixture.stores.bind(2,Some("good")).unwrap();
    let reply=fixture.stores.checkpoint(2,saved,stamp,true).unwrap();fixture.run();assert!(reply.try_take().unwrap().is_ok());
    let good_head=std::fs::read(good.path().join("head.json")).unwrap();
    let (saved,stamp,_,_)=capture();let bad=fixture.stores.bind(1,Some("corrupt")).unwrap();
    let reply=fixture.stores.checkpoint(1,saved,stamp,true).unwrap();fixture.run();assert!(reply.try_take().unwrap().is_ok());
    assert!(SessionStore::try_open(bad.path()).unwrap().is_none());
    let path=bad.path().to_path_buf();drop(bad);
    {let mut state=fixture.stores.0.state.lock().unwrap();state.aliases.remove(&1);state.drawings.remove("corrupt");}
    std::fs::write(path.join("head.json"),b"invalid head").unwrap();
    assert!(SessionStore::open(&path).is_err());
    let reply=fixture.stores.prepare_retirement("corrupt").unwrap();fixture.run();
    assert!(matches!(reply.try_take(),Some(Ok(StoreResult::RetirementPrepared))));
    collect_unreferenced_stores(&fixture.root,&["corrupt".into()].into(),&cancel).unwrap();
    assert_eq!(std::fs::read(path.join("head.json")).unwrap(),b"invalid head");
    assert_eq!(std::fs::read_dir(path.join("generations")).unwrap().count(),1);
    let manifest=layer_ui::SessionManifest::default().stage(vec![layer_ui::session_recovery::SessionDrawing {id:1,key:"corrupt".into()}],1).unwrap();
    manifest.remove(1).unwrap().publish(&fixture.root.join("session.json")).unwrap();
    let reply=fixture.stores.retire_copy("corrupt").unwrap();fixture.run();
    assert!(matches!(reply.try_take(),Some(Ok(StoreResult::Retired))));
    assert!(!path.join("head.json").exists());assert!(path.join(".retired").is_file());
    for directory in ["resources","generations","packs"] {assert!(!path.join(directory).exists());}
    assert!(SessionStore::open(&path).is_err());
    assert_eq!(std::fs::read(good.path().join("head.json")).unwrap(),good_head);
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
    assert!(matches!(prepared.try_take(),Some(Err(error)) if error=="Invalid drawing session directory"));
    let retired=fixture.stores.retire_copy("unrecognized").unwrap();fixture.run();
    assert!(matches!(retired.try_take(),Some(Err(error)) if error=="Invalid drawing session directory"));
    assert_eq!(std::fs::read_dir(&unknown).unwrap().count(),1);
    assert_eq!(std::fs::read(unknown.join("keep")).unwrap(),b"unrecognized bytes");
    let locked=fixture.root.join("locked");let owner=SessionStore::open(&locked).unwrap();
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
            drop(SessionStore::open(&path).unwrap());let link=path.join(name);
            if link.is_dir() {std::fs::remove_dir(&link).unwrap();}else if link.exists() {std::fs::remove_file(&link).unwrap();}
            let target=if ["resources","generations","packs"].contains(&name) {outside.root.clone()}else {outside.root.join("keep")};
            symlink(target,link).unwrap();
        }
        let prepared=fixture.stores.prepare_retirement(&key).unwrap();fixture.run();assert!(prepared.try_take().unwrap().is_err());
        let retired=fixture.stores.retire_copy(&key).unwrap();fixture.run();assert!(retired.try_take().unwrap().is_err());
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
        collect_unreferenced_stores(&fixture.root,&Default::default(),&AtomicBool::new(false)).unwrap();
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
    binding.on_worker(|store| store.collect_garbage()).unwrap();
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
        binding.on_worker(|store| capture.checkpoint(store, &AtomicBool::new(false))).unwrap();
        let head = std::fs::read(binding.path().join("head.json")).unwrap();
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
        assert_eq!(fixture.stores.observe(1,adopted,SessionStores::now_ms()+SessionStores::checkpoint_interval_ms(),false).unwrap(),edit_after_restore!=0);
        assert_eq!(std::fs::read(binding.path().join("head.json")).unwrap(), head);
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
    let name = binding.on_worker(|store| Ok(store.load(ProjectLimits::default(), &AtomicBool::new(false))
        .unwrap().unwrap().metadata.value["unsaved_name"].as_str().unwrap().to_owned())).unwrap();
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
    let name = binding.on_worker(|store| Ok(store.load(ProjectLimits::default(), &AtomicBool::new(false))
        .unwrap().unwrap().metadata.value["unsaved_name"].as_str().unwrap().to_owned())).unwrap();
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
    fixture.stores.0.state.lock().unwrap().drawings.get_mut(binding.key()).unwrap().due = Some(SessionStores::now_ms());
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
    documents.append((), retained, &localization);
    assert!(tile.resident_bytes() > 0);
    assert!(!fixture.queue.lock().unwrap().is_empty());
    while !fixture.queue.lock().unwrap().is_empty() { fixture.run(); }
    assert_eq!(tile.resident_bytes(), 0);
    assert_eq!(fixture.stores.bindings().len(), 1);
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
impl std::task::Wake for ReplyWake {
    fn wake(self: Arc<Self>) {self.0.fetch_add(1, Ordering::SeqCst);}
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
