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
fn grouped_adjustment() -> layer_core::Document {
    use layer_core::authored::*;
    let mut document = super::native_navigation::photo([1024, 768]);
    let remove: Vec<_> = document.scene().order().iter().copied().filter(|h| document.scene().paint_source(*h).is_none() && !document.scene().constant_backdrop().contains(h)).collect();
    if !remove.is_empty() { document.apply(document.delete_layers_edit(&remove).unwrap()).unwrap(); }
    let draft = layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("black_white").unwrap().program());
    let definition = RecordChange::insert(&document.artwork.definitions, Definition { program: draft.program });
    let effect = RecordChange::insert(&document.artwork.effects, EffectApplication { definition: definition.handle, values: draft.values});
    let adjustment = RecordChange::insert(&document.artwork.occurrences, Occurrence::new(OccurrenceContent::Effect(effect.handle), "Black & White"));
    document.apply(layer_core::Edit::Batch(vec![layer_core::Edit::Definition(definition), layer_core::Edit::Effect(effect), layer_core::Edit::Occurrence(adjustment.clone())])).unwrap();
    let children = RecordChange::insert(&document.artwork.stacks, Stack { entries: vec![adjustment.handle] });
    let group = RecordChange::insert(&document.artwork.occurrences, Occurrence::new(OccurrenceContent::Stack(children.handle), "Adjustments"));
    let root = document.composition().result;
    let mut stack = document.artwork.stacks.get(root).unwrap().clone();
    stack.entries.insert(0, group.handle);
    let stack = RecordChange::replace(&document.artwork.stacks, root, Some(stack)).unwrap();
    let mut working = document.working.clone(); working.occurrence = Some(group.handle); working.target = None;
    document.apply(layer_core::Edit::Batch(vec![layer_core::Edit::Stack(children), layer_core::Edit::Occurrence(group), layer_core::Edit::Stack(stack), layer_core::Edit::Working(working)])).unwrap();
    document
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse and touch delivery"]
fn native_pass_through_group_and_new_group_preference() {
    let app = native_test_app("art.capycanvas.PassThroughGroups");
    let w = Workspace::with_project(&app, Some((grouped_adjustment(), None)));
    w.window.maximize();
    w.window.present();
    pump(1600);
    until(|| w.gpu.borrow().as_ref().is_some_and(|g| g.session.engine().backend().startup.brush_ready), "canvas startup");
    apply_fixture_theme(&w);
    new_photo::ready(&w);
    let group = document(&w).working.occurrence.unwrap();
    let doc = document(&w);
    let photo = doc.scene().order().iter().copied().find(|h| doc.scene().paint_source(*h).is_some_and(|p| p.original.is_some())).unwrap();
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

    w.dispatch(UiAction::Layer { action: LayerAction::Select { id: layer_ui::occurrence_token(photo), mask: false } });
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
        crate::capture(&w, directory.join(format!("{}-preferences.png", format!("{theme:?}").to_lowercase())).to_str().unwrap());
    }
    w.preferences.dialog.close();
    until(|| !w.preferences.dialog.is_mapped(), "Preferences close");
    w.dispatch(UiAction::Layer { action: LayerAction::New { group: true, clipped: false } });
    until(
        || { let doc = document(&w); doc.working.occurrence.is_some_and(|h| h != group && doc.scene().occurrence(h).is_some_and(|o| o.kind() == layer_core::LayerKind::Group)) },
        "New Group adds a group",
    );
    until(|| blend(&w) == "Pass Through" && shown() == "Pass Through", "the new group passes through");
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.area.grab_focus();
        pump(200);
        let name = format!("{theme:?}").to_lowercase();
        crate::capture(&w, directory.join(format!("{name}-window.png")).to_str().unwrap());
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

#[test]
#[ignore = "isolated compositor and native menu input"]
fn native_layer_color_modes_and_add_filter() {
    let app = native_test_app("art.capycanvas.LayerModes");
    let w = fixture_workspace(&app); w.window.maximize(); w.window.present(); pump(1600);
    let owner = state(&w).layer_tools.editing_layer.unwrap().id;
    let color = named::<gtk::MenuButton>(w.layer_panel.root.upcast_ref(), "layer-color-mode");
    let add = named::<gtk::MenuButton>(w.layer_panel.root.upcast_ref(), "layer-add-filter");
    let mut input = RemoteInput::new().settle_ms(150); input.ready();
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) }); pump(250);
        for label in ["Grayscale", "Two-tone (black & white)", "Full color"] {
            input.click(screen_point(color.upcast_ref(), &w.window, [0.5, 0.5]));
            let popover = color.popover().unwrap();
            until(|| popover.is_mapped() && mapped_label(popover.upcast_ref(), label).is_some(), "color mode opens");
            input.click(screen_point(&mapped_label(popover.upcast_ref(), label).unwrap(), &w.window, [0.5, 0.5]));
            until(|| state(&w).layer_tools.color_mode.as_ref().is_some_and(|m| m.value.as_ref() == label) && !popover.is_mapped(), "color mode selected");
        }
        for filter in ["Exposure", "Curves"] {
            w.dispatch(UiAction::SelectPanelTab { group: state(&w).workspace.layout.panel_group(Panel::Layers).unwrap(), panel: Panel::Layers }); pump(200);
            input.click(screen_point(add.upcast_ref(), &w.window, [0.5, 0.5]));
            let popover = add.popover().unwrap();
            let category = || widgets(popover.upcast_ref()).find(|node| node.is_mapped() && node.type_().name() == "GtkModelButton" && node.property::<String>("text") == "Tone");
            until(|| category().is_some(), "filter categories open");
            let category = category().unwrap();
            let submenu = category.property::<Option<gtk::PopoverMenu>>("popover").unwrap();
            input.perform(serde_json::json!([{"point":screen_point(&category, &w.window, [0.5,0.5])},{"wait_ms":250}]));
            if !submenu.is_mapped() { input.click(screen_point(&category, &w.window, [0.5,0.5])); }
            until(|| submenu.is_mapped() && mapped_label(submenu.upcast_ref(), filter).is_some(), "filter submenu opens");
            input.click(screen_point(&mapped_label(submenu.upcast_ref(), filter).unwrap(), &w.window, [0.5,0.5]));
            until(|| state(&w).layer_tools.editing_layer.as_ref().is_some_and(|l| l.adjustment_effect), "local filter selected");
            let layers = state(&w).layers; let selected = layers.iter().find(|l| l.editing).unwrap();
            assert!(state(&w).layer_tools.connections.iter().any(|edge| edge.from == selected.id || edge.to == selected.id));
        }
        w.dispatch(UiAction::Invoke { command: CommandId::Undo }); pump(250);
        w.dispatch(UiAction::Invoke { command: CommandId::Undo }); pump(250);
        assert_eq!(state(&w).layer_tools.editing_layer.unwrap().id, owner);
        w.dispatch(UiAction::SelectPanelTab { group: state(&w).workspace.layout.panel_group(Panel::Layers).unwrap(), panel: Panel::Layers }); pump(200);
    }
    w.window.close(); pump(200);
}
