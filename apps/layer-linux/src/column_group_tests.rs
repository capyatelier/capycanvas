use super::*;

#[path = "column_group_resize_tests.rs"]
mod resize;

fn text_widget(root: &gtk::Widget, text: &str) -> Option<gtk::Widget> {
    if root
        .downcast_ref::<gtk::Label>()
        .is_some_and(|l| l.text() == text)
    {
        return Some(root.clone());
    }
    let mut child = root.first_child();
    while let Some(w) = child {
        if let Some(found) = text_widget(&w, text) {
            return Some(found);
        }
        child = w.next_sibling();
    }
    None
}
#[test]
#[ignore = "isolated Mutter mouse/touch driver: --column-groups"]
fn native_column_group_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let output = std::path::PathBuf::from(
        std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| dir.to_string_lossy().into()),
    );
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.ColumnGroups");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !w.workspaces.ready.get() || w.workspaces.busy.get() {
        assert!(Instant::now() < deadline, "workspace storage startup");
        pump(10);
    }
    std::fs::write(dir.join("ready"), "ready").unwrap();
    let mut step = 0;
    let mut perform = |events: serde_json::Value| {
        std::fs::write(
            dir.join(format!("step-{step}.json")),
            serde_json::to_vec(&events).unwrap(),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !dir.join(format!("done-{step}")).exists() && Instant::now() < deadline {
            pump(5);
        }
        assert!(
            dir.join(format!("done-{step}")).exists(),
            "native input timeout"
        );
        step += 1;
        pump(100);
    };
    let click = |p| serde_json::json!([{"point":p},{"down":true},{"down":false}]);
    let center = |b: Bounds| [b.x + b.width * 0.5, b.y + b.height * 0.5];
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    for (theme, edge) in [(Theme::Dark, Edge::Left), (Theme::Light, Edge::Right)] {
        let mut initial = layer_ui::WorkspaceState::default();
        let group = initial.layout.panel_group(Panel::Brushes).unwrap();
        initial
            .layout
            .move_panel(
                viewport,
                Panel::Sizes,
                DockTarget::Tab { group, index: None },
            )
            .unwrap();
        if edge == Edge::Right {
            initial
                .layout
                .move_item(
                    viewport,
                    DockItem::Group { group },
                    DockTarget::Edge { edge, outer: true },
                )
                .unwrap();
        }
        let group = initial.layout.panel_group(Panel::Brushes).unwrap();
        initial
            .layout
            .set_panel_visible(Panel::Color, true)
            .unwrap();
        initial
            .layout
            .move_panel(
                viewport,
                Panel::Color,
                DockTarget::Split {
                    group,
                    edge: Edge::Bottom,
                },
            )
            .unwrap();
        initial
            .layout
            .set_column_collapsed(group, true, viewport)
            .unwrap();
        let column = initial.layout.collapsed_column_for_group(group).unwrap();
        w.dispatch(UiAction::RestoreWorkspace { workspace: initial });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        let c = w
            .resolved()
            .collapsed
            .into_iter()
            .find(|c| c.id == column)
            .unwrap();
        let empty = [
            c.bounds.x + c.bounds.width * 0.5,
            (c.empty.y + 50.).min(c.grip.y - 10.),
        ];
        perform(
            serde_json::json!([{"point":empty},{"button":273,"down":true},{"button":273,"down":false}]),
        );
        assert!(
            find_css(w.surface.upcast_ref(), "panel-context-menu").is_some_and(|p| p.is_visible()),
            "empty column context menu"
        );
        let menu = find_css(w.surface.upcast_ref(), "panel-context-menu")
            .unwrap()
            .downcast::<gtk::PopoverMenu>()
            .unwrap();
        assert_eq!(menu.menu_model().unwrap().n_items(), 4);
        let label = text_widget(
            find_css(w.surface.upcast_ref(), "panel-context-menu")
                .unwrap()
                .upcast_ref(),
            "Group panel",
        )
        .unwrap();
        let b = label.compute_bounds(&w.surface).unwrap();
        perform(click([b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]));
        assert_eq!(
            state(&w).workspace.layout.column_settings(column).mode,
            ColumnMode::GroupPanel
        );
        assert!(find_css(w.surface.upcast_ref(), "group-panel-strip").is_none());
        capture_reference(
            &w,
            output.join(format!("column-closed-{theme:?}.png")).to_str().unwrap(),
            1.,
        );
        let c = w
            .resolved()
            .collapsed
            .into_iter()
            .find(|c| c.id == column)
            .unwrap();
        let icon = c.groups.iter().find(|g| g.group == group).unwrap().icons[0].bounds;
        perform(click(center(icon)));
        let p = w
            .resolved()
            .collapsed
            .into_iter()
            .find(|c| c.id == column)
            .unwrap()
            .group_panel
            .unwrap();
        assert_eq!(
            p.panels.len(),
            state(&w)
                .workspace
                .layout
                .group_panels(group)
                .unwrap()
                .len()
        );
        assert!(p.panels.len() >= 2);
        assert!(find_css(w.surface.upcast_ref(), "active-column-group").is_some());
        let strip = find_css(w.surface.upcast_ref(), "group-panel-strip").unwrap();
        let strip_bounds = strip.compute_bounds(&w.surface).unwrap();
        let body = find_named(w.surface.upcast_ref(), &format!("column-drawer-{column}")).unwrap();
        let body_bounds = body.compute_bounds(&w.surface).unwrap();
        let (gap, source_class, corners) = if edge == Edge::Left {
            (
                body_bounds.x() - strip_bounds.x() - strip_bounds.width(),
                "group-opens-right",
                ["join-nw", "join-sw"],
            )
        } else {
            (
                strip_bounds.x() - body_bounds.x() - body_bounds.width(),
                "group-opens-left",
                ["join-ne", "join-se"],
            )
        };
        assert!(gap.abs() < 1., "exposed canvas at attached seam: {gap}");
        assert!(strip.has_css_class(source_class));
        assert!(corners.iter().all(|class| body.has_css_class(class)));
        capture_reference(
            &w,
            output
                .join(format!("group-panel-{theme:?}.png"))
                .to_str()
                .unwrap(),
            1.,
        );
        for touch in [false, true] {
            for after in [None, Some(Panel::Brushes)] {
                resize::measure(&w, &mut perform, column, after, (touch, false), &output, theme);
            }
            resize::measure(&w, &mut perform, column, None, (touch, true), &output, theme);
        }
        let saved_width = state(&w)
            .workspace
            .layout
            .column_settings(column)
            .width
            .unwrap();
        let p = w
            .resolved()
            .collapsed
            .into_iter()
            .find(|c| c.id == column)
            .unwrap()
            .group_panel
            .unwrap();
        let split = center(p.dividers[0]);
        perform(
            serde_json::json!([{"touch":"down","point":split},{"touch":"move","point":[split[0],split[1]+65.]},{"touch":"up"}]),
        );
        let saved = state(&w).workspace.layout.column_settings(column);
        assert!(!saved.heights.is_empty(), "touch split resize");
        w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace }); pump(100);
        assert!(find_named(w.surface.upcast_ref(), &format!("column-drawer-{column}")).is_some());
        assert_ne!(state(&w).workspace.layout.column_settings(column).heights, saved.heights);
        w.dispatch(UiAction::Invoke { command: CommandId::RedoWorkspace }); pump(100);
        assert_eq!(state(&w).workspace.layout.column_settings(column).heights, saved.heights);
        assert!(find_named(w.surface.upcast_ref(), &format!("column-drawer-{column}")).is_some());
        perform(click(center(icon)));
        assert!(
            w.resolved()
                .collapsed
                .iter()
                .find(|c| c.id == column)
                .unwrap()
                .group_panel
                .is_none()
        );
        assert!(find_css(w.surface.upcast_ref(), "group-panel-strip").is_none());
        perform(click(center(icon)));
        let restored = state(&w).workspace.layout.column_settings(column);
        assert_eq!(restored.width, Some(saved_width));
        assert_eq!(restored.heights, saved.heights);
        let c = w
            .resolved()
            .collapsed
            .into_iter()
            .find(|c| c.id == column)
            .unwrap();
        let other = c.groups.iter().find(|g| g.group != group).unwrap();
        perform(click(center(other.icons[0].bounds)));
        let swapped = w
            .resolved()
            .collapsed
            .into_iter()
            .find(|c| c.id == column)
            .unwrap()
            .group_panel
            .unwrap();
        assert_eq!(swapped.group, other.group);
        assert_eq!(swapped.panels.len(), other.icons.len());
        assert!((swapped.bounds.width - saved_width).abs() < 1.);
        perform(click(center(icon)));
        assert_eq!(
            state(&w).workspace.layout.column_settings(column).heights,
            saved.heights
        );
        // Periodic cleanup runs against the real private SQLite store. It must
        // never reload the active workspace or discard an explicitly open panel.
        for mode in [ColumnMode::Drawers, ColumnMode::GroupPanel] {
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetColumnMode { column, mode },
            });
            pump(300);
            perform(click(center(w.resolved().work_area)));
            let before = state(&w);
            let root = find_named(w.surface.upcast_ref(), &format!("column-drawer-{column}")).unwrap();
            let refreshes = w.publication.refreshes.get();
            glib::MainContext::default().block_on(w.workspaces.maintain_storage(&w)).unwrap();
            pump(400);
            assert_eq!(state(&w).workspace, before.workspace, "maintenance changed open column");
            assert_eq!(
                serde_json::to_value(state(&w).customization).unwrap(),
                serde_json::to_value(before.customization).unwrap(),
                "maintenance closed projection"
            );
            assert_eq!(w.publication.refreshes.get(), refreshes, "maintenance rebuilt UI");
            assert_eq!(root, find_named(w.surface.upcast_ref(), &format!("column-drawer-{column}")).unwrap());
        }
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetColumnAutoHide {
                column,
                auto_hide: true,
            },
        });
        pump(100);
        perform(click(center(w.resolved().work_area)));
        assert!(
            state(&w).customization.column_drawers.is_empty(),
            "outside mouse dismisses group panel"
        );
        perform(serde_json::json!([{"point":empty},{"down":true}]));
        pump(700);
        assert!(
            !find_css(w.surface.upcast_ref(), "panel-context-menu").is_some_and(|p| p.is_visible()),
            "mouse holds do not open menus"
        );
        perform(serde_json::json!([{"down":false}]));
        // A touch hold on unused strip space opens the same menu.
        perform(serde_json::json!([{"touch":"down","point":empty}]));
        pump(700);
        assert!(
            find_css(w.surface.upcast_ref(), "panel-context-menu").is_some_and(|p| p.is_visible())
        );
        perform(serde_json::json!([{"touch":"up"}]));
        capture_popover(
            menu.upcast_ref(),
            output
                .join(format!("column-menu-{theme:?}.png"))
                .to_str()
                .unwrap(),
        );
        w.dismiss_context();
        pump(100);
    }
    std::fs::write(dir.join("finished"), "done").unwrap();
    w.window.destroy();
    pump(100);
}
