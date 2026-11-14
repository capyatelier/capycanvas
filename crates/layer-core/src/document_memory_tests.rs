use super::*;
use crate::{Document, DocumentNames, Editor, ProjectLimits, SourceTarget, Edit, RecordChange,
    color::source::{SourceImage, SourceInterpretation, SourceChannels},
    package::{self, ByteSource, ByteRange, RangeState, ImmutableBacking}, raster::{TileBlob, TILE_SIZE}};
use std::{collections::BTreeMap, fs::File, io::{Read, Seek, SeekFrom}, sync::{Arc, Mutex, atomic::AtomicBool}};

fn stored_inventory(editor: &Editor) -> (crate::raster_storage::tests::Store, Arc<crate::raster_storage::RetainedTiles>) {
    let store = crate::raster_storage::tests::Store::new(&editor.retained_tiles());
    let inventory = editor.document().memory.owner.inventories.lock().unwrap().current.clone();
    (store, inventory)
}
fn current_inventory(editor: &Editor) -> Arc<crate::raster_storage::RetainedTiles> {
    editor.document().memory.owner.inventories.lock().unwrap().current.clone()
}

#[test]
fn shared_raster_content_metadata_is_charged_once_across_history_and_captures() {
    use crate::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileKey};
    use crate::raster_storage::CONTENT_TREE_METADATA;
    for revisions in [1,4] {for capture_count in [0,1,3] {
        let memory=budget();
        let document=Document::new(crate::PortableId::random(),256,256,DocumentNames {paint:"Ink".into(),paper:"Paper".into()})
            .with_memory(DocumentMemory::new(memory,None));
        let target=document.working.target.unwrap();
        let descriptor=RasterPlane::Color.descriptor(Default::default());
        let blob=Arc::new(TileBlob::encode(descriptor,&[31,63,127,255].repeat(256*256)).unwrap());
        let data=||RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0,0]},RasterTile::backed_shared(blob.clone()))].into(),..Default::default()};
        let old=data();let current=data();
        let old_owners=old.tiles.metadata_owners();let current_owners=current.tiles.metadata_owners();
        assert!(old_owners.iter().all(|(id,_)|current_owners.iter().all(|(other,_)|id!=other)));
        let mut editor=Editor::new(document);
        editor.perform(Edit::SetRaster {target,revision:RasterRevision::backed(old)}).unwrap();
        for _ in 0..revisions {
            editor.perform(Edit::SetRaster {target,revision:RasterRevision::backed(current.clone())}).unwrap();
        }
        editor.flush_memory();
        let inventory=current_inventory(&editor);
        let captures=(0..capture_count).map(|_|editor.document().memory.capture_current()).collect::<Vec<_>>();
        let (parts,_)=editor.document().memory.owner.parts(false);
        let mut owners=std::collections::HashMap::new();inventory.resident_owners(&mut owners);
        let authored_trees=owners.iter().filter(|((kind,_),(_,authored))|*kind==CONTENT_TREE_METADATA && *authored).map(|(_, (bytes,_))|*bytes as u64).sum::<u64>();
        let history_trees=owners.iter().filter(|((kind,_),(_,authored))|*kind==CONTENT_TREE_METADATA && !*authored).map(|(_, (bytes,_))|*bytes as u64).sum::<u64>();
        assert!(authored_trees>=current_owners.iter().map(|(_,bytes)|*bytes as u64).sum());
        assert!(history_trees>=old_owners.iter().map(|(_,bytes)|*bytes as u64).sum());
        assert_eq!(parts[2],inventory.authored_metadata as u64+capture_count as u64*inventory.metadata_bytes as u64+authored_trees,
            "authored tree owners are metadata, shared by every capture and cloned revision");
        assert_eq!(parts[3],(inventory.metadata_bytes-inventory.authored_metadata) as u64+history_trees,
            "history-only tree owners remain history metadata");
        let payload=owners.iter().filter(|((kind,_),_)|*kind!=CONTENT_TREE_METADATA).map(|(_, (bytes,_))|*bytes as u64).sum::<u64>();
        assert_eq!(parts[0]+parts[1],payload,"tree metadata must not remain in spillable resident parts");
        assert_eq!(parts.iter().sum::<u64>(),owners.values().map(|(bytes,_)|*bytes as u64).sum::<u64>()
            +(capture_count as u64+1)*inventory.metadata_bytes as u64,"classification preserves the single-inventory physical total");
        drop(captures);
    }}
}

fn budget() -> MemoryBudget {
    MemoryBudget::with_facts(crate::MemoryFacts { total_bytes:Some(16 << 30),available_bytes:Some(12 << 30),override_bytes:Some(2 << 30),..Default::default() })
}
struct Files(std::path::PathBuf);
impl Drop for Files { fn drop(&mut self) { let _=std::fs::remove_dir_all(&self.0); } }
struct Backing(Mutex<File>,u64);
impl ByteSource for Backing {
    fn byte_len(&self)->u64 {self.1}
    fn resident_bytes(&self)->usize {0}
    fn poll(&self,offset:u64,length:usize)->Result<RangeState,String> {
        if length>package::MAX_RANGE_BYTES || offset.checked_add(length as u64).is_none_or(|end|end>self.1) {return Err("Invalid fixture file range".into());}
        let mut bytes=vec![0;length];let mut file=self.0.lock().unwrap();
        file.seek(SeekFrom::Start(offset)).map_err(|error|error.to_string())?;
        file.read_exact(&mut bytes).map_err(|error|error.to_string())?;
        Ok(RangeState::Ready(ByteRange::new(bytes.into(),0..length).unwrap()))
    }
}
fn backing(path:&std::path::Path)->ImmutableBacking {
    let file=File::open(path).unwrap();let length=file.metadata().unwrap().len();
    ImmutableBacking::new(Arc::new(Backing(Mutex::new(file),length))).unwrap()
}
fn limits(memory:&MemoryBudget)->ProjectLimits {
    ProjectLimits {asset_bytes:memory.ram_limit_bytes(),raster_bytes:memory.ram_limit_bytes(),..Default::default()}
}
fn source_grid(width:u32,height:u32)->Arc<SourceImage> {
    let interpretation=SourceInterpretation {channels:SourceChannels::Rgba,depth:Default::default(),profile:Default::default(),profile_assumed:false};
    let descriptor=interpretation.descriptor();let mut bytes=vec![0;descriptor.byte_len([TILE_SIZE;2]).unwrap()];
    let mut random=17u32;let mut tiles=BTreeMap::new();
    for y in 0..height {for x in 0..width {
        for byte in &mut bytes {random^=random<<13;random^=random>>17;random^=random<<5;*byte=random as u8;}
        tiles.insert([x,y],Arc::new(TileBlob::encode(descriptor,&bytes).unwrap()));
    }}
    Arc::new(SourceImage {extent:[width*TILE_SIZE,height*TILE_SIZE],resolution:None,interpretation,tiles})
}

fn reserved_preview() -> (MemoryBudget, Editor, Edit, Edit, crate::MemoryReservation, u64) {
    let memory=budget();
    let document=Document::new(crate::PortableId::random(),768,768,DocumentNames {paint:"Preview".into(),paper:"Paper".into()})
        .with_memory(DocumentMemory::new(memory.clone(),None));
    let source=source_grid(3,3);let bytes=source.resident_bytes() as u64;
    let mut editor=Editor::new(document);
    let SourceTarget::Paint(target)=editor.document().working.target.unwrap() else {panic!()};
    let mut paint=editor.document().artwork.paint.get(target).unwrap().clone();
    paint.domain=[896;2];
    paint.base=Some(crate::PaintBase::new(source.into()));
    let insert=Edit::Paint(RecordChange::replace(&editor.document().artwork.paint,target,Some(paint)).unwrap());
    let inverse=editor.document().clone().apply(insert.clone()).unwrap();
    let reservation=memory.admit_import(16*1024*1024).unwrap();
    editor.preview_reserved(insert.clone(),reservation.clone()).unwrap();
    assert!(reservation.active());assert_eq!(memory.snapshot().reserved_bytes,16*1024*1024);
    (memory,editor,insert,inverse,reservation,bytes)
}

#[test]
fn reserved_preview_motion_keeps_admission_until_cancel_or_perform() {
    for apply in [false,true] {
        let (memory,mut editor,insert,inverse,reservation,bytes)=reserved_preview();
        let physical=memory.snapshot().ram_physical_bytes;
        let SourceTarget::Paint(target)=editor.document().working.target.unwrap() else {panic!()};
        for offset in 1..=10 {
            let mut paint=editor.document().artwork.paint.get(target).unwrap().clone();
            paint.base.as_mut().unwrap().offset=[offset,0];
            editor.preview(Edit::Paint(RecordChange::replace(&editor.document().artwork.paint,target,Some(paint)).unwrap())).unwrap();
            assert!(reservation.active());assert_eq!(memory.snapshot().ram_physical_bytes,physical);
        }
        if apply {
            editor.preview(inverse).unwrap();
            assert!(reservation.active());
            editor.perform(insert).unwrap();
            assert!(memory.snapshot().ram_pinned_bytes>=bytes);
            assert!(editor.can_undo());
        }else{
            editor.cancel_preview(inverse).unwrap();
            assert!(!editor.can_undo());
            assert!(editor.document().artwork.paint.get(target).unwrap().base.is_none());
        }
        assert!(!reservation.active());assert_eq!(memory.snapshot().reserved_bytes,0);
    }
}

#[test]
fn preview_capture_keeps_new_assets_after_commit_cancel_and_history_retirement() {
    for apply in [false,true] {
        let (memory,mut editor,insert,inverse,reservation,bytes)=reserved_preview();
        let snapshot=editor.document().scene().snapshot();
        let nested=snapshot.view().snapshot();
        assert!(memory.snapshot().ram_pinned_bytes<bytes+17*1024*1024);
        if apply {
            editor.preview(inverse).unwrap();
            editor.perform(insert).unwrap();
            editor.undo().unwrap();
            let SourceTarget::Paint(target)=editor.document().working.target.unwrap() else {panic!()};
            let mut paint=editor.document().artwork.paint.get(target).unwrap().clone();paint.domain=[769,768];
            editor.perform(Edit::Paint(RecordChange::replace(&editor.document().artwork.paint,target,Some(paint)).unwrap())).unwrap();
            assert!(!editor.can_redo());
        }else{editor.cancel_preview(inverse).unwrap();drop(insert);}
        assert!(!reservation.active());assert_eq!(memory.snapshot().reserved_bytes,0);
        assert!(memory.snapshot().ram_pinned_bytes>=bytes);
        assert!(memory.snapshot().ram_pinned_bytes<2*bytes);
        drop(editor);drop(snapshot);
        assert!(memory.snapshot().ram_pinned_bytes>=bytes);
        drop(nested);
        assert_eq!(memory.snapshot().ram_physical_bytes,0);
    }
}

#[test]
fn failed_reserved_preview_preserves_the_existing_admission() {
    let (memory,mut editor,_insert,inverse,reservation,_)=reserved_preview();
    let before=memory.snapshot().reserved_bytes;
    let SourceTarget::Paint(target)=editor.document().working.target.unwrap() else {panic!()};
    let paint=editor.document().artwork.paint.get(target).unwrap().clone();
    let bad=Edit::Paint(RecordChange {handle:crate::PaintHandle::from_index(u32::MAX),id:crate::PortableId::random(),value:Some(paint)});
    assert!(editor.preview_reserved(bad,memory.admit_import(1024*1024).unwrap()).is_err());
    assert!(reservation.active());assert_eq!(memory.snapshot().reserved_bytes,before);
    editor.cancel_preview(inverse).unwrap();
    assert_eq!(memory.snapshot().reserved_bytes,0);
}

#[test]
fn colour_previews_with_owned_programs_do_not_inventory_retained_storage() {
    let memory=budget();
    let mut document=Document::new(crate::PortableId::random(),256,256,DocumentNames {paint:"Paint".into(),paper:"Paper".into()});
    let SourceTarget::Paint(target)=document.working.target.unwrap() else {panic!()};
    document.artwork.paint.get_mut(target).unwrap().domain=[258;2];
    document.artwork.paint.get_mut(target).unwrap().base=Some(crate::PaintBase::new(source_grid(1,1).into()));
    let effect=document.artwork.effects.iter().next().unwrap().0;
    Arc::make_mut(&mut document.artwork.effects.get_mut(effect).unwrap().program).id="owned_colour_preview".into();
    let mut editor=Editor::new(document.with_memory(DocumentMemory::new(memory.clone(),None)));
    let (_store, _)=stored_inventory(&editor);
    for offset in [1,0] {
        let mut paint=editor.document().artwork.paint.get(target).unwrap().clone();paint.base.as_mut().unwrap().offset=[offset,0];
        editor.perform(Edit::Paint(RecordChange::replace(&editor.document().artwork.paint,target,Some(paint)).unwrap())).unwrap();
    }
    let inventory=current_inventory(&editor);
    for step in 0..10 {
        let mut application=editor.document().artwork.effects.get(effect).unwrap().clone();
        application.program=Arc::new((*application.program).clone());
        let index=application.values.iter().position(|value|matches!(value,crate::EffectValue::Number(_))).unwrap();
        application.values[index]=crate::EffectValue::Number(step as f32*0.05);
        let edit=Edit::Effect(RecordChange::replace(&editor.document().artwork.effects,effect,Some(application)).unwrap());
        assert!(!edit.requires_history_admission(editor.document()));
        editor.preview(edit).unwrap();
    }
    assert!(Arc::ptr_eq(&inventory,&current_inventory(&editor)));
    assert_eq!(memory.snapshot().reserved_bytes,0);
    let mut application=editor.document().artwork.effects.get(effect).unwrap().clone();
    let copied=|source:&crate::authored::Resource<str>|crate::authored::Resource::with_id(source.id(),source.storage().clone());
    let replacement=match &application.program.wgsl {
        crate::EffectShader::Code(source)=>crate::EffectShader::Code(copied(source)),
        crate::EffectShader::Linked {sources}=>{
            let mut sources=sources.to_vec();sources[0]=copied(&sources[0]);crate::EffectShader::Linked {sources:sources.into()}
        },
        crate::EffectShader::Modules(_)=>panic!("test program has resolved sources"),
    };
    Arc::make_mut(&mut application.program).wgsl=replacement;
    let edit=Edit::Effect(RecordChange::replace(&editor.document().artwork.effects,effect,Some(application)).unwrap());
    assert!(edit.requires_history_admission(editor.document()),"equal source text with a different owner still needs admission");
    editor.undo().unwrap();
    assert!(Arc::ptr_eq(&inventory,&current_inventory(&editor)));
    editor.flush_memory();
    assert!(!Arc::ptr_eq(&inventory,&current_inventory(&editor)));
}
fn assert_roundtrip(editor: &Editor, manifest: &[u8], identities: &[crate::PortableId], digests: &[[u8; 32]]) {
    let cancelled = AtomicBool::new(false);
    let capture = editor.capture(1, Default::default()).unwrap();
    let prepared = package::codec::PreparedPackage::prepare(&capture, None, &cancelled).unwrap();
    assert_eq!(prepared.manifest(), manifest);
    let images = editor.document().artwork.images().unwrap();
    let image = images.values().next().unwrap();
    assert_eq!(image.tiles.values().map(|tile| tile.resource_id()).collect::<Vec<_>>(), identities);
    for (tile, digest) in image.tiles.values().zip(digests) {
        assert_eq!(tile.content_digest().unwrap(), *digest);
    }
}
#[test]
fn import_above_512_mib_saves_reopens_and_recovers_with_the_same_budget() {
    let files=Files(std::env::temp_dir().join(format!("capy-budget-roundtrip-{}",crate::PortableId::random())));
    std::fs::create_dir_all(&files.0).unwrap();let package_path=files.0.join("large.capy");let recovery_path=files.0.join("recovery");
    let cancelled=AtomicBool::new(false);let memory=budget();
    let reservation=memory.admit_import(600*1024*1024).unwrap();
    let source=source_grid(41,50);assert!(source.resident_bytes()>512*1024*1024);
    let mut document=Document::new(crate::PortableId::random(),source.extent[0],source.extent[1],DocumentNames {paint:"Photo".into(),paper:"Paper".into()});
    let SourceTarget::Paint(target)=document.working.target.unwrap() else {panic!()};
    document.artwork.paint.get_mut(target).unwrap().base=Some(crate::PaintBase::new(source.clone().into()));
    document.validate(limits(&memory)).unwrap();
    let source_ids:Vec<_>=source.tiles.values().map(|tile|tile.resource_id()).collect();
    let source_digests:Vec<_>=source.tiles.values().map(|tile|tile.content_digest().unwrap()).collect();
    let editor=Editor::new(document.with_memory(DocumentMemory::new(memory.clone(),Some(reservation))));
    memory.update_ram(crate::MemoryFacts {override_bytes:Some(384*1024*1024),host_default_bytes:Some(384*1024*1024),..Default::default()},0);
    assert!(editor.retained_tiles().resident_bytes() as u64>memory.ram_limit_bytes());
    assert_eq!(ProjectLimits::default().asset_bytes,u64::MAX);
    assert_eq!(ProjectLimits::default().raster_bytes,u64::MAX);
    let capture=editor.capture(1,Default::default()).unwrap();
    let original = {
        let prepared = package::codec::PreparedPackage::prepare(&capture, None, &cancelled).unwrap();
        let manifest = prepared.manifest().to_vec();
        prepared.write(&mut File::create(&package_path).unwrap(), &cancelled).unwrap();
        manifest
    };
    let constrained=MemoryBudget::with_facts(crate::MemoryFacts {host_default_bytes:Some(384*1024*1024),override_bytes:Some(384*1024*1024),..Default::default()});
    let mut refused=Editor::new(Document::new(crate::PortableId::random(),64,64,DocumentNames {paint:"Unchanged".into(),paper:"Paper".into()}).with_memory(DocumentMemory::new(constrained.clone(),None)));
    let before=refused.document().clone();let checkpoint=refused.checkpoint();
    let SourceTarget::Paint(target)=before.working.target.unwrap() else {panic!()};
    let mut paint=before.artwork.paint.get(target).unwrap().clone();paint.domain=source.extent;paint.base=Some(crate::PaintBase::new(source.into()));
    assert!(refused.perform(Edit::Paint(RecordChange::replace(&before.artwork.paint,target,Some(paint)).unwrap())).is_err());
    assert_eq!(refused.document(),&before);assert_eq!(refused.checkpoint(),checkpoint);assert!(!refused.can_undo());
    assert_eq!(constrained.snapshot().reserved_bytes,0);assert!(constrained.snapshot().refusals>0);
    let session=editor.capture_session(capture.clone()).unwrap();
    let saved=package::session_store::tests::SavedSession::new(&session,serde_json::Value::Null,&cancelled).unwrap();
    let restored=saved.load().unwrap();
    assert_eq!(restored.editor.document().artwork,editor.document().artwork);
    drop((restored,saved,session));
    drop((editor,capture));
    let fresh=budget();
    let package::codec::OpenOutcome::Candidate {artwork,memory:admission,..}=package::codec::open_with_budget(backing(&package_path),limits(&fresh),&cancelled,fresh.clone()).unwrap() else {panic!("large artwork must reopen")};
    let reopened=Editor::new(Document::from_artwork(artwork).unwrap().with_memory(admission));
    assert_roundtrip(&reopened, &original, &source_ids, &source_digests);
    assert_eq!(fresh.snapshot().reserved_bytes,0);assert_eq!(reopened.retained_tiles().resident_bytes(),0);
    assert!(fresh.snapshot().ram_pinned_bytes<16*1024*1024);
    let capture=reopened.capture_session(reopened.capture(1,Default::default()).unwrap()).unwrap();
    let mut store=pollster::block_on(package::session_store::SessionStore::open(&recovery_path)).unwrap();
    pollster::block_on(store.checkpoint(&capture,serde_json::json!({}).into(),&cancelled)).unwrap();
    drop((reopened,capture,store));
    let recovery_budget=budget();let cache=crate::tile_io::ReadCache::new(recovery_budget.clone());
    let mut store=pollster::block_on(package::session_store::SessionStore::open_with_cache(&recovery_path,cache)).unwrap();
    let recovered=pollster::block_on(store.load_with_budget(limits(&recovery_budget),&cancelled,recovery_budget.clone())).unwrap().unwrap();
    assert_roundtrip(&recovered.editor, &original, &source_ids, &source_digests);
    let image=recovered.editor.document().artwork.images().unwrap();
    assert_eq!(image.values().next().unwrap().tiles.values().map(|tile|tile.resource_id()).collect::<Vec<_>>(),source_ids);
    assert_eq!(recovery_budget.snapshot().reserved_bytes,0);
    let pinned=recovery_budget.snapshot().ram_pinned_bytes;
    assert!(pinned>0);assert!(pinned<512*1024*1024);
    assert_eq!(recovered.editor.retained_tiles().resident_bytes(),0);
}

#[test]
fn native_capture_keeps_its_document_memory_owner_after_the_editor_drops() {
    let memory = budget();
    let document = Document::new(crate::PortableId::random(), 64, 64, DocumentNames { paint:"Paint".into(), paper:"Paper".into() })
        .with_memory(DocumentMemory::new(memory.clone(), None));
    let editor = Editor::new(document);
    let capture = editor.document().capture_artwork(Default::default());
    let before = memory.snapshot().ram_pinned_bytes;
    assert!(before > 0);
    drop(editor);
    assert_eq!(memory.snapshot().ram_pinned_bytes, before);
    drop(capture);
    assert_eq!(memory.snapshot().ram_physical_bytes, 0);
}

#[test]
fn scene_snapshot_keeps_import_bytes_after_undo_and_redo_retirement() {
    retained_import_snapshot(true);
}
#[test]
fn capture_keeps_import_bytes_after_undo_and_redo_retirement() {
    retained_import_snapshot(false);
}
fn retained_import_snapshot(scene:bool) {
    let memory=budget();
    let document=Document::new(crate::PortableId::random(),64,64,DocumentNames {paint:"Paint".into(),paper:"Paper".into()})
        .with_memory(DocumentMemory::new(memory.clone(),None));
    let mut editor=Editor::new(document);
    let SourceTarget::Paint(target)=editor.document().working.target.unwrap() else {panic!()};
    let mut paint=editor.document().artwork.paint.get(target).unwrap().clone();
    let source=source_grid(8,1);let bytes=source.resident_bytes() as u64;
    assert!(bytes>1024*1024);
    paint.domain=source.extent;paint.base=Some(crate::PaintBase::new(source.into()));
    editor.perform(Edit::Paint(RecordChange::replace(&editor.document().artwork.paint,target,Some(paint)).unwrap())).unwrap();
    let capture=if scene {None}else{Some(editor.capture(1,Default::default()).unwrap())};
    let snapshot=scene.then(||editor.document().scene().snapshot());
    let nested=snapshot.as_ref().map(|snapshot|snapshot.view().snapshot());
    assert!(memory.snapshot().ram_pinned_bytes<2*bytes,"capture shares physical source allocations");
    editor.undo().unwrap();
    let mut paint=editor.document().artwork.paint.get(target).unwrap().clone();
    paint.domain=[65,64];
    editor.perform(Edit::Paint(RecordChange::replace(&editor.document().artwork.paint,target,Some(paint)).unwrap())).unwrap();
    assert!(!editor.can_redo());
    assert!(memory.snapshot().ram_pinned_bytes>=bytes);
    let resident=editor.retained_tiles().resident_bytes() as u64;
    assert!(resident<bytes);
    drop(editor);
    assert!(memory.snapshot().ram_pinned_bytes>=bytes);
    drop(capture);
    drop(snapshot);
    if nested.is_some() {assert!(memory.snapshot().ram_pinned_bytes>=bytes);}
    drop(nested);
    assert_eq!(memory.snapshot().ram_physical_bytes,0);
}

#[test]
fn unchanged_frame_captures_share_the_current_inventory() {
    let memory=budget();
    let mut document=Document::new(crate::PortableId::random(),256,256,DocumentNames {paint:"Paint".into(),paper:"Paper".into()});
    let SourceTarget::Paint(target)=document.working.target.unwrap() else {panic!()};
    document.artwork.paint.get_mut(target).unwrap().base=Some(crate::PaintBase::new(source_grid(1,1).into()));
    let editor=Editor::new(document.with_memory(DocumentMemory::new(memory.clone(),None)));
    let (_store, inventory)=stored_inventory(&editor);
    let document_memory=&editor.document().memory;
    let current=document_memory.owner.inventories.lock().unwrap().current.clone();
    for _ in 0..10 {
        let capture=document_memory.capture_current();
        assert!(Arc::ptr_eq(&current,&capture._capture.as_ref().unwrap().inventory));
        drop(capture);
    }
    assert!(Arc::ptr_eq(&inventory,&current_inventory(&editor)));
    let capture=document_memory.capture_current();
    document_memory.owner.parked(true);
    let pinned=memory.snapshot().ram_pinned_bytes;
    assert!(pinned>0);
    drop(capture);
    memory.flush_reports();
    assert!(memory.snapshot().ram_pinned_bytes<pinned);
}

#[test]
fn ordinary_edits_share_capture_inventory_until_one_idle_flush() {
    let memory=budget();
    let mut document=Document::new(crate::PortableId::random(),256,256,DocumentNames {paint:"Paint".into(),paper:"Paper".into()});
    let SourceTarget::Paint(target)=document.working.target.unwrap() else {panic!()};
    document.artwork.paint.get_mut(target).unwrap().base=Some(crate::PaintBase::new(source_grid(1,1).into()));
    let mut editor=Editor::new(document.with_memory(DocumentMemory::new(memory,None)));
    let (_store, inventory)=stored_inventory(&editor);
    let current=editor.document().memory.owner.inventories.lock().unwrap().current.clone();
    let occurrence=editor.document().artwork.occurrences.iter().next().unwrap().0;
    let rename=|editor:&Editor,name:&str| {
        let mut value=editor.document().artwork.occurrences.get(occurrence).unwrap().clone();value.name=name.into();
        Edit::Occurrence(RecordChange::replace(&editor.document().artwork.occurrences,occurrence,Some(value)).unwrap())
    };
    editor.perform(rename(&editor,"Renamed")).unwrap();
    assert!(editor.undo().unwrap());
    assert!(editor.redo().unwrap());
    let preview=rename(&editor,"Preview");
    let inverse=editor.document().clone().apply(preview.clone()).unwrap();
    editor.preview(preview).unwrap();
    editor.cancel_preview(inverse).unwrap();
    assert!(editor.memory_report_pending());
    let capture=editor.document().memory.capture_current();
    assert!(Arc::ptr_eq(&current,&capture._capture.as_ref().unwrap().inventory));
    drop(editor.document().scene().snapshot());
    drop(capture);
    assert!(Arc::ptr_eq(&inventory,&current_inventory(&editor)));
    editor.flush_memory();
    assert!(!editor.memory_report_pending());
    assert!(!Arc::ptr_eq(&inventory,&current_inventory(&editor)));
    let inventory=current_inventory(&editor);
    editor.flush_memory();
    assert!(Arc::ptr_eq(&inventory,&current_inventory(&editor)));
}

#[test]
fn new_and_pending_raster_publications_defer_inventory_until_idle() {
    use crate::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileKey};
    for pending in [false,true] {
        let memory=budget();
        let mut document=Document::new(crate::PortableId::random(),2048,256,DocumentNames {paint:"Paint".into(),paper:"Paper".into()});
        let SourceTarget::Paint(target)=document.working.target.unwrap() else {panic!()};
        document.artwork.paint.get_mut(target).unwrap().base=Some(crate::PaintBase::new(source_grid(1,1).into()));
        let mut editor=Editor::new(document.with_memory(DocumentMemory::new(memory.clone(),None)));
        let (_store, inventory)=stored_inventory(&editor);
        let current=editor.document().memory.owner.inventories.lock().unwrap().current.clone();
        let before=memory.snapshot().ram_physical_bytes;
        let revision=if pending {RasterRevision::pending_within(4*1024*1024)} else {
            let descriptor=editor.document().composition().color.paint_descriptor();
            let tiles=(0..8).map(|x|(TileKey {plane:RasterPlane::Color,coordinate:[x,0]},RasterTile::pending(descriptor))).collect();
            RasterRevision::backed(RasterData {tiles,watercolor:None})
        };
        editor.perform(Edit::SetRaster {target:SourceTarget::Paint(target),revision:revision.clone()}).unwrap();
        assert!(editor.memory_report_pending());
        assert!(Arc::ptr_eq(&inventory,&current_inventory(&editor)));
        assert_eq!(memory.snapshot().reserved_bytes,0);
        assert_eq!(memory.snapshot().ram_physical_bytes,before);
        assert!(Arc::ptr_eq(&current,&editor.document().memory.owner.inventories.lock().unwrap().current));
        editor.flush_memory();
        assert!(!editor.memory_report_pending());
        assert!(!Arc::ptr_eq(&inventory,&current_inventory(&editor)));
        let capture=editor.document().memory.capture_current();
        let inventory=&capture._capture.as_ref().unwrap().inventory;
        assert!(!Arc::ptr_eq(&current,inventory));
        assert!(inventory.authored_rasters.contains(&revision.identity()));
        memory.flush_reports();
        let physical=memory.snapshot().ram_physical_bytes;
        assert!(physical>before+1024*1024);
        assert_eq!(physical,inventory.total_bytes() as u64);
    }
}

#[test]
fn reserved_import_commits_ownership_before_idle() {
    let (memory,mut editor,insert,inverse,reservation,bytes)=reserved_preview();
    editor.preview(inverse).unwrap();
    editor.perform_reserved(insert,reservation.clone()).unwrap();
    assert!(!reservation.active());
    assert_eq!(memory.snapshot().reserved_bytes,0);
    assert!(!editor.memory_report_pending());
    let capture=editor.document().memory.capture_current();
    assert!(capture._capture.as_ref().unwrap().inventory.resident_bytes() as u64>=bytes);
    assert!(memory.snapshot().ram_pinned_bytes>=bytes);
}

#[test]
fn undo_and_redo_reuse_the_retained_history_inventory_until_idle() {
    let (_,mut editor,insert,inverse,reservation,_)=reserved_preview();
    editor.preview(inverse).unwrap();
    editor.perform_reserved(insert,reservation).unwrap();
    let (_store, inventory)=stored_inventory(&editor);
    let current=editor.document().memory.owner.inventories.lock().unwrap().current.clone();
    assert!(editor.undo().unwrap());
    assert!(editor.memory_report_pending());
    assert!(editor.redo().unwrap());
    let capture=editor.document().memory.capture_current();
    assert!(Arc::ptr_eq(&current,&capture._capture.as_ref().unwrap().inventory));
    assert!(Arc::ptr_eq(&inventory,&current_inventory(&editor)));
    editor.flush_memory();
    assert!(!editor.memory_report_pending());
    assert!(!Arc::ptr_eq(&inventory,&current_inventory(&editor)));
}

#[test]
fn pressure_durability_releases_captured_tile_pins_before_a_store_read() {
    let memory = budget();
    let mut document = Document::new(crate::PortableId::random(), 512, 512,
        DocumentNames {paint: "Ink".into(), paper: "Paper".into()});
    let SourceTarget::Paint(target) = document.working.target.unwrap() else {panic!()};
    document.artwork.paint.get_mut(target).unwrap().base = Some(crate::PaintBase::new(source_grid(2, 2).into()));
    let mut editor = Editor::new(document.with_memory(DocumentMemory::new(memory.clone(), None)));
    let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
    let mut paint = editor.document().artwork.paint.get(target).unwrap().clone();
    paint.base = Some(crate::PaintBase::new(source_grid(1, 1).into()));
    editor.perform(Edit::Paint(RecordChange::replace(&editor.document().artwork.paint, target, Some(paint)).unwrap())).unwrap();
    let retained = editor.retained_tiles();
    let tile = retained.blobs().unwrap().into_iter().next().unwrap();
    let expected = tile.decode_on_worker().unwrap();
    let path = Files(std::env::temp_dir().join(format!("capy-durable-accounting-{}", crate::PortableId::random())));
    let reads = crate::tile_io::ReadCache::new(memory.clone());
    let mut store = pollster::block_on(package::session_store::SessionStore::open_with_cache(&path.0, reads)).unwrap();
    memory.flush_reports();
    let pressure = memory.admit_import(memory.import_headroom_bytes()).unwrap();
    assert_eq!(memory.import_headroom_bytes(), 0);
    let before = memory.snapshot().ram_pinned_bytes;
    let released = pollster::block_on(store.make_durable(&retained, &AtomicBool::new(false))).unwrap() as u64;
    assert!(released > 0);
    assert_eq!(retained.resident_bytes(), 0);
    memory.flush_reports();
    assert_eq!(memory.snapshot().ram_pinned_bytes, before - released,
        "durability must release cached owner pins while editor, history and capture remain alive");
    assert!(memory.import_headroom_bytes() >= released);
    assert_eq!(tile.decode_on_worker().unwrap(), expected);
    assert!(editor.can_undo());
    assert_eq!(capture.artwork().checkpoint.document, editor.document().artwork.id);
    assert!(pressure.active());
}
