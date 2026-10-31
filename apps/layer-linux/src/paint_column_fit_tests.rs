use super::new_photo::{capture_ui, ready};
use super::*;
use layer_core::color::SampleDepth;

#[test]
#[ignore = "private Wayland display, GPU and native input"]
fn native_default_workspace_panel_groups() {
    let app = native_test_app("art.capycanvas.DefaultPanelGroups");
    let w = Workspace::new(&app);
    w.window.maximize();w.window.present();ready(&w);
    let mut input = RemoteInput::new();input.ready();
    for preset in [layer_ui::WorkspacePreset::Illustrator, layer_ui::WorkspacePreset::Photographer] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(layer_ui::WorkspaceState { layout: preset.layout(Platform::Gtk), ..Default::default() }),
        });
        pump(800);
        let layout = state(&w).workspace.layout;
        let group = layout.panel_group(Panel::Layers).unwrap();
        assert_eq!(layout.group_panels(group).unwrap(), [Panel::Layers, Panel::Adjustments]);
        let bounds = || {
            let resolved = w.resolved();
            [Panel::Properties, Panel::Layers].map(|panel| {
                resolved.groups.iter().find(|g| g.panels.contains(&panel)).unwrap().bounds
            })
        };
        let original = bounds();
        let fraction = original[0].height / (original[0].height + original[1].height);
        assert!((fraction - 0.4).abs() < 0.01, "{preset:?}: {original:?}");
        for panel in [Panel::Adjustments, Panel::Layers] {
            let tab = w.groups.borrow().iter().flat_map(|group| &group.tabs)
                .find(|(candidate, button)| *candidate == panel && button.is_mapped()).unwrap().1.clone();
            input.click(screen_point(tab.upcast_ref(), &w.window, [0.5, 0.5]));
            pump(200);
            assert_eq!(state(&w).workspace.layout.active_panel(panel), Some(panel));
            assert!(w.panel_widget(Panel::Properties).is_mapped());
            assert_eq!(bounds(), original);
        }
    }
    w.window.destroy();pump(100);
}

fn painted_layer_footer_width(w: &Rc<Workspace>) {
    let settings = gtk::Settings::default().unwrap();
    let animations = settings.is_gtk_enable_animations();
    settings.set_gtk_enable_animations(false);
    w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState::default()) });
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    w.dispatch(UiAction::MovePanel { panel: Panel::Layers, target: DockTarget::Float { position: [500., 200.] }, viewport });
    let mut fixture = state(w).workspace;
    fixture.layout.floating.last_mut().unwrap().width = 420.;
    fixture.layout.floating.last_mut().unwrap().height = Some(400.);
    w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(fixture.clone()) });
    pump(200);
    let more = find_named(w.layer_panel.footer.upcast_ref(), "layer-actions").unwrap();
    let room = w.layer_panel.footer.parent().unwrap();
    let css = paint_color(&more, "#ff00ff");
    assert!(idle_paint(&room).color_bounds(&w.window, [255, 0, 255, 255]).is_some(), "wide Layers paints its actions button");
    let frames = sample_after_paint(&room, || ());
    fixture.layout.floating.last_mut().unwrap().width = 220.;
    w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(fixture) });
    pump(200);
    let frames = frames.finish();
    assert!(frames.last().unwrap().color_bounds(&w.window, [255, 0, 255, 255]).is_some(), "settled narrow Layers paints its actions button");
    for frame in frames {
        assert!(frame.color_bounds(&w.window, [255, 0, 255, 255]).is_some(),
            "narrow Layers paints its actions button in the first allocation: frame {}", frame.time);
    }
    gtk::style_context_remove_provider_for_display(&w.surface.display(), &css);
    settings.set_gtk_enable_animations(animations);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_paint_fitted_columns() {
    let app = native_test_app("art.capycanvas.PaintFittedColumns");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("capy-paint-fitted-columns"));
    std::fs::create_dir_all(&output).unwrap();
    let mut sdr_color = None;
    for (hdr, document) in [(false, [1600, 900]), (true, [900, 1200])] {
        let project = new_drawing_at(document[0], document[1], if hdr { SampleDepth::F16 } else { SampleDepth::U8 });
        let w = Workspace::with_project(&app, Some((project, None)));
        w.window.present();
        ready(&w);
        set_transparency(&w, "LAYER_MOTION_TRANSPARENCY");
        if !hdr { painted_layer_footer_width(&w); }
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(layer_ui::WorkspaceState {
                layout: layer_ui::WorkspacePreset::Illustrator.layout(Platform::Gtk),
                ..Default::default()
            }),
        });
        for (name, size) in [("short", Some((1280, 640))), ("maximized", None)] {
            match size {
                Some((width, height)) => {
                    w.window.unmaximize();
                    w.window.set_default_size(width, height);
                }
                None => w.window.maximize(),
            }
            pump(800);
            let resolved = w.resolved();
            let layout = state(&w).workspace.layout;
            let group = |panel| {
                resolved
                    .groups
                    .iter()
                    .find(|g| g.panels.contains(&panel))
                    .unwrap()
                    .bounds
            };
            let natural = |panel| {
                layout
                    .measurements
                    .iter()
                    .find(|m| m.panel == panel)
                    .unwrap()
                    .content_height
                    + layer_ui::TAB_BAR_HEIGHT
            };
            let (color, tool_set, tool) = (
                group(Panel::Color),
                group(Panel::Brushes),
                group(Panel::ToolSettings),
            );
            assert!(
                (tool_set.height - tool.height).abs() <= 1.,
                "{name}: {tool_set:?} {tool:?}"
            );
            if (color.height - natural(Panel::Color)).abs() > 0.5 {
                assert!(color.height < natural(Panel::Color), "{name}: {color:?}");
                assert_eq!(tool.height, layer_ui::TAB_BAR_HEIGHT + layer_ui::TILE_SIZE, "{name}");
            } else {
                let scroll = w
                    .panel_widget(Panel::Color)
                    .downcast::<gtk::ScrolledWindow>()
                    .unwrap()
                    .vadjustment();
                assert!(
                    scroll.upper() <= scroll.page_size() + 1.,
                    "{name}: Color scrolls {} > {}",
                    scroll.upper(),
                    scroll.page_size()
                );
            }
            let navigator = group(Panel::Navigator);
            let measurement = layout.measurements.iter().find(|m| m.panel == Panel::Navigator).unwrap();
            let scroll = measurement.scroll.unwrap();
            let preferred = measurement.content_height.min(scroll.fixed_height + 4. * layer_ui::TILE_SIZE)
                + layer_ui::TAB_BAR_HEIGHT;
            let minimum = resolved.groups.iter().find(|group| group.panels.contains(&Panel::Navigator)).unwrap().min_body_height
                + layer_ui::TAB_BAR_HEIGHT;
            assert!(
                navigator.height >= minimum - 0.5 && navigator.height <= preferred + 0.5,
                "{name}: {navigator:?}, minimum {minimum}, preferred {preferred}"
            );
            if name == "maximized" {
                assert!((navigator.height - preferred).abs() <= 0.5, "{name}: {navigator:?}, preferred {preferred}");
            }
            let overview = find_named(w.window.upcast_ref(), "navigator-overview").unwrap();
            let aspect = layer_ui::NavigatorGeometry::overview_aspect(document);
            let expected = (overview.width() as f32 * aspect).min(navigator.height - layer_ui::TAB_BAR_HEIGHT - scroll.fixed_height);
            assert!(
                (overview.height() as f32 - expected).abs() <= 2.,
                "{name}: overview {}x{}",
                overview.width(),
                overview.height()
            );
            if name == "maximized" {
                match sdr_color {
                    None => sdr_color = Some(color.height),
                    Some(sdr) => assert!(color.height > sdr + 10., "HDR {} SDR {sdr}", color.height),
                }
            }
            capture_ui(
                &w,
                &output,
                &format!("paint-{}-{name}.png", if hdr { "hdr" } else { "sdr" }),
            );
        }
        w.window.destroy();
        pump(100);
    }
}
