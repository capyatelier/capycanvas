use super::*;
use crate::resident::Resident;
use layer_core::raster::{ContentName, RasterData, RasterPlane, TileKey};
use crate::raster::contents::Contents;
use std::sync::{Arc, atomic::{AtomicU8, Ordering}};

fn key() -> TileKey { TileKey { plane: RasterPlane::Color, coordinate: [0; 2] } }
fn contents() -> Contents { Contents::new(Arc::new(RasterData::default())) }

fn receipt(state: SubmissionState) -> Submission { Submission(Arc::new(AtomicU8::new(state as u8))) }

#[test]
fn native_capture_visits_only_changed_tree_coordinates_and_declared_outputs() {
    use layer_core::{color::DocumentColor,raster::{RasterTile,TileBlob}};
    use std::collections::BTreeSet;
    let descriptor=DocumentColor::default().paint_descriptor();
    let tile=RasterTile::backed(TileBlob::encode(descriptor,&vec![0;descriptor.byte_len([256;2]).unwrap()]).unwrap());
    let mut data=RasterData::default();
    for x in 0..15_000 {data.tiles.insert(TileKey {plane:RasterPlane::Color,coordinate:[x,0]},tile.clone());}
    let selected=Arc::new(data);let mut owner=Contents::new(selected.clone());
    let changed=TileKey {plane:RasterPlane::Color,coordinate:[73,0]};
    let planned=TileKey {plane:RasterPlane::Color,coordinate:[119,0]};
    let submitted=receipt(SubmissionState::Submitted);
    let mut physical=Resident::unwritten(std::cell::Cell::new(0u32));
    physical.initialize(&submitted).unwrap().publish(owner.version(changed));
    let write=physical.write(&owner.intent(changed),&submitted).unwrap();
    write.value().set(1);
    assert!(owner.written(write.publish(ContentName::fresh())));
    let mut visited=BTreeSet::new();
    let keys=crate::raster::native_edit::native_capture_candidates(&owner,[planned],|key| {
        assert!(visited.insert(key));
        if key==planned {DocumentColor {depth:layer_core::color::SampleDepth::U16,..Default::default()}.paint_descriptor()}
        else {key.plane.descriptor(DocumentColor::default())}
    });
    assert_eq!(keys,BTreeSet::from([changed,planned]));
    assert_eq!(visited,BTreeSet::from([changed,planned,
        TileKey {plane:RasterPlane::Mask,coordinate:changed.coordinate},
        TileKey {plane:RasterPlane::WatercolorWetness,coordinate:changed.coordinate}]));
    assert_eq!(owner.data().tiles.len(),15_000);
}

#[test]
fn resident_reads_and_destinations_require_the_selected_name_and_queue_order() {
    let owner = contents();
    let selected = owner.version(key());
    let edited = ContentName::fresh();
    let producer = receipt(SubmissionState::Encoding);
    let next = receipt(SubmissionState::Encoding);
    let mut resident = Resident::unwritten(7u32);
    assert!(resident.read(&selected, Some(&producer)).is_none());
    let write = resident.initialize(&producer).unwrap();
    assert_eq!(*write.value(), 7);
    write.publish(&selected);
    let read = resident.read(&selected, Some(&producer)).unwrap();
    assert_eq!(*read.value(), 7);
    assert_eq!(read.name(), selected.identity());
    assert!(read.submission().same(&producer));
    assert!(resident.read(&selected, Some(&next)).is_none());
    assert!(resident.read(&selected, None).is_none());
    producer.0.store(SubmissionState::Submitted as u8, Ordering::Release);
    assert!(resident.read(&selected, Some(&next)).is_some());
    assert!(resident.read(&selected, None).is_some());
    resident.write(&owner.intent(key()), &next).unwrap().publish(edited);
    assert!(resident.read(&selected, Some(&next)).is_none());
    assert!(resident.read(edited, Some(&next)).is_some());
    assert!(resident.initialize(&next).is_none());
}

#[test]
fn a_wrong_destination_waits_without_renaming_or_exposing_its_storage() {
    let owner = contents();
    let selected = owner.version(key());
    let old = ContentName::fresh();
    let submitted = receipt(SubmissionState::Submitted);
    let mut resident = Resident::unwritten(9u32);
    resident.initialize(&submitted).unwrap().publish(old);
    let before = crate::test_support::PassCounts::snapshot();
    assert!(resident.write(&owner.intent(key()), &submitted).is_none());
    assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).raster_write_defers, 1);
    assert_eq!(resident.name(), Some(old.stamp()));
    assert!(resident.read(&selected, Some(&submitted)).is_none());
    assert_eq!(*resident.read(old, Some(&submitted)).unwrap().value(), 9);
}

#[test]
fn physical_aliases_share_written_names_and_discard_restores_every_alias() {
    let mut owner = contents();
    let selected = owner.version(key());
    let submitted = receipt(SubmissionState::Submitted);
    let mut resident = Resident::unwritten(13u32);
    resident.initialize(&submitted).unwrap().publish(&selected);
    let historical = resident.clone();
    let changed = ContentName::fresh();
    let encoding = receipt(SubmissionState::Encoding);
    assert!(owner.written(resident.write(&owner.intent(key()), &encoding).unwrap().publish(changed)));
    assert!(historical.read(&selected, Some(&encoding)).is_none());
    assert!(historical.read(changed, Some(&encoding)).is_some());
    assert!(historical.read(changed, Some(&submitted)).is_none());
    encoding.0.store(SubmissionState::Discarded as u8, Ordering::Release);
    assert_eq!(resident.name(), Some(selected.identity()));
    assert_eq!(historical.name(), resident.name());
    assert!(historical.read(&selected, Some(&submitted)).is_some());
    assert_eq!(owner.version(key()).matches(&selected), Some(true));
}

#[test]
fn full_replacement_uses_a_verified_input_and_rolls_back_with_the_authoring_submission() {
    let mut owner=contents();let selected=owner.version(key());
    let source_name=ContentName::fresh();let produced=receipt(SubmissionState::Submitted);
    let mut source=Resident::unwritten(17u32);source.initialize(&produced).unwrap().publish(source_name);
    let authoring=receipt(SubmissionState::Encoding);
    let source_read=source.read(source_name,Some(&authoring)).unwrap();
    assert!(source_read.submission().same(&produced));
    assert!(owner.written(source_read.replace(&owner.intent(key()),&authoring)));
    assert_eq!(owner.version(key()).matches(&source_name.into()),Some(true));
    authoring.0.store(SubmissionState::Discarded as u8,Ordering::Release);
    assert_eq!(owner.version(key()).matches(&selected),Some(true));
    assert!(source.read(source_name,Some(&produced)).is_some());
}

#[test]
fn captured_input_versions_keep_undo_and_unrelated_footprints_independent() {
    let captured=layer_core::raster::RasterTile::pending(layer_core::color::DocumentColor::default().paint_descriptor());
    let selected=Arc::new(RasterData {tiles:[(key(),captured)].into(),..Default::default()});
    let mut owner=Contents::new(selected.clone());let before=owner.versions();
    assert!(!owner.modified([0;2]));assert!(owner.modified_coordinates().is_empty());
    let submitted=receipt(SubmissionState::Submitted);
    let mut physical=Resident::unwritten(23u32);
    physical.initialize(&submitted).unwrap().publish(owner.version(key()));
    assert!(owner.written(physical.write(&owner.intent(key()),&submitted).unwrap().publish(ContentName::fresh())));
    let after=owner.versions();let inside=crate::page_rect([0;2]);let outside=crate::page_rect([1,0]);
    assert!(owner.modified([0;2]));assert_eq!(owner.modified_coordinates(),[[0;2]].into());
    assert!(!owner.modified([1,0]));
    assert_eq!(before.region(RasterPlane::Color,inside).matches(&after.region(RasterPlane::Color,inside)),Some(false));
    assert_eq!(before.region(RasterPlane::Color,outside).matches(&after.region(RasterPlane::Color,outside)),Some(true));
    let mut changed=Vec::new();assert!(before.authored_changes(&after,RasterPlane::Color,|coordinate|changed.push(coordinate)));
    assert_eq!(changed,[[0;2]]);
    owner.select(selected);
    assert!(!owner.modified([0;2]));assert!(owner.modified_coordinates().is_empty());
    assert_eq!(before.root(RasterPlane::Color).matches(&owner.versions().root(RasterPlane::Color)),Some(true));
    assert_eq!(before.region(RasterPlane::Color,inside).matches(&after.region(RasterPlane::Color,inside)),Some(false));
}

#[test]
fn unresolved_packing_keeps_the_authored_input_snapshot_without_an_edit() {
    let mut owner=contents();let submitted=receipt(SubmissionState::Submitted);
    let mut physical=Resident::unwritten(29u32);
    physical.initialize(&submitted).unwrap().publish(owner.version(key()));
    assert!(owner.written(physical.write(&owner.intent(key()),&submitted).unwrap().publish(ContentName::fresh())));
    let before=owner.versions();
    let canonical=layer_core::raster::CanonicalName::new(owner.version(key()));
    assert!(owner.written(physical.write(&owner.intent(key()),&submitted).unwrap().publish(canonical.version())));
    let pending=owner.versions();let mut authored=Vec::new();
    assert!(before.authored_changes(&pending,RasterPlane::Color,|coordinate|authored.push(coordinate)));
    assert!(authored.is_empty());
    assert_eq!(before.root(RasterPlane::Color).matches(&pending.root(RasterPlane::Color)),None);
    canonical.resolve(false).unwrap();
    assert_eq!(before.root(RasterPlane::Color).matches(&pending.root(RasterPlane::Color)),Some(true));
}

#[test]
fn discarding_multiple_writes_in_one_encoder_restores_the_prior_content_name() {
    let mut owner = contents();
    let a = owner.version(key());
    let b = ContentName::fresh();
    let c = ContentName::fresh();
    let committed = receipt(SubmissionState::Complete);
    let discarded = receipt(SubmissionState::Encoding);
    let mut resident = Resident::unwritten(3u32);
    resident.initialize(&committed).unwrap().publish(&a);
    assert!(owner.written(resident.write(&owner.intent(key()), &discarded).unwrap().publish(b)));
    assert!(owner.written(resident.write(&owner.intent(key()), &discarded).unwrap().publish(c)));
    assert_eq!(resident.name(), Some(c.stamp()));
    discarded.0.store(SubmissionState::Discarded as u8, Ordering::Release);
    assert_eq!(resident.name(), Some(a.identity()));
    assert_eq!(owner.version(key()).matches(&a), Some(true));
    assert!(resident.read(b, Some(&committed)).is_none());
    assert!(resident.read(c, Some(&committed)).is_none());
    assert!(resident.read(&a, Some(&committed)).is_some());
}

#[test]
fn discarded_initialization_is_unwritten_and_a_later_failed_write_keeps_a_submitted_one() {
    let mut owner = contents();
    let a = owner.version(key());
    let b = ContentName::fresh();
    let failed = receipt(SubmissionState::Encoding);
    let mut resident = Resident::unwritten(5u32);
    resident.initialize(&failed).unwrap().publish(&a);
    failed.0.store(SubmissionState::Discarded as u8, Ordering::Release);
    assert_eq!(resident.name(), None);
    let submitted = receipt(SubmissionState::Submitted);
    resident.initialize(&submitted).unwrap().publish(&a);
    let failed = receipt(SubmissionState::Encoding);
    assert!(owner.written(resident.write(&owner.intent(key()), &failed).unwrap().publish(b)));
    failed.0.store(SubmissionState::Discarded as u8, Ordering::Release);
    assert_eq!(resident.name(), Some(a.identity()));
    assert_eq!(owner.version(key()).matches(&a), Some(true));
}

#[test]
fn canonical_comparison_pending_is_not_a_content_mismatch_or_an_implicit_rename() {
    use layer_core::raster::CanonicalName;
    let owner = contents();
    let before = owner.version(key());
    let name = CanonicalName::new(&before);
    let identity = name.version().identity();
    let submitted = receipt(SubmissionState::Submitted);
    let mut resident = Resident::unwritten(4u32);
    resident.initialize(&submitted).unwrap().publish(name.version());
    assert!(resident.read(name.version(), Some(&submitted)).is_some());
    assert!(resident.read(&before, Some(&submitted)).is_none());
    name.resolve(false).unwrap();
    let read = resident.read(&before, Some(&submitted)).unwrap();
    assert_eq!(read.version().resolved(), Some(before.identity()));
    assert_eq!(read.name(), identity);
    resident.write(&owner.intent(key()), &submitted).unwrap().publish(ContentName::fresh());
}

#[test]
fn an_old_selection_or_a_read_only_install_cannot_publish_authored_contents() {
    let mut owner = contents();
    let selected = owner.version(key());
    let submitted = receipt(SubmissionState::Submitted);
    let mut resident = Resident::unwritten(11u32);
    let installation = resident.initialize(&submitted).unwrap().publish(&selected);
    assert!(!owner.written(installation));
    assert_eq!(owner.version(key()).matches(&selected), Some(true));
    let mut foreign = contents();
    let encoded = receipt(SubmissionState::Encoding);
    let write = resident.write(&owner.intent(key()), &encoded).unwrap().publish(ContentName::fresh());
    assert!(!foreign.written(write), "equal pixels do not authorize another target's write");
    encoded.0.store(SubmissionState::Discarded as u8, Ordering::Release);
    let encoded = receipt(SubmissionState::Encoding);
    let write = resident.write(&owner.intent(key()), &encoded).unwrap().publish(ContentName::fresh());
    owner.select(owner.data().clone());
    assert!(!owner.written(write), "selection replacement retires an older write ticket even when pixels match");
    assert_eq!(owner.version(key()).matches(&selected), Some(true));
}

#[test]
fn replacing_an_immutable_paint_base_changes_its_default_inputs_and_retires_old_writes() {
    let mut doc = layer_core::Document::new(layer_core::PortableId::random(), 256, 256,
        layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let target = doc.working.target.unwrap();
    let layer_core::SourceTarget::Paint(paint) = target else { unreachable!() };
    let image = layer_core::authored::Image::new(layer_core::color::source::rgba8_source([1; 2], |_, _| [1, 2, 3, 255]));
    doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new(image.clone()));
    let mut owner = contents();
    owner.configure(doc.scene(), target);
    let before = owner.version(key());
    let submitted = receipt(SubmissionState::Submitted);
    let mut resident = Resident::unwritten(13u32);
    resident.initialize(&submitted).unwrap().publish(&before);
    let pending = receipt(SubmissionState::Encoding);
    let write = resident.write(&owner.intent(key()), &pending).unwrap().publish(ContentName::fresh());
    let replacement = layer_core::authored::Image::with_id(image.id(),
        layer_core::color::source::rgba8_source([1; 2], |_, _| [19, 23, 29, 255]));
    doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new(replacement));
    owner.configure(doc.scene(), target);
    assert_eq!(owner.version(key()).matches(&before), Some(false));
    assert!(!owner.written(write));
    pending.0.store(SubmissionState::Discarded as u8, Ordering::Release);
    assert!(resident.read(owner.version(key()), Some(&submitted)).is_none());
    doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new(image));
    owner.configure(doc.scene(), target);
    assert_eq!(owner.version(key()).matches(&before), Some(true));
    assert!(resident.read(owner.version(key()), Some(&submitted)).is_some());
}

#[test]
fn full_replacement_checks_the_current_alias_producer_even_when_the_name_is_unchanged() {
    let owner=contents();let selected=owner.version(key());let submitted=receipt(SubmissionState::Submitted);
    let mut source=Resident::unwritten(17u32);source.initialize(&submitted).unwrap().publish(&selected);
    let input=source.read(&selected,None).unwrap().input();let mut alias=source.clone();
    let other=receipt(SubmissionState::Encoding);alias.revise(&selected,&other).unwrap().publish(&selected);
    let consumer=receipt(SubmissionState::Encoding);let mut destination=Resident::unwritten(19u32);
    assert!(destination.overwrite_from(&owner.intent(key()),&input,&consumer).is_none());
    other.0.store(SubmissionState::Submitted as u8,Ordering::Release);
    assert!(destination.overwrite_from(&owner.intent(key()),&input,&consumer).is_some());
    let discarded=receipt(SubmissionState::Encoding);alias.revise(&selected,&discarded).unwrap().publish(&selected);
    assert!(destination.overwrite_from(&owner.intent(key()),&input,&consumer).is_none());
    discarded.0.store(SubmissionState::Discarded as u8,Ordering::Release);
    assert!(destination.overwrite_from(&owner.intent(key()),&input,&consumer).is_some());
}

#[test]
fn deferred_output_names_follow_the_encoder_that_actually_records_the_write() {
    let owner=contents();let selected=owner.version(key());let produced=receipt(SubmissionState::Submitted);
    let mut page=Resident::unwritten(23u32);page.initialize(&produced).unwrap().publish(&selected);
    let planned=receipt(SubmissionState::Encoding);let actual=receipt(SubmissionState::Encoding);let next=ContentName::fresh();
    let output=page.write(&owner.intent(key()),&planned).unwrap().encoded(next);
    planned.0.store(SubmissionState::Discarded as u8,Ordering::Release);
    assert!(page.read(&selected,None).is_some());
    let written=output.publish(&actual);
    assert!(written.submission().same(&actual));
    assert!(page.read(next,Some(&planned)).is_none());
    assert!(page.read(next,Some(&actual)).is_some());
    actual.0.store(SubmissionState::Discarded as u8,Ordering::Release);
    assert!(page.read(&selected,None).is_some());
}

#[test]
fn continuing_a_batched_channel_keeps_its_original_selected_write_authority() {
    let mut owner=contents();let selected=owner.version(key());let submitted=receipt(SubmissionState::Submitted);
    let mut page=Resident::unwritten(29u32);page.initialize(&submitted).unwrap().publish(&selected);
    let encoding=receipt(SubmissionState::Encoding);let first=ContentName::fresh();let final_name=ContentName::fresh();
    let first=page.write(&owner.intent(key()),&encoding).unwrap().publish(first);
    assert_eq!(owner.version(key()).matches(&selected),Some(true));
    let output=page.continue_written(&first,&encoding).unwrap().publish(final_name);
    assert!(owner.written(output));
    assert_eq!(owner.version(key()).matches(&final_name.into()),Some(true));
    assert!(page.continue_written(&first,&encoding).is_none());
}

#[test]
fn selecting_the_same_backing_still_restores_a_live_fork_before_publication() {
    let tile=layer_core::raster::RasterTile::pending(layer_core::color::DocumentColor::default().paint_descriptor());
    let neighbor=TileKey {coordinate:[1,0],..key()};
    let data=Arc::new(RasterData {tiles:[(key(),tile.clone()),(neighbor,tile)].into(),..Default::default()});
    let mut owner=Contents::new(data.clone());let selected=owner.version(key());
    let submitted=receipt(SubmissionState::Submitted);let mut pixels=Resident::unwritten(41u32);
    pixels.initialize(&submitted).unwrap().publish(&selected);
    let write=pixels.write(&owner.intent(key()),&submitted).unwrap().publish(ContentName::fresh());
    assert!(owner.written(write));
    assert_eq!(owner.selection_regions(&data,[512,256]),[crate::PixelRect::new(0,0,256,256)]);
    let before=owner.restorable_data();
    assert!(!before.tiles.contains_key(&key()));assert!(before.tiles.contains_key(&neighbor));
    owner.select(data);
    assert!(owner.selection_regions(owner.data(),[512,256]).is_empty());
    assert!(owner.restorable_data().tiles.contains_key(&key()));
}

#[test]
fn preview_expected_and_producer_share_write_once_captured_contact_inputs() {
    use crate::scene::{PreviewDeclaration,PreviewQuality,PreviewStage,PreviewPass};
    use layer_render::{Dab,DabBatchKind,DabStyle};
    let mut artwork=layer_core::Artwork::new([256;2]).unwrap();let (_,target)=crate::test_support::add_paint(&mut artwork,"Paint",[256;2]);let document=layer_core::Document::from_artwork(artwork).unwrap();
    let mut style=DabStyle::for_brush(&layer_core::BrushSnapshot::default(),layer_core::StrokeTool::Brush);style.rendering.accumulation=layer_core::BrushAccumulation::Uniform;
    let mut batch=crate::test_support::dab_batch(target,style,layer_core::Rect::from_extent([64;2]));batch.kind=DabBatchKind::Preview;
    let dab=Dab {center:layer_core::Point {x:32.,y:32.},radii:[8.;2],rotation:[1.,0.],motion:[0.;2],color_rgba_linear:[1.;4],flow:1.,hardness:1.,texture_sign:[1.;2],material:[0.;4],previous:[0.;4],contact:[0.;4],previous_contact:[0.;4]};
    let packet=layer_render::FramePacket {dab_batches:&[batch],dabs:&[dab],..crate::test_support::packet(document.scene(),[256;2])};
    let declaration=Arc::new(PreviewDeclaration::from_scene(packet,Default::default(),Default::default(),PreviewQuality::default(),None,None).unwrap().unwrap());
    assert!(matches!(declaration.version(target,RasterPlane::Color,[0;2],PreviewStage::Final),Err(crate::GpuRasterError::NotReady)));
    let producer=receipt(SubmissionState::Encoding);let name:layer_core::raster::ContentVersion=ContentName::fresh().into();let mut contact=Resident::unwritten(());contact.initialize(&producer).unwrap().publish(&name);
    declaration.accept_inputs(target,RasterPlane::Color,[0;2],PreviewStage::AfterBatch(0),PreviewPass::Color,vec![contact.read(&name,Some(&producer)).unwrap().input()]).unwrap();
    let final_name=declaration.version(target,RasterPlane::Color,[0;2],PreviewStage::Final).unwrap();
    let mut original=Contents::new(Arc::new(RasterData::default()));original.configure(document.scene(),target);let input=original.intent(key());
    let ticket=declaration.ticket_for_accepted(target,RasterPlane::Color,[0;2],PreviewStage::AfterBatch(0),PreviewPass::Color,input).unwrap();
    assert_eq!(final_name.matches(ticket.version()),Some(true));
    assert_eq!(ticket.intermediate(PreviewPass::Transport,0).version().matches(ticket.version()),Some(true));
    declaration.accept_inputs(target,RasterPlane::Color,[0;2],PreviewStage::AfterBatch(0),PreviewPass::Color,vec![contact.read(&name,Some(&producer)).unwrap().input()]).unwrap();
    let other:layer_core::raster::ContentVersion=ContentName::fresh().into();let mut conflicting=Resident::unwritten(());conflicting.initialize(&producer).unwrap().publish(&other);
    assert!(matches!(declaration.accept_inputs(target,RasterPlane::Color,[0;2],PreviewStage::AfterBatch(0),PreviewPass::Color,vec![conflicting.read(&other,Some(&producer)).unwrap().input()]),Err(crate::GpuRasterError::Effect(_))));
    assert_eq!(final_name.matches(&declaration.version(target,RasterPlane::Color,[0;2],PreviewStage::Final).unwrap()),Some(true));
}

#[test]
fn an_unbacked_live_write_is_not_the_selected_missing_page_default() {
    for plane in [RasterPlane::Color,RasterPlane::Mask,RasterPlane::WatercolorWetness] {
        let mut owner=contents();
        let key=TileKey {plane,coordinate:[7,3]};
        let selected=owner.selected_version(key);
        assert!(owner.data().tiles.get(&key).is_none());
        assert_eq!(owner.version(key).matches(&selected),Some(true));
        let producer=receipt(SubmissionState::Encoding);
        let mut physical=Resident::unwritten(());
        physical.initialize(&producer).unwrap().publish(&selected);
        let changed=ContentName::fresh();
        assert!(owner.written(physical.write(&owner.intent(key),&producer).unwrap().publish(changed)));
        assert!(owner.data().tiles.get(&key).is_none());
        assert_eq!(owner.version(key).matches(&selected),Some(false));
        assert!(physical.read(owner.version(key),Some(&producer)).is_some());
        assert!(physical.read(&selected,Some(&producer)).is_none());
        producer.0.store(SubmissionState::Discarded as u8,Ordering::Release);
        assert_eq!(owner.version(key).matches(&selected),Some(true));
    }
}
