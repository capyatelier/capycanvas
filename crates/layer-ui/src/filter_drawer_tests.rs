fn paper_color(document:&layer_core::Document)->Option<layer_core::color::RgbColor> {
    let scene = document.scene();
    scene.order().iter().rev().find_map(|id| scene.effect(*id)?.constant_color())
}
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
fn adding_an_adjustment_above_a_fill_previews_the_fill() {
    let (mut s, _) = filters();
    s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
    s.frame(0, 0).unwrap();
    assert!(s.request_filter_previews(1, vec!["exposure".into()], [120, 40]).unwrap());
    let request = s.engine.backend().filter_preview.as_ref().unwrap();
    assert_eq!(request.source, layer_render::FilterPreviewSource::LayerStack(occurrence_handle(2).unwrap()));
    assert_eq!(request.snapshot.view().order().len(), 2);
}
#[test]
fn filter_drawer_replaces_the_selected_layer_after_reopening_and_cancel_is_undoable() {
    let (mut s, opener) = filters();
    assert_eq!(s.state.customization.drawer.as_ref().unwrap().columns,
        [vec![Panel::FilterTypes], vec![Panel::Adjustments], vec![Panel::Properties]]);
    assert!(s.state.adjustments.iter().all(|c| Some(&c.category) == s.state.filter_picker.category.as_ref()));
    let base=s.engine.document().scene().order()[0];let paper=s.engine.document().scene().order()[1];
    insert_effect(&mut s, "brightness_contrast");
    let id = s.engine.document().working.occurrence.unwrap();
    assert_eq!(s.engine.document().scene().order(), &[id,base,paper]);
    assert!(s.filter_drawer_open());
    s.dispatch(UiAction::Effect { action: EffectAction::Set {
        layer: occurrence_token(id), key: "brightness".into(), value: layer_core::EffectValue::Number(0.3),
    } }).unwrap();
    let edited=s.engine.document().clone();
    let application=edited.scene().effect_handle(id).unwrap();
    s.dispatch(UiAction::ActivateHeaderItem { id: opener }).unwrap();
    assert!(!s.filter_drawer_open());
    s.dispatch(UiAction::ActivateHeaderItem { id: opener }).unwrap();
    assert_live_artwork_eq(s.engine.document(),&edited);
    insert_effect(&mut s, "curves");
    assert_eq!(s.engine.document().working.occurrence.unwrap(), id);
    assert_eq!(s.engine.document().scene().order().len(), 3);
    assert_eq!(s.state.filter_picker.selected.as_deref(), Some("curves"));
    assert_eq!(s.engine.document().artwork.effects.get(application).unwrap().program.id.as_ref(), "curves");
    assert_eq!(package_roundtrip(s.engine.document()).artwork.effects.len(),edited.artwork.effects.len());
    invoke(&mut s, CommandId::Undo);
    assert_live_artwork_eq(s.engine.document(),&edited);
    assert_eq!(s.engine.document().artwork.effects.get(application), edited.artwork.effects.get(application));
    invoke(&mut s, CommandId::Redo);
    s.dispatch(UiAction::Effect { action: EffectAction::CancelFilter }).unwrap();
    assert!(!s.filter_drawer_open());
    assert!(s.engine.document().scene().occurrence(id).is_none());
    assert_eq!(s.engine.document().artwork.effects.len(),1);
    invoke(&mut s, CommandId::Undo);
    assert!(s.engine.document().scene().occurrence(id).is_some());
}

#[test]
fn filter_replacement_and_undo_preserve_literal_renamed_layer_names() {
    let (mut s, _) = filters();
    insert_effect(&mut s, "brightness_contrast");
    let id = s.engine.document().working.occurrence.unwrap();
    assert_eq!(s.state.layer_properties.title, "Brightness / Contrast");
    let name = "私の曲線 한글 🎨 { $name } \u{2068}لوحة\u{2069}";
    s.dispatch(UiAction::Layer { action: LayerAction::Rename {
        id: occurrence_token(id), name: name.into(),
    } }).unwrap();
    assert!(s.filter_drawer_open());
    insert_effect(&mut s, "curves");
    let layer = s.engine.document().scene().occurrence(id).unwrap();
    assert_eq!(s.state.layer_properties.title, format!("{name} (Curves)"));
    assert_eq!(layer.name.as_ref(), name);
    assert_eq!(s.engine.document().scene().effect(id).unwrap().program.id.as_ref(), "curves");
    invoke(&mut s, CommandId::Undo);
    let layer = s.engine.document().scene().occurrence(id).unwrap();
    assert_eq!(s.state.layer_properties.title, format!("{name} (Brightness / Contrast)"));
    assert_eq!(layer.name.as_ref(), name);
    assert_eq!(s.engine.document().scene().effect(id).unwrap().program.id.as_ref(), "brightness_contrast");
    invoke(&mut s, CommandId::Redo);
    assert_eq!(s.engine.document().scene().occurrence(id).unwrap().name.as_ref(), name);
    assert_eq!(s.state.layer_properties.title, format!("{name} (Curves)"));
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
    s.layer_edit(effect_test_selection_edit(s.engine.document(),Some(selection.clone()))).unwrap();
    insert_effect(&mut s, "brightness_contrast");
    let id = s.engine.document().working.occurrence.unwrap();
    let mask=s.engine.document().scene().mask(id).map(|(use_,source)|(use_.clone(),source.clone())).expect("a new effect takes the selection");
    assert_eq!(mask.1.initial.as_ref(), Some(&selection));
    invoke(&mut s, CommandId::Reselect);
    insert_effect(&mut s, "curves");
    assert_eq!(s.engine.document().working.occurrence.unwrap(), id, "the drawer replaces the effect");
    assert_eq!(s.engine.document().scene().mask(id).map(|(use_,source)|(use_.clone(),source.clone())), Some(mask), "and keeps its mask");
    assert_eq!(s.engine.document().working.selection, Some(selection), "replacing leaves the selection");
}

#[test]
fn filters_resolve_drawing_without_borrowing_other_masks_or_entering_groups() {
    let (mut s, _) = filters();
    insert_effect(&mut s, "curves");
    let first = s.engine.document().working.occurrence.unwrap();
    let mut doc=s.engine.document().clone();let base=effect_test_paint(&doc);let paint=doc.scene().source_target(base).unwrap();
    assert_eq!(doc.drawing_target(),Some(paint));
    let (upper,edit)=effect_insertion(&doc,effects::effect_draft(&doc,first).unwrap(),"Upper filter");doc.apply(edit).unwrap();
    doc.apply(doc.select_occurrence_edit(upper).unwrap()).unwrap();
    doc.apply(effect_test_mask_edit(&doc,first,1.)).unwrap();
    assert_eq!(doc.drawing_target(),Some(paint),"ignore a lower filter's mask");
    for handle in [first,upper] {doc.apply(doc.attachment_edit(handle,true,false).unwrap()).unwrap();}
    assert_eq!(doc.drawing_target(),Some(paint),"attached filter uses its owner");
    doc.apply(effect_test_mask_edit(&doc,upper,1.)).unwrap();
    let mask=doc.scene().mask(upper).unwrap().0.source;
    assert_eq!(doc.drawing_target(),Some(layer_core::SourceTarget::Coverage(mask)),"own mask wins without a separate mask click");
    doc.apply(effect_test_occurrence_edit(&doc,upper,|o|o.locked=true)).unwrap();assert_eq!(doc.drawing_target(),None);
    doc.apply(effect_test_occurrence_edit(&doc,upper,|o|{o.locked=false;o.mask=None;o.attachment=layer_core::Attachment::None;})).unwrap();
    doc.apply(effect_test_occurrence_edit(&doc,base,|o|o.locked=true)).unwrap();
    assert_eq!(doc.drawing_target(),None,"do not skip a locked drawing layer");
    let group=layer_core::authored::RecordChange::insert(&doc.artwork.stacks,Default::default());let group_handle=group.handle;
    doc.apply(layer_core::Edit::Batch(vec![layer_core::Edit::Stack(group),effect_test_occurrence_edit(&doc,base,|o|{o.locked=false;o.content=layer_core::authored::OccurrenceContent::Stack(group_handle);})])).unwrap();
    doc.apply(effect_test_mask_edit(&doc,base,1.)).unwrap();
    assert_eq!(doc.drawing_target(),None,"a group and its mask are not drawing fallbacks");
    doc.apply(doc.attachment_edit(first,false,false).unwrap()).unwrap();
    let fill = doc.scene().effect_handle(occurrence_handle(2).unwrap()).unwrap();
    let fill = layer_core::authored::RecordChange::insert(&doc.artwork.effects, doc.artwork.effects.get(fill).unwrap().clone());
    let content = layer_core::authored::OccurrenceContent::Effect(fill.handle);
    let occurrence = effect_test_occurrence_edit(&doc,base,|o|{ o.content = content; o.mask = None; });
    doc.apply(layer_core::Edit::Batch(vec![layer_core::Edit::Effect(fill), occurrence])).unwrap();
    assert_eq!(doc.drawing_target(),None);
    assert!(s.state.layers.iter().any(|l| l.id == 1 && l.drawing && l.selection_icon == "layer-brush-symbolic"));
    assert!(s.state.layers.iter().any(|l| l.id == occurrence_token(first) && l.editing && !l.drawing));
}

#[test]
fn strokes_through_a_selected_filter_keep_selection_and_undo_on_the_drawing_target() {
    let (mut s, _) = filters();
    insert_effect(&mut s, "curves");
    let filter = s.engine.document().working.occurrence.unwrap();
    let base=effect_test_paint(s.engine.document());
    let stroke = |s: &mut UiSession<Recorder>| {
        for (sequence, phase) in [(1, PenPhase::Down), (2, PenPhase::Move), (3, PenPhase::Up)] {
            s.pen(event(s, sequence, phase, 1.)).unwrap();
        }
        s.frame(30_000_000, 38_000_000).unwrap();
    };
    stroke(&mut s);
    assert_eq!(s.engine.document().working.occurrence.unwrap(), filter);
    assert!(!s.engine.document().scene().paint_source(base).unwrap().raster.is_empty());
    assert!(s.engine.document().scene().paint_source(filter).is_none());
    invoke(&mut s, CommandId::Undo);
    assert!(s.engine.document().scene().paint_source(base).unwrap().raster.is_empty());
    s.dispatch(UiAction::Layer { action: LayerAction::AddMask { id: occurrence_token(filter), replace: false } }).unwrap();
    s.dispatch(UiAction::SelectLayer { id: occurrence_token(filter) }).unwrap();
    assert!(s.engine.document().working.inspect_mask.is_none());
    stroke(&mut s);
    assert_eq!(s.engine.document().working.occurrence.unwrap(), filter);
    assert!(!s.engine.document().scene().mask(filter).unwrap().1.raster.is_empty());
    assert!(s.engine.document().scene().paint_source(base).unwrap().raster.is_empty());
    invoke(&mut s, CommandId::Undo);
    assert!(s.engine.document().scene().mask(filter).unwrap().1.raster.is_empty());
    s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: occurrence_token(filter), value: true } }).unwrap();
    stroke(&mut s);
    assert!(s.engine.document().scene().mask(filter).unwrap().1.raster.is_empty());
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
    assert_eq!(paper_color(s.engine.document()), Some(color));
    let changed = s.state.layers.iter().find(|layer| layer.id == 2).unwrap().paint_revision;
    assert_ne!(changed, revision);
    assert!(changed < 1u64 << 53);
    let loaded=package_roundtrip(s.engine.document());
    assert_eq!(paper_color(&loaded), Some(color));
    invoke(&mut s, CommandId::Undo);
    assert_eq!(paper_color(s.engine.document()), Some(layer_core::color::RgbColor::WHITE));
    invoke(&mut s, CommandId::Redo);
    s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: 2, value: true } }).unwrap();
    assert!(s.dispatch(action).is_err());
    s.cursor_input(Some(event(&s, 1, PenPhase::Hover, 0.)));
    assert_eq!(s.canvas_cursor().unwrap().segments[0].marker, 6.);
    s.dispatch(UiAction::SelectLayer { id: 1 }).unwrap();
    assert!(s.canvas_cursor().unwrap().segments.iter().all(|p| p.marker != 6.));
}

#[test]
fn layer_thumbnails_and_type_icons_are_independent_on_every_host() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        let paint = s.state.layers.iter().find(|l| l.id == 1).unwrap();
        assert!(paint.has_thumbnail);
        assert!(paint.content_icon.is_none());
        let paper = s.state.layers.iter().find(|l| l.id == 2).unwrap();
        assert!(paper.has_thumbnail);
        assert_eq!(paper.content_icon.as_deref(), Some("layer-fill-symbolic"));
        insert_effect(&mut s, "gradient_fill");
        let fill = s.state.layer_tools.editing_layer.as_ref().unwrap();
        assert!(fill.has_thumbnail);
        assert_eq!(fill.content_icon.as_deref(), Some("layer-gradient-symbolic"));
        insert_effect(&mut s, "brightness_contrast");
        let adjustment = s.state.layer_tools.editing_layer.as_ref().unwrap();
        assert!(!adjustment.has_thumbnail);
        assert!(adjustment.content_icon.is_some());
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
        assert!(!s.state.layer_tools.editing_layer.as_ref().unwrap().has_thumbnail);
        invoke(&mut s, CommandId::SelectAll);
        invoke(&mut s, CommandId::SaveSelectionLayer);
        let selection = s.state.layers.iter().find(|l| l.selection_layer).unwrap();
        assert!(selection.has_thumbnail);
        assert_eq!(selection.content_icon.as_deref(), Some("layer-selection-brush-symbolic"));
    }
}

#[test]
fn every_generator_has_a_thumbnail_revision_for_its_parameters_and_history() {
    let mut s = session(Platform::Gtk);
    insert_effect(&mut s, "gradient_fill");
    let id = s.engine.document().working.occurrence.unwrap();
    let row = |s: &UiSession<Recorder>| s.state.layers.iter().find(|layer| layer.id == occurrence_token(id)).unwrap().clone();
    assert!(row(&s).has_thumbnail);
    assert_eq!(row(&s).content_icon.as_deref(), Some("layer-gradient-symbolic"));
    let original = row(&s).paint_revision;
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer: occurrence_token(id), key: "angle".into(), value: layer_core::EffectValue::Number(30.) } }).unwrap();
    let changed = row(&s).paint_revision;
    assert_ne!(original, changed);
    invoke(&mut s, CommandId::Undo);
    assert_ne!(row(&s).paint_revision, changed);
    let undone = row(&s).paint_revision;
    invoke(&mut s, CommandId::Redo);
    assert_ne!(row(&s).paint_revision, undone);
    let redone = row(&s).paint_revision;
    let mut future = effects::effect_draft(s.engine.document(), id).unwrap();
    Arc::make_mut(&mut future.program).id = "future_fill".into();
    s.layer_edit(effects::effect_edit(s.engine.document(), id, future).unwrap()).unwrap();
    s.refresh_document();
    assert!(row(&s).has_thumbnail);
    assert_eq!(row(&s).content_icon.as_deref(), Some("layer-adjustments-symbolic"));
    assert_ne!(row(&s).paint_revision, redone);
    let mut document = s.engine.document().clone();
    let mut revisions = art_layers::PreviewRevisions::default();
    revisions.update(&document);
    let initial = revisions.id(id);
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().size[0] *= 2;
    revisions.update(&document);
    assert_ne!(revisions.id(id), initial);
    let resized = revisions.id(id);
    let composition = document.artwork.compositions.get_mut(document.artwork.root).unwrap();
    composition.blend = if composition.blend == layer_core::BlendSpace::Linear {
        layer_core::BlendSpace::Perceptual
    } else { layer_core::BlendSpace::Linear };
    revisions.update(&document);
    assert_ne!(revisions.id(id), resized);
}

#[test]
fn empty_layer_stack_roundtrips_and_accepts_a_new_layer_with_undo() {
    let mut s = session(Platform::Gtk);
    for id in [1, 2] {
        s.dispatch(UiAction::Layer { action: LayerAction::Delete { id } }).unwrap();
    }
    assert!(s.state.layers.is_empty());
    let project = s.capture_artwork().unwrap();
    let loaded=reopen_capture(&project);
    assert!(loaded.scene().order().is_empty());
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
    assert_eq!(clock.advance(effect.view(), 10.), 10.);
    effect.set("speed", layer_core::EffectValue::Number(2.)).unwrap();
    assert_eq!(clock.advance(effect.view(), 10.), 10.);
    assert_eq!(clock.advance(effect.view(), 11.), 12.);
    effect.set("speed", layer_core::EffectValue::Number(0.)).unwrap();
    assert_eq!(clock.advance(effect.view(), 11.), 12.);
    assert_eq!(clock.advance(effect.view(), 21.), 12.);
    effect.set("speed", layer_core::EffectValue::Number(1.)).unwrap();
    assert_eq!(clock.advance(effect.view(), 21.), 12.);
    assert_eq!(clock.advance(effect.view(), 22.), 13.);
}

#[test]
fn curve_points_detach_off_the_graph_until_release_and_commit_one_step() {
    let mut s = session(Platform::Gtk);
    insert_effect(&mut s, "curves");
    let layer = s.state.layer_properties.layer.unwrap();
    let curve = |s: &UiSession<Recorder>| match &s.state.layer_properties.controls.iter().find(|c| c.key == "rgb").unwrap().value {
        layer_core::EffectValue::Curve(points) => points.clone(),
        _ => unreachable!(),
    };
    let point = |index, point| EffectAction::CurvePoint { layer, key: "rgb".into(), index, point, remove: false };
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

    let modified = |s: &UiSession<Recorder>| s.state.layer_properties.controls.iter().find(|c| c.key == "rgb").unwrap().modified;
    assert!(modified(&s));
    s.dispatch(UiAction::Effect { action: EffectAction::Reset { layer, key: "rgb".into() } }).unwrap();
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
    let base = s.engine.document().working.occurrence.unwrap();
    let menu = s.application_menu(ApplicationMenu::Layer);
    let new = menu_item(&menu.sections, "New").unwrap();
    let solid = menu_item(&new.sections, "Solid Color Fill").unwrap().clone();
    assert!(solid.enabled);
    assert!(menu_item(&new.sections, "Gradient Fill").is_some());
    let before = s.engine.document().clone();
    s.dispatch(solid.action.unwrap()).unwrap();
    let doc = s.engine.document();
    let handle=doc.working.occurrence.unwrap();
    let fill=doc.scene().occurrence(handle).unwrap();
    assert_eq!((fill.kind(), fill.name.as_ref()), (LayerKind::Effect, "Solid Color"));
    let effect=doc.scene().effect(handle).unwrap();
    assert_eq!(effect.program.kind, layer_core::EffectKind::Generator);
    assert_eq!(effect.value("color"), Some(&layer_core::EffectValue::Color(s.state.colors.definition())));
    assert!(doc.scene().mask(handle).is_none());
    let row = |s: &UiSession<Recorder>, handle| s.state.layers.iter().find(|row| row.id == occurrence_token(handle)).unwrap().fill_color.clone();
    assert_eq!(row(&s, handle), Some(crate::LayerFillColor { key: "color".into(), color: s.state.colors.definition(), opaque: false }), "the fill thumbnail edits its color");
    assert_eq!(row(&s, base), None);
    assert_eq!(doc.scene().order().iter().position(|h|*h==handle).unwrap()+1,doc.scene().order().iter().position(|h|*h==base).unwrap());
    assert_eq!(doc.drawing_target(), None);
    invoke(&mut s, CommandId::Undo);
    assert_live_artwork_eq(s.engine.document(),&before);

    let selection = layer_core::Selection::polygon(vec![
        Point { x: 40., y: 40. },
        Point { x: 240., y: 60. },
        Point { x: 120., y: 200. },
    ])
    .unwrap();
    s.layer_edit(effect_test_selection_edit(s.engine.document(),Some(selection.clone()))).unwrap();
    s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "gradient_fill".into() } }).unwrap();
    let doc = s.engine.document();
    let handle=doc.working.occurrence.unwrap();
    assert_eq!(doc.scene().effect(handle).unwrap().program.id.as_ref(), "gradient_fill");
    assert_eq!(row(&s, handle), None, "gradient fills open their gradient in Properties instead");
    assert_eq!(doc.scene().mask(handle).unwrap().1.initial.as_ref(), Some(&selection), "the selection becomes the mask");
    assert_eq!(doc.working.selection, None, "and is consumed");
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().working.selection, Some(selection), "undo restores the selection");
}

#[test]
fn the_filter_menu_inserts_fill_generators_without_default_masks_on_every_host() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        let menu = s.application_menu(ApplicationMenu::Filter);
        let fill = menu_item(&menu.sections, "Fill").unwrap();
        assert!(menu_item(&menu.sections, "Curves").is_some());
        for label in ["Solid Color", "Gradient Fill"] {
            let item = menu_item(&fill.sections, label).unwrap();
            assert!(item.enabled);
            let before = s.engine.document().clone();
            s.dispatch(item.action.clone().unwrap()).unwrap();
            let doc = s.engine.document();
            let handle = doc.working.occurrence.unwrap();
            assert_eq!(doc.scene().effect(handle).unwrap().program.kind, layer_core::EffectKind::Generator);
            assert!(doc.scene().mask(handle).is_none());
            assert!(package_roundtrip(doc).scene().mask(handle).is_none());
            let created = doc.clone();
            invoke(&mut s, CommandId::Undo);
            assert_live_artwork_eq(s.engine.document(), &before);
            invoke(&mut s, CommandId::Redo);
            assert_live_artwork_eq(s.engine.document(), &created);
            invoke(&mut s, CommandId::Undo);
        }
    }
}

#[test]
fn all_layer_filter_entry_points_attach_to_the_captured_layer() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        let base = s.engine.document().working.occurrence.unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } }).unwrap();
        let top = s.engine.document().working.occurrence.unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::Context { id: occurrence_token(base), mask: false } }).unwrap();
        let filter = s.localization().text(MessageId::RESOURCES_LAYER_ADD_FILTER).to_string();
        let row = s.layer_menu(occurrence_token(base), false).unwrap();
        let submenu = menu_item(&row.sections, &filter).unwrap();
        assert_eq!(submenu.icon, Some("add-filter"));
        assert_eq!(serde_json::to_value(&submenu.sections).unwrap(), serde_json::to_value(&s.state.layer_tools.add_filter.as_ref().unwrap().sections).unwrap());
        assert_eq!(s.state.layer_properties.add_filter, s.state.layer_tools.add_filter);
        assert!(menu_item(&s.application_menu(ApplicationMenu::Layer).sections, &filter).is_none(), "the menu bar keeps a single Filter menu");
        let other = s.layer_menu(occurrence_token(top), false).unwrap();
        let Some(UiAction::Effect { action: EffectAction::InsertAttached { owner, .. } }) = &menu_item(&other.sections, "Curves").unwrap().action else { panic!("local filter") };
        assert_eq!(*owner, occurrence_token(top));
        s.dispatch(UiAction::SelectLayer { id: occurrence_token(top) }).unwrap();
        s.dispatch(menu_item(&submenu.sections, "Curves").unwrap().action.clone().unwrap()).unwrap();
        let doc = s.engine.document();
        let created = doc.working.occurrence.unwrap();
        assert_eq!(doc.scene().effect_owner(created), Some(base));
        assert_eq!(doc.scene().effect(created).unwrap().program.id.as_ref(), "curves");
        let position = |handle| doc.scene().order().iter().position(|h| *h == handle).unwrap();
        assert!(position(base).min(position(top)) < position(created) && position(created) < position(base).max(position(top)), "the filter lands on that layer, below the newer one");
    }
}

#[test]
fn replacing_fill_generators_preserves_the_presence_or_absence_of_a_mask() {
    let (mut s, _) = filters();
    insert_effect(&mut s, "solid_color");
    let id = s.engine.document().working.occurrence.unwrap();
    assert!(s.engine.document().scene().mask(id).is_none());
    insert_effect(&mut s, "gradient_fill");
    assert_eq!(s.engine.document().working.occurrence, Some(id));
    assert!(s.engine.document().scene().mask(id).is_none());
    s.dispatch(UiAction::Layer { action: LayerAction::AddMask { id: occurrence_token(id), replace: false } }).unwrap();
    let mask = s.engine.document().scene().mask(id).map(|(use_, source)| (use_.clone(), source.clone())).unwrap();
    insert_effect(&mut s, "solid_color");
    assert_eq!(s.engine.document().working.occurrence, Some(id));
    assert_eq!(s.engine.document().scene().mask(id).map(|(use_, source)| (use_.clone(), source.clone())), Some(mask));
}

#[test]
fn new_adjustments_from_an_existing_owner_stay_above_its_relationships() {
    let (mut s,_) = filters();let base=s.engine.document().working.occurrence.unwrap();
    insert_effect(&mut s,"motion_blur");let blur=s.engine.document().working.occurrence.unwrap();
    layer(&mut s,LayerAction::Clip{id:occurrence_token(blur),value:true});
    for clipped in [false,true] {
        layer(&mut s,LayerAction::Select{id:occurrence_token(base),mask:false});
        if clipped {layer(&mut s,LayerAction::New{group:false,clipped:true});}
        let top=s.engine.document().scene().children(None)[0];
        layer(&mut s,LayerAction::Select{id:occurrence_token(base),mask:false});let before=s.engine.document().clone();
        s.frame(0,0).unwrap();assert!(s.request_filter_previews(1,vec!["curves".into()],[120,40]).unwrap());
        assert_eq!(s.engine.backend().filter_preview.as_ref().unwrap().source,layer_render::FilterPreviewSource::LayerStack(top));s.renderer_mut().cancel_filter_previews();
        insert_effect(&mut s,"curves");let doc=s.engine.document();let added=doc.working.occurrence.unwrap();
        assert_eq!(&doc.scene().children(None)[..2],&[added,top]);assert_eq!(doc.scene().occurrence(added).unwrap().attachment,layer_core::Attachment::None);
        assert_eq!(doc.scene().effect_owner(blur),Some(base));
        invoke(&mut s,CommandId::Undo);assert_live_artwork_eq(s.engine.document(),&before);
    }
}

#[test]
fn fill_insertion_preserves_existing_effect_owners_and_common_base() {
    let (mut s, _) = filters();let base=s.engine.document().working.occurrence.unwrap();
    layer(&mut s,LayerAction::New{group:false,clipped:true});let owner=s.engine.document().working.occurrence.unwrap();
    insert_effect(&mut s,"curves");let curves=s.engine.document().working.occurrence.unwrap();
    layer(&mut s,LayerAction::Select{id:occurrence_token(owner),mask:false});insert_effect(&mut s,"gaussian_blur");let blur=s.engine.document().working.occurrence.unwrap();
    layer(&mut s,LayerAction::Select{id:occurrence_token(owner),mask:false});layer(&mut s,LayerAction::New{group:false,clipped:true});let top=s.engine.document().working.occurrence.unwrap();
    for selected in [blur,owner,base] {
        layer(&mut s,LayerAction::Select{id:occurrence_token(selected),mask:false});assert!(s.filter_drawer_open());let before=s.engine.document().clone();
        insert_effect(&mut s,"gradient_fill");let doc=s.engine.document();let fill=doc.working.occurrence.unwrap();
        assert_eq!(doc.scene().effect(fill).unwrap().program.kind,layer_core::EffectKind::Generator);
        for effect in [blur,curves]{assert_eq!(doc.scene().effect_owner(effect),Some(owner));}
        for clip in [fill,owner,top]{assert_eq!(doc.scene().clipping_base(clip),Some(base));}
        if selected!=base{assert!(doc.scene().position(fill)<doc.scene().position(curves));}
        invoke(&mut s,CommandId::Undo);assert_live_artwork_eq(s.engine.document(),&before);
    }
    layer(&mut s,LayerAction::Select{id:occurrence_token(owner),mask:false});let before=s.engine.document().clone();
    s.frame(0,0).unwrap();assert!(s.request_filter_previews(1,vec!["exposure".into()],[120,40]).unwrap());
    assert_eq!(s.engine.backend().filter_preview.as_ref().unwrap().source,layer_render::FilterPreviewSource::OwnerContent(owner));
    insert_effect(&mut s,"exposure");let added=s.engine.document().working.occurrence.unwrap();
    assert_eq!(s.engine.document().scene().attached_effects(owner),[added,blur,curves]);
    invoke(&mut s,CommandId::Undo);assert_live_artwork_eq(s.engine.document(),&before);
    insert_effect(&mut s,"gradient_fill");let fill=s.engine.document().working.occurrence.unwrap();
    assert_eq!(s.engine.document().scene().clipping_base(fill),Some(base));
    let before=s.engine.document().clone();insert_effect(&mut s,"curves");let added=s.engine.document().working.occurrence.unwrap();
    assert_eq!(s.engine.document().scene().children(None)[0],added);assert_eq!(s.engine.document().scene().effect_owner(added),None);
    assert_eq!(s.engine.document().scene().clipping_base(fill),Some(base));
    invoke(&mut s,CommandId::Undo);assert_live_artwork_eq(s.engine.document(),&before);
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
        let layer = occurrence_token(s.engine.document().working.occurrence.unwrap());
        let colors: Vec<_> = s.state.layer_properties.controls.iter()
            .filter(|c| matches!(c.kind,PropertyKind::Color {..})).cloned().collect();
        assert!(!colors.is_empty(), "{id}");
        for control in colors {
            let action = UiAction::Effect { action: EffectAction::UseCurrentColor { layer, key: control.key.clone() } };
            assert_eq!(control.color_action.as_ref(), Some(&action), "{id}.{}", control.key);
            assert!(!control.label.is_empty());
            s.dispatch(action).unwrap();
            let value = s.engine.document().scene().effect(occurrence_handle(layer).unwrap()).unwrap().value(&control.key).cloned();
            assert_eq!(value, Some(layer_core::EffectValue::Color(s.state.colors.definition())), "{id}.{}", control.key);
        }
    }
}

#[test]
fn layer_add_filter_captures_owner_appends_to_local_chain_and_undoes_one_step() {
    let (mut s, _) = filters();
    let owner = s.engine.document().working.occurrence.unwrap();
    let action = |effect: &str, s: &UiSession<Recorder>| UiAction::Effect { action: EffectAction::InsertAttached {
        effect: effect.into(), owner: occurrence_token(owner), epoch: s.state.document_file.epoch } };
    let captured = action("exposure", &s);
    s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
    let before = s.engine.document().clone();
    s.dispatch(captured).unwrap();
    let first = s.engine.document().working.occurrence.unwrap();
    assert_eq!(s.engine.document().scene().effect_owner(first), Some(owner));
    assert_eq!(s.engine.document().scene().attached_effects(owner), &[first]);
    assert!(s.state.layer_tools.add_filter.is_some());
    s.dispatch(action("curves", &s)).unwrap();
    let second = s.engine.document().working.occurrence.unwrap();
    assert_ne!(first, second);
    assert_eq!(s.engine.document().scene().attached_effects(owner), &[first, second]);
    assert_eq!(s.engine.document().scene().order()[..3], [second, first, owner]);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().scene().attached_effects(owner), &[first]);
    invoke(&mut s, CommandId::Undo);
    assert_live_artwork_eq(s.engine.document(), &before);
    let stale = action("exposure", &s);
    s.state.document_file.epoch += 1;
    s.dispatch(stale).unwrap();
    assert_live_artwork_eq(s.engine.document(), &before);
}

#[test]
fn layer_add_filter_menu_excludes_generators_and_refuses_unsupported_or_locked_owners() {
    let mut s = session(Platform::Gtk);
    let menu = s.state.layer_tools.add_filter.as_ref().unwrap();
    for item in menu.sections.iter().flatten().flat_map(|category| category.sections.iter().flatten()) {
        let Some(UiAction::Effect { action: EffectAction::InsertAttached { effect, .. } }) = &item.action else { panic!("attached action") };
        assert_eq!(s.effect_catalog.get(effect).unwrap().program.kind, layer_core::EffectKind::Adjustment);
    }
    s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: 1, value: true } }).unwrap();
    assert!(s.state.layer_tools.add_filter.is_none());
    s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
    assert!(s.state.layer_tools.add_filter.is_none());
    s.dispatch(UiAction::Effect { action: EffectAction::InsertAttached { effect: "fill".into(), owner: 1, epoch: s.state.document_file.epoch } }).unwrap_err();
}

#[test]
fn layer_color_mode_is_a_paint_property_and_undoable() {
    let mut s = session(Platform::Gtk);
    let before = s.engine.document().artwork.clone();
    let control = s.state.layer_properties.controls.iter().find(|c| c.key == "color_mode").unwrap();
    assert_eq!(control.label, "Color mode");
    assert_eq!(control.value, layer_core::EffectValue::Choice(0));
    assert!(!serde_json::to_value(&s.state.layer_tools).unwrap().as_object().unwrap().contains_key("color_mode"));
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer: 1, key: "color_mode".into(), value: layer_core::EffectValue::Choice(1) } }).unwrap(); s.frame(1, 1).unwrap();
    assert_eq!(s.state.layer_properties.controls.iter().find(|c| c.key == "color_mode").unwrap().value, layer_core::EffectValue::Choice(1));
    assert!(s.state.layers.iter().find(|l| l.id == 1).unwrap().description.contains("Grayscale"));
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.document().artwork, before);
    s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
    assert!(s.state.layer_properties.controls.iter().all(|c| c.key != "color_mode"));
}

#[test]
fn a_captured_color_mode_action_cannot_change_a_locked_empty_layer() {
    let mut s = session(Platform::Gtk);
    let action = UiAction::Effect { action: EffectAction::Set { layer: 1, key: "color_mode".into(), value: layer_core::EffectValue::Choice(1) } };
    s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: 1, value: true } }).unwrap();
    let before = s.engine.document().artwork.clone();
    assert!(!s.state.layer_properties.enabled);
    assert!(s.dispatch(action).is_err());
    assert_eq!(s.engine.document().artwork, before);
    invoke(&mut s, CommandId::Undo);
    assert!(s.state.layer_properties.enabled);
}

#[test]
fn drawing_activation_rebuilds_layer_menu_actions_with_the_current_epoch() {
    let previous = session(Platform::Gtk);
    let mut next = session(Platform::Gtk);
    let stale = next.state.layer_tools.add_filter.as_ref().unwrap().sections[0][0].sections[0][0].action.clone().unwrap();
    next.inherit_window_state(&previous).unwrap();
    let before = next.engine.document().artwork.clone();
    next.dispatch(stale).unwrap();
    assert_eq!(next.engine.document().artwork, before);
    assert_eq!(next.state.layer_properties.add_filter, next.state.layer_tools.add_filter);
    let menu = next.state.layer_properties.add_filter.as_ref().unwrap();
    let action = menu.sections.iter().flatten().flat_map(|c| c.sections.iter().flatten()).find(|i| i.label == "Exposure").unwrap().action.clone().unwrap();
    let UiAction::Effect { action: EffectAction::InsertAttached { epoch, .. } } = action.clone() else { panic!("filter action") };
    assert_eq!(epoch, next.state.document_file.epoch);
    let count = next.state.layers.len(); next.dispatch(action).unwrap();
    assert_eq!(next.state.layers.len(), count + 1);
}
