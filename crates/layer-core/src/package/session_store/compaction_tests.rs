use super::*;
use super::compaction::*;
use super::super::session::EditorCapture;
use super::tests::{control,open_store,Fault,Paths,Wait};
use crate::{Document,DocumentNames,Editor,authored::Resource,raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey}};
use std::fs::{self,OpenOptions};

struct Directory(PathBuf);
impl Directory {fn new()->Self {Self(std::env::temp_dir().join(format!("capy-compaction-{}",PortableId::random())))}}
impl Drop for Directory {fn drop(&mut self) {let _=fs::remove_dir_all(&self.0);}}
fn capture(marker:u8)->EditorCapture {
    let mut document=Document::new(PortableId::random(),256,256,DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let tile=RasterTile::backed(TileBlob::encode(RasterPlane::Color.descriptor(Default::default()),&vec![marker;256*256*4]).unwrap());
    *document.target_raster_mut(document.working.target.unwrap()).unwrap()=RasterRevision::backed(RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0;2]},tile)].into(),..Default::default()});
    let editor=Editor::new(document);editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap()
}
fn checkpoint(store:&mut SessionStore,capture:&EditorCapture,marker:u8) {
    store.checkpoint(capture,serde_json::json!({"marker":marker}).into(),&AtomicBool::new(false)).wait().unwrap();
}
fn input(resource:Resource<[u8]>)->StoreInput {
    StoreInput::entry(&super::super::resources::ResourceEntry {kind:"capy.test/1",data:serde_json::json!({}),raw_encoding:"capy.test/1",payload:Payload::Encoded(resource)},&AtomicBool::new(false)).unwrap()
}
fn sparse_dead_tail(store:&mut SessionStore)->PortableId {
    let (id,extent)=store.append.unwrap();
    OpenOptions::new().write(true).open(store.pack_path(id)).unwrap().set_len(extent+DEAD_BYTES+extent+1).unwrap();
    store.pack(id,extent).wait().unwrap();
    store.append=None;id
}
fn step(store:&mut SessionStore)->Result<CompactionStep,String> {
    let activity=CompactionActivity {state:Arc::new(Activity::default()),busy:false};let epoch=activity.revision();store.compact_ready_step(&activity,epoch).wait()
}
fn finish(store:&mut SessionStore)->Vec<u64> {
    let mut copied=Vec::new();
    for _ in 0..128 {match step(store).unwrap() {
        CompactionStep::Copied(bytes)=>{assert!(bytes<=CHUNK_BYTES);copied.push(bytes);},
        CompactionStep::Published=>return copied,
        CompactionStep::Idle=>panic!("qualified compaction did not progress"),
    }}panic!("bounded compaction did not finish")
}
#[test]
fn activity_domains_keep_cloned_holds_until_the_last_marker_drops() {
    let domain=CompactionActivity::new();
    let other=CompactionActivity::new();
    let mut active=domain.clone();active.set_busy(true);
    let held=active.clone();drop(active);
    assert!(!CompactionActivity::idle_at(&domain.state,u64::MAX,CHECKPOINT_QUIET_MS));
    assert!(CompactionActivity::idle_at(&other.state,u64::MAX,CHECKPOINT_QUIET_MS));
    drop(held);
    assert!(CompactionActivity::idle_at(&domain.state,u64::MAX,CHECKPOINT_QUIET_MS));
}
#[test]
fn qualification_trigger_ratio_and_idle_boundaries_are_exact() {
    let candidate=|live,dead|Candidate {id:PortableId::random(),live,dead};
    assert!(!candidate(0,DEAD_BYTES).qualifies());assert!(!candidate(DEAD_BYTES,DEAD_BYTES).qualifies());
    assert!(!candidate(1,DEAD_BYTES-1).qualifies());assert!(candidate(1,DEAD_BYTES).qualifies());
    assert!(candidate(2,8).before(&candidate(3,9)));assert!(!candidate(2,8).before(&candidate(1,4)));
    assert!(!triggered(400,100,None));assert!(triggered(400,101,None));
    assert!(!triggered(4*(DEAD_TRIGGER+1),DEAD_TRIGGER,None));assert!(triggered(4*(DEAD_TRIGGER+1),DEAD_TRIGGER+1,None));
    let reserve=PACK_BYTES+MAX_RANGE_BYTES as u64;
    assert!(!triggered(400,0,Some(reserve)));assert!(triggered(400,0,Some(reserve-1)));
    let state=Activity::default();state.last.store(7,Ordering::Relaxed);
    assert!(!CompactionActivity::idle_at(&state,7+IDLE_MS-1,IDLE_MS));assert!(CompactionActivity::idle_at(&state,7+IDLE_MS,IDLE_MS));
    for now in (7..10007).step_by(100) {state.last.store(now,Ordering::Relaxed);assert!(!CompactionActivity::idle_at(&state,now+100,CHECKPOINT_QUIET_MS));}
    let last=state.last.load(Ordering::Relaxed);
    assert!(!CompactionActivity::idle_at(&state,last+CHECKPOINT_QUIET_MS-1,CHECKPOINT_QUIET_MS));
    assert!(CompactionActivity::idle_at(&state,last+CHECKPOINT_QUIET_MS,CHECKPOINT_QUIET_MS));
    assert!(CompactionActivity::idle_at(&state,last+1000,CHECKPOINT_QUIET_MS));
    state.busy.store(1,Ordering::Relaxed);assert!(!CompactionActivity::idle_at(&state,u64::MAX,CHECKPOINT_QUIET_MS));
    state.busy.store(0,Ordering::Relaxed);state.background.store(true,Ordering::Relaxed);assert!(!CompactionActivity::idle_at(&state,u64::MAX,CHECKPOINT_QUIET_MS));
}
#[test]
fn copy_steps_are_bounded_and_remap_live_ranges_without_copying_aliases_twice() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    let resource:Resource<[u8]>=vec![71;CHUNK_BYTES as usize+17].into();let alias=resource.alias(PortableId::random());
    let id=resource.id();let alias_id=alias.id();
    store.store_inputs(vec![input(resource.clone()),input(alias.clone())],&AtomicBool::new(false)).wait().unwrap().commit().unwrap();
    let location=store.index[&id].location.clone();let old=sparse_dead_tail(&mut store);
    assert!(Arc::ptr_eq(&location,&store.index[&alias_id].location));
    assert_eq!(finish(&mut store),[CHUNK_BYTES,17]);
    assert_ne!(location.range().pack.id,old);assert!(Arc::ptr_eq(&location,&store.index[&alias_id].location));
    assert_eq!(&*location.read_all().wait().unwrap(),&*resource);assert!(!store.pack_path(old).exists());
}
#[test]
fn reference_transfers_hold_the_pack_range_accepted_before_compaction() {
    use super::super::transfer::{PreparedTransfer,TransferReceiver};
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();let capture=capture(37);
    checkpoint(&mut store,&capture,37);let cancel=AtomicBool::new(false);
    let transfer=PreparedTransfer::capture_referenced(&capture.artwork,&None,&cancel,&crate::MemoryBudget::process()).wait().unwrap();
    assert_eq!(transfer.payload_count(),0);assert_eq!(transfer.descriptor().backings().len(),1);
    let old=sparse_dead_tail(&mut store);
    let received=pollster::block_on(TransferReceiver::with_sources(transfer.descriptor().clone(),ProjectLimits::default(),|reference| {
        let backing=transfer.backings().iter().find(|backing|backing.transfer_reference().as_ref()==Some(reference)).unwrap();
        Ok(Arc::new(backing.read_source().unwrap()) as Arc<dyn ByteSource>)
    })).unwrap().finish().unwrap();
    let adopted=received.adopt_verified(ProjectLimits::default(),&cancel).unwrap();drop(received);drop(transfer);
    assert!(!finish(&mut store).is_empty());assert!(store.pack_path(old).exists());
    let editor=crate::Editor::new(adopted.into_document().unwrap());
    let tiles=editor.retained_tiles().blobs().unwrap();assert_eq!(tiles.len(),1);assert_eq!(tiles[0].decode_on_worker().unwrap(),vec![37;256*256*4]);
    drop(tiles);drop(editor);assert_eq!(step(&mut store).unwrap(),CompactionStep::Idle);assert!(!store.pack_path(old).exists());
}
#[test]
fn publication_preserves_current_previous_capture_and_a_physical_read_lease() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    let previous=capture(13);checkpoint(&mut store,&previous,13);
    let mut editor=Editor::new(previous.document.clone());let revised=capture(29);
    editor.perform(crate::Edit::SetRaster {target:editor.document().working.target.unwrap(),revision:revised.document.target_raster(revised.document.working.target.unwrap()).unwrap().clone()}).unwrap();
    let current=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();checkpoint(&mut store,&current,29);
    let locations=store.index.values().map(|entry|entry.location.clone()).collect::<Vec<_>>();
    let old=sparse_dead_tail(&mut store);let location=locations.iter().find(|location|location.range().pack.id==old).unwrap();
    let leased=location.read().wait().unwrap();let expected=leased.to_vec();
    assert!(!finish(&mut store).is_empty());
    assert!(store.pack_path(old).exists());assert_eq!(&*leased,&expected);
    assert!(locations.iter().all(|location|location.range().pack.id!=old));
    assert_eq!(store.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().unwrap().unwrap().metadata.value["marker"],29);
    for (capture,expected) in [(&previous,vec![13]),(&current,vec![13,29])] {
        let markers=capture.retained_tiles().blobs().unwrap().iter().map(|tile| {let bytes=tile.decode_on_worker().unwrap();assert!(bytes.iter().all(|byte|*byte==bytes[0]));bytes[0]}).collect::<BTreeSet<_>>();
        assert_eq!(markers,expected.into_iter().collect());
    }
    drop(leased);let state=control(&store);state.lock().unwrap().fault=Some(Fault::Remove);
    assert!(step(&mut store).is_err());assert!(store.pack_path(old).exists());
    state.lock().unwrap().fault=None;
    assert_eq!(step(&mut store).unwrap(),CompactionStep::Idle);assert!(!store.pack_path(old).exists());
    let before=state.lock().unwrap().io;
    for _ in 0..2 {assert_eq!(step(&mut store).unwrap(),CompactionStep::Idle);assert_eq!(state.lock().unwrap().io,before);}
    let head=store.head.clone().unwrap();drop(locations);drop(store);
    let mut reopened=open_store(&directory.0).wait().unwrap();assert!(reopened.head.as_ref().unwrap()==&head);
    assert_eq!(reopened.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().unwrap().unwrap().metadata.value["marker"],29);
    let current_path=reopened.generation_path(head.current.id);drop(reopened);fs::write(current_path,b"corrupt").unwrap();
    let mut fallback=open_store(&directory.0).wait().unwrap();
    assert_eq!(fallback.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().unwrap().unwrap().metadata.value["marker"],13);
}
#[test]
fn failed_copy_checksum_flush_and_head_publication_leave_the_old_generation_readable() {
    for fault in ["checksum","copy","flush","head"] {
        let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
        let captured=capture(43);checkpoint(&mut store,&captured,43);let head=store.head.clone().unwrap();let old=sparse_dead_tail(&mut store);
        let original=fs::read(store.pack_path(old)).unwrap();
        if fault=="checksum" {let mut bytes=original.clone();bytes[0]^=1;fs::write(store.pack_path(old),bytes).unwrap();}
        else {control(&store).lock().unwrap().fault=Some(match fault {"copy"=>Fault::Partial(b"partial"),"flush"=>Fault::Sync("Compaction sync failed"),_=>Fault::HeadWrite(0)});}
        let mut error=None;
        for _ in 0..32 {match step(&mut store) {Err(found)=>{error=Some(found);break;},Ok(CompactionStep::Copied(bytes))=>assert!(bytes<=CHUNK_BYTES),other=>panic!("unexpected {other:?}")}}
        assert!(error.is_some(),"{fault}");assert!(store.head.as_ref().unwrap()==&head);assert!(store.pack_path(old).exists());
        control(&store).lock().unwrap().fault=None;
        if fault=="checksum" {fs::write(store.pack_path(old),original).unwrap();}
        drop(store);let mut reopened=open_store(&directory.0).wait().unwrap();
        assert_eq!(reopened.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().unwrap().unwrap().metadata.value["marker"],43,"{fault}");
    }
}
#[test]
fn a_checkpoint_between_copy_steps_survives_publication_and_restart() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    let first=capture(11);checkpoint(&mut store,&first,11);
    let resource:Resource<[u8]>=vec![67;CHUNK_BYTES as usize+31].into();let id=resource.id();
    store.store_inputs(vec![input(resource.clone())],&AtomicBool::new(false)).wait().unwrap().commit().unwrap();sparse_dead_tail(&mut store);
    assert!(matches!(step(&mut store).unwrap(),CompactionStep::Copied(_)));
    let partial=fs::read_dir(directory.0.join("packs")).unwrap().map(|entry|entry.unwrap().path()).collect::<BTreeSet<_>>();
    store.collect_garbage().wait().unwrap();assert!(partial.iter().all(|path|path.exists()));
    let next=capture(59);checkpoint(&mut store,&next,59);finish(&mut store);
    assert_eq!(&*store.index[&id].location.read_all().wait().unwrap(),&*resource);
    drop(store);let mut reopened=open_store(&directory.0).wait().unwrap();
    assert_eq!(reopened.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().unwrap().unwrap().metadata.value["marker"],59);
}

#[test]
fn output_space_is_reserved_before_any_copy_or_head_change() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    let captured=capture(83);checkpoint(&mut store,&captured,83);let old=sparse_dead_tail(&mut store);
    let head=store.head.clone();let before=fs::read_dir(directory.0.join("packs")).unwrap().count();
    control(&store).lock().unwrap().space=Some(Some(0));
    assert_eq!(step(&mut store).unwrap_err(),crate::package::storage_budget::STORAGE_UNAVAILABLE);
    assert!(store.head==head);assert!(store.pack_path(old).exists());assert!(store.compaction.is_none());
    assert_eq!(fs::read_dir(directory.0.join("packs")).unwrap().count(),before);
}

#[test]
fn reopening_recovers_the_append_pack_and_excludes_it_from_compaction() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    let saved=capture(89);checkpoint(&mut store,&saved,89);let old=sparse_dead_tail(&mut store);drop(store);
    let mut reopened=open_store(&directory.0).wait().unwrap();assert_eq!(reopened.append.unwrap().0,old);
    let head=reopened.head.clone();assert_eq!(step(&mut reopened).unwrap(),CompactionStep::Idle);
    assert!(reopened.head==head);assert_eq!(fs::read_dir(directory.0.join("packs")).unwrap().count(),1);
}

#[test]
fn compaction_refusal_is_latched_until_real_cleanup_without_blocking_foreground_writes() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    let saved=capture(97);checkpoint(&mut store,&saved,97);sparse_dead_tail(&mut store);
    control(&store).lock().unwrap().space=Some(Some(1<<30));control(&store).lock().unwrap().fault=Some(Fault::StorageFull);
    assert_eq!(step(&mut store).unwrap_err(),crate::package::storage_budget::STORAGE_UNAVAILABLE);
    control(&store).lock().unwrap().fault=None;
    let packs=||fs::read_dir(directory.0.join("packs")).unwrap().map(|entry| {let entry=entry.unwrap();(entry.path(),entry.metadata().unwrap().len())}).collect::<BTreeMap<_,_>>();
    let before=packs();for _ in 0..20 {assert_eq!(step(&mut store).unwrap(),CompactionStep::Idle);}assert_eq!(packs(),before);
    let foreground:Resource<[u8]>=vec![101;7].into();
    store.store_inputs(vec![input(foreground.clone())],&AtomicBool::new(false)).wait().unwrap().commit().unwrap();
    assert!(store.compaction_refusal.is_some());assert_eq!(step(&mut store).unwrap(),CompactionStep::Idle);
    let cleanup="cleanup.unused";let file=store._owner.io.open(cleanup,super::super::store_io::FileAccess::Create).wait().unwrap();
    file.write_at(0,b"freed").wait().unwrap();file.flush().wait().unwrap();drop(file);
    super::super::storage_budget::remove(&*store._owner.io,cleanup).wait().unwrap();
    assert!(matches!(step(&mut store).unwrap(),CompactionStep::Copied(_)));finish(&mut store);
    assert!(store.compaction_refusal.is_none());
}

#[test]
fn restart_collects_unpublished_partial_output_and_preserves_the_head() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    let saved=capture(107);checkpoint(&mut store,&saved,107);let head=store.head.clone();
    let resource:Resource<[u8]>=vec![109;CHUNK_BYTES as usize+19].into();
    store.store_inputs(vec![input(resource.clone())],&AtomicBool::new(false)).wait().unwrap().commit().unwrap();let old=sparse_dead_tail(&mut store);
    assert!(matches!(step(&mut store).unwrap(),CompactionStep::Copied(_)));
    let partial=fs::read_dir(store.path.join("packs")).unwrap().map(|entry|entry.unwrap().path()).filter(|path|*path!=store.pack_path(old)).collect::<Vec<_>>();assert_eq!(partial.len(),1);
    drop((store,resource));let mut reopened=open_store(&directory.0).wait().unwrap();
    assert!(reopened.head==head);assert!(partial.iter().all(|path|!path.exists()));
    assert_eq!(reopened.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().unwrap().unwrap().metadata.value["marker"],107);
}

#[test]
fn a_loaded_logical_lease_survives_both_heads_being_replaced_and_compacted() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    {let first=capture(113);checkpoint(&mut store,&first,113);}
    let loaded=store.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().unwrap().unwrap();
    let tiles=loaded.editor.retained_tiles().blobs().unwrap();
    for marker in [127,131] {let next=capture(marker);checkpoint(&mut store,&next,marker);}
    let old=sparse_dead_tail(&mut store);finish(&mut store);
    assert!(!store.pack_path(old).exists());
    for tile in &tiles {assert!(tile.decode_on_worker().unwrap().iter().all(|byte|*byte==113));}
    assert_eq!(store.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().unwrap().unwrap().metadata.value["marker"],131);
}

#[test]
fn input_after_admission_prevents_planning_and_copying() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    let captured=capture(57);checkpoint(&mut store,&captured,57);sparse_dead_tail(&mut store);
    let before=fs::read_dir(store.path.join("packs")).unwrap().count();let head=store.head.clone();
    let activity=CompactionActivity {state:Arc::new(Activity::default()),busy:false};let epoch=activity.revision();activity.touch();
    assert_eq!(store.compact_ready_step(&activity,epoch).wait().unwrap(),CompactionStep::Idle);
    assert!(store.compaction.is_none());assert!(store.head==head);assert_eq!(fs::read_dir(store.path.join("packs")).unwrap().count(),before);
}

fn head_flush_operation()->usize {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    let previous=capture(151);checkpoint(&mut store,&previous,151);
    let saved=capture(163);checkpoint(&mut store,&saved,163);sparse_dead_tail(&mut store);
    *control(&store).lock().unwrap()=Default::default();finish(&mut store);
    let operations=control(&store).lock().unwrap().operations.clone();
    operations.iter().rposition(|operation|operation=="head").unwrap()+1
}

#[test]
fn a_visible_head_flush_error_preserves_live_bytes_and_can_be_reloaded_and_checkpointed() {
    let operation=head_flush_operation();
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    let previous=capture(151);checkpoint(&mut store,&previous,151);
    let saved=capture(163);checkpoint(&mut store,&saved,163);sparse_dead_tail(&mut store);
    let state=control(&store);*state.lock().unwrap()=Default::default();
    state.lock().unwrap().fault=Some(Fault::Interrupt {operation,after:true});let before_sequence=store.sequence;let old_head=store.head.clone().unwrap();
    let error=loop {match step(&mut store) {Ok(CompactionStep::Copied(bytes))=>assert!(bytes<=CHUNK_BYTES),Err(error)=>break error,other=>panic!("expected head flush interruption, got {other:?}")}};
    assert_eq!(error,"Interrupted flush");state.lock().unwrap().fault=None;
    let visible=head::read(&*store._owner.io).wait().unwrap().unwrap();assert!(visible.sequence>before_sequence);
    let restored=store.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().unwrap().unwrap();
    assert_eq!(restored.metadata.value["marker"],163);
    for tiles in [saved.retained_tiles(),restored.editor.retained_tiles()] {for tile in tiles.blobs().unwrap() {assert!(tile.decode_on_worker().unwrap().iter().all(|byte|*byte==163));}}
    store.collect_garbage().wait().unwrap();
    for generation in std::iter::once(&old_head.current).chain(old_head.previous.as_ref()) {assert!(store.generation_path(generation.id).is_file(),"failed head flush must retain both old generation files");}
    let next=capture(167);checkpoint(&mut store,&next,167);store.collect_garbage().wait().unwrap();
    for generation in std::iter::once(&old_head.current).chain(old_head.previous.as_ref()) {assert!(!store.generation_path(generation.id).exists(),"successful checkpoint releases unreferenced fallback generations");}
    drop((restored,store));let mut reopened=open_store(&directory.0).wait().unwrap();
    assert_eq!(reopened.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().unwrap().unwrap().metadata.value["marker"],167);
    for tile in saved.retained_tiles().blobs().unwrap() {assert!(tile.decode_on_worker().unwrap().iter().all(|byte|*byte==163));}
}

#[test]
fn retirement_releases_failed_publication_roots_and_pending_copy_state() {
    let operation=head_flush_operation();
    for failed_publication in [false,true] {
        let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
        let previous=capture(151);checkpoint(&mut store,&previous,151);let saved=capture(163);checkpoint(&mut store,&saved,163);
        let old_head=store.head.clone().unwrap();let location=store.index.values().next().unwrap().location.clone();
        let leased=location.read().wait().unwrap();let expected=leased.to_vec();
        let extra=(!failed_publication).then(||Resource::<[u8]>::from(vec![173;CHUNK_BYTES as usize+23]));
        if let Some(extra)=&extra {store.store_inputs(vec![input(extra.clone())],&AtomicBool::new(false)).wait().unwrap().commit().unwrap();}
        let old=sparse_dead_tail(&mut store);
        if failed_publication {
            let state=control(&store);*state.lock().unwrap()=Default::default();state.lock().unwrap().fault=Some(Fault::Interrupt {operation,after:true});
            let error=loop {match step(&mut store) {Ok(CompactionStep::Copied(_))=>{},Err(error)=>break error,other=>panic!("expected flush interruption: {other:?}")}};
            assert_eq!(error,"Interrupted flush");state.lock().unwrap().fault=None;assert!(store.compaction_fallback.is_some());
        }else {assert!(matches!(step(&mut store).unwrap(),CompactionStep::Copied(_)));assert!(store.compaction.is_some());}
        let output=fs::read_dir(store.path.join("packs")).unwrap().map(|entry|entry.unwrap().path()).filter(|path|*path!=store.pack_path(old)).collect::<Vec<_>>();assert!(!output.is_empty());
        store.retire().wait().unwrap();
        assert!(store.compaction.is_none());assert!(store.compaction_fallback.is_none());assert!(store.compaction_refusal.is_none());
        for generation in std::iter::once(&old_head.current).chain(old_head.previous.as_ref()) {assert!(!store.generation_path(generation.id).exists());}
        assert!(output.iter().all(|path|!path.exists()));assert_eq!(&*leased,&expected);
    }
}

#[test]
fn reopen_indexes_missing_generations_on_the_first_idle_turn_only() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();
    let previous=capture(179);checkpoint(&mut store,&previous,179);
    let saved=capture(181);checkpoint(&mut store,&saved,181);
    let head=store.head.as_ref().unwrap();
    let previous_ids=store.generation_roots[&head.previous.as_ref().unwrap().id].1.identities.clone();
    let current_ids=store.generation_roots[&head.current.id].1.identities.clone();
    drop((store,previous,saved));
    let mut store=open_store(&directory.0).wait().unwrap();assert!(store.index.is_empty());
    let restored=store.load(ProjectLimits::default(),&AtomicBool::new(false)).wait().unwrap().unwrap();
    assert_eq!(restored.metadata.value["marker"],181);
    assert!(current_ids.iter().all(|id|store.index.contains_key(id)));
    assert!(previous_ids.difference(&current_ids).any(|id|!store.index.contains_key(id)));
    let append=store.append;
    let state=control(&store);let before=state.lock().unwrap().io;
    assert_eq!(step(&mut store).unwrap(),CompactionStep::Idle);
    assert_eq!(store.append,append);
    assert!(previous_ids.iter().all(|id|store.index.contains_key(id)));
    let after=state.lock().unwrap().io;
    assert!(after.opens>before.opens);assert!(after.reads>before.reads);
    let before=after;
    for _ in 0..2 {assert_eq!(step(&mut store).unwrap(),CompactionStep::Idle);assert_eq!(state.lock().unwrap().io,before);}
}

#[test]
fn package_copy_bounds_each_idle_chunk_and_stops_on_input_before_writing() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();let cancelled=AtomicBool::new(false);
    let descriptor=RasterPlane::Color.descriptor(Default::default());let mut random=17u32;let mut bytes=Vec::new();let mut records=Vec::new();
    for _ in 0..24 {
        let raw=(0..256*256*4).map(|_|{random^=random<<13;random^=random>>17;random^=random<<5;random as u8}).collect::<Vec<_>>();
        let tile=TileBlob::encode(descriptor,&raw).unwrap();let encoded=tile.memory_compressed().unwrap().unwrap();
        records.push((bytes.len() as u64,encoded.len() as u64,crc32fast::hash(&encoded)));bytes.extend_from_slice(&encoded);
    }
    let lease=super::super::PackageLease::new(ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap());
    let tiles=crate::raster_storage::RetainedTiles {sources:records.iter().map(|&(offset,length,crc)|Arc::new(TileBlob::from_package_range(PortableId::random(),descriptor,lease.range(offset,length,crc,descriptor,false).unwrap()).unwrap())).collect(),..Default::default()};
    let mut copy=PackageCopy::new(&tiles).wait().unwrap();let activity=CompactionActivity {state:Arc::new(Activity::default()),busy:false};
    let epoch=activity.revision();activity.touch();
    store.copy_package_ready(&mut copy,&cancelled,&activity,epoch).wait().unwrap();
    assert_eq!(copy.remaining(),24);assert!(store.index.is_empty());
    let mut previous=0;let mut chunks=0;
    while !copy.complete() {
        let before=copy.remaining();store.copy_package_ready(&mut copy,&cancelled,&activity,activity.revision()).wait().unwrap();
        let bytes=fs::read_dir(directory.0.join("packs")).unwrap().map(|entry|entry.unwrap().metadata().unwrap().len()).sum::<u64>();
        assert!(copy.remaining()<before);assert!(bytes-previous<=CHUNK_BYTES);previous=bytes;chunks+=1;
        assert_eq!(store.generation(),None);
    }
    assert!(chunks>1);assert!(tiles.blobs().unwrap().iter().all(|tile|tile.compressed.location().unwrap().is_some()));
}

#[test]
fn package_copy_admits_a_full_chunk_of_known_compressed_tiles() {
    let directory=Directory::new();let mut store=open_store(&directory.0).wait().unwrap();let cancel=AtomicBool::new(false);
    let descriptor=RasterPlane::Color.descriptor(Default::default());let mut bytes=Vec::new();let mut records=Vec::new();
    for marker in 0..64 {
        let tile=TileBlob::encode(descriptor,&vec![marker;256*256*4]).unwrap();let encoded=tile.memory_compressed().unwrap().unwrap();
        records.push((bytes.len() as u64,encoded.len() as u64,crc32fast::hash(&encoded)));bytes.extend_from_slice(&encoded);
    }
    assert!((bytes.len() as u64)<CHUNK_BYTES);assert!(64*TileBlob::max_compressed_len(descriptor).unwrap() as u64>CHUNK_BYTES);
    let lease=super::super::PackageLease::new(ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap());
    let tiles=crate::raster_storage::RetainedTiles {sources:records.iter().map(|&(offset,length,crc)|Arc::new(TileBlob::from_package_range(PortableId::random(),descriptor,lease.range(offset,length,crc,descriptor,false).unwrap()).unwrap())).collect(),..Default::default()};
    let mut copy=PackageCopy::new(&tiles).wait().unwrap();let activity=CompactionActivity {state:Arc::new(Activity::default()),busy:false};
    store.copy_package_ready(&mut copy,&cancel,&activity,activity.revision()).wait().unwrap();
    assert!(copy.complete());assert_eq!(store.index.len(),64);
}
