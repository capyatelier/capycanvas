use super::*;

#[test]
#[ignore = "isolated Mutter remote-input driver: --drawer-style"]
fn native_drawer_style_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let output = std::path::PathBuf::from(
        std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| dir.to_string_lossy().into()),
    );
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.DrawerStyle");
    let mut step = 0;
    // Each window completes its theme matrix before periodic workspace storage
    // maintenance temporarily disables editor input. Storage has its own tests.
    for theme in [Theme::Dark, Theme::Light] {
        check_theme(&app, theme, &dir, &output, &mut step);
    }
    std::fs::write(dir.join("finished"), "done").unwrap();
}

fn check_theme(
    app: &adw::Application,
    theme: Theme,
    dir: &std::path::Path,
    output: &std::path::Path,
    step: &mut usize,
) {
    let w = fixture_workspace(app);
    w.window.maximize();
    w.window.present();
    pump(1400);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut initial = layer_ui::WorkspaceState::default();
    let old = initial
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()
        .to_vec();
    for tile in old {
        initial.layout.remove_tool(Panel::Toolbar, tile.id).unwrap();
    }
    initial
        .layout
        .insert_tools(
            Panel::Toolbar,
            None,
            &[
                ToolbarControl::Panel {
                    panel: Panel::Brushes,
                },
                ToolbarControl::Panel {
                    panel: Panel::Sizes,
                },
                ToolbarControl::Panel {
                    panel: Panel::Brushes,
                },
                ToolbarControl::Divider,
                ToolbarControl::Command {
                    command: CommandId::Pen,
                },
                ToolbarControl::Command {
                    command: CommandId::Pencil,
                },
                ToolbarControl::Command {
                    command: CommandId::Brush,
                },
            ],
        )
        .unwrap();
    let ids = initial
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()
        .iter()
        .map(|t| t.id)
        .collect::<Vec<_>>();
    if *step == 0 {
        std::fs::write(dir.join("ready"), "ready").unwrap();
    }
    let mut perform = |events: serde_json::Value| {
        std::fs::write(
            dir.join(format!("step-{step}.json")),
            serde_json::to_vec(&events).unwrap(),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.join(format!("done-{step}")).exists() && Instant::now() < deadline {
            pump(10);
        }
        assert!(
            dir.join(format!("done-{step}")).exists(),
            "native pointer timed out"
        );
        *step += 1;
        pump(280);
    };
    let button = |id| {
        w.customization
            .drawer_button(TileAnchor {
                panel: Panel::Toolbar,
                tile: id,
            })
            .unwrap()
    };
    let center = |widget: &gtk::Widget| {
        let b = widget.compute_bounds(&w.surface).unwrap();
        [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]
    };
    let classes = [
        "drawer-origin-top",
        "drawer-origin-bottom",
        "drawer-origin-left",
        "drawer-origin-right",
    ];
    w.dispatch(UiAction::SetTheme { theme: Some(theme) });
    for edge in [Edge::Top, Edge::Left, Edge::Bottom, Edge::Right] {
        let mut workspace = initial.clone();
        workspace
            .layout
            .move_panel(
                viewport,
                Panel::Toolbar,
                DockTarget::Edge { edge, outer: true },
            )
            .unwrap();
        w.dispatch(UiAction::RestoreWorkspace { workspace });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(300);
        perform(serde_json::json!([{"point":[800,700]}]));
        for id in &ids[..3] {
            assert!(!button(*id).has_css_class("selected-tool"));
        }
        let synchronous_origin = Rc::new(Cell::new(true));
        for id in &ids {
            let valid = synchronous_origin.clone();
            button(*id).connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| {
                    if let Some(anchor) =
                        state(&w).customization.drawer.and_then(|d| d.anchor.tile())
                    {
                        let b = w.customization.drawer_button(anchor).unwrap();
                        valid
                            .set(valid.get() && classes.iter().any(|class| b.has_css_class(class)));
                    }
                }
            ));
        }
        // Switch to a different body, then to the same body at another opener.
        for id in [ids[0], ids[1], ids[2], ids[4], ids[5], ids[6], ids[0]] {
            let p = center(button(id).upcast_ref());
            perform(
                serde_json::json!([{"point":p},{"down":true},{"down":false},{"point":[800,700]}]),
            );
            let drawer = state(&w).customization.drawer.unwrap();
            assert!(
                synchronous_origin.get(),
                "opener styling changes before the click returns"
            );
            if let Some(control) = state(&w)
                .workspace
                .layout
                .panel(Panel::Toolbar)
                .unwrap()
                .tiles()
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.control)
                && let ToolbarControl::Command { command } = control
            {
                assert!(
                    state(&w)
                        .commands
                        .iter()
                        .find(|c| c.id == command)
                        .unwrap()
                        .selected,
                    "the first click also activates the tool"
                );
            }
            assert_eq!(
                drawer.anchor.tile(),
                Some(TileAnchor {
                    panel: Panel::Toolbar,
                    tile: id
                })
            );
            let placement = w.drawer.placement().unwrap();
            let b = button(id).compute_bounds(&w.surface).unwrap();
            assert!(
                (placement.anchor.x - b.x()).abs() <= 1.
                    && (placement.anchor.y - b.y()).abs() <= 1.,
                "{theme:?}/{edge:?}: connector follows native opener"
            );
            assert!(placement.connection().is_some());
            let expected = match placement.direction {
                Edge::Top => classes[0],
                Edge::Bottom => classes[1],
                Edge::Left => classes[2],
                Edge::Right => classes[3],
            };
            for other in &ids {
                for class in classes {
                    assert_eq!(
                        button(*other).has_css_class(class),
                        *other == id && class == expected,
                        "{theme:?}/{edge:?}: tile {other} class {class}"
                    );
                }
            }
            capture_reference(
                &w,
                &output
                    .join(format!("{theme:?}-{edge:?}-{id}.png"))
                    .to_string_lossy(),
                1.,
            );
        }
        // A press on another eligible tool preserves the drawer until
        // activation. Releasing outside the button cancels that switch.
        let before = state(&w).customization.drawer;
        let p = center(button(ids[5]).upcast_ref());
        perform(serde_json::json!([{"point":p},{"down":true}]));
        assert_eq!(state(&w).customization.drawer, before);
        perform(serde_json::json!([{"point":[800,20]},{"down":false}]));
        assert_eq!(state(&w).customization.drawer, before);
        let p = center(button(ids[0]).upcast_ref());
        perform(serde_json::json!([{"point":p},{"down":true},{"down":false},{"point":[800,700]}]));
        assert!(state(&w).customization.drawer.is_none());
        for id in &ids[..3] {
            for class in classes {
                assert!(!button(*id).has_css_class(class));
            }
        }
    }
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: initial.clone(),
    });
    w.dispatch(UiAction::DoubleClickPanelHandle { group: 5, viewport });
    w.dispatch(UiAction::DoubleClickPanelHandle { group: 8, viewport });
    pump(300);
    capture_reference(
        &w,
        &output
            .join(format!("collapsed-idle-{theme:?}.png"))
            .to_string_lossy(),
        1.,
    );
    for column in &w.resolved().collapsed {
        let root = find_named(
            w.surface.upcast_ref(),
            &format!("collapsed-column-{}", column.id),
        )
        .unwrap();
        fn separators(root: &gtk::Widget, lines: &mut Vec<gtk::Widget>) {
            if root.is::<gtk::Separator>() {
                lines.push(root.clone());
            }
            let mut child = root.first_child();
            while let Some(widget) = child {
                separators(&widget, lines);
                child = widget.next_sibling();
            }
        }
        let mut lines = Vec::new();
        separators(&root, &mut lines);
        assert_eq!(
            lines.len(),
            column.groups.len(),
            "divider above first group and between groups"
        );
        for (line, group) in lines.iter().zip(&column.groups) {
            let b = line.compute_bounds(&w.surface).unwrap();
            assert_eq!(
                [b.width(), b.height()],
                [18., 1.],
                "same native separator as vertical toolbar"
            );
            assert!(b.y() + b.height() <= group.bounds.y);
        }
        for icon in column.groups.iter().flat_map(|g| &g.icons) {
            let b = w.columns.button(column.id, icon.panel).unwrap();
            assert!(
                !b.has_css_class("selected-tool"),
                "idle collapsed {:?} is neutral",
                icon.panel
            );
            let bounds = b.compute_bounds(&w.surface).unwrap();
            assert!(
                (bounds.y() - icon.bounds.y).abs() <= 1.,
                "separator spacing preserves shared hit slots"
            );
        }
        let opener = w
            .columns
            .button(column.id, column.groups[0].icons[0].panel)
            .unwrap();
        let p = center(opener.upcast_ref());
        perform(serde_json::json!([{"point":p},{"down":true},{"down":false},{"point":[800,700]}]));
        assert!(
            opener.has_css_class("selected-tool"),
            "{theme:?}/column {}: open drawer is selected",
            column.id
        );
        perform(serde_json::json!([{"point":p},{"down":true},{"down":false},{"point":[800,700]}]));
        assert!(!opener.has_css_class("selected-tool"));
    }
    w.window.destroy();
    pump(100);
}
