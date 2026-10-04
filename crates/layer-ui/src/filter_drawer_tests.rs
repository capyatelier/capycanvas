fn filters() -> (UiSession<Recorder>, u32) {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
        layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default()
    }) }).unwrap();
    let id = s.state.workspace.layout.header.entries().find(|e| e.item == HeaderItem::Tool {
        control: ToolbarControl::Panel { panel: Panel::Adjustments }
    }).unwrap().id;
    s.dispatch(UiAction::MeasureHeader { height: 60., items: vec![HeaderItemBounds {
        id, bounds: Bounds { x: 80., y: 0., width: 40., height: 60. }
    }] }).unwrap();
    s.dispatch(UiAction::ActivateHeaderItem { id }).unwrap();
    (s, id)
}
#[test]
fn replacing_the_bottom_fill_previews_its_empty_input() {
    let (mut s, _) = filters();
    s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
    s.frame(0, 0).unwrap();
    assert!(s.request_filter_previews(1, vec!["exposure".into()], [120, 40]).unwrap());
    let request = s.engine.backend().filter_preview.as_ref().unwrap();
    assert_eq!(request.source, layer_render::FilterPreviewSource::EffectInput(LayerId(2)));
    assert_eq!(request.layers.len(), 2);
}
#[test]
fn filter_drawer_replaces_the_selected_layer_after_reopening_and_cancel_is_undoable() {
    let (mut s, opener) = filters();
    assert_eq!(s.state.customization.drawer.as_ref().unwrap().columns,
        [vec![Panel::FilterTypes], vec![Panel::Adjustments], vec![Panel::Properties]]);
    assert!(s.state.adjustments.iter().all(|c| Some(&c.category) == s.state.filter_picker.category.as_ref()));
    insert_effect(&mut s, "brightness_contrast");
    let id = s.engine.document().active_layer;
    assert_eq!(s.engine.document().layers.iter().map(|l| l.id).collect::<Vec<_>>(), [id, LayerId(1), LayerId(2)]);
    assert!(s.filter_drawer_open());
    s.dispatch(UiAction::Effect { action: EffectAction::Set {
        layer: id.0, key: "brightness".into(), value: layer_core::EffectValue::Number(0.3),
    } }).unwrap();
    let edited = s.engine.document().layer(id).unwrap().clone();
    s.dispatch(UiAction::ActivateHeaderItem { id: opener }).unwrap();
    assert!(!s.filter_drawer_open());
    s.dispatch(UiAction::ActivateHeaderItem { id: opener }).unwrap();
    assert_eq!(s.engine.document().layer(id), Some(&edited));
    insert_effect(&mut s, "curves");
    assert_eq!(s.engine.document().active_layer, id);
    assert_eq!(s.engine.document().layers.len(), 3);
    assert_eq!(s.state.filter_picker.selected.as_deref(), Some("curves"));
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().layer(id), Some(&edited));
    invoke(&mut s, CommandId::Redo);
    s.dispatch(UiAction::Effect { action: EffectAction::CancelFilter }).unwrap();
    assert!(!s.filter_drawer_open());
    assert!(s.engine.document().layer(id).is_none());
    invoke(&mut s, CommandId::Undo);
    assert!(s.engine.document().layer(id).is_some());
}

#[test]
fn filter_replacement_and_undo_preserve_literal_renamed_layer_names() {
    let (mut s, _) = filters();
    insert_effect(&mut s, "brightness_contrast");
    let id = s.engine.document().active_layer;
    let name = "私の曲線 한글 🎨 { $name } \u{2068}لوحة\u{2069}";
    s.dispatch(UiAction::Layer { action: LayerAction::Rename {
        id: id.0, name: name.into(),
    } }).unwrap();
    assert!(s.filter_drawer_open());
    insert_effect(&mut s, "curves");
    let layer = s.engine.document().layer(id).unwrap();
    assert_eq!(layer.name.as_ref(), name);
    assert_eq!(layer.effect.as_ref().unwrap().program.id.as_ref(), "curves");
    invoke(&mut s, CommandId::Undo);
    let layer = s.engine.document().layer(id).unwrap();
    assert_eq!(layer.name.as_ref(), name);
    assert_eq!(layer.effect.as_ref().unwrap().program.id.as_ref(), "brightness_contrast");
    invoke(&mut s, CommandId::Redo);
    assert_eq!(s.engine.document().layer(id).unwrap().name.as_ref(), name);
}

#[test]
fn the_filter_drawer_masks_a_new_effect_but_never_the_one_it_replaces() {
    let (mut s, _) = filters();
    let selection = layer_core::Selection::polygon(vec![
        Point { x: 50., y: 50. },
        Point { x: 250., y: 50. },
        Point { x: 250., y: 200. },
    ])
    .unwrap();
    s.layer_edit(layer_core::Edit::SetSelection(Some(selection.clone()))).unwrap();
    insert_effect(&mut s, "brightness_contrast");
    let id = s.engine.document().active_layer;
    let mask = s.engine.document().layer(id).unwrap().mask.clone().expect("a new effect takes the selection");
    assert_eq!(mask.initial.as_ref(), Some(&selection));
    invoke(&mut s, CommandId::Reselect);
    insert_effect(&mut s, "curves");
    assert_eq!(s.engine.document().active_layer, id, "the drawer replaces the effect");
    assert_eq!(s.engine.document().layer(id).unwrap().mask, Some(mask), "and keeps its mask");
    assert_eq!(s.engine.document().selection, Some(selection), "replacing leaves the selection");
}

#[test]
fn filters_resolve_drawing_without_borrowing_other_masks_or_entering_groups() {
    let (mut s, _) = filters();
    insert_effect(&mut s, "curves");
    let first = s.engine.document().active_layer;
    let mut doc = s.engine.document().clone();
    assert_eq!(doc.drawing_target(), Some(LayerId(1)));
    let mut upper = doc.layer(first).unwrap().clone();
    upper.id = LayerId(20);
    doc.layers.insert(0, upper);
    doc.active_layer = LayerId(20);
    doc.layers.iter_mut().find(|l| l.id == first).unwrap().mask = Some(layer_core::LayerMask::reveal_all(LayerId(21), Point::default()));
    assert_eq!(doc.drawing_target(), Some(LayerId(1)), "ignore a lower filter's mask");
    doc.layers[0].properties.clipped = true;
    doc.layers.iter_mut().find(|l| l.id == first).unwrap().properties.clipped = true;
    assert_eq!(doc.drawing_target(), Some(LayerId(1)), "clipped filter uses its base");
    doc.layers[0].mask = Some(layer_core::LayerMask::reveal_all(LayerId(22), Point::default()));
    assert_eq!(doc.drawing_target(), Some(LayerId(22)), "own mask wins without a separate mask click");
    doc.layers[0].properties.locked = true;
    assert_eq!(doc.drawing_target(), None);
    doc.layers[0].properties.locked = false;
    doc.layers[0].mask = None;
    doc.layers[0].properties.clipped = false;
    doc.layers.iter_mut().find(|l| l.id == LayerId(1)).unwrap().properties.locked = true;
    assert_eq!(doc.drawing_target(), None, "do not skip a locked drawing layer");
    let base = doc.layers.iter_mut().find(|l| l.id == LayerId(1)).unwrap();
    base.properties.locked = false;
    base.kind = LayerKind::Group;
    base.mask = Some(layer_core::LayerMask::reveal_all(LayerId(23), Point::default()));
    assert_eq!(doc.drawing_target(), None, "a group and its mask are not drawing fallbacks");
    *doc.layers.iter_mut().find(|l| l.id == LayerId(1)).unwrap() = layer_core::Layer::solid_color(LayerId(1), "Fill", layer_core::color::RgbColor::WHITE);
    assert_eq!(doc.drawing_target(), None);
    assert!(s.state.layers.iter().any(|l| l.id == 1 && l.drawing && l.selection_icon == "layer-brush-symbolic"));
    assert!(s.state.layers.iter().any(|l| l.id == first.0 && l.editing && !l.drawing));
}

#[test]
fn strokes_through_a_selected_filter_keep_selection_and_undo_on_the_drawing_target() {
    let (mut s, _) = filters();
    insert_effect(&mut s, "curves");
    let filter = s.engine.document().active_layer;
    let stroke = |s: &mut UiSession<Recorder>| {
        for (sequence, phase) in [(1, PenPhase::Down), (2, PenPhase::Move), (3, PenPhase::Up)] {
            s.pen(event(s, sequence, phase, 1.)).unwrap();
        }
        s.frame(30_000_000, 38_000_000).unwrap();
    };
    stroke(&mut s);
    assert_eq!(s.engine.document().active_layer, filter);
    assert!(!s.engine.document().layer(LayerId(1)).unwrap().raster.is_empty());
    assert!(s.engine.document().layer(filter).unwrap().raster.is_empty());
    invoke(&mut s, CommandId::Undo);
    assert!(s.engine.document().layer(LayerId(1)).unwrap().raster.is_empty());
    s.dispatch(UiAction::Layer { action: LayerAction::AddMask { id: filter.0, replace: false } }).unwrap();
    s.dispatch(UiAction::SelectLayer { id: filter.0 }).unwrap();
    assert!(!s.engine.document().active_mask);
    stroke(&mut s);
    assert_eq!(s.engine.document().active_layer, filter);
    assert!(!s.engine.document().layer(filter).unwrap().mask.as_ref().unwrap().raster.is_empty());
    assert!(s.engine.document().layer(LayerId(1)).unwrap().raster.is_empty());
    invoke(&mut s, CommandId::Undo);
    assert!(s.engine.document().layer(filter).unwrap().mask.as_ref().unwrap().raster.is_empty());
    s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: filter.0, value: true } }).unwrap();
    stroke(&mut s);
    assert!(s.engine.document().layer(filter).unwrap().mask.as_ref().unwrap().raster.is_empty());
}

#[test]
fn fill_color_lock_history_and_blocked_cursor_share_document_policy() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
    let controls = s.state.layer_tools.controls;
    assert!(controls.edit_lock);
    assert!(controls.opacity && controls.blend && controls.mask && !controls.alpha_lock);
    assert_eq!(s.state.layer_properties.controls.len(), 1);
    let revision = s.state.layers.iter().find(|layer| layer.id == 2).unwrap().paint_revision;
    s.dispatch(UiAction::SetColor { rgba: [0.08, 0.1, 0.15, 1.] }).unwrap();
    let color = s.state.colors.definition();
    let action = s.state.layer_properties.controls[0].color_action.clone().unwrap();
    s.dispatch(action.clone()).unwrap();
    let changed = s.state.layers.iter().find(|layer| layer.id == 2).unwrap().paint_revision;
    assert_ne!(changed, revision);
    assert!(changed < 1u64 << 53);
    assert_eq!(s.engine.document().layer(LayerId(2)).unwrap().effect.as_ref().unwrap().constant_color(), Some(color));
    let serialized = serde_json::to_string(s.engine.document()).unwrap();
    let loaded: Document = serde_json::from_str(&serialized).unwrap();
    assert_eq!(loaded.layer(LayerId(2)).unwrap().effect.as_ref().unwrap().constant_color(), Some(color));
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().layer(LayerId(2)).unwrap().effect.as_ref().unwrap().constant_color(), Some(layer_core::color::RgbColor::WHITE));
    invoke(&mut s, CommandId::Redo);
    s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: 2, value: true } }).unwrap();
    assert!(s.dispatch(action).is_err());
    s.cursor_input(Some(event(&s, 1, PenPhase::Hover, 0.)));
    assert_eq!(s.canvas_cursor().unwrap().segments[0].marker, 6.);
    s.dispatch(UiAction::SelectLayer { id: 1 }).unwrap();
    assert!(s.canvas_cursor().unwrap().segments.iter().all(|p| p.marker != 6.));
}

#[test]
fn empty_layer_stack_roundtrips_and_accepts_a_new_layer_with_undo() {
    let mut s = session(Platform::Gtk);
    for id in [1, 2] {
        s.dispatch(UiAction::Layer { action: LayerAction::Delete { id } }).unwrap();
    }
    assert!(s.state.layers.is_empty());
    let project = s.capture_project_recovery().unwrap();
    let mut bytes = Vec::new();
    project.write(&mut bytes).unwrap();
    let loaded = layer_core::Project::read(bytes.as_slice(), Default::default()).unwrap();
    assert!(loaded.document.layers.is_empty());
    s.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } }).unwrap();
    assert_eq!(s.state.layers.len(), 1);
    invoke(&mut s, CommandId::Undo);
    assert!(s.state.layers.is_empty());
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.state.layers.len(), 1);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.state.layers.len(), 2);
}

#[test]
fn animated_speed_changes_preserve_playback_phase_including_zero_and_restored_speed() {
    let s = session(Platform::Gtk);
    let definition = s.effect_catalog.filters().iter().find(|d| d.program.time && d.program.parameters.iter().any(|p| &*p.key == "speed")).unwrap();
    let mut effect = layer_core::EffectInstance::new(definition.program());
    effect.set("animate", layer_core::EffectValue::Toggle(true)).unwrap();
    effect.set("speed", layer_core::EffectValue::Number(1.)).unwrap();
    let mut clock = layer_core::EffectClock::default();
    assert_eq!(clock.advance(&effect, 10.), 10.);
    effect.set("speed", layer_core::EffectValue::Number(2.)).unwrap();
    assert_eq!(clock.advance(&effect, 10.), 10.);
    assert_eq!(clock.advance(&effect, 11.), 12.);
    effect.set("speed", layer_core::EffectValue::Number(0.)).unwrap();
    assert_eq!(clock.advance(&effect, 11.), 12.);
    assert_eq!(clock.advance(&effect, 21.), 12.);
    effect.set("speed", layer_core::EffectValue::Number(1.)).unwrap();
    assert_eq!(clock.advance(&effect, 21.), 12.);
    assert_eq!(clock.advance(&effect, 22.), 13.);
}

#[test]
fn curve_points_detach_off_the_graph_until_release_and_commit_one_step() {
    let mut s = session(Platform::Gtk);
    insert_effect(&mut s, "curves");
    let layer = s.state.layer_properties.layer.unwrap();
    let curve = |s: &UiSession<Recorder>| match &s.state.layer_properties.controls.iter().find(|c| c.key == "curve_0").unwrap().value {
        layer_core::EffectValue::Curve(points) => points.clone(),
        _ => unreachable!(),
    };
    let point = |index, point| EffectAction::CurvePoint { layer, key: "curve_0".into(), index, point, remove: false };
    let drag = |s: &mut UiSession<Recorder>, phase, index, at| {
        s.dispatch(UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(point(index, at)) } }).unwrap();
    };
    s.dispatch(UiAction::Effect { action: point(None, [0.5, 0.75]) }).unwrap();
    let placed = vec![[0., 0.], [0.5, 0.75], [1., 1.]];
    assert_eq!(curve(&s), placed);

    drag(&mut s, ContactPhase::Down, Some(1), [0.5, 0.75]);
    drag(&mut s, ContactPhase::Move, Some(1), [0.5, 1.05]);
    assert_eq!(curve(&s), [[0., 0.], [0.5, 1.], [1., 1.]], "Overshoot inside the margin clamps");
    drag(&mut s, ContactPhase::Move, Some(1), [0.5, 1.2]);
    assert_eq!(curve(&s), [[0., 0.], [1., 1.]]);
    drag(&mut s, ContactPhase::Move, Some(1), [1.3, 0.25]);
    assert_eq!(curve(&s), [[0., 0.], [1., 1.]]);
    drag(&mut s, ContactPhase::Move, Some(1), [0.25, 0.5]);
    assert_eq!(curve(&s), [[0., 0.], [0.25, 0.5], [1., 1.]], "Returning restores the point");
    drag(&mut s, ContactPhase::Move, Some(1), [-0.5, 0.5]);
    drag(&mut s, ContactPhase::Up, Some(1), [-0.5, 0.5]);
    assert_eq!(curve(&s), [[0., 0.], [1., 1.]]);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(curve(&s), placed);

    drag(&mut s, ContactPhase::Down, Some(1), [0.5, 0.75]);
    drag(&mut s, ContactPhase::Move, Some(1), [0.5, -0.5]);
    drag(&mut s, ContactPhase::Cancel, Some(1), [0.5, -0.5]);
    assert_eq!(curve(&s), placed);

    drag(&mut s, ContactPhase::Down, Some(0), [0., 0.]);
    drag(&mut s, ContactPhase::Move, Some(0), [-0.5, 0.25]);
    drag(&mut s, ContactPhase::Up, Some(0), [-0.5, 0.25]);
    assert_eq!(curve(&s), [[0., 0.25], [0.5, 0.75], [1., 1.]], "Endpoints stay and follow the pointer");

    let modified = |s: &UiSession<Recorder>| s.state.layer_properties.controls.iter().find(|c| c.key == "curve_0").unwrap().modified;
    assert!(modified(&s));
    s.dispatch(UiAction::Effect { action: EffectAction::Reset { layer, key: "curve_0".into() } }).unwrap();
    assert_eq!(curve(&s), [[0., 0.], [1., 1.]]);
    assert!(!modified(&s), "Reset hides the on-chart reset control");
}

fn menu_item<'a>(sections: &'a [Vec<ContextMenuItem>], label: &str) -> Option<&'a ContextMenuItem> {
    sections.iter().flatten().find_map(|item| if item.label == label { Some(item) } else { menu_item(&item.sections, label) })
}

#[test]
fn fill_layers_start_from_the_current_color_and_mask_to_the_selection() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::SetColor { rgba: [0.9, 0.2, 0.1, 1.] }).unwrap();
    let base = s.engine.document().active_layer;
    let menu = s.application_menu(ApplicationMenu::Layer);
    let new = menu_item(&menu.sections, "New").unwrap();
    let solid = menu_item(&new.sections, "Solid Color Fill").unwrap().clone();
    assert!(solid.enabled);
    assert!(menu_item(&new.sections, "Gradient Fill").is_some());
    let before = s.engine.document().layers.clone();
    s.dispatch(solid.action.unwrap()).unwrap();
    let doc = s.engine.document();
    let fill = doc.layer(doc.active_layer).unwrap();
    assert_eq!((fill.kind, fill.name.as_ref()), (LayerKind::Effect, "Solid Color"));
    let effect = fill.effect.as_ref().unwrap();
    assert_eq!(effect.program.kind, layer_core::EffectKind::Generator);
    assert_eq!(effect.value("color"), Some(&layer_core::EffectValue::Color(s.state.colors.definition())));
    let mask = fill.mask.as_ref().expect("painting on a fill goes to its mask");
    assert_eq!((mask.initial.as_ref(), mask.default_coverage), (None, 1.), "a reveal-all mask");
    assert_eq!(doc.layers.iter().position(|l| l.id == fill.id).unwrap() + 1, doc.layers.iter().position(|l| l.id == base).unwrap());
    assert_eq!(doc.drawing_target(), Some(mask.id));
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().layers, before, "inserting is one undo step");

    let selection = layer_core::Selection::polygon(vec![
        Point { x: 40., y: 40. },
        Point { x: 240., y: 60. },
        Point { x: 120., y: 200. },
    ])
    .unwrap();
    s.layer_edit(layer_core::Edit::SetSelection(Some(selection.clone()))).unwrap();
    s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "gradient_fill".into() } }).unwrap();
    let doc = s.engine.document();
    let fill = doc.layer(doc.active_layer).unwrap();
    assert_eq!(fill.effect.as_ref().unwrap().program.id.as_ref(), "gradient_fill");
    assert_eq!(fill.mask.as_ref().unwrap().initial.as_ref(), Some(&selection), "the selection becomes the mask");
    assert_eq!(doc.selection, None, "and is consumed");
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().selection, Some(selection), "undo restores the selection");
}

#[test]
fn the_filter_menu_leaves_fill_generators_to_layer_new() {
    let s = session(Platform::Gtk);
    let generators: Vec<_> = s.effect_catalog.filters().iter()
        .filter(|f| f.program.kind == layer_core::EffectKind::Generator)
        .map(|f| effects::resource_label(f.label(), &s.state.localization).to_string())
        .collect();
    assert_eq!(generators, ["Solid Color", "Gradient Fill"]);
    let menu = s.application_menu(ApplicationMenu::Filter);
    assert!(menu.sections[0].iter().all(|c| c.label != "Fill" && !c.sections.is_empty()));
    for label in &generators {
        assert!(menu_item(&menu.sections, label).is_none(), "{label} is not a filter");
    }
    assert!(menu_item(&menu.sections, "Curves").is_some());
    assert!(s.state.adjustments.iter().any(|c| c.id.as_ref() == "solid_color" && c.category_icon == "fill"),
        "the effect browser still lists fills");
}

#[test]
fn every_effect_color_control_offers_the_current_color() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::SetColor { rgba: [0.1, 0.7, 0.3, 1.] }).unwrap();
    let with_colors: Vec<_> = s.effect_catalog.filters().iter()
        .filter(|f| f.program.parameters.iter().any(|p| p.kind == layer_core::EffectParameterKind::Color))
        .map(|f| f.id().to_string())
        .collect();
    for id in ["black_white", "split_tone", "halftone", "crosshatch", "pencil", "solid_color"] {
        assert!(with_colors.iter().any(|c| c == id), "{id}");
    }
    for id in with_colors {
        s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: id.as_str().into() } }).unwrap();
        let layer = s.engine.document().active_layer.0;
        let colors: Vec<_> = s.state.layer_properties.controls.iter()
            .filter(|c| c.kind == PropertyKind::Color).cloned().collect();
        assert!(!colors.is_empty(), "{id}");
        for control in colors {
            let action = UiAction::Effect { action: EffectAction::UseCurrentColor { layer, key: control.key.clone() } };
            assert_eq!(control.color_action.as_ref(), Some(&action), "{id}.{}", control.key);
            assert!(!control.label.is_empty());
            s.dispatch(action).unwrap();
            let value = s.engine.document().layer(LayerId(layer)).unwrap().effect.as_ref().unwrap().value(&control.key).cloned();
            assert_eq!(value, Some(layer_core::EffectValue::Color(s.state.colors.definition())), "{id}.{}", control.key);
        }
    }
}
