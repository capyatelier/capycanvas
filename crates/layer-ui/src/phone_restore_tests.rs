mod phone_restore_tests {
    use super::*;

    fn allocator(layout: &DockLayout) -> u32 {
        serde_json::to_value(layout).unwrap()["next_tile_id"].as_u64().unwrap() as u32
    }

    fn with_allocator(layout: &DockLayout, next: u32) -> DockLayout {
        let mut saved = serde_json::to_value(layout).unwrap();
        saved["next_tile_id"] = next.into();
        serde_json::from_value(saved).unwrap()
    }

    fn desktop(layout: &DockLayout) -> serde_json::Value {
        let mut saved = serde_json::to_value(layout).unwrap();
        saved.as_object_mut().unwrap().remove("phone");
        saved.as_object_mut().unwrap().remove("next_tile_id");
        saved
    }

    fn legacy_id_22() -> (DockLayout, DockLayout) {
        let starting = WorkspacePreset::Painter.layout(Platform::Gtk);
        assert_eq!(starting.phone.as_ref().unwrap()[0].id, 22);
        let mut current = starting.clone();
        current.phone = None;
        current = with_allocator(&current, 22);
        current.validate().unwrap();
        current.insert_tools(Panel::Toolbar, None, &[ToolbarControl::Color]).unwrap();
        assert_eq!(current.panel(Panel::Toolbar).unwrap().tiles().last().unwrap().id, 22);
        current.resolve_phone_defaults(WorkspacePreset::Painter).unwrap();
        current.validate().unwrap();
        (current, starting)
    }

    #[test]
    fn phone_restore_legacy_id_22_preserves_desktop_and_noncolliding_phone_ids() {
        let (mut current, mut starting) = legacy_id_22();
        current.header.size = HeaderSize::Large;
        current.bands[0].extent += 19.;
        starting.phone.as_mut().unwrap()[0].slider_style.show_value = true;
        let before = current.clone();
        assert!(current.starting_layout_available(&starting, true));
        let restored = current.starting_layout_candidate(&starting, true).unwrap();
        restored.validate().unwrap();
        assert_eq!(desktop(&restored), desktop(&current));
        assert_eq!(restored.panels, current.panels);
        for (actual, original) in restored.phone.as_ref().unwrap().iter().zip(starting.phone.as_ref().unwrap()) {
            assert_eq!(actual.control, original.control);
            assert_eq!(actual.slider_style, original.slider_style);
            if original.id == 22 { assert!(actual.id >= allocator(&current)); }
            else { assert_eq!(actual.id, original.id); }
        }
        assert!(!restored.starting_layout_available(&starting, true));
        assert_eq!(current, before);
    }

    #[test]
    fn phone_restore_desktop_keeps_starting_ids_and_remaps_only_retained_phone_collisions() {
        let (mut current, mut starting) = legacy_id_22();
        current.header.size = HeaderSize::Large;
        current.bands[0].extent += 19.;
        current.phone.as_mut().unwrap().last_mut().unwrap().slider_style.expand = true;
        starting.insert_tools(Panel::Toolbar, None, &[ToolbarControl::Command { command: CommandId::Undo }]).unwrap();
        let collision = starting.panel(Panel::Toolbar).unwrap().tiles().last().unwrap().id;
        assert_eq!(collision, current.phone.as_ref().unwrap().last().unwrap().id);
        assert!(current.starting_layout_available(&starting, false));
        let restored = current.starting_layout_candidate(&starting, false).unwrap();
        restored.validate().unwrap();
        assert_eq!(desktop(&restored), desktop(&starting));
        assert_eq!(restored.panels, starting.panels);
        for (actual, original) in restored.phone.as_ref().unwrap().iter().zip(current.phone.as_ref().unwrap()) {
            assert_eq!(actual.control, original.control);
            assert_eq!(actual.slider_style, original.slider_style);
            if original.id == collision { assert!(actual.id >= allocator(&current)); }
            else { assert_eq!(actual.id, original.id); }
        }
        assert!(!restored.starting_layout_available(&starting, false));
        assert_eq!(restored.starting_layout_candidate(&starting, false).unwrap(), restored);
    }

    #[test]
    fn phone_restore_allocator_preserves_both_high_water_marks() {
        for phone in [false, true] {
            let (current, starting) = legacy_id_22();
            let current = with_allocator(&current, 500);
            let starting = with_allocator(&starting, 900);
            let mut restored = current.starting_layout_candidate(&starting, phone).unwrap();
            assert!(allocator(&restored) >= 900);
            restored.insert_tools(Panel::Toolbar, None, &[ToolbarControl::Color]).unwrap();
            let desktop_id = restored.panel(Panel::Toolbar).unwrap().tiles().last().unwrap().id;
            restored.insert_phone_tools(None, &[ToolbarControl::Divider]).unwrap();
            let phone_id = restored.phone.as_ref().unwrap().last().unwrap().id;
            assert!(desktop_id >= 900 && phone_id > desktop_id);
            assert!(allocator(&restored) > phone_id);
            restored.validate().unwrap();
            assert_eq!(allocator(&current), 500);
            assert_eq!(allocator(&starting), 900);
        }
    }

    #[test]
    fn phone_restore_explicit_empty_phone_survives_both_scopes_and_reload() {
        for phone in [false, true] {
            let (mut current, mut starting) = legacy_id_22();
            current.header.size = HeaderSize::Large;
            if phone { starting.phone = Some(Vec::new()); }
            else { current.phone = Some(Vec::new()); }
            assert!(current.starting_layout_available(&starting, phone));
            let restored = current.starting_layout_candidate(&starting, phone).unwrap();
            assert_eq!(restored.phone, Some(Vec::new()));
            assert_eq!(desktop(&restored), desktop(if phone { &current } else { &starting }));
            restored.validate().unwrap();
            let reloaded: DockLayout = serde_json::from_str(&serde_json::to_string(&restored).unwrap()).unwrap();
            assert_eq!(reloaded.phone, Some(Vec::new()));
            assert!(!reloaded.starting_layout_available(&starting, phone));
        }
    }

    #[test]
    fn phone_restore_availability_ignores_phone_ids_and_allocator_only_changes() {
        let (current, starting) = legacy_id_22();
        assert!(!current.starting_layout_available(&starting, true));
        let mut s = phone_session([412, 891]);
        s.state.workspace.layout = current;
        s.configure_workspace_manager(ManagedWorkspace {
            id: "legacy-sketch".into(), name: "Legacy Sketch".into(), baseline: starting.clone(), choices: Vec::new(),
        }).unwrap();
        assert!(!s.command(CommandId::ResetLayout).enabled);
        let first = s.state.workspace.layout.phone.as_ref().unwrap()[0].id;
        s.state.workspace.layout.remove_phone_tool(first).unwrap();
        assert!(s.command(CommandId::ResetLayout).enabled);
        s.state.workspace.layout = s.state.workspace.layout.starting_layout_candidate(&starting, true).unwrap();
        assert!(!s.command(CommandId::ResetLayout).enabled);
    }

    #[test]
    fn phone_remove_and_move_each_record_one_exact_layout_history_step() {
        for moving in [false, true] {
            let mut s = phone_session([412, 891]);
            let original = s.state.workspace.layout.clone();
            let tile = original.phone.as_ref().unwrap()[0].id;
            let action = if moving { PhoneAction::MoveTool { tile, before: None } } else { PhoneAction::RemoveTool { tile } };
            s.dispatch(UiAction::Phone { action }).unwrap();
            let edited = s.state.workspace.layout.clone();
            assert_ne!(edited, original);
            s.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace }).unwrap();
            assert_eq!(s.state.workspace.layout, original);
            assert!(!s.workspace_history.can_undo());
            s.dispatch(UiAction::Invoke { command: CommandId::RedoWorkspace }).unwrap();
            assert_eq!(s.state.workspace.layout, edited);
            assert!(!s.workspace_history.can_redo());
        }
    }
}
