fn content_session(size: [u32; 2]) -> UiSession<Recorder> {
    let mut doc = Document::new("content", size[0], size[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    doc.layers.retain(|l| l.kind != layer_core::LayerKind::Background);
    let mut s = UiSession::new(Recorder::default(), doc, [1600, 1000], Platform::Gtk).unwrap();
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
    [s.engine.document().width, s.engine.document().height]
}

#[test]
fn rotating_a_non_square_image_right_turns_every_pixel_in_one_step() {
    let mut s = crop_session();
    let paint = s.engine.document().layers[0].id;
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
    assert_eq!(s.engine.document().selection, Some(before.selection.as_ref().unwrap().transformed(turn).unwrap()));
    near_point(on_surface(&s, turn.map(Point { x: 500., y: 400. })), center, 0.01);
    invoke(&mut s, CommandId::Undo);
    s.frame(21, 21).unwrap();
    assert_eq!(size_of(&s), [1000, 800]);
    assert_eq!(s.engine.document().layers, before.layers, "one undo step");
    assert_eq!(s.engine.document().selection, before.selection);
}

#[test]
fn trim_shrinks_to_the_visible_pixels_and_reveal_all_brings_hidden_pixels_back() {
    let mut s = content_session([1000, 800]);
    assert!(s.command(CommandId::Trim).enabled);
    invoke(&mut s, CommandId::Trim);
    reply_bounds(&mut s, [0., 0., 542., 456.]);
    assert_eq!(size_of(&s), [542, 456], "from the pixel at 0,0 to the right and bottom edges");
    invoke(&mut s, CommandId::Undo);
    let paint = s.engine.document().layers[0].id;
    s.layer_edit(layer_core::Edit::ReplaceLayer(Box::new(layer_core::Layer {
        properties: layer_core::LayerProperties { offset: Point { x: -100., y: 0. }, ..s.engine.document().layers[0].properties.clone() },
        ..s.engine.document().layers[0].clone()
    }))).unwrap();
    s.frame(11, 11).unwrap();
    invoke(&mut s, CommandId::Trim);
    reply_bounds(&mut s, [166., 256., 442., 456.]);
    assert_eq!(size_of(&s), [276, 200], "only the pixels on the canvas count; the one at 0,0 lies beyond its left edge");
    assert_eq!(s.engine.document().layer(paint).unwrap().properties.offset, Point { x: -266., y: -256. });
    invoke(&mut s, CommandId::RevealAll);
    reply_bounds(&mut s, [-266., -256., 276., 200.]);
    let doc = s.engine.document();
    assert_eq!([doc.width, doc.height], [542, 456], "every pixel, including the one beyond the edge");
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
    s.layer_edit(layer_core::Edit::SetSelection(Some(selection))).unwrap();
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
    assert_eq!(s.engine.backend_mut().bounds_requests.last().unwrap().scope, layer_core::ContentScope::Target(before.active_target()));
    reply_bounds(&mut s, [100., 100., 300., 300.]);
    assert!(!s.content_bounds.busy());
    assert!(s.operation.active());
    invoke(&mut s, CommandId::CancelTransform);
    assert_eq!(s.engine.document(), &before);

    s.layer_edit(layer_core::Edit::SetSelection(Some(rectangle([110., 110., 290., 290.])))).unwrap();
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
