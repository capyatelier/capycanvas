fn canvas_size(s: &mut UiSession<Recorder>, action: CanvasSizeAction) -> UiChange {
    s.dispatch(UiAction::CanvasSize { action }).unwrap()
}

fn canvas_view(s: &UiSession<Recorder>) -> CanvasSizeView {
    s.state.layer_tools.canvas_size.clone().expect("Canvas Size is open")
}

/// Where the document point lies on screen.
fn on_screen(s: &UiSession<Recorder>, point: [f32; 2]) -> [f32; 2] {
    let [a, b, c, d, x, y] = s.state.camera.document_to_surface();
    [a * point[0] + c * point[1] + x, b * point[0] + d * point[1] + y]
}

fn canvas_size_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
    invoke(&mut s, CommandId::FitCanvas);
    s.frame(1, 1).unwrap();
    s
}

#[test]
fn canvas_size_grows_from_an_anchor_in_one_undo_step_and_keeps_the_view_still() {
    let mut s = canvas_size_session();
    let paint = s.engine.document().layers[0].id;
    invoke(&mut s, CommandId::CanvasSize);
    let view = canvas_view(&s);
    assert_eq!((view.title, view.values, view.unit, view.relative), ("Canvas Size", [1000.; 2], CanvasSizeUnit::Pixels, false));
    assert_eq!(view.anchor, CanvasAnchor::Center);
    assert_eq!(view.anchors.len(), 9);
    assert!(!view.can_apply);
    assert_eq!(view.message, "Current size: 1000 × 1000 px");
    canvas_size(&mut s, CanvasSizeAction::Width { value: 1200.4 });
    canvas_size(&mut s, CanvasSizeAction::Height { value: 1100. });
    canvas_size(&mut s, CanvasSizeAction::Anchor { anchor: CanvasAnchor::TopLeft });
    let view = canvas_view(&s);
    assert_eq!(view.values, [1200., 1100.]);
    assert!(view.can_apply);
    assert_eq!(view.message, "New size: 1200 × 1100 px");
    let screen = on_screen(&s, [10., 20.]);
    let change = canvas_size(&mut s, CanvasSizeAction::Apply);
    assert_ne!(change.regions & regions::DOCUMENT, 0);
    assert!(s.state.layer_tools.canvas_size.is_none());
    let doc = s.engine.document();
    assert_eq!([doc.width, doc.height], [1200, 1100]);
    assert_eq!(doc.layer(paint).unwrap().properties.offset, Point::default(), "a top-left anchor keeps the origin");
    assert_eq!(on_screen(&s, [10., 20.]), screen);
    invoke(&mut s, CommandId::Undo);
    assert_eq!([s.engine.document().width, s.engine.document().height], [1000, 1000]);
    assert!(!s.engine.can_undo(), "one undo step");

    invoke(&mut s, CommandId::CanvasSize);
    canvas_size(&mut s, CanvasSizeAction::Anchor { anchor: CanvasAnchor::BottomRight });
    canvas_size(&mut s, CanvasSizeAction::Width { value: 1300. });
    canvas_size(&mut s, CanvasSizeAction::Height { value: 1150. });
    let screen = on_screen(&s, [10., 20.]);
    canvas_size(&mut s, CanvasSizeAction::Apply);
    let doc = s.engine.document();
    assert_eq!(doc.layer(paint).unwrap().properties.offset, Point { x: 300. - 512., y: 150. - 256. }, "rebased by whole tiles");
    assert!(doc.extents_cover_canvas());
    assert_eq!(on_screen(&s, [310., 170.]), screen, "the image stays where it was");
    assert_eq!(s.engine.view().document_to_surface, s.state.camera.document_to_surface());
    invoke(&mut s, CommandId::Undo);
    assert_eq!(on_screen(&s, [10., 20.]), screen, "undo keeps the image still too");
    invoke(&mut s, CommandId::Redo);
    assert_eq!(on_screen(&s, [310., 170.]), screen);
}

#[test]
fn canvas_size_converts_percent_and_relative_values_and_validates_limits() {
    let mut s = canvas_size_session();
    s.renderer_mut().max_dimension = Some(4096);
    invoke(&mut s, CommandId::CanvasSize);
    canvas_size(&mut s, CanvasSizeAction::Unit { unit: CanvasSizeUnit::Percent });
    let view = canvas_view(&s);
    assert_eq!(view.values, [100.; 2]);
    assert_eq!(view.numeric[0].unit, "%");
    assert_eq!(view.numeric[0].max, 409.6);
    canvas_size(&mut s, CanvasSizeAction::Width { value: 50. });
    assert_eq!(canvas_view(&s).message, "New size: 500 × 1000 px");
    canvas_size(&mut s, CanvasSizeAction::Relative { relative: true });
    assert_eq!(canvas_view(&s).values, [-50., 0.]);
    canvas_size(&mut s, CanvasSizeAction::Unit { unit: CanvasSizeUnit::Pixels });
    let view = canvas_view(&s);
    assert_eq!(view.values, [-500., 0.]);
    assert_eq!((view.numeric[0].min, view.numeric[0].max), (-999., 3096.));
    canvas_size(&mut s, CanvasSizeAction::Relative { relative: false });
    assert_eq!(canvas_view(&s).values, [500., 1000.]);
    canvas_size(&mut s, CanvasSizeAction::Width { value: 5000. });
    let view = canvas_view(&s);
    assert!(!view.can_apply);
    assert_eq!(view.message, "The canvas can be at most 4096 px on each side");
    canvas_size(&mut s, CanvasSizeAction::Width { value: 0. });
    assert!(!canvas_view(&s).can_apply);
    assert!(s.dispatch(UiAction::CanvasSize { action: CanvasSizeAction::Width { value: f64::NAN } }).is_err());
    canvas_size(&mut s, CanvasSizeAction::Width { value: 500. });
    canvas_size(&mut s, CanvasSizeAction::Apply);
    let doc = s.engine.document();
    assert_eq!([doc.width, doc.height], [500, 1000]);
    assert_eq!(doc.layers[0].properties.offset, Point { x: -250., y: 0. }, "centered");
    assert_eq!(doc.layers[0].properties.extent, Some([1000, 1000]), "the cropped pixels stay");
    invoke(&mut s, CommandId::CanvasSize);
    canvas_size(&mut s, CanvasSizeAction::Cancel);
    assert!(s.state.layer_tools.canvas_size.is_none());
    assert!(s.dispatch(UiAction::CanvasSize { action: CanvasSizeAction::Apply }).is_err());
}

#[test]
fn a_refused_apply_keeps_the_canvas_size_draft() {
    let mut s = canvas_size_session();
    let mut layer = s.engine.document().layers[0].clone();
    layer.raster = layer_core::raster::RasterRevision::pending();
    s.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(layer.clone()))).unwrap();
    invoke(&mut s, CommandId::CanvasSize);
    canvas_size(&mut s, CanvasSizeAction::Anchor { anchor: CanvasAnchor::BottomRight });
    canvas_size(&mut s, CanvasSizeAction::Width { value: 1100. });
    assert!(!canvas_view(&s).can_apply);
    assert_eq!(canvas_view(&s).message, "Raster backing is busy; retry the edit");
    let error = s.dispatch(UiAction::CanvasSize { action: CanvasSizeAction::Apply }).unwrap_err();
    assert_eq!(error, "Raster backing is busy; retry the edit");
    assert_eq!((canvas_view(&s).values, canvas_view(&s).anchor), ([1100., 1000.], CanvasAnchor::BottomRight), "the draft survives");
    assert_eq!(s.engine.document().width, 1000);
    layer.raster.publish(Ok(Default::default())).unwrap();
    invoke(&mut s, CommandId::QuickMask);
    let error = s.dispatch(UiAction::CanvasSize { action: CanvasSizeAction::Apply }).unwrap_err();
    assert_eq!(error, "Return to the artwork first", "a mode entered beside the open panel is refused");
    assert!(s.state.layer_tools.canvas_size.is_some());
    invoke(&mut s, CommandId::QuickMask);
    canvas_size(&mut s, CanvasSizeAction::Apply);
    assert!(s.state.layer_tools.canvas_size.is_none());
    assert_eq!(s.engine.document().width, 1100);
}

#[test]
fn crop_canvas_to_selection_uses_the_coverage_bounds_and_refuses_inverted_selections() {
    let mut s = canvas_size_session();
    let crop = CommandId::CropCanvasToSelection;
    assert_eq!(s.command_disabled_reason(crop).as_deref(), Some("Make a selection first"));
    rectangle_selection(&mut s, [100.4, 50., 300.6, 250.]);
    invoke(&mut s, CommandId::InvertSelection);
    assert_eq!(s.command_disabled_reason(crop).as_deref(), Some("An inverted selection has no bounds to crop to"));
    invoke(&mut s, CommandId::InvertSelection);
    assert!(s.command(crop).enabled);
    invoke(&mut s, CommandId::RectangleSelect);
    let bar = s.state.canvas_bar.clone().unwrap();
    let item = bar.items.iter().find(|i| matches!(&i.option, ToolOption::Action { state, .. } if state.id == crop)).unwrap();
    assert_eq!(item.label.as_ref(), "Crop");
    let before = s.engine.document().clone();
    let screen = on_screen(&s, [150., 100.]);
    s.dispatch(UiAction::CanvasBarEdit { context: bar.context, action: Box::new(UiAction::Invoke { command: crop }) }).unwrap();
    let doc = s.engine.document();
    assert_eq!([doc.width, doc.height], [201, 200]);
    assert_eq!(doc.layers[0].properties.offset, Point { x: -100., y: -50. });
    assert_eq!(doc.selection, before.selection.as_ref().map(|sel| sel.translated(Point { x: -100., y: -50. })));
    assert_eq!(on_screen(&s, [50., 50.]), screen);
    assert!(doc.layers[0].raster == before.layers[0].raster, "a crop changes only metadata");
    invoke(&mut s, CommandId::Undo);
    assert_eq!([s.engine.document().width, s.engine.document().height], [1000, 1000]);
    assert_eq!(s.engine.document().selection, before.selection);
    invoke(&mut s, CommandId::SelectAll);
    assert!(s.dispatch(UiAction::Invoke { command: crop }).unwrap_err().contains("whole canvas"));
}

#[test]
fn edit_image_submenu_holds_the_geometry_commands_and_photo_keymaps_bind_canvas_size() {
    let s = canvas_size_session();
    let edit = s.application_menu(ApplicationMenu::Edit);
    let mut seen = Vec::new();
    let image = edit.sections.iter().flatten().find(|i| i.label == "Image").expect("Edit ▸ Image");
    let image: Vec<Vec<_>> = image.sections.iter().map(|s| s.iter().map(|i| i.action.clone()).collect()).collect();
    let invoke = |commands: &[CommandId]| commands.iter().map(|&command| Some(UiAction::Invoke { command })).collect::<Vec<_>>();
    assert_eq!(image, [
        invoke(&[CommandId::Crop, CommandId::CropCanvasToSelection, CommandId::CanvasSize, CommandId::ImageSize]),
        invoke(&[CommandId::RotateImageLeft, CommandId::RotateImageRight, CommandId::RotateImage180]),
        invoke(&[CommandId::FlipImageHorizontal, CommandId::FlipImageVertical]),
        invoke(&[CommandId::Trim, CommandId::RevealAll]),
    ]);
    for item in edit.sections.iter().flatten().filter(|i| i.action.is_some()) {
        assert!(!seen.contains(&item.action), "{}", item.label);
        seen.push(item.action.clone());
    }
    assert!(edit.sections.iter().all(|section| !section.is_empty()));
    let chord = KeyChord { key: "c".into(), command: true, alt: true, shift: false };
    for platform in [Platform::Gtk, Platform::Web] {
        assert!(chord.available(platform));
    }
    for (preset, bound) in [("capy", false), ("photoshop", true), ("affinity", true), ("gimp", false)] {
        let keys = crate::keymaps::preset(preset).unwrap().keys_for(&CommandId::CanvasSize.shortcut_id()).unwrap_or_default();
        assert_eq!(keys.contains(&chord), bound, "{preset}");
    }
}

#[test]
fn unselected_paint_operations_stay_inside_the_canvas_window() {
    let mut s = canvas_size_session();
    s.frame(2, 2).unwrap();
    let paint = s.engine.document().layers[0].id;
    let fill = s.fill_operation();
    s.paint_operation(None, fill.clone(), &[]).unwrap();
    s.frame(3, 3).unwrap();
    let (_, operation) = s.renderer_mut().pending_operations[0].clone();
    assert!(operation.coverage.initial.is_none(), "a layer without hidden pixels needs no bound");
    rectangle_selection(&mut s, [0., 0., 400., 400.]);
    invoke(&mut s, CommandId::CropCanvasToSelection);
    invoke(&mut s, CommandId::Deselect);
    s.frame(4, 4).unwrap();
    s.paint_operation(None, fill, &[]).unwrap();
    s.frame(5, 5).unwrap();
    let (id, operation) = s.renderer_mut().pending_operations[0].clone();
    assert_eq!(id, paint);
    let bounds = operation.bounds(s.engine.document().target_extent(paint));
    assert!(bounds.max.x <= 401. && bounds.max.y <= 401., "{bounds:?}");
}
