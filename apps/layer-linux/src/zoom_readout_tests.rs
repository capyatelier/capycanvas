//! The zoom readout's popover with actual Mutter mouse and key delivery: fixed
//! levels, Actual Pixels and the typed field, with the canvas keeping focus.
use super::*;

pub(super) use super::mapped_label;

fn readout_text(w: &Workspace) -> String {
    w.view_info.root.child().and_downcast::<gtk::Label>().unwrap().text().to_string()
}

fn whole_pixels(w: &Workspace) -> bool {
    state(w).camera.translation.iter().all(|v| v.fract() == 0.)
}

fn open(w: &Workspace, input: &mut RemoteInput) {
    input.click(screen_point(w.view_info.root.upcast_ref(), &w.window, [0.5, 0.5]));
    until(|| w.view_info.menu.is_visible() && w.view_info.field.is_mapped(), "the readout opens its popover and field");
}

pub(super) fn save_widget(widget: &impl IsA<gtk::Widget>, path: &std::path::Path) {
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(widget)).snapshot(&snapshot, widget.width() as f64, widget.height() as f64);
    let node = snapshot.to_node().unwrap();
    widget.native().unwrap().renderer().unwrap().render_texture(node, None).save_to_png(path).unwrap();
}

fn capture_menu(w: &Workspace, name: &str) {
    let directory = std::path::Path::new("../../artifacts/photo-m2/zoom-readout").join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    save_widget(&w.view_info.menu, &directory.join(name));
}

fn choose(w: &Workspace, input: &mut RemoteInput, label: &str) {
    let mut item = None;
    until(|| {
        item = mapped_label(w.view_info.menu.upcast_ref(), label);
        item.is_some()
    }, label);
    input.click(screen_point(&item.unwrap(), &w.window, [0.5, 0.5]));
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_zoom_readout_menu_and_field() {
    let app = native_test_app("art.capycanvas.ZoomReadout");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(200);
    let mut input = RemoteInput::new().settle_ms(200);
    input.ready();
    pump(300);
    w.area.grab_focus();
    assert!(w.area.has_focus());
    assert!(!w.view_info.root.can_focus(), "the readout never takes focus from the canvas");

    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        open(&w, &mut input);
        until(|| w.view_info.rotation.is_mapped(), "the rotation slider is visible");
        assert_eq!(w.view_info.rotation.height(), w.view_info.field.height());
        assert!(mapped_label(w.view_info.menu.upcast_ref(), "Rotation").is_none());
        let zoom = screen_point(w.view_info.field.upcast_ref(), &w.window, [0.5, 0.5]);
        let lock_zoom = screen_point(&mapped_label(w.view_info.menu.upcast_ref(), "Lock zoom").unwrap(), &w.window, [0.5, 0.5]);
        let rotation = screen_point(w.view_info.rotation.upcast_ref(), &w.window, [0.5, 0.5]);
        let reset = screen_point(&mapped_label(w.view_info.menu.upcast_ref(), "Reset rotation").unwrap(), &w.window, [0.5, 0.5]);
        let lock_rotation = screen_point(&mapped_label(w.view_info.menu.upcast_ref(), "Lock rotation").unwrap(), &w.window, [0.5, 0.5]);
        assert!(zoom[1] < lock_zoom[1] && lock_zoom[1] < rotation[1] && rotation[1] < reset[1] && reset[1] < lock_rotation[1]);
        capture_menu(&w, &format!("zoom-menu-{theme:?}.png"));
        choose(&w, &mut input, "Lock rotation");
        until(|| state(&w).camera.rotation_locked && !w.view_info.menu.is_visible(), "rotation lock applies");
        open(&w, &mut input);
        choose(&w, &mut input, "Lock zoom");
        until(|| state(&w).camera.zoom_locked && !w.view_info.menu.is_visible(), "zoom lock applies");
        open(&w, &mut input);
        capture_menu(&w, &format!("zoom-menu-locked-{theme:?}.png"));
        for (name, changed) in [("zoom-RotateRight", 0), ("zoom-RotateLeft", 0),
            ("zoom-ZoomIn", 1), ("zoom-ZoomOut", 1), ("zoom-FlipHorizontal", 2), ("zoom-FlipVertical", 2)] {
            let button = descendants::<gtk::Button>(w.view_info.menu.upcast_ref()).into_iter()
                .find(|b| b.widget_name() == name && b.is_mapped()).unwrap();
            let before = state(&w).camera;
            input.click(screen_point(button.upcast_ref(), &w.window, [0.5, 0.5]));
            until(|| {
                let after = state(&w).camera;
                match changed { 0 => after.rotation != before.rotation, 1 => after.zoom != before.zoom, _ => after.flipped != before.flipped }
            }, "the navigation button works while gestures are locked");
            assert!(w.view_info.menu.is_visible());
        }
        let value = descendants::<gtk::Button>(w.view_info.rotation.upcast_ref()).into_iter()
            .find(|b| b.is_mapped() && b.has_css_class("number-value")).unwrap();
        input.click(screen_point(value.upcast_ref(), &w.window, [0.5, 0.5]));
        let entry = descendants::<gtk::Entry>(w.view_info.rotation.upcast_ref()).into_iter().next().unwrap();
        until(|| entry.is_mapped(), "the rotation editor opens");
        for key in ['4' as u32, '5' as u32, 0xff0d] { input.key(key); }
        until(|| (state(&w).camera.rotation - std::f32::consts::FRAC_PI_4).abs() < 1e-5, "typed rotation applies while locked");
        let slider = descendants::<gtk::Scale>(w.view_info.rotation.upcast_ref()).into_iter().find(|s| s.is_mapped()).unwrap();
        input.click(screen_point(slider.upcast_ref(), &w.window, [0.3, 0.5]));
        until(|| (state(&w).camera.rotation - std::f32::consts::FRAC_PI_4).abs() > 0.01, "the rotation slider works while locked");
        choose(&w, &mut input, "Reset rotation");
        until(|| state(&w).camera.rotation.abs() < 1e-6 && !w.view_info.menu.is_visible(), "reset rotation applies");
        open(&w, &mut input);
        choose(&w, &mut input, "Lock rotation");
        open(&w, &mut input);
        choose(&w, &mut input, "Lock zoom");
        w.dispatch(UiAction::Invoke { command: CommandId::FlipHorizontal });
        w.dispatch(UiAction::Invoke { command: CommandId::FlipVertical });
        assert!(!state(&w).camera.zoom_locked && !state(&w).camera.rotation_locked);
    }

    open(&w, &mut input);
    pump(200);
    capture_menu(&w, "zoom-menu.png");
    choose(&w, &mut input, "200%");
    until(|| !w.view_info.menu.is_visible() && state(&w).camera.zoom == 2.0, "200% applies and closes the menu");
    assert!(whole_pixels(&w));
    until(|| readout_text(&w) == "200% · 0°", "the readout follows the camera");
    assert!(w.area.has_focus(), "the canvas keeps keyboard focus after a menu choice");

    w.dispatch(UiAction::SetRotation { rotation: std::f32::consts::FRAC_PI_2 });
    w.dispatch(UiAction::SetZoom { zoom: 0.37 });
    pump(100);
    open(&w, &mut input);
    choose(&w, &mut input, "Actual Pixels");
    until(|| !w.view_info.menu.is_visible() && state(&w).camera.zoom == 1.0, "Actual Pixels applies");
    assert!(whole_pixels(&w), "a quarter-turned 1:1 view lands on whole device pixels");
    until(|| readout_text(&w) == "100% · 90°", "the readout shows the turned 1:1 view");
    assert!(w.area.has_focus());

    open(&w, &mut input);
    let value = descendants::<gtk::Button>(w.view_info.field.upcast_ref())
        .into_iter()
        .find(|b| b.is_mapped() && b.has_css_class("number-value"))
        .unwrap();
    input.click(screen_point(value.upcast_ref(), &w.window, [0.5, 0.5]));
    let entry = descendants::<gtk::Entry>(w.view_info.field.upcast_ref()).into_iter().next().unwrap();
    until(
        || entry.is_mapped() && gtk::prelude::RootExt::focus(&w.window).is_some_and(|f| f.is_ancestor(&entry)),
        "the typed field takes the keys while open",
    );
    for key in ['5' as u32, '0' as u32, 0xff0d] {
        input.key(key);
    }
    until(|| state(&w).camera.zoom == 0.5, "a typed percentage applies");
    assert!(whole_pixels(&w));
    assert!(w.view_info.menu.is_visible(), "typing keeps the menu open");
    input.key(0xff1b);
    if w.view_info.menu.is_visible() {
        input.key(0xff1b);
    }
    until(|| !w.view_info.menu.is_visible(), "Escape closes the menu");
    until(|| w.area.has_focus(), "closing hands focus back to the canvas");

    for chord in [&[0xffe3, '1' as u32][..], &[0xffe3, 0xffe9, '0' as u32]] {
        w.dispatch(UiAction::SetZoom { zoom: 0.37 });
        pump(100);
        let events: Vec<_> = chord
            .iter()
            .map(|key| serde_json::json!({ "key": key, "down": true }))
            .chain(chord.iter().rev().map(|key| serde_json::json!({ "key": key, "down": false })))
            .collect();
        input.perform(serde_json::Value::Array(events));
        until(|| state(&w).camera.zoom == 1.0 && whole_pixels(&w), "the Actual Pixels chord applies");
    }
    input.finish();
    w.window.destroy();
    pump(100);
}
