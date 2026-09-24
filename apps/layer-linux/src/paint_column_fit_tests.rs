use super::new_photo::{capture_ui, ready};
use super::*;
use layer_core::color::SampleDepth;

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
        let mut project = new_drawing(document[0], document[1]).unwrap();
        if hdr {
            project.document.color.depth = SampleDepth::F16;
        }
        let w = Workspace::with_project(&app, Some((project, None)));
        w.window.present();
        ready(&w);
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
            assert!(
                (navigator.height - natural(Panel::Navigator)).abs() <= 0.5,
                "{name}: {navigator:?}"
            );
            let overview = find_named(w.window.upcast_ref(), "navigator-overview").unwrap();
            let aspect = layer_ui::NavigatorGeometry::overview_aspect(document);
            assert!(
                (overview.height() as f32 - overview.width() as f32 * aspect).abs() <= 2.,
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
