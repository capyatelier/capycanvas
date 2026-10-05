use super::*;
use crate::{authored::*, color::{DocumentColor, SampleDepth, RgbSpace, ColorProfile, f16,
    source::{SourceImage, SourceKind, SourceInterpretation, SourceChannels}},
    raster::{RasterData, RasterPlane, RasterRevision, RasterTile, RasterWatercolor, TileBlob, TileKey, TILE_SIZE},
    Selection, SelectionPixels, EffectInstance, EffectValue, Lut3d, Point, PhotoMetadata};
use crate::package::{ByteSource, RangeState, transport::ChunkedBytes, MAX_RANGE_BYTES};
use std::{collections::BTreeSet, sync::{Mutex, atomic::AtomicUsize}};

#[path="admission.rs"]
mod admission;
#[path="optional.rs"]
mod optional;

fn identity(n: u128) -> PortableId { PortableId::from_bytes(n.to_be_bytes()) }
fn checkpoint(artwork: &Artwork) -> CaptureCheckpoint {
    CaptureCheckpoint {owner:17,document:artwork.id,session_generation:3,artwork_generation:7,working_generation:11,edit_checkpoint:13}
}
fn capture(artwork: &Artwork) -> ArtworkCapture { artwork.capture(checkpoint(artwork)).unwrap() }
fn preview() -> Preview { Preview::from_rgba([2,1],Arc::from([255,0,64,0,0,128,255,255])).unwrap() }
fn serialize(prepared: &PreparedPackage) -> Vec<u8> {
    let mut bytes=Vec::new(); prepared.write(&mut bytes,&AtomicBool::new(false)).unwrap(); bytes
}
fn prepare(artwork: &Artwork, with_preview: bool) -> PreparedPackage {
    let capture=capture(artwork);
    PreparedPackage::prepare(&capture,with_preview.then(||CapturedPreview {checkpoint:capture.checkpoint,context:capture.artwork.outputs.get(capture.artwork.default_output).unwrap().context.clone(),preview:preview()}),&AtomicBool::new(false)).unwrap()
}
fn backing(bytes: Vec<u8>) -> ImmutableBacking {
    let chunks=bytes.chunks(MAX_RANGE_BYTES).map(Arc::<[u8]>::from).collect();
    ImmutableBacking::new(Arc::new(ChunkedBytes::new(chunks).unwrap())).unwrap()
}
fn editable(bytes: Vec<u8>) -> Artwork {
    match open(backing(bytes),Default::default(),&AtomicBool::new(false)).unwrap() {
        OpenOutcome::Candidate {artwork,..}=>artwork, outcome=>panic!("expected editable artwork: {outcome:?}"),
    }
}
fn paint_samples(depth: SampleDepth) -> Vec<u8> {
    (0..TILE_SIZE*TILE_SIZE).flat_map(|pixel| {
        let values=if depth.is_float() {
            if pixel%2==0 {[if depth==SampleDepth::F16 {f16::from_bits(1).to_f32()} else {f32::from_bits(1)},-0.,-0.25,0.]}
            else {[if depth==SampleDepth::F16 {65504.} else {f32::MAX},2.125,0.5,1.]}
        } else if pixel%2==0 {[0.125,0.75,0.5,0.]} else {[1.,0.25,0.5,1.]};
        values.into_iter().flat_map(move |value| match depth {
            SampleDepth::U8=>vec![(value*255.) as u8], SampleDepth::U16=>((value*65535.) as u16).to_le_bytes().to_vec(),
            SampleDepth::F16=>f16::from_f32(value).to_le_bytes().to_vec(), SampleDepth::F32=>value.to_le_bytes().to_vec(),
        })
    }).collect()
}
fn fixture(depth: SampleDepth) -> Artwork {
    let mut artwork=Artwork::new([256,256]).unwrap();
    let color=DocumentColor {space:RgbSpace::DisplayP3,depth};
    let composition=artwork.compositions.get_mut(artwork.root).unwrap();
    composition.color=color; composition.origin=Point{x:-0.,y:1.125};
    composition.resolution=Some(crate::ImageResolution {unit:crate::ResolutionUnit::Inch,density:[[601,2],[300,1]]});
    let stack=composition.result;
    let paint=Arc::new(TileBlob::encode(color.paint_descriptor(),&paint_samples(depth)).unwrap());
    let coverage_size=color.coverage_descriptor().byte_len([256;2]).unwrap();
    let material=Arc::new(TileBlob::encode(color.coverage_descriptor(),&vec![0x55;coverage_size]).unwrap());
    let raster=RasterRevision::backed(RasterData {tiles:[
        (TileKey{plane:RasterPlane::Color,coordinate:[0,0]},RasterTile::backed_shared(paint)),
        (TileKey{plane:RasterPlane::WatercolorWetness,coordinate:[0,0]},RasterTile::backed_shared(material.clone())),
    ].into(),watercolor:Some(RasterWatercolor {wet_edge:0.25,burnt_edge:0.75,edge_width:3.5})});
    let interpretation=SourceInterpretation {channels:SourceChannels::Gray,depth:SampleDepth::U16,
        profile:ColorProfile::Icc(Resource::from((0..256).map(|n|n as u8).collect::<Vec<_>>())),profile_assumed:false};
    let source_bytes=(0..256*256).flat_map(|n|(n as u16).to_le_bytes()).collect::<Vec<_>>();
    let tile=TileBlob::encode(interpretation.descriptor(),&source_bytes).unwrap();
    let original=Arc::new(SourceImage {kind:SourceKind::Original,extent:[256;2],resolution:Some(crate::ImageResolution::ppi(300)),
        tiles:[([0,0],Arc::new(tile))].into(),interpretation});
    let source=PaintSource { color_mode: Default::default(),domain:[256;2],raster,original:Some(original),operations:Default::default()};
    let first=artwork.paint.insert(identity(10),source.clone()).unwrap();
    let second=artwork.paint.insert(identity(11),source).unwrap();
    let selection=Selection::pixels(Arc::new(SelectionPixels::bytes([5,2],[0,0,5,2],vec![0xff804020,0x7f,0x804020ff,1]).unwrap()));
    let mask=artwork.coverage.insert(identity(12),CoverageSource {operations:Default::default(),domain:[256;2],default_coverage:0.75,initial:Some(selection.clone()),
        raster:RasterRevision::backed(RasterData {tiles:[(TileKey{plane:RasterPlane::Mask,coordinate:[0,0]},RasterTile::backed_shared(material))].into(),watercolor:None})}).unwrap();
    let mut occurrence=Occurrence::new(OccurrenceContent::Paint(first),"Original source");
    occurrence.translation=Point{x:1.25,y:-0.}; occurrence.opacity=0.625; occurrence.locked=true;
    occurrence.mask=Some(MaskUse {source:mask,enabled:true,linked:false,inverted:true,translation:Point{x:2.,y:3.},placement:crate::Projective::IDENTITY});
    let a=artwork.occurrences.insert(identity(20),occurrence).unwrap();
    let b=artwork.occurrences.insert(identity(21),Occurrence::new(OccurrenceContent::Paint(second),"Independent copy")).unwrap();
    let saved=artwork.selections.insert(identity(13),SavedSelection {selection,}).unwrap();
    let c=artwork.occurrences.insert(identity(22),Occurrence::new(OccurrenceContent::Selection(saved),"Saved coverage")).unwrap();
    artwork.stacks.get_mut(stack).unwrap().entries=vec![a,b,c];
    let lut=Arc::new(Lut3d::from_samples(2,[[0.;3],[1.;3]],"Exact table".into(),vec![[0.125,0.75,0.5];8].into()).unwrap());
    for (index,key) in ["color_lookup","domain_warp"].into_iter().enumerate() {
        let mut instance=EffectInstance::new(crate::bundled_effect_catalog().get(key).unwrap().program());
        if key=="color_lookup" {instance.set("resource",EffectValue::Lut3d(Some(lut.clone()))).unwrap();}
        let definition=artwork.definitions.insert(identity(30+index as u128),Definition {program:instance.program.clone()}).unwrap();
        let effect=artwork.effects.insert(identity(40+index as u128),EffectApplication {definition,values:instance.values}).unwrap();
        let occurrence=artwork.occurrences.insert(identity(50+index as u128),Occurrence::new(OccurrenceContent::Effect(effect),key)).unwrap();
        artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
        Arc::make_mut(&mut artwork.outputs.get_mut(artwork.default_output).unwrap().context.phases).push((effect,2.125+index as f32));
    }
    artwork.guides.insert(identity(60),Guides {rulers:vec![(identity(61),crate::RulerGeometry::Radial {center:Point{x:12.5,y:8.}})]}).unwrap();
    artwork.metadata=Arc::new(PhotoMetadata {exif:Some(Resource::from(vec![0,255,17,5])),
        xmp:Some(Resource::from(b"<xmp>paint</xmp>".to_vec())),iptc:Some(Resource::from(vec![0x1c,2,120,0,1,42]))});
    let output=artwork.outputs.get_mut(artwork.default_output).unwrap();
    output.name="Captured output".into(); output.scale=[0.5,0.75];
    output.frame=Some((Point{x:-2.,y:3.5},[240,200])); output.sdr.exposure=0.5;
    artwork
}
fn rewrite(bytes: &[u8], change: impl FnOnce(&mut Value)) -> Vec<u8> { rewrite_with(bytes,Vec::new(),change) }
fn rewrite_with(bytes: &[u8], added: Vec<(String,Vec<u8>)>, change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let directory=Directory::read(&mut Cursor::new(bytes),262144,64*1024*1024).unwrap();
    let mut manifest:Value=serde_json::from_slice(&directory.read_member(&mut Cursor::new(bytes),directory.member("manifest.json").unwrap(),64*1024*1024).unwrap()).unwrap();
    change(&mut manifest);
    let changed=serde_json::to_vec(&manifest).unwrap();
    let mut inputs=directory.members.iter().map(|member| {
        let data=if member.name=="manifest.json" {changed.clone()} else {
            directory.read_member(&mut Cursor::new(bytes),member,MAX_RANGE_BYTES).unwrap()
        };
        (member.name.clone(),Cursor::new(data))
    }).chain(added.into_iter().map(|(name,data)|(name,Cursor::new(data)))).collect::<Vec<_>>();
    let mut members=inputs.iter_mut().map(|(name,input)|InputMember {name,length:input.get_ref().len() as u64,crc32:crc32fast::hash(input.get_ref()),input}).collect::<Vec<_>>();
    let mut output=Vec::new(); archive::write_archive(&mut output,&mut members,64*1024*1024).unwrap(); output
}

#[test]
fn full_archives_preserve_authored_graph_exact_samples_material_and_resource_bytes() {
    for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32] {
        let artwork=fixture(depth); let prepared=prepare(&artwork,true); let bytes=serialize(&prepared);
        let reopened=editable(bytes.clone());
        assert_eq!(reopened.id,artwork.id);
        let (loaded_shape,original_shape)=(reopened.topology().unwrap(),artwork.topology().unwrap());
        assert_eq!(loaded_shape.objects,original_shape.objects); assert_eq!(loaded_shape.outputs,original_shape.outputs);
        assert_eq!(loaded_shape.default_output,original_shape.default_output);
        assert_eq!(reopened.metadata,artwork.metadata);
        for (_,id,expected) in artwork.paint.iter() {
            let loaded=reopened.paint.get(reopened.paint.resolve(id).unwrap()).unwrap();
            assert_eq!(loaded.domain,expected.domain); assert_eq!(loaded.original,expected.original);
            let original_tile=&expected.original.as_ref().unwrap().tiles[&[0,0]];
            let loaded_tile=&loaded.original.as_ref().unwrap().tiles[&[0,0]];
            assert_eq!(loaded_tile.resource_id(),original_tile.resource_id());
            let (original,loaded)=(expected.raster.wait_data().unwrap(),loaded.raster.wait_data().unwrap());
            assert_eq!(loaded.watercolor,original.watercolor);
            for (key,tile) in &original.tiles {
                let (original,loaded)=(tile.wait_backing().unwrap(),loaded.tiles[key].wait_backing().unwrap());
                assert_eq!(loaded.resource_id(),original.resource_id()); assert_eq!(loaded.descriptor,original.descriptor);
                assert_eq!(loaded.decode().unwrap(),original.decode().unwrap()); assert_eq!(loaded.compressed().unwrap(),original.compressed().unwrap());
                if key.plane==RasterPlane::Color {assert_eq!(loaded.decode().unwrap(),paint_samples(depth));}
            }
        }
        for (_,id,expected) in artwork.selections.iter() {
            assert_eq!(reopened.selections.get(reopened.selections.resolve(id).unwrap()).unwrap(),expected);
        }
        let saved=reopened.paint.get(reopened.paint.resolve(identity(10)).unwrap()).unwrap().raster.wait_data().unwrap();
        let duplicate=reopened.paint.get(reopened.paint.resolve(identity(11)).unwrap()).unwrap().raster.wait_data().unwrap();
        for (key,tile) in &saved.tiles {assert!(Arc::ptr_eq(&tile.wait_backing().unwrap(),&duplicate.tiles[key].wait_backing().unwrap()));}
        assert_eq!(serialize(&prepare(&reopened,true)),bytes);
        assert_eq!(serialize(&prepare(&artwork,true)),bytes);
        let mut zip=zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        assert_eq!(zip.by_name("mimetype").unwrap().compression(),zip::CompressionMethod::Stored);
        assert!(prepared.resources().entries.iter().any(|entry|entry.record["type"]=="capy.icc/1"));
        assert!(prepared.resources().entries.iter().any(|entry|entry.record["type"]=="capy.lut3d/1"));
        assert!(!prepared.resources().entries.iter().any(|entry|entry.record["type"]=="capy.wgsl/1"));
    }
}

#[test]
fn preview_is_checkpoint_bound_and_does_not_replace_editable_authorship() {
    let artwork=fixture(SampleDepth::U8); let capture=capture(&artwork);
    let mut stale=capture.checkpoint; stale.artwork_generation+=1;
    for (provided,status,expected_preview) in [(None,PreviewStatus::Unavailable,false),
        (Some(CapturedPreview {checkpoint:stale,context:capture.artwork.outputs.get(capture.artwork.default_output).unwrap().context.clone(),preview:preview()}),PreviewStatus::Stale,false),
        (Some(CapturedPreview {checkpoint:capture.checkpoint,context:capture.artwork.outputs.get(capture.artwork.default_output).unwrap().context.clone(),preview:preview()}),PreviewStatus::Included,true)] {
        let prepared=PreparedPackage::prepare(&capture,provided,&AtomicBool::new(false)).unwrap();
        assert_eq!(prepared.checkpoint,capture.checkpoint); assert_eq!(prepared.preview_status,status);
        let bytes=serialize(&prepared);
        let directory=Directory::read(&mut Cursor::new(&bytes),262144,64*1024*1024).unwrap();
        assert_eq!(directory.member("preview.png").is_some(),expected_preview);
        match open(backing(bytes),Default::default(),&AtomicBool::new(false)).unwrap() {
            OpenOutcome::Candidate {artwork:restored,preview:provided,..}=> {
                assert_eq!(restored.id,artwork.id); assert_eq!(provided.is_some(),expected_preview);
                if let Some(provided)=provided {assert_eq!(provided.pixels(),preview().pixels());}
            }, outcome=>panic!("expected editable capture: {outcome:?}"),
        }
    }
    let mut wrong=capture; wrong.checkpoint.document=PortableId::random();
    assert!(PreparedPackage::prepare(&wrong,None,&AtomicBool::new(false)).is_err());
}

#[test]
fn preview_from_an_earlier_phase_is_stale_at_the_same_edit_checkpoint() {
    let editor=crate::Editor::new(crate::Document::from_artwork(fixture(SampleDepth::U8)).unwrap());
    let context=editor.document().artwork.outputs.get(editor.document().artwork.default_output).unwrap().context.clone();
    let first=editor.capture(3,context.clone()).unwrap();
    let mut later=context.clone();
    Arc::make_mut(&mut later.phases)[0].1+=1.;
    let second=editor.capture(3,later.clone()).unwrap();
    assert_eq!(first.checkpoint,second.checkpoint);
    let provided=CapturedPreview {checkpoint:first.checkpoint,context,preview:preview()};
    let prepared=PreparedPackage::prepare(&second,Some(provided),&AtomicBool::new(false)).unwrap();
    assert_eq!(prepared.preview_status,PreviewStatus::Stale);
    assert!(Directory::read(&mut Cursor::new(serialize(&prepared)),262144,64*1024*1024).unwrap().member("preview.png").is_none());
    let matching=CapturedPreview {checkpoint:second.checkpoint,context:later,preview:preview()};
    assert_eq!(PreparedPackage::prepare(&second,Some(matching),&AtomicBool::new(false)).unwrap().preview_status,PreviewStatus::Included);
}

#[test]
fn unsupported_known_fields_future_objects_and_shared_sources_preserve_original_bytes() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),true));
    let variants=[rewrite(&bytes,|manifest|manifest["version"]=json!(2)),
        rewrite(&bytes,|manifest|manifest["objects"].as_array_mut().unwrap().push(json!({"id":identity(999),"type":"future.paint/1","data":{}}))),
        rewrite(&bytes,|manifest|manifest["objects"].as_array_mut().unwrap().iter_mut().find(|record|record["id"]==json!(identity(20))).unwrap()["data"]["future_behavior"]=json!(true)),
        rewrite(&bytes,|manifest|manifest["objects"].as_array_mut().unwrap().iter_mut().find(|record|record["id"]==json!(identity(21))).unwrap()["data"]["content"]["paint"]=resources::reference(identity(10)))];
    for bytes in variants {
        let source=backing(bytes.clone());
        match open(source.clone(),Default::default(),&AtomicBool::new(false)).unwrap() {
            OpenOutcome::Preserved {source:original,preview,..}=>{
                assert_eq!(original.identity(),source.identity()); assert!(preview.is_some());
                let mut copied=Vec::new(); copy_original(&original,&mut copied,&AtomicBool::new(false)).unwrap(); assert_eq!(copied,bytes);
            },outcome=>panic!("unsupported package was not preserved: {outcome:?}"),
        }
    }
}

#[test]
fn future_record_types_and_selection_descriptors_preserve_the_package() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),true));
    for (collection,kind) in [("objects","capy.composition/1"),("objects","capy.stack/1"),
        ("objects","capy.occurrence/2"),("objects","capy.paint-source/1"),("objects","capy.coverage-source/1"),
        ("objects","capy.effect-definition/1"),("objects","capy.effect/1"),("objects","capy.selection/1"),
        ("objects","capy.guides/1"),("objects","capy.output/1"),("resources","capy.raster-tile/1"),
        ("resources","capy.selection-coverage/1"),("resources","capy.icc/1"),
        ("resources","capy.photo-metadata/1"),("resources","capy.lut3d/1")] {
        let changed=rewrite(&bytes,|manifest| {
            let records=manifest[collection].as_array_mut().unwrap();
            assert!(records.iter().any(|r|r["type"]==kind));
            for record in records.iter_mut().filter(|r|r["type"]==kind) {record["type"]=json!(format!("{kind}-future"));}
        });
        let OpenOutcome::Preserved {source,preview,..}=open(backing(changed.clone()),Default::default(),&AtomicBool::new(false)).unwrap()
            else {panic!("future {kind} must be preserved")};
        assert!(preview.is_some());
        let mut copied=Vec::new();copy_original(&source,&mut copied,&AtomicBool::new(false)).unwrap();assert_eq!(copied,changed);
    }
    for (key,value,preserved) in [("future_sampling",json!(true),true),("depth",json!("f32"),true),
        ("chunk",json!(7),false),("bounds",json!([0,0,4,2]),false)] {
        let changed=rewrite(&bytes,|manifest| {
            for record in manifest["resources"].as_array_mut().unwrap().iter_mut().filter(|r|r["type"]=="capy.selection-coverage/1") {
                record["data"][key]=value.clone();
            }
        });
        let outcome=open(backing(changed),Default::default(),&AtomicBool::new(false)).unwrap();
        if preserved {assert!(matches!(outcome,OpenOutcome::Preserved {..}),"{key}: {outcome:?}");}
        else {assert!(matches!(outcome,OpenOutcome::RecoveredView {..}),"{key}: {outcome:?}");}
    }
    let changed=rewrite(&bytes,|manifest| {
        for record in manifest["resources"].as_array_mut().unwrap().iter_mut().filter(|r|r["type"]=="capy.raster-tile/1") {
            record["type"]=json!("capy.icc/1");
        }
    });
    assert!(matches!(open(backing(changed),Default::default(),&AtomicBool::new(false)).unwrap(),OpenOutcome::RecoveredView {..}));
}

#[test]
fn selection_admission_preserves_a_valid_package() {
    let mut artwork=Artwork::new([8,1]).unwrap();
    artwork.selections.insert(identity(1),SavedSelection {selection:Selection::pixels(Arc::new(
        SelectionPixels::bytes([8,1],[0,0,8,1],vec![u32::MAX;2]).unwrap()))}).unwrap();
    let bytes=serialize(&prepare(&artwork,false));
    for limits in [crate::ProjectLimits {raster_bytes:3,..Default::default()},crate::ProjectLimits {tiles:0,..Default::default()}] {
        let OpenOutcome::Preserved {source,..}=open(backing(bytes.clone()),limits,&AtomicBool::new(false)).unwrap()
            else {panic!("selection admission must preserve the package")};
        let mut copied=Vec::new();copy_original(&source,&mut copied,&AtomicBool::new(false)).unwrap();assert_eq!(copied,bytes);
        assert_eq!(editable(copied).selections.len(),1);
    }
}

#[test]
fn corrupt_authorship_returns_only_a_verified_preview_and_latches_resource_failure() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),true));
    for mutation in [0,1] {
        let mut broken=bytes.clone();
        let directory=Directory::read(&mut Cursor::new(&broken),262144,64*1024*1024).unwrap();
        let name=if mutation==0 {"manifest.json"} else {"data/tiles-1.bin"};
        broken[directory.member(name).unwrap().offset as usize]^=1;
        let source=backing(broken.clone()); let owner=source.clone();
        match open(source,Default::default(),&AtomicBool::new(false)).unwrap() {
            OpenOutcome::RecoveredView {preview:recovered,reason,..}=>{
                assert_eq!(recovered.pixels(),preview().pixels()); assert!(!reason.is_empty());
                if mutation==1 {assert!(reason.contains("checksum")); assert_eq!(owner.poll(0,1).unwrap_err(),reason);}
            },outcome=>panic!("corrupt authorship became editable: {outcome:?}"),
        }
        let mut copied=Vec::new();
        copy_original(&owner,&mut copied,&AtomicBool::new(false)).unwrap();
        assert_eq!(copied,broken);
        if mutation==1 {assert!(owner.poll(0,1).is_err());}
    }
    let mut without_preview=serialize(&prepare(&fixture(SampleDepth::U8),false));
    let directory=Directory::read(&mut Cursor::new(&without_preview),262144,64*1024*1024).unwrap();
    without_preview[directory.member("manifest.json").unwrap().offset as usize]^=1;
    assert!(matches!(open(backing(without_preview),Default::default(),&AtomicBool::new(false)).unwrap(),OpenOutcome::Failure {..}));
    let cyclic=rewrite(&bytes,|manifest| {
        let stack=manifest["objects"].as_array().unwrap().iter().find(|record|record["type"]=="capy.stack/1").unwrap()["id"].clone();
        let occurrence=manifest["objects"].as_array_mut().unwrap().iter_mut().find(|record|record["id"]==json!(identity(20))).unwrap();
        occurrence["data"]["content"]=json!({"stack":{"ref":stack}});
    });
    assert!(matches!(open(backing(cyclic),Default::default(),&AtomicBool::new(false)).unwrap(),OpenOutcome::RecoveredView {..}));
}

#[test]
fn outputless_artwork_is_preserved_after_validating_its_known_records() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),false));
    let bytes=rewrite(&bytes,|manifest| {
        manifest["outputs"]=json!([]);
        manifest.as_object_mut().unwrap().remove("default_output");
        manifest["objects"].as_array_mut().unwrap().retain(|record|record["type"]!="capy.output/1");
    });
    let cancel=AtomicBool::new(false);
    let OpenOutcome::Preserved{source,outputs,..}=open(backing(bytes.clone()),Default::default(),&cancel).unwrap() else {panic!("Outputless artwork must be preserved")};
    assert!(outputs.is_empty());
    let mut copied=Vec::new();copy_original(&source,&mut copied,&cancel).unwrap();assert_eq!(copied,bytes);
    let invalid=rewrite(&bytes,|manifest| {
        manifest["objects"].as_array_mut().unwrap().iter_mut().find(|record|record["id"]==json!(identity(20))).unwrap()["data"]["opacity"]=json!(-1);
    });
    assert!(matches!(open(backing(invalid),Default::default(),&cancel).unwrap(),OpenOutcome::Failure{..}));
}

#[test]
fn reopened_animation_can_be_deleted_or_merged_and_undone() {
    let mut document=crate::Document::new(PortableId::random(),16,16,crate::DocumentNames{paint:"Ink".into(),paper:"Paper".into()});
    let occurrence=document.scene().order()[1];
    let effect=document.scene().effect_handle(occurrence).unwrap();
    let definition=document.artwork.effects.get(effect).unwrap().definition;
    let program=crate::bundled_effect_catalog().get("domain_warp").unwrap().program();
    document.artwork.effects.get_mut(effect).unwrap().values=EffectInstance::new(program.clone()).values;
    document.artwork.definitions.get_mut(definition).unwrap().program=program;
    let capture=crate::Editor::new(document).capture(0,EvaluationContext{elapsed:2.,phases:vec![(effect,0.75)].into()}).unwrap();
    let original=editable(serialize(&PreparedPackage::prepare(&capture,None,&AtomicBool::new(false)).unwrap()));
    let before=serialize(&prepare(&original,false));
    for merge in [false,true] {
        let document=crate::Document::from_artwork(original.clone()).unwrap();
        let occurrence=document.scene().order()[1];
        let effect=document.scene().effect_handle(occurrence).unwrap();
        let definition=document.artwork.effects.get(effect).unwrap().definition;
        let edit=if merge {crate::Edit::Batch(document.merge_plan(crate::MergeKind::Flatten).unwrap().edits)}
            else {document.delete_layers_edit(&[occurrence]).unwrap()};
        let mut editor=crate::Editor::new(document);editor.perform(edit).unwrap();
        assert!(editor.document().output().context.phases.is_empty());
        assert!(editor.document().artwork.definitions.get(definition).is_none());
        assert!(editor.undo().unwrap());
        assert_eq!(serialize(&prepare(&editor.document().artwork,false)),before);
        assert!(editor.redo().unwrap());
        assert!(editor.document().output().context.phases.is_empty());
        assert!(editor.document().artwork.definitions.get(definition).is_none());
    }
}

#[test]
fn effect_removal_and_replacement_reclaim_only_newly_unused_definitions() {
    let artwork=editable(serialize(&prepare(&fixture(SampleDepth::U8),false)));
    let mut document=crate::Document::from_artwork(artwork).unwrap();
    let occurrence=document.artwork.occurrences.resolve(identity(51)).unwrap();
    let application=document.scene().effect_application(occurrence).unwrap().clone();
    let unused=document.artwork.definitions.insert(identity(90),document.artwork.definitions.get(application.definition).unwrap().clone()).unwrap();
    let (edit,copies)=document.duplicate_layers_edit(&[occurrence]).unwrap();document.apply(edit).unwrap();
    document.apply(document.delete_layers_edit(&[occurrence]).unwrap()).unwrap();
    assert!(document.artwork.definitions.get(application.definition).is_some());
    let before=document.artwork.clone();
    for target in [identity(30),identity(90)] {
        let mut editor=crate::Editor::new(document.clone());
        let effect=editor.document().scene().effect_handle(copies[0]).unwrap();
        let definition=editor.document().artwork.definitions.resolve(target).unwrap();
        let values=EffectInstance::new(editor.document().artwork.definitions.get(definition).unwrap().program.clone()).values;
        let replacement=EffectApplication{definition,values};
        let changes=editor.document().effect_edits(vec![RecordChange::replace(&editor.document().artwork.effects,effect,Some(replacement)).unwrap()]).unwrap();
        editor.perform(crate::Edit::Batch(changes)).unwrap();
        assert!(editor.document().artwork.definitions.get(application.definition).is_none());
        assert!(editor.document().artwork.definitions.get(unused).is_some());
        let reopened=editable(serialize(&prepare(&editor.document().artwork,false)));
        assert!(reopened.definitions.resolve(identity(31)).is_none());
        assert!(reopened.definitions.resolve(identity(90)).is_some());
        assert!(editor.undo().unwrap());assert_eq!(editor.document().artwork,before);
        assert!(editor.redo().unwrap());assert!(editor.document().artwork.definitions.get(application.definition).is_none());
    }
}

#[test]
fn custom_filters_stay_out_of_portable_files() {
    let mut document=crate::Document::new(PortableId::random(),16,16,crate::DocumentNames{paint:"Ink".into(),paper:"Paper".into()});
    let bytes=serialize(&prepare(&document.artwork,false));
    let paper=document.scene().effect_handle(document.scene().order()[1]).unwrap();
    let definition=document.artwork.effects.get(paper).unwrap().definition;
    let program=crate::effect_catalog::custom_program("solid_color");
    document.artwork.definitions.get_mut(definition).unwrap().program=program.clone();
    let error=PreparedPackage::prepare(&capture(&document.artwork),None,&AtomicBool::new(false)).unwrap_err();
    assert_eq!(error,"Drawings with custom filters can't be saved yet");
    let mut inventory=resources::ResourceInventory::default();
    let custom=super::super::effect_records::encode_definition(&Definition{program:program.clone()},&mut inventory).unwrap();
    let sources=program.wgsl.sources().unwrap();
    let records:Vec<_>=sources.iter().map(|source|json!({"id":source.id(),"type":"capy.wgsl/1","data":{},"encoding":"utf8",
        "location":{"member":format!("data/{}",source.id())},"bytes":source.len().to_string(),"crc32":format!("{:08x}",crc32fast::hash(source.as_bytes()))})).collect();
    let members=sources.iter().map(|source|(format!("data/{}",source.id()),source.as_bytes().to_vec())).collect();
    let id=json!(document.artwork.definitions.id(definition).unwrap());
    let bytes=rewrite_with(&bytes,members,|manifest| {
        manifest["objects"].as_array_mut().unwrap().iter_mut().find(|record|record["id"]==id).unwrap()["data"]=custom;
        manifest["resources"].as_array_mut().unwrap().extend(records);
    });
    let OpenOutcome::Preserved {reason,..}=open(backing(bytes),Default::default(),&AtomicBool::new(false)).unwrap() else {panic!("custom filters must open read-only")};
    assert_eq!(reason,"Custom filters are unsupported");
}
#[test]
fn effects_resize_by_declared_dimension_independently_of_display_units() {
    for dimension in [Dimension::Scalar,Dimension::Angle,Dimension::Time,Dimension::SourcePixels,Dimension::CompositionPixels,Dimension::Normalized] {
        for unit in ["","px"] {
            let mut document=crate::Document::new(PortableId::random(),16,16,crate::DocumentNames{paint:"Ink".into(),paper:"Paper".into()});
            let effect=document.scene().effect_handle(document.scene().order()[1]).unwrap();
            let definition=document.artwork.effects.get(effect).unwrap().definition;
            let mut program=crate::effect_catalog::custom_program("gaussian_blur");
            let parameters=Arc::make_mut(&mut Arc::make_mut(&mut program).parameters);
            let sigma=parameters.iter().position(|p|p.key.as_ref()=="sigma").unwrap();
            parameters[sigma].dimension=dimension;
            let crate::EffectParameterKind::Number{unit:label,..}=&mut parameters[sigma].kind else {unreachable!()};*label=unit.into();
            let mut instance=EffectInstance::new(program.clone());instance.set("sigma",EffectValue::Number(3.)).unwrap();
            document.artwork.definitions.get_mut(definition).unwrap().program=program;
            document.artwork.effects.get_mut(effect).unwrap().values=instance.values;
            let mut editor=crate::Editor::new(crate::Document::from_artwork(document.artwork).unwrap());
            let geometry=crate::CanvasGeometry::resize([16,16],[32,32],crate::Interpolation::Bicubic);
            let plan=editor.document().canvas_geometry_plan(&geometry,crate::GeometryLimits{project:Default::default(),device_dimension:8192}).unwrap();
            editor.perform(crate::Edit::Batch(plan.edits)).unwrap();
            let effect=editor.document().scene().effect(editor.document().scene().order()[1]).unwrap();
            let expected=if matches!(dimension,Dimension::SourcePixels|Dimension::CompositionPixels){6.}else{3.};
            assert_eq!(effect.value("sigma"),Some(&EffectValue::Number(expected)),"{dimension:?}, {unit}");
            assert!(editor.undo().unwrap());
            assert_eq!(editor.document().scene().effect(editor.document().scene().order()[1]).unwrap().value("sigma"),Some(&EffectValue::Number(3.)));
        }
    }
}

#[test]
fn cancellation_never_publishes_an_archive_and_does_not_poison_valid_owners() {
    let artwork=fixture(SampleDepth::U8); let cancelled=AtomicBool::new(true);
    assert!(PreparedPackage::prepare(&capture(&artwork),None,&cancelled).unwrap_err().contains("cancelled"));
    let prepared=prepare(&artwork,true); let mut bytes=Vec::new();
    assert!(prepared.write(&mut bytes,&cancelled).unwrap_err().contains("cancelled")); assert!(bytes.is_empty());
    struct CancelWriter<'a> {bytes:Vec<u8>,cancelled:&'a AtomicBool}
    impl Write for CancelWriter<'_> {
        fn write(&mut self,bytes:&[u8])->std::io::Result<usize> {self.bytes.extend_from_slice(bytes);self.cancelled.store(true,Ordering::Relaxed);Ok(bytes.len())}
        fn flush(&mut self)->std::io::Result<()> {Ok(())}
    }
    cancelled.store(false,Ordering::Relaxed);
    let mut output=CancelWriter {bytes:Vec::new(),cancelled:&cancelled};
    assert!(prepared.write(&mut output,&cancelled).is_err());
    assert!(Directory::read(&mut Cursor::new(&output.bytes),262144,64*1024*1024).is_err());
    let source=backing(serialize(&prepared));
    assert!(open(source.clone(),Default::default(),&cancelled).unwrap_err().contains("cancelled"));
    let mut copy=Vec::new(); assert!(copy_original(&source,&mut copy,&cancelled).is_err()); assert!(copy.is_empty());
    cancelled.store(false,Ordering::Relaxed);
    assert!(matches!(open(source,Default::default(),&cancelled).unwrap(),OpenOutcome::Candidate {..}));
}

struct CountingSource {bytes:ChunkedBytes,requests:Mutex<Vec<(u64,usize)>>,drops:Arc<AtomicUsize>}
impl ByteSource for CountingSource {
    fn byte_len(&self)->u64 {self.bytes.byte_len()}
    fn poll(&self,offset:u64,length:usize)->Result<RangeState,String> {
        self.requests.lock().unwrap().push((offset,length)); self.bytes.poll(offset,length)
    }
}
impl Drop for CountingSource {fn drop(&mut self) {self.drops.fetch_add(1,Ordering::Relaxed);}}

#[test]
fn opaque_attachment_ranges_are_bounded_and_live_until_the_last_captured_save_owner() {
    let mut artwork=fixture(SampleDepth::U8);
    let payload:Arc<[u8]>=vec![0x91;MAX_RANGE_BYTES+31].into();
    let drops=Arc::new(AtomicUsize::new(0));
    let source=Arc::new(CountingSource {bytes:ChunkedBytes::new(payload.chunks(MAX_RANGE_BYTES).map(Arc::<[u8]>::from).collect()).unwrap(),
        requests:Mutex::new(Vec::new()),drops:drops.clone()});
    let source_owner=Arc::downgrade(&source);
    let attachment=Arc::new(OpaqueResource {id:identity(701),kind:"future.samples/1".into(),data:json!({"mode":3}),encoding:"future.binary/1".into(),
        extra_fields:[("future_descriptor".into(),json!([1,2,3]))].into_iter().collect(),backing:ImmutableBacking::new(source.clone()).unwrap(),offset:0,
        length:payload.len() as u64,crc32:crc32fast::hash(&payload)});
    Arc::make_mut(&mut artwork.extensions).resources.insert(attachment.id,attachment.clone());
    Arc::make_mut(&mut artwork.extensions).records.insert(identity(700),json!({"id":identity(700),"type":"future.note/1","ancillary":true,"copy_safe":true,
        "data":{"subject":resources::reference(identity(10)),"payload":resources::reference(attachment.id)}}));
    let capture=capture(&artwork); let prepared=PreparedPackage::prepare(&capture,None,&AtomicBool::new(false)).unwrap();
    drop(artwork); drop(capture); drop(attachment); drop(source);
    assert_eq!(drops.load(Ordering::Relaxed),0); assert!(source_owner.upgrade().is_some());
    let bytes=serialize(&prepared);
    let reopened=editable(bytes);
    assert_eq!(reopened.extensions.resources.len(),1);
    let loaded=&reopened.extensions.resources[&identity(701)];
    assert_eq!(loaded.data,json!({"mode":3})); assert_eq!(loaded.extra_fields["future_descriptor"],json!([1,2,3]));
    let live=BTreeSet::from_iter(reopened.paint.iter().map(|(_,id,_)|id));
    assert_eq!(reopened.extensions.edited_retained(&live).unwrap().1.len(),1);
    let mut emitted=Vec::new(); loaded.verify(&AtomicBool::new(false)).unwrap();
    for start in (0..payload.len()).step_by(MAX_RANGE_BYTES) {
        emitted.extend_from_slice(&loaded.read_chunk(start as u64,(payload.len()-start).min(MAX_RANGE_BYTES),&AtomicBool::new(false)).unwrap());
    }
    assert_eq!(emitted,&*payload);
    assert!(source_owner.upgrade().unwrap().requests.lock().unwrap().iter().all(|(_,length)|*length<=MAX_RANGE_BYTES));
    drop(prepared); assert_eq!(drops.load(Ordering::Relaxed),1); assert!(source_owner.upgrade().is_none());
}

#[test]
fn valid_preview_remains_independent_when_its_png_is_corrupt_or_mismatched() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),true));
    let mut corrupted=bytes.clone();
    let directory=Directory::read(&mut Cursor::new(&corrupted),262144,64*1024*1024).unwrap();
    corrupted[directory.member("preview.png").unwrap().offset as usize+25]^=1;
    match open(backing(corrupted),Default::default(),&AtomicBool::new(false)).unwrap() {
        OpenOutcome::Candidate {preview,..}=>assert!(preview.is_none()), outcome=>panic!("preview corruption damaged authored content: {outcome:?}"),
    }
    let mismatched=rewrite(&bytes,|manifest| {
        manifest["objects"].as_array_mut().unwrap().iter_mut().find(|record|record["type"]=="capy.output/1").unwrap()["data"]["representation"]["size"]=json!([1,1]);
    });
    match open(backing(mismatched),Default::default(),&AtomicBool::new(false)).unwrap() {
        OpenOutcome::Candidate {preview,..}=>assert!(preview.is_none()), outcome=>panic!("mismatched preview replaced authored content: {outcome:?}"),
    }
}

#[path = "roundtrip_semantics.rs"]
mod roundtrip_semantics;

#[path="current_design.rs"]
mod current_design;
