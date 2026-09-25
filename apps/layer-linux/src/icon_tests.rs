//! Production GTK paintables and controls on the runner's private Wayland display.
use super::*;
use std::path::{Path, PathBuf};

fn capture_widget(window: &adw::ApplicationWindow, widget: &impl IsA<gtk::Widget>) -> gdk::Texture {
    let widget = widget.as_ref();
    assert!(widget.is_mapped());
    if let Ok(expected) = std::env::var("LAYER_MOTION_SCALE") {
        assert_eq!(widget.scale_factor(), expected.parse::<i32>().unwrap());
    }
    let scale = widget.scale_factor() as f32;
    let snapshot = gtk::Snapshot::new();
    snapshot.scale(scale, scale);
    let bounds = widget.compute_bounds(window).unwrap();
    let mut child = window.first_child();
    while let Some(view) = child {
        child = view.next_sibling();
        window.snapshot_child(&view, &snapshot);
    }
    let node = snapshot
        .to_node()
        .expect("mapped icon fixture has a render tree");
    window.renderer().unwrap().render_texture(
        &node,
        Some(&gtk::graphene::Rect::new(
            bounds.x() * scale,
            bounds.y() * scale,
            bounds.width() * scale,
            bounds.height() * scale,
        )),
    )
}

fn artwork(app: &adw::Application, output: &Path) {
    crate::icons::register();
    let mut names: Vec<_> = gtk::gio::resources_enumerate_children(
        "/dev/layer/icons/scalable/actions/",
        gtk::gio::ResourceLookupFlags::NONE,
    )
    .unwrap()
    .into_iter()
    .map(|s| s.trim_end_matches(".svg").to_owned())
    .collect();
    names.sort();
    let grid_height = names.len().div_ceil(12) as i32 * 48;
    let mut expected: Vec<_> =
        std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../layer-web/icons"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter_map(|n| n.strip_suffix(".svg").map(str::to_owned))
            .collect();
    expected.sort();
    assert_eq!(names, expected, "audit the entire packaged bank");
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_widget_name("icon-audit-root");
    let grid = gtk::Grid::builder()
        .row_homogeneous(true)
        .column_homogeneous(true)
        .build();
    let mut images = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let cell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        cell.set_size_request(48, 48);
        let image = crate::icons::image(name);
        image.set_halign(gtk::Align::Center);
        image.set_valign(gtk::Align::Center);
        image.set_hexpand(true);
        image.set_vexpand(true);
        assert!(image.paintable().unwrap().is::<gtk::Svg>());
        cell.append(&image);
        grid.attach(&cell, i as i32 % 12, i as i32 / 12, 1, 1);
        images.push(image);
    }
    root.append(&grid);
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .decorated(false)
        .default_width(576)
        .default_height(grid_height)
        .content(&root)
        .build();
    let css = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &root.display(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
    );
    window.present();
    let mut fixtures = Vec::new();
    for (theme, bg, fg, scheme) in [
        ("light", "#fafafa", "#292a2d", adw::ColorScheme::ForceLight),
        ("dark", "#242629", "#f0f0f1", adw::ColorScheme::ForceDark),
    ] {
        app.style_manager().set_color_scheme(scheme);
        for size in [16, 24, 32] {
            for (state, opacity) in [("normal", 1.), ("accent", 1.), ("disabled", 0.35)] {
                let foreground = if state == "accent" { "#3584e4" } else { fg };
                css.load_from_string(&format!("#icon-audit-root {{ background: {bg}; color: {foreground}; -gtk-icon-palette: success #33d17a; }}"));
                grid.set_opacity(opacity);
                for image in &images {
                    image.set_pixel_size(size);
                }
                pump(180);
                assert_eq!((root.width(), root.height()), (576, grid_height));
                let texture = capture_widget(&window, &root);
                let scale = root.scale_factor() as usize;
                let mut downloader = gdk::TextureDownloader::new(&texture);
                downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
                downloader.set_color_state(&gdk::ColorState::srgb());
                let (pixels, stride) = downloader.download_bytes();
                let background = gdk::RGBA::parse(bg).unwrap();
                let background = [background.red(), background.green(), background.blue()]
                    .map(|v| (v * 255.).round() as u8);
                let differs = |x: usize, y: usize| {
                    (0..3).any(|c| pixels[y * stride + x * 4 + c].abs_diff(background[c]) > 6)
                };
                for (i, name) in names.iter().enumerate() {
                    let left = (i % 12 * 48 + 24 - size as usize / 2) * scale;
                    let top = (i / 12 * 48 + 24 - size as usize / 2) * scale;
                    let extent = size as usize * scale;
                    let mut ink = 0;
                    for y in top..top + extent {
                        for x in left..left + extent {
                            ink += usize::from(differs(x, y));
                        }
                    }
                    // The single-pixel cursor glyph intentionally has one ink
                    // pixel at its native size; it still must be visible.
                    assert!(ink > 0, "{name} is visible at {theme}/{size}/{state}");
                }
                let point = |name: &str, x: usize, y: usize| {
                    let i = names.iter().position(|n| n == name).unwrap();
                    (
                        (i % 12 * 48 + 24 - size as usize / 2) * scale
                            + x * size as usize * scale / 16,
                        (i / 12 * 48 + 24 - size as usize / 2) * scale
                            + y * size as usize * scale / 16,
                    )
                };
                let (x, y) = point("layer-clear-symbolic", 8, 8);
                assert!(!differs(x, y), "clear retains its empty center");
                for (x, y, value) in [(4, 4, 0.), (12, 12, 255.)] {
                    let (x, y) = point("layer-colors-symbolic", x, y);
                    for c in 0..3 {
                        let expected = (value * opacity + f64::from(background[c]) * (1. - opacity))
                            .round() as u8;
                        assert!(
                            pixels[y * stride + x * 4 + c].abs_diff(expected) <= 2,
                            "fixed swatch paint survives {theme}/{state}"
                        );
                    }
                }
                let name = format!("{theme}-{size}-{state}");
                texture
                    .save_to_png(output.join(format!("native-{name}.png")))
                    .unwrap();
                fixtures.push(serde_json::json!({"name":name,"theme":theme,"size":size,"width":576,"height":grid_height,"scale":scale,
                    "foreground":foreground,"background":bg,"opacity":opacity,"icons":names}));
            }
        }
    }
    std::fs::write(
        output.join("fixtures.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"schema":1,"fixtures":fixtures})).unwrap(),
    )
    .unwrap();
    window.destroy();
    gtk::style_context_remove_provider_for_display(&root.display(), &css);
    pump(50);
}

#[test]
#[ignore = "private Wayland display and hardware GPU: complete native icon audit"]
fn native_icon_audit() {
    let app = native_test_app("art.capycanvas.IconAudit");
    let output = PathBuf::from(
        std::env::var("LAYER_TEST_ARTIFACTS")
            .unwrap_or_else(|_| "../../artifacts/icon-audit/gtk".into()),
    );
    std::fs::create_dir_all(&output).unwrap();
    artwork(&app, &output);
    controls(&app, &output);
}

fn show(w: &Rc<Workspace>, panel: Panel) {
    if let Some(group) = w
        .resolved()
        .groups
        .into_iter()
        .find(|g| g.panels.contains(&panel))
    {
        if group.active != panel {
            w.dispatch(UiAction::SelectPanelTab {
                group: group.id,
                panel,
            });
        }
    } else {
        panic!("fixture panel {panel:?} must be visible");
    }
    pump(100);
    assert!(w.panel_widget(panel).is_mapped());
}

fn scroll_to(widget: &impl IsA<gtk::Widget>) {
    let widget = widget.as_ref();
    if let Some(scroll) = widget
        .ancestor(gtk::ScrolledWindow::static_type())
        .and_downcast::<gtk::ScrolledWindow>()
    {
        let bounds = widget.compute_bounds(&scroll).unwrap();
        let adjustment = scroll.vadjustment();
        let delta = if bounds.y() < 0. {
            bounds.y()
        } else {
            (bounds.y() + bounds.height() - scroll.height() as f32).max(0.)
        };
        adjustment.set_value(adjustment.value() + f64::from(delta));
        pump(80);
        let bounds = widget.compute_bounds(&scroll).unwrap();
        assert!(
            bounds.y() >= -1. && bounds.y() + bounds.height() <= scroll.height() as f32 + 1.,
            "{} at {:?} is on screen in {}px viewport",
            widget.widget_name(),
            bounds,
            scroll.height()
        );
    }
}

fn check_icon(button: &impl IsA<gtk::Widget>, icon: &str) -> gtk::Image {
    let image = find_named(button.as_ref(), &format!("layer-{icon}-symbolic"))
        .unwrap_or_else(|| panic!("{} is missing {icon}", button.as_ref().widget_name()))
        .downcast::<gtk::Image>()
        .unwrap();
    assert!(image.is_mapped(), "{icon} must be mapped");
    assert!(
        image.paintable().unwrap().is::<gtk::Svg>(),
        "{icon} uses the production vector renderer"
    );
    let bounds = image.compute_bounds(button.as_ref()).unwrap();
    assert!(
        bounds.x() >= 0. && bounds.y() >= 0. && bounds.width() >= 16. && bounds.height() >= 16.
    );
    assert!(bounds.x() + bounds.width() <= button.as_ref().width() as f32 + 0.5);
    assert!(bounds.y() + bounds.height() <= button.as_ref().height() as f32 + 0.5);
    image
}

fn check_tool_set(w: &Rc<Workspace>, records: &mut Vec<serde_json::Value>, theme: &str) {
    let current = state(w);
    for (item, button) in current
        .tool_set
        .groups
        .iter()
        .zip(w.tool_set.group_buttons.borrow().iter())
    {
        check_icon(button, item.icon);
        records.push(serde_json::json!({"theme":theme,"kind":"category","label":item.label,"icon":item.icon}));
    }
    for (item, button, preview) in w.tool_set.buttons.borrow().iter() {
        check_icon(button, item.icon);
        if item.preview.is_some() {
            assert!(
                preview.as_ref().unwrap().paintable().is_some(),
                "{} keeps its brush preview",
                item.label
            );
        }
        records.push(
            serde_json::json!({"theme":theme,"kind":"subtool","label":item.label,"icon":item.icon}),
        );
    }
}

fn controls(app: &adw::Application, output: &Path) {
    use layer_ui::{FilterPickerAction, LayerAction, LayerCanvasTool};
    let w = fixture_workspace(app);
    w.window.present();
    pump(1200);
    let deadline = Instant::now() + Duration::from_secs(25);
    while w
        .gpu
        .borrow()
        .as_ref()
        .is_none_or(|g| !g.session.engine().backend().startup.brush_ready)
    {
        assert!(Instant::now() < deadline, "native brush startup");
        pump(30);
    }
    let mut workspace = state(&w).workspace;
    // Show the real tool settings beside the docked category/preset projection.
    workspace
        .layout
        .set_panel_visible(Panel::ToolSettings, true)
        .unwrap();
    workspace
        .layout
        .move_panel(
            [1200., 900.],
            Panel::ToolSettings,
            DockTarget::Float {
                position: [600., 180.],
            },
        )
        .unwrap();
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(150);
    let output = output.join("controls");
    std::fs::create_dir_all(&output).unwrap();
    // Exercise the live color tile independently of the fixed-paint bank.
    w.dispatch(UiAction::Color {
        action: layer_ui::ColorAction::Select {
            slot: layer_ui::ColorSlot::Background,
        },
    });
    w.dispatch(UiAction::SetColor {
        rgba: [0.2, 0.7, 0.4, 1.],
    });
    w.dispatch(UiAction::Color {
        action: layer_ui::ColorAction::Select {
            slot: layer_ui::ColorSlot::Foreground,
        },
    });
    w.dispatch(UiAction::SetColor {
        rgba: [0.8, 0.3, 0.1, 1.],
    });
    pump(100);
    let mut records = Vec::new();
    for (theme, label) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        show(&w, Panel::Brushes);
        for category in ui_catalog().brush_categories {
            w.dispatch(UiAction::SelectBrush {
                id: category.brushes[0].id,
            });
            pump(100);
            let index = state(&w)
                .tool_set
                .groups
                .iter()
                .position(|g| g.label == category.label)
                .unwrap();
            let button = w.tool_set.group_buttons.borrow()[index].clone();
            click(&button);
            assert!(button.has_css_class("selected-tool"));
            for choice in &category.brushes {
                let button = w
                    .tool_set
                    .buttons
                    .borrow()
                    .iter()
                    .find(|(item, _, _)| item.preview == Some(choice.id))
                    .unwrap()
                    .1
                    .clone();
                scroll_to(&button);
                click(&button);
                check_icon(&button, choice.icon);
                capture_widget(&w.window, &button)
                    .save_to_png(output.join(format!("{label}-preset-{}.png", choice.id)))
                    .unwrap();
                assert_eq!(state(&w).brush.preset, choice.id);
                assert!(button.has_css_class("selected-tool"));
            }
            let first_group = w.tool_set.group_buttons.borrow()[0].clone();
            scroll_to(&first_group);
            check_tool_set(&w, &mut records, label);
            capture_widget(&w.window, &w.panel_widget(Panel::Brushes))
                .save_to_png(output.join(format!("{label}-medium-{}.png", category.icon)))
                .unwrap();
        }
        for command in CommandId::TOOLS {
            if matches!(
                command,
                CommandId::Pen
                    | CommandId::Pencil
                    | CommandId::Brush
                    | CommandId::Airbrush
                    | CommandId::Decoration
                    | CommandId::Blend
                    | CommandId::Liquify
                    | CommandId::Eraser
            ) {
                continue;
            }
            if !state(&w)
                .commands
                .iter()
                .find(|c| c.id == command)
                .unwrap()
                .enabled
            {
                assert_eq!(
                    command,
                    CommandId::ScaleRotate,
                    "only content transforms are disabled on this blank fixture"
                );
                continue;
            }
            w.dispatch(UiAction::Invoke { command });
            pump(100);
            for group in state(&w).tool_set.groups {
                if let UiAction::Invoke { command: action } = group.action {
                    if !state(&w)
                        .commands
                        .iter()
                        .find(|c| c.id == action)
                        .unwrap()
                        .enabled
                    {
                        assert_eq!(action, CommandId::ScaleRotate);
                        check_tool_set(&w, &mut records, label);
                        continue;
                    }
                }
                let index = state(&w)
                    .tool_set
                    .groups
                    .iter()
                    .position(|g| g.label == group.label)
                    .unwrap();
                let button = w.tool_set.group_buttons.borrow()[index].clone();
                click(&button);
                let buttons = w.tool_set.buttons.borrow().clone();
                for (_, button, _) in buttons {
                    click(&button);
                }
                check_tool_set(&w, &mut records, label);
                for action in state(&w).tool_actions.into_iter().filter(|a| !a.checkable) {
                    let button = find_named(
                        w.tool_settings.root.upcast_ref(),
                        &format!("tool-action-{:?}", action.command),
                    )
                    .unwrap();
                    check_icon(&button, action.command.icon().unwrap());
                }
                capture_widget(&w.window, &w.panel_widget(Panel::Brushes))
                    .save_to_png(output.join(format!("{label}-{command:?}-{}.png", group.icon)))
                    .unwrap();
            }
        }
        w.dispatch(UiAction::Layer {
            action: LayerAction::Tool {
                tool: LayerCanvasTool::LassoFill,
            },
        });
        pump(80);
        check_tool_set(&w, &mut records, label);
        capture_widget(&w.window, &w.panel_widget(Panel::Brushes))
            .save_to_png(output.join(format!("{label}-lasso-fill.png")))
            .unwrap();
        show(&w, Panel::Adjustments);
        let mut count = 0;
        for category in state(&w)
            .filter_categories
            .into_iter()
            .filter(|c| c.id.is_some())
        {
            w.dispatch(UiAction::FilterPicker {
                action: FilterPickerAction::Category {
                    category: category.id.clone(),
                },
            });
            pump(150);
            check_icon(&w.effects.adjustments, category.icon);
            for choice in state(&w).adjustments {
                let button = find_named(
                    w.effects.adjustments.upcast_ref(),
                    &format!("adjustment-{}", choice.id),
                )
                .unwrap();
                scroll_to(&button);
                check_icon(&button, &choice.icon);
                records.push(serde_json::json!({"theme":label,"kind":"filter","label":choice.label,"icon":choice.icon}));
                count += 1;
            }
            let scroll = find_css(w.effects.adjustments.upcast_ref(), "filter-picker-scroll")
                .unwrap()
                .downcast::<gtk::ScrolledWindow>()
                .unwrap();
            scroll.vadjustment().set_value(0.);
            pump(100);
            capture_widget(&w.window, &w.effects.adjustments)
                .save_to_png(output.join(format!("{label}-filters-{}.png", category.id.unwrap())))
                .unwrap();
        }
        assert_eq!(count, 40);
        // Native toolbar size changes retain the approved vector geometry.
        for style in [TileStyle::Small, TileStyle::Medium, TileStyle::Large] {
            w.customize(CustomizationAction::SetTileStyle {
                panel: Panel::Toolbar,
                style,
            });
            pump(150);
            let toolbar = w.panel_widget(Panel::Toolbar);
            let mut child = toolbar.first_child();
            while let Some(tile) = child {
                child = tile.next_sibling();
                if let Some(image) = tile.first_child().and_downcast::<gtk::Image>() {
                    assert_eq!(image.pixel_size(), style.icon_size() as i32);
                    let bounds = image.compute_bounds(&tile).unwrap();
                    assert!(
                        (bounds.x() + bounds.width() * 0.5 - tile.width() as f32 * 0.5).abs() <= 1.
                    );
                    assert!(
                        (bounds.y() + bounds.height() * 0.5 - tile.height() as f32 * 0.5).abs()
                            <= 1.
                    );
                }
            }
            let color_icon = find_named(&toolbar, "layer-colors-symbolic").unwrap();
            let color_texture = capture_widget(&w.window, &color_icon);
            color_texture
                .save_to_png(output.join(format!("{label}-color-{style:?}.png")))
                .unwrap();
            let mut download = gdk::TextureDownloader::new(&color_texture);
            download.set_format(gdk::MemoryFormat::R8g8b8a8);
            download.set_color_state(&gdk::ColorState::srgb());
            let (pixels, stride) = download.download_bytes();
            // GtkImage can fill its tile allocation while centering the glyph.
            let size = style.icon_size() as usize * color_icon.scale_factor() as usize;
            let left = (color_texture.width() as usize - size) / 2;
            let top = (color_texture.height() as usize - size) / 2;
            for (x, y, expected) in [
                (4, 4, state(&w).colors.preview(state(&w).colors.foreground)),
                (
                    12,
                    12,
                    state(&w).colors.preview(state(&w).colors.background),
                ),
            ] {
                for c in 0..3 {
                    let at = (top + y * size / 16) * stride + (left + x * size / 16) * 4 + c;
                    assert!(
                        pixels[at].abs_diff((expected[c] * 255.).round() as u8) <= 2,
                        "live color tile follows both color slots: {style:?} x={x} y={y} channel={c} actual={} expected={} size={size}",
                        pixels[at],
                        (expected[c] * 255.).round()
                    );
                }
            }
            capture_widget(&w.window, &toolbar)
                .save_to_png(output.join(format!("{label}-toolbar-{style:?}.png")))
                .unwrap();
        }
    }
    std::fs::write(
        output.join("controls.json"),
        serde_json::to_vec_pretty(&records).unwrap(),
    )
    .unwrap();
    eprintln!(
        "GTK production icon controls: {} checks in both themes",
        records.len()
    );
    w.window.destroy();
    pump(100);
}
