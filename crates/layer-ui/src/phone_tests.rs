fn phone_session(viewport: [u32; 2]) -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.state.workspace.layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    s.set_viewport(viewport.map(|v| v as f32), viewport).unwrap();
    s
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
            let placement = s.phone_layout().unwrap().menu(viewport.map(|v| v as f32), [242.; 2], [200., 300.]).unwrap();
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
            ToolbarControl::Color, DrawerAnchor::Phone { tile: None },
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
fn phone_color_drawer_wraps_and_keeps_overflow_reachable() {
    let mut s = phone_session([320, 568]);
    let action = s.application_menu(ApplicationMenu::Window).sections.into_iter().flatten()
        .filter_map(|item| item.action)
        .find(|action| matches!(action, UiAction::Phone { action: PhoneAction::Panel { panel: Panel::Color } }))
        .unwrap();
    s.dispatch(action).unwrap();
    let drawer = s.state.customization.drawer.as_ref().unwrap();
    assert_eq!(drawer.anchor, DrawerAnchor::Phone { tile: None });
    assert_eq!(drawer.columns, vec![vec![Panel::Color], vec![Panel::Palettes]]);
    let p = s.state.customization.drawer_placement(
        drawer, &s.layout_resolve_input(), [320., 568.], &[220., 500.],
    ).unwrap();
    assert_eq!(p.columns[0].x, p.columns[1].x);
    assert_eq!(p.bounds.x, s.phone_layout().unwrap().bars[0].x);
    assert_eq!(p.bounds.width, s.phone_layout().unwrap().bars[0].width);
    assert!(p.columns[1].y >= p.columns[0].y + p.columns[0].height);
    assert!(p.columns[1].height < 500.);
    assert!(p.columns[0].height < 220.);
    assert!(p.columns.iter().all(|c| c.y + c.height <= p.bounds.height + 0.01));
    assert!(p.bounds.y + p.bounds.height <= s.phone_layout().unwrap().capy.y);
    assert!(p.connection().is_some());
    s.dispatch(UiAction::Phone { action: PhoneAction::Metrics {
        native: [0., 48.], native_counts: [0, 1], insets: [0.; 4],
    } }).unwrap();
    assert_eq!(s.state.customization.drawer.as_ref().unwrap().anchor, DrawerAnchor::Phone { tile: None });
}

#[test]
fn phone_brush_sets_and_tools_share_a_row_before_settings_wrap() {
    let s = phone_session([320, 568]);
    let tile = s.state.workspace.layout.phone.as_ref().unwrap().iter().find(|t| {
        t.control == ToolbarControl::Command { command: CommandId::DrawingBrush }
    }).unwrap().id;
    let drawer = ContentDrawer::for_control(
        ToolbarControl::Command { command: CommandId::DrawingBrush },
        DrawerAnchor::Phone { tile: Some(tile) },
    ).unwrap();
    let p = s.state.customization.drawer_placement(
        &drawer, &s.layout_resolve_input(), [320., 568.], &[160., 240., 200.],
    ).unwrap();
    assert_eq!(p.columns.len(), 3);
    assert_eq!(p.bounds.x, s.phone_layout().unwrap().bars[0].x);
    assert_eq!(p.bounds.width, s.phone_layout().unwrap().bars[0].width);
    assert_eq!(p.columns[0].y, p.columns[1].y);
    assert!(p.columns[1].x >= p.columns[0].x + p.columns[0].width);
    assert!(p.columns[2].y >= p.columns[1].y + p.columns[1].height);
    assert!(p.columns.iter().all(|c| c.x + c.width <= p.bounds.width + 0.01));
    assert!(p.connection().is_some());
}

#[test]
fn phone_layers_use_full_edge_and_retain_scroll_content() {
    for viewport in [[891, 412], [568, 320], [320, 568]] {
        let s = phone_session(viewport);
        let drawer = ContentDrawer::for_control(
            ToolbarControl::Panel { panel: Panel::Layers }, DrawerAnchor::Phone { tile: None },
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
            native: [0., 48.], native_counts: [0, 1], insets: [0., 24., 0., 16.],
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
        let columns = s.phone_menu_columns();
        assert_eq!(columns[0].sections, primary.sections[..2]);
        assert_eq!(columns[1].sections, primary.sections[2..]);
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
            let p = phone.menu(viewport.map(|v| v as f32), [PANEL_MIN_WIDTH; 2], [600., 700.]).unwrap();
            assert_eq!(p.columns.len(), 2);
            assert!(p.bounds.x >= 0. && p.bounds.y >= 0.);
            assert!(p.bounds.x + p.bounds.width <= viewport[0] as f32 + 0.01);
            assert!(p.bounds.y + p.bounds.height <= viewport[1] as f32 + 0.01);
            assert!(p.columns.iter().all(|c| c.x >= 0. && c.y >= 0.
                && c.x + c.width <= p.bounds.width + 0.01
                && c.y + c.height <= p.bounds.height + 0.01));
            if viewport[0] > viewport[1] {
                assert!(p.bounds.y >= phone.bars[1].y);
                assert!(p.bounds.y + p.bounds.height <= phone.bars[1].y + phone.bars[1].height + 0.01);
                if viewport[0] == 891 {
                    assert_eq!(p.columns[0].y, p.columns[1].y);
                    assert!(p.columns[1].x >= p.columns[0].x + p.columns[0].width);
                } else {
                    assert_eq!(p.columns[0].x, p.columns[1].x);
                    assert!(p.columns[1].y >= p.columns[0].y + p.columns[0].height);
                }
            } else {
                assert_eq!(p.bounds.x, phone.bars[0].x);
                assert_eq!(p.bounds.width, phone.bars[0].width);
                assert_eq!(p.columns[0].x, p.columns[1].x);
                assert!(p.columns[1].y >= p.columns[0].y + p.columns[0].height);
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
    let [_, menu] = s.phone_menu_columns();
    let mut navigation = PhoneMenuNavigation::default();
    assert!(navigation.workspaces(&menu, s.localization()));
    assert_eq!(navigation.name(), "page-0-7-1-0");
    let page = PhoneMenuNavigation::pages(&menu).into_iter().find(|page| page.name() == navigation.name()).unwrap();
    assert!(page.menu.sections.iter().flatten().any(|item| matches!(item.action,
        Some(UiAction::WorkspaceManager { command: WorkspaceCommand::Manage }))));
    assert!(navigation.back());
    assert_eq!(navigation.name(), "page-0-7");
    assert!(navigation.back());
    assert!(navigation.is_root());
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
            assert_eq!(resolved.phone.is_some(), platform == Platform::Gtk);
            assert_eq!(s.phone_layout().is_some(), platform == Platform::Gtk);
            if platform != Platform::Gtk {
                let desktop = s.state.workspace.layout.resolved(viewport);
                assert_eq!(resolved.work_area, desktop.work_area);
                assert_eq!(resolved.groups.len(), desktop.groups.len());
            }
            let rows = s.state.settings.pages(platform).into_iter().flat_map(|page| page.groups).flat_map(|group| group.rows);
            assert_eq!(rows.into_iter().any(|row| row.id == PreferenceId::FingerDrawing), platform == Platform::Gtk);
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
        let panel = s.state.dialog_presentation(DialogRole::CanvasPanel, viewport).bounds([500., 800.]).unwrap();
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
