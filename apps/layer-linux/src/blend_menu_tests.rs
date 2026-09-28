//! The Layers header's grouped blend menu with actual Mutter mouse and touch
//! delivery, captured in the light and dark themes.
use super::crop::{Device, tap};
use super::zoom_readout::{mapped_label, save_widget};
use super::*;

fn section_labels(model: &gtk::gio::MenuModel) -> Vec<Vec<String>> {
    (0..model.n_items())
        .filter_map(|i| model.item_link(i, "section"))
        .map(|section| {
            (0..section.n_items())
                .filter_map(|i| section.item_attribute_value(i, "label", None)?.get::<String>())
                .collect()
        })
        .collect()
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse and touch delivery"]
fn native_layer_blend_menu() {
    let app = native_test_app("art.capycanvas.LayerBlendMenu");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    w.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } });
    pump(300);
    let button = find_named(w.layer_panel.root.upcast_ref(), "layer-blend")
        .and_downcast::<gtk::MenuButton>()
        .expect("the Layers header's blend control");
    let popover = button.popover().and_downcast::<gtk::PopoverMenu>().unwrap();
    let blend = |w: &Workspace| state(w).layer_tools.editing_layer.as_ref().unwrap().blend_label.clone();
    let shown = || button.child().and_downcast::<gtk::Label>().unwrap().text().to_string();
    let groups: Vec<Vec<String>> = layer_core::LayerBlend::MENU
        .iter()
        .map(|group| group.iter().map(|b| b.label().to_string()).collect())
        .collect();
    let mut input = RemoteInput::new().settle_ms(150);
    input.ready();
    pump(300);
    let open = |input: &mut RemoteInput, device| {
        tap(input, device, screen_point(button.upcast_ref(), &w.window, [0.5, 0.5]));
        until(
            || popover.is_visible() && mapped_label(popover.upcast_ref(), "Luminosity").is_some(),
            "the blend control opens its grouped menu",
        );
    };
    for (device, mode) in [(Device::Mouse, "Multiply"), (Device::Touch, "Luminosity"), (Device::Mouse, "Normal")] {
        open(&mut input, device);
        assert_eq!(section_labels(&popover.menu_model().unwrap()), groups, "the menu holds the groups of LayerBlend::MENU");
        let item = mapped_label(popover.upcast_ref(), mode).unwrap();
        tap(&mut input, device, screen_point(&item, &w.window, [0.5, 0.5]));
        until(|| !popover.is_visible() && blend(&w) == mode, &format!("{device:?} chooses {mode}"));
        until(|| shown() == mode, "the control shows the layer's mode");
    }
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| blend(&w) == "Luminosity", "each choice is one undo step");
    let directory = std::path::Path::new("../../artifacts/photo-m4/blend-menu").join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.area.grab_focus();
        pump(200);
        let name = format!("{theme:?}").to_lowercase();
        save_widget(&w.window, &directory.join(format!("{name}-window.png")));
        open(&mut input, Device::Mouse);
        pump(200);
        capture_popover(popover.upcast_ref(), directory.join(format!("{name}.png")).to_str().unwrap());
        input.key(0xff1b);
        until(|| !popover.is_visible(), "Escape closes the blend menu");
    }
    input.finish();
    w.window.destroy();
    pump(100);
}
