use layer_core::authored::*;

fn content_session(size: [u32; 2]) -> UiSession<Recorder> {
    let mut doc = Document::new(PortableId::random(), size[0], size[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let ink = doc.working.occurrence.unwrap();
    let paper: Vec<_> = doc.scene().order().iter().copied().filter(|h| *h != ink).collect();
    let root = doc.composition().result;
    doc.artwork.stacks.get_mut(root).unwrap().entries = vec![ink];
    for handle in paper {
        let id = doc.artwork.occurrences.id(handle).unwrap();
        doc.artwork.occurrences.change(handle, id, None).unwrap();
    }
    let working = doc.working.clone();
    let mut doc = Document::from_artwork(doc.artwork).unwrap();
    doc.working = working;
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [1600, 1000], Platform::Gtk).unwrap();
    s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
    invoke(&mut s, CommandId::FitCanvas);
    s.frame(1, 1).unwrap();
    s
}

fn reply_bounds(s: &mut UiSession<Recorder>, values: [f32; 4]) {
    let bounds = if values == [0.; 4] { layer_core::Rect::EMPTY } else { layer_core::Rect {
        min: Point { x: values[0], y: values[1] },
        max: Point { x: values[2], y: values[3] },
    } };
    s.engine.backend_mut().bounds_reply = Some(Ok(bounds));
    s.frame(100, 100).unwrap();
    s.frame(101, 101).unwrap();
}

fn size_of(s: &UiSession<Recorder>) -> [u32; 2] {
    s.engine.document().composition().size
}

#[test]
fn rotating_a_non_square_image_right_turns_every_pixel_in_one_step() {
    let mut s = crop_session();
    let paint = s.engine.document().working.target.unwrap();
    rectangle_selection(&mut s, [100., 100., 300., 200.]);
    let before = s.engine.document().clone();
    let center = on_surface(&s, Point { x: 500., y: 400. });
    for command in [CommandId::RotateImageLeft, CommandId::RotateImageRight, CommandId::RotateImage180, CommandId::FlipImageHorizontal, CommandId::FlipImageVertical] {
        assert!(s.command(command).enabled, "{command:?}");
    }
    invoke(&mut s, CommandId::RotateImageRight);
    s.frame(20, 20).unwrap();
    assert_eq!(size_of(&s), [800, 1000]);
    let [(id, transform)] = resampled(&mut s).try_into().unwrap();
    assert_eq!(id, paint);
    assert_eq!(transform.placement.interpolation, layer_core::Interpolation::Nearest, "an exact permutation");
    assert_eq!(s.engine.document().target_extent(paint), [1000, 1000], "a square scratch extent");
    let turn = layer_core::Affine([0., 1., -1., 0., 800., 0.]);
    assert_eq!(s.engine.document().working.selection, Some(before.working.selection.as_ref().unwrap().transformed(turn).unwrap()));
    near_point(on_surface(&s, turn.map(Point { x: 500., y: 400. })), center, 0.01);
    invoke(&mut s, CommandId::Undo);
    s.frame(21, 21).unwrap();
    assert_eq!(size_of(&s), [1000, 800]);
    assert_live_artwork_eq(s.engine.document(), &before);
    assert_eq!(s.engine.document().working.selection, before.working.selection);
}

#[test]
fn trim_shrinks_to_the_visible_pixels_and_reveal_all_brings_hidden_pixels_back() {
    let mut s = content_session([1000, 800]);
    assert!(s.command(CommandId::Trim).enabled);
    invoke(&mut s, CommandId::Trim);
    reply_bounds(&mut s, [0., 0., 542., 456.]);
    assert_eq!(size_of(&s), [542, 456], "from the pixel at 0,0 to the right and bottom edges");
    invoke(&mut s, CommandId::Undo);
    let paint = s.engine.document().working.occurrence.unwrap();
    let mut occurrence = s.engine.document().scene().occurrence(paint).unwrap().clone();
    occurrence.translation = Point { x: -100., y: 0. };
    s.layer_edit(layer_core::Edit::Occurrence(RecordChange::replace(&s.engine.document().artwork.occurrences, paint, Some(occurrence)).unwrap())).unwrap();
    s.frame(11, 11).unwrap();
    invoke(&mut s, CommandId::Trim);
    reply_bounds(&mut s, [166., 256., 442., 456.]);
    assert_eq!(size_of(&s), [276, 200], "only the pixels on the canvas count; the one at 0,0 lies beyond its left edge");
    assert_eq!(s.engine.document().scene().occurrence(paint).unwrap().translation, Point { x: -266., y: -256. });
    invoke(&mut s, CommandId::RevealAll);
    reply_bounds(&mut s, [-266., -256., 276., 200.]);
    let doc = s.engine.document();
    assert_eq!(doc.composition().size, [542, 456], "every pixel, including the one beyond the edge");
    invoke(&mut s, CommandId::RevealAll);
    reply_bounds(&mut s, [0., 0., 542., 456.]);
    assert_eq!(notice_text(&s), Some("Every pixel is already on the canvas"));
    invoke(&mut s, CommandId::Undo);
    invoke(&mut s, CommandId::Undo);
    s.frame(14, 14).unwrap();
    assert_eq!(size_of(&s), [1000, 800], "Trim and Reveal All are one step each");
}

#[test]
fn trim_and_fit_content_refuse_when_nothing_is_visible() {
    let mut s = content_session([600, 400]);
    invoke(&mut s, CommandId::Trim);
    reply_bounds(&mut s, [0.; 4]);
    assert_eq!(notice_text(&s), Some("There are no visible pixels to trim to"));
    assert_eq!(s.command_disabled_reason(CommandId::CropFitContent).as_deref(), Some("Choose the Crop tool first"));
    invoke(&mut s, CommandId::Crop);
    assert!(!s.command(CommandId::Trim).enabled);
    assert_eq!(s.command_disabled_reason(CommandId::Trim).as_deref(), Some("Apply or cancel the crop first"));
    invoke(&mut s, CommandId::CropFitContent);
    reply_bounds(&mut s, [0.; 4]);
    assert_eq!(notice_text(&s), Some("There are no visible pixels to fit the crop to"));
    let mut full = content_session([256, 256]);
    invoke(&mut full, CommandId::Trim);
    reply_bounds(&mut full, [0., 0., 256., 256.]);
    assert_eq!(notice_text(&full), Some("The visible pixels already reach every edge of the canvas"));
}

#[test]
fn fit_content_sets_the_crop_to_the_content_including_pixels_beyond_the_canvas() {
    let mut s = content_session([600, 400]);
    invoke(&mut s, CommandId::Crop);
    invoke(&mut s, CommandId::CropRatioSquare);
    let bar: Vec<_> = s.state.tool_actions.iter().map(|a| a.command).collect();
    assert!(bar.contains(&CommandId::CropFitContent), "Fit Content is on the crop bar");
    invoke(&mut s, CommandId::CropFitContent);
    reply_bounds(&mut s, [50., 60., 762., 266.]);
    let frame = crop_frame(&s);
    assert_eq!((frame.center, frame.size, frame.angle), (Point { x: 406., y: 163. }, [712., 206.], 0.));
    assert!(s.command(CommandId::CropRatioFree).selected, "the frame is exact, so the ratio is free");
    invoke(&mut s, CommandId::ApplyTransform);
    s.frame(10, 10).unwrap();
    assert_eq!(size_of(&s), [712, 206], "the canvas grows to hold the pixels past its edge");
}

#[test]
fn pending_bounds_keep_frames_running_and_reject_changed_drawings() {
    let mut s = content_session([1536, 1536]);
    invoke(&mut s, CommandId::Trim);
    assert!(s.content_bounds.busy());
    assert!(s.wants_continuous_frames());
    assert_eq!(size_of(&s), [1536, 1536]);
    s.frame(2, 2).unwrap();
    assert_eq!(s.engine.backend_mut().bounds_requests[0].scope, layer_core::ContentScope::Canvas);
    s.frame(2, 2).unwrap();
    assert!(s.content_bounds.busy());
    reply_bounds(&mut s, [1., 1., 1530., 1530.]);
    assert_eq!(size_of(&s), [1529, 1529]);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(size_of(&s), [1536, 1536]);

    let mut s = content_session([1536, 1536]);
    invoke(&mut s, CommandId::RevealAll);
    assert!(s.content_bounds.busy());
    let selection = layer_core::Selection::polygon(layer_core::Rect { min: Point { x: 1., y: 1. }, max: Point { x: 9., y: 9. } }.corners().to_vec()).unwrap();
    s.layer_edit(canvas_bar_selection_edit(s.engine.document(), Some(selection))).unwrap();
    reply_bounds(&mut s, [-100., -100., 2000., 2000.]);
    assert!(!s.content_bounds.busy());
    assert_eq!(size_of(&s), [1536, 1536]);
    assert_eq!(notice_text(&s), Some("The content bounds scan stopped because the drawing changed"));
    assert!(s.engine.backend_mut().bounds_cancels > 0);
}

#[test]
fn bounds_failure_leaves_the_document_unchanged_and_can_be_retried() {
    let mut s = content_session([600, 400]);
    let before = s.engine.document().clone();
    invoke(&mut s, CommandId::Trim);
    s.frame(2, 2).unwrap();
    s.engine.backend_mut().bounds_reply = Some(Err(layer_render::BackendError("bounds failed")));
    s.frame(3, 3).unwrap();
    assert!(!s.content_bounds.busy());
    assert_eq!(s.engine.document(), &before);
    assert_eq!(notice_text(&s), Some("bounds failed"));
    invoke(&mut s, CommandId::Trim);
    reply_bounds(&mut s, [10., 20., 110., 120.]);
    assert_eq!(size_of(&s), [100, 100]);
}

#[test]
fn bounds_admission_retries_without_duplicating_accepted_requests() {
    let mut s = content_session([600, 400]);
    s.engine.backend_mut().bounds_wait = true;
    invoke(&mut s, CommandId::Trim);
    s.frame(2, 2).unwrap();
    assert!(s.content_bounds.busy());
    assert!(s.engine.backend_mut().bounds_requests.is_empty());
    s.engine.backend_mut().bounds_wait = false;
    s.frame(3, 3).unwrap();
    assert_eq!(s.engine.backend_mut().bounds_requests.len(), 1);
    s.frame(4, 4).unwrap();
    assert_eq!(s.engine.backend_mut().bounds_requests.len(), 1);
    reply_bounds(&mut s, [10., 20., 110., 120.]);
    assert_eq!(size_of(&s), [100, 100]);
}

#[test]
fn transform_waits_for_measured_target_coverage_and_cancel_discards_it() {
    let mut s = filled_selection_session();
    let before = s.engine.document().clone();
    s.dispatch(UiAction::Invoke { command: CommandId::ScaleRotate }).unwrap();
    assert!(s.content_bounds.busy());
    assert!(!s.operation.active());
    assert_eq!(s.engine.backend_mut().bounds_requests.last().unwrap().scope, layer_core::ContentScope::Target(before.active_target().unwrap()));
    reply_bounds(&mut s, [100., 100., 300., 300.]);
    assert!(!s.content_bounds.busy());
    assert!(s.operation.active());
    invoke(&mut s, CommandId::CancelTransform);
    assert_eq!(s.engine.document(), &before);

    s.layer_edit(canvas_bar_selection_edit(s.engine.document(), Some(rectangle([110., 110., 290., 290.])))).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::ScaleRotate }).unwrap();
    assert!(s.content_bounds.busy());
    invoke(&mut s, CommandId::CancelTransform);
    assert!(!s.content_bounds.busy());
    reply_bounds(&mut s, [110., 110., 290., 290.]);
    assert!(!s.operation.active());
}

mod bounds_review {
    use super::*;
    include!("image_geometry_review_tests.rs");
}

mod transform_pixels {
    use super::*;
    include!("transform_pixels_tests.rs");
}

#[test]
fn snapping_request_failures_cache_empty_without_history_or_retry_loops() {
    for retry in [false, true] {
        let mut s = content_session([1000, 800]);
        let id = s.engine.document().working.occurrence.unwrap();
        let before = s.engine.document().clone();
        let purpose = super::image_geometry::ContentUse::PrepareSnap(id);
        s.engine.backend_mut().bounds_wait = retry;
        s.engine.backend_mut().bounds_fails = !retry;
        let result = s.request_content_bounds(purpose);
        if retry {
            result.unwrap();
            assert!(s.content_bounds.busy());
            s.engine.backend_mut().bounds_wait = false;
            s.engine.backend_mut().bounds_fails = true;
            s.poll_content_bounds();
        } else {
            assert!(result.is_err());
        }
        assert!(!s.content_bounds.busy());
        let calls = s.engine.backend().bounds_attempts;
        for _ in 0..4 { s.request_content_bounds(purpose).unwrap(); }
        assert_eq!(s.engine.backend().bounds_attempts, calls);
        assert_eq!(s.engine.document(), &before);
        assert!(!s.engine.can_undo());
    }
}

#[test]
fn snapping_failed_completion_is_quiet_and_cached_for_the_current_revision() {
    let mut s = content_session([1000, 800]);
    let id = s.engine.document().working.occurrence.unwrap();
    let before = s.engine.document().clone();
    let purpose = super::image_geometry::ContentUse::PrepareSnap(id);
    s.request_content_bounds(purpose).unwrap();
    s.engine.backend_mut().bounds_reply = Some(Err(layer_render::BackendError("bounds readback failed")));
    s.poll_content_bounds();
    assert!(!s.content_bounds.busy());
    let calls = s.engine.backend().bounds_attempts;
    s.request_content_bounds(purpose).unwrap();
    assert_eq!(s.engine.backend().bounds_attempts, calls);
    assert_eq!(s.engine.document(), &before);
    assert!(!s.engine.can_undo());
}

fn snapping_document() -> Document {
    let mut doc = content_session([1000, 800]).engine.document().clone();
    let ink = doc.working.occurrence.unwrap();
    let SourceTarget::Paint(paint) = doc.working.target.unwrap() else { panic!("paint") };
    let source = doc.artwork.paint.get(paint).unwrap().clone();
    let other_paint = doc.artwork.paint.insert(PortableId::random(), source).unwrap();
    let mut other = doc.scene().occurrence(ink).unwrap().clone(); other.content = OccurrenceContent::Paint(other_paint);
    let other = doc.artwork.occurrences.insert(PortableId::random(), other).unwrap();
    let root = doc.composition().result;
    doc.artwork.stacks.get_mut(root).unwrap().entries.push(other);
    doc.artwork.paint.get_mut(paint).unwrap().original = Some(layer_core::color::source::rgba8_source([100, 80], |_, _| [120, 120, 120, 255]));
    let working = doc.working.clone();
    let mut doc = Document::from_artwork(doc.artwork).unwrap(); doc.working = working;
    doc
}

#[test]
fn held_transform_nudge_defers_background_snapping_preparation_until_release() {
    let doc = snapping_document();
    let target = doc.scene().order()[1];
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [1600, 1000], Platform::Gtk).unwrap();
    s.frame(1, 1).unwrap();
    s.begin_transform().unwrap();
    if s.content_bounds.busy() { reply_bounds(&mut s, [0., 0., 100., 80.]); }
    assert!(s.operation.transforming());
    s.operation.snapping = true;
    assert!(s.transform_nudge("arrowright", true, true, Modifiers::default()).unwrap());
    let calls = s.engine.backend().bounds_attempts;
    for _ in 0..4 { s.prepare_transform_snapping().unwrap(); }
    assert_eq!(s.engine.backend().bounds_attempts, calls);
    assert!(!s.content_bounds.busy());
    assert!(s.transform_nudge("arrowright", false, true, Modifiers::default()).unwrap());
    s.prepare_transform_snapping().unwrap();
    assert_eq!(s.engine.backend().bounds_attempts, calls + 1);
    assert_eq!(s.engine.backend().bounds_requests.last().unwrap().scope, layer_core::ContentScope::PlacedTarget(target));
}

#[test]
fn failed_background_query_does_not_refuse_snapping_toggle() {
    let doc = snapping_document();
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [1600, 1000], Platform::Gtk).unwrap();
    s.frame(1, 1).unwrap();
    s.begin_transform().unwrap();
    if s.content_bounds.busy() { reply_bounds(&mut s, [0., 0., 100., 80.]); }
    assert!(s.operation.transforming());
    let before = s.engine.document().clone();
    let calls = s.engine.backend().bounds_attempts;
    s.engine.backend_mut().bounds_fails = true;
    s.dispatch(UiAction::Invoke { command: CommandId::TransformSnapping }).unwrap();
    assert!(s.operation.snapping);
    assert!(!s.content_bounds.busy());
    assert_eq!(s.engine.backend().bounds_attempts, calls + 1);
    assert_eq!(s.engine.document(), &before);
    assert!(!s.engine.can_undo());
}

#[test]
fn blurring_held_transform_nudge_publishes_document_and_command_changes() {
    let doc = snapping_document();
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [1600, 1000], Platform::Gtk).unwrap();
    s.frame(1, 1).unwrap();
    s.begin_transform().unwrap();
    if s.content_bounds.busy() { reply_bounds(&mut s, [0., 0., 100., 80.]); }
    assert!(s.operation.transforming());
    assert!(s.transform_nudge("arrowright", true, true, Modifiers::default()).unwrap());
    assert!(s.localization_input_busy());
    let changed = s.input(UiInput::Blur).unwrap().change;
    assert_ne!(changed.regions & regions::DOCUMENT, 0);
    assert_ne!(changed.regions & regions::COMMANDS, 0);
    assert!(!s.operation.nudging());
    assert!(!s.localization_input_busy());
}

#[test]
fn enabled_snapping_prepares_after_pending_input_drains_without_idle_resubmission() {
    let doc = snapping_document();
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [1600, 1000], Platform::Gtk).unwrap();
    s.frame(1, 1).unwrap();
    s.begin_transform().unwrap();
    if s.content_bounds.busy() { reply_bounds(&mut s, [0., 0., 100., 80.]); }
    assert!(s.operation.transforming());
    let target = s.engine.document().scene().order()[1];
    s.dispatch(UiAction::Invoke { command: CommandId::TransformSnapping }).unwrap();
    s.cancel_content_bounds();
    let calls = s.engine.backend().bounds_attempts;
    s.input_pending = true;
    s.refresh_tools();
    assert!(s.operation.snapping && s.operation.transforming());
    assert_eq!(s.engine.backend().bounds_attempts, calls);
    assert!(!s.content_bounds.busy());
    s.frame(102, 102).unwrap();
    assert!(!s.input_pending);
    assert!(s.content_bounds.busy());
    assert_eq!(s.engine.backend().bounds_attempts, calls + 1);
    assert_eq!(s.engine.backend().bounds_requests.last().unwrap().scope, layer_core::ContentScope::PlacedTarget(target));
    reply_bounds(&mut s, [400., 200., 500., 300.]);
    assert!(s.operation.transforming());
    assert_eq!(s.measured_snap_bounds(), vec![(target, layer_core::Rect { min: Point { x: 400., y: 200. }, max: Point { x: 500., y: 300. } })]);
    let calls = s.engine.backend().bounds_attempts;
    for frame in 103..107 { s.frame(frame, frame).unwrap(); }
    assert_eq!(s.engine.backend().bounds_attempts, calls);

}

#[test]
fn snapping_excludes_moved_nested_ancestors_but_keeps_siblings_and_cousins() {
    let mut doc=Document::new(PortableId::random(),1000,800,layer_core::DocumentNames{paint:"Ink".into(),paper:"Paper".into()});
    let old: Vec<_> = doc.artwork.occurrences.iter().map(|(h, id, _)| (h, id)).collect();
    for (h, id) in old { doc.artwork.occurrences.change(h, id, None).unwrap(); }
    let root = doc.composition().result;
    doc.artwork.stacks.get_mut(root).unwrap().entries.clear();
    let extent = doc.composition().size;
    let mut add=|name:&str,parent:Option<OccurrenceHandle>,group| {
        let content = if group { OccurrenceContent::Stack(doc.artwork.stacks.insert(PortableId::random(), Stack::default()).unwrap()) }
            else { OccurrenceContent::Paint(doc.artwork.paint.insert(PortableId::random(), PaintSource { domain: extent, raster: Default::default(), original: None, operations: Default::default() }).unwrap()) };
        let id = doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(content, name)).unwrap();
        let stack = parent.map(|h| match doc.artwork.occurrences.get(h).unwrap().content { OccurrenceContent::Stack(h) => h, _ => panic!("parent stack") }).unwrap_or(root);
        doc.artwork.stacks.get_mut(stack).unwrap().entries.push(id);
        id
    };
    let outer=add("Outer",None,true);
    let nested=add("Nested",Some(outer),true);
    let moved=add("Moved",Some(nested),false);
    let sibling=add("Sibling",Some(nested),false);
    let cousin_group=add("Cousin group",Some(outer),true);
    let cousin=add("Cousin",Some(cousin_group),false);
    let unrelated=add("Unrelated",None,false);
    let hidden_group=add("Hidden group",None,true);
    let _hidden_child=add("Hidden child",Some(hidden_group),false);
    doc.artwork.occurrences.get_mut(hidden_group).unwrap().visible=false;
    let mut doc = Document::from_artwork(doc.artwork).unwrap();
    doc.apply(doc.select_occurrence_edit(moved).unwrap()).unwrap();
    let mut s=UiSession::new(Recorder::default(),doc,[1600,1000],Platform::Gtk).unwrap();
    s.layer_interaction.tool=LayerCanvasTool::Move;
    s.operation.snapping=true;
    s.frame(1,1).unwrap();
    for _ in 0..20 {
        s.prepare_transform_snapping().unwrap();
        if !s.content_bounds.busy() {break;}
        reply_bounds(&mut s,[0.,0.,10.,10.]);
    }
    let eligible=|s:&UiSession<Recorder>|s.measured_snap_bounds().into_iter().map(|(id,_)|id).collect::<Vec<_>>();
    let mut expected=vec![sibling,cousin_group,cousin,unrelated];expected.sort();
    assert_eq!(eligible(&s),expected);
    s.set_selected_layers([nested].into_iter().collect()).unwrap();
    s.frame(2,2).unwrap();
    for _ in 0..20 {
        s.prepare_transform_snapping().unwrap();
        if !s.content_bounds.busy() {break;}
        reply_bounds(&mut s,[0.,0.,10.,10.]);
    }
    let mut expected=vec![cousin_group,cousin,unrelated];expected.sort();
    assert_eq!(eligible(&s),expected,"selected group excludes its subtree and ancestors");
    s.set_selected_layers([moved,cousin].into_iter().collect()).unwrap();
    s.frame(3,3).unwrap();
    for _ in 0..20 {
        s.prepare_transform_snapping().unwrap();
        if !s.content_bounds.busy() {break;}
        reply_bounds(&mut s,[0.,0.,10.,10.]);
    }
    let mut expected=vec![sibling,unrelated];expected.sort();
    assert_eq!(eligible(&s),expected,"multiple nested roots exclude each ancestor chain only");
}
