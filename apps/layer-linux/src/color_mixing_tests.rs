//! The brush Color mixing choice in GTK Tool Options, with compositor input.
use super::super::canvas_bar_tests::canvas_point;
use super::toolbar_components::drag;
use super::*;
use layer_core::{ColorMixSpace, DefaultBrushPreset};

fn mixing(d: &Driver) -> ColorMixSpace {
    ui_session(&d.w).engine().configured_brush().wet_mix.mix_space
}

fn committed(d: &Driver) -> u64 {
    ui_session(&d.w).engine().metrics().committed_strokes
}

fn stroke(d: &mut Driver, device: &str, from: [f32; 2], to: [f32; 2]) {
    let before = committed(d);
    let [a, b] = [from, to].map(|p| canvas_point(&d.w, p));
    drag(d, device, a, b, false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while committed(d) == before {
        assert!(Instant::now() < deadline, "{device}: stroke from {from:?} did not commit");
        pump(20);
    }
}

fn show_tool_options(d: &Driver) {
    let mut workspace = state(&d.w).workspace;
    workspace.layout.set_panel_visible(Panel::ToolSettings, true).unwrap();
    workspace
        .layout
        .move_panel([1600., 1000.], Panel::ToolSettings, DockTarget::Edge { edge: Edge::Right, outer: false })
        .unwrap();
    d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
    pump(200);
}

/// Paints two colors with `painter`, then smudges across them once in each
/// space, choosing the space in the Tool Options panel with `devices` in turn.
fn journey(d: &mut Driver, devices: &[&str], painter: &str) {
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        show_tool_options(d);
        d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 });
        d.w.dispatch(UiAction::SetBrushSize { value: 120. });
        for (y, rgba) in [(300., [0.9, 0.05, 0.05, 1.]), (420., [0.05, 0.3, 0.9, 1.])] {
            d.w.dispatch(UiAction::SetColor { rgba });
            stroke(d, painter, [300., y], [1100., y]);
        }
        let panel = d.w.panel_widget(Panel::ToolSettings);
        assert!(find_named(&panel, "tool-action-ColorMixOklab").is_none(), "G-Pen does not mix paint");
        d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::NaturalBlender as u32 });
        d.w.dispatch(UiAction::SetBrushSize { value: 160. });
        pump(150);
        assert_eq!(mixing(d), ColorMixSpace::Oklab);
        for (i, (command, space)) in [
            (CommandId::ColorMixClassic, ColorMixSpace::Classic),
            (CommandId::ColorMixLinear, ColorMixSpace::LinearRgb),
            (CommandId::ColorMixOklab, ColorMixSpace::Oklab),
        ]
        .into_iter()
        .enumerate()
        {
            let device = devices[i % devices.len()];
            let panel = d.w.panel_widget(Panel::ToolSettings);
            let button = find_named(&panel, &format!("tool-action-{command:?}")).unwrap();
            let p = d.point(&button);
            drag(d, device, p, p, false);
            pump(50);
            assert!(button.downcast_ref::<gtk::CheckButton>().unwrap().is_active(), "{device}: {command:?}");
            assert_eq!(mixing(d), space, "{device}: {command:?}");
            assert!(state(&d.w).commands.iter().any(|c| c.id == command && c.selected));
            let x = 450. + i as f32 * 250.;
            stroke(d, painter, [x, 220.], [x, 520.]);
            assert_eq!(mixing(d), space, "painting keeps the choice");
        }
        d.capture_canvas(&format!("color-mixing-{painter}-{theme:?}.png"));
        d.w.dispatch(UiAction::Invoke { command: CommandId::ClearLayer });
        pump(100);
    }
    assert!(state(&d.w).host_error.is_none());
}

#[test]
#[ignore = "private Mutter: --native-test=native_color_mixing_tool_options_input"]
fn native_color_mixing_tool_options_input() {
    let mut d = Driver::new("art.capycanvas.ColorMixing");
    journey(&mut d, &["mouse", "touch"], "mouse");
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState {
                layout: WorkspacePreset::Photographer.layout(Platform::Gtk),
                ..WorkspaceState::default()
            }),
        });
        d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Smudge as u32 });
        pump(250);
        d.w.dispatch(UiAction::Invoke { command: CommandId::ColorMixOklab });
        pump(100);
        assert!(d.named("toolbar-choice-color-mixing").is::<gtk::DropDown>());
        d.click_name("toolbar-choice-color-mixing");
        d.capture_canvas(&format!("color-mixing-toolbar-{theme:?}.png"));
        d.input.key(0xff54);
        d.input.key(0xff0d);
        pump(100);
        assert_eq!(mixing(&d), ColorMixSpace::LinearRgb, "the toolbar list chooses the next space");
    }
    d.finish();
}

#[test]
#[ignore = "private Mutter: --native-test=native_color_mixing_tool_options_pen_input --tablet"]
fn native_color_mixing_tool_options_pen_input() {
    let mut d = Driver::new("art.capycanvas.ColorMixingPen");
    journey(&mut d, &["pen"], "pen");
    d.finish();
}
