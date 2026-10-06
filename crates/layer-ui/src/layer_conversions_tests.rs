use super::*;
use crate::session::test_support::{Recorder, invoke, layer as layer_action, select, rectangle};
use layer_core::{Document, DocumentNames, ImageObject, PortableId, SceneScope, authored::{Affine64, OccurrenceContent}, color::source::rgba8_source};

fn session() -> UiSession<Recorder> {
    UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new(PortableId::random(), 600, 400, DocumentNames { paint: "Ink".into(), paper: "Paper".into() }), [600, 400], Platform::Gtk).unwrap()
}
fn picture() -> Image { rgba8_source([80, 60], |x, y| [x as u8, y as u8, 7, 255]).into() }
fn images(s: &mut UiSession<Recorder>, x: f64) -> OccurrenceHandle {
    let (layer, edit) = s.engine.document().create_object_layer_edit("Images", None, 0).unwrap();
    s.engine.apply_edit(edit).unwrap();
    let mut object = ImageObject::new(picture(), "Photo");
    object.affine = Affine64([1., 0., 0., 1., x, 20.]);
    let (_, edit) = s.engine.document().add_image_object_edit(layer, object, 0).unwrap();
    s.engine.apply_edit(edit).unwrap();
    layer_action(s, LayerAction::Select { id: occurrence_token(layer), mask: false });
    s.refresh_document();
    layer
}
fn captured(s: &mut UiSession<Recorder>) -> layer_core::ImageCapture {
    let Some(layer_render::SnapshotRequest::Image(capture)) = s.engine.backend().snapshot_requests.last().cloned() else { panic!("an image capture") };
    capture
}
fn reply(s: &mut UiSession<Recorder>, origin: [i64; 2]) {
    let pixels = rgba8_source([4, 3], |_, _| [200, 10, 10, 255]);
    s.engine.backend_mut().snapshot_reply = Some(Ok(layer_render::SnapshotResult::Image(Some((pixels, origin)))));
    s.frame(10, 10).unwrap();
}

#[test]
fn rasterize_layer_captures_raw_object_content_then_publishes_one_undoable_paint_layer() {
    let mut s = session();
    let layer = images(&mut s, -100.);
    assert!(s.command(CommandId::RasterizeLayer).enabled);
    assert!(!s.command(CommandId::ConvertToObject).enabled);
    invoke(&mut s, CommandId::RasterizeLayer);
    let capture = captured(&mut s);
    assert_eq!(capture.scope, SceneScope::RawObjects(layer));
    assert!(s.document_idle_reason().is_some(), "the drawing waits for the capture");
    assert!(!s.command(CommandId::RasterizeLayer).enabled);
    let before = s.engine.document().clone();
    reply(&mut s, [70, 20]);
    let doc = s.engine.document();
    let occurrence = doc.scene().occurrence(layer).unwrap();
    assert_eq!(occurrence.kind(), LayerKind::Paint);
    assert!(doc.artwork.objects.is_empty());
    let base = doc.scene().paint_source(layer).unwrap().base.as_ref().unwrap();
    assert_eq!((base.offset, base.policy), ([70, 20], layer_core::authored::PaintBasePolicy::WorkingPixels));
    assert_eq!(doc.working.target, doc.scene().source_target(layer), "the new paint is the drawing target");
    assert!(doc.working.objects.is_empty());
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().artwork.objects.len(), before.artwork.objects.len());
    assert_eq!(s.engine.document().scene().occurrence(layer).unwrap().kind(), LayerKind::Object);
}

#[test]
fn a_changed_drawing_cancels_the_capture_without_editing() {
    let mut s = session();
    let layer = images(&mut s, 10.);
    invoke(&mut s, CommandId::RasterizeLayer);
    let mut occurrence = s.engine.document().scene().occurrence(layer).unwrap().clone();
    occurrence.opacity = 0.5;
    s.engine.apply_edit(layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&s.engine.document().artwork.occurrences, layer, Some(occurrence)).unwrap())).unwrap();
    let before = s.engine.document().clone();
    reply(&mut s, [0, 0]);
    assert_eq!(s.engine.document().artwork, before.artwork, "a stale capture is never published");
    assert!(s.engine.backend().snapshot_cancels > 0);
    assert!(s.document_idle_reason().is_none());
}

#[test]
fn convert_to_image_layer_shares_an_untouched_photo_without_a_capture() {
    let mut s = session();
    let ink = s.engine.document().working.occurrence.unwrap();
    let OccurrenceContent::Paint(paint) = s.engine.document().scene().occurrence(ink).unwrap().content else { panic!("paint") };
    let photo = picture();
    let mut source = s.engine.document().artwork.paint.get(paint).unwrap().clone();
    source.base = Some(layer_core::authored::PaintBase { image: photo.clone(), offset: [5, 6], policy: layer_core::authored::PaintBasePolicy::SourceProfile });
    s.engine.apply_edit(layer_core::Edit::Paint(layer_core::RecordChange::replace(&s.engine.document().artwork.paint, paint, Some(source)).unwrap())).unwrap();
    s.refresh_document();
    assert!(s.command(CommandId::ConvertToObject).enabled);
    invoke(&mut s, CommandId::ConvertToObject);
    assert!(s.engine.backend().snapshot_requests.is_empty());
    let doc = s.engine.document();
    let [object] = doc.scene().object_layer(ink).unwrap().children[..] else { panic!("one image") };
    assert!(doc.scene().object(object).unwrap().image.same_owner(&photo));
}

#[test]
fn image_layers_take_mask_merge_alpha_and_reference_commands_but_refuse_paint_only_ones() {
    let mut s = session();
    let layer = images(&mut s, 10.);
    layer_action(&mut s, LayerAction::AddMask { id: occurrence_token(layer), replace: false });
    layer_action(&mut s, LayerAction::Select { id: occurrence_token(layer), mask: false });
    s.refresh_document();
    assert_eq!(s.command(CommandId::ApplyLayerMask).label.as_ref(), "Rasterize and Apply Mask");
    assert!(s.command(CommandId::ApplyLayerMask).enabled);
    layer_action(&mut s, LayerAction::ApplyMask { id: occurrence_token(layer) });
    let capture = captured(&mut s);
    assert_eq!(capture.scope, SceneScope::Members(vec![layer].into()));
    s.cancel_conversion();
    assert!(s.command(CommandId::MergeDown).enabled);
    assert!(s.command(CommandId::MergeVisible).enabled);
    invoke(&mut s, CommandId::BlendPerceptual);
    invoke(&mut s, CommandId::FrequencySeparation);
    assert_eq!(s.state.notice.as_ref().map(|n| n.actions.len()), Some(3), "Frequency Separation offers Rasterize Layer first");
    assert!(s.frequency_separation_view().is_none());
    s.dispatch(UiAction::Selection { action: SelectionAction::LoadCoverage { id: occurrence_token(layer), mask: false, mode: layer_core::SelectionMode::New } }).unwrap();
    for tick in 20..30 { if s.engine.backend().region_requests.is_empty() { s.frame(tick, tick).unwrap(); } }
    let request = s.engine.backend().region_requests.last().unwrap();
    assert!(matches!(request.source, layer_render::RegionSource::ObjectCoverage(handle) if handle == layer), "Select Layer Opacity reads the image layer's own alpha");
}

#[test]
fn copy_selection_to_layer_captures_image_pixels_and_cut_refuses() {
    let mut s = session();
    let layer = images(&mut s, 10.);
    s.dispatch(UiAction::Invoke { command: CommandId::Select }).unwrap();
    select(&mut s, rectangle([20., 30., 60., 70.]));
    assert!(s.selection_to_layer_refusal(true).is_none());
    let before = s.engine.checkpoint();
    s.selection_to_layer(true).unwrap();
    assert_eq!(s.state.notice.as_ref().map(|n| n.actions.len()), Some(3), "Cut Selection to Layer refuses with the image actions");
    assert_eq!(s.engine.checkpoint(), before);
    assert!(s.selection_to_layer_refusal(false).is_none());
    s.selection_to_layer(false).unwrap();
    let capture = captured(&mut s);
    assert!(matches!(&capture.scope, SceneScope::Members(members) if members.contains(&layer)));
    assert!(capture.selection.is_some());
    reply(&mut s, [20, 30]);
    let doc = s.engine.document();
    let copy = doc.working.occurrence.unwrap();
    assert_ne!(copy, layer);
    assert_eq!(doc.scene().occurrence(copy).unwrap().kind(), LayerKind::Paint);
    assert_eq!(doc.scene().occurrence(layer).unwrap().kind(), LayerKind::Object, "the image layer stays");
}

#[test]
fn every_new_command_label_and_reason_is_localized() {
    let s = session();
    let l = s.localization();
    for refusal in [ConversionRefusal::NoLayer, ConversionRefusal::NotPaint, ConversionRefusal::NotObjects, ConversionRefusal::Locked,
        ConversionRefusal::NoMask, ConversionRefusal::MaskDisabled, ConversionRefusal::TooLarge] {
        assert!(!conversion_refusal_text(refusal, l).is_empty());
    }
    assert_eq!(CommandId::ConvertToObject.localized_label(l).as_ref(), "Convert to Image Layer");
}

#[test]
fn image_layer_rows_show_content_thumbnails_and_layer_controls_without_alpha_lock() {
    let mut s = session();
    let layer = images(&mut s, 10.);
    let row = s.state.layers.iter().find(|l| l.id == occurrence_token(layer)).unwrap();
    assert!(row.has_thumbnail);
    let controls = s.state.layer_tools.controls;
    assert!(controls.opacity && controls.blend && controls.mask && controls.edit_lock);
    assert!(!controls.alpha_lock && !controls.fill);
    let menu = s.layer_menu_with(occurrence_token(layer), false, true).unwrap();
    fn labels(items: &[Vec<ContextMenuItem>], into: &mut Vec<String>) {
        for item in items.iter().flatten() { into.push(item.label.clone()); labels(&item.sections, into); }
    }
    let mut all = Vec::new();
    labels(&menu.sections, &mut all);
    let labels = all;
    assert!(labels.iter().any(|label| label == "Rasterize Layer"));
    assert!(!labels.iter().any(|label| label == s.localization().text(MessageId::RESOURCES_LAYER_MENU_ALPHA_LOCK).as_ref()));
    assert!(s.layer_filter_menu(layer).is_some(), "filters attach to image layers");
}

#[test]
fn repairing_an_image_objects_profile_leaves_other_users_of_the_image_unchanged() {
    let mut s = session();
    let ink = s.engine.document().working.occurrence.unwrap();
    let OccurrenceContent::Paint(paint) = s.engine.document().scene().occurrence(ink).unwrap().content else { panic!("paint") };
    let layer = images(&mut s, 10.);
    let object = s.engine.document().scene().object_layer(layer).unwrap().children[0];
    let shared = s.engine.document().scene().object(object).unwrap().image.clone();
    let mut source = s.engine.document().artwork.paint.get(paint).unwrap().clone();
    source.base = Some(layer_core::authored::PaintBase { image: shared.clone(), offset: [0, 0], policy: layer_core::authored::PaintBasePolicy::SourceProfile });
    s.engine.apply_edit(layer_core::Edit::Paint(layer_core::RecordChange::replace(&s.engine.document().artwork.paint, paint, Some(source)).unwrap())).unwrap();
    layer_action(&mut s, LayerAction::Select { id: occurrence_token(layer), mask: false });
    let mut working = s.engine.document().working.clone();
    working.objects = [object].into();
    s.engine.apply_edit(layer_core::Edit::Working(working)).unwrap();
    s.refresh_commands();
    assert_eq!(s.active_source_use(), Some(SourceUse::Object(object)));
    assert!(s.command(CommandId::RepairSourceProfile).enabled);
    let mut corrected = (**shared.storage()).clone();
    corrected.interpretation.profile = layer_core::color::ColorProfile::Builtin(layer_core::color::RgbSpace::DisplayP3);
    assert_eq!(s.repair_layer_source(SourceUse::Object(object), shared.storage(), corrected).unwrap(), SourceUse::Object(object));
    let doc = s.engine.document();
    assert!(!doc.scene().object(object).unwrap().image.same_owner(&shared));
    assert!(doc.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.same_owner(&shared), "the photo layer keeps its interpretation");
    invoke(&mut s, CommandId::Undo);
    assert!(s.engine.document().scene().object(object).unwrap().image.same_owner(&shared));
}
