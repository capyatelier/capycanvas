//! Compositor-delivered input and screenshots of the retained palette surfaces.
use super::*;

fn capture_palette(d: &mut Driver, name: &str) {
    d.capture_canvas(name);
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
        d.perform(serde_json::json!([{"capture":name.trim_end_matches(".png").to_lowercase()}]));
    }
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_palette_reorder_input"]
fn native_palette_reorder_input() {
    check_palette_reorder(&["mouse", "touch"]);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_palette_reorder_pen_input --tablet"]
fn native_palette_reorder_pen_input() {
    check_palette_reorder(&["pen"]);
}

fn check_palette_reorder(devices: &[&str]) {
    let mut d = Driver::new("art.capycanvas.PaletteReorder");
    let event = |device: &str, phase: &str, point: [f32; 2]| match device {
        "touch" => serde_json::json!({"touch":phase,"point":point}),
        "pen" => serde_json::json!({"pen":phase,"point":point}),
        _ => match phase {
            "down" => serde_json::json!({"point":point,"down":true}),
            "up" => serde_json::json!({"point":point,"down":false}),
            _ => serde_json::json!({"point":point}),
        },
    };
    for surface in ["drawer", "dock", "float"] {
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState {
                layout: if surface == "drawer" {
                    WorkspacePreset::Painter
                } else {
                    WorkspacePreset::Illustrator
                }
                .layout(Platform::Gtk),
                ..Default::default()
            }),
        });
        pump(300);
        if surface == "drawer" {
            d.click_name(&d.header_tool(ToolbarControl::Color));
        } else if surface == "float" {
            d.w.dispatch(UiAction::MovePanel {
                panel: Panel::Palettes,
                target: DockTarget::Float {
                    position: [400., 180.],
                },
                viewport: [1600., 1000.],
            });
        } else {
            let group = state(&d.w)
                .workspace
                .layout
                .panel_group(Panel::Palettes)
                .unwrap();
            d.w.dispatch(UiAction::SelectPanelTab {
                group,
                panel: Panel::Palettes,
            });
        }
        pump(350);
        for device in devices {
            let palette = state(&d.w).colors.library.active_palette().clone();
            let original = palette.swatches.clone();
            let tile = d.named(&format!("palette-swatch-{}", original[0].id));
            let target = d.named(&format!("palette-swatch-{}", original[8].id));
            d.click_name(&tile.widget_name());
            let start = d.point(&tile);
            let mut end = d.point(&target);
            end[0] += 10.;
            let grid = tile
                .parent()
                .unwrap()
                .downcast::<crate::palette_grid::PaletteGrid>()
                .unwrap();
            let neighbor = d.named(&format!("palette-swatch-{}", original[1].id));
            let original_neighbor = grid.visual_bounds(&neighbor).unwrap();
            let original_source = grid.visual_bounds(&tile).unwrap();
            let colors = |d: &Driver| state(&d.w).colors.library.active_palette().swatches.clone();
            // Tiny motion remains a tap; dragging needs movement slop, not a hold.
            let jitter = [start[0] + 2., start[1] + 2.];
            d.perform(serde_json::json!([
                event(device, "down", start),
                event(device, "move", jitter),
                event(device, "up", jitter)
            ]));
            assert_eq!(colors(&d), original, "{surface} {device}: movement slop");
            let current = state(&d.w).colors.definition();
            // Immediate pickup and dragging out of a held menu use the same contact.
            for (hold_ms, cancel) in [(0, true), (850, true), (0, false)] {
                d.perform(serde_json::json!([event(device, "down", start), {"wait_ms":hold_ms}]));
                if *device == "mouse" {
                    assert_eq!(
                        tile.cursor().and_then(|c| c.name()).as_deref(),
                        Some("grab")
                    );
                    assert!(
                        !d.w.popovers
                            .borrow()
                            .iter()
                            .filter_map(|p| p.upgrade())
                            .any(|p| p.is_visible())
                    );
                }
                let frames = Rc::new(std::cell::RefCell::new(Vec::new()));
                let observed = frames.clone();
                let watched = neighbor.clone();
                let tick = grid.add_tick_callback(move |grid, _| {
                    observed
                        .borrow_mut()
                        .push(grid.visual_bounds(&watched).unwrap().x());
                    glib::ControlFlow::Continue
                });
                let motion: Vec<_> = (1..=12)
                    .map(|i| {
                        event(
                            device,
                            "move",
                            [
                                start[0] + (end[0] - start[0]) * i as f32 / 12.,
                                start[1] + (end[1] - start[1]) * i as f32 / 12.,
                            ],
                        )
                    })
                    .collect();
                d.perform(serde_json::json!(motion));
                assert!(
                    tile.has_css_class("palette-drag-source"),
                    "{surface} {device}: dragging after {hold_ms}ms"
                );
                pump(180);
                tick.remove();
                let moved_neighbor = grid.visual_bounds(&neighbor).unwrap();
                assert!(
                    (moved_neighbor.x() - original_source.x()).abs() < 1.,
                    "{surface} {device}: neighbor slides into the vacant slot"
                );
                if gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations()) {
                    assert!(
                        frames
                            .borrow()
                            .iter()
                            .any(|x| *x > original_source.x() + 1.
                                && *x < original_neighbor.x() - 1.),
                        "neighbor must slide through intermediate positions"
                    );
                }
                let pointer =
                    d.w.window
                        .compute_point(&d.w.surface, &gtk::graphene::Point::new(end[0], end[1]))
                        .unwrap();
                let lifted =
                    d.w.surface
                        .drag_overlay_position()
                        .expect("lifted swatch above the panel");
                assert!((lifted.x() + tile.width() as f32 * 0.5 - pointer.x()).abs() < 1.);
                assert!((lifted.y() + tile.height() as f32 * 0.5 - pointer.y()).abs() < 1.);
                // Dwelling over an animated neighbor cannot cause oscillation.
                d.perform(serde_json::json!([event(device, "move", end), {"wait_ms":180}]));
                assert_eq!(grid.visual_bounds(&neighbor), Some(moved_neighbor));
                assert_eq!(colors(&d), original, "preview never edits the palette");
                assert!(
                    !d.w.popovers
                        .borrow()
                        .iter()
                        .filter_map(|p| p.upgrade())
                        .any(|p| p.is_visible())
                );
                if !cancel && *device == "mouse" && surface == "dock" {
                    capture_palette(&mut d, "palette-reorder-drag.png");
                }
                let outside = cancel && hold_ms == 0;
                let release = if outside { [800., 500.] } else { end };
                if outside {
                    d.perform(serde_json::json!([event(device, "move", release), {"wait_ms":180}]));
                    assert!(
                        d.w.surface.drag_overlay_position().is_some(),
                        "lifted swatch follows outside the panel"
                    );
                    assert_eq!(grid.visual_bounds(&neighbor), Some(original_neighbor));
                } else if cancel {
                    d.key(0xff1b);
                }
                d.perform(serde_json::json!([event(device, "up", release)]));
                if *device == "pen" {
                    d.perform(serde_json::json!([{"pen":"leave"}]));
                }
                assert!(!tile.has_css_class("palette-drag-source"));
                assert!(!grid.is_reordering());
                assert!(d.w.surface.drag_overlay_position().is_none());
                assert_eq!(
                    state(&d.w).colors.definition(),
                    current,
                    "reordering never selects a color"
                );
                if cancel {
                    assert_eq!(colors(&d), original);
                }
            }
            let mut expected = original.clone();
            let moved = expected.remove(0);
            expected.insert(8, moved);
            assert_eq!(
                colors(&d),
                expected,
                "{surface} {device}: one move on release"
            );
            assert_eq!(
                d.named(&format!("palette-swatch-{}", original[0].id)),
                tile,
                "retained tile"
            );
            assert!(tile.grab_focus());
            for (key, expected) in [(0x7a, &original), (0x79, &expected)] {
                d.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":key,"down":true},{"key":key,"down":false},{"key":0xffe3,"down":false}]));
                assert_eq!(
                    &colors(&d),
                    expected,
                    "{surface} {device}: keyboard undo/redo"
                );
            }
            d.w.dispatch(UiAction::Color {
                action: layer_ui::ColorAction::Library {
                    action: layer_ui::ColorLibraryAction::UndoReorder {
                        palette: palette.id,
                    },
                },
            });
            pump(200); // Let the retained grid allocate the undone order before the next device.
            assert_eq!(colors(&d), original);
            assert!(state(&d.w).colors.library.history.is_empty());
        }
        capture_palette(&mut d, &format!("palette-reorder-{surface}.png"));
    }
    // Long grids scroll from their gutters, and edge scrolling follows a drag.
    // Removing its source cancels atomically.
    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(WorkspaceState {
            layout: WorkspacePreset::Painter.layout(Platform::Gtk),
            ..Default::default()
        }),
    });
    pump(250);
    d.click_name(&d.header_tool(ToolbarControl::Color));
    for device in devices {
        d.w.dispatch(UiAction::Color {
            action: layer_ui::ColorAction::Library {
                action: layer_ui::ColorLibraryAction::Import {
                    name: "Reorder scroll".into(),
                    swatches: (0..36)
                        .map(|i| (format!("Gray {i}"), layer_core::color::RgbColor::BLACK))
                        .collect(),
                },
            },
        });
        pump(250);
        let original = state(&d.w).colors.library.active_palette().swatches.clone();
        let tile = d.named(&format!("palette-swatch-{}", original[0].id));
        let scroll = tile
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>()
            .unwrap();
        if *device != "mouse" {
            let mut p = d.point(scroll.upcast_ref());
            p[0] = d.point(&tile)[0] + tile.width() as f32 * 0.5 + 2.;
            d.perform(serde_json::json!([
                event(device, "down", p),
                event(device, "move", [p[0], p[1] - 70.]),
                event(device, "up", [p[0], p[1] - 70.])
            ]));
            assert!(
                scroll.vadjustment().value() > 0.,
                "{device}: scroll from gutter"
            );
            assert_eq!(
                state(&d.w).colors.library.active_palette().swatches,
                original
            );
            scroll.vadjustment().set_value(0.);
            pump(350);
        }
        let start = d.point(&tile);
        let b = scroll.compute_bounds(&d.w.window).unwrap();
        let edge = [b.x() + 60., b.y() + b.height() - 8.];
        d.perform(serde_json::json!([event(device, "down", start), event(device, "move", edge), {"wait_ms":500}]));
        assert!(
            tile.has_css_class("palette-drag-source"),
            "{device}: retained scrolled source"
        );
        assert!(
            scroll.vadjustment().value() > 0.,
            "{device}: edge hover scrolls"
        );
        d.w.dispatch(UiAction::Color {
            action: layer_ui::ColorAction::Library {
                action: layer_ui::ColorLibraryAction::Remove { id: original[0].id },
            },
        });
        assert!(!tile.has_css_class("palette-drag-source"));
        assert!(d.w.surface.drag_overlay_position().is_none());
        d.perform(serde_json::json!([event(device, "up", edge)]));
        if *device == "pen" {
            d.perform(serde_json::json!([{"pen":"leave"}]));
        }
        assert_eq!(
            state(&d.w).colors.library.active_palette().swatches,
            original[1..]
        );
        assert!(state(&d.w).colors.library.history.is_empty());
    }
    d.w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_palette_pen_input --tablet"]
fn native_palette_pen_input() {
    use layer_core::color::{RgbColor, RgbSpace};
    use layer_ui::{ColorAction, ColorLibraryAction};
    let mut d = Driver::new("art.capycanvas.PalettePenReview");
    let swatches = (0..36)
        .map(|i| {
            (
                format!("Study {}", i + 1),
                RgbColor::new(RgbSpace::Srgb, [i as f32 / 36., 0.3, 0.6, 1.]).unwrap(),
            )
        })
        .collect();
    d.w.dispatch(UiAction::Color {
        action: ColorAction::Library {
            action: ColorLibraryAction::Import {
                name: "Study".into(),
                swatches,
            },
        },
    });
    d.click_name(&d.header_tool(ToolbarControl::Color));
    let palette = state(&d.w).colors.library.active_palette().clone();
    let tile = d.named(&format!("palette-swatch-{}", palette.swatches[0].id));
    let point = d.point(&tile);
    d.perform(serde_json::json!([{"pen":"down","point":point},{"pen":"up"},{"pen":"leave"}]));
    assert_eq!(state(&d.w).colors.definition(), palette.swatches[0].color);
    assert!(state(&d.w).colors.library.history.is_empty());
    let next = d.named(&format!("palette-swatch-{}", palette.swatches[1].id));
    let point = d.point(&next);
    d.perform(serde_json::json!([{"pen":"down","point":point},{"wait_ms":800},{"pen":"up"},{"pen":"leave"}]));
    // The proxy's synthetic serial cannot authorize a compositor popup grab.
    // Check pen recognition and click suppression here; the mouse/touch journey
    // below validates actual popup mapping, keyboard input and dismissal.
    let menu =
        d.w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .find(|p| p.is_visible() && p.widget_name() == "palette-context-menu")
            .expect("pen hold opens the swatch menu");
    assert_eq!(
        state(&d.w).colors.definition(),
        palette.swatches[0].color,
        "hold does not select"
    );
    menu.popdown();
    pump(100);
    assert!(state(&d.w).customization.drawer.is_some());
    // The gap between tiles remains available for ordinary pen scrolling.
    let scroll = tile
        .ancestor(gtk::ScrolledWindow::static_type())
        .unwrap()
        .downcast::<gtk::ScrolledWindow>()
        .unwrap();
    assert!(scroll.height() <= 172);
    let mut p = d.point(scroll.upcast_ref());
    p[0] = d.point(&tile)[0] + tile.width() as f32 * 0.5 + 2.;
    d.perform(serde_json::json!([{"pen":"down","point":p},{"pen":"move","point":[p[0],p[1]-72.]},{"pen":"up"},{"pen":"leave"}]));
    assert!(scroll.vadjustment().value() > 0.);
    assert_eq!(state(&d.w).colors.definition(), palette.swatches[0].color);
    capture_palette(&mut d, "palette-pen-scroll.png");
    d.w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_palette_panel_input"]
fn native_palette_panel_input() {
    use layer_core::color::{RgbColor, RgbSpace};
    use layer_ui::{ColorAction, ColorLibraryAction};
    let mut d = Driver::new("art.capycanvas.PaletteReview");
    assert_eq!(state(&d.w).colors.library.palettes.len(), 10);
    assert!(
        state(&d.w)
            .colors
            .library
            .palettes
            .iter()
            .all(|p| (11..=17).contains(&p.swatches.len()))
    );
    let color_button = d.header_tool(ToolbarControl::Color);
    d.click_name(&color_button);
    assert_eq!(
        state(&d.w).customization.drawer.as_ref().unwrap().columns,
        [vec![Panel::Color, Panel::Palettes]]
    );
    capture_palette(&mut d, "palette-starter-sketch.png");
    d.click_name("palette-chooser");
    capture_palette(&mut d, "palette-starter-list.png");
    d.key(0xff1b);
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
        let palettes = state(&d.w).colors.library.palettes.clone();
        for (index, palette) in palettes.iter().enumerate() {
            d.w.dispatch(UiAction::Color {
                action: ColorAction::Library {
                    action: ColorLibraryAction::SelectPalette { id: palette.id },
                },
            });
            pump(150);
            d.click_name(&format!("palette-swatch-{}", palette.swatches[0].id));
            d.perform(serde_json::json!([{"point": [500., 70.]}]));
            pump(150);
            capture_palette(&mut d, &format!("palette-starter-{index:02}.png"));
        }
    }
    d.w.dispatch(UiAction::Color {
        action: ColorAction::Library {
            action: ColorLibraryAction::CreatePalette {
                name: "My study".into(),
            },
        },
    });
    pump(100);
    let study_id = state(&d.w).colors.library.active_palette().id;
    let colors = [
        [0.78, 0.25, 0.21, 1.],
        [0.96, 0.55, 0.24, 1.],
        [0.96, 0.78, 0.4, 1.],
        [0.19, 0.49, 0.46, 1.],
        [0.15, 0.3, 0.45, 1.],
        [0.35, 0.29, 0.46, 1.],
        [0.1, 0.15, 0.21, 1.],
        [0.76, 0.66, 0.55, 1.],
        [0.93, 0.87, 0.74, 1.],
    ];
    for rgba in colors {
        d.w.dispatch(UiAction::SetColor { rgba });
        pump(40);
        d.click_name("palette-add-color");
    }
    assert!(
        state(&d.w).colors.library.history.is_empty(),
        "selection and storing are not paint use"
    );
    assert_eq!(
        state(&d.w).colors.library.active_palette().swatches.len(),
        colors.len()
    );
    d.click_name("palette-color-name");
    let entry = d
        .named("palette-name-editor")
        .downcast::<gtk::Entry>()
        .unwrap();
    entry.set_text("Linen");
    d.key(0xff0d);
    assert_eq!(
        state(&d.w)
            .colors
            .library
            .active_palette()
            .swatches
            .last()
            .unwrap()
            .name,
        "Linen"
    );
    let palette = state(&d.w).colors.library.active_palette().clone();
    d.click_name(&format!("palette-swatch-{}", palette.swatches[0].id));
    d.click_name("palette-color-name");
    entry.set_text("linen");
    d.key(0xff0d);
    assert!(entry.has_css_class("error"));
    assert_ne!(
        state(&d.w)
            .colors
            .library
            .swatch(palette.swatches[0].id)
            .unwrap()
            .name,
        "linen"
    );
    d.key(0xff1b);
    assert!(!entry.is_mapped());
    assert!(
        state(&d.w).customization.drawer.is_some(),
        "Escape cancels the editor before closing its drawer"
    );
    // Mouse holds never open a context menu.
    let tile = d.named(&format!("palette-swatch-{}", palette.swatches[0].id));
    let point = d.point(&tile);
    d.perform(serde_json::json!([{"point":point,"down":true},{"wait_ms":800},{"down":false}]));
    assert!(
        !d.w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .any(|p| p.is_visible())
    );
    d.perform(
        serde_json::json!([{"point":point},{"button":273,"down":true},{"button":273,"down":false}]),
    );
    assert!(d.label("Rename Color…").is_mapped());
    let menu = d
        .named("palette-context-menu")
        .downcast::<gtk::PopoverMenu>()
        .unwrap();
    assert!(!menu.has_arrow());
    capture_palette(&mut d, "palette-color-menu.png");
    d.click(&d.label("Rename Color…"));
    assert!(entry.is_mapped(), "native menu begins inline naming");
    d.key(0xff1b);
    assert!(tile.grab_focus());
    d.key(0xff67); // Menu key.
    assert!(d.label("Rename Color…").is_mapped());
    d.key(0xff1b);
    let next = d.named(&format!("palette-swatch-{}", palette.swatches[1].id));
    let point = d.point(&next);
    d.perform(serde_json::json!([{"touch":"down","point":point},{"wait_ms":800},{"touch":"up"}]));
    assert!(d.label("Rename Color…").is_mapped());
    assert_eq!(
        state(&d.w).colors.definition(),
        palette.swatches[0].color,
        "touch hold suppresses selection"
    );
    d.key(0xff1b);
    assert!(state(&d.w).customization.drawer.is_some());
    assert!(state(&d.w).colors.library.history.is_empty());
    capture_palette(&mut d, "palette-sketch.png");
    // Native drawing appends usage; recalling that color does not duplicate it.
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Brush,
    });
    d.perform(
        serde_json::json!([{"point":[760,600],"down":true},{"point":[840,640]},{"down":false}]),
    );
    assert_eq!(state(&d.w).colors.library.history.len(), 1);
    if state(&d.w).customization.drawer.is_none() {
        d.click_name(&color_button);
    }
    let used = state(&d.w).colors.library.history.clone();
    d.click_name("palette-recent-color");
    assert_eq!(state(&d.w).colors.library.history, used);
    d.w.dispatch(UiAction::SetBrushSize { value: 12. });
    for (index, rgba) in colors.into_iter().enumerate() {
        d.w.dispatch(UiAction::SetColor { rgba });
        let x = 1050 + index * 14;
        d.perform(
            serde_json::json!([{"point":[x,780],"down":true},{"point":[x+8,790]},{"down":false}]),
        );
    }
    assert_eq!(state(&d.w).colors.library.history.len(), colors.len());
    if state(&d.w).customization.drawer.is_none() {
        d.click_name(&color_button);
    }
    let footer = d.named("palette-chooser");
    let before = footer.compute_bounds(&d.w.window).unwrap();
    d.click_name("palette-history-expand");
    assert_eq!(footer.compute_bounds(&d.w.window).unwrap(), before);
    capture_palette(&mut d, "palette-history.png");
    d.click_name("palette-history-collapse");
    d.click_name("palette-chooser");
    assert_eq!(footer.compute_bounds(&d.w.window).unwrap(), before);
    d.click_name("palette-library-add");
    assert!(d.label("New Palette…").is_mapped() && d.label("Import Palette…").is_mapped());
    d.click(&d.label("New Palette…"));
    let name = d
        .named("palette-library-name")
        .downcast::<gtk::Entry>()
        .unwrap();
    name.set_text("Night study");
    d.click_label("Save");
    assert_eq!(
        state(&d.w).colors.library.active_palette().name,
        "Night study"
    );
    d.click_name("palette-chooser");
    let search = d
        .named("palette-search")
        .downcast::<gtk::SearchEntry>()
        .unwrap();
    search.set_text("missing");
    pump(250);
    capture_palette(&mut d, "palette-search-empty.png");
    assert!(d.named("palette-browser").is_mapped());
    assert!(d.label("No matching palettes").is_mapped());
    search.set_text("");
    pump(250);
    capture_palette(&mut d, "palette-chooser.png");
    search.set_text("My study");
    pump(250);
    d.click_name(&format!("palette-choice-{study_id}"));
    assert_eq!(state(&d.w).colors.library.active_palette().id, study_id);
    // The same retained component is docked next to Color in Paint and Photo.
    for (preset, name) in [
        (WorkspacePreset::Illustrator, "paint"),
        (WorkspacePreset::Photographer, "photo"),
    ] {
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState {
                layout: preset.layout(Platform::Gtk),
                ..WorkspaceState::default()
            }),
        });
        pump(500);
        let group = state(&d.w)
            .workspace
            .layout
            .panel_group(Panel::Color)
            .unwrap();
        assert_eq!(
            state(&d.w).workspace.layout.group_panels(group).unwrap(),
            [Panel::Color, Panel::Palettes]
        );
        let before =
            d.w.resolved()
                .groups
                .into_iter()
                .find(|g| g.id == group)
                .unwrap()
                .bounds;
        d.w.dispatch(UiAction::SelectPanelTab {
            group,
            panel: Panel::Palettes,
        });
        pump(250);
        assert_eq!(
            d.w.resolved()
                .groups
                .into_iter()
                .find(|g| g.id == group)
                .unwrap()
                .bounds,
            before,
            "{name}: selecting Palettes preserves the fitted Color group"
        );
        assert!(d.named("palette-add-color").is_mapped());
        for theme in [Theme::Dark, Theme::Light] {
            d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            pump(250);
            let viewport =
                d.w.panel_widget(Panel::Palettes)
                    .compute_bounds(&d.w.window)
                    .unwrap();
            let detail = d
                .named("palette-color-detail")
                .compute_bounds(&d.w.window)
                .unwrap();
            assert!(
                detail.y() + detail.height() <= viewport.y() + viewport.height(),
                "{name}: footer remains fully visible"
            );
            capture_palette(&mut d, &format!("palette-{name}-{theme:?}.png"));
        }
    }
    // Existing wide-gamut and exact low-alpha definitions survive the panel.
    let exact = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0.1234567, 123. / 65535.]).unwrap();
    d.w.dispatch(UiAction::Color {
        action: ColorAction::Library {
            action: ColorLibraryAction::Store {
                palette: study_id,
                name: "Wide red".into(),
                color: exact,
            },
        },
    });
    pump(150);
    let id = state(&d.w)
        .colors
        .library
        .active_palette()
        .swatches
        .last()
        .unwrap()
        .id;
    d.click_name(&format!("palette-swatch-{id}"));
    assert_eq!(state(&d.w).colors.definition(), exact);
    let first = state(&d.w).colors.library.active_palette().swatches[0].clone();
    let point = d.point(&d.named(&format!("palette-swatch-{}", first.id)));
    let history = state(&d.w).colors.library.history.clone();
    d.perform(serde_json::json!([{"touch":"down","point":point},{"touch":"up"}]));
    assert_eq!(state(&d.w).colors.definition(), first.color);
    assert_eq!(state(&d.w).colors.library.history, history);
    // Floating widths exercise native allocation independently of dock defaults.
    let mut layout = state(&d.w).workspace.layout.clone();
    layout
        .move_panel(
            [1600., 1000.],
            Panel::Palettes,
            DockTarget::Float {
                position: [450., 120.],
            },
        )
        .unwrap();
    for width in [280., 460.] {
        for float in &mut layout.floating {
            if matches!(&float.root, DockNode::Tabs { panels, .. } if panels.contains(&Panel::Palettes))
            {
                float.width = width;
                float.height = None;
            }
        }
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState {
                layout: layout.clone(),
                ..WorkspaceState::default()
            }),
        });
        pump(350);
        let palette = state(&d.w).colors.library.active_palette().clone();
        let first = d.named(&format!("palette-swatch-{}", palette.swatches[0].id));
        let seventh = d.named(&format!("palette-swatch-{}", palette.swatches[6].id));
        let a = first.compute_bounds(&d.w.window).unwrap();
        let b = seventh.compute_bounds(&d.w.window).unwrap();
        assert!(a.width() >= 40. && a.width() < 44.);
        if width == 280. {
            assert!(b.y() > a.y());
        } else {
            assert_eq!(b.y(), a.y());
        }
        capture_palette(&mut d, &format!("palette-width-{}.png", width as u32));
    }
    d.w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_adaptive_panel_tabs"]
fn native_adaptive_panel_tabs() {
    let mut d = Driver::new("art.capycanvas.AdaptiveTabs");
    let mut layout = WorkspacePreset::Photographer.layout(Platform::Gtk);
    let group = layout.panel_group(Panel::Color).unwrap();
    for panel in [Panel::Navigator, Panel::Proof, Panel::Stats] {
        layout
            .move_panel(
                [1600., 1000.],
                panel,
                DockTarget::Tab { group, index: None },
            )
            .unwrap();
    }
    layout
        .move_item(
            [1600., 1000.],
            DockItem::Group { group },
            DockTarget::Float {
                position: [350., 130.],
            },
        )
        .unwrap();
    // A manual resize releases the width fitted when joining tabs.
    layout.fit_tab_groups.retain(|id| *id != group);
    for width in [700., 280., 460., 700.] {
        layout
            .floating
            .iter_mut()
            .find(|f| f.root.id() == group)
            .unwrap()
            .width = width;
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState {
                layout: layout.clone(),
                ..Default::default()
            }),
        });
        pump(400);
        let buttons =
            d.w.groups
                .borrow()
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .tabs
                .clone();
        let names = || {
            buttons
                .iter()
                .map(|(_, b)| b.child().unwrap().last_child().unwrap().is_visible())
                .collect::<Vec<_>>()
        };
        let before = names();
        if width == 700. {
            assert!(
                before.iter().all(|n| *n),
                "wide strip retains every name: {before:?}"
            );
        } else if width == 280. {
            assert!(before[0]);
            assert!(before.iter().any(|n| !n));
            assert!(
                before.iter().filter(|n| **n).count() >= 2,
                "spare width restores later short names: {before:?}"
            );
        }
        d.click(buttons.first().unwrap().1.upcast_ref());
        assert_eq!(names(), before, "selection does not reorder name priority");
        for (_, button) in &buttons {
            let label = button
                .child()
                .unwrap()
                .last_child()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap();
            if label.is_visible() {
                assert!(
                    label.width() >= label.layout().pixel_size().0,
                    "full label {}",
                    label.text()
                );
            }
        }
        capture_palette(&mut d, &format!("palette-tabs-{}.png", width as u32));
    }
    d.w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_palette_context_input"]
fn native_palette_context_input() {
    check_palette_context(&["mouse", "touch"]);
}
#[test]
#[ignore = "isolated native-input.js --native-test=native_palette_context_pen_input --tablet"]
fn native_palette_context_pen_input() {
    check_palette_context(&["pen"]);
}
fn check_palette_context(devices: &[&str]) {
    let mut d = Driver::new("art.capycanvas.PaletteMenus");
    for drawer in [true, false] {
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState {
                layout: if drawer {
                    WorkspacePreset::Painter
                } else {
                    WorkspacePreset::Illustrator
                }
                .layout(Platform::Gtk),
                ..Default::default()
            }),
        });
        pump(300);
        if drawer {
            d.click_name(&d.header_tool(ToolbarControl::Color));
        } else {
            let group = state(&d.w)
                .workspace
                .layout
                .panel_group(Panel::Palettes)
                .unwrap();
            d.w.dispatch(UiAction::SelectPanelTab {
                group,
                panel: Panel::Palettes,
            });
        }
        pump(200);
        if devices.contains(&"pen") {
            // Setup must not depend on the virtual tablet's unsupported popup
            // grab serial. The row hold itself still uses native tablet events.
            d.named("palette-chooser")
                .downcast::<gtk::Button>()
                .unwrap()
                .emit_clicked();
            pump(150);
        } else {
            d.click_name("palette-chooser");
        }
        let library = state(&d.w).colors.library.clone();
        let row = d.named(&format!("palette-choice-{}", library.palettes[1].id));
        assert!(
            find_named(
                d.w.window.upcast_ref(),
                &format!("palette-actions-{}", library.palettes[1].id)
            )
            .is_none()
        );
        for device in devices {
            let p = d.point(&row);
            match *device {
                "mouse" => d.perform(serde_json::json!([{"point":p},{"button":273,"down":true},{"button":273,"down":false}])),
                "touch" => d.perform(serde_json::json!([{"touch":"down","point":p},{"wait_ms":850},{"touch":"up"}])),
                _ => d.perform(serde_json::json!([{"pen":"down","point":p},{"wait_ms":850},{"pen":"up"},{"pen":"leave"}])),
            }
            let menu = d
                .named("palette-context-menu")
                .downcast::<gtk::PopoverMenu>()
                .unwrap();
            assert!(menu.is_visible(), "{device}: row context menu");
            assert!(!menu.has_arrow());
            assert_eq!(
                state(&d.w).colors.library.active,
                library.active,
                "hold/right click must not activate the row"
            );
            assert!(d.named("palette-browser").is_visible());
            if *device != "pen" {
                assert!(d.label("Rename Palette…").is_mapped());
                capture_palette(&mut d, "palette-row-menu.png");
                d.click(&d.label("Rename Palette…"));
            } else {
                // Synthetic tablet serials cannot grant compositor popup grabs.
                // Pen verifies hold recognition and click suppression; the
                // mouse/touch fixture exercises mapped menus and their actions.
                menu.popdown();
                pump(100);
                continue;
            }
            let dialog =
                d.w.window
                    .visible_dialog()
                    .expect("native row menu opens naming dialog");
            let entry = find_named(dialog.upcast_ref(), "palette-library-name")
                .unwrap()
                .downcast::<gtk::Entry>()
                .unwrap();
            assert_eq!(entry.text(), library.palettes[1].name);
            dialog.close();
            pump(150);
            assert_eq!(state(&d.w).colors.library, library);
        }
        if !devices.contains(&"pen") {
            assert!(row.grab_focus());
            d.key(0xff67);
            assert!(d.label("Rename Palette…").is_mapped());
            d.click(&d.label("Export Palette"));
            for format in layer_ui::PaletteFormat::ALL {
                assert!(d.label(format.label()).is_mapped(), "{format:?}");
            }
            capture_palette(&mut d, "palette-export-menu.png");
            d.named("palette-context-menu")
                .downcast::<gtk::PopoverMenu>()
                .unwrap()
                .popdown();
            pump(100);
        }
        assert!(d.named("palette-browser").is_visible());
        // Swiping the list before its hold must scroll without opening a menu.
        if devices.contains(&"touch") {
            let scroll = row
                .ancestor(gtk::ScrolledWindow::static_type())
                .and_downcast::<gtk::ScrolledWindow>()
                .unwrap();
            let p = d.point(scroll.upcast_ref());
            d.perform(serde_json::json!([{"touch":"down","point":p},{"touch":"move","point":[p[0],p[1]-65.]},{"touch":"up"}]));
            assert!(scroll.vadjustment().value() > 0.);
            assert!(!d.named("palette-context-menu").is_visible());
            assert_eq!(state(&d.w).colors.library.active, library.active);
        }
    }
    d.w.window.destroy();
    pump(100);
}
