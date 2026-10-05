mod retouch_layer_checks {
    use super::*;
    use layer_core::{BlendSpace, Edit, LayerBlend, LayerKind, RasterOperationKind};
    use std::collections::BTreeSet;

    const LINEAR_REASON: &str = "Frequency Separation needs Perceptual blending. Change it in Edit ▸ Blending.";

    fn perceptual() -> UiSession<Recorder> {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::BlendPerceptual);
        s.frame(1, 1).unwrap();
        s
    }

    fn names(s: &UiSession<Recorder>) -> Vec<String> {
        s.engine.document().scene().order().iter().map(|h| s.engine.document().scene().occurrence(*h).unwrap().name.to_string()).collect()
    }

    fn submenu(menu: &ContextMenu, label: &str) -> Vec<ContextMenuItem> {
        menu.sections.iter().flatten().find(|i| i.label == label).expect(label).sections.concat()
    }

    fn separation(s: &mut UiSession<Recorder>, action: FrequencySeparationAction) -> Result<UiChange, String> {
        s.dispatch(UiAction::FrequencySeparation { action })
    }

    fn previewed(s: &mut UiSession<Recorder>, time: u64) -> Option<(SceneSnapshot, OccurrenceHandle)> {
        s.frame(time, time).unwrap();
        let document: BTreeSet<_> = s.engine.document().scene().order().iter().copied().collect();
        let frame = s.renderer_mut().frame_scene.as_ref()?;
        let handle = frame.view().order().iter().copied().find(|h| frame.view().occurrence(*h).is_some_and(|o| o.visible) && !document.contains(h))?;
        Some((frame.clone(), handle))
    }

    #[test]
    fn new_dodge_and_burn_layer_fills_a_soft_light_layer_with_neutral_gray_in_one_step() {
        for (space, gray) in [(BlendSpace::Perceptual, layer_core::color::RgbSpace::Srgb.decode(128. / 255.) as f32), (BlendSpace::Linear, 0.5)] {
            let mut s = session(Platform::Gtk);
            if space == BlendSpace::Perceptual {
                invoke(&mut s, CommandId::BlendPerceptual);
            }
            let before = s.engine.document().clone();
            let new = submenu(&s.application_menu(ApplicationMenu::Layer), "New");
            assert!(new.iter().any(|i| i.label == "New Dodge & Burn Layer" && i.enabled), "Layer › New");
            invoke(&mut s, CommandId::NewDodgeBurnLayer);
            s.frame(1, 1).unwrap();
            let id = s.engine.document().working.occurrence.unwrap();
            let layer = s.engine.document().scene().occurrence(id).unwrap();
            assert_eq!((&*layer.name, layer.blend), ("Dodge & Burn", LayerBlend::SoftLight));
            assert_eq!(s.engine.document().scene().order()[0], id, "above the active layer");
            assert_eq!(s.engine.document().working.layer_selection, [id].into());
            let source = s.engine.document().scene().source_target(id);
            let [(target, fill)] = &s.renderer_mut().pending_operations[..] else { panic!("one fill") };
            assert_eq!(Some(*target), source);
            let RasterOperationKind::Fill { color, alpha_locked: false } = fill.kind else { panic!("a fill") };
            assert!((color[0] - gray).abs() < 1e-7 && color[..3].iter().all(|c| *c == color[0]) && color[3] == 1., "{space:?}: {color:?}");
            invoke(&mut s, CommandId::Undo);
            assert_live_artwork_eq(s.engine.document(), &before);
        }
    }

    #[test]
    fn new_dodge_and_burn_layer_explains_why_it_is_unavailable() {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::QuickMask);
        assert_eq!(s.command_disabled_reason(CommandId::NewDodgeBurnLayer).as_deref(), Some("Return to the artwork first"));
        assert!(s.dispatch(UiAction::Invoke { command: CommandId::NewDodgeBurnLayer }).is_err());
    }

    #[test]
    fn frequency_separation_needs_perceptual_blending_and_names_the_fix() {
        let mut s = session(Platform::Gtk);
        assert_eq!(s.command_disabled_reason(CommandId::FrequencySeparation).as_deref(), Some(LINEAR_REASON));
        let filter = s.application_menu(ApplicationMenu::Filter);
        let item = filter.sections.iter().flatten().find(|i| i.label == "Frequency Separation…").expect("Filter menu");
        assert!(!item.enabled);
        assert_eq!(s.dispatch(UiAction::Invoke { command: CommandId::FrequencySeparation }).unwrap_err(), LINEAR_REASON);
        let mut s = perceptual();
        assert_eq!(s.command_disabled_reason(CommandId::FrequencySeparation), None);
        let photo = s.engine.document().working.occurrence.unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: occurrence_token(photo), value: false } }).unwrap();
        assert_eq!(s.command_disabled_reason(CommandId::FrequencySeparation).as_deref(), Some("Show the layer first"));
    }

    #[test]
    fn frequency_separation_previews_the_blur_live_and_cancel_leaves_nothing() {
        let mut s = perceptual();
        let photo = s.engine.document().working.occurrence.unwrap();
        let before = s.engine.document().clone();
        let undo = s.command(CommandId::Undo).enabled;
        assert!(previewed(&mut s, 2).is_none());
        invoke(&mut s, CommandId::FrequencySeparation);
        let view = s.state.layer_tools.frequency_separation.clone().expect("the dialog opens");
        assert_eq!((view.title, view.label, view.radius, view.numeric.unit.as_str()), ("Frequency Separation", "Radius", 4., "px"));
        let (preview, handle) = previewed(&mut s, 3).expect("the blur shows on the canvas");
        let effect = preview.view().effect(handle).unwrap();
        assert_eq!((effect.program.id.as_ref(), effect.value("sigma")), ("gaussian_blur", Some(&layer_core::EffectValue::Number(4.))));
        let frame = preview.view();
        let index = frame.position(handle).unwrap();
        assert!(frame.occurrence(handle).unwrap().attachment == layer_core::Attachment::Effect && frame.order()[index + 1] == photo, "attached to the layer, directly above it");
        let change = separation(&mut s, FrequencySeparationAction::Radius { radius: 9.5 }).unwrap();
        assert_eq!(change.regions, 0, "the dialog shows its own value");
        assert_eq!(s.state.layer_tools.frequency_separation.as_ref().unwrap().radius, 9.5);
        let (preview, handle) = previewed(&mut s, 4).unwrap();
        assert_eq!(preview.view().effect(handle).unwrap().value("sigma"), Some(&layer_core::EffectValue::Number(9.5)));
        assert!(s.wants_continuous_frames(), "frames continue until the value rests");
        let rested = s.frame(4 + selection_refine::SETTLE_NS, 4 + selection_refine::SETTLE_NS).unwrap();
        assert_ne!(rested.regions & regions::BRUSH, 0, "the rested value is published");
        assert!(!s.wants_continuous_frames());
        assert!(separation(&mut s, FrequencySeparationAction::Radius { radius: 1000. }).is_err());
        assert_live_artwork_eq(s.engine.document(), &before);
        separation(&mut s, FrequencySeparationAction::Cancel).unwrap();
        assert!(s.state.layer_tools.frequency_separation.is_none());
        assert!(previewed(&mut s, 5).is_none(), "the canvas shows the layer again");
        assert_live_artwork_eq(s.engine.document(), &before);
        assert_eq!(s.command(CommandId::Undo).enabled, undo, "no history step");
        assert!(separation(&mut s, FrequencySeparationAction::Apply).is_err());
    }

    #[test]
    fn color_sampling_waits_for_every_region_of_a_bake() {
        let mut s = perceptual();
        let doc = s.engine.document();
        let mut composition = doc.composition().clone(); composition.size = [1280, 768];
        let SourceTarget::Paint(handle) = doc.working.target.unwrap() else { panic!("paint") };
        let mut source = doc.artwork.paint.get(handle).unwrap().clone();
        source.domain = [1280, 768]; source.base = Some(layer_core::PaintBase::new(layer_core::Image::new(layer_core::color::source::rgba8_source([1280, 768], |_, _| [80, 120, 160, 255]))));
        let edit = Edit::Batch(vec![Edit::Composition(RecordChange::replace(&doc.artwork.compositions, doc.artwork.root, Some(composition)).unwrap()), Edit::Paint(RecordChange::replace(&doc.artwork.paint, handle, Some(source)).unwrap())]);
        s.engine.apply_edit(edit).unwrap();
        s.frame(1, 1).unwrap();
        invoke(&mut s, CommandId::FrequencySeparation);
        separation(&mut s, FrequencySeparationAction::Apply).unwrap();
        s.eyedropper.queue(layer_render::ColorSampleSource::Composite, [32, 32]);
        s.frame(2, 2).unwrap();
        assert!(s.engine.has_pending_document_edits());
        assert!(s.renderer_mut().sample_requests.is_empty(), "partial bakes cannot supply artwork samples");
        for frame in 3..100 {
            if !s.engine.has_pending_document_edits() { break; }
            s.frame(frame, frame).unwrap();
            assert!(!s.refresh_commands(), "bake polling keeps command availability current");
        }
        assert!(!s.engine.has_pending_document_edits());
        assert_eq!(s.renderer_mut().sample_requests.len(), 1, "the queued sample survives the bake");
    }

    #[test]
    fn frequency_separation_applies_low_and_high_in_an_isolated_group_in_one_undo_step() {
        let mut s = perceptual();
        preference(&mut s,
            PreferenceAction::Edit { id: PreferenceId::PassThroughGroups, value: PreferenceValue::Bool(true) });
        let photo = s.engine.document().working.occurrence.unwrap();
        let mut occurrence = s.engine.document().scene().occurrence(photo).unwrap().clone(); occurrence.opacity = 0.75;
        s.engine.apply_edit(Edit::Occurrence(RecordChange::replace(&s.engine.document().artwork.occurrences, photo, Some(occurrence)).unwrap())).unwrap();
        let before = s.engine.document().clone();
        invoke(&mut s, CommandId::FrequencySeparation);
        separation(&mut s, FrequencySeparationAction::Radius { radius: 6. }).unwrap();
        separation(&mut s, FrequencySeparationAction::Apply).unwrap();
        assert!(s.state.layer_tools.frequency_separation.is_none());
        assert_eq!(names(&s), ["Frequency Separation", "High", "Low", &*before.scene().occurrence(before.scene().order()[0]).unwrap().name, "Paper"]);
        let doc = s.engine.document();
        let group = doc.scene().occurrence(doc.scene().order()[0]).unwrap();
        assert_eq!((group.kind(), group.blend, group.opacity), (LayerKind::Group, LayerBlend::Normal, 0.75), "isolated whatever the preference");
        assert_eq!(doc.scene().occurrence(doc.scene().order()[1]).unwrap().blend, LayerBlend::LinearLight);
        assert_eq!(doc.working.occurrence.unwrap(), doc.scene().order()[1], "High is active");
        assert!(!doc.scene().occurrence(photo).unwrap().visible);
        let (low, high) = (doc.scene().order()[2], doc.scene().order()[1]);
        assert_eq!(s.engine.document().working.layer_selection, [high].into());
        assert!(previewed(&mut s, 6).is_none(), "the preview ends");
        let low_target = s.engine.document().scene().source_target(low).unwrap();
        let high_target = s.engine.document().scene().source_target(high).unwrap();
        let SourceTarget::Paint(low_paint) = low_target else { panic!("paint") };
        let bakes: Vec<_> = s.renderer_mut().pending_operations.iter().map(|(target, op)| match &op.kind {
            RasterOperationKind::Bake { scene, scope: SceneScope::Members(members), .. } => {
                let effect = scene.view().effect(members[0]).unwrap();
                (*target, effect.value("sigma").cloned(), members[1])
            }
            RasterOperationKind::FrequencyDetail { scope: SceneScope::Members(members), low: reference, .. } => {
                assert_eq!(*reference, low_paint);
                (*target, None, members[0])
            }
            _ => panic!("a separation operation"),
        }).collect();
        let radius = Some(layer_core::EffectValue::Number(6.));
        assert_eq!(bakes, [(low_target, radius, photo), (high_target, None, photo)]);
        invoke(&mut s, CommandId::Undo);
        assert_live_artwork_eq(s.engine.document(), &before);
    }

    #[test]
    fn frequency_separation_closes_when_the_drawing_changes_under_it() {
        let mut s = perceptual();
        invoke(&mut s, CommandId::FrequencySeparation);
        assert!(previewed(&mut s, 2).is_some());
        let photo = s.engine.document().working.occurrence.unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: occurrence_token(photo), value: false } }).unwrap();
        s.frame(3, 3).unwrap();
        assert!(s.state.layer_tools.frequency_separation.is_none());
        assert_eq!(s.state.notice.as_ref().unwrap().text, "The drawing changed, so Frequency Separation was closed");
        assert!(previewed(&mut s, 4).is_none());
    }
    #[test]
    fn retouch_names_are_generated_at_creation_and_reopening_preserves_literal_renames() {
        let l = Localizer::shared(UiLanguage::Japanese);
        let mut s = UiSession::new_localized(Recorder::default(), Document::new(PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }),
            [640, 480], Platform::Gtk, l.clone()).unwrap();
        invoke(&mut s, CommandId::NewDodgeBurnLayer);
        s.frame(1, 1).unwrap();
        let id = s.engine.document().working.occurrence.unwrap();
        assert_eq!(s.engine.document().scene().occurrence(id).unwrap().name, l.text(MessageId::RESOURCES_LAYER_DODGE_BURN));
        let name = "私の補正 { $name } \u{2068}لوحة\u{2069} 한글 🎨";
        s.dispatch(UiAction::Layer { action: LayerAction::Rename { id: occurrence_token(id), name: name.into() } }).unwrap();
        let document = s.engine.document().clone();
        let reopened = UiSession::new_localized(Recorder::default(), document, [640, 480], Platform::Gtk,
            Localizer::shared(UiLanguage::English)).unwrap();
        assert_eq!(reopened.engine.document().scene().occurrence(id).unwrap().name.as_ref(), name);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().scene().occurrence(id).unwrap().name, l.text(MessageId::RESOURCES_LAYER_DODGE_BURN));
        invoke(&mut s, CommandId::Undo);
        assert!(s.engine.document().scene().occurrence(id).is_none());
    }

#[test]
fn frequency_separation_accepts_sigma_85_in_one_undo_step_and_refuses_above_limit() {
    let mut s=perceptual();let before=s.engine.document().clone();
    invoke(&mut s,CommandId::FrequencySeparation);
    let numeric=&s.state.layer_tools.frequency_separation.as_ref().unwrap().numeric;
    assert_eq!((numeric.min,numeric.max,numeric.soft_min,numeric.soft_max,numeric.step),(0.,85.,0.,21.,f64::from(0.1_f32)));
    assert_eq!(numeric.mapping,NumericMapping::Power{exponent:0.5});
    separation(&mut s,FrequencySeparationAction::Radius{radius:85.}).unwrap();
    assert_eq!(s.state.layer_tools.frequency_separation.as_ref().unwrap().radius,85.);
    let checkpoint=s.engine.checkpoint();
    assert!(separation(&mut s,FrequencySeparationAction::Radius{radius:85.1}).is_err());
    assert_eq!(s.engine.checkpoint(),checkpoint);
    separation(&mut s,FrequencySeparationAction::Apply).unwrap();
    s.frame(1,1).unwrap();
    let sigma=s.renderer_mut().pending_operations.iter().find_map(|(_,op)|match &op.kind {
        RasterOperationKind::Bake{scene,scope:SceneScope::Members(members),..}=>Some(scene.view().effect(members[0]).unwrap().value("sigma").cloned()),_=>None
    }).unwrap();assert_eq!(sigma,Some(layer_core::EffectValue::Number(85.)));
    invoke(&mut s,CommandId::Undo);assert_live_artwork_eq(s.engine.document(),&before);
    assert_eq!((s.engine.document().working.occurrence,s.engine.document().composition().blend),(before.working.occurrence,before.composition().blend));
    invoke(&mut s,CommandId::Redo);assert_eq!(s.engine.document().scene().occurrence(s.engine.document().scene().order()[0]).unwrap().name.as_ref(),"Frequency Separation");
}

}
