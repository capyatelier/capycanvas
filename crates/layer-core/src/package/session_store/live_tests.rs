use super::*;
use crate::{Document, DocumentNames, Editor, Edit, raster::*, raster_storage::RetainedTiles};
use std::sync::mpsc;

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
fn prepared(editor: &Editor) -> PreparedSession {
    let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
    PreparedSession::prepare(&capture, serde_json::json!({}), &AtomicBool::new(false)).unwrap()
}
fn tiles(blob: Arc<TileBlob>) -> RetainedTiles { RetainedTiles {sources: vec![blob], ..Default::default()} }
fn pack_lengths(store: &SessionStore) -> BTreeMap<PathBuf, u64> {
    fs::read_dir(store.path.join("packs")).unwrap().map(|entry| {
        let entry = entry.unwrap(); (entry.path(), entry.metadata().unwrap().len())
    }).collect()
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
    let mut store = SessionStore::open(&directory.0).unwrap();
    assert!(store.make_durable(&retained, &cancel).unwrap() > 0);
    let pressure_packs = pack_lengths(&store);
    assert!(!pressure_packs.is_empty());
    for path in pressure_packs.keys() { OpenOptions::new().write(true).open(path).unwrap().set_len(PACK_BYTES).unwrap(); }
    store.commit(&prepared(&document_editor(blob(141))), &cancel).unwrap();
    store.collect_garbage().unwrap();
    for path in pressure_packs.keys() { assert!(path.exists()); }
    assert_eq!(first.decode_on_worker().unwrap(), expected);
    drop((first, retained, editor));
    store.collect_garbage().unwrap();
    for path in pressure_packs.keys() { assert!(path.exists(), "capture retains history locations"); }
    drop(capture);
    store.collect_garbage().unwrap();
    for path in pressure_packs.keys() { assert!(!path.exists(), "unreferenced pressure pack is collectible"); }
}

#[test]
fn reopening_collects_unheaded_pressure_packs_without_live_roots() {
    let directory = Directory::new(); let cancel = AtomicBool::new(false);
    let mut store = SessionStore::open(&directory.0).unwrap();
    let retained = tiles(blob(43));
    store.make_durable(&retained, &cancel).unwrap();
    let packs = pack_lengths(&store); assert!(!packs.is_empty());
    drop((retained, store));
    let store = SessionStore::open(&directory.0).unwrap();
    assert!(store.generation().is_none());
    assert!(pack_lengths(&store).is_empty());
}

#[test]
fn pressure_then_checkpoint_keeps_one_physical_tile_copy() {
    let directory = Directory::new(); let cancel = AtomicBool::new(false);
    let tile = blob(57); let editor = document_editor(tile.clone());
    let retained = editor.retained_tiles();
    let mut store = SessionStore::open(&directory.0).unwrap();
    assert_eq!(store.make_durable(&retained, &cancel).unwrap(), tile.compressed_len());
    let before = pack_lengths(&store);
    store.commit(&prepared(&editor), &cancel).unwrap();
    assert_eq!(pack_lengths(&store), before);
    assert_eq!(store.make_durable(&retained, &cancel).unwrap(), 0);
}

#[test]
fn pressure_alias_only_checkpoint_reuses_the_shared_durable_location() {
    let directory = Directory::new(); let cancel = AtomicBool::new(false);
    let original = blob(73);
    let alias = Arc::new(original.alias(PortableId::random()));
    assert_ne!(original.resource_id(), alias.resource_id());
    let retained = RetainedTiles {sources: vec![original.clone(), alias.clone()], ..Default::default()};
    let mut store = SessionStore::open(&directory.0).unwrap();
    assert_eq!(store.make_durable(&retained, &cancel).unwrap(), original.compressed_len());
    let before = pack_lengths(&store);
    let editor = document_editor(alias.clone());
    let checkpoint = prepared(&editor);
    let index = resource_index(checkpoint.metadata(), ProjectLimits::default()).unwrap_or_else(|error| panic!("{}", error.message()));
    assert!(index.records.iter().any(|record| record.id == alias.resource_id()));
    assert!(index.records.iter().all(|record| record.id != original.resource_id()));
    store.commit(&checkpoint, &cancel).unwrap();
    assert_eq!(pack_lengths(&store), before);
    drop((checkpoint, editor, retained, alias, original, store));
    let mut store = SessionStore::open(&directory.0).unwrap();
    let opened = store.load(ProjectLimits::default(), &cancel).unwrap().unwrap();
    let blobs = opened.editor.retained_tiles().blobs().unwrap();
    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs[0].decode_on_worker().unwrap(), vec![73; 256 * 256 * 4]);
    assert_eq!(pack_lengths(&store), before);
}

struct FailedAppend { sync: bool }
impl Publication for FailedAppend {
    fn publish(&mut self, _: &Path, _: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>,
        _: impl FnOnce() -> Result<(), String>) -> Result<(), AtomicReplaceError> { panic!("existing live pack must be appended") }
    fn append(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        if self.sync { append_using(path, write, ready, |_| Err("Injected data sync failure".into())) }
        else {
            let mut file = OpenOptions::new().append(true).open(path).unwrap();
            file.write_all(b"partial pressure write").unwrap();
            Err(std::io::Error::from(std::io::ErrorKind::StorageFull).to_string())
        }
    }
}
#[test]
fn failed_pressure_write_or_sync_keeps_original_ram_and_last_good_head() {
    for sync in [false, true] {
        let directory = Directory::new(); let cancel = AtomicBool::new(false);
        let seed = document_editor(blob(63)); let retained = seed.retained_tiles();
        let mut store = SessionStore::open(&directory.0).unwrap();
        store.make_durable(&retained, &cancel).unwrap();
        store.commit(&prepared(&seed), &cancel).unwrap();
        let head = fs::read(directory.0.join("head.json")).unwrap();
        let tile = blob(91); let bytes = tile.compressed_on_worker().unwrap().into_arc(); let expected = tile.decode_on_worker().unwrap();
        assert!(store.make_durable_using(&tiles(tile.clone()), &cancel, FailedAppend {sync}).is_err());
        assert_eq!(tile.resident_bytes(), bytes.len());
        assert!(Arc::ptr_eq(&tile.compressed_on_worker().unwrap().into_arc(), &bytes));
        assert_eq!(tile.decode_on_worker().unwrap(), expected);
        assert_eq!(fs::read(directory.0.join("head.json")).unwrap(), head);
        assert!(store.load(ProjectLimits::default(), &cancel).unwrap().is_some());
    }
}

struct BlockedAppend { entered: mpsc::Sender<()>, resume: mpsc::Receiver<()> }
impl Publication for BlockedAppend {
    fn publish(&mut self, _: &Path, _: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>,
        _: impl FnOnce() -> Result<(), String>) -> Result<(), AtomicReplaceError> { panic!("existing live pack must be appended") }
    fn append(&mut self, path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>, ready: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        append_using(path, write, ready, |_| {
            self.entered.send(()).unwrap(); self.resume.recv().unwrap(); Ok(())
        })
    }
}
#[test]
fn blocked_pressure_sync_does_not_hold_tile_storage_lock() {
    let directory = Directory::new(); let cancel = AtomicBool::new(false);
    let seed = tiles(blob(117));
    let mut store = SessionStore::open(&directory.0).unwrap();
    store.make_durable(&seed, &cancel).unwrap();
    let tile = blob(131); let bytes = tile.compressed_on_worker().unwrap().into_arc(); let expected = tile.decode_on_worker().unwrap();
    let retained = tiles(tile.clone());
    let (entered, writing) = mpsc::channel(); let (resume, proceed) = mpsc::channel();
    let writer = std::thread::spawn(move || store.make_durable_using(&retained, &cancel,
        BlockedAppend {entered, resume: proceed}));
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
    let mut store = SessionStore::open(&directory.0).unwrap();
    let expected = blobs.iter().map(|blob| blob.compressed_len()).sum::<usize>();
    assert_eq!(store.make_durable(&retained, &cancel).unwrap(), expected);
    let physical = pack_lengths(&store);
    assert!(!physical.is_empty());
    assert_eq!(store.packs.len(), physical.len());
    let file_owners = store.packs.values().map(|pack| Arc::as_ptr(&pack.root.file))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(file_owners.len(), physical.len());
    for blob in &blobs {
        let location = blob.compressed.location().unwrap().unwrap();
        assert!(store.packs.values().any(|pack| Arc::ptr_eq(pack, &location.pack)));
    }
    let original = blobs[0].compressed.location().unwrap().unwrap();
    let aliased = alias.compressed.location().unwrap().unwrap();
    assert!(Arc::ptr_eq(&original, &aliased));
    assert!(Arc::ptr_eq(&original.pack, &aliased.pack));
    assert_eq!(original.offset, aliased.offset);
    assert_eq!(retained.resident_bytes(), 0);
}

#[test]
fn a_cold_durable_tile_polls_pending_and_decodes_on_its_worker() {
    let directory=Directory::new();let cancel=AtomicBool::new(false);
    let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {override_bytes:Some(1<<30),..Default::default()});
    let cache=crate::tile_io::ReadCache::new(budget);cache.set_scheduler(Arc::new(|_|{}));
    let tile=blob(73);let expected=tile.decode_on_worker().unwrap();
    let mut store=SessionStore::open_with_cache(&directory.0,cache.clone()).unwrap();
    store.make_durable(&tiles(tile.clone()),&cancel).unwrap();
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
    let mut store=SessionStore::open_with_cache(&directory.0,cache).unwrap();
    store.make_durable(&tiles(tile.clone()),&cancel).unwrap();
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
