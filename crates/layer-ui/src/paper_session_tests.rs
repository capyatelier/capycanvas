fn first_imported() -> u32 {
    layer_core::paper::built_in_papers().len() as u32
}

fn paper_layer(s: &UiSession<Recorder>) -> u64 {
    occurrence_token(layer_core::paper::paper_occurrence(s.engine.document().scene(), None).unwrap())
}

fn paper_control(s: &UiSession<Recorder>, key: &str) -> PropertyControl {
    s.state.layer_properties.controls.iter().find(|control| control.key == key).cloned().unwrap()
}

fn paper_scale(s: &UiSession<Recorder>) -> f32 {
    let layer_core::EffectValue::Number(scale) = paper_control(s, "scale").value else { panic!("Scale is a number") };
    scale
}

#[test]
fn choosing_and_scaling_paper_are_single_undo_steps_and_new_paper_starts_on_the_choice() {
    let mut s = session(Platform::Gtk);
    let layer = paper_layer(&s);
    s.dispatch(UiAction::SelectLayer { id: layer }).unwrap();
    assert_eq!(s.state.layer_properties.title, "Paper");
    let checkpoint = s.engine.checkpoint();
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "texture".into(), value: layer_core::EffectValue::Choice(2) } }).unwrap();
    assert!(s.state.requests.iter().any(|r| matches!(r.kind, HostRequestKind::SaveSettings { .. })), "the choice is remembered for new drawings");
    assert_eq!(s.state.settings.new_document.paper.as_deref(), Some("rough"));
    assert_eq!(s.engine.document().paper(None).texture, layer_core::paper::built_in_paper("rough").unwrap().texture);
    assert_eq!(paper_control(&s, "texture").value, layer_core::EffectValue::Choice(2));
    let gesture = |phase, value| UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(EffectAction::Number {
        layer, key: "scale".into(), operation: NumericOperation::Value { value } }) } };
    let chosen = s.engine.checkpoint();
    assert_ne!(chosen, checkpoint);
    s.dispatch(gesture(ContactPhase::Down, 1.5)).unwrap();
    s.dispatch(gesture(ContactPhase::Move, 3.)).unwrap();
    assert!((paper_scale(&s) - 3.).abs() < 1e-4);
    s.dispatch(gesture(ContactPhase::Up, 2.)).unwrap();
    assert!((paper_scale(&s) - 2.).abs() < 1e-4);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.checkpoint(), chosen, "the drag is one step");
    assert!((paper_scale(&s) - 1.).abs() < 1e-4);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.checkpoint(), checkpoint, "the choice is one step");
    assert_eq!(s.engine.document().paper(None).texture, layer_core::paper::default_texture());
    assert_eq!(s.state.settings.new_document.paper.as_deref(), Some("rough"), "undo keeps the remembered paper");
    let menu = s.application_menu(ApplicationMenu::Layer);
    let new = menu.sections.iter().flatten().find(|item| item.label == "New").unwrap();
    let paper = new.sections.iter().flatten().find(|item| item.label == "Paper").expect("Layer › New › Paper").clone();
    s.dispatch(UiAction::Layer { action: LayerAction::Delete { id: layer } }).unwrap();
    s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
    s.dispatch(paper.action.unwrap()).unwrap();
    let inserted = s.engine.document().working.occurrence.unwrap();
    assert_eq!(s.engine.document().scene().children(None).last(), Some(&inserted));
    assert_eq!(s.engine.document().scene().parent(inserted), None);
    let application = s.engine.document().scene().effect_application(inserted).unwrap();
    assert!(layer_core::paper::is_paper(&application.program));
    assert_eq!(s.engine.document().scene().effect(inserted).unwrap().constant_color(), Some(layer_core::color::RgbColor::WHITE));
    assert_eq!(s.engine.document().paper(s.engine.document().working.target).texture, layer_core::paper::built_in_paper("rough").unwrap().texture);
    assert!((paper_scale(&s) - 1.).abs() < 1e-4);
    assert_eq!(s.state.layer_properties.title, "Paper");
}

#[test]
fn adding_paper_again_selects_the_existing_sheet_without_an_edit() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        let paper = paper_layer(&s);
        let before = s.engine.checkpoint();
        s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "paper".into() } }).unwrap();
        assert_eq!(s.state.layers.len(), 2);
        assert_eq!(s.engine.document().working.occurrence, Some(occurrence_handle(paper).unwrap()));
        assert_eq!(s.engine.checkpoint(), before);
        assert_eq!(s.state.layers.last().unwrap().id, paper);
    }
}

#[test]
fn add_paper_row_tracks_the_missing_sheet_through_undo_and_redo() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        assert!(s.state.layer_tools.add_paper.is_none());
        assert!(!s.command(CommandId::LowerLayer).enabled);
        let paper = paper_layer(&s);
        s.dispatch(UiAction::Layer { action: LayerAction::Delete { id: paper } }).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
        let row = s.state.layer_tools.add_paper.clone().unwrap();
        assert_eq!(row.label, "Add paper");
        let before = s.engine.checkpoint();
        s.dispatch(UiAction::Effect { action: row.action }).unwrap();
        let paper = s.engine.document().bottom_paper().unwrap();
        assert_eq!(s.engine.document().working.occurrence, Some(paper));
        assert!(s.state.layer_tools.add_paper.is_none());
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.checkpoint(), before);
        assert!(s.state.layer_tools.add_paper.is_some());
        assert!(s.engine.document().bottom_paper().is_none());
        invoke(&mut s, CommandId::Redo);
        assert_eq!(s.engine.document().bottom_paper(), Some(paper));
        assert!(s.state.layer_tools.add_paper.is_none());
    }
}

#[test]
fn selected_fills_keep_properties_and_mark_only_an_ungrouped_layer_drawable() {
    for platform in Platform::ALL {
        for (program, title) in [("paper", "Paper"), ("solid_color", "Solid Color"), ("gradient_fill", "Gradient Fill")] {
            let mut s = session(platform);
            s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: program.into() } }).unwrap();
            let fill = occurrence_token(s.engine.document().working.occurrence.unwrap());
            s.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } }).unwrap();
            let ink = occurrence_token(s.engine.document().working.occurrence.unwrap());
            s.dispatch(UiAction::SelectLayer { id: fill }).unwrap();
            assert_eq!(s.state.layer_properties.title, title);
            assert_eq!(s.state.layers.iter().filter(|row| row.drawing).map(|row| row.id).collect::<Vec<_>>(), [ink]);
            s.dispatch(UiAction::SelectLayer { id: ink }).unwrap();
            s.dispatch(UiAction::Layer { action: LayerAction::GroupSelected }).unwrap();
            s.dispatch(UiAction::SelectLayer { id: fill }).unwrap();
            assert!(s.state.layers.iter().all(|row| !row.drawing));
            assert_eq!(s.engine.document().drawing_target(), None);
            invoke(&mut s, CommandId::Undo);
            assert_eq!(s.state.layers.iter().filter(|row| row.drawing).map(|row| row.id).collect::<Vec<_>>(), [ink]);
            if program == "paper" { continue; }
            s.dispatch(UiAction::Layer { action: LayerAction::AddMask { id: fill, replace: false } }).unwrap();
            s.dispatch(UiAction::SelectLayer { id: fill }).unwrap();
            assert!(s.state.layers.iter().all(|row| row.id == fill || !row.drawing));
            assert!(s.state.layers.iter().find(|row| row.id == fill).unwrap().mask_selected);
        }
    }
}

#[test]
fn paper_disables_every_way_to_add_a_mask() {
    let mut s = session(Platform::Gtk);
    let paper = paper_layer(&s);
    let ink = occurrence_token(s.engine.document().working.occurrence.unwrap());
    s.dispatch(UiAction::Layer { action: LayerAction::AddMask { id: ink, replace: false } }).unwrap();
    s.dispatch(UiAction::Layer { action: LayerAction::CopyMask { id: ink } }).unwrap();
    s.dispatch(UiAction::SelectLayer { id: paper }).unwrap();
    invoke(&mut s, CommandId::SelectAll);
    let tool = s.state.layer_tools.tool;
    assert!(!s.state.layer_tools.controls.mask, "the Layers mask button is off");
    fn adding_mask(sections: Vec<Vec<ContextMenuItem>>, into: &mut Vec<(String, bool)>) {
        for item in sections.into_iter().flatten() {
            if matches!(item.action, Some(UiAction::Layer { action: LayerAction::AddMask { .. } | LayerAction::MaskSelection { .. } | LayerAction::PasteMask { .. } })) {
                into.push((item.label.clone(), item.enabled));
            }
            adding_mask(item.sections, into);
        }
    }
    let mut mask_items = Vec::new();
    adding_mask(s.layer_menu(paper, false).unwrap().sections, &mut mask_items);
    assert_eq!(mask_items.len(), 4, "{mask_items:?}");
    assert!(mask_items.iter().all(|(_, enabled)| !enabled), "{mask_items:?}");
    let command = s.command(CommandId::MaskSelection);
    assert!(!command.enabled);
    assert_eq!(command.disabled_reason.as_deref(), Some("Paper can't have a mask"));
    for action in [LayerAction::AddMask { id: paper, replace: false }, LayerAction::MaskSelection { id: paper, hide: false }, LayerAction::PasteMask { id: paper }] {
        assert_eq!(s.dispatch(UiAction::Layer { action }).err().as_deref(), Some("Paper can't have a mask"));
    }
    assert_eq!(s.state.layer_tools.tool, tool);
    assert!(s.state.layers.iter().find(|row| row.id == paper).unwrap().mask_id.is_none());
    s.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } }).unwrap();
    assert!(s.state.layer_tools.controls.mask && s.command(CommandId::MaskSelection).enabled, "other layers keep their masks");
    s.dispatch(UiAction::Layer { action: LayerAction::Delete { id: paper } }).unwrap();
    assert!(s.engine.document().working.selection.is_some());
    s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "paper".into() } }).unwrap();
    let inserted = s.engine.document().working.occurrence.unwrap();
    assert!(layer_core::paper::is_paper_layer(s.engine.document().scene(), inserted));
    assert!(s.engine.document().scene().occurrence(inserted).unwrap().mask.is_none(), "a selection does not mask a new Paper");
}

#[test]
fn image_commands_carry_the_paper_and_scale_reports_it() {
    let mut s = session(Platform::Gtk);
    let layer = paper_layer(&s);
    s.dispatch(UiAction::SelectLayer { id: layer }).unwrap();
    invoke(&mut s, CommandId::ImageSize);
    assert_eq!(image_size_view(&s).resolution, 300.);
    image_size(&mut s, ImageSizeAction::Width { value: 2000. });
    image_size(&mut s, ImageSizeAction::Apply);
    assert!((paper_scale(&s) - 2.).abs() < 1e-4, "the paper grows with the marks");
    invoke(&mut s, CommandId::ImageSize);
    image_size(&mut s, ImageSizeAction::Resample { resample: ImageResample::Automatic });
    image_size(&mut s, ImageSizeAction::Resolution { value: 600. });
    image_size(&mut s, ImageSizeAction::Width { value: 2000. });
    image_size(&mut s, ImageSizeAction::Apply);
    assert!((paper_scale(&s) - 1.).abs() < 1e-4, "the same pixels against a finer print");
    s.dispatch(UiAction::Effect { action: EffectAction::Reset { layer, key: "scale".into() } }).unwrap();
    assert!((paper_scale(&s) - 1.).abs() < 1e-4);
    let mapping = s.engine.document().paper(None).mapping;
    assert!((mapping.0[0] - 600. / 25.4).abs() < 1e-9, "100% is real size at the drawing's resolution");
}

#[test]
fn paper_relief_shows_while_scaling_and_briefly_after_a_choice() {
    let mut s = session(Platform::Gtk);
    let layer = paper_layer(&s);
    s.dispatch(UiAction::SelectLayer { id: layer }).unwrap();
    let opacity = |s: &mut UiSession<Recorder>| s.renderer_mut().paper_overlay.as_ref().map(|overlay| overlay.opacity);
    s.frame(1_000_000_000, 1_000_000_000).unwrap();
    assert_eq!(opacity(&mut s), None);
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "texture".into(), value: layer_core::EffectValue::Choice(2) } }).unwrap();
    assert!(s.wants_continuous_frames());
    s.frame(2_000_000_000, 2_000_000_000).unwrap();
    assert_eq!(opacity(&mut s), Some(1.));
    assert_eq!(s.renderer_mut().paper_overlay.as_ref().unwrap().paper.surface.texture, layer_core::paper::built_in_paper("rough").unwrap().texture);
    s.frame(3_050_000_000, 3_050_000_000).unwrap();
    assert!(opacity(&mut s).is_some_and(|o| (o - 0.5).abs() < 1e-3), "fades out");
    s.frame(3_300_000_000, 3_300_000_000).unwrap();
    assert_eq!(opacity(&mut s), None);
    assert!(!s.wants_continuous_frames());
    let gesture = |phase, value| UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(EffectAction::Number {
        layer, key: "scale".into(), operation: NumericOperation::Value { value } }) } };
    s.dispatch(gesture(ContactPhase::Down, 1.5)).unwrap();
    for (step, now) in [4_000_000_000u64, 6_000_000_000, 9_000_000_000].into_iter().enumerate() {
        s.dispatch(gesture(ContactPhase::Move, 1.5 + step as f64 * 0.1)).unwrap();
        s.frame(now, now).unwrap();
        assert_eq!(opacity(&mut s), Some(1.), "shown for the whole drag");
        assert!(s.renderer_mut().paper_overlay.as_ref().unwrap().moving);
        let mapping = s.renderer_mut().paper_overlay.as_ref().unwrap().paper.surface.mapping;
        assert!((mapping.0[0] - (1.5 + step as f64 * 0.1) * 300. / 25.4).abs() < 1e-3, "shows the scale being set");
    }
    s.dispatch(gesture(ContactPhase::Up, 2.)).unwrap();
    s.frame(9_400_000_000, 9_400_000_000).unwrap();
    assert!(opacity(&mut s).is_some());
    assert!(!s.renderer_mut().paper_overlay.as_ref().unwrap().moving, "release refines the relief");
    s.frame(9_700_000_000, 9_700_000_000).unwrap();
    assert_eq!(opacity(&mut s), None, "hidden shortly after release");
}

#[test]
fn lighting_the_paper_is_one_step_per_drag_turns_on_a_dial_recomposites_only_to_hide_texture_and_replaces_the_scale_flash() {
    let mut s = session(Platform::Gtk);
    let layer = paper_layer(&s);
    s.dispatch(UiAction::SelectLayer { id: layer }).unwrap();
    let number = |key: &str, operation| UiAction::Effect { action: EffectAction::Number { layer, key: key.into(), operation } };
    let gesture = |phase, key: &str, operation| UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(EffectAction::Number {
        layer, key: key.into(), operation }) } };
    let before = s.engine.checkpoint();
    assert!(s.dispatch(number("light", NumericOperation::Value { value: 90. })).is_err(), "a flat paper has no light to turn");
    assert_eq!(s.engine.checkpoint(), before);
    s.frame(500_000_000, 500_000_000).unwrap();
    let composites = s.renderer_mut().composites;
    s.dispatch(gesture(ContactPhase::Down, "relief", NumericOperation::Value { value: 50. })).unwrap();
    s.frame(600_000_000, 600_000_000).unwrap();
    s.dispatch(gesture(ContactPhase::Move, "relief", NumericOperation::Value { value: 120. })).unwrap();
    s.frame(700_000_000, 700_000_000).unwrap();
    assert_eq!(s.renderer_mut().composites, composites, "lighting the paper recomposites nothing");
    assert_eq!(paper_control(&s, "relief").value, layer_core::EffectValue::Number(120.));
    assert!(!paper_control(&s, "light").disabled);
    s.dispatch(gesture(ContactPhase::Up, "relief", NumericOperation::Value { value: 150. })).unwrap();
    let lit = s.engine.checkpoint();
    assert_ne!(lit, before);
    assert_eq!(layer_core::paper::lit_paper(s.engine.document().scene()).map(|(_, light)| light.relief), Some(1.5));
    s.dispatch(gesture(ContactPhase::Down, "light", NumericOperation::Dial { x: 10., y: 0.5, radius: 20. })).unwrap();
    assert_eq!(paper_control(&s, "light").value, layer_core::EffectValue::Number(0.), "pointing right lights from the right");
    s.dispatch(gesture(ContactPhase::Move, "light", NumericOperation::Dial { x: -10., y: -10.4, radius: 20. })).unwrap();
    s.dispatch(gesture(ContactPhase::Up, "light", NumericOperation::Dial { x: -60., y: 10., radius: 20. })).unwrap();
    assert_eq!(paper_control(&s, "light").value, layer_core::EffectValue::Number(171.), "far from the dial it turns freely");
    s.dispatch(number("light", NumericOperation::Step { steps: 200. })).unwrap();
    assert_eq!(paper_control(&s, "light").value, layer_core::EffectValue::Number(11.), "the direction wraps around");
    invoke(&mut s, CommandId::Undo);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.checkpoint(), lit, "each drag is one step");
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.checkpoint(), before);
    invoke(&mut s, CommandId::Redo);
    s.frame(1_000_000_000, 1_000_000_000).unwrap();
    let composites = s.renderer_mut().composites;
    s.dispatch(gesture(ContactPhase::Down, "through_paint", NumericOperation::Value { value: 100. })).unwrap();
    s.dispatch(gesture(ContactPhase::Move, "through_paint", NumericOperation::Value { value: 60. })).unwrap();
    s.frame(1_100_000_000, 1_100_000_000).unwrap();
    let separated = s.renderer_mut().composites;
    assert!(separated > composites, "once paint hides some texture, the paper leaves the composite");
    s.dispatch(gesture(ContactPhase::Move, "through_paint", NumericOperation::Value { value: 30. })).unwrap();
    s.frame(1_200_000_000, 1_200_000_000).unwrap();
    assert_eq!(s.renderer_mut().composites, separated, "and the drag goes on without recompositing");
    s.dispatch(gesture(ContactPhase::Up, "through_paint", NumericOperation::Value { value: 30. })).unwrap();
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "texture".into(), value: layer_core::EffectValue::Choice(2) } }).unwrap();
    s.frame(2_000_000_000, 2_000_000_000).unwrap();
    assert!(s.renderer_mut().paper_overlay.is_none(), "a lit paper shows its own relief instead of the flash");
}

#[test]
fn paper_relief_waits_for_preparation_and_survives_renderer_replacement() {
    let mut s = session(Platform::Windows);
    let layer = paper_layer(&s);
    s.dispatch(UiAction::SelectLayer { id: layer }).unwrap();
    s.renderer_mut().paper_overlay_pending = true;
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "texture".into(), value: layer_core::EffectValue::Choice(2) } }).unwrap();
    for now in [1_000_000_000, 4_000_000_000] {
        s.frame(now, now).unwrap();
        assert_eq!(s.renderer_mut().paper_overlay.as_ref().map(|o| o.opacity), Some(1.));
        assert!(s.wants_continuous_frames());
    }
    let color = s.engine.backend().document_color();
    s.replace_renderer(Recorder { color, paper_overlay_pending: true, ..Default::default() }).unwrap();
    assert!(s.wants_continuous_frames());
    s.frame(8_000_000_000, 8_000_000_000).unwrap();
    assert_eq!(s.renderer_mut().paper_overlay.as_ref().map(|o| o.opacity), Some(1.));
    s.renderer_mut().paper_overlay_pending = false;
    s.frame(12_000_000_000, 12_000_000_000).unwrap();
    assert_eq!(s.renderer_mut().paper_overlay.as_ref().map(|o| o.opacity), Some(1.));
    s.frame(13_050_000_000, 13_050_000_000).unwrap();
    assert!((s.renderer_mut().paper_overlay.as_ref().unwrap().opacity - 0.5).abs() < 1e-3);
    s.frame(13_300_000_000, 13_300_000_000).unwrap();
    assert!(s.renderer_mut().paper_overlay.is_none());
    assert!(!s.wants_continuous_frames());
}

fn gray_paper(value: u8) -> layer_core::color::source::SourceImage {
    use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
    let interpretation = SourceInterpretation { channels: SourceChannels::Gray, depth: layer_core::color::SampleDepth::U8, profile: Default::default(), profile_assumed: false };
    let mut builder = SourceBuilder::new([8, 8], interpretation, 1 << 20).unwrap();
    for y in 0..8u8 { builder.push_row(&(0..8u8).map(|x| value.wrapping_add(x * 9 + y * 5)).collect::<Vec<_>>()).unwrap(); }
    let mut image = builder.finish().unwrap();
    image.resolution = Some(layer_core::ImageResolution::ppi(600));
    image
}

fn paper_requests(s: &UiSession<Recorder>) -> Vec<crate::paper_library::PaperRequest> {
    s.state.requests.iter().filter_map(|r| match &r.kind { HostRequestKind::Paper { request } => Some(request.clone()), _ => None }).collect()
}

#[test]
fn library_papers_load_when_chosen_and_new_drawings_remember_them() {
    use crate::paper_library::{PaperEntry, PaperRequest, paper_identity};
    let mut s = session(Platform::Gtk);
    let layer = paper_layer(&s);
    s.dispatch(UiAction::SelectLayer { id: layer }).unwrap();
    let picker = |s: &UiSession<Recorder>| paper_control(s, "texture").paper.unwrap();
    assert!(picker(&s).import.is_none(), "no Import without paper storage");
    let image = gray_paper(10);
    let id = paper_identity(&image).unwrap();
    let entry = PaperEntry { id: id.clone(), name: "scan_0042".into(), bytes: 64, preview: vec![128; 96 * 96].into() };
    s.set_paper_library(vec![entry.clone()]).unwrap();
    let view = picker(&s);
    assert_eq!(view.tiles.len(), first_imported() as usize + 1);
    assert_eq!(view.tiles[first_imported() as usize].label.as_ref(), "scan_0042");
    assert_eq!(view.tiles[first_imported() as usize].actions.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(), ["Rename…", "Delete"]);
    assert_eq!(view.import.as_ref().map(|a| a.action.clone()), Some(EffectAction::ImportPaper { layer }));
    let checkpoint = s.engine.checkpoint();
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "texture".into(), value: layer_core::EffectValue::Choice(first_imported()) } }).unwrap();
    assert_eq!(paper_requests(&s), [PaperRequest::Load { id: id.clone() }]);
    assert_eq!(s.engine.checkpoint(), checkpoint, "nothing changes until the paper is read");
    s.paper_loaded(&id, image.clone()).unwrap();
    let layer_core::TextureSource::Image(applied) = s.engine.document().paper(None).texture else { panic!("an image paper") };
    assert_eq!(*applied.storage().as_ref(), image);
    assert_eq!(applied.id(), crate::paper_library::paper_image_id(&id).unwrap());
    assert_eq!(paper_control(&s, "texture").value, layer_core::EffectValue::Choice(first_imported()));
    assert_eq!(s.state.settings.new_document.paper.as_deref(), Some(id.as_str()));
    assert_eq!(s.new_drawing_paper(), layer_core::TextureSource::Image(applied.clone()));
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.engine.checkpoint(), checkpoint, "one step");

    s.dispatch(UiAction::Effect { action: EffectAction::RenamePaper { id: id.clone(), name: " Cotton rag ".into() } }).unwrap();
    assert_eq!(paper_requests(&s).last(), Some(&PaperRequest::Rename { id: id.clone(), name: "Cotton rag".into() }));
    assert!(s.dispatch(UiAction::Effect { action: EffectAction::RenamePaper { id: id.clone(), name: " ".into() } }).is_err());
    s.dispatch(UiAction::Effect { action: EffectAction::ImportPaper { layer } }).unwrap();
    assert_eq!(paper_requests(&s).last(), Some(&PaperRequest::Import { layer: Some(layer) }));
    let imported = gray_paper(90);
    let imported_id = paper_identity(&imported).unwrap();
    let entries = vec![entry, PaperEntry { id: imported_id.clone(), name: "linen scan".into(), bytes: 64, preview: vec![90; 96 * 96].into() }];
    s.paper_imported(Some(layer), entries.clone(), &imported_id, imported).unwrap();
    assert_eq!(paper_control(&s, "texture").value, layer_core::EffectValue::Choice(first_imported() + 1));
    assert_eq!(s.state.settings.new_document.paper.as_deref(), Some(imported_id.as_str()));
    assert!(matches!(s.new_drawing_paper(), layer_core::TextureSource::Image(_)));
    s.set_paper_library(entries[..1].to_vec()).unwrap();
    assert_eq!(s.new_drawing_paper(), layer_core::paper::default_texture(), "a deleted paper is not remembered");
    assert!(matches!(s.engine.document().paper(None).texture, layer_core::TextureSource::Image(_)), "the drawing keeps its copy");

    let mut reopened = UiSession::new(Recorder::default(), s.engine.document().clone(), [1000, 1000], Platform::Gtk).unwrap();
    let paper = paper_layer(&reopened);
    reopened.dispatch(UiAction::SelectLayer { id: paper }).unwrap();
    let own = paper_control(&reopened, "texture").paper.unwrap();
    assert_eq!(own.tiles[0].label.as_ref(), "This drawing’s paper", "another library does not know the image");
    assert!(own.tiles[0].actions.is_empty(), "nothing to add to without paper storage");
    reopened.set_paper_library(Vec::new()).unwrap();
    let own = paper_control(&reopened, "texture").paper.unwrap();
    assert_eq!(own.tiles[0].actions.iter().map(|a| a.action.clone()).collect::<Vec<_>>(), [EffectAction::AddPaper { layer: paper }]);
    reopened.dispatch(UiAction::Effect { action: own.tiles[0].actions[0].action.clone() }).unwrap();
    assert!(matches!(paper_requests(&reopened).last(), Some(PaperRequest::Add { layer, .. }) if *layer == paper));
    assert!(reopened.paper_image(paper).is_some());
}

#[test]
fn paper_rows_name_their_texture_and_previews_resolve_by_identity() {
    use crate::paper_library::{PaperEntry, paper_identity};
    let mut s = session(Platform::Gtk);
    let layer = paper_layer(&s);
    let row = |s: &UiSession<Recorder>| s.state.layers.iter().find(|row| row.id == layer).unwrap().paper.clone().unwrap();
    assert_eq!(row(&s), crate::LayerPaper { id: Some("drawing_paper".into()), label: "Fine tooth".into(), editable: true });
    assert!(s.state.layers.iter().filter(|row| row.id != layer).all(|row| row.paper.is_none()));
    assert_eq!(s.paper_preview("drawing_paper"), Some(layer_core::paper::built_in_paper("drawing_paper").unwrap().preview.clone()));
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "texture".into(), value: layer_core::EffectValue::Choice(2) } }).unwrap();
    assert_eq!(row(&s).id.as_deref(), Some("rough"));
    s.dispatch(UiAction::SelectLayer { id: layer }).unwrap();
    let tiles = paper_control(&s, "texture").paper.unwrap().tiles;
    assert_eq!(tiles[2].id.as_deref(), Some("rough"));
    assert!(tiles.iter().all(|tile| tile.id.as_deref().and_then(|id| s.paper_preview(id)).is_some()));
    let image = gray_paper(40);
    let id = paper_identity(&image).unwrap();
    s.set_paper_library(vec![PaperEntry { id: id.clone(), name: "scan".into(), bytes: 64, preview: vec![7; 96 * 96].into() }]).unwrap();
    s.paper_loaded(&id, image).unwrap();
    s.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "texture".into(), value: layer_core::EffectValue::Choice(first_imported()) } }).unwrap();
    s.paper_loaded(&id, gray_paper(40)).unwrap();
    assert_eq!(row(&s), crate::LayerPaper { id: Some(id.as_str().into()), label: "scan".into(), editable: true });
    assert_eq!(s.paper_preview(&id).as_deref(), Some(&[7; 96 * 96][..]));
    s.set_paper_library(Vec::new()).unwrap();
    assert_eq!(row(&s), crate::LayerPaper { id: None, label: "This drawing’s paper".into(), editable: true });
    assert_eq!(s.paper_preview(&id), None);
    s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: layer, value: true } }).unwrap();
    assert!(!row(&s).editable);
}

#[test]
fn a_late_library_read_cannot_replace_a_newer_paper_choice() {
    use crate::paper_library::{PaperEntry, paper_identity};
    for platform in [Platform::Gtk, Platform::Mac, Platform::Ios, Platform::Windows] {
        for different in [false,true] {
            let mut s = session(platform);
            let layer = paper_layer(&s);
            s.dispatch(UiAction::SelectLayer { id: layer }).unwrap();
            let current = paper_control(&s,"texture").value;
            let image = gray_paper(90);
            let id = paper_identity(&image).unwrap();
            s.set_paper_library(vec![PaperEntry { id: id.clone(), name: "linen scan".into(), bytes: 64,
                preview: vec![90;96 * 96].into() }]).unwrap();
            s.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "texture".into(), value: layer_core::EffectValue::Choice(first_imported()) } }).unwrap();
            let request = s.state.requests.iter().find(|request|matches!(&request.kind,HostRequestKind::Paper { .. })).unwrap().id;
            let value = if different { layer_core::EffectValue::Choice(2) } else { current };
            s.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "texture".into(), value: value.clone() } }).unwrap();
            let document = s.engine.document().clone();
            let mark = s.engine.checkpoint();
            let modified = s.state.document_file.modified;
            s.paper_loaded(&id,image).unwrap();
            s.dispatch(UiAction::CompleteRequest { id:request,error:None }).unwrap();
            assert_live_artwork_eq(s.engine.document(),&document);
            assert_eq!(s.engine.checkpoint(),mark,"the stale read must not add history");
            assert_eq!(s.state.document_file.modified,modified);
            assert_eq!(paper_control(&s,"texture").value,value);
            assert!(!s.state.requests.iter().any(|item|item.id==request));
        }
    }
}

#[test]
fn scaling_paper_preserves_the_pending_library_choice() {
    use crate::paper_library::{PaperEntry, paper_identity};
    for platform in [Platform::Mac, Platform::Windows] {
        let mut s = session(platform);
        let layer = paper_layer(&s);
        s.dispatch(UiAction::SelectLayer { id:layer }).unwrap();
        let image = gray_paper(90);let id = paper_identity(&image).unwrap();
        s.set_paper_library(vec![PaperEntry { id:id.clone(),name:"linen scan".into(),bytes:64,preview:vec![90;96 * 96].into() }]).unwrap();
        s.dispatch(UiAction::Effect { action:EffectAction::Set {layer,key:"texture".into(),value:layer_core::EffectValue::Choice(first_imported())} }).unwrap();
        s.dispatch(UiAction::Effect { action:EffectAction::Number {layer,key:"scale".into(),operation:NumericOperation::Value {value:1.5}} }).unwrap();
        s.paper_loaded(&id,image.clone()).unwrap();
        assert_eq!(*s.paper_image(layer).unwrap(),image);
        assert!((paper_scale(&s)-1.5).abs()<1e-4);
    }
}

#[test]
fn imported_and_loaded_papers_save_the_remembered_paper_through_host_requests() {
    use crate::paper_library::{PaperEntry,paper_identity};
    for imported in [false,true] {
        let mut s=session(Platform::Mac);let layer=paper_layer(&s);
        s.dispatch(UiAction::SelectLayer {id:layer}).unwrap();
        let image=gray_paper(90);let id=paper_identity(&image).unwrap();
        let entry=PaperEntry {id:id.clone(),name:"linen scan".into(),bytes:64,preview:vec![90;96 * 96].into()};
        if imported { s.paper_imported(Some(layer),vec![entry],&id,image).unwrap(); }
        else {
            s.set_paper_library(vec![entry]).unwrap();
            s.dispatch(UiAction::Effect {action:EffectAction::Set {layer,key:"texture".into(),value:layer_core::EffectValue::Choice(first_imported())}}).unwrap();
            s.paper_loaded(&id,image).unwrap();
        }
        assert_eq!(s.state.requests.iter().filter(|request|matches!(&request.kind,
            HostRequestKind::SaveSettings {settings} if settings.new_document.paper.as_deref()==Some(id.as_str()))).count(),1,
            "async adoption must enqueue exactly one save containing the adopted paper identity");
        let mark=s.engine.checkpoint();
        let saves=s.state.requests.iter().filter(|request|matches!(request.kind,HostRequestKind::SaveSettings {..})).count();
        s.paper_loaded(&id,gray_paper(90)).unwrap();
        assert_eq!(s.engine.checkpoint(),mark);
        assert_eq!(s.state.requests.iter().filter(|request|matches!(request.kind,HostRequestKind::SaveSettings {..})).count(),saves,
            "cache-only reads must not request redundant preference writes");
    }
}

#[test]
fn late_remembered_library_loading_preserves_the_existing_startup_stroke() {
    use crate::paper_library::{PaperEntry,paper_identity};
    for platform in [Platform::Gtk,Platform::Mac,Platform::Ios] {
        let mut s=UiSession::blank(Recorder::default(),[1000,1000],platform).unwrap();
        let image=gray_paper(90);let id=paper_identity(&image).unwrap();
        let mut settings=Settings::default();settings.new_document.paper=Some(id.clone());
        s.dispatch(UiAction::RestoreSavedSettings {saved:serde_json::to_string(&settings).unwrap()}).unwrap();
        s.set_paper_library(vec![PaperEntry {id:id.clone(),name:"linen scan".into(),bytes:64,preview:vec![90;96 * 96].into()}]).unwrap();
        let request=s.state.requests.iter().find(|request|matches!(&request.kind,HostRequestKind::Paper {..})).unwrap().id;
        stroke(&mut s,10);assert!(s.engine.can_undo());
        let before=s.engine.document().clone();let mark=s.engine.checkpoint();let modified=s.state.document_file.modified;
        s.paper_loaded(&id,image.clone()).unwrap();s.dispatch(UiAction::CompleteRequest {id:request,error:None}).unwrap();
        assert_live_artwork_eq(s.engine.document(),&before);assert_eq!(s.engine.checkpoint(),mark);
        assert_eq!(s.state.document_file.modified,modified);assert!(!s.state.requests.iter().any(|item|item.id==request));
        let layer_core::TextureSource::Image(warmed)=s.new_drawing_paper() else {panic!("remembered image is available only to subsequent new drawings")};
        assert_eq!(*warmed.storage().as_ref(),image);
    }
}

#[test]
fn recovery_keeps_its_authored_paper_when_the_initial_default_was_prepared_from_another_image() {
    use crate::paper_library::{PaperEntry,paper_identity,paper_image_id};
    use layer_core::package::ImmutableBacking;
    use std::sync::{Arc,atomic::AtomicBool};
    let mut source=session(Platform::Mac);let source_layer=paper_layer(&source);
    let authored=gray_paper(90);let authored_id=paper_identity(&authored).unwrap();
    source.paper_imported(Some(source_layer),vec![],&authored_id,authored.clone()).unwrap();
    source.frame(1,1).unwrap();
    let expected=source.engine.document().clone();let mark=source.engine.checkpoint();
    let cancelled=AtomicBool::new(false);let mut bytes=Vec::new();
    source.capture_session().unwrap().prepare(&cancelled).unwrap().write(&mut bytes,&cancelled).unwrap();
    let remembered=gray_paper(10);let id=paper_identity(&remembered).unwrap();
    let texture=layer_core::TextureSource::Image(layer_core::authored::Image::with_id(paper_image_id(&id).unwrap(),Arc::new(remembered.clone())));
    let mut settings=Settings::default();settings.new_document.paper=Some(id.clone());
    let mut initial=UiSession::blank_with_settings(Recorder::default(),[1000,1000],Platform::Mac,source.localization().clone(),settings,None).unwrap();
    initial.initialize_paper(Some(id.clone()),texture.clone(),None);initial.frame(0,0).unwrap();
    assert_eq!(initial.new_drawing_paper(),texture);
    assert!(!initial.engine.can_undo());assert!(!initial.engine.can_redo());assert!(!initial.state.document_file.modified);
    let backing=ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();
    let recovery=crate::session_recovery::SessionRestore::open(backing,layer_core::ProjectLimits::default(),&cancelled).unwrap();
    let mut restored=UiSession::from_project_localized(Recorder::default(),recovery.document().clone(),None,[1000,1000],Platform::Mac,source.localization().clone()).unwrap();
    restored.frame(0,0).unwrap();restored.restore_session(recovery,false,None).unwrap();
    restored.inherit_window_state(&initial).unwrap();
    restored.set_paper_library(vec![PaperEntry {id:id.clone(),name:"Cotton".into(),bytes:64,preview:vec![10;96 * 96].into()}]).unwrap();
    restored.paper_loaded(&id,remembered).unwrap();
    assert_live_artwork_eq(restored.engine.document(),&expected);assert_eq!(restored.engine.checkpoint(),mark);
    let layer_core::TextureSource::Image(actual)=restored.engine.document().paper(None).texture else {panic!("recovered authored image")};
    assert_eq!(*actual.storage().as_ref(),authored);
    assert_eq!(restored.state.settings.new_document.paper.as_deref(),Some(id.as_str()));
}

#[test]
fn deleted_and_readded_paper_reuses_retained_history_source_after_reload() {
    use crate::paper_library::{PaperEntry, paper_identity};
    for platform in [Platform::Gtk,Platform::Mac,Platform::Ios] {
        let mut s = session(platform); let layer = paper_layer(&s);
        s.dispatch(UiAction::SelectLayer { id:layer }).unwrap();
        let initial = s.engine.document().clone(); let initial_checkpoint = s.engine.checkpoint();
        let source = gray_paper(37); let id = paper_identity(&source).unwrap();
        let entry = PaperEntry { id:id.clone(),name:"Cotton".into(),bytes:64,preview:vec![128;96 * 96].into() };
        s.paper_imported(Some(layer),vec![entry.clone()],&id,source.clone()).unwrap();
        let imported = s.engine.document().clone(); let imported_checkpoint = s.engine.checkpoint();
        s.set_paper_library(Vec::new()).unwrap();
        s.dispatch(UiAction::Effect { action:EffectAction::AddPaper { layer } }).unwrap();
        let add = s.state.requests.iter().find(|r|matches!(&r.kind,HostRequestKind::Paper { request:crate::paper_library::PaperRequest::Add { .. } })).unwrap().id;
        s.set_paper_library(vec![entry]).unwrap();
        s.dispatch(UiAction::CompleteRequest { id:add,error:None }).unwrap();
        let load = s.state.requests.iter().find(|r|matches!(&r.kind,HostRequestKind::Paper { request:crate::paper_library::PaperRequest::Load { .. } })).unwrap().id;
        let decoded = layer_core::paper::decode_png(&layer_core::paper::encode_png(&source).unwrap()).unwrap();
        assert_eq!(paper_identity(&decoded).unwrap(),id);
        s.paper_loaded(&id,decoded).unwrap();
        s.dispatch(UiAction::CompleteRequest { id:load,error:None }).unwrap();
        assert_eq!(s.engine.checkpoint(),imported_checkpoint); assert_live_artwork_eq(s.engine.document(),&imported);
        s.dispatch(UiAction::Effect { action:EffectAction::Reset { layer,key:"texture".into() } }).unwrap();
        let reset = s.engine.checkpoint();
        s.dispatch(UiAction::Effect { action:EffectAction::Set { layer,key:"texture".into(),value:layer_core::EffectValue::Choice(first_imported()) } })
            .expect("reselecting a reloaded library paper must retain its authored source identity");
        assert_live_artwork_eq(s.engine.document(),&imported);
        invoke(&mut s,CommandId::Undo); assert_eq!(s.engine.checkpoint(),reset);
        invoke(&mut s,CommandId::Undo); assert_eq!(s.engine.checkpoint(),imported_checkpoint); assert_live_artwork_eq(s.engine.document(),&imported);
        invoke(&mut s,CommandId::Undo); assert_eq!(s.engine.checkpoint(),initial_checkpoint); assert_live_artwork_eq(s.engine.document(),&initial);
        for _ in 0..3 { invoke(&mut s,CommandId::Redo); }
        assert_live_artwork_eq(s.engine.document(),&imported);
        let document = s.engine.document();
        let occurrence = layer_core::paper::paper_occurrence(document.scene(),None).unwrap();
        let effect = document.scene().effect_handle(occurrence).unwrap();
        let bad = layer_core::authored::Image::with_id(crate::paper_library::paper_image_id(&id).unwrap(),std::sync::Arc::new(gray_paper(99)));
        let mut edited = document.scene().effect_application(occurrence).unwrap().clone();
        let mut draft=layer_core::EffectInstance {program:edited.program.clone(),values:edited.values};
        draft.set("texture",layer_core::EffectValue::Texture(layer_core::TextureSource::Image(bad))).unwrap();edited.values=draft.values;
        let edit = layer_core::Edit::Effect(layer_core::authored::RecordChange::replace(&document.artwork.effects,effect,Some(edited)).unwrap());
        let checkpoint = s.engine.checkpoint();
        assert!(s.engine.apply_edit(edit).unwrap_err().to_string().contains("Immutable image identity conflicts with retained history"));
        assert_eq!(s.engine.checkpoint(),checkpoint); assert_live_artwork_eq(s.engine.document(),&imported);
    }
}

#[test]
fn evicted_library_paper_reuses_retained_history_source_after_reload() {
    use crate::paper_library::{PaperEntry,paper_identity};
    for platform in [Platform::Gtk,Platform::Mac,Platform::Ios] {
        let mut s = session(platform); let layer = paper_layer(&s);
        s.dispatch(UiAction::SelectLayer { id:layer }).unwrap();
        let images = (0..4).map(|n|gray_paper(20 + n * 30)).collect::<Vec<_>>();
        let ids = images.iter().map(|image|paper_identity(image).unwrap()).collect::<Vec<_>>();
        let entries = ids.iter().enumerate().map(|(n,id)|PaperEntry { id:id.clone(),name:format!("Paper {n}"),bytes:64,preview:vec![128;96 * 96].into() }).collect::<Vec<_>>();
        s.paper_imported(Some(layer),entries,&ids[0],images[0].clone()).unwrap();
        let original = s.engine.document().clone(); let checkpoint = s.engine.checkpoint();
        for (id,image) in ids.iter().zip(&images).skip(1) { s.paper_loaded(id,image.clone()).unwrap(); }
        assert!(s.papers.image(&ids[0]).is_none(),"the original three-entry strong cache still evicts");
        s.dispatch(UiAction::Effect { action:EffectAction::Reset { layer,key:"texture".into() } }).unwrap();
        let reset = s.engine.checkpoint();
        s.dispatch(UiAction::Effect { action:EffectAction::Set { layer,key:"texture".into(),value:layer_core::EffectValue::Choice(first_imported()) } }).unwrap();
        let decoded = layer_core::paper::decode_png(&layer_core::paper::encode_png(&images[0]).unwrap()).unwrap();
        assert_eq!(paper_identity(&decoded).unwrap(),ids[0]);
        s.paper_loaded(&ids[0],decoded).expect("an evicted paper must reuse the source retained by authored history");
        assert_live_artwork_eq(s.engine.document(),&original);
        invoke(&mut s,CommandId::Undo); assert_eq!(s.engine.checkpoint(),reset);
        invoke(&mut s,CommandId::Undo); assert_eq!(s.engine.checkpoint(),checkpoint); assert_live_artwork_eq(s.engine.document(),&original);
        for _ in 0..2 { invoke(&mut s,CommandId::Redo); }
        assert_live_artwork_eq(s.engine.document(),&original);
    }
}

#[test]
fn opened_and_recovered_paper_uses_destination_owners_before_inherited_cache() {
    use crate::paper_library::{PaperEntry,paper_identity,paper_image_id};
    use layer_core::package::ImmutableBacking;
    use std::sync::{Arc,atomic::AtomicBool};
    let portable=|document:&layer_core::Document| {
        package_bytes(&layer_core::Editor::new(document.clone()).capture(0,document.output().context.clone()).unwrap())
    };
    for platform in [Platform::Mac,Platform::Ios] {
        for mode in 0..3 {
            let source=gray_paper(63);let id=paper_identity(&source).unwrap();
            let entry=PaperEntry {id:id.clone(),name:"Cotton".into(),bytes:64,preview:vec![128;96 * 96].into()};
            let mut authored=session(platform);let layer=paper_layer(&authored);
            authored.dispatch(UiAction::SelectLayer {id:layer}).unwrap();
            authored.paper_imported(Some(layer),vec![entry.clone()],&id,source.clone()).unwrap();
            let imported=authored.engine.document().clone();
            if mode==2 {authored.dispatch(UiAction::Effect {action:EffectAction::Reset {layer,key:"texture".into()}}).unwrap();}
            authored.frame(1,1).unwrap();
            let expected=authored.engine.document().clone();
            let mut opened=if mode==0 {
                UiSession::from_project(Recorder::default(),package_roundtrip(&expected),None,[1000,1000],platform).unwrap()
            } else {
                let cancel=AtomicBool::new(false);let mut bytes=Vec::new();
                authored.capture_session().unwrap().prepare(&cancel).unwrap().write(&mut bytes,&cancel).unwrap();
                let backing=ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();
                let recovery=crate::session_recovery::SessionRestore::open(backing,layer_core::ProjectLimits::default(),&cancel).unwrap();
                let mut recovered=UiSession::from_project_localized(Recorder::default(),recovery.document().clone(),None,[1000,1000],platform,authored.localization().clone()).unwrap();
                recovered.frame(0,0).unwrap();recovered.restore_session(recovery,false,None).unwrap();recovered
            };
            let decoded=layer_core::paper::decode_png(&layer_core::paper::encode_png(&source).unwrap()).unwrap();
            let texture=layer_core::TextureSource::Image(layer_core::Image::with_id(paper_image_id(&id).unwrap(),Arc::new(decoded)));
            let mut settings=Settings::default();settings.new_document.paper=Some(id.clone());
            let mut previous=UiSession::blank_with_settings(Recorder::default(),[1000,1000],platform,authored.localization().clone(),settings,None).unwrap();
            previous.initialize_paper(Some(id.clone()),texture,None);previous.frame(0,0).unwrap();
            if mode==0 {opened.frame(0,0).unwrap();}opened.inherit_window_state(&previous).unwrap();
            opened.set_paper_library(vec![entry]).unwrap();
            let layer=paper_layer(&opened);opened.dispatch(UiAction::SelectLayer {id:layer}).unwrap();
            let baseline=opened.engine.document().clone();
            assert_eq!(portable(&baseline),portable(&expected));
            let checkpoint=opened.engine.checkpoint();
            opened.dispatch(UiAction::Effect {action:EffectAction::Set {layer,key:"texture".into(),value:layer_core::EffectValue::Choice(first_imported())}})
                .expect("an inherited cached owner must defer to the destination editor's verified Paper worker");
            assert_eq!(paper_requests(&opened).last(),Some(&crate::paper_library::PaperRequest::Load {id:id.clone()}));
            assert_eq!(opened.engine.checkpoint(),checkpoint);assert_live_artwork_eq(opened.engine.document(),&baseline);
            opened.paper_loaded(&id,source.clone()).unwrap();
            assert_eq!(portable(opened.engine.document()),portable(&imported));
            let applied=opened.engine.document().clone();
            if mode==2 {
                invoke(&mut opened,CommandId::Undo);assert_eq!(opened.engine.checkpoint(),checkpoint);assert_live_artwork_eq(opened.engine.document(),&baseline);
                invoke(&mut opened,CommandId::Redo);assert_live_artwork_eq(opened.engine.document(),&applied);
            } else {assert_eq!(opened.engine.checkpoint(),checkpoint,"same authored Paper adds no history");assert_live_artwork_eq(opened.engine.document(),&baseline);}
        }
    }
}

#[test]
fn a_library_import_from_new_drawing_becomes_its_paper_without_changing_the_drawing() {
    use crate::paper_library::{PaperEntry, PaperRequest, paper_identity};
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::NewDocument);
    let new_drawing = |s: &mut UiSession<Recorder>, action| { s.dispatch(UiAction::NewDrawing { action }).unwrap(); };
    new_drawing(&mut s, NewDrawingAction::Open { screen: None, untitled: 2 });
    new_drawing(&mut s, NewDrawingAction::Paper);
    let checkpoint = s.engine.checkpoint();
    assert!(matches!(s.capture_library_import().action, Some(PaperRequest::Import { layer: None })));
    let image = gray_paper(70);
    let id = paper_identity(&image).unwrap();
    let entry = PaperEntry { id: id.clone(), name: "Cotton".into(), bytes: 64, preview: vec![70; 96 * 96].into() };
    s.paper_imported(None, vec![entry], &id, image).unwrap();
    let Some(NewDrawingDrawerView::Paper { tiles, .. }) = s.state.new_drawing.clone().unwrap().drawer else { panic!("the paper drawer") };
    assert!(tiles.iter().any(|tile| tile.selected && tile.id.as_deref() == Some(id.as_str()) && tile.label.as_ref() == "Cotton"));
    assert_eq!(s.engine.checkpoint(), checkpoint);
    new_drawing(&mut s, NewDrawingAction::UsePaper);
    new_drawing(&mut s, NewDrawingAction::Create);
    assert!(matches!(s.new_drawing_order().unwrap().paper, crate::paper_library::PaperMaterial::Ready(layer_core::TextureSource::Image(_))));
}
