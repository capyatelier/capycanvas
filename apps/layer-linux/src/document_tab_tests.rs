//! Real retained GTK window/session/worker lifecycle on the isolated compositor.
use super::*;
use layer_core::raster::*;

fn switch(w: &Rc<Workspace>, id: u64) {
    glib::MainContext::default()
        .block_on(w.documents.activate(w, id))
        .unwrap();
    new_photo::ready(w);
    assert!(std::sync::Arc::ptr_eq(ui_session(w).localization(), &w.localization()));
    assert_eq!(ui_session(w).localization().language(), w.localization().language());
}

#[test]
#[ignore = "isolated native-input.js with LAYER_NATIVE_CAPTURE_DIR"]
fn native_canvas_background_during_startup_and_tab_switch() {
    use std::sync::atomic::Ordering;
    let app = native_test_app("art.capycanvas.OpaqueCanvas");
    let captures = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_CAPTURE_DIR").unwrap());
    // A real window behind the editor makes transparency observable in the
    // compositor capture. WidgetPaintable snapshots cannot exercise this bug.
    let behind = gtk::Window::builder()
        .application(&*app)
        .decorated(false)
        .build();
    behind.set_widget_name("transparency-sentinel");
    let css = gtk::CssProvider::new();
    css.load_from_string("#transparency-sentinel { background: #ff00ff; }");
    gtk::style_context_add_provider_for_display(
        &gtk::prelude::WidgetExt::display(&behind),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    behind.maximize();
    behind.present();
    pump(400);
    let mut input = RemoteInput::new().settle_ms(0).timeout_secs(30);
    input.ready();
    let mut capture = |name: &str, expected: [u8; 3]| {
        input.perform(serde_json::json!([{"wait_ms":250}, {"capture":name}]));
        let mut reader =
            png::Decoder::new(std::fs::File::open(captures.join(format!("{name}.png"))).unwrap())
                .read_info()
                .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgb);
        for y in info.height / 2 - 20..info.height / 2 + 20 {
            for x in info.width / 2 - 20..info.width / 2 + 20 {
                let i = ((y * info.width + x) * 3) as usize;
                assert!(
                    pixels[i..i + 3]
                        .iter()
                        .zip(expected)
                        .all(|(a, b)| a.abs_diff(b) <= 2),
                    "{name}: expected {expected:?}, got {:?} at {x},{y}",
                    &pixels[i..i + 3]
                );
            }
        }
        if name == "switch-resized" {
            // Mutter restores this window centered at the requested logical
            // extent. Check the slice joins and transparent outer corners in
            // the actual composite, including the separate scale-2 run.
            let scale: u32 = std::env::var("LAYER_MOTION_SCALE")
                .unwrap_or("1".into())
                .parse()
                .unwrap();
            let left = (info.width - 1100 * scale) / 2;
            let top = (info.height - 750 * scale) / 2;
            let right = left + 1100 * scale;
            let bottom = top + 750 * scale;
            let r = 12 * scale;
            let pixel = |x, y| {
                let i = ((y * info.width + x) * 3) as usize;
                &pixels[i..i + 3]
            };
            for (x, y) in [
                (left + r, top + 2 * scale),
                (right - r - 1, top + 2 * scale),
                (left + r, bottom - 2 * scale - 1),
                (right - r - 1, bottom - 2 * scale - 1),
                (left + 2 * scale, top + r),
                (left + 2 * scale, bottom - r - 1),
                (right - 2 * scale - 1, top + r),
                (right - 2 * scale - 1, bottom - r - 1),
            ] {
                assert!(
                    pixel(x, y)
                        .iter()
                        .zip(expected)
                        .all(|(a, b)| a.abs_diff(b) <= 2),
                    "background slice join at {x},{y}: {:?}",
                    pixel(x, y)
                );
            }
            for (x, y) in [
                (left, top),
                (right - 1, top),
                (left, bottom - 1),
                (right - 1, bottom - 1),
            ] {
                let p = pixel(x, y);
                assert!(
                    p[0] > 150 && p[1] < 20 && p[2] > 150,
                    "rounded corner must expose the sentinel: {p:?}"
                );
            }
        }
    };
    capture("behind", [255, 0, 255]);
    let pause = crate::render_thread::pause_next_startup();
    let w = Workspace::with_project(&app, Some((new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)));
    w.window.maximize();
    w.window.present();
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    // Allow the compositor's window-opening fade to finish; the GPU remains
    // paused, so this cannot hide a missing application background.
    pump(600);
    capture("startup-wait", [51; 3]);
    assert!(
        !ui_session(&w)
            .engine()
            .backend()
            .startup
            .complete
    );
    pause.store(false, Ordering::Release);
    new_photo::ready(&w);
    capture("startup-ready", [255; 3]);

    let pause = crate::render_thread::pause_next_startup();
    glib::MainContext::default()
        .block_on(
            w.documents
                .open(&w, (new_drawing(128, 128, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)),
        )
        .unwrap();
    capture("new-tab-wait", [51; 3]);
    assert!(
        w.documents.parked_memory().1,
        "inactive GPU worker still stops"
    );
    pause.store(false, Ordering::Release);
    new_photo::ready(&w);
    capture("new-tab-ready", [255; 3]);

    let pause = crate::render_thread::pause_next_startup();
    glib::MainContext::default()
        .block_on(w.documents.activate(&w, 1))
        .unwrap();
    capture("switch-wait", [51; 3]);
    w.window.unmaximize();
    w.window.set_default_size(1100, 750);
    pump(350);
    capture("switch-resized", [51; 3]);
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Light),
    });
    capture("switch-light", [184; 3]);
    pause.store(false, Ordering::Release);
    new_photo::ready(&w);
    capture("switch-ready", [255; 3]);

    ui_session(&w)
        .engine()
        .backend()
        .fail_next_frame();
    w.wake();
    until(
        || {
            ui_session(&w)
                .rendering_suspended()
        },
        "failed GPU worker",
    );
    capture("failed-worker", [184; 3]);
    // Simulate the opaque GTK fallback after a background transport failure.
    // A successful restart must reveal the new drawing again, not mask it.
    w.window.remove_css_class("native-canvas-background");
    let pause = crate::render_thread::pause_next_startup();
    w.restart_gpu();
    capture("restart-wait", [184; 3]);
    pause.store(false, Ordering::Release);
    new_photo::ready(&w);
    capture("restart-ready", [255; 3]);
    w.window.destroy();
    capture("closed", [255, 0, 255]);
    behind.destroy();
    input.finish();
}

#[test]
#[ignore = "isolated Wayland and GPU"]
fn native_document_tabs_history_storage_and_close() {
    let (app, windows) = crate::application("art.capycanvas.DocumentTabs");
    let app = NativeTestApp(app);
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let mut project = new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let blob = TileBlob::encode(
        project.composition().color.paint_descriptor(),
        &vec![128; 256 * 256 * 4],
    )
    .unwrap();
    let initial_paint=match occurrence_at(&project,0).content {layer_core::authored::OccurrenceContent::Paint(handle)=>handle,_=>panic!("Initial occurrence must be paint")};
    let initial_paint_id=project.artwork.paint.id(initial_paint).unwrap();
    project.artwork.paint.get_mut(initial_paint).unwrap().raster = RasterRevision::backed(RasterData {
        tiles: [(
            TileKey {
                plane: RasterPlane::Color,
                coordinate: [0, 0],
            },
            RasterTile::backed(blob),
        )]
        .into(),
        ..Default::default()
    });
    crate::open_workspace(&app, &windows, Some((project.clone(), None)));
    until(|| windows.borrow().first().is_some_and(|workspace| workspace.window.is_mapped()), "new document window mapped");
    let w = windows.borrow()[0].clone();
    new_photo::ready(&w);
    apply_fixture_theme(&w);
    new_photo::ready(&w);
    let plain_title = named::<gtk::Label>(w.window.upcast_ref(), "single-document-title");
    assert_eq!(
        w.documents.root.visible_child_name().as_deref(),
        Some("title")
    );
    assert_eq!(plain_title.text(), "Untitled · 256 × 256");
    assert!(plain_title.is_mapped());
    assert!(
        !find_named(w.window.upcast_ref(), "document-tab-1")
            .unwrap()
            .is_mapped()
    );
    let single_title_width = w.documents.root.width();
    w.documents.ram_budget.set(0);
    new_photo::invoke(&w, CommandId::AddLayer);
    assert_eq!(plain_title.text(), "• Untitled · 256 × 256");
    w.dispatch(UiAction::SetBrushSize { value: 42. });
    new_photo::invoke(&w, CommandId::ZoomIn);
    let original_view = state(&w).camera;
    let original = ui_session(&w)
        .engine()
        .document()
        .clone();
    let first = w.documents.selected();
    // The production New dialog callback must append in this window, including
    // while the previous drawing has unsaved edits.
    new_photo::invoke(&w, CommandId::NewDocument);
    for (name, value) in [("new-document-width", 128.), ("new-document-height", 96.)] {
        named::<adw::SpinRow>(w.window.upcast_ref(), name)
            .set_value(value);
    }
    new_photo::response(&w, "create");
    until(
        || w.documents.len() == 2 && !w.documents.changing.get(),
        "New creates a tab",
    );
    new_photo::ready(&w);
    let second = w.documents.selected();
    assert_eq!(app.windows().len(), 1);
    assert_eq!(windows.borrow().len(), 1);
    assert_eq!(w.documents.len(), 2);
    assert_eq!(
        w.documents.parked_memory(),
        (0, true),
        "inactive CPU tiles spilled and renderer joined"
    );
    assert!(!state(&w).document_file.modified);
    new_photo::invoke(&w, CommandId::AddLayer);
    w.recovery().capture(&w);
    glib::MainContext::default().block_on(w.recovery().drain());
    let mut owners=w.documents.model.borrow().parked().map(|(_,p)|p.owner.recovery.clone()).collect::<Vec<_>>();
    owners.push(w.recovery());
    let mut widths=owners.iter().map(|owner|glib::MainContext::default().block_on(owner.read_snapshot()).unwrap().document().composition().size[0]).collect::<Vec<_>>();
    widths.sort();
    assert_eq!(
        widths,
        [128, 256],
        "each recovery owner captured its own drawing"
    );
    new_photo::invoke(&w, CommandId::Undo);
    switch(&w, first);
    assert_eq!(
        ui_session(&w).engine().document(),
        &original
    );
    let view = state(&w).camera;
    assert!(
        view.revision > original_view.revision,
        "reactivation retires queued input"
    );
    assert_eq!(
        layer_ui::Camera {
            revision: original_view.revision,
            ..view
        },
        original_view
    );
    assert_eq!(state(&w).brush.diameter, 42.);
    assert!(state(&w).document_file.modified);
    new_photo::invoke(&w, CommandId::Undo);
    assert_eq!(
        ui_session(&w)
            .engine()
            .document().scene().order().len(),
        project.scene().order().len()
    );
    new_photo::invoke(&w, CommandId::Redo);
    assert_eq!(
        ui_session(&w)
            .engine()
            .document().scene().order().len(),
        original.scene().order().len()
    );
    // A native save traverses disk-backed tiles, including exact historical data.
    let captured = ui_session(&w)
        .capture_artwork()
        .unwrap();
    let mut bytes = Vec::new();
    write_capture(&captured, &mut bytes).unwrap();
    let reopened = open_native_document(std::io::Cursor::new(bytes.as_slice()));
    assert_eq!(
        reopened.artwork.paint.get(reopened.artwork.paint.resolve(initial_paint_id).unwrap()).unwrap().raster
            .wait_data()
            .unwrap()
            .tiles
            .values()
            .next()
            .unwrap()
            .wait_backing()
            .unwrap()
            .decode()
            .unwrap(),
        vec![128; 256 * 256 * 4]
    );
    // Repeated selection must not create a window or leave a second live worker.
    for _ in 0..3 {
        switch(&w, second);
        switch(&w, first);
    }
    assert_eq!(w.documents.parked_memory(), (0, true));
    glib::MainContext::default()
        .block_on(
            w.documents
                .open(&w, (new_drawing(80, 80, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)),
        )
        .unwrap();
    new_photo::ready(&w);
    let third = w.documents.selected();
    w.window.set_default_size(1550, 900);
    pump(300);
    assert_eq!(
        w.documents.root.visible_child_name().as_deref(),
        Some("tabs")
    );
    let tab = find_named(w.window.upcast_ref(), "document-tab-1").unwrap();
    let other = find_named(w.window.upcast_ref(), &format!("document-tab-{second}")).unwrap();
    assert!((tab.width() - other.width()).abs() <= 1);
    assert!(tab.width() >= 140);
    w.window.set_default_size(480, 650);
    pump(500);
    assert_eq!(
        w.documents.root.visible_child_name().as_deref(),
        Some("selector")
    );
    w.window.set_default_size(1550, 900);
    pump(300);
    assert_eq!(
        w.documents.root.visible_child_name().as_deref(),
        Some("tabs")
    );
    w.documents.select(&w, third, true);
    until(
        || w.documents.len() == 2 && !w.documents.changing.get(),
        "temporary tab closes",
    );
    new_photo::ready(&w);
    // Background close uses the same dirty/save decision as Ctrl+W.
    w.documents.select(&w, first, true);
    until(
        || w.window.visible_dialog().is_some(),
        "dirty close decision",
    );
    new_photo::response(&w, "cancel");
    pump(100);
    assert_eq!(w.documents.len(), 2);
    assert!(!w.documents.closing_tab.get());
    w.documents.select(&w, first, true);
    until(
        || w.window.visible_dialog().is_some(),
        "save before tab close",
    );
    new_photo::response(&w, "save");
    #[allow(deprecated)]
    {
        new_photo::chooser().response(gtk::ResponseType::Cancel);
    }
    until(|| !w.servicing.get(), "cancelled save completes");
    pump(200);
    assert_eq!(w.documents.len(), 2);
    assert!(state(&w).document_file.modified);
    // A clean background tab can close without affecting the dirty neighbor.
    w.documents.select(&w, second, true);
    until(
        || w.documents.len() == 1 && !w.documents.changing.get(),
        "clean tab closes",
    );
    new_photo::ready(&w);
    assert_eq!(w.documents.selected(), first);
    assert!(state(&w).document_file.modified);
    assert_eq!(
        w.documents.root.visible_child_name().as_deref(),
        Some("title")
    );
    assert_eq!(plain_title.text(), "• Untitled · 256 × 256");
    assert!(plain_title.is_mapped());
    assert!(
        !find_named(w.window.upcast_ref(), "document-tab-1")
            .unwrap()
            .is_mapped()
    );
    assert_eq!(w.documents.root.width(), single_title_width);
    crate::capture(&w, "/tmp/capy-single-drawing-title.png");
    w.documents.select(&w,w.documents.selected(),true);
    until(|| w.window.visible_dialog().is_some(), "final dirty close");
    new_photo::response(&w, "discard");
    until(|| !w.window.is_visible(), "last tab closes window");
    assert!(windows.borrow().is_empty());
}

#[test]
#[ignore = "native-input.js --native-test=native_document_tab_input --native-storage"]
fn native_document_tab_input() {
    let app = native_test_app("art.capycanvas.DocumentTabInput");
    let w = Workspace::with_project(&app, Some((new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)));
    w.window.maximize();
    w.window.present();
    new_photo::ready(&w);
    pump(250);
    let mut input = RemoteInput::new().settle_ms(180).timeout_secs(30);
    input.ready();
    pump(500);
    // A single drawing restores the native draggable/double-clickable caption.
    let title = find_named(w.window.upcast_ref(), "single-document-title").unwrap();
    let b = title.compute_bounds(&w.window).unwrap();
    input.perform(
        serde_json::json!([{"point":[b.x()+b.width()/2., b.y()+b.height()/2.]},
        {"down":true},{"down":false},{"down":true},{"wait_ms":250},{"down":false}]),
    );
    assert!(!w.window.is_maximized());
    w.window.maximize();
    pump(400);
    for _ in 0..2 {
        glib::MainContext::default()
            .block_on(
                w.documents
                    .open(&w, (new_drawing(128, 128, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)),
            )
            .unwrap();
        new_photo::ready(&w);
    }
    pump(250);
    let size = state(&w).workspace.layout.header.size;
    let tab = |id: u64| find_named(w.window.upcast_ref(), &format!("document-tab-{id}")).unwrap();
    let (first, second) = (tab(1).compute_bounds(&w.surface).unwrap(), tab(2).compute_bounds(&w.surface).unwrap());
    assert_eq!(second.x() - first.x() - first.width(), size.gap(), "tabs use the title-bar tile gap");
    assert_eq!(first.height(), size.tile(), "tabs fill the title-bar tile height");
    let close = tab(1).last_child().unwrap().compute_bounds(&tab(1)).unwrap();
    let inset = (first.height() - close.height()) / 2.;
    assert!(
        (close.y() - inset).abs() < 0.5 && (first.width() - close.x() - close.width() - inset).abs() < 0.5,
        "close button stays concentric with its tab: {close:?} in {first:?}"
    );
    let point = |id: u64, fraction: f32| {
        let tab = find_named(w.window.upcast_ref(), &format!("document-tab-{id}")).unwrap();
        let b = tab.compute_bounds(&w.window).unwrap();
        [b.x() + b.width() * fraction, b.y() + b.height() / 2.]
    };
    for touch in [false, true] {
        let device = if touch { "touch" } else { "mouse" };
        assert_eq!(
            w.documents.root.visible_child_name().as_deref(),
            Some("tabs")
        );
        let from = point(1, 0.3);
        let to = point(3, 0.8);

        input.perform(serde_json::json!([
            contact(device, "down", from),
            contact(device, "move", [from[0] + 60., from[1]])
        ]));
        assert!(
            w.documents.drag_active(),
            "native tab drag started without hold, touch={touch}"
        );
        let bounds = |id: u64| {
            let tab = find_named(w.window.upcast_ref(), &format!("document-tab-{id}")).unwrap();
            tab.compute_bounds(&w.surface).unwrap()
        };
        let (first, pitch) = (bounds(1), bounds(2).x() - bounds(1).x());
        let (held, offsets) = w.documents.slide().expect("live tab slide");
        assert!(
            (held - first.x() - 60.).abs() < 1.,
            "held tab follows, touch={touch}"
        );
        assert_eq!(
            offsets,
            [0., 0., 0.],
            "neighbors wait for halfway, touch={touch}"
        );
        let halfway = [from[0] + pitch * 0.9, from[1]];
        input.perform(serde_json::json!([contact(device, "move", halfway)]));
        assert_eq!(
            w.documents.slide().unwrap().1,
            [0., -pitch, 0.],
            "first neighbor slides, touch={touch}"
        );
        input.perform(serde_json::json!([
            contact(device, "move", to),
            contact(device, "move", [to[0] - 2., to[1]])
        ]));
        let (held, offsets) = w.documents.slide().unwrap();
        assert_eq!(
            offsets,
            [0., -pitch, -pitch],
            "both neighbors slide, touch={touch}"
        );
        let last = bounds(3);
        assert!(
            (held + first.width() - last.x() - last.width()).abs() < 1.,
            "held tab stays in the strip, touch={touch}: {held} {first:?} {last:?}"
        );
        if !touch {
            crate::capture(&w, "/tmp/capy-document-tabs-slide.png");
        }
        let away = [to[0], to[1] + first.height() * 2.];
        input.perform(serde_json::json!([contact(device, "move", away)]));
        let (held, offsets) = w.documents.slide().unwrap();
        assert_eq!(
            offsets,
            [0., 0., 0.],
            "leaving the strip detaches, touch={touch}"
        );
        assert_eq!(held, first.x());
        input.perform(serde_json::json!([
            contact(device, "move", to),
            contact(device, "move", [to[0] - 2., to[1]])
        ]));
        assert_eq!(
            w.documents.slide().unwrap().1,
            [0., -pitch, -pitch],
            "returning reattaches, touch={touch}"
        );
        input.perform(serde_json::json!([contact(device, "up", to)]));
        assert_eq!(
            w.documents.model.borrow().order(),
            &[2, 3, 1],
            "immediate tab reorder, touch={touch}"
        );
        assert_eq!(w.documents.selected(), 3, "drag must not select on release");
        w.documents.model.borrow_mut().undo();
        w.documents.refresh(&w);
        pump(150);
        assert_eq!(w.documents.model.borrow().order(), &[1, 2, 3]);
        assert!(w.documents.model.borrow().can_redo());
        let from = point(1, 0.3);
        let to = point(3, 0.8);
        input.perform(serde_json::json!([
            contact(device, "down", from),
            contact(device, "move", to),
            {"key":0xff1b,"down":true},
            {"key":0xff1b,"down":false},
            contact(device, "up", to)
        ]));
        assert_eq!(
            w.documents.model.borrow().order(),
            &[1, 2, 3],
            "cancelled drag"
        );
        assert!(w.documents.slide().is_none());
        assert_eq!(bounds(1).x(), first.x(), "cancel restores tab widgets");
        let away = [to[0], to[1] + first.height() * 2.];
        input.perform(serde_json::json!([
            contact(device, "down", from),
            contact(device, "move", to),
            contact(device, "move", away),
            contact(device, "up", away)
        ]));
        assert_eq!(
            w.documents.model.borrow().order(),
            &[1, 2, 3],
            "release outside the strip cancels"
        );
        assert!(!w.documents.model.borrow().can_undo());
    }
    // Native clicks still select, and keyboard cycling follows visual order.
    let first = point(1, 0.3);
    input.click(first);
    until(
        || w.documents.selected() == 1 && !w.documents.changing.get(),
        "mouse tab selection",
    );
    new_photo::ready(&w);
    input.perform(
        serde_json::json!([{"key":0xffe3,"down":true},{"key":0xff09,"down":true},{"key":0xff09,"down":false},{"key":0xffe3,"down":false}]),
    );
    until(
        || w.documents.selected() == 2 && !w.documents.changing.get(),
        "Ctrl+Tab selection",
    );
    new_photo::ready(&w);
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for (modifier, key, target) in [(0xffe3, 0xff56, 3), (0xffe3, 0xff55, 2),
            (0xffe9, 0xff56, 3), (0xffe9, 0xff55, 2)] {
            w.dispatch(UiAction::Invoke { command: CommandId::Hand });
            w.area.grab_focus();
            input.perform(serde_json::json!([{ "key": modifier, "down": true }, { "key": key, "down": true },
                { "key": key, "down": false }, { "key": modifier, "down": false }]));
            until(|| w.documents.selected() == target && !w.documents.changing.get(), "modified Page key cycles drawings with Hand selected");
            new_photo::ready(&w);
        }
    }
    w.dispatch(UiAction::OpenSettings { page: SettingsPage::Shortcuts });
    w.dispatch(UiAction::Preferences { action: layer_ui::PreferenceAction::BeginShortcut { id: "command.NextDrawing".into() } });
    pump(250);
    input.key(0xffc5);
    w.dispatch(UiAction::Preferences { action: layer_ui::PreferenceAction::ConfirmShortcut { replace: true } });
    w.dispatch(UiAction::CloseSettings);
    pump(150);
    w.dispatch(UiAction::Invoke { command: CommandId::Hand });
    w.area.grab_focus();
    input.key(0xffc5);
    until(|| w.documents.selected() == 3 && !w.documents.changing.get(), "custom F8 binding cycles drawings through the native host");
    new_photo::ready(&w);
    w.dispatch(UiAction::OpenSettings { page: SettingsPage::Shortcuts });
    w.dispatch(UiAction::Preferences { action: layer_ui::PreferenceAction::ResetShortcut { id: "command.NextDrawing".into() } });
    w.dispatch(UiAction::CloseSettings);
    let last = point(3, 0.3);
    input.perform(serde_json::json!([{"touch":"down","point":last},{"touch":"up"}]));
    until(
        || w.documents.selected() == 3 && !w.documents.changing.get(),
        "touch tab selection",
    );
    new_photo::ready(&w);
    crate::capture(&w, "/tmp/capy-document-tabs-wide.png");
    // Exercise the actual pointer states for visual comparison with AdwTabBar.
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for (state, target) in [
            ("idle", [800., 200.]),
            ("hover", point(1, 0.4)),
            ("selected", point(3, 0.4)),
        ] {
            input.perform(serde_json::json!([{"point":target}]));
            crate::capture(&w, &format!("/tmp/capy-document-tabs-{name}-{state}.png"));
        }
        let tab = find_named(w.window.upcast_ref(), "document-tab-3").unwrap();
        let close = tab.last_child().unwrap();
        let b = close.compute_bounds(&w.window).unwrap();
        input.perform(serde_json::json!([{"point":[b.x()+b.width()/2., b.y()+b.height()/2.]}]));
        crate::capture(&w, &format!("/tmp/capy-document-tabs-{name}-close.png"));
        input.perform(serde_json::json!([{"point":point(3, 0.4)}, {"down":true}]));
        crate::capture(&w, &format!("/tmp/capy-document-tabs-{name}-pressed.png"));
        input.perform(serde_json::json!([{"down":false}]));
        assert!(tab.first_child().unwrap().grab_focus());
        w.window.set_focus_visible(true);
        pump(100);
        crate::capture(&w, &format!("/tmp/capy-document-tabs-{name}-focus.png"));
        assert!(!state(&w).workspace.zen_mode);
    }
    // Keyboard selector remains available when workspace customization removes
    // the title component; its native menu also advertises this command.
    let mut workspace = state(&w).workspace;
    for zone in &mut workspace.layout.header.zones {
        zone.retain(|e| e.item != HeaderItem::DocumentTitle);
    }
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(100);
    input.perform(
        serde_json::json!([{"key":0xffe3,"down":true},{"key":0xffe1,"down":true},{"key":0x61,"down":true},{"key":0x61,"down":false},{"key":0xffe1,"down":false},{"key":0xffe3,"down":false}]),
    );
    assert!(
        find_named(w.window.upcast_ref(), "drawing-selector-popup").is_some_and(|p| p.is_visible())
    );
    input.perform(serde_json::json!([{"key":0xff1b,"down":true},{"key":0xff1b,"down":false}]));
    input.finish();
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Wayland and GPU"]
fn native_document_tabs_failed_renderer_remains_navigable() {
    let app = native_test_app("art.capycanvas.TabFailure");
    let w = Workspace::with_project(&app, Some((new_drawing(96, 96, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)));
    w.window.present();
    new_photo::ready(&w);
    apply_fixture_theme(&w);
    new_photo::ready(&w);
    new_photo::invoke(&w, CommandId::AddLayer);
    let first = w.documents.selected();
    glib::MainContext::default()
        .block_on(
            w.documents
                .open(&w, (new_drawing(128, 128, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)),
        )
        .unwrap();
    new_photo::ready(&w);
    let second = w.documents.selected();
    let epoch = state(&w).document_file.epoch;
    ui_session(&w)
        .engine()
        .backend()
        .fail_next_frame();
    w.wake();
    until(
        || {
            ui_session(&w)
                .rendering_suspended()
        },
        "injected renderer failure",
    );
    switch(&w, first);
    assert!(state(&w).document_file.epoch > epoch);
    assert!(state(&w).document_file.modified);
    switch(&w, second);
    assert!(
        !ui_session(&w)
            .rendering_suspended()
    );
    assert!(!state(&w).document_file.modified);
    // The restored clean tab closes while the dirty drawing is retained.
    w.documents.select(&w, second, true);
    until(
        || w.documents.len() == 1 && !w.documents.changing.get(),
        "recovered renderer tab closes",
    );
    assert_eq!(w.documents.selected(), first);
    assert!(state(&w).document_file.modified);
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Wayland and private session directory"]
fn native_session_restart() {
    let (app,windows)=crate::application("art.capycanvas.SessionRestart");
    let app=NativeTestApp(app);app.register(None::<&gtk::gio::Cancellable>).unwrap();
    crate::open_workspace(&app,&windows,Some((new_drawing(256,256,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),None)));
    until(||windows.borrow().first().is_some_and(|w|w.window.is_mapped()),"initial window mapped");
    let w=windows.borrow()[0].clone();new_photo::ready(&w);apply_fixture_theme(&w);
    new_photo::invoke(&w,CommandId::AddLayer);
    new_photo::invoke(&w,CommandId::ZoomIn);
    let camera=state(&w).camera;
    let layers=ui_session(&w).engine().document().scene().order().len();
    glib::MainContext::default().block_on(w.documents.open(&w,(new_drawing(128,96,&w.localization()).unwrap(),None))).unwrap();
    new_photo::ready(&w);
    w.documents.model.borrow_mut().reorder(2,Some(1));
    switch(&w,1);
    w.window.close();
    until(||!w.window.is_visible(),"quit flushed both drawings");
    assert!(w.window.visible_dialog().is_none());
    assert!(windows.borrow().is_empty());
    drop(w);pump(100);
    crate::open_workspace(&app,&windows,None);
    until(||windows.borrow().first().is_some_and(|w|!w.restart.is_restoring()&&!w.documents.changing.get()),"session restored automatically");
    let w=windows.borrow()[0].clone();new_photo::ready(&w);
    assert_eq!(w.documents.len(),2);
    assert!(w.window.visible_dialog().is_none());
    assert_eq!(w.documents.model.borrow().order(),&[2,1]);
    assert_eq!(w.documents.selected(),1);
    assert!(state(&w).document_file.modified);
    assert!(!state(&w).document_file.recovered);
    assert_eq!(state(&w).camera.zoom,camera.zoom);
    assert_eq!(ui_session(&w).engine().document().scene().order().len(),layers);
    new_photo::invoke(&w,CommandId::Undo);
    assert_eq!(ui_session(&w).engine().document().scene().order().len(),layers-1);
    new_photo::invoke(&w,CommandId::Redo);
    assert_eq!(ui_session(&w).engine().document().scene().order().len(),layers);
    w.documents.select(&w,1,true);
    until(||w.window.visible_dialog().is_some(),"explicit dirty close asks");
    new_photo::response(&w,"cancel");until(||!w.servicing.get(),"cancel close acknowledged");
    assert_eq!(w.documents.len(),2);
    w.documents.select(&w,1,true);
    until(||w.window.visible_dialog().is_some(),"explicit dirty close asks again");
    new_photo::response(&w,"discard");
    until(||w.documents.len()==1&&!w.documents.changing.get(),"discarded drawing removed");
    assert_eq!(w.documents.selected(),2);
    assert!(!state(&w).document_file.modified);
    w.documents.select(&w,2,true);
    until(||!w.window.is_visible(),"last explicitly closed drawing exits");
}

#[test]
#[ignore = "isolated Wayland and private session directory"]
fn native_session_restart_saved_origins() {
    let (app,windows)=crate::application("art.capycanvas.SessionSavedOrigins");
    let app=NativeTestApp(app);app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let root=crate::storage::roots().unwrap().data.join("originals");
    std::fs::create_dir_all(&root).unwrap();
    let paths=[root.join("intact.capy"),root.join("missing.capy"),root.join("changed.capy")];
    let mut drawing=new_drawing(128,96,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    drawing.working.selection=Some(layer_core::Selection::polygon(vec![Point{x:8.,y:8.},Point{x:96.,y:8.},Point{x:96.,y:72.}]).unwrap());
    crate::open_workspace(&app,&windows,Some((drawing,None)));
    until(||windows.borrow().first().is_some_and(|w|w.window.is_mapped()),"saved-origin window mapped");
    let w=windows.borrow()[0].clone();new_photo::ready(&w);apply_fixture_theme(&w);
    let mut overlay_id=None;
    let mut overlay_occurrence_id=None;
    for (i,path) in paths.iter().enumerate() {
        if i>0 {glib::MainContext::default().block_on(w.documents.open(&w,(new_drawing(128,96,&w.localization()).unwrap(),None))).unwrap();new_photo::ready(&w);}
        new_photo::invoke(&w,CommandId::AddLayer);
        if i==0 {
            new_photo::invoke(&w,CommandId::SaveSelectionLayer);
            w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::CancelRename});new_photo::ready(&w);
        }
        crate::files::choose_next_save(path.clone());
        new_photo::invoke(&w,CommandId::SaveDocument);
        until(||state(&w).document_file.location.is_some()&&!state(&w).document_file.busy&&!w.servicing.get(),"saved origin acknowledged");
        assert!(!state(&w).document_file.modified,"{:?}",state(&w).host_error);
        if i==0 {
            let before=ui_session(&w).engine().document().clone();
            let checkpoint=ui_session(&w).engine().checkpoint();
            let occurrence=ui_session(&w).engine().document().working.occurrence.unwrap();
            let layer_core::authored::SourceTarget::Selection(handle)=ui_session(&w).engine().document().scene().source_target(occurrence).unwrap() else {panic!("saved selection target")};
            overlay_id=Some(before.artwork.selections.id(handle).unwrap());
            overlay_occurrence_id=Some(before.artwork.occurrences.id(occurrence).unwrap());
            w.window.maximize();until(||w.window.is_maximized(),"selection window maximized");
            super::pointwise::configure_properties(&w);
            until(||widgets(w.window.upcast_ref()).any(|widget|widget.widget_name()=="property-mask_opacity" && widget.is_mapped()),"saved-selection opacity control mapped");
            let overlay_before=ui_session(&w).engine().backend().capture().unwrap().bytes;
            let mut input=RemoteInput::new().timeout_secs(30);input.ready();
            let control=super::pointwise::number(&w,"mask_opacity");super::histogram::scroll_to(control.upcast_ref());
            input.click(screen_point(&find_css(control.upcast_ref(),"number-value").unwrap(),&w.window,[0.5,0.5]));
            descendant::<gtk::Entry>(&control).unwrap().set_text("37");input.key(0xff0d);new_photo::ready(&w);
            assert_ne!(ui_session(&w).engine().backend().capture().unwrap().bytes,overlay_before,"native canvas updates saved-selection overlay opacity");
            w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Layers,visible:true});
            w.dispatch(UiAction::MovePanel {panel:Panel::Layers,target:DockTarget::Edge {edge:Edge::Left,outer:false},viewport:[w.window.width() as f32,800.]});new_photo::ready(&w);
            let row=named::<gtk::Box>(w.layer_panel.root.upcast_ref(),&format!("art-layer-{}",layer_ui::occurrence_token(occurrence)));
            let eye=row.first_child().unwrap();histogram::scroll_to(&eye);
            until(||eye.is_mapped(),"saved-selection eye mapped");
            input.click(screen_point(&eye,&w.window,[0.5,0.5]));new_photo::ready(&w);input.finish();
            assert_eq!(ui_session(&w).engine().document().working.selection_overlays.visibility.get(&occurrence),Some(&false));
            assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint,"selection display edits preserve the authored checkpoint");
            assert_eq!(ui_session(&w).engine().document().artwork,before.artwork);
            assert_live_artwork_eq(ui_session(&w).engine().document(),&before);
            assert!(!state(&w).document_file.modified,"overlay edits keep saved artwork clean");
            assert_eq!(ui_session(&w).engine().document().working.selection_overlays.properties[&handle].opacity,0.37);
            let (_,bytes)=super::pointwise::saved_artwork(&w);
            let reopened=open_native_document(std::io::Cursor::new(bytes));
            assert!(reopened.working.selection_overlays.properties.is_empty(),"portable artwork omits selection display properties");
            assert!(reopened.working.selection_overlays.visibility.is_empty(),"portable artwork omits selection overlay visibility");
            assert_eq!(reopened.artwork.selections.get(reopened.artwork.selections.resolve(overlay_id.unwrap()).unwrap()),before.artwork.selections.get(handle));
            assert_eq!(reopened.artwork.occurrences.get(reopened.artwork.occurrences.resolve(overlay_occurrence_id.unwrap()).unwrap()),before.artwork.occurrences.get(occurrence));
        }
    }
    switch(&w,1);w.window.close();
    until(||!w.window.is_visible()&&windows.borrow().is_empty(),"saved drawings quit without prompting");
    assert!(w.window.visible_dialog().is_none());drop(w);pump(100);
    let original=open_native_document(std::fs::File::open(&paths[1]).unwrap());
    std::fs::remove_file(&paths[1]).unwrap();
    let mut external=std::fs::read(&paths[2]).unwrap();external[0]^=1;std::fs::write(&paths[2],&external).unwrap();
    crate::open_workspace(&app,&windows,None);
    until(||windows.borrow().first().is_some_and(|w|!w.restart.is_restoring()&&!w.documents.changing.get()),"saved origins automatically restored");
    let w=windows.borrow()[0].clone();new_photo::ready(&w);
    assert_eq!(w.documents.len(),3);
    assert_eq!(w.documents.selected(),1);assert!(w.window.visible_dialog().is_none());
    assert!(!state(&w).document_file.modified);assert!(!state(&w).document_file.recovered);
    let handle=ui_session(&w).engine().document().artwork.selections.resolve(overlay_id.unwrap()).unwrap();
    assert_eq!(ui_session(&w).engine().document().working.selection_overlays.properties[&handle].opacity,0.37,"native restart restores private selection display properties");
    let occurrence=ui_session(&w).engine().document().artwork.occurrences.resolve(overlay_occurrence_id.unwrap()).unwrap();
    assert_eq!(ui_session(&w).engine().document().working.selection_overlays.visibility.get(&occurrence),Some(&false),"native restart restores private selection visibility");
    assert!(ui_session(&w).engine().document().scene().occurrence(occurrence).unwrap().visible);
    assert!(!state(&w).layers.iter().find(|layer|layer.id==layer_ui::occurrence_token(occurrence)).unwrap().visible);
    w.documents.select(&w,1,true);
    until(||w.documents.len()==2&&!w.documents.changing.get(),"intact saved original closes cleanly");
    assert!(w.window.visible_dialog().is_none());
    switch(&w,3);assert!(state(&w).document_file.modified);assert!(!state(&w).document_file.recovered);
    w.documents.select(&w,3,true);until(||w.window.visible_dialog().is_some(),"changed saved original close asks");
    new_photo::response(&w,"cancel");until(||!w.servicing.get(),"changed-origin cancel acknowledged");
    assert_eq!(w.documents.len(),2);assert!(w.recovery().published_path().join("head.json").exists());
    assert_eq!(std::fs::read(&paths[2]).unwrap(),external);
    w.documents.select(&w,3,true);until(||w.window.visible_dialog().is_some(),"changed saved original close asks again");
    new_photo::response(&w,"discard");until(||w.documents.len()==1&&!w.documents.changing.get(),"explicit discard retires changed-origin session");
    assert_eq!(std::fs::read(&paths[2]).unwrap(),external);
    assert_eq!(w.documents.selected(),2);assert!(state(&w).document_file.modified);
    w.documents.select(&w,2,true);until(||w.window.visible_dialog().is_some(),"missing saved original close asks");
    new_photo::response(&w,"cancel");until(||!w.servicing.get(),"missing-origin cancel acknowledged");
    assert_eq!(w.documents.len(),1);assert!(!paths[1].exists());
    let copy=root.join("missing-saved-copy.capy");crate::files::choose_next_save(copy.clone());
    w.documents.select(&w,2,true);until(||w.window.visible_dialog().is_some(),"missing saved original close offers save");
    new_photo::response(&w,"save");until(||!w.window.is_visible(),"Save As protects missing original before close");
    assert!(copy.exists());assert!(!paths[1].exists());assert_eq!(std::fs::read(&paths[2]).unwrap(),external);
    let mut saved=open_native_document(std::fs::File::open(copy).unwrap());
    saved.artwork.outputs.get_mut(saved.artwork.default_output).unwrap().context.elapsed=original.output().context.elapsed;
    assert_live_artwork_eq(&saved,&original);
}

#[test]
#[ignore = "isolated Wayland and GPU"]
fn native_document_tabs_immediate_stroke_and_undo() {
    let app = native_test_app("art.capycanvas.TabPendingStroke");
    let w = Workspace::with_project(&app, Some((new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)));
    w.window.present();
    new_photo::ready(&w);
    apply_fixture_theme(&w);
    new_photo::ready(&w);
    w.documents.ram_budget.set(0);
    glib::MainContext::default()
        .block_on(
            w.documents
                .open(&w, (new_drawing(96, 96, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)),
        )
        .unwrap();
    new_photo::ready(&w);
    let second = w.documents.selected();
    switch(&w, 1);
    for undo in [false, true] {
        let root = {
            let mut gpu = w.gpu.borrow_mut();
            let session = &mut gpu.as_mut().unwrap().session;
            let camera = session.state().camera.clone();
            let now = glib::monotonic_time() as u64 * 1000;
            for (i, (phase, x)) in [(PenPhase::Down, 50.), (PenPhase::Up, 180.)]
                .into_iter()
                .enumerate()
            {
                session
                    .pen(PenEvent {
                        device_id: 71,
                        timestamp_ns: now + i as u64,
                        pressure: 0.7,
                        ..pen_event(&camera, [x, 120.], phase, now + i as u64)
                    })
                    .unwrap();
            }
            if undo {
                // Submit, then immediately move its still-pending capture to redo.
                session.frame(now + 2, now + 2).unwrap();
                let root = active_paint(session.engine().document()).raster.clone();
                session
                    .dispatch(UiAction::Invoke {
                        command: CommandId::Undo,
                    })
                    .unwrap();
                Some(root)
            } else {
                None
            }
        };
        // No event-loop pumping between contact/Undo and the switch request.
        switch(&w, second);
        assert_eq!(w.documents.parked_memory(), (0, true));
        if let Some(root) = &root {
            assert!(
                root.host_backed(),
                "redo backing resolves before worker stops"
            );
        }
        switch(&w, 1);
        if undo {
            new_photo::invoke(&w, CommandId::Redo);
            new_photo::ready(&w);
        }
        let document = ui_session(&w)
            .engine()
            .document()
            .clone();
        let raster = &active_paint(&document).raster;
        if let Some(root) = root {
            assert_eq!(raster.identity(), root.identity());
        }
        let data = raster.wait_data().unwrap();
        assert!(!data.tiles.is_empty());
        assert!(data.tiles.values().any(|t| {
            t.wait_backing()
                .unwrap()
                .decode()
                .unwrap()
                .chunks_exact(4)
                .any(|pixel| pixel[3] > 0)
        }));
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Wayland and private storage"]
fn native_document_tabs_disk_failure_keeps_data() {
    let temp = &crate::storage::roots().unwrap().temp;
    let _ = std::fs::remove_dir_all(temp);
    std::fs::write(temp, b"").unwrap();
    let app = native_test_app("art.capycanvas.TabDiskFailure");
    let mut project = new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let blob = std::sync::Arc::new(
        TileBlob::encode(
            project.composition().color.paint_descriptor(),
            &vec![128; 256 * 256 * 4],
        )
        .unwrap(),
    );
    active_paint_mut(&mut project).raster = RasterRevision::backed(RasterData {
        tiles: [(
            TileKey {
                plane: RasterPlane::Color,
                coordinate: [0, 0],
            },
            RasterTile::backed_shared(blob.clone()),
        )]
        .into(),
        ..Default::default()
    });
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    new_photo::ready(&w);
    apply_fixture_theme(&w);
    new_photo::ready(&w);
    w.documents.ram_budget.set(0);
    glib::MainContext::default()
        .block_on(
            w.documents
                .open(&w, (new_drawing(96, 96, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)),
        )
        .unwrap();
    new_photo::ready(&w);
    assert!(w.documents.parked_memory().0 > 0);
    assert!(
        w.documents.parked_memory().1,
        "failure still releases inactive GPU"
    );
    assert_eq!(blob.decode().unwrap(), vec![128; 256 * 256 * 4]);
    let error = glib::MainContext::default()
        .block_on(
            w.documents
                .open(&w, (new_drawing(80, 80, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)),
        )
        .unwrap_err();
    assert!(error.contains("Free disk space"));
    assert_eq!(w.documents.len(), 2);
    std::fs::remove_file(temp).unwrap();
    switch(&w, 1);
    let captured = ui_session(&w)
        .capture_artwork()
        .unwrap();
    let mut saved = Vec::new();
    write_capture(&captured, &mut saved).unwrap();
    open_native_document(std::io::Cursor::new(saved.as_slice()));
    w.documents.select(&w, 1, true);
    until(
        || w.documents.len() == 1 && !w.documents.changing.get(),
        "storage failure does not prevent closing",
    );
    w.window.destroy();
    pump(100);
}

fn begin_preserved_import(w: &Rc<Workspace>, imported: layer_ui::ImportedDocument, source: &std::path::Path) -> Rc<RefCell<Option<Result<(),String>>>> {
    let completed = Rc::new(RefCell::new(None));
    let previous = w.window.visible_dialog();
    let file = gtk::gio::File::for_path(source);
    let location = layer_ui::DocumentLocation {uri:file.uri().to_string(),name:"Original.capy".into()};
    glib::spawn_future_local(glib::clone!(#[strong] w, #[strong] completed, async move {
        *completed.borrow_mut() = Some(w.documents.open_imported(&w, imported, Some(location)).await);
    }));
    until(|| w.window.visible_dialog().is_some_and(|dialog|Some(&dialog)!=previous.as_ref()
        && dialog.widget_name()=="preserved-package-preview" && dialog.is_mapped()
        && dialog.downcast_ref::<adw::AlertDialog>().and_then(|dialog|dialog.extra_child()).is_some_and(|picture|picture.is_mapped())),"unsupported admission presents the package preview");
    completed
}

#[test]
#[ignore = "isolated Wayland and GPU"]
fn native_import_admission_failure_preserves_package_and_current_drawing() {
    use layer_core::package::{codec::{CapturedPreview,PreparedPackage},preview::Preview};
    use layer_render_wgpu::snapshot::{CaptureControl,SnapshotPreview};
    let app = native_test_app("art.capycanvas.ImportAdmission");
    let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    let w = Workspace::with_project(&app, Some((new_drawing(64,48,&localization).unwrap(), None)));
    w.window.present();
    new_photo::ready(&w);
    new_photo::invoke(&w, CommandId::AddLayer);
    let original = ui_session(&w).engine().document().clone();
    let checkpoint = ui_session(&w).engine().checkpoint();
    let selected = w.documents.selected();
    let directory = std::env::temp_dir().join(format!("capy-import-admission-{}",layer_core::PortableId::random()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("Original.capy");
    let symbolic = directory.join("Symbolic.png");
    let linked = directory.join("Linked.png");
    let document = layer_ui::NewDocumentOptions {extent:[80,60],background:layer_ui::DocumentBackground::White,..Default::default()}.project(&localization).unwrap();
    let backdrop = *document.scene().constant_backdrop().first().unwrap();
    let paper = document.scene().occurrence(backdrop).unwrap();
    assert_eq!(document.scene().effect(backdrop).unwrap().constant_color(),Some(layer_core::color::RgbColor::WHITE));
    assert!(paper.visible);
    assert_eq!(paper.opacity,1.);
    let context = document.output().context.clone();
    let capture = layer_core::Editor::new(document).capture(0,context).unwrap();
    let gpu = ui_session(&w).engine().backend().snapshot_gpu().unwrap();
    let path = source.clone();
    let preview = glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
        let mut renderer = gpu.capture(capture.clone(),CaptureControl::default()).unwrap();
        let pixels = renderer.read_region([0,0,80,60]).unwrap();
        let image = SnapshotPreview {extent:[80,60],space:renderer.color().space,pixels}.srgb_bytes().unwrap();
        assert!(image.chunks_exact(4).all(|pixel|pixel==[255,255,255,255]));
        let preview = Preview::from_rgba([80,60],image.into()).unwrap();
        let paired = CapturedPreview {checkpoint:capture.checkpoint,context:capture.output().context.clone(),preview:preview.clone()};
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        PreparedPackage::prepare(&capture,Some(paired),&cancelled).unwrap().write(&mut std::fs::File::create(path).unwrap(),&cancelled).unwrap();
        preview
    })).unwrap();
    std::os::unix::fs::symlink(&source,&symbolic).unwrap();
    std::fs::hard_link(&source,&linked).unwrap();
    let bytes = std::fs::read(&source).unwrap();
    crate::storage::roots();
    let imported = match layer_ui::read_import(std::io::Cursor::new(bytes.clone()),layer_ui::ImportIntent::Open,Default::default(),layer_ui::photo_document_names("Original.capy",&localization),Default::default(),Default::default(),&std::sync::atomic::AtomicBool::new(false)).unwrap() {
        layer_ui::ImportOutcome::Editable(imported) => imported,
        _ => panic!("Expected supported native package"),
    };
    w.documents.model.borrow_mut().budget.metadata = 0;
    let refusal = localization.text(layer_ui::MessageId::DOCUMENTS_PACKAGE_CHOOSE_DIFFERENT).to_string();
    for (theme,name) in [(Theme::Light,"light"),(Theme::Dark,"dark")] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        pump(100);
        let completed = begin_preserved_import(&w,imported.clone(),&source);
        let dialog = w.window.visible_dialog().unwrap().downcast::<adw::AlertDialog>().unwrap();
        assert!(dialog.is_response_enabled("copy"));
        assert!(dialog.is_response_enabled("export"));
        assert!(!dialog.has_response("edit"));
        assert!(!dialog.has_response("save"));
        assert_eq!(dialog.response_label("copy"),localization.text(layer_ui::MessageId::DOCUMENTS_PACKAGE_COPY_ORIGINAL).as_ref());
        assert_eq!(dialog.response_label("export"),localization.text(layer_ui::MessageId::DOCUMENTS_PACKAGE_EXPORT_PREVIEW).as_ref());
        let picture = dialog.extra_child().unwrap().downcast::<gtk::Picture>().unwrap();
        assert!(picture.is_mapped());
        let texture = picture.paintable().unwrap().downcast::<gtk::gdk::Texture>().unwrap();
        assert_eq!([texture.width(),texture.height()],[80,60]);
        let mut shown = vec![0;80*60*4];
        texture.download(&mut shown,80*4);
        assert!(shown.chunks_exact(4).all(|pixel|pixel==[255,255,255,255]));
        assert_eq!(w.documents.selected(),selected);
        assert_eq!(w.documents.len(),1);
        assert_eq!(ui_session(&w).engine().document(),&original);
        assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint);
        if let Ok(output) = std::env::var("LAYER_NATIVE_CAPTURE_DIR") {
            std::fs::create_dir_all(&output).unwrap();
            crate::capture(&w,&format!("{output}/preserved-package-{name}.png"));
        }
        let copy = directory.join(format!("Copy-{name}.capy"));
        crate::files::choose_next_save(copy.clone());
        new_photo::response(&w,"copy");
        until(||copy.exists()&&w.window.visible_dialog().is_some_and(|current|current!=dialog.clone().upcast::<adw::Dialog>()
            && current.widget_name()=="preserved-package-preview" && current.is_mapped()),"original package copied from retained backing");
        assert_eq!(std::fs::read(copy).unwrap(),bytes);
        let image = directory.join(format!("Preview-{name}.png"));
        crate::files::choose_next_save(image.clone());
        let previous = w.window.visible_dialog().unwrap();
        new_photo::response(&w,"export");
        until(||image.exists()&&w.window.visible_dialog().is_some_and(|current|current!=previous
            && current.widget_name()=="preserved-package-preview" && current.is_mapped()),"verified preview exported separately");
        let encoded = std::fs::read(image).unwrap();
        assert_eq!(encoded.as_slice(),preview.encoded().as_ref());
        let decoded = Preview::decode(encoded.into()).unwrap();
        assert_eq!(decoded.size(),[80,60]);
        assert!(decoded.pixels().chunks_exact(4).all(|pixel|pixel==[255,255,255,255]));
        new_photo::response(&w,"close");
        until(||completed.borrow().is_some(),"preserved view closes without adoption");
        assert_eq!(completed.borrow_mut().take().unwrap(),Ok(()));
        for destination in [&source,&symbolic,&linked] {
            let completed = begin_preserved_import(&w,imported.clone(),&source);
            crate::files::choose_next_save(destination.to_owned());
            new_photo::response(&w,"export");
            until(||completed.borrow().is_some(),"original destination refused before publication");
            assert_eq!(completed.borrow_mut().take().unwrap(),Err(refusal.clone()));
            assert_eq!(std::fs::read(&source).unwrap(),bytes);
            assert_eq!(std::fs::read(destination).unwrap(),bytes);
            assert_eq!(ui_session(&w).engine().document(),&original);
            assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint);
            assert_eq!(w.documents.selected(),selected);
            assert_eq!(w.documents.len(),1);
        }
    }
    w.documents.cancel_open.set(true);
    assert!(glib::MainContext::default().block_on(w.documents.open_imported(&w,imported,None)).is_err());
    assert!(w.window.visible_dialog().is_none());
    assert_eq!(ui_session(&w).engine().document(),&original);
    assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint);
    w.documents.cancel_open.set(false);
    w.window.destroy();
    pump(100);
    std::fs::remove_dir_all(directory).unwrap();
}
