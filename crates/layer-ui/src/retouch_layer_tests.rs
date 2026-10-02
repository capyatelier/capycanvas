mod retouch_layer_checks {
    use super::*;
    use layer_core::{BlendSpace, Edit, LayerBlend, LayerKind, LayerOperationKind};
    use std::collections::BTreeSet;

    const LINEAR_REASON: &str = "Frequency Separation needs Perceptual blending. Change it in Edit ▸ Blending.";

    fn perceptual() -> UiSession<Recorder> {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::BlendPerceptual);
        s.frame(1, 1).unwrap();
        s
    }

    fn names(s: &UiSession<Recorder>) -> Vec<String> {
        s.engine.document().layers.iter().map(|l| l.name.to_string()).collect()
    }

    fn submenu(menu: &ContextMenu, label: &str) -> Vec<ContextMenuItem> {
        menu.sections.iter().flatten().find(|i| i.label == label).expect(label).sections.concat()
    }

    fn separation(s: &mut UiSession<Recorder>, action: FrequencySeparationAction) -> Result<UiChange, String> {
        s.dispatch(UiAction::FrequencySeparation { action })
    }

    fn previewed(s: &mut UiSession<Recorder>, time: u64) -> Option<layer_core::Layer> {
        s.frame(time, time).unwrap();
        let document: BTreeSet<_> = s.engine.document().layers.iter().map(|l| l.id).collect();
        s.renderer_mut().frame_layers.iter().find(|l| l.visible && !document.contains(&l.id)).cloned()
    }

    #[test]
    fn new_dodge_and_burn_layer_fills_a_soft_light_layer_with_neutral_gray_in_one_step() {
        for (space, gray) in [(BlendSpace::Perceptual, layer_core::color::RgbSpace::Srgb.decode(128. / 255.) as f32), (BlendSpace::Linear, 0.5)] {
            let mut s = session(Platform::Gtk);
            if space == BlendSpace::Perceptual {
                invoke(&mut s, CommandId::BlendPerceptual);
            }
            let before = s.engine.document().layers.clone();
            let new = submenu(&s.application_menu(ApplicationMenu::Layer), "New");
            assert!(new.iter().any(|i| i.label == "New Dodge & Burn Layer" && i.enabled), "Layer › New");
            invoke(&mut s, CommandId::NewDodgeBurnLayer);
            s.frame(1, 1).unwrap();
            let id = s.engine.document().active_layer;
            let layer = s.engine.document().layer(id).unwrap();
            assert_eq!((&*layer.name, layer.properties.blend), ("Dodge & Burn", LayerBlend::SoftLight));
            assert_eq!(s.engine.document().layers[0].id, id, "above the active layer");
            assert_eq!(s.layer_interaction.selected, [id].into());
            let [(target, fill)] = &s.renderer_mut().pending_operations[..] else { panic!("one fill") };
            assert_eq!(*target, id);
            let LayerOperationKind::Fill { color, alpha_locked: false } = fill.kind else { panic!("a fill") };
            assert!((color[0] - gray).abs() < 1e-7 && color[..3].iter().all(|c| *c == color[0]) && color[3] == 1., "{space:?}: {color:?}");
            invoke(&mut s, CommandId::Undo);
            assert_eq!(s.engine.document().layers, before, "{space:?}: one undo step");
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
        let photo = s.engine.document().active_layer;
        s.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: photo.0, value: false } }).unwrap();
        assert_eq!(s.command_disabled_reason(CommandId::FrequencySeparation).as_deref(), Some("Show the layer first"));
    }

    #[test]
    fn frequency_separation_previews_the_blur_live_and_cancel_leaves_nothing() {
        let mut s = perceptual();
        let photo = s.engine.document().active_layer;
        let before = s.engine.document().layers.clone();
        let undo = s.command(CommandId::Undo).enabled;
        assert!(previewed(&mut s, 2).is_none());
        invoke(&mut s, CommandId::FrequencySeparation);
        let view = s.state.layer_tools.frequency_separation.clone().expect("the dialog opens");
        assert_eq!((view.title, view.label, view.radius, view.numeric.unit.as_str()), ("Frequency Separation", "Radius", 4., "px"));
        let preview = previewed(&mut s, 3).expect("the blur shows on the canvas");
        let effect = preview.effect.as_ref().unwrap();
        assert_eq!((effect.program.id.as_ref(), effect.value("sigma")), ("gaussian_blur", Some(&layer_core::EffectValue::Number(4.))));
        let frame = &s.renderer_mut().frame_layers;
        let index = frame.iter().position(|l| l.id == preview.id).unwrap();
        assert!(preview.properties.clipped && frame[index + 1].id == photo, "clipped to the layer, directly above it");
        let change = separation(&mut s, FrequencySeparationAction::Radius { radius: 9.5 }).unwrap();
        assert_eq!(change.regions, 0, "the dialog shows its own value");
        assert_eq!(s.state.layer_tools.frequency_separation.as_ref().unwrap().radius, 9.5);
        let preview = previewed(&mut s, 4).unwrap();
        assert_eq!(preview.effect.as_ref().unwrap().value("sigma"), Some(&layer_core::EffectValue::Number(9.5)));
        assert!(s.wants_continuous_frames(), "frames continue until the value rests");
        let rested = s.frame(4 + selection_refine::SETTLE_NS, 4 + selection_refine::SETTLE_NS).unwrap();
        assert_ne!(rested.regions & regions::BRUSH, 0, "the rested value is published");
        assert!(!s.wants_continuous_frames());
        assert!(separation(&mut s, FrequencySeparationAction::Radius { radius: 1000. }).is_err());
        assert_eq!(s.engine.document().layers, before, "previews never enter the document");
        separation(&mut s, FrequencySeparationAction::Cancel).unwrap();
        assert!(s.state.layer_tools.frequency_separation.is_none());
        assert!(previewed(&mut s, 5).is_none(), "the canvas shows the layer again");
        assert_eq!(s.engine.document().layers, before);
        assert_eq!(s.command(CommandId::Undo).enabled, undo, "no history step");
        assert!(separation(&mut s, FrequencySeparationAction::Apply).is_err());
    }

    #[test]
    fn color_sampling_waits_for_every_region_of_a_bake() {
        let mut s = perceptual();
        s.engine.apply_edit(Edit::SetCanvasSize { size: [1280, 768], origin: [0, 0] }).unwrap();
        let mut layer = s.engine.document().layers[0].clone();
        layer.source = Some(layer_core::color::source::rgba8_source([1280, 768], |_, _| [80, 120, 160, 255]));
        s.engine.apply_edit(Edit::ReplaceLayer(Box::new(layer))).unwrap();
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
        let photo = s.engine.document().active_layer;
        s.engine.apply_edit(Edit::SetLayerOpacity { id: photo, opacity: 0.75 }).unwrap();
        let before = s.engine.document().layers.clone();
        invoke(&mut s, CommandId::FrequencySeparation);
        separation(&mut s, FrequencySeparationAction::Radius { radius: 6. }).unwrap();
        separation(&mut s, FrequencySeparationAction::Apply).unwrap();
        assert!(s.state.layer_tools.frequency_separation.is_none());
        assert_eq!(names(&s), ["Frequency Separation", "High", "Low", &*before[0].name, "Paper"]);
        let doc = s.engine.document();
        let group = &doc.layers[0];
        assert_eq!((group.kind, group.properties.blend, group.opacity), (LayerKind::Group, LayerBlend::Normal, 0.75), "isolated whatever the preference");
        assert_eq!(doc.layers[1].properties.blend, LayerBlend::LinearLight);
        assert_eq!(doc.active_layer, doc.layers[1].id, "High is active");
        assert!(!doc.layer(photo).unwrap().visible);
        let (low, high) = (doc.layers[2].id, doc.layers[1].id);
        assert_eq!(s.layer_interaction.selected, [high].into());
        assert!(previewed(&mut s, 6).is_none(), "the preview ends");
        let bakes: Vec<_> = s
            .renderer_mut()
            .pending_operations
            .iter()
            .map(|(target, op)| match &op.kind {
                LayerOperationKind::Bake { members, .. } => (*target, members[0].effect.as_ref().unwrap().value("sigma").cloned(), members[1].id),
                LayerOperationKind::FrequencyDetail { members, low: reference, .. } => {
                    assert_eq!(*reference, low);
                    (*target, None, members[0].id)
                }
                _ => panic!("a separation operation"),
            })
            .collect();
        let radius = Some(layer_core::EffectValue::Number(6.));
        assert_eq!(bakes, [(low, radius, photo), (high, None, photo)]);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().layers, before, "one undo step");
    }

    #[test]
    fn frequency_separation_closes_when_the_drawing_changes_under_it() {
        let mut s = perceptual();
        invoke(&mut s, CommandId::FrequencySeparation);
        assert!(previewed(&mut s, 2).is_some());
        let photo = s.engine.document().active_layer;
        s.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: photo.0, value: false } }).unwrap();
        s.frame(3, 3).unwrap();
        assert!(s.state.layer_tools.frequency_separation.is_none());
        assert_eq!(s.state.notice.as_ref().unwrap().text, "The drawing changed, so Frequency Separation was closed");
        assert!(previewed(&mut s, 4).is_none());
    }
    #[test]
    fn retouch_names_are_generated_at_creation_and_reopening_preserves_literal_renames() {
        let l = Localizer::shared(UiLanguage::Japanese);
        let mut s = UiSession::new_localized(Recorder::default(), Document::new("retouch", 64, 64, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }),
            [640, 480], Platform::Gtk, l.clone()).unwrap();
        invoke(&mut s, CommandId::NewDodgeBurnLayer);
        s.frame(1, 1).unwrap();
        let id = s.engine.document().active_layer;
        assert_eq!(s.engine.document().layer(id).unwrap().name, l.text(MessageId::RESOURCES_LAYER_DODGE_BURN));
        let name = "私の補正 { $name } \u{2068}لوحة\u{2069} 한글 🎨";
        s.dispatch(UiAction::Layer { action: LayerAction::Rename { id: id.0, name: name.into() } }).unwrap();
        let document = s.engine.document().clone();
        let reopened = UiSession::new_localized(Recorder::default(), document, [640, 480], Platform::Gtk,
            Localizer::shared(UiLanguage::English)).unwrap();
        assert_eq!(reopened.engine.document().layer(id).unwrap().name.as_ref(), name);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().layer(id).unwrap().name, l.text(MessageId::RESOURCES_LAYER_DODGE_BURN));
        invoke(&mut s, CommandId::Undo);
        assert!(s.engine.document().layer(id).is_none());
    }

}
