//! The Layers header's grouped blend menu and Pass Through groups with actual
//! Mutter mouse and touch delivery, captured in the light and dark themes.
use super::canvas_bar_tests::document;
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
        .map(|group| {
            group.iter().filter(|b| b.offered(layer_core::LayerKind::Paint, false)).map(|b| b.label().to_string()).collect()
        })
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

/// A photo under an isolated group that holds a Black & White adjustment.
fn grouped_adjustment() -> layer_core::Project {
    let mut project = super::native_navigation::photo([1024, 768]);
    let document = &mut project.document;
    document.layers.retain(|l| l.source.is_some() || l.kind == layer_core::LayerKind::Background);
    let mut group = layer_core::Layer::paint(document.allocate_layer_id(), "Adjustments");
    group.kind = layer_core::LayerKind::Group;
    let mut adjustment = layer_core::Layer::paint(document.allocate_layer_id(), "Black & White");
    adjustment.kind = layer_core::LayerKind::Effect;
    adjustment.properties.parent = Some(group.id);
    adjustment.effect = Some(std::sync::Arc::new(layer_core::EffectInstance::new(
        layer_core::bundled_effect_catalog().get("black_white").unwrap().program(),
    )));
    document.active_layer = group.id;
    document.layers.splice(0..0, [group, adjustment]);
    project
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse and touch delivery"]
fn native_pass_through_group_and_new_group_preference() {
    let app = native_test_app("art.capycanvas.PassThroughGroups");
    let w = Workspace::with_project(&app, Some((grouped_adjustment(), None)));
    w.window.maximize();
    w.window.present();
    pump(1600);
    let group = document(&w).active_layer;
    let photo = document(&w).layers.iter().find(|l| l.source.is_some()).unwrap().id;
    let context = glib::MainContext::default();
    let mut readback = 9800;
    let mut saturation = |w: &Rc<Workspace>| {
        readback += 1;
        let image = context.block_on(read_canvas_pixels(w, readback)).unwrap();
        let at = 200 * image.stride as usize + 900 * 4;
        let [r, g, b, _] = image.bytes[at..at + 4] else { unreachable!() };
        r.max(g).max(b) - r.min(g).min(b)
    };
    assert!(saturation(&w) > 60, "inside an isolated group the adjustment leaves the photo alone");
    let button = find_named(w.layer_panel.root.upcast_ref(), "layer-blend")
        .and_downcast::<gtk::MenuButton>()
        .expect("the Layers header's blend control");
    let popover = button.popover().and_downcast::<gtk::PopoverMenu>().unwrap();
    let blend = |w: &Workspace| state(w).layer_tools.editing_layer.as_ref().unwrap().blend_label.clone();
    let shown = || button.child().and_downcast::<gtk::Label>().unwrap().text().to_string();
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
    open(&mut input, Device::Mouse);
    assert_eq!(
        section_labels(&popover.menu_model().unwrap())[0],
        ["Pass Through", "Normal"],
        "a group's menu leads with Pass Through"
    );
    let item = mapped_label(popover.upcast_ref(), "Pass Through").unwrap();
    tap(&mut input, Device::Touch, screen_point(&item, &w.window, [0.5, 0.5]));
    until(|| !popover.is_visible() && blend(&w) == "Pass Through", "touch chooses Pass Through");
    until(|| shown() == "Pass Through", "the control shows Pass Through");
    assert!(saturation(&w) < 4, "the adjustment inside now reaches the photo below the group");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| blend(&w) == "Normal", "choosing Pass Through is one undo step");
    assert!(saturation(&w) > 60);
    w.dispatch(UiAction::Invoke { command: CommandId::Redo });
    until(|| blend(&w) == "Pass Through", "Redo passes through again");

    w.dispatch(UiAction::Layer { action: LayerAction::Select { id: photo.0, mask: false } });
    until(|| blend(&w) == "Normal", "the photo is active");
    open(&mut input, Device::Mouse);
    assert!(mapped_label(popover.upcast_ref(), "Pass Through").is_none(), "only groups offer Pass Through");
    input.key(0xff1b);
    until(|| !popover.is_visible(), "Escape closes the blend menu");

    w.dispatch(UiAction::OpenSettings { page: SettingsPage::Canvas });
    until(|| w.preferences.dialog.is_mapped(), "Preferences open on the Canvas page");
    pump(300);
    let row = find_named(w.preferences.dialog.upcast_ref(), "setting-pass-through-groups")
        .and_downcast::<adw::SwitchRow>()
        .expect("the Use Pass Through for new groups row");
    assert_eq!(row.title(), "Use Pass Through for new groups");
    assert!(!row.is_active());
    let directory = std::path::Path::new("../../artifacts/photo-m4/pass-through").join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    tap(&mut input, Device::Mouse, screen_point(row.upcast_ref(), &w.window, [0.9, 0.5]));
    until(|| state(&w).settings.pass_through_groups && row.is_active(), "a click turns the setting on");
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        save_widget(&w.window, &directory.join(format!("{}-preferences.png", format!("{theme:?}").to_lowercase())));
    }
    w.preferences.dialog.close();
    until(|| !w.preferences.dialog.is_mapped(), "Preferences close");
    w.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } });
    until(
        || document(&w).layer(document(&w).active_layer).is_some_and(|l| l.kind == layer_core::LayerKind::Group && l.id != group),
        "New Group adds a group",
    );
    until(|| blend(&w) == "Pass Through" && shown() == "Pass Through", "the new group passes through");
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.area.grab_focus();
        pump(200);
        let name = format!("{theme:?}").to_lowercase();
        save_widget(&w.window, &directory.join(format!("{name}-window.png")));
        open(&mut input, Device::Mouse);
        pump(200);
        capture_popover(popover.upcast_ref(), directory.join(format!("{name}-menu.png")).to_str().unwrap());
        input.key(0xff1b);
        until(|| !popover.is_visible(), "Escape closes the blend menu");
    }
    input.finish();
    w.window.destroy();
    pump(100);
}
