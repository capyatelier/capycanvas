fn u16_source(extent: [u32; 2], profile: layer_core::color::ColorProfile, profile_assumed: bool, max_bytes: usize, row: &[u8]) -> layer_core::color::source::SourceImage {
    use layer_core::color::{SampleDepth, source::*};
    let mut builder = SourceBuilder::new(extent, SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::U16, profile, profile_assumed,
    }, max_bytes).unwrap();
    for _ in 0..extent[1] { builder.push_row(row).unwrap(); }
    builder.finish().unwrap()
}

use layer_core::color::source::rgba8_source;
use std::sync::Arc;

#[test]
fn image_placement_touch_claims_photo_handles_but_preserves_camera_contacts_outside() {
    let source = Arc::unwrap_or_clone(rgba8_source([20, 10], |_, _| [255; 4]));
    let mut session = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new(layer_core::authored::PortableId::random(), 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }), [800, 600], Platform::Gtk).unwrap();
    session.place_layer_source("Photo", source, None).unwrap();
    let input = |id, phase, position| pointer_input(id, phase, PointerKind::Touch, PointerButton::Primary, position, 0);
    assert!(!session.input(input(1, ContactPhase::Down, [10., 10.])).unwrap().paint);
    assert!(!session.input(input(2, ContactPhase::Down, [400., 300.])).unwrap().paint,
        "second contact joins camera navigation even over the photo");
    session.input(input(1, ContactPhase::Up, [10., 10.])).unwrap();
    session.input(input(2, ContactPhase::Up, [400., 300.])).unwrap();
    assert!(session.input(input(3, ContactPhase::Down, [400., 300.])).unwrap().paint);
    assert!(session.input(input(3, ContactPhase::Move, [410., 300.])).unwrap().paint);
    assert!(session.input(input(3, ContactPhase::Up, [410., 300.])).unwrap().paint);
    assert!(session.interaction.pointer.is_none());
    invoke(&mut session, CommandId::CancelTransform);
    assert!(!session.input(input(4, ContactPhase::Down, [400., 300.])).unwrap().paint,
        "ordinary single-finger contact still does not paint");
}

#[test]
fn image_placement_context_keeps_drop_point_and_rejects_changed_targets() {
    let mut session = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new(layer_core::authored::PortableId::random(), 2000, 1500, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }), [800, 600], Platform::Gtk).unwrap();
    let point = Point { x: 410., y: 280. };
    let expected = session.state.camera.input_transform().map(point);
    let context = session.image_placement_context(Some(point), None).unwrap();
    session.state.camera.zoom *= 2.;
    session.validate_image_placement(&context).unwrap();
    assert_eq!(context.center, Some(expected), "camera changes cannot retarget a queued drop");
    let paper = session.engine.document().scene().order()[1];
    session.engine.apply_edit(session.engine.document().select_occurrence_edit(paper).unwrap()).unwrap();
    assert!(session.validate_image_placement(&context).is_err());
    let context = session.image_placement_context(None, None).unwrap();
    let ink = session.engine.document().scene().order()[0];
    let mut occurrence = session.engine.document().scene().occurrence(ink).unwrap().clone(); occurrence.opacity = 0.5;
    session.engine.apply_edit(layer_core::Edit::Occurrence(layer_core::authored::RecordChange::replace(&session.engine.document().artwork.occurrences, ink, Some(occurrence)).unwrap())).unwrap();
    assert!(session.validate_image_placement(&context).is_err());
    let context = session.image_placement_context(None, None).unwrap();
    session.state.document_file.epoch += 1;
    assert!(session.validate_image_placement(&context).is_err());
    let (images, edit) = session.engine.document().create_object_layer_edit("Images", layer_core::ImageObject::new(rgba8_source([2, 1], |_, _| [255; 4]).into()), None, 0).unwrap();
    session.engine.apply_edit(edit).unwrap();
    session.engine.apply_edit(session.engine.document().select_occurrence_edit(images).unwrap()).unwrap();
    let context = session.image_placement_context(None, None).unwrap();
    assert_eq!(session.engine.document().active_target(), None);
    session.engine.apply_edit(session.engine.document().select_occurrence_edit(paper).unwrap()).unwrap();
    assert_eq!(session.engine.document().active_target(), None);
    assert!(session.validate_image_placement(&context).is_err(), "choosing another layer without paint still retires the import");
}

#[test]
fn image_batch_placement_creates_ordered_sibling_layers_in_one_atomic_edit() {
    let source = |extent| Arc::unwrap_or_clone(rgba8_source(extent, |_, _| [255; 4]));
    let first = source([600, 400]);
    let second = source([100, 300]);
    let mut photo = Document::new(layer_core::authored::PortableId::random(), 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    Arc::make_mut(&mut photo.artwork.metadata).xmp = Some(b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>".as_slice().into());
    let mut session = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        photo, [800, 600], Platform::Gtk).unwrap();
    session.set_selected_layers(session.engine.document().scene().order().iter().copied().collect()).unwrap();
    select(&mut session, rectangle([10., 20., 80., 90.]));
    let selected = session.engine.document().working.layer_selection.clone();
    let original = session.engine.document().clone();
    let checkpoint = session.engine.checkpoint();
    assert!(session.place_layer_sources(vec![("First".into(), first.clone()), ("\n".into(), second.clone())],
        None, None).is_err());
    assert_eq!(session.engine.document(), &original, "failed batch reserves no live IDs and inserts nothing");
    let images = || vec![("First".into(), first.clone()), ("Second".into(), second.clone())];
    session.place_layer_sources(images(), Some(Point { x: 75., y: 55. }), None).unwrap();
    let doc = session.engine.document();
    let layer = doc.working.occurrence.unwrap();
    let layers: Vec<_> = doc.scene().order().iter().copied().filter(|h| doc.working.layer_selection.contains(h)).collect();
    let children: Vec<_> = layers.iter().map(|h| doc.scene().object_handle(*h).unwrap()).collect();
    assert_eq!(session.engine.backend().moving_layer, Some(layers[0]));
    assert_eq!(layers.iter().map(|h| doc.scene().occurrence(*h).unwrap().name.as_ref()).collect::<Vec<_>>(), ["First", "Second"]);
    assert!(layers.contains(&layer));
    assert_eq!(doc.working.layer_selection, layers.iter().copied().collect());
    assert_eq!(doc.selected_objects(), children.iter().copied().collect());
    assert!(doc.working.selection.is_none(), "placement moves the imported images as a whole");
    assert_eq!(session.engine.checkpoint(), checkpoint);
    let before: Vec<_> = children.iter().map(|h| doc.scene().object(*h).unwrap().affine).collect();
    let third = 1. / 3.;
    assert_eq!(before, [layer_core::Affine64([third, 0., 0., third, 75. - 600. * third * 0.5, 55. - 400. * third * 0.5]), layer_core::Affine64([0.5, 0., 0., 0.5, 50., -20.])]);
    invoke(&mut session, CommandId::TransformFlipHorizontal);
    assert!(children.iter().zip(&before).all(|(h, start)| session.engine.document().scene().object(*h).unwrap().affine != *start));
    invoke(&mut session, CommandId::ResetTransform);
    assert_eq!(children.iter().map(|h| session.engine.document().scene().object(*h).unwrap().affine).collect::<Vec<_>>(), before, "Reset restores exact initial affines");
    assert_eq!(session.engine.checkpoint(), checkpoint, "provisional reset adds no history");
    invoke(&mut session, CommandId::CancelTransform);
    assert_eq!(session.engine.backend().moving_layer, None);
    assert_live_artwork_eq(session.engine.document(), &original);
    assert_eq!(session.engine.document().working.layer_selection, selected);
    assert_eq!(session.engine.document().working.selection, original.working.selection);
    assert_eq!(session.engine.checkpoint(), checkpoint);

    session.place_layer_sources(images(), None, None).unwrap();
    invoke(&mut session, CommandId::ApplyTransform);
    let committed = session.engine.document().clone();
    assert_eq!(session.engine.backend().moving_layer, None);
    invoke(&mut session, CommandId::Undo);
    assert_live_artwork_eq(session.engine.document(), &original);
    assert_eq!(session.engine.checkpoint(), checkpoint, "the whole batch is one artwork history entry");
    assert_eq!(session.engine.document().working.selection, original.working.selection);
    invoke(&mut session, CommandId::Redo);
    assert_live_artwork_eq(session.engine.document(), &committed);
    let restored = reopen_capture(&session.capture_artwork().unwrap());
    for (h, portable, object) in committed.artwork.objects.iter() {
        let restored = restored.scene().object(restored.artwork.objects.resolve(portable).unwrap()).unwrap();
        assert_eq!(restored.affine.0.map(f64::to_bits), object.affine.0.map(f64::to_bits), "{h:?}");
        assert_eq!(restored.image.as_ref(), object.image.as_ref());
    }
    assert_eq!(restored.artwork.metadata, original.artwork.metadata, "placing images keeps the document's own metadata");
}

#[test]
fn photo_drop_destination_respects_groups_locks_clipping_and_parent_offsets() {
    use crate::{ImageLayerDestination, LayerDropPosition};
    use layer_core::authored::*;
    let mut doc = Document::new(PortableId::random(), 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let ink = doc.working.occurrence.unwrap();
    let paper = doc.scene().order()[1];
    let nested = doc.artwork.stacks.insert(PortableId::random(), Stack { entries: vec![ink] }).unwrap();
    let mut group = Occurrence::new(OccurrenceContent::Stack(nested), "Group");
    group.offset = [40, -10];
    let group_id = doc.artwork.occurrences.insert(PortableId::random(), group).unwrap();
    let canvas = doc.composition().size;
    let paint = doc.artwork.paint.insert(PortableId::random(), PaintSource { color_mode: Default::default(), domain: canvas, raster: Default::default(), base: None, operations: Arc::default() }).unwrap();
    let mut clipped = Occurrence::new(OccurrenceContent::Paint(paint), "Clipped"); clipped.attachment = layer_core::Attachment::Clip;
    let clipped_id = doc.artwork.occurrences.insert(PortableId::random(), clipped).unwrap();
    doc.artwork.stacks.get_mut(nested).unwrap().entries.insert(0, clipped_id);
    let root = doc.composition().result; doc.artwork.stacks.get_mut(root).unwrap().entries = vec![group_id, paper];
    let mut doc = Document::from_artwork(doc.artwork).unwrap();
    doc.apply(doc.select_occurrence_edit(ink).unwrap()).unwrap();
    let mut session = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [800, 600], Platform::Gtk).unwrap();
    let source = Arc::unwrap_or_clone(rgba8_source([2, 1], |_, _| [255; 4]));
    assert_eq!(session.image_layer_drop_hint(occurrence_token(group_id), 0.5), Some(LayerDropPosition::Into));
    assert_eq!(session.image_layer_drop_hint(occurrence_token(paper), 0.9), Some(LayerDropPosition::Below));
    assert_eq!(session.image_layer_drop_hint(occurrence_token(ink), 0.1), Some(LayerDropPosition::Above));
    assert_eq!(session.image_layer_drop_hint(occurrence_token(clipped_id), 0.9), Some(LayerDropPosition::Below));
    assert_eq!(session.image_layer_drop_hint(occurrence_token(clipped_id), 0.1), Some(LayerDropPosition::Above));
    assert!(session.image_placement_context(None,Some(ImageLayerDestination{target:group_id,position:LayerDropPosition::Attach})).is_err());
    let original = session.engine.document().clone();
    let placed_layer = |session: &UiSession<Recorder>| session.engine.document().working.occurrence.unwrap();
    session.place_layer_sources(vec![("Outside import".into(), source.clone())], None,
        Some(ImageLayerDestination { target: clipped_id, position: LayerDropPosition::Above })).unwrap();
    assert!(session.engine.document().scene().object_layer(placed_layer(&session)).is_some());
    assert_eq!(session.engine.document().scene().occurrence(session.engine.document().working.occurrence.unwrap()).unwrap().attachment, layer_core::Attachment::None);
    invoke(&mut session, CommandId::CancelTransform);
    assert_live_artwork_eq(session.engine.document(), &original);
    session.place_layer_sources(vec![("Clipped import".into(), source.clone())], None,
        Some(ImageLayerDestination { target: ink, position: LayerDropPosition::Above })).unwrap();
    let imported = session.engine.document().working.occurrence.unwrap();
    assert_eq!(session.engine.document().scene().clipping_base(imported), Some(ink));
    assert_eq!(session.engine.document().scene().clipping_base(clipped_id), Some(ink));
    invoke(&mut session, CommandId::CancelTransform);
    assert_live_artwork_eq(session.engine.document(), &original);
    session.place_layer_sources(vec![("Menu import".into(), source.clone())], None, None).unwrap();
    let menu = session.engine.document().scene().order()[1];
    assert_eq!(menu, placed_layer(&session), "default Import goes above the complete clipped stack");
    assert_eq!(session.engine.document().scene().occurrence(menu).unwrap().name.as_ref(), "Menu import");
    assert_eq!(session.engine.document().scene().occurrence(session.engine.document().scene().order()[2]).unwrap().attachment, layer_core::Attachment::Clip);
    invoke(&mut session, CommandId::CancelTransform);
    assert_live_artwork_eq(session.engine.document(), &original);
    for position in [LayerDropPosition::Into, LayerDropPosition::Below] {
        session.place_layer_sources(vec![("Photo".into(), source.clone())], None,
            Some(ImageLayerDestination { target: group_id, position })).unwrap();
        let doc = session.engine.document();
        assert_eq!(doc.scene().occurrence(doc.working.occurrence.unwrap()).unwrap().attachment, layer_core::Attachment::None);
        assert_eq!(doc.scene().parent(doc.working.occurrence.unwrap()), (position == LayerDropPosition::Into).then_some(group_id));
        let object = *doc.selected_objects().iter().next().unwrap();
        assert_eq!(doc.object_document_affine(object).unwrap().map([1., 0.5]), [100., 75.]);
        assert_eq!(doc.scene().position(doc.working.occurrence.unwrap()).unwrap(),
            if position == LayerDropPosition::Into { 1 } else { 3 });
        invoke(&mut session, CommandId::CancelTransform);
        assert_live_artwork_eq(session.engine.document(), &original);
    }
    let mut group = session.engine.document().scene().occurrence(group_id).unwrap().clone();
    group.locked = true;
    session.engine.apply_edit(layer_core::Edit::Occurrence(RecordChange::replace(&session.engine.document().artwork.occurrences, group_id, Some(group)).unwrap())).unwrap();
    assert_eq!(session.image_layer_drop_hint(occurrence_token(group_id), 0.5), None);
    assert_eq!(session.image_layer_drop_hint(occurrence_token(ink), 0.1), None);
    assert_eq!(session.image_layer_drop_hint(occurrence_token(group_id), 0.1), Some(LayerDropPosition::Above));
    assert!(session.place_layer_sources(vec![("Photo".into(), source)], None,
        Some(ImageLayerDestination { target: group_id, position: LayerDropPosition::Into })).is_err());
}

#[test]
fn rejected_photo_placement_start_keeps_the_previous_tool_and_selection() {
    let mut doc = Document::new(layer_core::authored::PortableId::random(), 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let layer_core::authored::SourceTarget::Paint(paint) = doc.working.target.unwrap() else { panic!("paint") };
    let source = doc.artwork.paint.get_mut(paint).unwrap(); source.domain = [2, 1]; source.base = Some(layer_core::PaintBase::new(layer_core::Image::new(rgba8_source([2, 1], |_, _| [255; 4]))));
    doc.artwork.occurrences.get_mut(doc.working.occurrence.unwrap()).unwrap().locked = true;
    let mut session = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [800, 600], Platform::Gtk).unwrap();
    let before = session.engine.document().clone();
    let selected = session.engine.document().working.layer_selection.clone();
    let tool = session.layer_interaction.tool;
    assert!(session.dispatch(UiAction::Invoke { command: CommandId::ScaleRotate }).is_err());
    assert!(!session.operation.active());
    assert_eq!(session.layer_interaction.tool, tool);
    assert_eq!(session.engine.document().working.layer_selection, selected);
    assert_eq!(session.engine.document(), &before);
    assert!(session.capture_artwork().is_ok(), "failed start must not block Save/recovery");
}

#[test]
fn image_placement_centres_cancel_apply_and_one_step_history() {
    let source = u16_source([600, 400], Default::default(), false, 8 * 1024 * 1024, &[255; 600 * 8]);
    let mut session = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new(layer_core::authored::PortableId::random(), 200, 150, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }), [800, 600], Platform::Gtk).unwrap();
    let original = session.engine.document().clone();
    session.place_layer_source("Photo", source.clone(), None).unwrap();
    let object = *session.engine.document().selected_objects().iter().next().unwrap();
    let third = 1. / 3.;
    assert_eq!(session.engine.document().scene().object(object).unwrap().affine, layer_core::Affine64([third, 0., 0., third, 100. - 600. * third * 0.5, 75. - 400. * third * 0.5]));
    assert!(session.objects.placing());
    assert!(!session.engine.can_undo(), "provisional import has no artwork history");
    assert!(session.capture_artwork().is_err(), "pending placement cannot enter recovery/save");
    invoke(&mut session, CommandId::CancelTransform);
    assert_live_artwork_eq(session.engine.document(), &original);
    assert_eq!(session.engine.document().working.occurrence.unwrap(), original.working.occurrence.unwrap());
    assert!(!session.engine.can_undo());

    session.place_layer_source("Photo", source.clone(), Some(Point { x: 60.5, y: 80.5 })).unwrap();
    let second = *session.engine.document().selected_objects().iter().next().unwrap();
    assert_ne!(session.engine.document().artwork.objects.id(second), original.artwork.objects.iter().next().map(|(_, id, _)| id), "cancelled IDs are never reused");
    let placed = session.engine.document().scene().object(second).unwrap().clone();
    assert_eq!(placed.affine, layer_core::Affine64([third, 0., 0., third, 60.5 - 600. * third * 0.5, 80.5 - 400. * third * 0.5]));
    invoke(&mut session, CommandId::ApplyTransform);
    assert!(!session.objects.placing());
    invoke(&mut session, CommandId::Undo);
    assert_live_artwork_eq(session.engine.document(), &original);
    assert!(!session.engine.can_undo(), "insertion and placement are one history entry");
    invoke(&mut session, CommandId::Redo);
    assert_eq!(session.engine.document().scene().object(second).unwrap(), &placed);

    let portable = session.engine.document().artwork.objects.id(second).unwrap();
    let restored = reopen_capture(&session.capture_artwork().unwrap());
    let restored = restored.scene().object(restored.artwork.objects.resolve(portable).unwrap()).unwrap().clone();
    assert_eq!(restored.affine.0.map(f64::to_bits), placed.affine.0.map(f64::to_bits));
    assert_eq!((restored.image.extent, restored.image.interpretation.clone()), (source.extent, source.interpretation.clone()));
    let rows = |image: &layer_core::color::source::SourceImage| {
        let mut reader = image.rows();
        (0..image.extent[1]).map(|y| { let mut row = vec![0; image.row_bytes()]; reader.read(y, &mut row).unwrap(); row }).collect::<Vec<_>>()
    };
    assert_eq!(rows(&restored.image), rows(&source), "the reopened image keeps its exact samples");
}

#[test]
fn retained_import_transform_clear_and_undo_keep_source_precision() {
    use layer_core::color::{ColorProfile, RgbSpace};
    let row: Vec<u8> = [
        65535u16, 0, 1023, 1, 123, 45678, 32101, 0, 3, 65534, 65535, 32767,
    ]
    .into_iter()
    .flat_map(u16::to_le_bytes)
    .collect();
    let source = u16_source([3, 2], ColorProfile::Builtin(RgbSpace::DisplayP3), false, 1024 * 1024, &row);
    let mut session = session(Platform::Gtk);
    let before = session.engine.document().clone();
    assert!(
        session
            .import_layer_source("Photo", source.clone())
            .unwrap_err()
            .contains("does not support")
    );
    assert_eq!(session.engine.document(), &before);
    session.engine.backend_mut().tiled_sources = true;
    assert!(
        session
            .import_layer_source("bad\nname", source.clone())
            .is_err()
    );
    assert_eq!(session.engine.document(), &before);
    session
        .import_layer_source("Photo", source.clone())
        .unwrap();
    let id = session.engine.document().working.occurrence.unwrap();
    let check = |session: &UiSession<Recorder>| {
        assert_eq!(session.engine.document().composition().color, Default::default());
        let paint = session.engine.document().scene().paint_source(id).unwrap();
        assert_eq!(paint.base.as_ref().map(|base|base.image.as_ref()), Some(&source));
        assert!(paint.raster.is_empty());
    };
    check(&session);
    assert!(session.command(CommandId::ScaleRotate).enabled);
    let revision = session.engine.document().revision;
    invoke(&mut session, CommandId::ScaleRotate);
    assert!(session.operation.active());
    invoke(&mut session, CommandId::CancelTransform);
    assert_eq!(session.engine.document().revision, revision);
    check(&session);
    session
        .dispatch(UiAction::Layer {
            action: LayerAction::Clear { id: occurrence_token(id) },
        })
        .unwrap();
    assert!(
        session
            .engine
            .document()
            .scene().paint_source(id)
            .unwrap()
            .base
            .is_none()
    );
    invoke(&mut session, CommandId::Undo);
    check(&session);
    invoke(&mut session, CommandId::Redo);
    assert!(
        session
            .engine
            .document()
            .scene().paint_source(id)
            .unwrap()
            .base
            .is_none()
    );
    invoke(&mut session, CommandId::Undo);
    invoke(&mut session, CommandId::Undo);
    assert!(session.engine.document().scene().occurrence(id).is_none());
    assert_eq!(session.engine.document().working.occurrence.unwrap(), before.working.occurrence.unwrap());
    invoke(&mut session, CommandId::Redo);
    check(&session);
    let portable = session.engine.document().artwork.occurrences.id(id).unwrap();
    let restored = reopen_capture(&session.capture_artwork().unwrap());
    let handle = restored.artwork.occurrences.resolve(portable).unwrap();
    assert_eq!(restored.scene().paint_source(handle).unwrap().base.as_ref().map(|base|base.image.as_ref()), Some(&source));
}

#[test]
fn source_profile_repair_preserves_samples_and_baked_edits() {
    use layer_core::{color::{ColorProfile, RgbSpace}, raster::*, Edit, RecordChange, CoverageSource, MaskUse};
    let samples: Vec<u8> = [65535u16, 12345, 54321, 1].into_iter().flat_map(u16::to_le_bytes).collect();
    let source = u16_source([1, 1], ColorProfile::Builtin(RgbSpace::Srgb), true, 1024 * 1024, &samples);
    let mut session = session(Platform::Gtk);
    session.engine.backend_mut().tiled_sources = true;
    session.import_layer_source("Original", source).unwrap();
    let id = session.engine.document().working.occurrence.unwrap();
    let token = occurrence_token(id);
    let paint = match session.engine.document().scene().occurrence(id).unwrap().content { OccurrenceContent::Paint(paint) => paint, _ => unreachable!() };
    let change = session.dispatch(UiAction::Layer { action: LayerAction::RepairSourceProfile { id: token } }).unwrap();
    assert_ne!(change.regions & regions::HOST, 0, "native dialog must be serviced");
    let request_id = session.state.requests.last().unwrap().id;
    session.complete_document_request(request_id, Ok(false)).unwrap();
    let original = session.engine.document().artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.storage().clone();
    let mut corrected = (*original).clone();
    corrected.interpretation.profile = ColorProfile::Builtin(RgbSpace::DisplayP3);
    corrected.interpretation.profile_assumed = false;
    let before = session.engine.document().clone();
    let mut invalid = corrected.clone(); invalid.extent[0] += 1;
    assert!(session.repair_layer_source(SourceUse::Paint(id), &original, invalid).is_err());
    assert_eq!(session.engine.document(), &before);
    let mut preview = session.preview_layer_source(SourceUse::Paint(id), &original, corrected.clone()).unwrap();
    assert_eq!(session.engine.document(), &before, "preview does not mutate live content or IDs");
    let original_image = before.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.id();
    assert_ne!(preview.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.id(), original_image);
    let original_preview_revision = session.state.layers.iter().find(|layer| layer.id == token).unwrap().paint_revision;
    assert_eq!(session.repair_layer_source(SourceUse::Paint(id), &original, corrected.clone()).unwrap(), SourceUse::Paint(id));
    let accepted_image = session.engine.document().artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.id();
    assert_ne!(accepted_image, original_image);
    let base = preview.artwork.paint.get_mut(paint).unwrap().base.as_mut().unwrap();
    base.image = layer_core::Image::with_id(accepted_image, base.image.storage().clone());
    assert_eq!(preview.artwork, session.engine.document().artwork, "detached previews preserve every authored property except the newly allocated image identity");
    assert_eq!(preview.working, session.engine.document().working);
    let corrected_preview_revision = session.state.layers.iter().find(|layer| layer.id == token).unwrap().paint_revision;
    assert_ne!(corrected_preview_revision, original_preview_revision, "repair must refresh the Layers image even when source samples and raster history are unchanged");
    let after = session.engine.document().artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.storage();
    assert_eq!(after.interpretation, corrected.interpretation);
    assert!(Arc::ptr_eq(after.tiles.values().next().unwrap(), original.tiles.values().next().unwrap()));
    assert!(session.repair_layer_source(SourceUse::Paint(id), &original, corrected.clone()).unwrap_err().contains("source changed"));
    invoke(&mut session, CommandId::Undo);
    assert!(Arc::ptr_eq(session.engine.document().artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.storage(), &original));
    invoke(&mut session, CommandId::Redo);
    assert_eq!(session.engine.document().artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.id(), accepted_image);
    invoke(&mut session, CommandId::Undo);
    assert_ne!(session.state.layers.iter().find(|layer| layer.id == token).unwrap().paint_revision, corrected_preview_revision, "Undo must refresh the restored source interpretation");
    let document = session.engine.document();
    let mut occurrence = document.scene().occurrence(id).unwrap().clone();
    occurrence.offset = [4, 9];
    let coverage = RecordChange::insert(&document.artwork.coverage, CoverageSource { domain: original.extent, raster: Default::default(), default_coverage: 1., operations: Default::default() });
    occurrence.mask = Some(MaskUse { source: coverage.handle, enabled: true, linked: true, inverted: false, offset: [0, 0] });
    let descriptor = document.composition().color.paint_descriptor();
    let bytes = vec![55; descriptor.byte_len([TILE_SIZE; 2]).unwrap()];
    let key = TileKey { plane: RasterPlane::Color, coordinate: [0, 0] };
    let mut source = document.artwork.paint.get(paint).unwrap().clone();
    source.raster = RasterRevision::backed(RasterData { tiles: [(key, RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()))].into(), watercolor: None });
    let edit = Edit::Batch(vec![Edit::Coverage(coverage), Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences, id, Some(occurrence.clone())).unwrap()), Edit::Paint(RecordChange::replace(&document.artwork.paint, paint, Some(source.clone())).unwrap())]);
    session.engine.apply_edit(edit).unwrap();
    let original_document = session.engine.document().clone();
    let (preview, edit) = session.prepare_source_edit(SourceUse::Paint(id), &original, Arc::new(corrected.clone()), false).unwrap();
    assert_eq!(session.engine.document(), &original_document);
    let next_id = preview.working.occurrence.unwrap();
    assert_ne!(next_id, id);
    session.commit_prepared_source_edit(edit).unwrap();
    assert_eq!(&preview, session.engine.document());
    let doc = session.engine.document();
    assert_eq!(doc.scene().occurrence(id).unwrap(), &occurrence, "baked occurrence and its mask stay intact");
    assert_eq!(doc.artwork.paint.get(paint).unwrap(), &source, "baked pixels and retained source stay intact");
    let next = doc.scene().occurrence(next_id).unwrap();
    let next_paint = match next.content { OccurrenceContent::Paint(paint) => paint, _ => unreachable!() };
    assert_eq!(next.offset, occurrence.offset);
    assert_eq!(doc.artwork.paint.get(next_paint).unwrap().base.as_ref().map(|base|base.image.as_ref()), Some(&corrected));
    assert!(doc.artwork.paint.get(next_paint).unwrap().raster.is_empty());
    assert!(next.mask.is_none());
    assert_eq!(doc.working.occurrence, Some(next_id));
    let old_identity = doc.artwork.paint.id(paint).unwrap();
    let new_identity = doc.artwork.paint.id(next_paint).unwrap();
    let reopened = reopen_capture(&session.capture_artwork().unwrap());
    let old_paint = reopened.artwork.paint.resolve(old_identity).unwrap();
    let new_paint = reopened.artwork.paint.resolve(new_identity).unwrap();
    assert_eq!(reopened.artwork.paint.get(old_paint).unwrap().base.as_ref().map(|base|base.image.as_ref()), Some(original.as_ref()));
    assert_eq!(reopened.artwork.paint.get(new_paint).unwrap().base.as_ref().map(|base|base.image.as_ref()), Some(&corrected));
    assert_eq!(reopened.artwork.paint.get(old_paint).unwrap().raster.wait_data().unwrap().tiles[&key].wait_backing().unwrap().decode().unwrap(), bytes);
    invoke(&mut session, CommandId::Undo);
    assert!(session.engine.document().scene().occurrence(next_id).is_none());
    assert_eq!(session.engine.document().scene().occurrence(id).unwrap(), &occurrence);
    assert_eq!(session.engine.document().artwork.paint.get(paint).unwrap(), &source);
    assert_eq!(session.engine.document().working.occurrence, Some(id));
    invoke(&mut session, CommandId::Redo);
    assert_eq!(session.engine.document().scene().occurrence(id).unwrap(), &occurrence);
    assert_eq!(session.engine.document().artwork.paint.get(paint).unwrap(), &source);
    assert_eq!(session.engine.document().artwork.paint.get(next_paint).unwrap().base.as_ref().map(|base|base.image.as_ref()), Some(&corrected));
}

#[test]
fn rasterizing_an_image_preserves_full_extent_edits_masks_and_history() {
    use layer_core::{color::source::*, raster::*, Edit, RecordChange, CoverageSource, MaskUse};
    let mut session = session(Platform::Gtk); session.engine.backend_mut().tiled_sources = true;
    let mut builder = SourceBuilder::new([1500, 2], SourceInterpretation { channels: SourceChannels::Rgba, depth: Default::default(), profile: Default::default(), profile_assumed: true }, 1024 * 1024).unwrap();
    for _ in 0..2 { builder.push_row(&vec![127; 1500 * 4]).unwrap(); }
    session.import_layer_source("Reference", builder.finish().unwrap()).unwrap();
    let document = session.engine.document();
    let id = document.working.occurrence.unwrap();
    let mut occurrence = document.scene().occurrence(id).unwrap().clone();
    let paint = match occurrence.content { OccurrenceContent::Paint(paint) => paint, _ => unreachable!() };
    let original = document.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.storage().clone();
    occurrence.offset = [-550, 3];
    let coverage = RecordChange::insert(&document.artwork.coverage, CoverageSource { domain: original.extent, raster: Default::default(), default_coverage: 1., operations: Default::default() });
    occurrence.mask = Some(MaskUse { source: coverage.handle, enabled: true, linked: true, inverted: false, offset: [17, 3] });
    let key = TileKey { plane: RasterPlane::Color, coordinate: [0, 0] };
    let mut source = document.artwork.paint.get(paint).unwrap().clone();
    source.raster = RasterRevision::backed(RasterData { tiles: [(key, RasterTile::backed(TileBlob::encode(document.composition().color.paint_descriptor(), &vec![51; 256 * 256 * 4]).unwrap()))].into(), watercolor: None });
    let edit = Edit::Batch(vec![Edit::Coverage(coverage), Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences, id, Some(occurrence.clone())).unwrap()), Edit::Paint(RecordChange::replace(&document.artwork.paint, paint, Some(source.clone())).unwrap())]);
    session.engine.apply_edit(edit).unwrap();
    let before = session.engine.document().clone();
    let mut converted = (*original).clone(); converted.interpretation.profile_assumed = false;
    let converted = Arc::new(converted);
    let mut invalid = (*converted).clone(); invalid.extent[0] = 1499;
    assert!(session.apply_rasterized_source(id, &original, Arc::new(invalid)).is_err());
    assert_eq!(session.engine.document(), &before);
    let change = session.dispatch(UiAction::Layer { action: LayerAction::RasterizeSource { id: occurrence_token(id) } }).unwrap();
    assert_ne!(change.regions & regions::HOST, 0);
    let request = session.state.requests.last().unwrap().id; session.complete_document_request(request, Ok(false)).unwrap();
    let mut preview = session.preview_rasterized_source(id, &original, converted.clone()).unwrap();
    assert_eq!(session.engine.document(), &before);
    let original_image = before.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.id();
    assert_ne!(preview.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.id(), original_image);
    session.apply_rasterized_source(id, &original, converted.clone()).unwrap();
    let accepted_image = session.engine.document().artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.id();
    assert_ne!(accepted_image, original_image);
    let base = preview.artwork.paint.get_mut(paint).unwrap().base.as_mut().unwrap();
    base.image = layer_core::Image::with_id(accepted_image, base.image.storage().clone());
    assert_eq!(preview.artwork, session.engine.document().artwork);
    assert_eq!(preview.working, session.engine.document().working);
    let mut expected = source.clone(); expected.base = Some(layer_core::PaintBase {image:layer_core::Image::with_id(accepted_image, converted.clone()),offset:[0;2],policy:layer_core::PaintBasePolicy::WorkingPixels});
    assert_eq!(session.engine.document().artwork.paint.get(paint).unwrap(), &expected);
    assert_eq!(session.engine.document().scene().occurrence(id).unwrap(), &occurrence);
    assert!(!session.command(CommandId::RepairSourceProfile).enabled); assert!(!session.command(CommandId::RasterizeSource).enabled);
    let paint_identity = session.engine.document().artwork.paint.id(paint).unwrap();
    let occurrence_identity = session.engine.document().artwork.occurrences.id(id).unwrap();
    let restored = reopen_capture(&session.capture_artwork().unwrap());
    let restored_paint = restored.artwork.paint.resolve(paint_identity).unwrap();
    let restored_occurrence = restored.artwork.occurrences.resolve(occurrence_identity).unwrap();
    assert_eq!(restored.artwork.paint.get(restored_paint).unwrap().base.as_ref().map(|base|base.image.as_ref()), Some(converted.as_ref()));
    assert_eq!(restored.artwork.paint.get(restored_paint).unwrap().base.as_ref().unwrap().image.id(), accepted_image);
    let mask = restored.scene().occurrence(restored_occurrence).unwrap().mask.as_ref().unwrap();
    let expected_mask = occurrence.mask.as_ref().unwrap();
    assert_eq!(mask.enabled, expected_mask.enabled); assert_eq!(mask.linked, expected_mask.linked); assert_eq!(mask.inverted, expected_mask.inverted);
    assert_eq!(mask.offset, expected_mask.offset);
    assert_eq!(restored.artwork.coverage.id(mask.source), session.engine.document().artwork.coverage.id(expected_mask.source));
    assert_eq!(restored.artwork.coverage.get(mask.source), session.engine.document().artwork.coverage.get(expected_mask.source));
    session.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    assert_eq!(session.engine.document().artwork.paint.get(paint).unwrap(), &source);
    assert_eq!(session.engine.document().scene().occurrence(id).unwrap(), &occurrence);
    assert!(session.command(CommandId::RepairSourceProfile).enabled);
    session.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
    assert_eq!(session.engine.document().artwork.paint.get(paint).unwrap(), &expected);
    assert_eq!(session.engine.document().scene().occurrence(id).unwrap(), &occurrence);
}

#[test]
fn source_admission_counts_aggregate_ownership_before_mutating_document_or_ids() {
    use layer_core::{Edit, ProjectLimits};
    use layer_core::color::{ColorProfile, source::*};
    use std::sync::Arc;
    let fixture = || {
        let mut builder = SourceBuilder::new([256, 256], SourceInterpretation {
            channels: SourceChannels::Rgba, depth: Default::default(),
            profile: Default::default(), profile_assumed: false,
        }, 1024 * 1024).unwrap();
        let mut random = 17u32;
        for _ in 0..256 {
            let row: Vec<u8> = (0..1024).map(|_| {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                (random >> 24) as u8
            }).collect();
            builder.push_row(&row).unwrap();
        }
        builder.finish().unwrap()
    };
    let source = fixture();
    let mut session = session(Platform::Gtk);
    let fill = session.engine.document().scene().effect(occurrence_handle(2).unwrap()).unwrap();
    let program_bytes = fill.program.wgsl.sources().unwrap().iter().map(|source| source.len() as u64).sum::<u64>();
    let limits = ProjectLimits { asset_bytes: source.resident_bytes() as u64 + program_bytes + 1024, ..Default::default() };
    session.engine.backend_mut().tiled_sources = true;
    session.import_sources(vec![("First".into(), source.clone())], limits).unwrap();
    session.engine.set_layer_opacity(session.engine.document().working.occurrence.unwrap(), 0.5).unwrap();
    session.engine.undo().unwrap();
    let before = session.engine.document().clone();
    let checkpoint = session.engine.checkpoint();
    assert!(session.engine.can_redo());
    let error = session.import_sources(vec![("Independent allocation".into(), fixture())], limits).unwrap_err();
    assert!(error.contains("memory limit"), "{error}");
    assert_eq!(session.engine.document(), &before);
    assert_eq!(session.engine.checkpoint(), checkpoint);
    assert!(session.engine.can_redo());
    // Identical bytes in a different allocation count twice; shared tile backing
    // counts once even when separate layers own different image-index objects.
    session.import_sources(vec![("Shared backing".into(), source)], limits).unwrap();
    session.document_snapshot().unwrap().validate(limits).unwrap();
    let before = session.engine.document().clone();
    let handle = before.working.occurrence.unwrap();
    let paint = match before.scene().occurrence(handle).unwrap().content { OccurrenceContent::Paint(paint) => paint, _ => unreachable!() };
    let mut repaired = before.artwork.paint.get(paint).unwrap().clone();
    let mut source = repaired.base.as_ref().unwrap().image.storage().as_ref().clone();
    source.interpretation.profile = ColorProfile::Icc(vec![19; 16384].into());
    repaired.base = Some(layer_core::PaintBase::new(layer_core::Image::new(Arc::new(source))));
    let edit = Edit::Paint(layer_core::RecordChange::replace(&before.artwork.paint, paint, Some(repaired)).unwrap());
    let error = session.source_edit_candidates(&edit, limits).unwrap_err();
    assert!(error.contains("memory limit"), "{error}");
    assert_eq!(session.engine.document(), &before);
}

#[test]
fn source_workflow_requires_current_complete_comparison_and_preserves_original_samples() {
    use crate::SourceWorkflow;
    use layer_core::color::{ColorProfile, RgbSpace, SampleDepth};
    let source = std::sync::Arc::new(u16_source([2, 1], ColorProfile::Builtin(RgbSpace::ProPhoto), true, 1024 * 1024,
        &[1, 0, 2, 0, 3, 0, 0, 0, 4, 0, 5, 0, 6, 0, 255, 255]));
    let mut document = Document::new(layer_core::PortableId::random(), 20, 20, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let paint = match document.scene().occurrence(document.working.occurrence.unwrap()).unwrap().content { layer_core::OccurrenceContent::Paint(paint) => paint, _ => unreachable!() };
    document.artwork.paint.get_mut(paint).unwrap().domain = source.extent;
    document.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::PaintBase::new(layer_core::Image::new(source.clone())));
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, document, [800, 600], Platform::Gtk).unwrap();
    s.frame(1, 1).unwrap();
    invoke(&mut s, CommandId::RepairSourceProfile);
    let id = s.state.requests.first().unwrap().id;
    let mut workflow = SourceWorkflow::begin(&s, id).unwrap();
    assert!(!workflow.adds_layer());
    assert!(workflow.prepare(None, 1024 * 1024, || false).is_err());
    assert!(workflow.prepare(Some(ColorProfile::Builtin(RgbSpace::DisplayP3)), 1024 * 1024, || true).is_err());
    let (corrected, _) = workflow.prepare(Some(ColorProfile::Builtin(RgbSpace::DisplayP3)), 1024 * 1024, || false).unwrap();
    assert!(corrected.tiles.iter().zip(&source.tiles).all(|((_, a), (_, b))| std::sync::Arc::ptr_eq(a, b)));
    assert_eq!(corrected.interpretation.depth, SampleDepth::U16);
    assert!(workflow.preview(&s, corrected.clone(), false, false).is_err());
    workflow.preview(&s, corrected, false, true).unwrap();
    assert!(workflow.commit(&mut s, false, true).is_err());
    workflow.comparison_completed().unwrap();
    assert!(workflow.commit(&mut s, true, true).is_err());
    assert!(workflow.commit(&mut s, false, false).is_err());
    workflow.commit(&mut s, false, true).unwrap();
    assert!(workflow.commit(&mut s, false, true).is_err());
    s.complete_document_request(id, Ok(true)).unwrap();
    let repaired = s.engine.document().artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.storage().clone();
    assert!(!repaired.interpretation.profile_assumed);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.storage(), &source);
    invoke(&mut s, CommandId::Redo);
    assert_eq!(s.engine.document().artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.storage(), &repaired);
    s.frame(2, 2).unwrap();
    invoke(&mut s, CommandId::RasterizeSource);
    let id = s.state.requests.first().unwrap().id;
    let workflow = SourceWorkflow::begin(&s, id).unwrap();
    assert!(workflow.validate_choice(&Some(ColorProfile::Builtin(RgbSpace::Srgb))).is_err());
    assert!(workflow.validate_choice(&None).is_ok());
    assert!(workflow.prepare(None, 0, || false).is_err(), "executor memory budget must be enforced");
    assert!(workflow.prepare(None, 1024 * 1024, || false).is_ok());
    s.complete_document_request(id, Ok(false)).unwrap();
    assert!(workflow.identity.validate(&s, false, true).is_err());
}

#[test]
fn unchanged_source_profile_on_painted_layer_does_not_claim_to_add_a_layer() {
    use layer_core::{color::{ColorProfile, RgbSpace, source::*}, raster::*};
    let source = rgba8_source([1, 1], |_, _| [32, 64, 96, 255]);
    let mut document = Document::new(layer_core::PortableId::random(), 20, 20, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let paint = match document.scene().occurrence(document.working.occurrence.unwrap()).unwrap().content { layer_core::OccurrenceContent::Paint(paint) => paint, _ => unreachable!() };
    document.artwork.paint.get_mut(paint).unwrap().domain = source.extent;
    document.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::PaintBase::new(layer_core::Image::new(source)));
    let descriptor = document.composition().color.paint_descriptor();
    let tile = RasterTile::backed(TileBlob::encode(descriptor,
        &vec![55; descriptor.byte_len([TILE_SIZE; 2]).unwrap()]).unwrap());
    document.artwork.paint.get_mut(paint).unwrap().raster = RasterRevision::backed(RasterData {
        tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }, tile)].into(), watercolor: None,
    });
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, document, [800, 600], Platform::Gtk).unwrap();
    s.frame(1, 1).unwrap();
    invoke(&mut s, CommandId::RepairSourceProfile);
    let id = s.state.requests.first().unwrap().id;
    let mut workflow = crate::SourceWorkflow::begin(&s, id).unwrap();
    assert!(workflow.adds_layer(), "fixture contains committed paint before the choice");
    let (source, _) = workflow.prepare(Some(ColorProfile::Builtin(RgbSpace::Srgb)), 1024 * 1024, || false).unwrap();
    let before = s.engine.document().clone();
    workflow.preview(&s, source, false, true).unwrap();
    workflow.comparison_completed().unwrap();
    assert!(!workflow.adds_layer(), "unchanged interpretation adds no corrected layer");
    workflow.commit(&mut s, false, true).unwrap();
    assert_eq!(s.engine.document(), &before);
}

#[test]
fn window_blur_keeps_an_image_placement_open() {
    let mut session = placed_photo("blur placement");
    assert!(session.objects.placing());
    session.input(UiInput::Blur).unwrap();
    assert!(session.objects.placing(), "losing window focus keeps the placement");
    invoke(&mut session, CommandId::CancelTransform);
    assert!(!session.objects.placing());
}

#[test]
fn source_workflow_interns_verified_worker_tiles_shared_with_objects() {
    use layer_core::{authored::*,Editor,package::transfer::{PreparedTransfer,TransferReceiver},raster::*};
    use std::sync::atomic::AtomicBool;
    let cancelled=AtomicBool::new(false);
    let roundtrip=|document:Document| {
        let capture=Editor::new(document).capture(0,Default::default()).unwrap();
        let prepared=PreparedTransfer::capture(&capture,&cancelled).unwrap();
        let descriptor=serde_json::from_slice(&serde_json::to_vec(prepared.descriptor()).unwrap()).unwrap();
        let mut receiver=TransferReceiver::new(descriptor,Default::default()).unwrap();
        for index in 0..prepared.payload_count() {
            let mut offset=0;let length=prepared.payload_len(index).unwrap();
            while offset<length {
                let size=(length-offset).min(layer_core::package::MAX_RANGE_BYTES as u64) as usize;
                receiver.push_chunk(index,&prepared.read_chunk(index,offset,size).unwrap()).unwrap();offset+=size as u64;
            }
        }
        receiver.finish().unwrap().adopt_verified(Default::default(),&cancelled).unwrap().artwork.as_ref().clone()
    };
    let source=rgba8_source([2,1],|_,_|[17,29,81,255]);let image=Image::new(source.clone());
    let mut document=Document::new(PortableId::random(),40,40,layer_core::DocumentNames {paint:"Photo".into(),paper:"Paper".into()});
    let layer_core::SourceTarget::Paint(paint)=document.working.target.unwrap() else {panic!()};
    document.artwork.paint.get_mut(paint).unwrap().base=Some(PaintBase {image:image.clone(),offset:[7,11],policy:PaintBasePolicy::SourceProfile});
    let (layer,edit)=document.create_object_layer_edit("Shared",ImageObject::new(image.clone()),None,0).unwrap();document.apply(edit).unwrap();
    let object=document.scene().object_handle(layer).unwrap();
    let mut session=UiSession::new(Recorder {tiled_sources:true,..Default::default()},document,[800,600],Platform::Gtk).unwrap();
    session.frame(1,1).unwrap();invoke(&mut session,CommandId::RasterizeSource);
    let request=session.state.requests.first().unwrap().id;
    let mut workflow=crate::SourceWorkflow::begin(&session,request).unwrap();
    let mut worker=Document::new(PortableId::random(),2,1,layer_core::DocumentNames {paint:"".into(),paper:"".into()});
    let layer_core::SourceTarget::Paint(worker_paint)=worker.working.target.unwrap() else {panic!()};
    worker.artwork.paint.get_mut(worker_paint).unwrap().base=Some(PaintBase::new(Image::new(source.clone())));
    worker.artwork=roundtrip(worker.clone());
    let input=worker.artwork.paint.get(worker_paint).unwrap().base.as_ref().unwrap().image.storage();
    let (converted,_)=layer_color::rasterize_source(input,worker.composition().color,1024*1024,||false).unwrap();
    worker.artwork.paint.get_mut(worker_paint).unwrap().base.as_mut().unwrap().image=Image::new(Arc::new(converted));
    let returned=roundtrip(worker);
    let converted=returned.paint.get(worker_paint).unwrap().base.as_ref().unwrap().image.storage().clone();
    assert!(!Arc::ptr_eq(&converted.tiles[&[0,0]],&source.tiles[&[0,0]]));
    let mut conflict=(*converted).clone();
    let bad=TileBlob::encode(converted.interpretation.descriptor(),&[255;4].repeat((TILE_SIZE*TILE_SIZE) as usize)).unwrap();
    conflict.tiles.insert([0,0],Arc::new(TileBlob::from_package(source.tiles[&[0,0]].resource_id(),bad.descriptor,bad.compressed().unwrap()).unwrap()));
    let before=session.engine.document().clone();let checkpoint=session.engine.checkpoint();
    assert!(workflow.preview(&session,Arc::new(conflict),false,true).is_err());
    assert_eq!(session.engine.document(),&before);assert_eq!(session.engine.checkpoint(),checkpoint);
    let candidate=workflow.preview(&session,converted,false,true).unwrap();
    let base=candidate.artwork.paint.get(paint).unwrap().base.as_ref().unwrap();
    assert_eq!(base.offset,[7,11]);assert_eq!(base.policy,PaintBasePolicy::WorkingPixels);assert_ne!(base.image.id(),image.id());
    assert!(Arc::ptr_eq(&base.image.tiles[&[0,0]],&source.tiles[&[0,0]]));
    assert!(candidate.artwork.objects.get(object).unwrap().image.same_owner(&image));
    PreparedTransfer::capture(&Editor::new(candidate).capture(0,Default::default()).unwrap(),&cancelled).unwrap();
    workflow.comparison_completed().unwrap();workflow.commit(&mut session,false,true).unwrap();
    session.complete_document_request(request,Ok(true)).unwrap();
    invoke(&mut session,CommandId::Undo);assert!(session.engine.document().artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.same_owner(&image));
    invoke(&mut session,CommandId::Redo);
    let capture=Editor::new(session.engine.document().clone()).capture(0,Default::default()).unwrap();PreparedTransfer::capture(&capture,&cancelled).unwrap();
}
