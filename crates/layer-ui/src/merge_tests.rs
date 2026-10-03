mod merge_checks {
    use super::*;
    use layer_core::{Edit, Layer, LayerOperationKind};

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
        s.engine.document().layers.iter().map(|l| l.name.to_string()).collect()
    }

    fn baked(s: &mut UiSession<Recorder>) -> Vec<LayerId> {
        s.frame(2, 2).unwrap();
        s.renderer_mut()
            .pending_operations
            .iter()
            .filter_map(|(_, op)| match &op.kind {
                LayerOperationKind::Bake { members, .. } => Some(members.iter().map(|l| l.id).collect::<Vec<_>>()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn merge_down_bakes_into_one_layer_in_one_undo_step() {
        let mut s = stacked();
        let upper = s.engine.document().active_layer;
        let lower = s.engine.document().layers[1].id;
        let before = s.engine.document().layers.clone();
        assert!(s.command(CommandId::MergeDown).enabled);
        invoke(&mut s, CommandId::MergeDown);
        assert_eq!(baked(&mut s), [upper, lower]);
        let result = s.engine.document().active_layer;
        assert_eq!(s.engine.document().layers.len(), 2);
        assert_eq!(s.layer_interaction.selected, [result].into());
        assert!(s.engine.undo().unwrap());
        assert_eq!(s.engine.document().layers, before, "one undo step");
    }

    #[test]
    fn merges_explain_why_they_are_unavailable() {
        let s = session(Platform::Gtk);
        assert_eq!(s.command_disabled_reason(CommandId::MergeDown).as_deref(), Some("The paper can't receive merged pixels"));
        assert_eq!(s.command_disabled_reason(CommandId::MergeGroup).as_deref(), Some("Select a group to merge"));
        for command in [CommandId::MergeVisible, CommandId::FlattenImage, CommandId::StampVisible] {
            assert_eq!(s.command_disabled_reason(command), None, "{command:?}");
        }
        let mut s = stacked();
        let upper = s.engine.document().active_layer;
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: upper.0, value: true } }).unwrap();
        assert_eq!(s.command_disabled_reason(CommandId::MergeDown).as_deref(), Some("Unlock the layers to merge first"));
        assert!(s.dispatch(UiAction::Invoke { command: CommandId::MergeDown }).is_err());
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: upper.0, value: false } }).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: upper.0, value: false } }).unwrap();
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
        let upper = s.engine.document().active_layer;
        let mut clip = Layer::paint(s.engine.allocate_layer_id(), "Shade");
        clip.properties.clipped = true;
        s.layer_edit(Edit::InsertLayer { index: 0, layer: Box::new(clip) }).unwrap();
        s.frame(3, 3).unwrap();
        assert_eq!(s.command(CommandId::MergeDown).label.as_ref(), "Merge Clipped Layers");
        let mut effect = s.engine.document().layer(upper).unwrap().clone();
        effect.id = s.engine.allocate_layer_id();
        effect.kind = LayerKind::Effect;
        effect.effect = Some(std::sync::Arc::new(layer_core::EffectInstance::new(
            layer_core::bundled_effect_catalog().get("levels").unwrap().program(),
        )));
        let id = effect.id;
        s.layer_edit(Edit::InsertLayer { index: 0, layer: Box::new(effect) }).unwrap();
        s.layer_edit(Edit::SetActiveLayer { id }).unwrap();
        s.frame(4, 4).unwrap();
        let state = s.state.commands.iter().find(|c| c.id == CommandId::MergeDown).unwrap();
        assert_eq!(state.label.as_ref(), "Apply Effect to Layer Below");
        assert!(state.tooltip.starts_with("Apply Effect to Layer Below"));
    }

    #[test]
    fn flatten_asks_before_discarding_hidden_layers() {
        let mut s = stacked();
        let upper = s.engine.document().active_layer;
        invoke(&mut s, CommandId::FlattenImage);
        assert_eq!(s.engine.document().layers.len(), 2, "nothing hidden: flattens at once");
        assert!(s.engine.undo().unwrap());
        s.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: upper.0, value: false } }).unwrap();
        invoke(&mut s, CommandId::FlattenImage);
        let notice = s.state.notice.clone().expect("a confirmation");
        assert_eq!(notice.text, "Flattening discards 1 hidden layer");
        assert_eq!(notice.action.unwrap().label, "Flatten");
        assert_eq!(s.engine.document().layers.len(), 3, "nothing changes before it is accepted");
        s.dispatch(UiAction::Notice { id: notice.id, accept: true }).unwrap();
        assert_eq!(named(&s).len(), 2);
        assert!(s.engine.document().layer(upper).is_none(), "the hidden layer is discarded");
    }

    #[test]
    fn stamp_visible_and_merge_group_keep_their_structure() {
        let mut s = stacked();
        let layers = s.engine.document().layers.len();
        invoke(&mut s, CommandId::StampVisible);
        assert_eq!(s.engine.document().layers.len(), layers + 1);
        assert_eq!(named(&s)[0], "Visible");
        s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
        let group = s.engine.document().active_layer;
        assert!(s.command(CommandId::MergeGroup).enabled);
        invoke(&mut s, CommandId::MergeGroup);
        let result = s.engine.document().layer(s.engine.document().active_layer).unwrap();
        assert!(result.kind == LayerKind::Paint && s.engine.document().layer(group).is_none());
    }

    #[test]
    fn menus_offer_merges_with_their_live_state() {
        let s = stacked();
        let active = s.engine.document().active_layer.0;
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
        let lower = s.engine.document().layers[1].id.0;
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
