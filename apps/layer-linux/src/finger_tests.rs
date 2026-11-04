use super::*;
use super::workspace_header::Driver;
use layer_render::CanvasRenderer;

fn setup(name: &str) -> Driver {
    let d = super::phone_motion::setup(name);
    d.w.dispatch(UiAction::Invoke { command: CommandId::DrawingBrush });
    d.w.dispatch(UiAction::SetColor { rgba: [0., 0., 0., 1.] });
    d.w.dispatch(ToolbarControl::BrushSizeSlider.slider().unwrap().action(40.));
    until(|| d.w.gpu.borrow().as_ref().is_some_and(|g| {
        let e = g.session.engine();
        e.backend().paint_ready(e.document(), e.brush(), false)
    }), "finger journey brush is ready");
    assert_eq!(state(&d.w).settings.finger_drawing, layer_ui::FingerDrawing::Auto);
    assert!(!ui_session(&d.w).finger_drawing(), "GTK Desktop Auto leaves finger drawing off");
    d
}

fn path(d: &Driver) -> [[f32; 2]; 2] {
    let b = state(&d.w).phone_presentation([d.w.surface.width() as f32, d.w.surface.height() as f32]).unwrap().work_area;
    [[b.x + b.width * 0.4, b.y + b.height * 0.5], [b.x + b.width * 0.6, b.y + b.height * 0.5]]
}

fn point(d: &Driver, p: [f32; 2]) -> [f32; 2] {
    screen_point(d.w.surface.upcast_ref(), &d.w.window,
        [p[0] / d.w.surface.width() as f32, p[1] / d.w.surface.height() as f32])
}

fn pixels(d: &Driver, line: [[f32; 2]; 2]) -> Vec<Vec<u8>> {
    until(|| ui_session(&d.w).engine().backend().frames_idle(), "finger pixels use the published canvas frame");
    let image = ui_session(&d.w).engine().backend().capture().unwrap();
    (1..10).map(|i| {
        let x = ((line[0][0] + (line[1][0] - line[0][0]) * i as f32 / 10.) * image.width as f32 / d.w.surface.width() as f32) as usize;
        let y = (line[0][1] * image.height as f32 / d.w.surface.height() as f32) as usize;
        image.bytes[y * image.stride as usize + x * 4..][..4].to_vec()
    }).collect()
}

fn stroke(d: &mut Driver, line: [[f32; 2]; 2]) {
    let mut events = vec![contact("touch", "down", point(d, line[0]))];
    events.extend((1..=20).map(|i| contact("touch", "move", point(d,
        [line[0][0] + (line[1][0] - line[0][0]) * i as f32 / 20., line[0][1]]))));
    events.push(contact("touch", "up", point(d, line[1])));
    d.input.perform(serde_json::Value::Array(events));
}

fn capture(d: &mut Driver, name: &str) {
    until(|| ui_session(&d.w).engine().backend().frames_idle(), "finger capture is ready");
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
        let theme = std::env::var("CAPY_NATIVE_TEST_THEME").unwrap();
        let capture_name = format!("finger-{}x{}-{theme}-{name}", d.w.surface.width(), d.w.surface.height());
        d.input.perform(serde_json::json!([{ "capture_window": {
            "name": &capture_name,
            "maximized": d.w.window.is_maximized(),
        } }]));
        super::phone_motion::export_allocations(&d.w, name, &capture_name);
    }
}

#[test]
#[ignore = "private compositor tablet proxy: --native-test=native_finger_drawing --native-storage --tablet; 412x891 light"]
fn native_finger_drawing() {
    let mut d = setup("art.capycanvas.FingerDrawing");
    drawing(&mut d);
    d.w.dispatch(UiAction::SetColor { rgba: [0., 0., 0., 1.] });
    pen_priority(&mut d);
    d.finish();
}

fn drawing(d: &mut Driver) {
    let line = path(d);
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    let ink = active_raster(ui_session(&d.w).engine().document()).clone();
    let before = pixels(d, line);
    stroke(d, line);
    d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
    assert_eq!(pixels(d, line), before, "Auto Off touch navigation does not paint");
    d.click(&d.named("zen-capy"));
    let finger = d.named("phone-finger");
    assert!(!finger.property::<bool>("active"));
    d.click(&finger);
    until(|| ui_session(&d.w).finger_drawing(), "native Capy row enables finger drawing");
    assert_eq!(state(&d.w).settings.finger_drawing, layer_ui::FingerDrawing::On);
    if state(&d.w).phone.menu {
        assert!(d.named("phone-finger").property::<bool>("active"));
        super::phone_motion::settle_presentation(&d.w);
        capture(d, "menu-on");
        d.input.key(0xff1b);
        until(|| !state(&d.w).phone.menu, "Escape closes the finger menu before drawing");
    }
    stroke(d, line);
    until(|| ui_session(&d.w).engine().checkpoint() != checkpoint && !ui_session(&d.w).engine().has_pending_document_edits(), "finger stroke commits");
    let painted = pixels(d, line);
    assert!(painted.iter().zip(&before).filter(|(a, b)| a != b).count() >= 7, "real touch paints along its path");
    capture(d, "touch-stroke");
    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| ui_session(&d.w).engine().checkpoint() == checkpoint && pixels(d, line) == before, "one Undo restores the finger stroke");
    let p = point(d, line[0]);
    let q = point(d, line[1]);
    d.input.perform(serde_json::json!([{"touch": "down", "slot": 0, "point": p},
        {"touch": "move", "slot": 0, "point": [p[0] + 12., p[1]]},
        {"touch": "down", "slot": 1, "point": q}, {"touch": "up", "slot": 1}, {"touch": "up", "slot": 0}]));
    d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint, "second finger withdraws the provisional stroke");
    assert_eq!(pixels(d, line), before);
    let hold = gtk::Settings::default().unwrap().gtk_long_press_time() + 100;
    let tool = state(&d.w).layer_tools.tool;
    d.input.perform(serde_json::json!([{ "touch": "down", "point": p }, { "wait_ms": hold }]));
    until(|| ui_session(&d.w).color_picker_overlay().is_some(), "native hold publishes its picker loupe");
    let overlay = ui_session(&d.w).color_picker_overlay().unwrap();
    assert_eq!(ui_session(&d.w).engine().backend().picker, Some(overlay), "the GTK renderer receives the shared picker");
    assert!(!overlay.classic && overlay.original[..3].iter().all(|v| *v < 0.01));
    capture(d, "hold-active");
    let held = ui_session(&d.w).engine().backend().capture().unwrap();
    for angle in [30., 60., 90., 120., 150.] {
        let radians = (angle as f32).to_radians();
        let x = (overlay.center[0] + 39. * overlay.scale * radians.cos()).round() as usize;
        let y = (overlay.center[1] + 39. * overlay.scale * radians.sin()).round() as usize;
        let pixel = &held.bytes[y * held.stride as usize + x * 4..][..4];
        assert!(pixel[..3].iter().all(|v| *v < 64), "the black original-color ring at radius 39 is visible: {angle}, {pixel:?}, {overlay:?}");
    }
    d.input.perform(serde_json::json!([{ "touch": "up" }]));
    until(|| state(&d.w).layer_tools.tool == tool && ui_session(&d.w).color_picker_overlay().is_none(),
        "released picker completes and restores the tool");
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint, "color-picker hold withdraws without history");
    assert_eq!(active_raster(ui_session(&d.w).engine().document()), &ink, "hold restores the authored ink");
    super::phone::clear_canvas_pointer(d);
    assert_eq!(pixels(d, line), before);
    d.w.dispatch(UiAction::OpenSettings { page: SettingsPage::Input });
    let page = crate::page_dialog::parent(&d.w.preferences.dialog);
    let choice = named::<adw::ComboRow>(&page, "setting-finger-drawing");
    until(|| page.is_mapped() && choice.is_mapped(), "Settings Input and its native choice are mapped");
    assert!(choice.grab_focus());
    until(|| choice.compute_bounds(&d.w.window).is_some_and(|b| b.width() > 0. && b.height() > 0.
        && b.x() >= 0. && b.x() + b.width() <= d.w.window.width() as f32
        && b.y() >= 0. && b.y() + b.height() <= d.w.window.height() as f32), "the native finger choice is inside the visible window");
    assert_eq!(choice.selected(), 1);
    for (index, mode) in [(2, layer_ui::FingerDrawing::Off), (0, layer_ui::FingerDrawing::Auto), (1, layer_ui::FingerDrawing::On)] {
        d.click_label(choice.title().as_str());
        let point = d.point(choice.upcast_ref());
        let picked = d.w.window.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT);
        until(|| descendants::<gtk::Popover>(d.w.window.upcast_ref()).iter().any(|p| p.is_mapped()),
            &format!("native choice dropdown opens: point={point:?}, pick={picked:?}, row={:?}, focused={}",
                choice.compute_bounds(&d.w.window), choice.has_focus()));
        d.input.key(0xff50);
        for _ in 0..index { d.input.key(0xff54); }
        d.input.key(0xff0d);
        until(|| state(&d.w).settings.finger_drawing == mode, "native Input choice changes the shared policy");
        assert_eq!(choice.selected(), index);
        assert_eq!(ui_session(&d.w).finger_drawing(), index == 1);
    }
    super::phone_motion::settle_presentation(&d.w);
    capture(d, "settings-on");
    d.w.dispatch(UiAction::CloseSettings);
    until(|| !page.is_mapped(), "Settings closes before testing canvas hit targets");
    let layout = state(&d.w).workspace.layout;
    let color = layout.phone.unwrap().iter().find(|t| t.control == ToolbarControl::Color).unwrap().id;
    let button = d.named(&format!("phone-tool-{color}"));
    let hit_root = button.parent().unwrap();
    assert_eq!(hit_root.parent(), Some(d.w.phone.root.clone().upcast()));
    let phone = state(&d.w).phone_presentation([d.w.surface.width() as f32, d.w.surface.height() as f32]).unwrap();
    let index = state(&d.w).workspace.layout.phone.unwrap().iter().position(|t| t.id == color).unwrap();
    let tile = phone.tools[index];
    let gap = if matches!(phone.edge, Edge::Top | Edge::Bottom) { [tile.x - 0.5, tile.y + tile.height * 0.5] }
        else { [tile.x + tile.width * 0.5, tile.y - 0.5] };
    let margin = match phone.edge {
        Edge::Top => [tile.x + tile.width * 0.5, 2.], Edge::Bottom => [tile.x + tile.width * 0.5, d.w.surface.height() as f32 - 2.],
        Edge::Left => [2., tile.y + tile.height * 0.5], Edge::Right => [d.w.surface.width() as f32 - 2., tile.y + tile.height * 0.5],
    };
    for hit in [gap, margin] {
        let picked = d.w.surface.pick(hit[0] as f64, hit[1] as f64, gtk::PickFlags::DEFAULT).unwrap();
        assert!(picked == hit_root || picked.is_ancestor(&hit_root),
            "phone gap/margin {hit:?} belongs to the tile root: picked={} {}, root={:?}, button={:?}, tile={tile:?}, hit={:?}",
            picked.type_().name(), picked.widget_name(), hit_root.compute_bounds(&d.w.surface),
            button.compute_bounds(&d.w.surface), phone.tool_hits[index]);
        let hit = point(d, hit);
        d.input.perform(serde_json::json!([{"touch": "down", "point": hit}, {"touch": "up"}]));
        until(|| state(&d.w).customization.drawer.is_some(), "gap or margin touch opens the tile drawer");
        d.input.key(0xff1b);
        until(|| state(&d.w).customization.drawer.is_none(), "Escape closes the hit-target drawer");
    }
    let at = d.point(&button);
    d.input.perform(serde_json::json!([{"touch": "down", "point": at}, {"touch": "up"}]));
    until(|| state(&d.w).customization.drawer.is_some(), "touch on the bar opens its drawer");
    let width = d.w.surface.width() as f32;
    let height = d.w.surface.height() as f32;
    let outside = [2., width * 0.5, width - 2.].into_iter()
        .flat_map(|x| [2., height * 0.5, height - 2.].into_iter().map(move |y| [x, y]))
        .find(|p| d.w.surface.pick(p[0] as f64, p[1] as f64, gtk::PickFlags::DEFAULT)
            == Some(d.w.area.clone().upcast())).expect("a native canvas target outside the drawer");
    let outside = point(d, outside);
    d.input.perform(serde_json::json!([{"touch": "down", "point": outside}, {"touch": "up"}]));
    assert!(state(&d.w).customization.drawer.is_none(), "outside touch closes the drawer");
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint, "outside drawer contact is consumed");
    d.input.key(0xff09);
    until(|| state(&d.w).phone.hidden, "Tab hides phone chrome");
    let capy = d.point(&d.named("zen-capy"));
    d.input.perform(serde_json::json!([{"touch": "down", "point": capy}, {"touch": "up"}]));
    until(|| !state(&d.w).phone.hidden, "touch Capy restores chrome");
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint, "chrome touch never paints");
}

fn pen_priority(d: &mut Driver) {
    let line = path(d);
    let before = pixels(d, line);
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    let p = point(d, line[0]);
    let q = point(d, line[1]);
    d.input.perform(serde_json::json!([{"pen": "move", "point": p}]));
    assert!(!state(&d.w).settings.pen_seen, "tablet-proxy hover does not record admitted drawing");
    d.input.perform(serde_json::json!([{"touch": "down", "slot": 0, "point": p},
        {"touch": "move", "slot": 0, "point": [p[0] + 12., p[1]]},
        {"pen": "down", "point": p}, {"touch": "down", "slot": 1, "point": q},
        {"pen": "move", "point": q}, {"pen": "up"}]));
    until(|| state(&d.w).settings.pen_seen, "admitted tablet-proxy drawing records pen detection");
    until(|| ui_session(&d.w).engine().checkpoint() != checkpoint && !ui_session(&d.w).engine().has_pending_document_edits(), "pen wins the provisional finger");
    assert_ne!(pixels(d, line), before);
    capture(d, "proxy-pen-wins");
    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| ui_session(&d.w).engine().checkpoint() == checkpoint && pixels(d, line) == before, "one Undo removes only the winning pen stroke");
    d.input.perform(serde_json::json!([{"touch": "move", "slot": 0, "point": q},
        {"touch": "move", "slot": 1, "point": p}, {"touch": "up", "slot": 0}, {"touch": "up", "slot": 1}, {"pen": "leave"}]));
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint, "rejected finger cannot resume after pen lift");
    assert_eq!(pixels(d, line), before);
    stroke(d, line);
    until(|| ui_session(&d.w).engine().checkpoint() != checkpoint, "a new finger contact can draw after the rejected finger lifts");
}
