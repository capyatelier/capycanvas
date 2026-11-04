use super::*;

fn layout(d: &Driver) -> DockLayout {
    layer_ui::durable_layout(&state(&d.w).workspace.layout)
}

fn history(d: &Driver) -> layer_ui::WorkspaceCapture {
    ui_session_mut(&d.w).capture_workspace().unwrap()
}

fn popup(d: &Driver) -> Option<gtk::Popover> {
    d.w.popovers.borrow().iter().filter_map(|popup| popup.upgrade())
        .find(|popup| popup.is_mapped() && popup.has_css_class("panel-context-menu"))
}

fn hold(d: &mut Driver, tile: Option<u32>) {
    until(|| {
        let view = presentation(d);
        layout(d).phone.unwrap().iter().enumerate().all(|(index, tile)| {
            find_named(d.w.window.upcast_ref(), &format!("phone-tool-{}", tile.id))
                .and_then(|widget| widget.compute_bounds(&d.w.surface)).is_some_and(|bounds| {
                    let expected = view.tools[index];
                    bounds.width() > 0. && bounds.height() > 0. && (bounds.x() - expected.x).abs() <= 1.
                        && (bounds.y() - expected.y).abs() <= 1.
                })
        })
    }, "phone tiles are allocated at their shared positions");
    let point = tile.map_or_else(|| {
        let view = presentation(d);
        view.bars.into_iter().flat_map(|bar|
            (bar.y.ceil() as i32..(bar.y + bar.height).floor() as i32).flat_map(move |y|
                (bar.x.ceil() as i32..(bar.x + bar.width).floor() as i32).map(move |x| [x as f32 + 0.5, y as f32 + 0.5])))
            .find(|point| !view.tool_hits.iter().chain(&view.tools).chain(&view.controls)
                .chain([&view.capy_hit, &view.settings_hit, &view.size, &view.strength])
                .any(|bounds| bounds.width > 0. && bounds.height > 0. && bounds.contains(point[0], point[1])))
            .expect("the phone bars have native empty space outside every control")
    }, |id| d.point(&d.named(&format!("phone-tool-{id}"))));
    d.input.perform(serde_json::json!([contact("touch", "down", point), { "wait_ms": 900 }]));
    assert!(popup(d).is_none(), "holding lifts the tile without opening its release menu");
    d.input.perform(serde_json::json!([contact("touch", "up", point)]));
    until(|| popup(d).is_some(), "hold release opens the native editing menu");
}

fn menu_action(d: &mut Driver, label: &str) {
    let menu = popup(d).unwrap();
    let item = find_menu_item(menu.upcast_ref(), label).unwrap();
    d.click(&item);
    until(|| !menu.is_mapped(), "the native editing menu closes");
}

fn picker(d: &Driver) -> gtk::Widget {
    until(|| d.w.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "tool-picker"
        && crate::page_dialog::parent(&dialog).is_mapped()) && !d.w.pages.root.is_transition_running(),
        "the tool picker page is mapped");
    crate::page_dialog::parent(&d.w.visible_dialog().unwrap())
}

fn select(d: &mut Driver, count: usize) -> Vec<ToolbarControl> {
    let frame = picker(d);
    let controls = ui_session(&d.w).tool_picker().unwrap().choices.into_iter()
        .filter(|choice| choice.control != ToolbarControl::Divider).take(count).map(|choice| choice.control).collect::<Vec<_>>();
    assert_eq!(controls.len(), count);
    for control in &controls {
        let row = named::<gtk::CheckButton>(&frame, &format!("tool-choice-{}", serde_json::to_string(control).unwrap()));
        super::super::histogram::scroll_to(row.upcast_ref());
        d.click(row.upcast_ref());
        assert!(row.is_active());
    }
    assert_eq!(ui_session(&d.w).tool_picker().unwrap().selected_count, count);
    controls
}

fn confirm(d: &mut Driver) {
    let frame = picker(d);
    let button = descendants::<gtk::Button>(&frame).into_iter()
        .find(|button| button.is_mapped() && button.has_css_class("suggested-action")).unwrap();
    assert!(button.is_sensitive());
    d.click(button.upcast_ref());
    until(|| ui_session(&d.w).tool_picker().is_none() && d.w.visible_dialog().is_none()
        && !d.w.pages.root.is_transition_running(), "the confirmed picker closes");
}

fn restore(d: &mut Driver, phone: bool) {
    let before = layout(d);
    let prior = history(d);
    let starting = d.w.workspaces.manager().unwrap().current().unwrap().starting_layout(Platform::Gtk, false).unwrap();
    let expected = before.starting_layout_candidate(&starting, phone).unwrap();
    assert_ne!(expected, before);
    assert!(ui_session(&d.w).command(CommandId::ResetLayout).enabled);
    d.w.dispatch(UiAction::Invoke { command: CommandId::ResetLayout });
    until(|| d.w.visible_dialog().is_some() && mapped_label(d.w.window.upcast_ref(), "Restore Starting Layout").is_some(),
        "Restore Starting Layout maps its confirmation");
    assert_eq!(layout(d), if phone { before.clone() } else { expected.clone() }, "the confirmation uses its scoped preview policy");
    if phone {
        let body = widgets(d.w.window.upcast_ref()).filter_map(|widget| widget.downcast::<gtk::Label>().ok())
            .find(|label| label.is_mapped() && label.text().contains("starting phone toolbar"))
            .expect("phone confirmation names its starting phone toolbar");
        assert!(!body.text().contains("preview"), "phone confirmation never promises a live preview");
    }
    if phone { capture(d, "customize-phone-restore-confirm"); }
    else { capture_current(d, "customize-desktop-restore-confirm"); }
    if phone && let Some(directory) = std::env::var_os("LAYER_NATIVE_CAPTURE_DIR") {
        let theme = std::env::var("CAPY_NATIVE_TEST_THEME").unwrap_or_else(|_| "light".into());
        let path = std::path::PathBuf::from(directory).join(format!("phone-{}x{}-{theme}-customize-phone-restore-confirm.allocations.json", d.w.surface.width(), d.w.surface.height()));
        let capture: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(capture["missing"].as_array().unwrap().is_empty(), "a compact alert is not a missing editor page: {}", capture["missing"]);
    }
    let button = widgets(d.w.window.upcast_ref()).filter_map(|widget| widget.downcast::<gtk::Button>().ok())
        .find(|button| button.is_mapped() && button.label().as_deref() == Some("Restore")).unwrap();
    d.click(button.upcast_ref());
    until(|| layout(d) == expected && !d.w.workspaces.view().busy && d.w.visible_dialog().is_none(),
        "Restore Starting Layout adopts its scoped candidate");
    assert_eq!(history(d).history.undo.len(), prior.history.undo.len() + 1);
    assert!(!ui_session(&d.w).command(CommandId::ResetLayout).enabled, "a second restore is unavailable");
    let mut current = serde_json::to_value(if phone { &before } else { &starting }).unwrap();
    let mut restored = serde_json::to_value(&expected).unwrap();
    if phone {
        for value in [&mut current, &mut restored] {
            value.as_object_mut().unwrap().remove("phone");
            value.as_object_mut().unwrap().remove("next_tile_id");
        }
    } else {
        current = serde_json::to_value(before.phone.as_ref().unwrap().iter().map(|tile| { let mut tile = tile.clone(); tile.id = 0; tile }).collect::<Vec<_>>()).unwrap();
        restored = serde_json::to_value(expected.phone.as_ref().unwrap().iter().map(|tile| { let mut tile = tile.clone(); tile.id = 0; tile }).collect::<Vec<_>>()).unwrap();
    }
    assert_eq!(current, restored, "scoped restore preserves the other arrangement");
    d.w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
    assert_eq!(layout(d), before, "one Undo restores exact IDs and arrangement");
    d.w.dispatch(UiAction::Invoke { command: CommandId::RedoWorkspace });
    assert_eq!(layout(d), expected);
    if phone { capture(d, "customize-phone-restored"); }
    else { capture_current(d, "customize-desktop-restored"); }
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_customize --native-storage; 412x891 light / 891x412 dark"]
fn native_phone_customize() {
    let mut d = setup("art.capycanvas.PhoneCustomize");
    let before = layout(&d);
    let undo = history(&d).history.undo.len();
    let removed = before.phone.as_ref().unwrap()[0].id;
    hold(&mut d, Some(removed));
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() { inventory_monitor(&mut d, "customize-tile-menu"); }
    menu_action(&mut d, "Remove Tool");
    assert!(!layout(&d).phone.unwrap().iter().any(|tile| tile.id == removed));
    assert_eq!(history(&d).history.undo.len(), undo + 1);
    let after_remove = layout(&d);
    d.w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
    assert_eq!(layout(&d), before);
    d.w.dispatch(UiAction::Invoke { command: CommandId::RedoWorkspace });
    assert_eq!(layout(&d), after_remove);

    let anchor = after_remove.phone.as_ref().unwrap()[0].id;
    hold(&mut d, Some(anchor));
    menu_action(&mut d, "Insert Tools…");
    let controls = select(&mut d, 2);
    capture(&mut d, "customize-insert-two");
    let undo = history(&d).history.undo.len();
    confirm(&mut d);
    let inserted = layout(&d);
    assert_eq!(inserted.phone.as_ref().unwrap()[..2].iter().map(|tile| tile.control).collect::<Vec<_>>(), controls);
    assert_eq!(inserted.phone.as_ref().unwrap()[2].id, anchor);
    assert_eq!(history(&d).history.undo.len(), undo + 1);
    hold(&mut d, Some(anchor));
    menu_action(&mut d, "Insert Tools…");
    select(&mut d, 1);
    confirm(&mut d);
    assert_eq!(layout(&d).phone.as_ref().unwrap().iter().filter(|tile| tile.control != ToolbarControl::Divider).count(), PhoneLayout::MAX_TOOLS);
    hold(&mut d, None);
    menu_action(&mut d, "Add Tools…");
    let frame = picker(&d);
    let view = ui_session(&d.w).tool_picker().unwrap();
    assert_eq!(view.selected_label, "The phone toolbar holds up to ten tools.");
    assert!(mapped_label(&frame, &view.selected_label).is_some());
    for choice in &view.choices {
        let enabled = choice.control == ToolbarControl::Divider;
        assert_eq!(view.disabled.contains(&choice.control), !enabled);
        let check = named::<gtk::CheckButton>(&frame, &format!("tool-choice-{}", serde_json::to_string(&choice.control).unwrap()));
        let row = check.ancestor(adw::ActionRow::static_type()).unwrap().downcast::<adw::ActionRow>().unwrap();
        assert_eq!(row.get_sensitive(), enabled, "native catalog row: {:?}", choice.control);
        assert_eq!(row.is_sensitive(), enabled);
        assert_eq!(check.is_sensitive(), enabled, "native checkbox inherits row sensitivity");
        assert_eq!(check.state_flags().contains(gtk::StateFlags::INSENSITIVE), !enabled);
    }
    assert!(!view.can_confirm);
    capture(&mut d, "customize-capacity");
    let disabled = view.choices.iter().find(|choice| choice.control != ToolbarControl::Divider).unwrap().control;
    let check = named::<gtk::CheckButton>(&frame, &format!("tool-choice-{}", serde_json::to_string(&disabled).unwrap()));
    let row = check.ancestor(adw::ActionRow::static_type()).unwrap();
    super::super::histogram::scroll_to(&row);
    d.click(check.upcast_ref());
    assert!(!check.is_active(), "native clicking cannot select a tool at capacity");
    assert_eq!(ui_session(&d.w).tool_picker().unwrap().selected_count, 0);
    let divider = named::<gtk::CheckButton>(&frame, &format!("tool-choice-{}", serde_json::to_string(&ToolbarControl::Divider).unwrap()));
    let row = divider.ancestor(adw::ActionRow::static_type()).unwrap();
    super::super::histogram::scroll_to(&row);
    d.click(divider.upcast_ref());
    assert!(divider.is_active() && divider.is_sensitive(), "Divider remains natively selectable at capacity");
    assert!(ui_session(&d.w).tool_picker().unwrap().can_confirm);
    capture(&mut d, "customize-capacity-divider");
    let unchanged = history(&d);
    d.input.key(0xff1b);
    until(|| ui_session(&d.w).tool_picker().is_none() && d.w.visible_dialog().is_none()
        && !d.w.pages.root.is_transition_running(), "cancel closes the capacity picker");
    assert_eq!(history(&d), unchanged, "cancel adds no history");
    capture(&mut d, "frame-ten-tools");

    let before_move = layout(&d);
    let tiles = before_move.phone.as_ref().unwrap();
    let from = d.point(&d.named(&format!("phone-tool-{}", tiles[0].id)));
    let mut to = d.point(&d.named(&format!("phone-tool-{}", tiles[2].id)));
    to[usize::from(presentation(&d).edge != Edge::Top)] += 1.;
    let undo = history(&d).history.undo.len();
    d.input.perform(serde_json::json!([contact("touch", "down", from), { "wait_ms": 900 }, contact("touch", "move", to)]));
    capture_current(&mut d, "customize-reorder-held");
    d.input.perform(serde_json::json!([contact("touch", "up", to)]));
    let mut expected = tiles.clone();
    expected[..3].rotate_left(1);
    assert_eq!(layout(&d).phone.unwrap(), expected);
    assert_eq!(history(&d).history.undo.len(), undo + 1);
    d.w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
    assert_eq!(layout(&d), before_move);
    let unchanged = history(&d);
    let outside = [d.w.surface.width() as f32 * 0.5, d.w.surface.height() as f32 * 0.5];
    d.input.perform(serde_json::json!([contact("touch", "down", from), { "wait_ms": 900 },
        contact("touch", "move", outside), contact("touch", "up", outside)]));
    assert_eq!(history(&d), unchanged, "off-bar release cancels without history");
    capture(&mut d, "customize-offbar-cancelled");

    let mut empty = layout(&d);
    empty.phone = Some(Vec::new());
    d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState { layout: empty, ..Default::default() }) });
    capture(&mut d, "frame-empty-tools");
    hold(&mut d, None);
    let model = ui_session(&d.w).context_menu(ContextTarget::PhoneToolbar).unwrap();
    assert_eq!(model.sections.iter().flatten().map(|item| item.label.as_str()).collect::<Vec<_>>(), ["Add Tools…"]);
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() { inventory_monitor(&mut d, "customize-empty-toolbar"); }
    menu_action(&mut d, "Add Tools…");
    picker(&d);
    d.input.key(0xff1b);
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "empty toolbar picker cancels");
    restore(&mut d, true);
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_customize_desktop_restore --native-storage; 1600x1000"]
fn native_phone_customize_desktop_restore() {
    let mut d = setup("art.capycanvas.PhoneCustomizeDesktopRestore");
    assert!(state(&d.w).phone_presentation([d.w.surface.width() as f32, d.w.surface.height() as f32]).is_none());
    let mut custom = layout(&d);
    let panel = custom.panels.iter().find(|panel| matches!(panel.id, Panel::CustomToolbar(_))).unwrap().id;
    let tile = custom.phone.as_ref().unwrap()[0].id;
    custom.remove_phone_tool(tile).unwrap();
    d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState { layout: custom, ..Default::default() }) });
    d.w.dispatch(UiAction::MovePanel { panel, target: DockTarget::Float { position: [480., 220.] },
        viewport: [d.w.surface.width() as f32, d.w.surface.height() as f32] });
    restore(&mut d, false);
    d.finish();
}
