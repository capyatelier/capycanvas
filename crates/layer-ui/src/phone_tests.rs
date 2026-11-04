fn phone_session(viewport: [u32; 2]) -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.state.workspace.layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    s.set_viewport(viewport.map(|v| v as f32), viewport).unwrap();
    s
}

#[test]
fn phone_editor_pages_clip_to_the_visible_viewport_without_changing_modes() {
    let mut s = phone_session([412, 891]);
    s.state.phone.insets = [5., 7., 11., 13.];
    let visible = Bounds { x: 0., y: 0., width: 412., height: 480. };
    assert_eq!(s.state.dialog_presentation_in(DialogRole::Editor, [412., 891.], visible),
        DialogPresentation::Page(Bounds { x: 5., y: 7., width: 396., height: 473. }));
    s.set_viewport([1000., 800.], [1000, 800]).unwrap();
    assert_eq!(s.state.dialog_presentation_in(DialogRole::Editor, [1000., 800.], visible), DialogPresentation::Floating);
}

#[test]
fn phone_menu_search_escape_clears_then_closes_and_backs_out_of_parameters() {
    let mut s = phone_session([412, 891]);
    s.dispatch(UiAction::Invoke { command: CommandId::SearchCommands }).unwrap();
    s.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Query { text: "pencil".into() } }).unwrap();
    s.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Dismiss { search_focused: true, text: "pencil".into() } }).unwrap();
    assert_eq!(s.state.command_search.as_ref().unwrap().query, "");
    s.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Dismiss { search_focused: true, text: String::new() } }).unwrap();
    assert!(s.state.command_search.is_none());
    s.dispatch(UiAction::Invoke { command: CommandId::SearchCommands }).unwrap();
    s.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Query { text: "brush size".into() } }).unwrap();
    s.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Execute { id: "tool_setting.size".into(), value: None } }).unwrap();
    assert!(s.state.command_search.as_ref().unwrap().parameter.is_some());
    s.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Dismiss { search_focused: true, text: "24".into() } }).unwrap();
    assert!(s.state.command_search.as_ref().unwrap().parameter.is_none());
    assert_eq!(s.state.command_search.as_ref().unwrap().query, "brush size");
}

#[test]
fn phone_menu_shows_one_available_shortcut_per_row() {
    for platform in [Platform::Gtk, Platform::Web] {
        let mut s = session(platform);
        s.state.workspace.layout = WorkspacePreset::Painter.layout(platform);
        s.set_viewport([891., 412.], [891, 412]).unwrap();
        let labels = [DocumentTabLabel { id: 1, title: "Study".into(), location: String::new(), uri: None, export_uri: None, modified: false }];
        let selector = DrawingSelector::new(&labels, 1, &s.command(CommandId::NewDocument), s.localization());
        let mut menu = s.phone_menu().unwrap();
        let mut item = ContextMenuItem::command("Two shortcuts", UiAction::Invoke { command: CommandId::SoftProof });
        item.bindings = ["x", "y"].map(|key| KeyChord::new(key, Modifiers { command: true, ..Default::default() }).native_binding(platform)).to_vec();
        item.hint = "Ctrl+X / Ctrl+Y".into();
        menu.list.sections.push(vec![item]);
        let view = menu.view(&selector, platform, s.localization());
        for row in view.pages.iter().flat_map(|page| page.sections.iter().flatten()) {
            assert!(row.bindings.len() <= 1, "{platform:?}: {}", row.label);
            assert_eq!(row.hint, row.bindings.first().map(|binding| binding.chord.localized_label(platform, s.localization())).unwrap_or_default(), "{platform:?}: {}", row.label);
        }
        let row = view.pages.iter().flat_map(|page| page.sections.iter().flatten()).find(|row| row.label == "Two shortcuts").unwrap();
        assert_eq!(row.bindings.len(), 1);
        assert_eq!(row.hint, "Ctrl+X");
        let create = view.pages.iter().find(|page| page.id.page == Some(MenuPageId::Drawings)).unwrap().sections.last().unwrap().last().unwrap();
        assert_eq!(create.bindings, s.command(CommandId::NewDocument).bindings.into_iter().filter(|binding| binding.chord.available(platform)).take(1).collect::<Vec<_>>());
        if platform == Platform::Web { assert!(create.hint.is_empty()); }
    }
}

#[test]
fn phone_menu_projected_activation_closes_for_effects_and_rejects_stale_pages() {
    let mut s = phone_session([412, 891]);
    let labels = [DocumentTabLabel { id: 1, title: "Study".into(), location: String::new(), uri: None, export_uri: None, modified: false }];
    let selector = DrawingSelector::new(&labels, 1, &s.command(CommandId::NewDocument), s.localization());
    let view = s.phone_menu().unwrap().view(&selector, s.state.platform, s.localization());
    let (route, action) = view.pages.iter().find_map(|page| page.sections.iter().flatten().find_map(|row| {
        match &row.action {
            Some(action @ UiAction::Phone { action: PhoneAction::MenuActivate { action: inner, .. } })
                if row.enabled && matches!(&**inner, UiAction::Effect { action: EffectAction::Insert { .. } }) => Some((page.id.clone(), action.clone())),
            _ => None,
        }
    })).expect("an enabled filter menu command");
    s.dispatch(UiAction::Phone { action: PhoneAction::Capy }).unwrap();
    let revision = s.engine.document().revision;
    s.dispatch(action.clone()).unwrap();
    assert_eq!(s.engine.document().revision, revision, "a retired page cannot activate a row");
    assert!(s.state.phone.menu);
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuActivate { route: PhoneMenuRoute::default(), action: Box::new(UiAction::Phone { action: PhoneAction::SwapSides }) } }).unwrap();
    assert!(s.state.phone.menu); assert!(s.state.phone.swapped);
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuSelect { route } }).unwrap();
    s.dispatch(action).unwrap();
    assert!(s.engine.document().revision > revision);
    assert!(!s.state.phone.menu); assert!(s.state.phone.menu_navigation.is_root());
}

#[test]
fn phone_menu_route_stack_is_session_owned_validated_and_cleared() {
    let mut s = phone_session([412, 891]);
    s.configure_workspace_manager(ManagedWorkspace { id: "paint".into(), name: "Paint".into(),
        baseline: s.state.workspace.layout.clone(), choices: Vec::new() }).unwrap();
    let route = |page| PhoneMenuRoute { page: Some(page), path: Vec::new() };
    let workspaces = route(MenuPageId::Workspaces);
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuOpen { page: MenuPageId::Workspaces } }).unwrap();
    assert!(s.state.phone.menu); assert_eq!(s.state.phone.menu_navigation.current(), workspaces);
    assert_eq!(s.state.phone.menu_navigation.parent(), Some(route(MenuPageId::Application(ApplicationMenu::Window))));
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuBack { route: workspaces.clone() } }).unwrap();
    assert_eq!(s.state.phone.menu_navigation.current(), route(MenuPageId::Application(ApplicationMenu::Window)));
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuBack { route: workspaces } }).unwrap();
    assert_eq!(s.state.phone.menu_navigation.current(), route(MenuPageId::Application(ApplicationMenu::Window)), "a stale swipe cannot back out of another page");
    let invalid = PhoneMenuRoute { page: Some(MenuPageId::Application(ApplicationMenu::File)), path: vec![[99, 99]] };
    assert!(s.dispatch(UiAction::Phone { action: PhoneAction::MenuSelect { route: invalid } }).is_err());
    s.dispatch(UiAction::Phone { action: PhoneAction::Capy }).unwrap();
    assert!(!s.state.phone.menu); assert!(s.state.phone.menu_navigation.is_root());
    s.dispatch(UiAction::Invoke { command: CommandId::Drawings }).unwrap();
    assert!(s.state.phone.menu); assert_eq!(s.state.phone.menu_navigation.current(), route(MenuPageId::Drawings));
    assert!(!s.state.requests.iter().any(|request| matches!(request.kind, HostRequestKind::Drawings)));
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuSelect { route: route(MenuPageId::Application(ApplicationMenu::Window)) } }).unwrap();
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuSelect { route: route(MenuPageId::Workspaces) } }).unwrap();
    s.managed_workspace = None;
    s.dispatch(UiAction::Phone { action: PhoneAction::SwapSides }).unwrap();
    assert_eq!(s.state.phone.menu_navigation.current(), route(MenuPageId::Application(ApplicationMenu::Window)), "removed menu content returns to its surviving ancestor");
    let serialized = serde_json::to_value(&s.state.phone).unwrap();
    assert_eq!(serialized["menu_route"], serde_json::to_value(s.state.phone.menu_navigation.current()).unwrap());
    assert!(serialized.get("stack").is_none());
    s.dispatch(UiAction::Phone { action: PhoneAction::Panel { panel: Panel::Layers } }).unwrap();
    assert!(!s.state.phone.menu); assert!(s.state.phone.menu_navigation.is_root());
}

#[test]
fn phone_menu_projects_typed_pages_and_close_slots_without_reading_titles() {
    let mut s = phone_session([412, 891]);
    s.configure_workspace_manager(ManagedWorkspace {
        id: "paint".into(), name: "Same".into(), baseline: s.state.workspace.layout.clone(), choices: Vec::new(),
    }).unwrap();
    let labels = [1, 2].map(|id| DocumentTabLabel { id, title: format!("Study {{雪}} {id}"), location: String::new(), uri: None, export_uri: None, modified: id == 2 });
    let selector = DrawingSelector::new(&labels, 2, &s.command(CommandId::NewDocument), s.localization());
    let mut menu = s.phone_menu().unwrap();
    fn duplicate_titles(sections: &mut [Vec<ContextMenuItem>]) {
        for item in sections.iter_mut().flatten() { item.label = "Same".into(); duplicate_titles(&mut item.sections); }
    }
    duplicate_titles(&mut menu.list.sections);
    let view = menu.view(&selector, s.state.platform, s.localization());
    let named = |id| view.pages.iter().find(|page| page.id.page == Some(id) && page.id.path.is_empty()).unwrap();
    let root = &view.pages[0];
    assert_eq!(root.id, PhoneMenuRoute::default()); assert!(!root.checks);
    assert_eq!(root.sections[0][0].label, "• Study {雪} 2");
    assert_eq!(root.sections[0][0].count, "2");
    assert_eq!(root.sections[0][0].target.as_ref().unwrap().page, Some(MenuPageId::Drawings));
    let drawings = named(MenuPageId::Drawings);
    assert_eq!(drawings.parent.as_ref(), Some(&root.id)); assert!(drawings.checks);
    for (row, id) in drawings.sections[0].iter().zip([1, 2]) {
        let close = row.close.as_ref().unwrap();
        assert_eq!(close.action, UiAction::Phone { action: PhoneAction::Drawing { id, close: true } });
        assert!(close.label.contains(&format!("Study {{雪}} {id}")));
        assert!(row.hint.is_empty());
    }
    assert_eq!(named(MenuPageId::Workspaces).parent.as_ref(), Some(&named(MenuPageId::Application(ApplicationMenu::Window)).id));
    assert!(root.sections.iter().flatten().filter(|row| row.target.is_some()).all(|row| !row.is_heading));
}

#[test]
fn phone_color_columns_share_one_scroller_in_both_orientations() {
    for viewport in [[412, 891], [891, 412]] {
        let mut s = phone_session(viewport);
        let tile = s.state.workspace.layout.phone.as_ref().unwrap().iter().find(|tile| tile.control == ToolbarControl::Color).unwrap().id;
        s.dispatch(UiAction::Phone { action: PhoneAction::Tool { tile } }).unwrap();
        let mut drawer = s.state.customization.drawer.clone().unwrap();
        assert!(drawer.scrolls_together());
        drawer.anchor = DrawerAnchor::Header { id: 1 };
        assert!(!drawer.scrolls_together());
        s.dispatch(UiAction::Phone { action: PhoneAction::Panel { panel: Panel::Layers } }).unwrap();
        assert!(!s.state.customization.drawer.as_ref().unwrap().scrolls_together());
    }
}

#[test]
fn drawer_body_minimums_preserve_scroll_bodies_without_overriding_the_layers_inspector() {
    let mut s = phone_session([412, 891]);
    s.dispatch(UiAction::Phone { action: PhoneAction::Panel { panel: Panel::Layers } }).unwrap();
    let mut drawer = s.state.customization.drawer.clone().unwrap();
    assert_eq!(drawer.body_minimum_height(Panel::Layers), 0.);
    for panel in [Panel::Adjustments, Panel::FilterTypes] { assert_eq!(drawer.body_minimum_height(panel), 440.); }
    for panel in [Panel::Color, Panel::Tools, Panel::Palettes] { assert_eq!(drawer.body_minimum_height(panel), 0.); }
    drawer.layer_properties = None;
    assert_eq!(drawer.body_minimum_height(Panel::Layers), 364.);
}

#[test]
fn tool_panel_presentation_splits_sets_once_and_keeps_selection_rows_short() {
    for viewport in [[412, 891], [891, 412]] {
        let mut s = phone_session(viewport);
        invoke(&mut s, CommandId::DrawingBrush);
        let tile = s.state.workspace.layout.phone.as_ref().unwrap().iter()
            .find(|tile| tile.control == (ToolbarControl::Command { command: CommandId::DrawingBrush })).unwrap().id;
        s.dispatch(UiAction::Phone { action: PhoneAction::Tool { tile } }).unwrap();
        let drawer = s.state.customization.drawer.as_ref().unwrap();
        if viewport[0] > viewport[1] {
            let sets = s.state.tool_panel_presentation(Panel::BrushSets, Some(drawer));
            assert!(sets.groups_are_rows && !sets.groups.is_empty() && sets.subtools.is_empty());
            assert_eq!(sets.group_height, 44.); assert_eq!(sets.gap, 6.);
            let tools = s.state.tool_panel_presentation(Panel::Tools, Some(drawer));
            assert!(tools.groups.is_empty() && !tools.subtools.is_empty());
        } else {
            let tools = s.state.tool_panel_presentation(Panel::Brushes, Some(drawer));
            assert!(!tools.groups_are_rows && !tools.groups.is_empty() && !tools.subtools.is_empty());
            assert_eq!(tools.group_height, 26.);
        }
        invoke(&mut s, CommandId::Select);
        let view = s.state.tool_panel_presentation(Panel::Tools, None);
        assert!(view.subtools.iter().any(|item| matches!(item.action, UiAction::ChooseToolVariant { variant: ToolVariant::Command { command: CommandId::TonalSelect }, .. })));
        assert_eq!(view.plain_height, 44.);
        let mut commands = s.state.tool_panel(Panel::Tools).clone();
        commands.subtools[0].action = UiAction::Invoke { command: CommandId::TonalSelect };
        assert_eq!(commands.presentation(Panel::Tools, false).plain_height, 32.);
    }
}

#[test]
fn phone_metrics_are_host_reports_while_rendering_is_suspended() {
    for platform in [Platform::Gtk, Platform::Web] {
        let mut s = session(platform);
        s.set_viewport([412., 891.], [412, 891]).unwrap();
        s.suspend_renderer().unwrap();
        assert!(s.dispatch(UiAction::Phone { action: PhoneAction::SwapSides }).is_err());
        let action = UiAction::Phone { action: PhoneAction::Metrics {
            native: [32., 16.], native_counts: [1, 1], insets: [19., 7., 13., 5.], movable: true,
        } };
        assert!(action.is_host_report());
        s.dispatch(action).unwrap();
        assert_eq!(s.state.phone.insets, [19., 7., 13., 5.]);
        assert_eq!(s.state.phone.native, [32., 16.]);
        assert_eq!(s.state.phone.native_counts, [1, 1]);
        assert!(s.state.phone.movable);
        assert!(!s.state.phone.swapped);
    }
}

#[test]
fn document_interaction_admits_only_existing_reports_and_phone_metrics() {
    let context = CanvasBarContext { generation: 1, kind: CanvasBarKind::Selection };
    let allowed = [
        UiAction::PenInput { device: PenDeviceInfo { id: 1, key: "pen".into(), label: "Pen".into(), native_to_surface: layer_core::TiltAxes::quarter_turns(0) }, sample: None },
        UiAction::SystemMotionChanged { reduced: false },
        UiAction::RestoreSavedSettings { saved: String::new() },
        UiAction::CompleteRequest { id: 1, error: None },
        UiAction::MeasurePanels { measurements: PanelMeasurementBatch { context: PanelMetricContext {
            generation: 1, scale: 1., layout_grid: 1., text_scale: 1., style: "light".into(),
        }, reports: Vec::new() } },
        UiAction::MeasureCanvasBar { measure: CanvasBarMeasure { context, label: 0., items: Vec::new(), completion: Vec::new(), more: 0., height: 0., gap: 0., padding: 0. } },
        UiAction::RevealCanvasBar { context, hold: 1 },
        UiAction::MeasureTitlebar { insets: [0.; 3] },
        UiAction::MeasureWorkspaceBottom { inset: 0. },
        UiAction::MeasureColumnDrawers { measurements: Vec::new() },
        UiAction::MeasureDrawerTiles { measurements: Vec::new() },
        UiAction::MeasureColumnScroll { column: 1, offset: 0. },
        UiAction::Phone { action: PhoneAction::Metrics { native: [0.; 2], native_counts: [0; 2], insets: [0.; 4], movable: false } },
    ];
    for action in allowed { assert!(action.allowed_during_document_interaction(), "{action:?}"); }
    for action in [
        PhoneAction::Capy, PhoneAction::CapyHold, PhoneAction::CapyDrop { position: [0.; 2] },
        PhoneAction::Tool { tile: 1 }, PhoneAction::RemoveTool { tile: 1 },
        PhoneAction::InsertTools { before: None }, PhoneAction::MoveTool { tile: 1, before: None },
        PhoneAction::ToolSettings, PhoneAction::Panel { panel: Panel::Layers },
        PhoneAction::Drawing { id: 1, close: false }, PhoneAction::SwapSides,
        PhoneAction::Slider { control: ToolbarControl::BrushSizeSlider, phase: ContactPhase::Down },
    ] {
        let action = UiAction::Phone { action };
        assert!(!action.is_host_report(), "{action:?}");
        assert!(!action.allowed_during_document_interaction(), "{action:?}");
    }
    for action in [
        UiAction::Invoke { command: CommandId::Undo },
        UiAction::MeasureHeader { height: 0., items: Vec::new() },
        UiAction::SystemThemeChanged { theme: Theme::Light, accent: None },
        UiAction::WindowFullscreen { fullscreen: false },
        UiAction::RestoreSettings { settings: Box::default() }, UiAction::CloseSettings,
    ] { assert!(!action.allowed_during_document_interaction(), "{action:?}"); }
}

#[test]
fn phone_movable_metrics_update_the_title_without_changing_saved_state() {
    let mut s = phone_session([891, 412]);
    let workspace = s.capture_workspace().unwrap();
    let settings = s.state.settings.clone();
    assert!(!serde_json::from_str::<PhoneState>("{}").unwrap().movable);
    let legacy: PhoneAction = serde_json::from_value(serde_json::json!({
        "kind": "metrics", "native": [0., 0.], "native_counts": [0, 0], "insets": [0., 0., 0., 0.],
    })).unwrap();
    assert!(matches!(legacy, PhoneAction::Metrics { movable: false, .. }));
    assert_eq!(s.dispatch(UiAction::Phone { action: legacy }).unwrap().regions, 0);
    for movable in [true, false] {
        let action = PhoneAction::Metrics { native: [0.; 2], native_counts: [0; 2], insets: [0.; 4], movable };
        let change = s.dispatch(UiAction::Phone { action: action.clone() }).unwrap();
        assert_ne!(change.regions & regions::LAYOUT, 0);
        assert_eq!(s.state.phone.movable, movable);
        assert_eq!(s.phone_layout().unwrap().title.width > 0., movable);
        assert_eq!(s.dispatch(UiAction::Phone { action }).unwrap().regions, 0);
    }
    assert_eq!(s.capture_workspace().unwrap(), workspace);
    assert_eq!(s.state.settings, settings);
    assert!(s.state.requests.is_empty());
}

#[test]
fn phone_menu_toggles_keep_the_menu_open_and_zen_closes_it() {
    let mut s = phone_session([412, 891]);
    s.dispatch(UiAction::Phone { action: PhoneAction::Capy }).unwrap();
    for _ in 0..2 {
        let first = s.phone_menu().unwrap().strip;
        for item in [&first[2], &first[4]] {
            assert!(item.keep_open());
            assert_eq!(serde_json::to_value(item).unwrap()["keep_open"], true);
            s.dispatch(item.action.clone().unwrap()).unwrap();
            assert!(s.state.phone.menu);
        }
    }
    assert!(!s.finger_drawing() && !s.state.phone.swapped);
    let zen = s.phone_menu().unwrap().strip[3].clone();
    assert!(!zen.keep_open());
    s.dispatch(zen.action.unwrap()).unwrap();
    assert!(s.state.phone.hidden && !s.state.phone.menu);
}

#[test]
fn drawing_selector_preserves_order_current_dirty_text_and_new_action() {
    let mut s = phone_session([412, 891]);
    let labels = [1, 2].map(|id| DocumentTabLabel { id, title: format!("Study {id}"), location: String::new(), uri: None, export_uri: None, modified: id == 2 });
    let selector = DrawingSelector::new(&labels, 2, &s.command(CommandId::NewDocument), s.localization());
    assert_eq!(selector.label, "• Study 2"); assert_eq!(selector.count, "2");
    let rows = &selector.page.sections[0];
    assert_eq!(rows.iter().map(|row| row.label.as_str()).collect::<Vec<_>>(), ["Study 1", "• Study 2"]);
    assert_eq!(rows.iter().map(|row| row.selected).collect::<Vec<_>>(), [Some(false), Some(true)]);
    for (id, row) in [1, 2].into_iter().zip(rows) {
        assert!(matches!(row.action, Some(UiAction::Phone { action: PhoneAction::Drawing { id: actual, close: false } }) if actual == id));
        assert!(row.hint.contains(&format!("Study {id}")));
    }
    let create = &selector.page.sections.last().unwrap()[0];
    assert!(matches!(create.action, Some(UiAction::Invoke { command: CommandId::NewDocument })));
    assert_eq!(create.bindings, s.command(CommandId::NewDocument).bindings);
    assert_eq!(create.hint, s.command(CommandId::NewDocument).shortcut);
    assert!(DrawingSelector::new(&labels[..1], 1, &s.command(CommandId::NewDocument), s.localization()).count.is_empty());
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuOpen { page: MenuPageId::Drawings } }).unwrap();
    assert_eq!(s.state.phone.menu_navigation.current().page, Some(MenuPageId::Drawings));
    let route = s.state.phone.menu_navigation.current();
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuBack { route } }).unwrap();
    assert!(s.state.phone.menu_navigation.is_root());
    s.dispatch(UiAction::Phone { action: PhoneAction::Drawing { id: 1, close: true } }).unwrap();
    assert!(s.state.requests.iter().any(|r| matches!(r.kind, HostRequestKind::SelectDrawing { id: 1, close: true })));
}

#[test]
fn phone_menu_command_closure_notifies_layout_and_refusals_keep_it_open() {
    let mut s = phone_session([412, 891]);
    let layout = s.state.workspace.layout.clone();
    s.configure_workspace_manager(ManagedWorkspace {
        id: "sketch".into(), name: "Sketch".into(), baseline: layout, choices: Vec::new(),
    }).unwrap();
    for command in [CommandId::ActualPixels, CommandId::ZoomIn] {
        s.dispatch(UiAction::Invoke { command }).unwrap();
    }
    s.dispatch(UiAction::Phone { action: PhoneAction::Capy }).unwrap();
    let change = s.dispatch(UiAction::Invoke { command: CommandId::ZoomIn }).unwrap();
    assert!(!s.state.phone.menu);
    assert_eq!(change.regions & (regions::LAYOUT | regions::CUSTOMIZATION), regions::LAYOUT | regions::CUSTOMIZATION);
    assert_ne!(change.regions & regions::CAMERA, 0);
    s.dispatch(UiAction::Phone { action: PhoneAction::Capy }).unwrap();
    assert!(s.dispatch(UiAction::Invoke { command: CommandId::Undo }).is_err());
    assert!(s.state.phone.menu);
    let change = s.dispatch(UiAction::WorkspaceManager { command: WorkspaceCommand::Switch { id: "sketch".into() } }).unwrap();
    assert!(!s.state.phone.menu);
    assert_eq!(change.regions & (regions::LAYOUT | regions::CUSTOMIZATION), regions::LAYOUT | regions::CUSTOMIZATION);
    assert_eq!(change.regions & regions::HOST, 0);
}

#[test]
fn phone_menu_keeps_its_bridge_and_dismisses_only_outside_contacts() {
    for viewport in [[412, 891], [891, 412], [320, 568]] {
        for swapped in [false, true] {
            let mut s = phone_session(viewport);
            s.state.phone.swapped = swapped;
            s.dispatch(UiAction::Phone { action: PhoneAction::Capy }).unwrap();
            let placement = s.phone_layout().unwrap().menu(viewport.map(|v| v as f32), 242., 500., 32.).unwrap();
            let bridge = placement.connection().unwrap().bounds;
            let facts = ChromeFacts { content_drawer: Some(placement.bounds), drawer_connection: Some(bridge), ..Default::default() };
            let checkpoint = s.engine().checkpoint();
            for (position, open) in [([bridge.x + bridge.width * 0.5, bridge.y + bridge.height * 0.5], true), ([0.; 2], false)] {
                let reply = s.input(UiInput::Chrome { event: ChromeEvent::Contact { position, canvas: true }, facts,
                    viewport: viewport.map(|v| v as f32) }).unwrap();
                assert_eq!(s.state.phone.menu, open);
                assert_eq!(s.engine().checkpoint(), checkpoint);
                if !open { assert!(reply.handled); }
            }
        }
    }
}

#[test]
fn capy_caption_uses_the_presented_viewport_before_session_resize() {
    let mut s = phone_session([1000, 900]);
    let menu = s.localization().text(MessageId::MENU_MAIN_MENU);
    let capy = s.localization().text(MessageId::WORKSPACE_HEADER_CAPY);
    assert_eq!(s.capy_caption([412., 891.]), menu);
    s.set_viewport([412., 891.], [412, 891]).unwrap();
    assert_eq!(s.capy_caption([1000., 900.]), capy);
    s.state.phone.hidden = true;
    assert_eq!(s.capy_caption([412., 891.]), capy);
}

#[test]
fn retained_presentation_borrows_published_measurements_without_requesting_them_again() {
    let viewport = [1600., 1000.];
    let mut s = phone_session(viewport.map(|v| v as u32));
    s.dispatch(UiAction::MeasurePanels { measurements: measured_batch(&s, viewport, vec![PanelMeasurement {
        panel: Panel::Layers, tab_widths: [140., 40.], content_height: 700., scroll: None,
    }]) }).unwrap();
    let input = s.layout_presentation_input();
    assert!(std::ptr::eq(&*input, &s.state.workspace.layout));
    assert!(!input.measurements.is_empty());
    let resolved = input.resolve(viewport);
    assert!(resolved.phone.is_none());
    assert!(resolved.measurement_requests.is_empty());
    let mut measured = s.layout(viewport);
    assert!(!measured.measurement_requests.is_empty());
    measured.measurement_requests.clear();
    assert_eq!(serde_json::to_value(&resolved).unwrap(), serde_json::to_value(measured).unwrap());
    let retained: LayoutResolveInput<'static> = serde_json::from_value(serde_json::to_value(input.into_owned()).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(retained.resolve(viewport)).unwrap(), serde_json::to_value(resolved).unwrap());
}

#[test]
fn phone_transitions_cancel_chrome_without_recording_layout_or_zen() {
    let mut s = phone_session([480, 891]);
    s.state.workspace.zen_mode = true;
    s.state.settings.phone_swap_sides = true;
    s.state.phone.swapped = true;
    s.state.phone.hidden = true;
    let before = s.capture_workspace().unwrap();
    for viewport in [[479, 891], [891, 479], [480, 891], [412, 891]] {
        let slider = if s.phone_layout().is_some() {
            let size = s.state.brush.diameter;
            s.dispatch(UiAction::Phone { action: PhoneAction::Slider {
                control: ToolbarControl::BrushSizeSlider, phase: ContactPhase::Down,
            } }).unwrap();
            let context = s.state.toolbar_context();
            s.dispatch(UiAction::ToolbarEdit { context, action: Box::new(
                ToolbarControl::BrushSizeSlider.slider().unwrap().action(size + 10.),
            ) }).unwrap();
            assert_ne!(s.state.brush.diameter, size);
            s.dispatch(UiAction::Phone { action: PhoneAction::CapyHold }).unwrap();
            assert!(s.state.phone.capy_drag);
            Some((size, context))
        } else { None };
        s.workspace_history.begin(&s.state.workspace);
        s.state.workspace.layout.header.size = HeaderSize::Large;
        s.state.phone.menu = true;
        s.state.customization.drawer = Some(ContentDrawer::for_control(
            ToolbarControl::Color, DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::Capy },
        ).unwrap());
        s.set_viewport(viewport.map(|v| v as f32), viewport).unwrap();
        assert!(s.workspace_history.gesture_start().is_none());
        assert!(s.state.customization.drawer.is_none());
        assert!(!s.state.phone.menu);
        assert!(s.state.phone.hidden);
        assert!(s.state.phone.swapped);
        assert!(!s.state.phone.capy_drag);
        s.dispatch(UiAction::Phone { action: PhoneAction::CapyDrop { position: [0., 0.] } }).unwrap();
        assert!(s.state.settings.phone_swap_sides);
        assert!(s.state.workspace.zen_mode);
        assert_eq!(s.phone_layout().is_some(), viewport[0].min(viewport[1]) < 480);
        if let Some((size, context)) = slider {
            assert_eq!(s.state.brush.diameter, size);
            assert!(s.state.phone.slider.is_none());
            assert!(s.dispatch(UiAction::ToolbarEdit { context, action: Box::new(
                ToolbarControl::BrushSizeSlider.slider().unwrap().action(size + 20.),
            ) }).is_err());
            assert_eq!(s.state.brush.diameter, size);
        }
        assert_eq!(s.capture_workspace().unwrap(), before);
    }
    let mut ink = phone_session([412, 891]);
    ink.dispatch(UiAction::SetToolSetting { id: "smoothing_strength".into(), value: 0. }).unwrap();
    ink.frame(0, 0).unwrap();
    ink.pen(event(&ink, 1, PenPhase::Down, 0.5));
    ink.frame(10_000_000, 18_000_000).unwrap();
    assert!(ink.engine.has_active_stroke());
    ink.set_viewport([891., 412.], [891, 412]).unwrap();
    assert!(ink.engine.has_active_stroke());
    ink.pen(event(&ink, 2, PenPhase::Move, 0.7));
    ink.frame(20_000_000, 28_000_000).unwrap();
    ink.pen(event(&ink, 3, PenPhase::Up, 0.7));
    ink.frame(30_000_000, 38_000_000).unwrap();
    assert!(!ink.engine.has_active_stroke());
    assert!(ink.command(CommandId::Undo).enabled);
}

#[test]
fn phone_drawers_stay_inside_the_window_during_resize() {
    let layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    for short in std::iter::once(317).chain(280..480) {
        for viewport in [[short as f32, 568.], [568., short as f32]] {
            for swapped in [false, true] {
                let phone = PhoneLayout::resolve(&layout, viewport, [0.; 4], [0.; 2], [0; 2], swapped, false).unwrap();
                let drawer = phone.drawer(phone.tools[0], viewport, &[160., 240.], &[360., 240.], 8.).unwrap();
                let menu = phone.menu(viewport, 242., 500., 32.).unwrap();
                for bounds in [drawer.bounds, menu.bounds] {
                    assert!(bounds.x >= 0. && bounds.y >= 0., "{viewport:?}/{swapped}: {bounds:?}");
                    assert!(bounds.x + bounds.width <= viewport[0] + 0.001, "{viewport:?}/{swapped}: {bounds:?}");
                    assert!(bounds.y + bounds.height <= viewport[1] + 0.001, "{viewport:?}/{swapped}: {bounds:?}");
                }
            }
        }
    }
}

#[test]
fn phone_color_drawer_budgets_the_whole_wheel_before_scrolling() {
    for viewport in [[320, 568], [320, 320], [412, 891], [891, 412], [412, 320]] {
        let mut s = phone_session(viewport);
        s.dispatch(UiAction::Phone { action: PhoneAction::Panel { panel: Panel::Color } }).unwrap();
        let drawer = s.state.customization.drawer.as_ref().unwrap();
        let measure = s.state.customization.drawer_measure(drawer, &s.layout_resolve_input(), viewport.map(|v| v as f32)).unwrap();
        for hdr in [false, true] {
            let fit = measure.color_fit(&[0., 240.], 16., hdr).unwrap();
            let phone = s.phone_layout().unwrap();
            let stacked = measure.placement.columns[1].y > measure.placement.columns[0].y;
            let available = if phone.edge == Edge::Top { phone.work_area.height } else { phone.bars[0].height };
            let budget = available - if stacked { 248. } else { 0. };
            let minimum = ColorPanelLayout::stage_for_wheel(128.).unwrap().ceil();
            let width = measure.placement.columns[0].width - 16.;
            let expected = (minimum as u32..=width.floor() as u32).rev().find(|stage| {
                let layout = if hdr { ColorPanelLayout::with_hdr(*stage as f32) } else { ColorPanelLayout::new(*stage as f32) }.unwrap();
                layout.height().max(*stage as f32) <= budget - 16.
            }).map_or(minimum, |size| size as f32);
            assert_eq!(fit.size, expected, "{viewport:?}/{hdr}");
            assert!(fit.size <= width && fit.layout.height() <= fit.height);
            assert!(fit.layout.wheel[2] >= 128.);
            let p = measure.placement(&[fit.height + 16., 240.]).unwrap();
            assert_eq!(p.columns[0].height, fit.height + 16.);
            let natural = p.columns.iter().map(|b| b.y + b.height).fold(0., f32::max);
            assert_eq!(natural > p.bounds.height + 0.01, [ [320, 320], [412, 320] ].contains(&viewport));
            if natural > p.bounds.height + 0.01 { assert_eq!(fit.size, minimum); }
        }
    }
}

#[test]
fn phone_brush_sets_and_tools_share_a_row_without_settings() {
    let s = phone_session([320, 568]);
    let tile = s.state.workspace.layout.phone.as_ref().unwrap().iter().find(|t| {
        t.control == ToolbarControl::Command { command: CommandId::DrawingBrush }
    }).unwrap().id;
    let drawer = ContentDrawer::for_control(
        ToolbarControl::Command { command: CommandId::DrawingBrush },
        DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::Tool(tile) },
    ).unwrap();
    let p = s.state.customization.drawer_placement(
        &drawer, &s.layout_resolve_input(), [320., 568.], &[160., 240.],
    ).unwrap();
    assert_eq!(p.columns.len(), 2);
    assert_eq!(p.bounds.x, s.phone_layout().unwrap().bars[0].x);
    assert_eq!(p.bounds.width, s.phone_layout().unwrap().bars[0].width);
    assert_eq!(p.columns[0].y, p.columns[1].y);
    assert!(p.columns[1].x >= p.columns[0].x + p.columns[0].width);
    assert!(p.columns.iter().all(|c| c.x + c.width <= p.bounds.width + 0.01));
    assert!(p.connection().is_some());
}

#[test]
fn phone_tool_drawers_adapt_sets_and_keep_their_origin_when_a_set_changes() {
    let mut portrait_views = [None, None];
    for viewport in [[412, 891], [891, 412]] {
        for control in [ToolbarControl::Command { command: CommandId::DrawingBrush },
            ToolbarControl::Command { command: CommandId::Sculpt }, ToolbarControl::ToolSlot { slot: ToolSlotId::Blend }] {
            let landscape = viewport[0] > viewport[1];
            if matches!(control, ToolbarControl::ToolSlot { .. }) && !landscape { continue; }
            let mut s = phone_session(viewport);
            if matches!(control, ToolbarControl::ToolSlot { .. }) {
                let first = s.state.workspace.layout.phone.as_ref().unwrap()[0].id;
                s.state.workspace.layout.remove_phone_tool(first).unwrap();
                s.state.workspace.layout.insert_phone_tools(None, &[control]).unwrap();
            }
            let tile = s.state.workspace.layout.phone.as_ref().unwrap().iter().find(|tile| tile.control == control).unwrap().id;
            let anchor = DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::Tool(tile) };
            match control {
                ToolbarControl::Command { command } => { invoke(&mut s, command); },
                ToolbarControl::ToolSlot { slot } => { s.dispatch(slot_choice(&s, anchor, slot.variants()[0])).unwrap(); },
                _ => unreachable!(),
            }
            s.dispatch(UiAction::Phone { action: PhoneAction::Tool { tile } }).unwrap();
            let drawer = s.state.customization.drawer.as_ref().unwrap();
            let sets = if control == (ToolbarControl::Command { command: CommandId::Sculpt }) { Panel::SculptSets } else { Panel::BrushSets };
            let columns = if landscape { vec![vec![sets], vec![Panel::Tools]] } else { vec![vec![Panel::Brushes]] };
            assert_eq!(drawer.anchor, anchor);
            assert_eq!(drawer.columns, columns, "{viewport:?}/{control:?}");
            assert_eq!(drawer.sets_column, landscape);
            assert_eq!(serde_json::to_value(drawer).unwrap().get("sets_column").and_then(|value| value.as_bool()), landscape.then_some(true));
            let view = drawer.tool_set.as_ref().unwrap().clone();
            assert!(!view.subtools.is_empty());
            if let ToolbarControl::Command { command } = control {
                let portrait = &mut portrait_views[usize::from(command == CommandId::Sculpt)];
                if landscape { assert_eq!(portrait.as_ref(), Some(&view)); } else { *portrait = Some(view.clone()); }
            }
            let labels = view.groups.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>();
            match control {
                ToolbarControl::Command { command: CommandId::Sculpt } => assert_eq!(labels, ["Blend", "Paint mixing", "Effects", "Liquify", "Retouch"]),
                ToolbarControl::ToolSlot { .. } => assert_eq!(labels, ["Blend", "Paint mixing", "Effects"]),
                _ => assert_eq!(view.groups, s.state.tool_panel(Panel::BrushSets).groups),
            }
            let action = view.groups.iter().find(|item| !item.selected).unwrap().action.clone();
            s.dispatch(action.clone()).unwrap();
            let drawer = s.state.customization.drawer.as_ref().unwrap();
            assert_eq!(drawer.anchor, anchor);
            assert_eq!(drawer.columns, columns);
            assert_eq!(drawer.sets_column, landscape);
            let updated = drawer.tool_set.as_ref().unwrap();
            assert!(updated.groups.iter().any(|item| item.selected && item.action == action));
            assert_ne!(updated.subtools.iter().map(|item| &item.action).collect::<Vec<_>>(),
                view.subtools.iter().map(|item| &item.action).collect::<Vec<_>>());
            assert_eq!(updated.groups.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>(), labels);
        }
    }
}

#[test]
fn phone_paint_defaults_offer_sculpt_and_preserve_an_authored_blend_tile() {
    let sculpt = ToolbarControl::Command { command: CommandId::Sculpt };
    let blend = ToolbarControl::ToolSlot { slot: ToolSlotId::Blend };
    let starting = WorkspacePreset::Illustrator.layout(Platform::Gtk);
    let controls = WorkspacePreset::Illustrator.phone_controls();
    assert!(controls.contains(&sculpt));
    assert!(!controls.contains(&blend));
    assert!(WorkspacePreset::Photographer.phone_controls().contains(&ToolbarControl::ToolSlot { slot: ToolSlotId::Healing }));
    assert!(starting.panel(Panel::Toolbar).unwrap().tiles().iter().any(|tile| tile.control == blend));
    let mut saved = starting.clone();
    saved.phone.as_mut().unwrap().iter_mut().find(|tile| tile.control == sculpt).unwrap().control = blend;
    let mut restored: DockLayout = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
    restored.resolve_phone_defaults(WorkspacePreset::Illustrator).unwrap();
    assert_eq!(restored, saved);
    let reset = restored.starting_layout_candidate(&starting, true).unwrap();
    assert_eq!(reset.phone.as_ref().unwrap().iter().map(|tile| tile.control).collect::<Vec<_>>(), controls);
    assert_eq!(reset.panels, saved.panels);
}

#[test]
fn phone_choice_only_and_leaf_drawers_publish_choices_as_subtools() {
    for viewport in [[412, 891], [891, 412]] {
        for control in [ToolbarControl::ToolSlot { slot: ToolSlotId::Fill },
            ToolbarControl::ToolSlot { slot: ToolSlotId::Gradient }, ToolbarControl::Command { command: CommandId::Select },
            ToolbarControl::ToolSlot { slot: ToolSlotId::Healing }] {
            let mut s = phone_session(viewport);
            s.state.workspace.layout.phone = Some(Vec::new());
            s.state.workspace.layout.insert_phone_tools(None, &[control]).unwrap();
            let tile = s.state.workspace.layout.phone.as_ref().unwrap()[0].id;
            let anchor = DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::Tool(tile) };
            match control {
                ToolbarControl::Command { command } => { invoke(&mut s, command); },
                ToolbarControl::ToolSlot { slot } => { s.dispatch(slot_choice(&s, anchor, slot.variants()[0])).unwrap(); },
                _ => unreachable!(),
            }
            s.dispatch(UiAction::Phone { action: PhoneAction::Tool { tile } }).unwrap();
            let drawer = s.state.customization.drawer.as_ref().unwrap();
            assert_eq!(drawer.anchor, anchor);
            assert_eq!(drawer.columns, [vec![if matches!(control, ToolbarControl::Command { .. } | ToolbarControl::ToolSlot { slot: ToolSlotId::Fill }) { Panel::Tools } else { Panel::Brushes }]]);
            assert!(!drawer.sets_column);
            assert!(serde_json::to_value(drawer).unwrap().get("sets_column").is_none());
            if let ToolbarControl::ToolSlot { slot } = control {
                let view = drawer.tool_set.as_ref().unwrap();
                assert!(view.groups.is_empty());
                assert_eq!(view.subtools.iter().map(|item| item.action.clone()).collect::<Vec<_>>(),
                    slot.variants().iter().map(|&variant| UiAction::ChooseToolVariant { anchor, variant }).collect::<Vec<_>>());
            } else {
                assert!(drawer.tool_set.is_none());
                assert!(!s.state.tool_panel(Panel::Tools).subtools.is_empty());
            }
        }
    }
}

#[test]
fn phone_layers_use_full_edge_and_retain_scroll_content() {
    for viewport in [[891, 412], [568, 320], [320, 568]] {
        let s = phone_session(viewport);
        let drawer = ContentDrawer::for_control(
            ToolbarControl::Panel { panel: Panel::Layers }, DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::Capy },
        ).unwrap();
        let phone = s.phone_layout().unwrap();
        let p = s.state.customization.drawer_placement(
            &drawer, &s.layout_resolve_input(), viewport.map(|v| v as f32), &[1000.],
        ).unwrap();
        if viewport[0] > viewport[1] {
            assert_eq!(p.bounds.width, 280.);
            assert_eq!(p.bounds.height, phone.bars[0].height);
        } else {
            assert_eq!(p.bounds.x, phone.bars[0].x);
            assert_eq!(p.bounds.width, phone.bars[0].width);
            assert!(p.bounds.y + p.bounds.height <= phone.capy.y);
        }
        assert!(p.bounds.height < 1000.);
        let measurement = PanelScrollMeasurement { fixed_height: 116., unit_height: 40. };
        let pages = LayerDrawerSizing::phone_pages(
            measurement, 20, p.bounds.height, viewport[0] > viewport[1], 900.,
        );
        assert_eq!(pages, [p.bounds.height; 2]);
        assert!(pages[0] - LayerDrawerSizing::FOOTER_HEIGHT - measurement.fixed_height
            < 20. * measurement.unit_height);
        if viewport[0] < viewport[1] {
            let small = LayerDrawerSizing::phone_pages(measurement, 1, p.bounds.height, false, 80.);
            assert_eq!(small[0], 116. + 40. + LayerDrawerSizing::FOOTER_HEIGHT);
            assert_eq!(small[1], 80. + LayerDrawerSizing::FOOTER_HEIGHT);
        }
        let input = s.layout_resolve_input();
        let logical = viewport.map(|v| v as f32);
        let sizes = s.state.customization.layer_drawer_sizing(
            Some(&drawer), &input, logical, measurement, 1, 80.,
        ).unwrap();
        assert_eq!(sizes["heights"], serde_json::to_value(LayerDrawerSizing::phone_pages(
            measurement, 1, p.bounds.height, viewport[0] > viewport[1], 80.,
        )).unwrap());
        let measure = s.state.customization.drawer_measure(&drawer, &input, logical).unwrap();
        let retained = drawer_frame(&serde_json::json!({
            "measure": measure, "progress": 1.,
            "layer": { "measurement": measurement, "rows": 1,
                "viewport_height": logical[1], "properties_height": 80. },
        }).to_string()).unwrap();
        assert_eq!(retained["sizes"], sizes);
        assert!(p.connection().is_some());
    }
}

#[test]
fn open_phone_layers_follow_edits_and_visible_rows_up_to_the_bar() {
    let mut s = phone_session([412, 891]);
    let paper = occurrence_token(s.engine.document().bottom_paper().unwrap());
    s.dispatch(UiAction::SelectLayer { id: paper }).unwrap();
    invoke(&mut s, CommandId::DeleteLayer);
    assert!(s.state.layer_tools.add_paper.is_some());
    let tile = s.state.workspace.layout.phone.as_ref().unwrap().iter()
        .find(|tile| tile.control == ToolbarControl::Panel { panel: Panel::Layers }).unwrap().id;
    s.dispatch(UiAction::Phone { action: PhoneAction::Tool { tile } }).unwrap();
    let drawer = s.state.customization.drawer.clone().unwrap();
    let measurement = PanelScrollMeasurement { fixed_height: 116., unit_height: 40. };
    let height = |s: &UiSession<Recorder>| {
        assert_eq!(s.state.customization.drawer.as_ref(), Some(&drawer));
        let rows = s.state.layer_tools.presented_rows(&s.state.layers);
        assert_eq!(rows, s.state.layers.len() + 1, "Add Paper occupies a visible row");
        let sizes = s.state.customization.layer_drawer_sizing(Some(&drawer), &s.layout_resolve_input(), [412., 891.], measurement, rows, 200.).unwrap();
        let h = sizes["placements"][0]["bounds"]["height"].as_f64().unwrap() as f32;
        let content = measurement.fixed_height + rows as f32 * measurement.unit_height + LayerDrawerSizing::FOOTER_HEIGHT;
        assert_eq!(h, content.min(s.phone_layout().unwrap().work_area.height));
        (h, content)
    };
    let before = height(&s).0;
    invoke(&mut s, CommandId::AddLayer);
    assert_eq!(height(&s).0, before + measurement.unit_height);
    invoke(&mut s, CommandId::DeleteLayer);
    assert_eq!(height(&s).0, before);
    s.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } }).unwrap();
    let group = s.state.layer_tools.editing_layer.as_ref().unwrap().id;
    invoke(&mut s, CommandId::AddLayer);
    let expanded = height(&s).0;
    s.dispatch(UiAction::Layer { action: LayerAction::Collapse { id: group } }).unwrap();
    assert_eq!(height(&s).0, expanded - measurement.unit_height);
    s.dispatch(UiAction::Layer { action: LayerAction::Collapse { id: group } }).unwrap();
    assert_eq!(height(&s).0, expanded);
    for _ in 0..20 { invoke(&mut s, CommandId::AddLayer); }
    let (capped, content) = height(&s);
    assert_eq!(capped, s.phone_layout().unwrap().work_area.height);
    assert!(content > capped && s.state.layers.len() > 20, "the capped drawer retains scrollable rows");
}

#[test]
fn phone_tool_settings_uses_its_fixed_tile_and_releases_multi_column_drawers() {
    let mut s = phone_session([412, 891]);
    let view = s.state.phone_settings();
    assert_eq!(view.id, None);
    assert_eq!(view.resolved_control, PhoneLayout::SETTINGS);
    assert!(serde_json::to_value(view).unwrap().get("id").is_none());
    let desktop = s.panel_view(Panel::Toolbar).unwrap();
    assert!(!desktop.tiles.is_empty());
    for tile in desktop.tiles {
        let id = tile.id.unwrap();
        assert_eq!(serde_json::to_value(tile).unwrap()["id"].as_u64(), Some(id as u64));
    }
    let before = s.capture_workspace().unwrap();
    s.dispatch(UiAction::Phone { action: PhoneAction::ToolSettings }).unwrap();
    let drawer = s.state.customization.drawer.as_ref().unwrap();
    assert_eq!(drawer.anchor, DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::ToolSettings });
    assert_eq!(drawer.columns, vec![vec![Panel::ToolSettings]]);
    assert_eq!(s.capture_workspace().unwrap(), before);
    for command in [CommandId::DrawingBrush, CommandId::Select] {
        invoke(&mut s, command);
        let tile = s.state.workspace.layout.phone.as_ref().unwrap().iter().find(|tile| tile.control == ToolbarControl::Command { command }).unwrap().id;
        s.dispatch(UiAction::Phone { action: PhoneAction::Tool { tile } }).unwrap();
        assert!(!s.state.customization.drawer.as_ref().unwrap().panels().any(|panel| panel == Panel::ToolSettings));
        if command == CommandId::Select {
            let anchor = s.state.customization.drawer.as_ref().unwrap().anchor;
            invoke(&mut s, CommandId::RectangleSelect);
            let drawer = s.state.customization.drawer.as_ref().unwrap();
            assert_eq!(drawer.anchor, anchor);
            assert!(!drawer.panels().any(|panel| panel == Panel::ToolSettings));
        }
    }
    for control in [ToolbarControl::ToolSlot { slot: ToolSlotId::Drawing }, ToolbarControl::Command { command: CommandId::Fill },
        ToolbarControl::Command { command: CommandId::Select }, ToolbarControl::Command { command: CommandId::Sculpt }] {
        let phone = ContentDrawer::for_control(control, DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::Tool(1) }).unwrap();
        let desktop = ContentDrawer::for_control(control, DrawerAnchor::Header { id: 1 }).unwrap();
        assert!(!phone.panels().any(|panel| panel == Panel::ToolSettings));
        assert!(desktop.panels().any(|panel| panel == Panel::ToolSettings));
        assert!(!phone.columns.is_empty());
    }
    for control in [ToolbarControl::ColorPicker, ToolbarControl::Command { command: CommandId::Crop }] {
        assert_eq!(ContentDrawer::for_control(control, DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::Tool(1) }).unwrap().columns, vec![vec![Panel::ToolSettings]]);
    }
}

#[test]
fn phone_runtime_controls_preserve_raw_layout_and_retained_resolution() {
    let mut s = phone_session([412, 891]);
    s.state.settings.zen_show_capy = false;
    let layout = s.state.workspace.layout.clone();
    let history = s.capture_workspace().unwrap().history;
    let context = s.state.toolbar_context();
    for (index, action) in [
        UiAction::Phone { action: PhoneAction::Capy },
        UiAction::Phone { action: PhoneAction::Capy },
        UiAction::Invoke { command: CommandId::ZenMode },
        UiAction::Phone { action: PhoneAction::Capy },
        UiAction::Phone { action: PhoneAction::Slider {
            control: ToolbarControl::BrushSizeSlider, phase: ContactPhase::Down,
        } },
        UiAction::ToolbarEdit { context, action: Box::new(
            ToolbarControl::BrushSizeSlider.slider().unwrap().action(43.),
        ) },
        UiAction::Phone { action: PhoneAction::Slider {
            control: ToolbarControl::BrushSizeSlider, phase: ContactPhase::Up,
        } },
        UiAction::Phone { action: PhoneAction::Metrics {
            native: [0., 48.], native_counts: [0, 1], insets: [0., 24., 0., 16.], movable: false,
        } },
    ].into_iter().enumerate() {
        s.dispatch(action).unwrap();
        assert_eq!(s.state.phone.menu, index == 0);
        assert_eq!(s.state.phone.hidden, index == 2);
        if index == 2 {
            let reply = s.input(UiInput::Chrome { event: ChromeEvent::Refresh, facts: Default::default(), viewport: [412., 891.] }).unwrap();
            assert!(reply.chrome_hidden && reply.keep_zen_button);
        }
        if index == 5 { assert_eq!(s.state.brush.diameter, 43.); }
        assert_eq!(s.state.workspace.layout, layout);
        assert_eq!(s.capture_workspace().unwrap().history, history);
        for input in [s.layout_resolve_input(), s.layout_presentation_input()] {
            let encoded = serde_json::to_string(&input).unwrap();
            let retained: LayoutResolveInput<'static> = serde_json::from_str(&encoded).unwrap();
            assert_eq!(*retained, layout);
            assert_eq!(retained.phone_state(), s.state.phone);
            assert_eq!(retained.phone_presentation([412., 891.]).unwrap().bars, s.phone_layout().unwrap().bars);
        }
    }
}

#[test]
fn phone_main_menu_extras_follow_header_and_title_bar_edit_is_refused() {
    let mut s = phone_session([1000, 900]);
    for preset in [WorkspacePreset::Painter, WorkspacePreset::Illustrator, WorkspacePreset::Photographer] {
        s.state.workspace.layout = preset.layout(Platform::Gtk);
        assert!(matches!(s.capy_action(), UiAction::Invoke { command: CommandId::ZenMode }));
        assert_eq!(s.main_menu_extras(), [preset == WorkspacePreset::Painter, false]);
        s.set_viewport([412., 891.], [412, 891]).unwrap();
        assert!(matches!(s.capy_action(), UiAction::Phone { action: PhoneAction::Capy }));
        assert_eq!(s.main_menu_extras(), [true, true]);
        let primary = s.application_menu(ApplicationMenu::Primary);
        assert!(matches!(primary.sections[0][0].action, Some(UiAction::Invoke { command: CommandId::SearchCommands })));
        assert!(matches!(primary.sections[1][0].action, Some(UiAction::Preferences { action: PreferenceAction::Edit {
            id: PreferenceId::FingerDrawing, value: PreferenceValue::Choice(1),
        } })));
        assert_eq!(primary.sections[1][0].selected, Some(false));
        assert!(matches!(primary.sections[1][1].action, Some(UiAction::Invoke { command: CommandId::ZenMode })));
        assert!(matches!(primary.sections[1][2].action, Some(UiAction::Phone { action: PhoneAction::SwapSides })));
        assert_eq!(primary.sections[2].iter().map(|item| item.label.as_str()).collect::<Vec<_>>(),
            ApplicationMenu::ALL.iter().map(|menu| menu.localized_label(s.localization()).to_string()).collect::<Vec<_>>());
        assert!(!primary.sections.iter().flatten().any(|item| matches!(item.action,
            Some(UiAction::Invoke { command: CommandId::Settings }))));
        let menu = s.phone_menu().unwrap();
        assert_eq!(menu.strip.len(), 5);
        assert_eq!(menu.strip.iter().map(|item| item.selected).collect::<Vec<_>>(), [None, None, Some(false), None, Some(false)]);
        assert!(matches!(menu.strip[0].action, Some(UiAction::Invoke { command: CommandId::Settings })));
        assert_eq!(menu.strip[1..].iter().map(|item| &item.action).collect::<Vec<_>>(), primary.sections[..2].iter().flatten().map(|item| &item.action).collect::<Vec<_>>());
        assert_eq!(menu.list.sections, primary.sections[2..]);
        assert!(!s.application_menu(ApplicationMenu::Window).sections.iter().flatten().any(|item|
            matches!(item.action, Some(UiAction::Invoke { command: CommandId::CustomizeWorkspaceUi }))));
        assert!(!s.command(CommandId::CustomizeWorkspaceUi).enabled);
        let before = s.capture_workspace().unwrap();
        assert!(s.dispatch(HeaderAction::Edit { editing: true }.action()).is_err());
        assert!(s.dispatch(UiAction::Invoke { command: CommandId::CustomizeWorkspaceUi }).is_err());
        assert_eq!(s.capture_workspace().unwrap(), before);
        s.set_viewport([1000., 900.], [1000, 900]).unwrap();
        assert!(s.command(CommandId::CustomizeWorkspaceUi).enabled);
        assert!(matches!(s.application_menu(ApplicationMenu::Primary).sections.last().unwrap()[0].action,
            Some(UiAction::Invoke { command: CommandId::Settings })));
    }
    for viewport in [[891, 412], [568, 320], [320, 568], [320, 320]] {
        let mut s = phone_session(viewport);
        let layout = s.state.workspace.layout.clone();
        let history = s.capture_workspace().unwrap().history;
        for swapped in [false, true] {
            if swapped { s.dispatch(UiAction::Phone { action: PhoneAction::SwapSides }).unwrap(); }
            s.dispatch(UiAction::Phone { action: PhoneAction::Capy }).unwrap();
            assert!(s.state.phone.menu);
            let phone = s.phone_layout().unwrap();
            let p = phone.menu(viewport.map(|v| v as f32), PANEL_MIN_WIDTH, 700., 32.).unwrap();
            assert_eq!(p.columns.len(), 1);
            assert!(p.bounds.x >= 0. && p.bounds.y >= 0.);
            assert!(p.bounds.x + p.bounds.width <= viewport[0] as f32 + 0.01);
            assert!(p.bounds.y + p.bounds.height <= viewport[1] as f32 + 0.01);
            assert!(p.columns.iter().all(|c| c.x >= 0. && c.y >= 0.
                && c.x + c.width <= p.bounds.width + 0.01
                && c.y + c.height <= p.bounds.height + 0.01));
            if viewport[0] > viewport[1] {
                assert!(p.bounds.y >= phone.bars[1].y);
                assert!(p.bounds.y + p.bounds.height <= phone.bars[1].y + phone.bars[1].height + 0.01);
            } else {
                assert_eq!(p.bounds.x, phone.bars[0].x);
                assert_eq!(p.bounds.width, phone.bars[0].width);
                assert_eq!(p.columns[0].x, 0.);
            }
            assert!(p.connection().is_some());
            assert_eq!(s.state.workspace.layout, layout);
            assert_eq!(s.capture_workspace().unwrap().history, history);
            s.dispatch(UiAction::Phone { action: PhoneAction::Capy }).unwrap();
            assert!(!s.state.phone.menu);
            assert_eq!(s.state.workspace.layout, layout);
            assert_eq!(s.capture_workspace().unwrap().history, history);
        }
    }
}

#[test]
fn phone_workspace_options_enter_window_workspaces_and_keep_both_back_destinations() {
    let mut s = phone_session([412, 891]);
    s.configure_workspace_manager(ManagedWorkspace {
        id: "paint".into(), name: "Paint".into(), baseline: s.state.workspace.layout.clone(),
        choices: vec![WorkspaceChoice { id: "sketch".into(), name: "Sketch".into() }],
    }).unwrap();
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuOpen { page: MenuPageId::Workspaces } }).unwrap();
    let route = s.state.phone.menu_navigation.current();
    let labels = [DocumentTabLabel { id: 1, title: "Study".into(), location: String::new(), uri: None, export_uri: None, modified: false }];
    let selector = DrawingSelector::new(&labels, 1, &s.command(CommandId::NewDocument), s.localization());
    let menu = s.phone_menu().unwrap().view(&selector, s.state.platform, s.localization());
    let page = menu.pages.iter().find(|page| page.id == route).unwrap();
    assert!(page.sections.iter().flatten().any(|item| matches!(&item.action,
        Some(UiAction::Phone { action: PhoneAction::MenuActivate { route: source, action } })
            if *source == route && matches!(&**action, UiAction::WorkspaceManager { command: WorkspaceCommand::Manage }))));
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuBack { route } }).unwrap();
    assert_eq!(s.state.phone.menu_navigation.current().page, Some(MenuPageId::Application(ApplicationMenu::Window)));
    let route = s.state.phone.menu_navigation.current();
    s.dispatch(UiAction::Phone { action: PhoneAction::MenuBack { route } }).unwrap();
    assert!(s.state.phone.menu && s.state.phone.menu_navigation.is_root());

}

#[test]
fn phone_presentation_waits_for_each_hosts_capability() {
    for platform in Platform::ALL {
        for viewport in [[360., 640.], [640., 360.]] {
            let mut s = session(platform);
            s.state.workspace.layout = WorkspacePreset::Painter.layout(platform);
            s.set_viewport(viewport, viewport.map(|v| v as u32)).unwrap();
            let input = s.layout_resolve_input();
            let resolved = input.resolve(viewport);
            assert_eq!(resolved.phone.is_some(), matches!(platform, Platform::Gtk | Platform::Web));
            assert_eq!(s.phone_layout().is_some(), matches!(platform, Platform::Gtk | Platform::Web));
            if !matches!(platform, Platform::Gtk | Platform::Web) {
                let desktop = s.state.workspace.layout.resolved(viewport);
                assert_eq!(resolved.work_area, desktop.work_area);
                assert_eq!(resolved.groups.len(), desktop.groups.len());
            }
            let rows = s.state.settings.pages(platform).into_iter().flat_map(|page| page.groups).flat_map(|group| group.rows);
            assert_eq!(rows.into_iter().any(|row| row.id == PreferenceId::FingerDrawing), matches!(platform, Platform::Gtk | Platform::Web));
            assert!(s.state.workspace.layout.phone.is_some());
        }
    }
}

#[test]
fn dialog_roles_follow_the_same_phone_threshold_and_safe_area_as_chrome() {
    let mut session = phone_session([412, 891]);
    let state = &mut session.state;
    state.phone.insets = [6., 12., 4., 8.];
    for viewport in [[412., 891.], [891., 412.], [320., 568.]] {
        assert_eq!(state.dialog_presentation(DialogRole::Editor, viewport),
            DialogPresentation::Page(Bounds { x: 6., y: 12., width: viewport[0] - 10., height: viewport[1] - 20. }));
        assert_eq!(state.dialog_presentation(DialogRole::CanvasPanel, viewport),
            DialogPresentation::CanvasPanel(state.phone_presentation(viewport).unwrap().work_area));
        assert_eq!(state.dialog_presentation(DialogRole::Alert, viewport), DialogPresentation::Floating);
    }
    assert_eq!(state.dialog_presentation(DialogRole::Editor, [480., 891.]), DialogPresentation::Floating);
}

#[test]
fn phone_editor_panels_notices_and_search_stay_between_the_bars() {
    for viewport in [[412, 891], [891, 412], [320, 568], [320, 320]] {
        let mut s = phone_session(viewport);
        let viewport = viewport.map(|v| v as f32);
        let area = s.phone_layout().unwrap().work_area;
        let contains = |bounds: Bounds| bounds.x >= area.x && bounds.y >= area.y
            && bounds.x + bounds.width <= area.x + area.width + 0.01
            && bounds.y + bounds.height <= area.y + area.height + 0.01;
        let panel = s.state.dialog_presentation(DialogRole::CanvasPanel, viewport).bounds(|| [500., 800.]).unwrap();
        assert!(contains(panel));
        let notice = s.layout(viewport).notice;
        assert!(!notice.window && notice.y < area.y + area.height);
        let search = COMMAND_SEARCH_STYLE.placement(viewport, area, true);
        assert!(contains(search));
        if viewport[0] < 600. { assert_eq!(search.y, area.y + 16.); }
        s.dispatch(UiAction::Invoke { command: CommandId::PenPressure }).unwrap();
        s.dispatch(UiAction::PressureCalibration { action: PressureCalibrationAction::Measure { extent: [384., 360.], viewport } }).unwrap();
        let before = s.state.pressure_calibration.as_ref().unwrap().bounds;
        assert!(contains(before));
        for (phase, position) in [(ContactPhase::Down, [before.x, before.y]), (ContactPhase::Move, [-1000., 1000.])] {
            s.dispatch(UiAction::PressureCalibration { action: PressureCalibrationAction::Drag { phase, position, viewport } }).unwrap();
        }
        assert!(contains(s.state.pressure_calibration.as_ref().unwrap().bounds));
    }
}

#[test]
fn phone_toolbar_splices_preserve_ids_and_reject_overflow_atomically() {
    let mut layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    layout.phone = Some(Vec::new());
    let color = ToolbarControl::Color;
    layout.insert_phone_tools(None, &[color; PhoneLayout::MAX_TOOLS - 1]).unwrap();
    let first = layout.phone.as_ref().unwrap()[0].id;
    layout.insert_phone_tools(Some(first), &[ToolbarControl::Divider, color]).unwrap();
    let before = layout.clone();
    assert!(layout.insert_phone_tools(None, &[color]).is_err());
    assert_eq!(layout, before);
    layout.move_phone_tool(first, None).unwrap();
    assert_eq!(layout.phone.as_ref().unwrap().last().unwrap().id, first);
    layout.remove_phone_tool(first).unwrap();
    assert!(!layout.phone.as_ref().unwrap().iter().any(|tile| tile.id == first));
    layout.insert_phone_tools(None, &[ToolbarControl::Divider, ToolbarControl::Divider, color, ToolbarControl::Divider]).unwrap();
    assert!(!layout.phone.as_ref().unwrap().windows(2).any(|tiles| tiles.iter().all(|tile| tile.control == ToolbarControl::Divider)));
    layout.validate().unwrap();
    let mut unresolved = DockLayout::default();
    let before = unresolved.clone();
    for error in [unresolved.insert_phone_tools(None, &[color]), unresolved.remove_phone_tool(first), unresolved.move_phone_tool(first, None)] {
        assert_eq!(error.unwrap_err(), "The phone toolbar isn't resolved");
    }
    assert_eq!(unresolved, before);
    unresolved.resolve_phone_defaults(WorkspacePreset::Photographer).unwrap();
    unresolved.insert_phone_tools(None, &[color]).unwrap();
    assert_eq!(unresolved.phone.as_ref().unwrap().len(), WorkspacePreset::Photographer.phone_controls().len() + 1);
}

#[test]
fn phone_toolbar_dividers_are_free_at_both_ends_and_empty_stays_empty() {
    let mut layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    layout.phone = Some(Vec::new());
    let controls: Vec<_> = (0..PhoneLayout::MAX_TOOLS * 2 + 1).map(|index| if index % 2 == 0 { ToolbarControl::Divider } else { ToolbarControl::Color }).collect();
    layout.insert_phone_tools(None, &controls).unwrap();
    layout.validate().unwrap();
    let presentation = PhoneLayout::resolve(&layout, [412., 891.], [0.; 4], [0.; 2], [0; 2], false, false).unwrap();
    assert!(presentation.tools[PhoneLayout::MAX_TOOLS * 2].width > 0.);
    for tile in layout.phone.clone().unwrap() { layout.remove_phone_tool(tile.id).unwrap(); }
    layout.resolve_phone_defaults(WorkspacePreset::Photographer).unwrap();
    assert_eq!(layout.phone, Some(Vec::new()));
    assert_eq!(serde_json::from_str::<DockLayout>(&serde_json::to_string(&layout).unwrap()).unwrap().phone, Some(Vec::new()));
}

#[test]
fn phone_picker_limits_tools_keeps_divider_enabled_and_records_one_edit() {
    let mut s = phone_session([412, 891]);
    let original = s.state.workspace.layout.clone();
    s.dispatch(UiAction::Phone { action: PhoneAction::InsertTools { before: None } }).unwrap();
    for command in [CommandId::Undo, CommandId::Redo] {
        s.dispatch(UiAction::Customize { action: CustomizationAction::PickerSelect { control: ToolbarControl::Command { command }, selected: true } }).unwrap();
    }
    let picker = s.tool_picker().unwrap();
    assert_eq!(picker.selected_label, "The phone toolbar holds up to ten tools.");
    assert!(picker.choices.iter().all(|choice| crate::phone::tool_allowed(choice.control)));
    assert!(!picker.disabled.contains(&ToolbarControl::Divider));
    assert!(picker.disabled.contains(&ToolbarControl::Color));
    assert!(s.dispatch(UiAction::Customize { action: CustomizationAction::PickerSelect { control: ToolbarControl::Color, selected: true } }).is_err());
    s.dispatch(UiAction::Customize { action: CustomizationAction::ConfirmTools }).unwrap();
    let inserted = s.state.workspace.layout.clone();
    s.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace }).unwrap();
    assert_eq!(s.state.workspace.layout, original);
    assert!(!s.workspace_history.can_undo());
    s.dispatch(UiAction::Invoke { command: CommandId::RedoWorkspace }).unwrap();
    assert_eq!(s.state.workspace.layout, inserted);
    s.dispatch(UiAction::Phone { action: PhoneAction::InsertTools { before: None } }).unwrap();
    s.dispatch(UiAction::Customize { action: CustomizationAction::CancelTools }).unwrap();
    assert_eq!(s.state.workspace.layout, inserted);
}

include!("phone_restore_tests.rs");
