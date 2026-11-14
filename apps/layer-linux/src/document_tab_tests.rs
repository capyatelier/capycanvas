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
    let corner = std::cell::Cell::new(26u32);
    let window_style = |radius| format!("#transparency-sentinel {{ background: #ff00ff; }} window.capy-workspace:not(.maximized):not(.fullscreen) {{ --window-radius: {radius}px; }}");
    css.load_from_string(&window_style(corner.get()));
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
    let capture = |input: &mut RemoteInput, name: &str, expected: Option<[u8; 3]>| -> [u8; 3] {
        input.perform(serde_json::json!([{"wait_ms":250}, {"capture":name}]));
        let mut reader =
            png::Decoder::new(std::fs::File::open(captures.join(format!("{name}.png"))).unwrap())
                .read_info()
                .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgb);
        let center = ((info.height / 2 * info.width + info.width / 2) * 3) as usize;
        let expected = expected.unwrap_or_else(|| pixels[center..center + 3].try_into().unwrap());
        let sample_x = if name == "startup-wait" {
            let mark_pixels = (info.height / 2 - 80..info.height / 2 + 80).flat_map(|y| {
                (info.width / 2 - 80..info.width / 2 + 80).map(move |x| ((y * info.width + x) * 3) as usize)
            }).filter(|&i| pixels[i..i + 3].iter().zip(expected).any(|(a, b)| a.abs_diff(b) > 24)).count();
            assert!(mark_pixels > 100, "the centered launch mark is visible above the waiting canvas");
            info.width / 2 + 160
        } else { info.width / 2 };
        for y in info.height / 2 - 20..info.height / 2 + 20 {
            for x in sample_x - 20..sample_x + 20 {
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
        if matches!(name, "switch-resized" | "switch-light" | "switch-ready" | "radius-changed" | "restart-ready") {
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
            let r = corner.get() * scale;
            let cut = corner.get() / 4 * scale;
            let pixel = |x, y| {
                let i = ((y * info.width + x) * 3) as usize;
                &pixels[i..i + 3]
            };
            if name == "switch-resized" {
                for (x, y) in [
                    (left + r, top + scale),
                    (right - r - 1, top + scale),
                    (left + r, bottom - scale - 1),
                    (right - r - 1, bottom - scale - 1),
                    (left + scale, top + r),
                    (left + scale, bottom - r - 1),
                    (right - scale - 1, top + r),
                    (right - scale - 1, bottom - r - 1),
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
            }
            for (x, y) in [
                (left, top),
                (right - 1, top),
                (left, bottom - 1),
                (right - 1, bottom - 1),
                (left + cut, top + cut),
                (right - cut - 1, top + cut),
                (left + cut, bottom - cut - 1),
                (right - cut - 1, bottom - cut - 1),
            ] {
                let p = pixel(x, y);
                assert!(
                    p[0] > 150 && p[1] < 20 && p[2] > 150,
                    "rounded corner must expose the sentinel: {p:?}"
                );
            }
        }
        expected
    };
    let sentinel = capture(&mut input, "behind", None);
    assert!(
        sentinel[0] > 250 && sentinel[1] < 20 && sentinel[2] > 250,
        "the capture shows the sentinel window: {sentinel:?}"
    );
    let mut palette_failures = Vec::new();
    let surround = [[behind.width() as f32 * 0.5, behind.height() as f32 * 0.8]];
    let check_palette = |name: &str, frames: &[[[u8; 3]; 1]], expected: [u8; 3], failures: &mut Vec<String>| {
        let mut visible = 0;
        for (index, [pixel]) in frames.iter().enumerate() {
            let delta = std::array::from_fn::<_, 3, _>(|c| expected[c] as f32 - sentinel[c] as f32);
            let length = delta.iter().map(|v| v * v).sum::<f32>();
            let opacity = (0..3).map(|c| (pixel[c] as f32 - sentinel[c] as f32) * delta[c]).sum::<f32>() / length;
            if opacity < 0.05 { continue; }
            visible += 1;
            if (0..3).any(|c| (pixel[c] as f32 - (sentinel[c] as f32 + opacity * delta[c])).abs() > 4.) {
                failures.push(format!("{name} frame {index}: startup/reveal surround {pixel:?}, expected {expected:?} over {sentinel:?}"));
            }
        }
        if visible == 0 { failures.push(format!("{name}: no presented editor surround")); }
    };
    let prepare = |w: &Rc<Workspace>, theme| {
        w.dispatch(UiAction::RestoreSettings { settings: Box::new(layer_ui::Settings {theme:Some(theme),..Default::default()}) });
        w.window.maximize();
        gtk::prelude::WidgetExt::realize(&w.window);
        w.surface.realize();
        w.area.realize();
        wait_workspaces(w);
        w.dispatch(UiAction::SetZoom { zoom: 0.1 });
    };
    let light = Workspace::with_project(&app, Some((new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)));
    prepare(&light, Theme::Light);
    let frames = super::ui_motion::record_pixels(&light, &mut input, "first-startup-light", surround, || { light.window.present(); new_photo::ready(&light); });
    check_palette("first-startup-light", &frames, [184; 3], &mut palette_failures);
    light.window.destroy();
    pump(400);
    let pause = crate::render_thread::pause_next_startup();
    let w = Workspace::with_project(&app, Some((new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)));
    prepare(&w, Theme::Dark);
    w.window.present();
    // Allow the compositor's window-opening fade to finish; the GPU remains
    // paused, so this cannot hide a missing application background.
    pump(600);
    capture(&mut input, "startup-wait", Some([51; 3]));
    assert!(
        !ui_session(&w)
            .engine()
            .backend()
            .startup
            .complete
    );
    w.dispatch(UiAction::SetZoom { zoom: 0.1 });
    let frames = super::ui_motion::record_pixels(&w, &mut input, "first-startup-dark", surround, || {
        pause.store(false, Ordering::Release);
        new_photo::ready(&w);
    });
    check_palette("first-startup-dark", &frames, [51; 3], &mut palette_failures);
    let tab = {
        let groups = w.groups.borrow();
        let group = groups.iter().find(|group| group.tabs_visible).unwrap();
        let tab = group.tabs.iter().find(|(_, tab)| tab.has_css_class("selected-tool")).unwrap().1.compute_bounds(&w.surface).unwrap();
        [tab.x() + 4., tab.y() + tab.height() - 3.]
    };
    w.window.set_visible(false);
    pump(400);
    w.dispatch(UiAction::SetTheme { theme: Some(Theme::Light) });
    let hidden = super::ui_motion::record_pixels(&w, &mut input, "hidden-theme-reveal", [surround[0], tab], || w.window.present());
    behind.present();
    pump(200);
    assert!(!w.window.is_active(), "sentinel takes focus before the unfocused theme change");
    w.dispatch(UiAction::SetTheme { theme: Some(Theme::Dark) });
    let unfocused = super::ui_motion::record_pixels(&w, &mut input, "unfocused-theme-reveal", [surround[0], tab], || w.window.present());
    let light_panel = hidden.last().unwrap()[1];
    let dark_panel = unfocused.last().unwrap()[1];
    for (name, frames, expected, new_panel, old_panel) in [
        ("hidden-theme-reveal", &hidden, [184; 3], light_panel, dark_panel),
        ("unfocused-theme-reveal", &unfocused, [51; 3], dark_panel, light_panel),
    ] {
        let mut checked = 0;
        for (index, [pixel, panel]) in frames.iter().enumerate() {
            let distance = |a: [u8; 3], b: [u8; 3]| a.into_iter().zip(b).map(|(a, b)| u32::from(a.abs_diff(b)).pow(2)).sum::<u32>();
            if distance(*panel, new_panel) > 12 || distance(*panel, old_panel) < 100 { continue; }
            checked += 1;
            if pixel.iter().zip(expected).any(|(actual, expected)| actual.abs_diff(expected) > 4) {
                palette_failures.push(format!("{name} frame {index}: new native palette {panel:?} with surround {pixel:?}, expected {expected:?}"));
            }
        }
        if checked == 0 { palette_failures.push(format!("{name}: no new native palette frame with distinct theme endpoints")); }
    }
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(300);
    capture(&mut input, "startup-ready", Some([255; 3]));

    let pause = crate::render_thread::pause_next_startup();
    glib::MainContext::default()
        .block_on(
            w.documents
                .open(&w, (new_drawing(128, 128, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)),
        )
        .unwrap();
    capture(&mut input, "new-tab-wait", Some([255; 3]));
    assert!(
        w.documents.parked_memory().1,
        "inactive GPU worker still stops"
    );
    pause.store(false, Ordering::Release);
    new_photo::ready(&w);
    capture(&mut input, "new-tab-ready", Some([255; 3]));

    let pause = crate::render_thread::pause_next_startup();
    glib::MainContext::default()
        .block_on(w.documents.activate(&w, 1))
        .unwrap();
    capture(&mut input, "switch-wait", Some([255; 3]));
    w.window.unmaximize();
    w.window.set_default_size(1100, 750);
    pump(350);
    capture(&mut input, "switch-resized", Some([51; 3]));
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Light),
    });
    capture(&mut input, "switch-light", Some([184; 3]));
    pause.store(false, Ordering::Release);
    new_photo::ready(&w);
    capture(&mut input, "switch-ready", Some([255; 3]));
    corner.set(36);
    css.load_from_string(&window_style(corner.get()));
    capture(&mut input, "radius-changed", Some([255; 3]));

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
    capture(&mut input, "failed-worker", Some([184; 3]));
    // Simulate the opaque GTK fallback after a background transport failure.
    // A successful restart must reveal the new drawing again, not mask it.
    w.window.remove_css_class("native-canvas-background");
    let pause = crate::render_thread::pause_next_startup();
    w.restart_gpu();
    capture(&mut input, "restart-wait", Some([184; 3]));
    pause.store(false, Ordering::Release);
    new_photo::ready(&w);
    capture(&mut input, "restart-ready", Some([255; 3]));
    w.window.destroy();
    capture(&mut input, "closed", Some(sentinel));
    behind.destroy();
    input.finish();
    assert!(palette_failures.is_empty(), "{}", palette_failures.join("\n"));
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
    let mut workspace = state(&w).workspace;
    workspace.layout.header = layer_ui::HeaderLayout::default();
    w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
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
    super::new_drawing_review::open_new_drawing(&w);
    for action in [NewDrawingAction::Unit { unit: LengthUnit::Px }, NewDrawingAction::Width { value: 128. }, NewDrawingAction::Height { value: 96. }] {
        super::new_drawing_review::new_drawing_action(&w, action);
    }
    click(&super::new_drawing_review::part::<gtk::Button>(&w, "new-drawing-create"));
    until(
        || w.documents.len() == 2 && !w.documents.changing.get(),
        "New creates a tab",
    );
    new_photo::ready(&w);
    let second = w.documents.selected();
    assert_eq!(app.windows().len(), 1);
    assert_eq!(windows.borrow().len(), 1);
    assert_eq!(w.documents.len(), 2);
    until(|| w.documents.parked_memory() == (0, true), "inactive CPU tiles spilled and renderer joined");
    assert!(!state(&w).document_file.modified);
    new_photo::invoke(&w, CommandId::AddLayer);
    w.recovery().capture(&w);
    glib::MainContext::default().block_on(w.recovery().drain());
    let mut owners=w.documents.model.borrow().parked().map(|(_,p)|p.owner.recovery.clone()).collect::<Vec<_>>();
    owners.push(w.recovery());
    for owner in &owners { glib::MainContext::default().block_on(owner.flush()).unwrap(); }
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
            .decode_on_worker()
            .unwrap(),
        vec![128; 256 * 256 * 4]
    );
    // Repeated selection must not create a window or leave a second live worker.
    for _ in 0..3 {
        switch(&w, second);
        switch(&w, first);
    }
    until(|| w.documents.parked_memory() == (0, true), "inactive storage completed");
    let mut temporary = Vec::new();
    for _ in 0..2 {
        glib::MainContext::default()
            .block_on(
                w.documents
                    .open(&w, (new_drawing(80, 80, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)),
            )
            .unwrap();
        new_photo::ready(&w);
        temporary.push(w.documents.selected());
    }
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
        Some("selector"),
        "four drawings do not fit as tabs in the narrowest window"
    );
    w.window.set_default_size(1550, 900);
    pump(300);
    assert_eq!(
        w.documents.root.visible_child_name().as_deref(),
        Some("tabs")
    );
    for (remaining, id) in [(3, temporary[1]), (2, temporary[0])] {
        w.documents.select(&w, id, true);
        until(
            || w.documents.len() == remaining && !w.documents.changing.get(),
            "temporary tab closes",
        );
        new_photo::ready(&w);
    }
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
    until(|| !w.window.is_visible() && windows.borrow().is_empty(), "last tab retires its window");
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
    apply_fixture_theme(&w);
    set_transparency(&w, "LAYER_MOTION_TRANSPARENCY");
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
    let retained = [tab(1), tab(2), tab(3)];
    let check_retained = || {
        for (index, row) in retained.iter().enumerate() {
            assert_eq!(tab(index as u64 + 1), *row, "drawing tab updates retain native controls");
            assert_eq!(row.has_css_class("selected"), index as u64 + 1 == w.documents.selected());
        }
    };
    let settings = gtk::Settings::default().unwrap();
    let animations = settings.is_gtk_enable_animations();
    settings.set_gtk_enable_animations(false);
    let strip = retained[0].parent().unwrap();
    input.perform(serde_json::json!([{"point": [800., 500.]}]));
    let idle = idle_hover_pixel(&w, &strip, &retained[1], [0.5, 0.75]);
    let start = point(1, 0.3);
    let end = [start[0] + (second.x() - first.x()) * 0.9, start[1]];
    input.perform(serde_json::json!([{"point": start}, {"down": true}, {"point": end}]));
    let frames = hover_frames(&w, &strip, end);
    input.perform(serde_json::json!([{"down": false}]));
    assert_hover_frames(&w, frames.finish(), idle, [0.5, 0.75]);
    w.documents.model.borrow_mut().undo();
    w.documents.refresh(&w);
    pump(150);
    input.perform(serde_json::json!([{"point": point(2, 0.5)}]));
    assert_ne!(idle_hover_pixel(&w, &strip, &retained[1], [0.5, 0.75]), idle, "drawing tabs retain ordinary hover feedback");
    for opening in [true, false] {
        let resting = point(2, 0.9);
        input.perform(serde_json::json!([{"point": resting}]));
        let frames = hover_frames(&w, &strip, resting);
        if opening {
            glib::MainContext::default().block_on(w.documents.open(&w,
                (new_drawing(128, 128, &w.localization()).unwrap(), None))).unwrap();
        } else {
            input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x77,"down":true},
                {"key":0x77,"down":false},{"key":0xffe3,"down":false}]));
        }
        new_photo::ready(&w);
        pump(150);
        assert_hover_frames(&w, frames.finish(), idle, [0.5, 0.75]);
    }
    switch(&w, 2);
    input.perform(serde_json::json!([{"point": [800., 500.]}]));
    let idle = idle_hover_pixel(&w, &strip, &retained[1], [0.5, 0.75]);
    let touch = point(2, 0.5);
    input.perform(serde_json::json!([contact("touch", "down", touch)]));
    let frames = sample_after_paint(&strip, glib::clone!(#[weak] strip, #[strong(rename_to = row)] retained[1], #[upgrade_or_default] move || {
        if row.state_flags().contains(gtk::StateFlags::ACTIVE) { Vec::new() }
        else { vec![(row.widget_name().to_string(), row.compute_bounds(&strip).unwrap(), false)] }
    }));
    input.perform(serde_json::json!([contact("touch", "up", touch)]));
    assert_hover_frames(&w, frames.finish(), idle, [0.5, 0.75]);
    switch(&w, 3);
    settings.set_gtk_enable_animations(animations);
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
            crate::capture(&w, input.dir.join("capy-document-tabs-slide.png").to_str().unwrap());
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
        check_retained();
        w.documents.model.borrow_mut().undo();
        w.documents.refresh(&w);
        pump(150);
        assert_eq!(w.documents.model.borrow().order(), &[1, 2, 3]);
        assert!(w.documents.model.borrow().can_redo());
        check_retained();
        w.documents.model.borrow_mut().redo();
        w.documents.refresh(&w);
        pump(150);
        check_retained();
        w.documents.model.borrow_mut().undo();
        w.documents.refresh(&w);
        pump(150);
        input.perform(serde_json::json!([
            contact(device, "down", point(3, 0.3)),
            contact(device, "move", point(1, 0.2)),
            contact(device, "up", point(1, 0.2))
        ]));
        assert_eq!(w.documents.model.borrow().order(), &[3, 1, 2]);
        assert_eq!(w.documents.selected(), 3);
        check_retained();
        w.documents.model.borrow_mut().undo();
        w.documents.refresh(&w);
        pump(150);
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
    crate::capture(&w, input.dir.join("capy-document-tabs-wide.png").to_str().unwrap());
    // Exercise the actual pointer states for visual comparison with AdwTabBar.
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for (state, target) in [
            ("idle", [800., 200.]),
            ("hover", point(1, 0.4)),
            ("selected", point(3, 0.4)),
        ] {
            input.perform(serde_json::json!([{"point":target}]));
            crate::capture(&w, input.dir.join(format!("capy-document-tabs-{name}-{state}.png")).to_str().unwrap());
        }
        let tab = find_named(w.window.upcast_ref(), "document-tab-3").unwrap();
        let close = tab.last_child().unwrap();
        let b = close.compute_bounds(&w.window).unwrap();
        input.perform(serde_json::json!([{"point":[b.x()+b.width()/2., b.y()+b.height()/2.]}]));
        crate::capture(&w, input.dir.join(format!("capy-document-tabs-{name}-close.png")).to_str().unwrap());
        input.perform(serde_json::json!([{"point":point(3, 0.4)}, {"down":true}]));
        crate::capture(&w, input.dir.join(format!("capy-document-tabs-{name}-pressed.png")).to_str().unwrap());
        input.perform(serde_json::json!([{"down":false}]));
        assert!(tab.first_child().unwrap().grab_focus());
        w.window.set_focus_visible(true);
        pump(100);
        crate::capture(&w, input.dir.join(format!("capy-document-tabs-{name}-focus.png")).to_str().unwrap());
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
    until(||!w.window.is_visible()&&windows.borrow().is_empty(),"quit flushed both drawings");
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
    until(||!w.window.is_visible()&&windows.borrow().is_empty(),"last explicitly closed drawing exits");
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
            let overlay_before=w.capture_canvas(crate::display_color::ViewColor::Srgb).unwrap().bytes;
            let mut input=RemoteInput::new().timeout_secs(30);input.ready();
            let control=super::pointwise::number(&w,"mask_opacity");super::histogram::scroll_to(control.upcast_ref());
            input.click(screen_point(&find_css(control.upcast_ref(),"number-value").unwrap(),&w.window,[0.5,0.5]));
            descendant::<gtk::Entry>(&control).unwrap().set_text("37");input.key(0xff0d);new_photo::ready(&w);
            assert_ne!(w.capture_canvas(crate::display_color::ViewColor::Srgb).unwrap().bytes,overlay_before,"native canvas updates saved-selection overlay opacity");
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
    assert_eq!(w.documents.len(),2);assert!(["head.0", "head.1"].into_iter().any(|name| w.recovery().published_path().join(name).exists()));
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
#[ignore = "isolated native-input.js with --native-recovery"]
fn native_document_tabs_blocked_writer_keeps_canvas_live() {
    let app = native_test_app("art.capycanvas.TabBlockedWriter");
    let w = Workspace::with_project(&app, Some((new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)));
    w.window.maximize();
    w.window.present();
    new_photo::ready(&w);
    apply_fixture_theme(&w);
    glib::MainContext::default().block_on(w.documents.open(&w,
        (new_drawing(256, 256, &w.localization()).unwrap(), None))).unwrap();
    new_photo::ready(&w);
    let incoming = w.documents.selected();
    switch(&w, 1);
    let outgoing = w.recovery();
    outgoing.capture(&w);
    glib::MainContext::default().block_on(outgoing.flush()).unwrap();
    let store = outgoing.writer_store();
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let lock = std::thread::spawn(move || {
        pollster::block_on(store.with_store(async |_| {
            locked_tx.send(()).unwrap();
            Ok(release_rx.recv_timeout(std::time::Duration::from_secs(30)).is_ok())
        })).unwrap()
    });
    locked_rx.recv().unwrap();
    new_photo::invoke(&w, CommandId::AddLayer);
    let expected = ui_session(&w).engine().document().clone();
    outgoing.capture(&w);
    let started = std::time::Instant::now();
    glib::MainContext::default().block_on(w.documents.activate(&w, incoming)).unwrap();
    new_photo::ready(&w);
    let mut input = RemoteInput::new().settle_ms(0).timeout_secs(10);
    input.ready();
    let a = super::canvas_bar_tests::canvas_point(&w, [80., 128.]);
    let b = super::canvas_bar_tests::canvas_point(&w, [176., 128.]);
    input.perform(serde_json::json!([{"point":a,"down":true},{"point":b},{"down":false}]));
    new_photo::ready(&w);
    let painted = !active_paint(ui_session(&w).engine().document()).raster.wait_data().unwrap().tiles.is_empty();
    let elapsed = started.elapsed();
    let released = release_tx.send(()).is_ok();
    let held = lock.join().unwrap();
    assert!(released && held, "incoming canvas and input completed while the real SessionStore writer was blocked: {elapsed:?}");
    assert!(painted, "incoming native contact paints before outgoing storage completes");
    input.finish();
    glib::MainContext::default().block_on(outgoing.flush()).unwrap();
    let recovered = glib::MainContext::default().block_on(outgoing.read_snapshot()).unwrap();
    let mut saved = recovered.document().clone();
    saved.artwork.outputs.get_mut(saved.artwork.default_output).unwrap().context.elapsed = expected.output().context.elapsed;
    assert_live_artwork_eq(&saved, &expected);
    switch(&w, 1);
    assert_eq!(ui_session(&w).engine().document(), &expected);
    w.window.destroy();
    pump(100);
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
                    });
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
        until(|| w.documents.parked_memory() == (0, true), "inactive storage completed");
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
                .decode_on_worker()
                .unwrap()
                .as_chunks::<4>().0.iter()
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
    assert_eq!(blob.decode_on_worker().unwrap(), vec![128; 256 * 256 * 4]);
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
    let previous = w.visible_dialog();
    let file = gtk::gio::File::for_path(source);
    let location = layer_ui::DocumentLocation {uri:file.uri().to_string(),name:"Original.capy".into()};
    glib::spawn_future_local(glib::clone!(#[strong] w, #[strong] completed, async move {
        *completed.borrow_mut() = Some(w.documents.open_imported(&w, imported, Some(location)).await);
    }));
    until(|| w.visible_dialog().is_some_and(|dialog| Some(&dialog) != previous.as_ref()
        && dialog.widget_name() == "preserved-package-preview" && {
            let frame = crate::page_dialog::parent(&dialog);
            frame.is_mapped() && descendants::<gtk::Picture>(&frame).iter().any(|picture| picture.is_mapped() && picture.paintable().is_some())
        }),"unsupported admission presents the package preview");
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
    let phone_viewport = std::env::var("LAYER_MOTION_VIEWPORT").ok().map(|viewport| {
        let (width, height) = viewport.split_once('x').unwrap();
        [width.parse::<i32>().unwrap(), height.parse::<i32>().unwrap()]
    }).filter(|size| size[0].min(size[1]) < 480);
    if phone_viewport.is_some() { w.window.maximize(); }
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
        assert!(image.as_chunks::<4>().0.iter().all(|pixel|*pixel==[255,255,255,255]));
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
    let imported = match layer_ui::read_import(std::io::Cursor::new(bytes.clone()),layer_ui::ImportIntent::Open,Default::default(),layer_ui::photo_name("Original.capy",&localization),Default::default(),layer_ui::photo_decode_limits(),&std::sync::atomic::AtomicBool::new(false)).unwrap() {
        layer_ui::ImportOutcome::Editable(imported) => imported,
        _ => panic!("Expected supported native package"),
    };
    w.documents.model.borrow_mut().budget.metadata = 0;
    if let Some(expected) = phone_viewport {
        until(|| [w.surface.width(), w.surface.height()] == expected, "the preserved package journey uses its phone viewport");
        let mut d = super::workspace_header::Driver::start(w.clone(), app);
        let diagnostics = super::page_dialog::bounds::size_warnings();
        for theme in [Theme::Light, Theme::Dark] {
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            let completed = begin_preserved_import(&w, imported.clone(), &source);
            let (dialog, frame) = super::page_dialog::dialog_frame(&d);
            let header = super::page_dialog::done_header(&d, &frame, true);
            super::page_dialog::capture(&mut d, "preserved-package-preview");
            let alert = dialog.downcast_ref::<adw::AlertDialog>().unwrap();
            let responses = Rc::new(RefCell::new(Vec::new()));
            let observed = responses.clone();
            let handler = alert.connect_response(None, move |_, response| observed.borrow_mut().push(response.to_owned()));
            diagnostics.lock().unwrap().clear();
            d.click(super::page_dialog::done_button(&d, &header).upcast_ref());
            until(|| completed.borrow().is_some(), "Preserved Package Done completes its close response");
            assert_eq!(completed.borrow_mut().take().unwrap(), Ok(()));
            assert_eq!(*responses.borrow(), [alert.close_response().to_string()], "Preserved Package Done emits its close response exactly once");
            assert!(!diagnostics.lock().unwrap().iter().any(|message| message.starts_with("Adwaita critical:")), "Preserved Package Done has no Adwaita critical");
            alert.disconnect(handler);
            assert_eq!(ui_session(&w).engine().document(), &original);
            assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
            assert_eq!(w.documents.selected(), selected);
        }
        d.finish();
        std::fs::remove_dir_all(directory).unwrap();
        return;
    }
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
        assert!(shown.as_chunks::<4>().0.iter().all(|pixel|*pixel==[255,255,255,255]));
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
        assert!(decoded.pixels().as_chunks::<4>().0.iter().all(|pixel|*pixel==[255,255,255,255]));
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

#[test]
#[ignore = "isolated native-input.js with compositor capture"]
fn native_document_tabs_canvas_transition() {
    use layer_core::color::{ColorProfile, RgbSpace, SampleDepth, source::*};
    use layer_core::authored::OccurrenceContent;
    use std::sync::{Arc, atomic::Ordering};
    struct Resume(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Resume {fn drop(&mut self) {self.0.store(false,Ordering::Release);}}
    let (app,windows)=crate::application("art.capycanvas.CanvasTransition");
    let app=NativeTestApp(app); app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    let drawing=|rgb:[u8;3]| {
        let mut drawing=new_drawing(256,256,&localization).unwrap();
        let mut source=SourceBuilder::new([256,256],SourceInterpretation {
            channels:SourceChannels::Rgba,depth:SampleDepth::U8,profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false,
        },1024*1024).unwrap();
        let row:Vec<u8>=(0..256).flat_map(|_|[rgb[0],rgb[1],rgb[2],255]).collect();
        for _ in 0..256 {source.push_row(&row).unwrap();}
        let active=drawing.working.occurrence.unwrap();
        let OccurrenceContent::Paint(paint)=drawing.scene().occurrence(active).unwrap().content else {unreachable!()};
        drawing.artwork.paint.get_mut(paint).unwrap().base=Some(layer_core::PaintBase::new(Arc::new(source.finish().unwrap()).into()));
        drawing
    };
    crate::open_workspace(&app,&windows,Some((drawing([216,48,24]),None)));
    until(||!windows.borrow().is_empty(),"transition window created");
    let w=windows.borrow()[0].clone();
    w.window.maximize(); w.window.present(); new_photo::ready(&w);
    let mut workspace=state(&w).workspace;
    workspace.layout.set_panel_visible(layer_ui::Panel::Navigator,true).unwrap();
    let group=workspace.layout.panel_group(layer_ui::Panel::Navigator).unwrap();
    if let Some(column)=workspace.layout.column_for_group(group) {
        workspace.layout.set_column_collapsed(column,false,[w.surface.width() as f32,w.surface.height() as f32]).unwrap();
    }
    workspace.layout.select_tab(group,layer_ui::Panel::Navigator).unwrap();
    w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(workspace)});
    w.dispatch(UiAction::Invoke {command:CommandId::FitCanvas}); new_photo::ready(&w);
    let mut input=RemoteInput::new().settle_ms(0).timeout_secs(30); input.ready();
    glib::MainContext::default().block_on(w.documents.open(&w,(drawing([24,80,216]),None))).unwrap();
    new_photo::ready(&w);
    w.dispatch(UiAction::Invoke {command:CommandId::FitCanvas}); new_photo::ready(&w);
    let incoming=std::cell::Cell::new(w.documents.selected());
    let file=std::path::PathBuf::from(std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").unwrap()).join("transition-blue.capy");
    crate::files::choose_next_save(file.clone()); new_photo::invoke(&w,CommandId::SaveDocument);
    until(||file.exists() && !w.servicing.get() && !state(&w).document_file.modified,"transition source saved");
    let [x,y]=super::canvas_bar_tests::canvas_point(&w,[160.,128.]);
    let center=w.window.compute_point(&w.surface,&gtk::graphene::Point::new(x,y)).unwrap();
    let scale=w.area.scale_factor() as f32;
    let overview=w.navigator_overviews.placements(ui_session(&w).state(),scale).into_iter().find(|p|p.opacity>0.99).unwrap();
    let [x,y,width,height]=overview.bounds;
    let [cx,cy,cw,ch]=overview.clip.unwrap_or(overview.bounds);
    let [left,top,right,bottom]=[x.max(cx),y.max(cy),(x+width).min(cx+cw),(y+height).min(cy+ch)];
    assert!((right-left)/scale>40. && (bottom-top)/scale>40.,"Navigator image has room for a 32-pixel block");
    let navigator=[(left+right)/(2.*scale),(top+bottom)/(2.*scale)];
    let points:[[f32;2];2048]=std::array::from_fn(|i| {
        let origin=if i<1024 {[center.x(),center.y()]}else {navigator};
        let i=i%1024;
        [origin[0]+(i%32) as f32-16.,origin[1]+(i/32) as f32-16.]
    });
    switch(&w,1);
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)}); new_photo::ready(&w);
        for operation in ["select","close","open"] {
        until(||ui_session(&w).frame_demand() == layer_render::FrameDemand::Wait,"old artwork presentation settled");
        let name=format!("canvas-transition-{theme:?}-{operation}").to_lowercase();
        let mut prepared=if operation=="open" {
            let path=file.clone(); let names=layer_ui::photo_name("transition-blue.capy",&localization);
            Some(glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
                match layer_ui::read_import(std::io::BufReader::new(std::fs::File::open(path).unwrap()),layer_ui::ImportIntent::Open,Default::default(),names,Default::default(),layer_ui::photo_decode_limits(),&std::sync::atomic::AtomicBool::new(false)).unwrap() {
                    layer_ui::ImportOutcome::Editable(imported)=>imported,_=>panic!("transition package must be editable"),
                }
            })).unwrap())
        }else {None};
        let pause=crate::render_thread::pause_next_startup(); let _resume=Resume(pause.clone());
        let paints=sample_after_paint(&w.surface,||());
        let frames=super::ui_motion::record_pixels(&w,&mut input,&name,points,|| {
            let changed=glib::monotonic_time();
            match operation {
                "select"=>glib::MainContext::default().block_on(w.documents.activate(&w,incoming.get())).unwrap(),
                "close"=> {w.documents.select(&w,incoming.get(),true); until(||w.documents.len()==1 && !w.documents.changing.get(),"active tab closed into its parked neighbour");},
                "open"=> {w.documents.enqueue_imported(&w,prepared.take().unwrap(),Some(layer_ui::DocumentLocation {uri:gtk::gio::File::for_path(&file).uri().to_string(),name:"transition-blue.capy".into()})); until(||w.documents.changing.get(),"prepared file admission started");},
                _=>unreachable!(),
            }
            if operation!="open" {
                until(||paints.samples.borrow().iter().any(|paint|paint.time>=changed),"incoming tab selected while old canvas is held");
                assert!(!ui_session(&w).engine().backend().startup.complete);
            }
            pause.store(false,Ordering::Release);
            if operation=="open" {until(||w.documents.len()==2 && !w.documents.changing.get() && !w.documents.has_pending_open(),"prepared file opened in this window");incoming.set(w.documents.selected());}
            new_photo::ready(&w);
            until(||ui_session(&w).frame_demand() == layer_render::FrameDemand::Wait,"incoming artwork presentation settled");
        });
        paints.finish();
        for (region,offset) in [("canvas",0),("navigator",1024)] {
        let name=format!("{name}-{region}");
        let old=frames.first().unwrap()[offset]; let new=frames.last().unwrap()[offset];
        assert!(old[0].abs_diff(new[0])>50 && old[2].abs_diff(new[2])>50,"{name}: distinct old and new artwork endpoints {old:?} -> {new:?}");
        let mut mixed=0;
        let mut previous_phase=0f32;
        for (index,frame) in frames.iter().enumerate() {
            let frame=&frame[offset..offset+1024];
            let pixel=frame[0];
            assert!(frame.iter().all(|p|p.iter().zip(pixel).all(|(a,b)|a.abs_diff(b)<=4)),"{name} frame {index}: no checkerboard during the transition");
            assert!(pixel.iter().enumerate().all(|(c,value)|*value>=old[c].min(new[c]).saturating_sub(8) && *value<=old[c].max(new[c]).saturating_add(8)),"{name} frame {index}: no surround or paper flash {pixel:?}");
            let phase=[false,true].into_iter().find_map(|linear| {
                let decode=|v:u8|if linear {(v as f32/255.).powf(2.2)}else {v as f32/255.};
                let t:[f32;2]=[0,2].map(|c|(decode(pixel[c])-decode(old[c]))/(decode(new[c])-decode(old[c])));
                ((t[0]-t[1]).abs()<=0.16).then_some((t[0]+t[1])*0.5)
            });
            let phase=phase.unwrap_or_else(||panic!("{name} frame {index}: expected old, blended, or new artwork, got {pixel:?}"));
            assert!(phase+0.08>=previous_phase,"{name} frame {index}: fade moved backwards from {previous_phase} to {phase}");
            previous_phase=previous_phase.max(phase);
            if phase>0.05 && phase<0.95 {mixed+=1;}
        }
        assert!(mixed>0,"{name}: presented at least one intermediate fade frame");
        }
        }
        switch(&w,1);
    }
    input.finish(); w.window.destroy(); pump(100);
}

#[test]
#[ignore = "private hardware compositor, M3 own-store cold motion"]
fn native_document_tabs_cold_store_motion() {
    cold_store_motion(None);
}

#[test]
#[ignore = "private hardware compositor, own-store cold stroke at 38 percent"]
fn native_document_tabs_cold_store_stroke_38_percent() {
    cold_store_motion(Some(0.38));
}

fn cold_store_motion(zoom: Option<f32>) {
    use layer_core::authored::{Occurrence, OccurrenceContent, PaintSource, PortableId};
    use layer_core::color::{RgbSpace, SampleDepth};
    use std::{os::fd::AsRawFd, sync::Arc};
    use serde_json::json;
    let output=std::path::PathBuf::from(std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").unwrap());
    let (project,held,decoded_bytes,encoded_bytes,first_tile)=glib::MainContext::default().block_on(gtk::gio::spawn_blocking(|| {
        let localization=layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        let mut project=new_drawing(4216,3016,&localization).unwrap();
        composition_mut(&mut project).color.space=RgbSpace::Srgb;
        composition_mut(&mut project).color.depth=SampleDepth::U16;
        let descriptor=project.composition().color.paint_descriptor();
        let first=project.working.occurrence.unwrap();
        let mut decoded_bytes=0usize;let mut encoded_bytes=0usize;let mut first_tile=None;
        for layer in 0..6u32 {
            let occurrence=if layer==0 {first}else {
                let paint=project.artwork.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain:[4216,3016],base:None,raster:Default::default(),operations:Default::default()}).unwrap();
                let mut occurrence=Occurrence::new(OccurrenceContent::Paint(paint),format!("Cold raster {layer}"));
                occurrence.opacity=0.12;
                let occurrence=project.artwork.occurrences.insert(PortableId::random(),occurrence).unwrap();
                let stack=project.composition().result;
                project.artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
                occurrence
            };
            let mut tiles=RasterTiles::default();
            for y in 0..12u32 {for x in 0..17u32 {
                let mut bytes=Vec::with_capacity(256*256*8);
                let mut random=0x1357abcdu32^(layer*7919)^((x+y*17)*101);
                for py in 0..256u32 {for px in 0..256u32 {
                    for _ in 0..3 {
                        random^=random<<13;random^=random>>17;random^=random<<5;
                        let value=(10000+(px+py*3+x*19+y*37+layer*271)%40000+(random&8191))as u16;
                        bytes.extend_from_slice(&value.to_le_bytes());
                    }
                    bytes.extend_from_slice(&65535u16.to_le_bytes());
                }}
                decoded_bytes+=bytes.len();
                let blob=Arc::new(TileBlob::encode(descriptor,&bytes).unwrap());encoded_bytes+=blob.compressed_len();
                if layer==0 && x==1 && y==0 {first_tile=Some(blob.clone());}
                tiles.insert(TileKey {plane:RasterPlane::Color,coordinate:[x,y]},RasterTile::backed_shared(blob));
            }}
            let OccurrenceContent::Paint(paint)=project.artwork.occurrences.get(occurrence).unwrap().content else {unreachable!()};
            project.artwork.paint.get_mut(paint).unwrap().raster=RasterRevision::backed(RasterData {tiles,..Default::default()});
        }
        project.working.occurrence=Some(first);
        let working=project.working.clone();project=layer_core::Document::from_artwork(project.artwork.clone()).unwrap();project.working=working;
        let mut held=new_drawing(256,256,&localization).unwrap();
        let held_descriptor=held.composition().color.paint_descriptor();
        active_paint_mut(&mut held).raster=RasterRevision::backed(RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0,0]},RasterTile::backed(TileBlob::encode(held_descriptor,&[24,160,32,255].repeat(256*256)).unwrap()))].into(),..Default::default()});
        let working=held.working.clone();held=layer_core::Document::from_artwork(held.artwork.clone()).unwrap();held.working=working;
        (project,held,decoded_bytes,encoded_bytes,first_tile.unwrap())
    })).unwrap();
    assert!(decoded_bytes>512*1024*1024,"first-created raster exceeds the shared sample cache with later distinct rasters");
    assert!(encoded_bytes>256*1024*1024 && encoded_bytes<512*1024*1024,"encoded fixture exceeds the compressed hot set without exceeding parent asset admission");
    let app=native_test_app("art.capycanvas.ColdStoreMotion");
    let w=Workspace::with_project(&app,Some((project,None)));
    w.window.maximize();w.window.present();new_photo::ready(&w);
    w.dispatch(UiAction::Invoke {command:CommandId::FitCanvas});
    w.dispatch(UiAction::SelectBrush {id:layer_core::DefaultBrushPreset::GPen as u32});
    w.dispatch(UiAction::SetBrushSize {value:384.});
    w.dispatch(UiAction::SetColor {rgba:[1.,0.,1.,1.]});
    new_photo::ready(&w);
    until(||ui_session(&w).frame_demand() == layer_render::FrameDemand::Wait,"warm raster presentation settled");
    let baseline=glib::MainContext::default().block_on(read_canvas_pixels(&w,8300)).unwrap().bytes;
    if let Some(zoom)=zoom {w.dispatch(UiAction::SetZoom {zoom});new_photo::ready(&w);}
    let [x,y]=super::canvas_bar_tests::canvas_point(&w,[384.,128.]);
    let point=w.window.compute_point(&w.surface,&gtk::graphene::Point::new(x,y)).unwrap();
    let first=w.documents.selected();let recovery=w.recovery();
    let mut input=RemoteInput::new().settle_ms(0).timeout_secs(60);input.ready();
    w.documents.ram_budget.set(0);
    glib::MainContext::default().block_on(w.documents.open(&w,(held,None))).unwrap();new_photo::ready(&w);
    w.dispatch(UiAction::Invoke {command:CommandId::FitCanvas});new_photo::ready(&w);
    let [hx,hy]=super::canvas_bar_tests::canvas_point(&w,[128.,128.]);
    let held_point=w.window.compute_point(&w.surface,&gtk::graphene::Point::new(hx,hy)).unwrap();
    let other=w.documents.selected();
    let phases:&[(&str,u32)]=if zoom.is_some() {&[("stroke",8320)]}else {&[("pan",8310),("stroke",8320)]};
    for &(phase,nonce) in phases {
        until(||w.documents.parked_memory()==(0,true),"cold drawing pressure writes and renderer retirement completed");
        glib::MainContext::default().block_on(recovery.flush()).unwrap();
        assert_eq!(first_tile.resident_bytes(),0,"selected old raster range is store-backed, not a resident compressed copy");
        let store=recovery.published_path();
        let advised=glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
            fn collect(path:&std::path::Path,files:&mut Vec<std::path::PathBuf>) {
                if path.is_file() {files.push(path.to_owned());}
                else if path.is_dir() {for entry in std::fs::read_dir(path).unwrap() {collect(&entry.unwrap().path(),files);}}
            }
            let mut files=Vec::new();collect(&store,&mut files);
            let prefix=format!("tiles-{}-",std::process::id());
            for entry in std::fs::read_dir("/proc/self/fd").unwrap().filter_map(Result::ok) {
                if std::fs::read_link(entry.path()).ok().and_then(|p|p.file_name().map(|n|n.to_string_lossy().into_owned())).is_some_and(|name|name.starts_with(&prefix)) {files.push(entry.path());}
            }
            let mut advised=Vec::new();
            for path in files {
                let file=std::fs::File::open(&path).unwrap();let bytes=file.metadata().unwrap().len();
                if bytes==0 {continue;}
                assert_eq!(unsafe {libc::posix_fadvise(file.as_raw_fd(),0,0,libc::POSIX_FADV_DONTNEED)},0);
                advised.push((path.to_string_lossy().into_owned(),bytes));
            }
            advised
        })).unwrap();
        assert!(!advised.is_empty(),"cold fixture advises only its durable store or own spill descriptors");
        let stats=std::cell::RefCell::new(None);
        let mut started=0u64;let mut contact=0u64;let mut contact_end=0u64;let mut before=0u64;
        let frames=super::ui_motion::record_pixels(&w,&mut input,&format!("cold-store-{phase}"),[[point.x(),point.y()],[held_point.x(),held_point.y()]],|| {
            started=glib::monotonic_time()as u64*1000;
            glib::MainContext::default().block_on(w.documents.activate(&w,first)).unwrap();
            let incoming=ui_session(&w).engine().backend().stats.clone();*incoming.lock().unwrap()=Default::default();stats.replace(Some(incoming));
            before=ui_session(&w).engine().metrics().committed_strokes;
            contact=glib::monotonic_time()as u64*1000;
            if phase=="stroke" {
                until(||{let gpu=w.gpu.borrow();let session=&gpu.as_ref().unwrap().session;session.engine().backend().paint_ready(session.engine().document(),session.engine().brush(),session.engine().transform_preview().is_some())},"cold contact brush shaders ready without exact drawing drain");
                let camera=state(&w).camera;contact=glib::monotonic_time()as u64*1000;
                for (index,x) in [256.,384.,512.,640.].into_iter().enumerate() {
                    let event=PenEvent {device_id:903,timestamp_ns:contact+index as u64*20_000_000,pressure:0.7,..pen_event(&camera,[x,128.],if index==0 {PenPhase::Down}else if index==3 {PenPhase::Up}else {PenPhase::Move},contact+index as u64)};
                    w.input.send(&w,event);pump(20);
                }
            }else {
                for index in 0..48 {w.dispatch(UiAction::SetZoom {zoom:0.65+index as f32*0.002});pump(8);}
            }
            contact_end=glib::monotonic_time()as u64*1000;
            new_photo::ready(&w);
            until(||ui_session(&w).frame_demand() == layer_render::FrameDemand::Wait,"cold motion presentation settled");
        });
        let stats=stats.into_inner().unwrap();let rows=stats.lock().unwrap();
        let publication=rows.raster_commits.first().and_then(|[id,_]|rows.presented.iter().find(|p|p[0]==*id&&p[3]==1).map(|p|p[1]));
        let committed=ui_session(&w).engine().metrics().committed_strokes;
        std::fs::write(output.join(format!("cold-store-{phase}.json")),serde_json::to_vec_pretty(&json!({"phase":phase,"decoded_document_bytes":decoded_bytes,"encoded_document_bytes":encoded_bytes,"decoded_cache_ceiling":512*1024*1024,"target":"first-created raster, first row","advised_owned_files":advised,"restore_start_ns":started,"contact_start_ns":contact,"contact_end_ns":contact_end,"old_fallback_pixel":frames[0][1],"first_changed_fallback_sample":frames.iter().position(|p|p[1].iter().zip(frames[0][1]).any(|(a,b)|a.abs_diff(b)>8)),"first_raster_present_ns":publication,"first_pink_sample":frames.iter().position(|p|p[0][0]>p[0][1].saturating_add(50)&&p[0][2]>p[0][1].saturating_add(50)),"committed_strokes_before":before,"committed_strokes_after":committed,"presented":rows.presented,"worker_thread_cpu":rows.thread_cpu,"camera_work":rows.camera_work,"raster_commits":rows.raster_commits,"read_wait":"unavailable in existing telemetry","memory_budget_snapshot":format!("{:?}",layer_core::MemoryBudget::process().snapshot())})).unwrap()).unwrap();drop(rows);
        w.dispatch(UiAction::Invoke {command:CommandId::FitCanvas});new_photo::ready(&w);
        let pixels=glib::MainContext::default().block_on(read_canvas_pixels(&w,nonce)).unwrap().bytes;
        std::fs::write(output.join(format!("cold-store-{phase}-pixels.bin")),&pixels).unwrap();
        if phase=="pan" {assert_eq!(pixels,baseline,"cold pan restores the same authored pixels");}
        else {
            assert_eq!(committed,before+1,"one ordered cold stroke committed");assert_ne!(pixels,baseline,"cold stroke is visible");
            new_photo::invoke(&w,CommandId::Undo);new_photo::ready(&w);
            assert_eq!(glib::MainContext::default().block_on(read_canvas_pixels(&w,nonce+1)).unwrap().bytes,baseline,"cold stroke undo is lossless");
            new_photo::invoke(&w,CommandId::Redo);new_photo::ready(&w);
            assert_eq!(glib::MainContext::default().block_on(read_canvas_pixels(&w,nonce+2)).unwrap().bytes,pixels,"cold stroke redo preserves final pixels");
        }
        switch(&w,other);
    }
    input.finish();w.window.destroy();pump(100);
}
