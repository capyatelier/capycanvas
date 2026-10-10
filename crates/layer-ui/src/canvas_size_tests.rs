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
    let same_screen = |a: [f32; 2], b: [f32; 2]| {
        assert!(a.into_iter().zip(b).all(|(a, b)| (a - b).abs() < 0.0001), "{a:?} != {b:?}");
    };
    let mut s = canvas_size_session();
    let paint = s.engine.document().working.occurrence.unwrap();
    invoke(&mut s, CommandId::CanvasSize);
    let view = canvas_view(&s);
    assert_eq!((view.title.as_ref(), view.values, view.unit, view.relative), ("Canvas Size", [1000.; 2], CanvasSizeUnit::Pixels, false));
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
    assert_eq!(doc.composition().size, [1200, 1100]);
    assert_eq!(doc.scene().occurrence(paint).unwrap().offset, [0, 0], "a top-left anchor keeps the origin");
    same_screen(on_screen(&s, [10., 20.]), screen);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().composition().size, [1000, 1000]);
    assert!(!s.engine.can_undo(), "one undo step");

    invoke(&mut s, CommandId::CanvasSize);
    canvas_size(&mut s, CanvasSizeAction::Anchor { anchor: CanvasAnchor::BottomRight });
    canvas_size(&mut s, CanvasSizeAction::Width { value: 1300. });
    canvas_size(&mut s, CanvasSizeAction::Height { value: 1150. });
    let screen = on_screen(&s, [10., 20.]);
    canvas_size(&mut s, CanvasSizeAction::Apply);
    let doc = s.engine.document();
    assert_eq!(doc.scene().occurrence(paint).unwrap().offset, [300 - 512, 150 - 256], "rebased by whole tiles");
    assert!(doc.extents_cover_canvas());
    same_screen(on_screen(&s, [310., 170.]), screen);
    assert_eq!(s.engine.view().document_to_surface, s.state.camera.document_to_surface());
    invoke(&mut s, CommandId::Undo);
    same_screen(on_screen(&s, [10., 20.]), screen);
    invoke(&mut s, CommandId::Redo);
    same_screen(on_screen(&s, [310., 170.]), screen);
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
    assert_eq!(doc.composition().size, [500, 1000]);
    assert_eq!(doc.scene().occurrence(doc.working.occurrence.unwrap()).unwrap().offset, [-250, 0], "centered");
    assert_eq!(doc.scene().local_extent(doc.working.occurrence.unwrap()), [1000, 1000], "the cropped pixels stay");
    invoke(&mut s, CommandId::CanvasSize);
    canvas_size(&mut s, CanvasSizeAction::Cancel);
    assert!(s.state.layer_tools.canvas_size.is_none());
    assert!(s.dispatch(UiAction::CanvasSize { action: CanvasSizeAction::Apply }).is_err());
}

#[test]
fn a_refused_apply_keeps_the_canvas_size_draft() {
    let mut s = canvas_size_session();
    let layer_core::authored::SourceTarget::Paint(paint) = s.engine.document().working.target.unwrap() else { panic!("paint") };
    let mut source = s.engine.document().artwork.paint.get(paint).unwrap().clone();
    source.raster = layer_core::raster::RasterRevision::pending();
    s.engine.apply_edit(layer_core::Edit::Paint(layer_core::authored::RecordChange::replace(&s.engine.document().artwork.paint, paint, Some(source.clone())).unwrap())).unwrap();
    invoke(&mut s, CommandId::CanvasSize);
    canvas_size(&mut s, CanvasSizeAction::Anchor { anchor: CanvasAnchor::BottomRight });
    canvas_size(&mut s, CanvasSizeAction::Width { value: 1100. });
    assert!(!canvas_view(&s).can_apply);
    assert_eq!(canvas_view(&s).message, "Raster backing is busy; retry the edit");
    let error = s.dispatch(UiAction::CanvasSize { action: CanvasSizeAction::Apply }).unwrap_err();
    assert_eq!(error, "Raster backing is busy; retry the edit");
    assert_eq!((canvas_view(&s).values, canvas_view(&s).anchor), ([1100., 1000.], CanvasAnchor::BottomRight), "the draft survives");
    assert_eq!(s.engine.document().composition().size[0], 1000);
    source.raster.publish(Ok(Default::default())).unwrap();
    invoke(&mut s, CommandId::QuickMask);
    let error = s.dispatch(UiAction::CanvasSize { action: CanvasSizeAction::Apply }).unwrap_err();
    assert_eq!(error, "Return to the artwork first", "a mode entered beside the open panel is refused");
    assert!(s.state.layer_tools.canvas_size.is_some());
    invoke(&mut s, CommandId::QuickMask);
    canvas_size(&mut s, CanvasSizeAction::Apply);
    assert!(s.state.layer_tools.canvas_size.is_none());
    assert_eq!(s.engine.document().composition().size[0], 1100);
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
    assert_eq!(doc.composition().size, [201, 200]);
    assert_eq!(doc.scene().occurrence(doc.working.occurrence.unwrap()).unwrap().offset, [-100, -50]);
    assert_eq!(doc.working.selection, before.working.selection.as_ref().map(|sel| sel.translated(Point { x: -100., y: -50. })));
    assert_eq!(on_screen(&s, [50., 50.]), screen);
    assert!(doc.scene().raster(doc.working.target.unwrap()) == before.scene().raster(before.working.target.unwrap()), "a crop changes only metadata");
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().composition().size, [1000, 1000]);
    assert_eq!(s.engine.document().working.selection, before.working.selection);
    invoke(&mut s, CommandId::SelectAll);
    assert!(s.dispatch(UiAction::Invoke { command: crop }).unwrap_err().contains("whole canvas"));
}

#[test]
fn image_menu_groups_geometry_and_color_commands_and_photo_keymaps_bind_canvas_size() {
    let s = canvas_size_session();
    let image = s.application_menu(ApplicationMenu::Image);
    let invoke = |commands: &[CommandId]| commands.iter().map(|&command| Some(UiAction::Invoke { command })).collect::<Vec<_>>();
    let actions = |sections: &[Vec<ContextMenuItem>]| sections.iter().map(|section| section.iter().map(|item| item.action.clone()).collect::<Vec<_>>()).collect::<Vec<_>>();
    assert_eq!(actions(&image.sections[..2]), [
        invoke(&[CommandId::ImageSize, CommandId::CanvasSize]),
        invoke(&[CommandId::Crop, CommandId::CropCanvasToSelection, CommandId::Trim, CommandId::RevealAll]),
    ]);
    let rotation = &image.sections[2][0];
    assert_eq!(rotation.label, "Rotate and Flip");
    assert_eq!(actions(&rotation.sections), [
        invoke(&[CommandId::RotateImageLeft, CommandId::RotateImageRight, CommandId::RotateImage180]),
        invoke(&[CommandId::FlipImageHorizontal, CommandId::FlipImageVertical]),
    ]);
    let color = &image.sections[3][0];
    assert_eq!(color.label, "Color Management");
    assert_eq!(actions(&color.sections), [invoke(&[CommandId::AssignProfile, CommandId::ConvertColorSpace, CommandId::ChangeBitDepth])]);
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
    let paint = s.engine.document().working.target.unwrap();
    let fill = s.fill_operation();
    s.paint_operation(None, fill.clone(), &[]).unwrap();
    s.frame(3, 3).unwrap();
    let (_, operation) = s.renderer_mut().pending_operations[0].clone();
    assert!(operation.coverage.selection.is_none(), "a layer without hidden pixels needs no bound");
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

#[test]
fn active_size_dialog_copy_is_retained_across_brush_and_camera_publication() {
    let localization = Localizer::shared(UiLanguage::Japanese);
    let mut s = UiSession::new_localized(Recorder::default(),
        Document::new(layer_core::PortableId::random(), 1000, 800, layer_core::DocumentNames { paint: "Literal paint".into(), paper: "Literal paper".into() }),
        [1000, 800], Platform::Gtk, localization.clone()).unwrap();
    s.state.document_file.unsaved_name = Some("写真 {document} 🖌".into());
    invoke(&mut s, CommandId::CanvasSize);
    let view = canvas_view(&s);
    assert_eq!(view.title.as_ref(), "キャンバスサイズ");
    assert_eq!(view.labels[0].as_ref(), "幅");
    assert_eq!(view.units.iter().map(|choice| choice.unit).collect::<Vec<_>>(), CanvasSizeUnit::ALL);
    assert_eq!(view.anchors.iter().map(|choice| choice.anchor).collect::<Vec<_>>(), CanvasAnchor::ALL);
    assert_eq!(view.message, "現在のサイズ：1000 × 800 px");
    for size in [8., 17.] {
        s.dispatch(UiAction::SetBrushSize { value: size }).unwrap();
        s.dispatch(UiAction::SetZoom { zoom: size / 8. }).unwrap();
        assert!(std::sync::Arc::ptr_eq(&view.title, &canvas_view(&s).title));
    }
    canvas_size(&mut s, CanvasSizeAction::Width { value: 1200. });
    assert_eq!(canvas_view(&s).message, "新しいサイズ：1200 × 800 px");
    canvas_size(&mut s, CanvasSizeAction::Cancel);
    invoke(&mut s, CommandId::ImageSize);
    let image = image_size_view(&s);
    s.dispatch(UiAction::SetBrushSize { value: 23. }).unwrap();
    s.dispatch(UiAction::SetZoom { zoom: 2. }).unwrap();
    assert!(std::sync::Arc::ptr_eq(&image.labels[0], &image_size_view(&s).labels[0]));
    assert_eq!(image.labels[0].as_ref(), "幅");
    assert_eq!(image.resamples.iter().map(|choice| choice.resample).collect::<Vec<_>>(), ImageResample::ALL);
    assert_eq!(s.state.document_file.title(), "写真 {document} 🖌");
}

#[test]
fn active_layer_menu_keeps_typed_actions_and_literal_mask_names() {
    let localization = Localizer::shared(UiLanguage::Japanese);
    let literal = "水彩 {name} 🖌";
    let mut s = UiSession::new_localized(Recorder::default(),
        Document::new(layer_core::PortableId::random(), 64, 64, layer_core::DocumentNames { paint: literal.into(), paper: "Literal paper".into() }),
        [640, 480], Platform::Gtk, localization.clone()).unwrap();
    let id = occurrence_token(s.engine.document().working.occurrence.unwrap());
    let menu = s.layer_menu(id, false).unwrap();
    assert_eq!(menu.title, format!("レイヤー：{literal}"));
    let new = menu.sections.iter().flatten().flat_map(|item| item.sections.iter().flatten()).find(|item| matches!(item.action, Some(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } }))).unwrap();
    assert_eq!(new.label, CommandId::AddLayer.localized_label(&localization).as_ref());
    s.dispatch(UiAction::Layer { action: LayerAction::AddMask { id, replace: false } }).unwrap();
    let menu = s.layer_menu(id, true).unwrap();
    assert_eq!(menu.title, format!("マスク：{literal}"));
    assert!(menu.sections.iter().flatten().any(|item| matches!(item.action, Some(UiAction::Invoke { command: CommandId::EditLayerContent }))));
    let index = ApplicationMenu::ALL.iter().position(|menu| *menu == ApplicationMenu::Layer).unwrap();
    let primary = s.header_view_with(true).primary_menu.unwrap();
    let on_open = s.application_menu(ApplicationMenu::Layer);
    assert_eq!(serde_json::to_value(&primary.sections[0][index].sections).unwrap(), serde_json::to_value(&on_open.sections).unwrap());
    s.dispatch(UiAction::Layer { action: LayerAction::Lock { id, value: true } }).unwrap();
    let primary = s.header_view_with(true).primary_menu.unwrap();
    let on_open = s.application_menu(ApplicationMenu::Layer);
    assert_eq!(serde_json::to_value(&primary.sections[0][index].sections).unwrap(), serde_json::to_value(&on_open.sections).unwrap());
    let coverage = s.coverage_menu_items(id, true);
    assert_eq!(coverage.len(), 4);
    for (item, mode) in coverage.iter().zip([SelectionMode::New, SelectionMode::Add, SelectionMode::Subtract, SelectionMode::Intersect]) {
        assert!(matches!(item.action, Some(UiAction::Selection { action: SelectionAction::LoadCoverage { id: target, mask: true, mode: actual } }) if target == id && actual == mode));
    }
}

#[test]
fn primary_layer_menu_follows_quick_mask_and_return_to_artwork() {
    let mut session = canvas_size_session();
    let index = ApplicationMenu::ALL.iter().position(|menu| *menu == ApplicationMenu::Layer).unwrap();
    let artwork = session.engine.document().working.occurrence;
    invoke(&mut session, CommandId::QuickMask);
    assert!(session.selection_masks.quick());
    assert_eq!(session.engine.document().working.occurrence, artwork);
    let primary = session.header_view_with(true).primary_menu.unwrap();
    let direct = session.application_menu(ApplicationMenu::Layer);
    assert_eq!(serde_json::to_value(&primary.sections[0][index].sections).unwrap(), serde_json::to_value(&direct.sections).unwrap());
    assert!(primary.sections[0][index].sections.iter().flatten().any(|item| matches!(item.action, Some(UiAction::Invoke { command: CommandId::ReturnToArtwork }))));
    assert!(primary.sections[0][index].sections.iter().flatten().any(|item| matches!(item.action, Some(UiAction::Invoke { command: CommandId::SaveSelectionLayer }))));
    invoke(&mut session, CommandId::ReturnToArtwork);
    assert!(!session.selection_masks.quick());
    let primary = session.header_view_with(true).primary_menu.unwrap();
    let direct = session.application_menu(ApplicationMenu::Layer);
    assert_eq!(serde_json::to_value(&primary.sections[0][index].sections).unwrap(), serde_json::to_value(&direct.sections).unwrap());
    assert!(!primary.sections[0][index].sections.iter().flatten().any(|item| matches!(item.action, Some(UiAction::Invoke { command: CommandId::ReturnToArtwork }))));
}
