fn filters() -> (UiSession<Recorder>, u32) { filters_on(Platform::Gtk) }
fn filters_on(platform: Platform) -> (UiSession<Recorder>, u32) {
    let mut s = session();
    s.set_platform(platform);
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
        layout: WorkspacePreset::Painter.layout(platform), ..WorkspaceState::default()
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
fn choose(s: &mut UiSession<Recorder>, effect: &str) {
    s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: effect.into() } }).unwrap();
}

#[test]
fn filter_drawer_replaces_the_selected_layer_after_reopening_and_cancel_is_undoable() {
    let (mut s, opener) = filters();
    assert_eq!(s.state.customization.drawer.as_ref().unwrap().columns,
        [vec![Panel::FilterTypes], vec![Panel::Adjustments], vec![Panel::Properties]]);
    assert!(s.state.adjustments.iter().all(|c| Some(&c.category) == s.state.filter_picker.category.as_ref()));
    choose(&mut s, "brightness_contrast");
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
    choose(&mut s, "curves");
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
fn filters_resolve_drawing_without_borrowing_other_masks_or_entering_groups() {
    let (mut s, _) = filters();
    choose(&mut s, "curves");
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
    doc.layers.iter_mut().find(|l| l.id == LayerId(1)).unwrap().kind = LayerKind::Background;
    assert_eq!(doc.drawing_target(), None);
    assert!(s.state.layers.iter().any(|l| l.id == 1 && l.drawing && l.selection_icon == "layer-brush-symbolic"));
    assert!(s.state.layers.iter().any(|l| l.id == first.0 && l.editing && !l.drawing));
}

#[test]
fn strokes_through_a_selected_filter_keep_selection_and_undo_on_the_drawing_target() {
    let (mut s, _) = filters();
    choose(&mut s, "curves");
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
fn paper_color_lock_history_and_blocked_cursor_share_document_policy() {
    let mut s = session();
    s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
    let controls = s.state.layer_tools.controls;
    assert!(controls.edit_lock);
    assert!(!controls.opacity && !controls.blend && !controls.mask && !controls.clip && !controls.alpha_lock);
    assert_eq!(s.state.layer_properties.controls.len(), 1);
    s.dispatch(UiAction::SetColor { rgba: [0.08, 0.1, 0.15, 1.] }).unwrap();
    let color = s.state.colors.definition();
    let action = s.state.layer_properties.controls[0].color_action.clone().unwrap();
    s.dispatch(action.clone()).unwrap();
    assert_eq!(s.engine.document().layer(LayerId(2)).unwrap().properties.paper_color, Some(color));
    assert_eq!(s.engine.view().background_rgba_linear, color.linear_in(s.engine.document().color.space).unwrap());
    let serialized = serde_json::to_string(s.engine.document()).unwrap();
    let loaded: Document = serde_json::from_str(&serialized).unwrap();
    assert_eq!(loaded.layer(LayerId(2)).unwrap().properties.paper_color, Some(color));
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.view().background_rgba_linear, [1.; 4]);
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
    let mut s = session();
    for id in [1, 2] {
        s.dispatch(UiAction::Layer { action: LayerAction::Delete { id } }).unwrap();
    }
    assert!(s.state.layers.is_empty());
    assert_eq!(s.engine.view().background_rgba_linear, [0.; 4]);
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
    let s = session();
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
fn windows_filter_drawer_uses_shared_replacement_cancel_and_history() {
    let (mut s, opener) = filters_on(Platform::Windows);
    assert_eq!(s.state.customization.drawer.as_ref().unwrap().columns,
        [vec![Panel::FilterTypes], vec![Panel::Adjustments], vec![Panel::Properties]]);
    let original = s.engine.document().layers.len();
    choose(&mut s, "brightness_contrast");
    let id = s.engine.document().active_layer;
    choose(&mut s, "exposure");
    assert_eq!(s.engine.document().active_layer, id);
    assert_eq!(s.engine.document().layers.len(), original + 1);
    s.dispatch(UiAction::ActivateHeaderItem { id: opener }).unwrap();
    s.dispatch(UiAction::ActivateHeaderItem { id: opener }).unwrap();
    assert_eq!(s.state.layer_properties.layer, Some(id.0));
    s.dispatch(UiAction::Effect { action: EffectAction::CancelFilter }).unwrap();
    assert!(s.state.customization.drawer.is_none());
    assert_eq!(s.engine.document().layers.len(), original);
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    assert!(s.engine.document().layers.iter().any(|layer| layer.id == id));
}
