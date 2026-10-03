use super::*;
use super::new_photo::ready;
use super::photo_edit::{document, shown, window_point};
use layer_core::color::{ColorProfile, RgbSpace, SampleDepth, source::*};
use layer_ui::{CanvasBarKind, EffectAction};
use serde_json::json;

fn fixture() -> layer_core::Project {
    let mut project = new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let mut source = SourceBuilder::new([256, 256], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false,
    }, 1024 * 1024).unwrap();
    let row = [150, 175, 200, 255].repeat(256);
    for _ in 0..256 { source.push_row(&row).unwrap(); }
    project.document.layers[0].source = Some(std::sync::Arc::new(source.finish().unwrap()));
    project
}

fn picker_button(w: &Workspace) -> gtk::Widget {
    find_named(w.effects.properties.upcast_ref(), "property-picker").unwrap()
}

#[test]
#[ignore = "private compositor, hardware GPU and native-input.js --tablet"]
fn native_white_balance_picker_contacts_and_atomic_history() {
    let app = native_test_app("art.capycanvas.WhiteBalancePicker");
    let w = Workspace::with_project(&app, Some((fixture(), None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    let mut input = RemoteInput::new().settle_ms(200).timeout_secs(30);
    input.ready();
    w.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "white_balance".into() } });
    w.dispatch(UiAction::SetColor { rgba: [0.8, 0.1, 0.2, 1.] });
    for panel in Panel::ALL.into_iter().filter(|panel| !matches!(panel, Panel::Toolbar | Panel::Commands | Panel::Properties)) {
        w.customize(CustomizationAction::SetPanelVisible { panel, visible: false });
    }
    w.customize(CustomizationAction::SetPanelVisible { panel: Panel::Properties, visible: true });
    let group = state(&w).workspace.layout.panel_group(Panel::Properties).unwrap();
    w.dispatch(UiAction::SelectPanelTab { group, panel: Panel::Properties });
    w.dispatch(UiAction::Customize { action: CustomizationAction::CloseExpanded });
    ready(&w);
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "../../artifacts/photo-editing-color/p15-gtk".into());
    std::fs::create_dir_all(&output).unwrap();
    let mut reports = Vec::new();
    let width = w.window.width();
    assert!(matches!(width, 640 | 1100), "run with LAYER_MOTION_VIEWPORT=640x800 or1100x800");
    {
        w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
        pump(300);
        assert_eq!(w.window.width(), width);
        for theme in [Theme::Light, Theme::Dark] {
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            pump(250);
            for kind in ["mouse", "pen", "touch"] {
                let before = document(&w);
                let foreground = state(&w).colors.clone();
                let tool = state(&w).layer_tools.tool;
                let pixel = shown(&w, [128., 128.]);
                let button = picker_button(&w);
                assert!(button.is_mapped());
                let button_point = screen_point(&button, &w.window, [0.5, 0.5]);
                let hit = w.window.pick(button_point[0] as f64, button_point[1] as f64, gtk::PickFlags::DEFAULT).unwrap();
                assert!(hit == button || hit.is_ancestor(&button), "picker button is the pointer target");
                crate::snapshot(&w).save_to_png(output.join(format!("armed-layout-{width}-{theme:?}-{kind}.png"))).unwrap();
                input.click(button_point);
                until(|| state(&w).canvas_bar.as_ref().is_some_and(|bar| bar.context.kind == CanvasBarKind::Picker), "White Balance picker bar");
                let point = window_point(&w, [128., 128.]);
                let hit = w.window.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT).unwrap();
                assert!(hit == w.area || hit.is_ancestor(&w.area), "sample point hits canvas, got {}", hit.widget_name());
                match kind {
                    "mouse" => input.click(point),
                    "pen" => input.perform(json!([{"pen":"down","point":point},{"pen":"up"},{"pen":"leave"}])),
                    _ => {
                        input.perform(json!([{"touch":"down","point":point},{"wait_ms":700}]));
                        let overlay = ui_session(&w).color_picker_overlay().unwrap();
                        let matrix = state(&w).camera.document_to_surface();
                        let contact_y = matrix[1] * 128. + matrix[3] * 128. + matrix[5];
                        assert!(overlay.sample[1] < contact_y - 20. * w.area.scale_factor() as f32);
                        crate::snapshot(&w).save_to_png(output.join(format!("loupe-{width}-{theme:?}.png"))).unwrap();
                        input.perform(json!([{"touch":"up"}]));
                    }
                }
                until(|| state(&w).canvas_bar.as_ref().is_none_or(|bar| bar.context.kind != CanvasBarKind::Picker), "released sample completes correction");
                ready(&w);
                let after = document(&w);
                assert_ne!(after.layers, before.layers, "{width} {theme:?} {kind}: correction changes the effect");
                for old in &before.layers {
                    let new = after.layer(old.id).unwrap();
                    assert_eq!(new.source, old.source);
                    assert_eq!(new.raster.identity(), old.raster.identity());
                    if old.id != before.active_layer { assert_eq!(new, old); }
                }
                assert_eq!(state(&w).colors, foreground);
                assert_eq!(state(&w).layer_tools.tool, tool);
                pump(250);
                let corrected = shown(&w, [128., 128.]);
                assert_ne!(corrected, pixel, "{kind}: presented image changes");
                crate::snapshot(&w).save_to_png(output.join(format!("corrected-{width}-{theme:?}-{kind}.png"))).unwrap();
                w.dispatch(UiAction::Invoke { command: CommandId::Undo });
                ready(&w);
                assert_eq!(document(&w).layers, before.layers, "one undo restores both parameters");
                input.click(screen_point(&picker_button(&w), &w.window, [0.5, 0.5]));
                input.key(0xff1b);
                assert_eq!(document(&w).layers, before.layers);
                assert_eq!(state(&w).colors, foreground);
                assert_eq!(state(&w).layer_tools.tool, tool);
                reports.push(json!({"width":width,"theme":format!("{theme:?}"),"input":kind,"before":pixel,"after":corrected}));
            }
        }
    }
    std::fs::write(output.join(format!("contacts-{width}.json")), serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    input.finish();
    w.window.destroy();
    pump(100);
}
