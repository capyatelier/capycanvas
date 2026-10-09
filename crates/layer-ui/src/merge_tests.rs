mod merge_checks {
    use super::*;
    use layer_core::{Edit, RasterOperationKind};
    use layer_core::authored::*;

    const MERGES: [CommandId; 5] = [
        CommandId::MergeDown,
        CommandId::MergeGroup,
        CommandId::MergeVisible,
        CommandId::FlattenImage,
        CommandId::StampVisible,
    ];

    /// "Upper" above the document's first layer, active, both drawn.
    fn stacked() -> UiSession<Recorder> {
        let mut s = session(Platform::Gtk);
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } }).unwrap();
        s.frame(1, 1).unwrap();
        s
    }

    fn named(s: &UiSession<Recorder>) -> Vec<String> {
        s.engine.document().scene().order().iter().map(|h| s.engine.document().scene().occurrence(*h).unwrap().name.to_string()).collect()
    }

    fn baked(s: &mut UiSession<Recorder>) -> Vec<OccurrenceHandle> {
        s.frame(2, 2).unwrap();
        s.renderer_mut()
            .pending_operations
            .iter()
            .filter_map(|(_, op)| match &op.kind {
                RasterOperationKind::Bake { scene, scope, .. } => Some(scene.view().with_scope(scope).order().iter().copied().filter(|h| scene.view().with_scope(scope).includes(*h)).collect::<Vec<_>>()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn merge_down_bakes_into_one_layer_in_one_undo_step() {
        let mut s = stacked();
        let upper = s.engine.document().working.occurrence.unwrap();
        let lower = s.engine.document().scene().order()[1];
        let before = s.engine.document().clone();
        assert!(s.command(CommandId::MergeDown).enabled);
        invoke(&mut s, CommandId::MergeDown);
        assert_eq!(baked(&mut s), [upper, lower]);
        let result = s.engine.document().working.occurrence.unwrap();
        assert_eq!(s.engine.document().scene().order().len(), 2);
        assert_eq!(s.engine.document().working.layer_selection, [result].into());
        assert!(s.engine.undo().unwrap());
        assert_live_artwork_eq(s.engine.document(), &before);
    }

    #[test]
    fn merges_explain_why_they_are_unavailable() {
        let s = session(Platform::Gtk);
        assert_eq!(s.command_disabled_reason(CommandId::MergeDown).as_deref(), None);
        assert_eq!(s.command_disabled_reason(CommandId::MergeGroup).as_deref(), Some("Select a group to merge"));
        for command in [CommandId::MergeVisible, CommandId::FlattenImage, CommandId::StampVisible] {
            assert_eq!(s.command_disabled_reason(command), None, "{command:?}");
        }
        let mut s = stacked();
        let upper = s.engine.document().working.occurrence.unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: occurrence_token(upper), value: true } }).unwrap();
        assert_eq!(s.command_disabled_reason(CommandId::MergeDown).as_deref(), Some("Unlock the layers to merge first"));
        assert!(s.dispatch(UiAction::Invoke { command: CommandId::MergeDown }).is_err());
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: occurrence_token(upper), value: false } }).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: occurrence_token(upper), value: false } }).unwrap();
        assert_eq!(s.command_disabled_reason(CommandId::MergeDown).as_deref(), Some("Show the layer before merging it"));
        invoke(&mut s, CommandId::QuickMask);
        for command in MERGES {
            assert_eq!(s.command_disabled_reason(command).as_deref(), Some("Return to the artwork first"), "{command:?}");
        }
    }

    #[test]
    fn merge_down_names_what_it_does() {
        let mut s = stacked();
        assert_eq!(s.command(CommandId::MergeDown).label.as_ref(), "Merge Down");
        let upper = s.engine.document().working.occurrence.unwrap();
        let doc = s.engine.document();
        let paint = RecordChange::insert(&doc.artwork.paint, PaintSource { color_mode: Default::default(), domain: doc.composition().size, raster: Default::default(), base: None, operations: Default::default() });
        let mut clip = Occurrence::new(OccurrenceContent::Paint(paint.handle), "Shade");
        clip.attachment = layer_core::Attachment::Clip;
        let clip = RecordChange::insert(&doc.artwork.occurrences, clip);
        let root = doc.composition().result;
        let mut stack = doc.artwork.stacks.get(root).unwrap().clone(); stack.entries.insert(0, clip.handle);
        let membership = RecordChange::replace(&doc.artwork.stacks, root, Some(stack)).unwrap();
        s.layer_edit(Edit::Batch(vec![Edit::Paint(paint), Edit::Occurrence(clip), Edit::Stack(membership)])).unwrap();
        s.frame(3, 3).unwrap();
        assert_eq!(s.command(CommandId::MergeDown).label.as_ref(), "Merge Clipped Layers");
        let instance = layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("levels").unwrap().program());
        let doc = s.engine.document();

        let application = RecordChange::insert(&doc.artwork.effects, EffectApplication::new(instance.program, instance.values, doc.composition().size));
        let mut effect = doc.scene().occurrence(upper).unwrap().clone();
        effect.content = OccurrenceContent::Effect(application.handle);
        let effect = RecordChange::insert(&doc.artwork.occurrences, effect);
        let id = effect.handle;
        let mut stack = doc.artwork.stacks.get(root).unwrap().clone(); stack.entries.insert(0, id);
        let membership = RecordChange::replace(&doc.artwork.stacks, root, Some(stack)).unwrap();
        s.layer_edit(Edit::Batch(vec![Edit::Effect(application), Edit::Occurrence(effect), Edit::Stack(membership)])).unwrap();
        s.layer_edit(s.engine.document().select_occurrence_edit(id).unwrap()).unwrap();
        s.frame(4, 4).unwrap();
        let state = s.state.commands.iter().find(|c| c.id == CommandId::MergeDown).unwrap();
        assert_eq!(state.label.as_ref(), "Apply Effect to Layer Below");
        assert!(state.tooltip.starts_with("Apply Effect to Layer Below"));
    }

    #[test]
    fn flatten_asks_before_discarding_hidden_layers() {
        let mut s = stacked();
        let upper = s.engine.document().working.occurrence.unwrap();
        invoke(&mut s, CommandId::FlattenImage);
        assert_eq!(s.engine.document().scene().order().len(), 1, "nothing hidden: flattens at once");
        assert!(s.engine.undo().unwrap());
        s.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: occurrence_token(upper), value: false } }).unwrap();
        invoke(&mut s, CommandId::FlattenImage);
        let notice = s.state.notice.clone().expect("a confirmation");
        assert_eq!(notice.text, "Flattening discards 1 hidden layer");
        assert_eq!(notice.actions[0].label, "Flatten Image");
        s.set_localization(crate::Localizer::shared(crate::UiLanguage::German));
        let german = s.state.notice.clone().unwrap();
        let mut args = crate::FluentArgs::new(); args.set("count", 1);
        assert_eq!(german.id, notice.id);
        assert_eq!(german.text, s.localization().format(crate::MessageId::COMMANDS_FLATTEN_DISCARDS_HIDDEN_LAYERS, &args));
        assert_ne!(german.text, notice.text, "the notice follows the selected language");
        assert_eq!(german.actions[0].label, CommandId::FlattenImage.localized_label(s.localization()).as_ref());
        assert_eq!(s.engine.document().scene().order().len(), 3, "nothing changes before it is accepted");
        s.dispatch(UiAction::Notice { id: notice.id, accept: true, action: Some(NoticeActionId::Flatten) }).unwrap();
        assert_eq!(named(&s).len(), 1);
        assert!(s.engine.document().scene().occurrence(upper).is_none(), "the hidden layer is discarded");
    }

    #[test]
    fn stamp_visible_and_merge_group_keep_their_structure() {
        let mut s = stacked();
        let layers = s.engine.document().scene().order().len();
        invoke(&mut s, CommandId::StampVisible);
        assert_eq!(s.engine.document().scene().order().len(), layers + 1);
        assert_eq!(named(&s)[0], "Visible");
        s.frame(2, 2).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
        let group = s.engine.document().working.occurrence.unwrap();
        assert!(s.command(CommandId::MergeGroup).enabled);
        invoke(&mut s, CommandId::MergeGroup);
        let result = s.engine.document().scene().occurrence(s.engine.document().working.occurrence.unwrap()).unwrap();
        assert!(result.kind() == LayerKind::Paint && s.engine.document().scene().occurrence(group).is_none());
    }

    #[test]
    fn menus_offer_merges_with_their_live_state() {
        let s = stacked();
        let active = occurrence_token(s.engine.document().working.occurrence.unwrap());
        let commands = |menu: ContextMenu| {
            fn walk(items: &[ContextMenuItem], out: &mut Vec<(CommandId, bool)>) {
                for item in items {
                    if let Some(UiAction::Invoke { command }) = &item.action {
                        out.push((*command, item.enabled));
                    }
                    for section in &item.sections {
                        walk(section, out);
                    }
                }
            }
            let mut out = Vec::new();
            for section in &menu.sections {
                walk(section, &mut out);
            }
            out.into_iter().filter(|(c, _)| MERGES.contains(c)).collect::<Vec<_>>()
        };
        let expected = vec![
            (CommandId::MergeDown, true),
            (CommandId::MergeVisible, true),
            (CommandId::StampVisible, true),
            (CommandId::FlattenImage, true),
        ];
        assert_eq!(commands(s.layer_menu(active, false).unwrap()), expected);
        assert_eq!(commands(s.application_menu(ApplicationMenu::Layer)), expected);
        let lower = occurrence_token(s.engine.document().scene().order()[1]);
        assert_eq!(commands(s.layer_menu(lower, false).unwrap()), expected[1..], "only the active layer merges down");
    }

    #[test]
    fn merge_shortcuts_follow_each_keymap() {
        let chord = |key: &str, command: bool, shift: bool, alt: bool| crate::shortcuts::KeyChord { key: key.into(), command, shift, alt };
        let merge_down = chord("e", true, false, false);
        let merge_visible = chord("e", true, true, false);
        let stamp = chord("e", true, true, true);
        for key in [&merge_down, &merge_visible, &stamp, &chord("m", true, false, false)] {
            assert!(key.available(Platform::Web), "{key:?}");
        }
        let settings = Settings::default();
        assert_eq!(settings.command_keys(CommandId::MergeDown), std::slice::from_ref(&merge_down));
        assert!(settings.command_keys(CommandId::MergeVisible).is_empty());
        assert!(settings.command_keys(CommandId::StampVisible).is_empty());
        for (preset, visible, stamped, down) in [
            ("photoshop", vec![merge_visible.clone()], vec![stamp.clone()], vec![merge_down.clone()]),
            ("affinity", vec![merge_visible.clone()], vec![stamp.clone()], vec![merge_down.clone()]),
            ("gimp", vec![chord("m", true, false, false)], vec![], vec![]),
            ("clip-studio", vec![], vec![], vec![merge_down.clone()]),
        ] {
            let mut settings = Settings::default();
            crate::keymaps::select(&mut settings, preset).unwrap();
            assert_eq!(settings.command_keys(CommandId::MergeVisible), visible, "{preset}");
            assert_eq!(settings.command_keys(CommandId::StampVisible), stamped, "{preset}");
            assert_eq!(settings.command_keys(CommandId::MergeDown), down, "{preset}");
            let preset = crate::keymaps::KEYMAP_PRESETS.iter().find(|p| p.id == preset).unwrap();
            assert!(preset.differences.iter().all(|(_, note)| !note.contains("merge-down")), "{}", preset.id);
        }
    }
}
