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
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1400);
    let ready_deadline = Instant::now() + Duration::from_secs(20);
    while !w.workspaces.ready.get() || w.workspaces.busy.get() {
        assert!(
            Instant::now() < ready_deadline,
            "workspace startup did not finish"
        );
        pump(20);
    }
    let mut step = 0;
    // Exercise live theme changes on the same editor. Workspace persistence and
    // opening another editor have separate integration tests.
    for theme in [Theme::Dark, Theme::Light] {
        check_theme(&w, theme, &dir, &output, &mut step);
    }
    std::fs::write(dir.join("finished"), "done").unwrap();
    w.window.destroy();
    pump(100);
}

fn check_theme(
    w: &Rc<Workspace>,
    theme: Theme,
    dir: &std::path::Path,
    output: &std::path::Path,
    step: &mut usize,
) {
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
        let ready_deadline = Instant::now() + Duration::from_secs(5);
        while !w.workspaces.accepts_input(&w) {
            assert!(
                Instant::now() < ready_deadline,
                "workspace ownership did not settle"
            );
            pump(10);
        }
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
            let drawer = state(&w).customization.drawer.unwrap_or_else(|| panic!(
                "{theme:?}/{edge:?}/{id}: drawer did not open; ready={}, busy={}, sensitive={}, error={:?}",
                w.workspaces.ready.get(), w.workspaces.busy.get(), w.surface.is_sensitive(), state(&w).host_error
            ));
            assert!(
                synchronous_origin.get(),
                "{theme:?}/{edge:?}/{id}: opener styling changes before the click returns"
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
            let texture = crate::snapshot(&w);
            let mut download = gdk::TextureDownloader::new(&texture);
            download.set_format(gdk::MemoryFormat::R8g8b8a8);
            let (pixels, stride) = download.download_bytes();
            for other in std::iter::once(id).chain(ids[..3].iter().copied()) {
                let bounds = button(other).compute_bounds(&w.window).unwrap();
                // Sample inside the tile, away from its glyph and rounded edge.
                let offset = (bounds.y() as usize + 8) * stride + (bounds.x() as usize + 8) * 4;
                let rgb = &pixels[offset..offset + 3];
                let blue = i16::from(rgb[2]) - i16::from(rgb[0]) > 12
                    && i16::from(rgb[2]) - i16::from(rgb[1]) > 6;
                assert_eq!(
                    blue,
                    other == id,
                    "{theme:?}/{edge:?}: tile {other} active blue follows the drawer: {rgb:?}"
                );
            }
            texture
                .save_to_png(output.join(format!("{theme:?}-{edge:?}-{id}.png")))
                .unwrap();
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
        let drawer = w.drawers().into_iter().find(|d| d.id == column.id).unwrap();
        check_connected_pixels(
            &w,
            &drawer,
            &opener,
            &output.join(format!("{theme:?}-column-{}.png", column.id)),
        );
        if let Some(icon) = column.groups[0].icons.get(1) {
            let tab = find_named(
                w.surface.upcast_ref(),
                &format!("column-drawer-tab-{:?}", icon.panel),
            )
            .unwrap();
            let target = center(&tab);
            perform(
                serde_json::json!([{"point":target},{"down":true},{"down":false},{"point":[800,700]}]),
            );
            let next = w.columns.button(column.id, icon.panel).unwrap();
            assert!(!opener.has_css_class("selected-tool"));
            assert!(next.has_css_class("selected-tool"));
            check_connected_pixels(
                &w,
                &drawer,
                &next,
                &output.join(format!("{theme:?}-column-{}-tab-switch.png", column.id)),
            );
            // The former opener switches back; only the current opener closes.
            perform(
                serde_json::json!([{"point":p},{"down":true},{"down":false},{"point":[800,700]}]),
            );
            assert!(opener.has_css_class("selected-tool"));
            check_connected_pixels(
                &w,
                &drawer,
                &opener,
                &output.join(format!("{theme:?}-column-{}-switch-back.png", column.id)),
            );
        }
        perform(serde_json::json!([{"point":p},{"down":true},{"down":false},{"point":[800,700]}]));
        assert!(!opener.has_css_class("selected-tool"));
        assert!(classes.iter().all(|class| !opener.has_css_class(class)));
    }
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Toolbar,
        target: DockTarget::Tab {
            group: 5,
            index: None,
        },
        viewport,
    });
    pump(300);
    let column = state(&w).workspace.layout.column_for_group(5).unwrap();
    let opener = w.columns.button(column, Panel::Toolbar).unwrap();
    let p = center(opener.upcast_ref());
    perform(serde_json::json!([{"point":p},{"down":true},{"down":false},{"point":[800,700]}]));
    // Start furthest from the sidebar so each next opener remains exposed.
    for id in [ids[2], ids[1], ids[0]] {
        let anchor = TileAnchor {
            panel: Panel::Toolbar,
            tile: id,
        };
        let opener = w
            .drawers()
            .iter()
            .find_map(|d| d.tile_button(anchor))
            .unwrap();
        let p = center(opener.upcast_ref());
        perform(serde_json::json!([{"point":p},{"down":true},{"down":false},{"point":[800,700]}]));
        check_connected_pixels(
            &w,
            &w.drawer,
            &opener,
            &output.join(format!("{theme:?}-nested-{id}.png")),
        );
    }
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::CloseExpanded,
    });
    pump(250);
}

fn check_connected_pixels(
    w: &Workspace,
    drawer: &drawers::Drawer,
    opener: &gtk::Button,
    path: &std::path::Path,
) {
    let placement = drawer.placement().unwrap();
    let connection = placement
        .connection()
        .expect("the drawer must connect to its current opener");
    let b = opener.compute_bounds(&w.surface).unwrap();
    assert!(
        (b.x() - placement.anchor.x).abs() <= 1. && (b.y() - placement.anchor.y).abs() <= 1.,
        "connector tracks the visible selected tile: native {b:?}, shared {:?}, model {:?} ({path:?})",
        placement.anchor,
        state(w).customization.drawer
    );
    let bounds = Bounds {
        x: b.x(),
        y: b.y(),
        width: b.width(),
        height: b.height(),
    };
    let corners = placement.source_corners(bounds);
    let a = opener.compute_bounds(&w.window).unwrap();
    let points = [
        [a.x() + 2., a.y() + a.height() * 0.5],
        [a.x() + a.width() - 2., a.y() + a.height() * 0.5],
        [a.x() + a.width() * 0.5, a.y() + 2.],
        [a.x() + a.width() * 0.5, a.y() + a.height() - 2.],
        [a.x() + 1., a.y() + 1.],
        [a.x() + a.width() - 2., a.y() + 1.],
        [a.x() + a.width() - 2., a.y() + a.height() - 2.],
        [a.x() + 1., a.y() + a.height() - 2.],
        [
            connection.bounds.x + connection.bounds.width * 0.5,
            connection.bounds.y + connection.bounds.height * 0.5,
        ],
    ];
    let sample = |texture: &gdk::Texture| {
        let mut download = gdk::TextureDownloader::new(texture);
        download.set_format(gdk::MemoryFormat::R8g8b8a8);
        let (bytes, stride) = download.download_bytes();
        points.map(|[x, y]| {
            let i = y.floor() as usize * stride + x.floor() as usize * 4;
            [bytes[i], bytes[i + 1], bytes[i + 2]]
        })
    };
    let texture = crate::snapshot(w);
    texture.save_to_png(path).unwrap();
    let pixels = sample(&texture);
    let fill = pixels[match placement.direction {
        Edge::Left => 0,
        Edge::Right => 1,
        Edge::Top => 2,
        Edge::Bottom => 3,
    }];
    assert!(
        i16::from(fill[2]) - i16::from(fill[0]) > 12,
        "active tile is blue: {fill:?} ({path:?})"
    );
    for (joined, corner) in corners.into_iter().zip(&pixels[4..8]) {
        if joined {
            assert!(
                corner.iter().zip(fill).all(|(a, b)| a.abs_diff(b) <= 3),
                "square tile corner must survive ancestor clipping: {corner:?} vs {fill:?} ({path:?})"
            );
        }
    }
    let bridge = pixels[8];
    assert!(
        bridge.iter().all(|v| v.abs_diff(bridge[0]) <= 3),
        "connector has the neutral drawer fill: {bridge:?}"
    );
    let provider = gtk::CssProvider::new();
    provider.load_from_string(".drawer-shadow { box-shadow: none; }");
    gtk::style_context_add_provider_for_display(
        &w.surface.display(),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
    );
    pump(80);
    let flat = sample(&crate::snapshot(w));
    gtk::style_context_remove_provider_for_display(&w.surface.display(), &provider);
    assert_eq!(
        pixels, flat,
        "drawer shadows must not darken the opener or connector ({path:?})"
    );
    pump(80);
}
