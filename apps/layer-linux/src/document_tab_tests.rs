//! Real retained GTK window/session/worker lifecycle on the isolated compositor.
use super::*;
use layer_core::{Project, raster::*};

fn until(mut predicate: impl FnMut() -> bool, message: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !predicate() {
        assert!(Instant::now() < deadline, "{message}");
        pump(10);
    }
}
fn switch(w: &Rc<Workspace>, id: u64) {
    glib::MainContext::default()
        .block_on(w.documents.activate(w, id))
        .unwrap();
    new_photo::ready(w);
}

#[test]
#[ignore = "isolated Wayland and GPU"]
fn native_document_tabs_history_storage_and_close() {
    let (app, windows) = crate::application("art.capycanvas.DocumentTabs");
    let app = NativeTestApp(app);
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let mut project = new_drawing(256, 256).unwrap();
    let blob = TileBlob::encode(
        project.document.color.paint_descriptor(),
        &vec![128; 256 * 256 * 4],
    )
    .unwrap();
    project.document.layers[0].raster = RasterRevision::backed(RasterData {
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
    crate::open_workspace(&app, &windows, Some((project.clone(), None)), None);
    let w = windows.borrow()[0].clone();
    new_photo::ready(&w);
    let plain_title = find_named(w.window.upcast_ref(), "single-document-title")
        .unwrap()
        .downcast::<gtk::Label>()
        .unwrap();
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
    let original = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .clone();
    let first = w.documents.selected();
    // The production New dialog callback must append in this window, including
    // while the previous drawing has unsaved edits.
    new_photo::invoke(&w, CommandId::NewDocument);
    for (name, value) in [("new-document-width", 128.), ("new-document-height", 96.)] {
        find_named(w.window.upcast_ref(), name)
            .unwrap()
            .downcast::<adw::SpinRow>()
            .unwrap()
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
    let recovery_dir = std::env::var_os("CAPY_RECOVERY_DIR").unwrap();
    let mut widths: Vec<_> = std::fs::read_dir(recovery_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|s| s == "capy"))
        .map(|e| {
            Project::read(std::fs::File::open(e.path()).unwrap(), Default::default())
                .unwrap()
                .document
                .width
        })
        .collect();
    widths.sort();
    assert_eq!(
        widths,
        [128, 256],
        "each recovery owner captured its own drawing"
    );
    new_photo::invoke(&w, CommandId::Undo);
    switch(&w, first);
    assert_eq!(
        w.gpu.borrow().as_ref().unwrap().session.engine().document(),
        &original
    );
    assert_eq!(state(&w).camera, original_view);
    assert_eq!(state(&w).brush.diameter, 42.);
    assert!(state(&w).document_file.modified);
    new_photo::invoke(&w, CommandId::Undo);
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers
            .len(),
        project.document.layers.len()
    );
    new_photo::invoke(&w, CommandId::Redo);
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers
            .len(),
        original.layers.len()
    );
    // A native save traverses disk-backed tiles, including exact historical data.
    let captured = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .capture_project_recovery()
        .unwrap();
    let mut bytes = Vec::new();
    captured.write(&mut bytes).unwrap();
    let reopened = Project::read(bytes.as_slice(), Default::default()).unwrap();
    assert_eq!(
        reopened
            .document
            .layers
            .iter()
            .find(|l| l.id == project.document.layers[0].id)
            .unwrap()
            .raster
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
                .open(&w, (new_drawing(80, 80).unwrap(), None, None)),
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
    // Cancel window close leaves all remaining drawings and the window alive.
    w.window.close();
    until(
        || w.window.visible_dialog().is_some(),
        "window close decision",
    );
    new_photo::response(&w, "cancel");
    pump(100);
    assert!(w.window.is_visible());
    assert_eq!(w.documents.len(), 2);
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
    w.window.close();
    until(|| w.window.visible_dialog().is_some(), "final dirty close");
    new_photo::response(&w, "discard");
    until(|| !w.window.is_visible(), "last tab closes window");
    assert!(windows.borrow().is_empty());
}

#[test]
#[ignore = "native-input.js --native-test=native_document_tab_input --native-storage"]
fn native_document_tab_input() {
    let app = native_test_app("art.capycanvas.DocumentTabInput");
    let w = Workspace::with_project(&app, Some((new_drawing(256, 256).unwrap(), None)));
    w.window.maximize();
    w.window.present();
    new_photo::ready(&w);
    pump(250);
    let directory = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    std::fs::write(directory.join("ready"), "ready").unwrap();
    pump(500);
    let mut step = 0;
    let mut perform = |events: serde_json::Value| {
        let path = directory.join(format!("step-{step}.json"));
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, serde_json::to_vec(&events).unwrap()).unwrap();
        std::fs::rename(temp, path).unwrap();
        until(
            || directory.join(format!("done-{step}")).exists(),
            "native input acknowledgement",
        );
        step += 1;
        pump(180);
    };
    // A single drawing restores the native draggable/double-clickable caption.
    let title = find_named(w.window.upcast_ref(), "single-document-title").unwrap();
    let b = title.compute_bounds(&w.window).unwrap();
    perform(
        serde_json::json!([{"point":[b.x()+b.width()/2., b.y()+b.height()/2.]},
        {"down":true},{"down":false},{"down":true},{"down":false}]),
    );
    assert!(!w.window.is_maximized());
    w.window.maximize();
    pump(400);
    for _ in 0..2 {
        glib::MainContext::default()
            .block_on(
                w.documents
                    .open(&w, (new_drawing(128, 128).unwrap(), None, None)),
            )
            .unwrap();
        new_photo::ready(&w);
    }
    let point = |id: u64, fraction: f32| {
        let tab = find_named(w.window.upcast_ref(), &format!("document-tab-{id}")).unwrap();
        let b = tab.compute_bounds(&w.window).unwrap();
        [b.x() + b.width() * fraction, b.y() + b.height() / 2.]
    };
    for touch in [false, true] {
        assert_eq!(
            w.documents.root.visible_child_name().as_deref(),
            Some("tabs")
        );
        let from = point(1, 0.3);
        let to = point(3, 0.8);

        perform(if touch {
            serde_json::json!([{"touch":"down", "point":from}, {"touch":"move", "point":[from[0]+60.,from[1]]}])
        } else {
            serde_json::json!([{"point":from}, {"down":true}, {"point":[from[0]+60.,from[1]]}])
        });
        assert!(
            w.documents.drag_active(),
            "native tab drag started without hold, touch={touch}"
        );
        perform(if touch {
            serde_json::json!([{"touch":"move", "point":to}, {"touch":"move", "point":[to[0]-2.,to[1]]}])
        } else {
            serde_json::json!([{"point":to}, {"point":[to[0]-2.,to[1]]}])
        });
        perform(if touch {
            serde_json::json!([{"touch":"up"}])
        } else {
            serde_json::json!([{"down":false}])
        });
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
        perform(if touch {
            serde_json::json!([{"touch":"down", "point":from}, {"touch":"move", "point":to}, {"key":0xff1b,"down":true}, {"key":0xff1b,"down":false}, {"touch":"up"}])
        } else {
            serde_json::json!([{"point":from}, {"down":true}, {"point":to}, {"key":0xff1b,"down":true}, {"key":0xff1b,"down":false}, {"down":false}])
        });
        assert_eq!(
            w.documents.model.borrow().order(),
            &[1, 2, 3],
            "cancelled drag"
        );
    }
    // Native clicks still select, and keyboard cycling follows visual order.
    let first = point(1, 0.3);
    perform(serde_json::json!([{"point":first},{"down":true},{"down":false}]));
    until(
        || w.documents.selected() == 1 && !w.documents.changing.get(),
        "mouse tab selection",
    );
    new_photo::ready(&w);
    perform(
        serde_json::json!([{"key":0xffe3,"down":true},{"key":0xff09,"down":true},{"key":0xff09,"down":false},{"key":0xffe3,"down":false}]),
    );
    until(
        || w.documents.selected() == 2 && !w.documents.changing.get(),
        "Ctrl+Tab selection",
    );
    new_photo::ready(&w);
    let last = point(3, 0.3);
    perform(serde_json::json!([{"touch":"down","point":last},{"touch":"up"}]));
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
            perform(serde_json::json!([{"point":target}]));
            crate::capture(&w, &format!("/tmp/capy-document-tabs-{name}-{state}.png"));
        }
        let tab = find_named(w.window.upcast_ref(), "document-tab-3").unwrap();
        let close = tab.last_child().unwrap();
        let b = close.compute_bounds(&w.window).unwrap();
        perform(serde_json::json!([{"point":[b.x()+b.width()/2., b.y()+b.height()/2.]}]));
        crate::capture(&w, &format!("/tmp/capy-document-tabs-{name}-close.png"));
        perform(serde_json::json!([{"point":point(3, 0.4)}, {"down":true}]));
        crate::capture(&w, &format!("/tmp/capy-document-tabs-{name}-pressed.png"));
        perform(serde_json::json!([{"down":false}]));
        assert!(tab.first_child().unwrap().grab_focus());
        perform(
            serde_json::json!([{"key":0xff09,"down":true},{"key":0xff09,"down":false},
            {"key":0xffe1,"down":true},{"key":0xff09,"down":true},{"key":0xff09,"down":false},{"key":0xffe1,"down":false}]),
        );
        crate::capture(&w, &format!("/tmp/capy-document-tabs-{name}-focus.png"));
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
    perform(
        serde_json::json!([{"key":0xffe3,"down":true},{"key":0xffe1,"down":true},{"key":0x61,"down":true},{"key":0x61,"down":false},{"key":0xffe1,"down":false},{"key":0xffe3,"down":false}]),
    );
    assert!(
        find_named(w.window.upcast_ref(), "drawing-selector-popup").is_some_and(|p| p.is_visible())
    );
    perform(serde_json::json!([{"key":0xff1b,"down":true},{"key":0xff1b,"down":false}]));
    std::fs::write(directory.join("finished"), "done").unwrap();
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Wayland and GPU"]
fn native_document_tabs_failed_renderer_remains_navigable() {
    let app = native_test_app("art.capycanvas.TabFailure");
    let w = Workspace::with_project(&app, Some((new_drawing(96, 96).unwrap(), None)));
    w.window.present();
    new_photo::ready(&w);
    new_photo::invoke(&w, CommandId::AddLayer);
    let first = w.documents.selected();
    glib::MainContext::default()
        .block_on(
            w.documents
                .open(&w, (new_drawing(128, 128).unwrap(), None, None)),
        )
        .unwrap();
    new_photo::ready(&w);
    let second = w.documents.selected();
    let epoch = state(&w).document_file.epoch;
    w.gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .backend()
        .fail_next_frame();
    w.wake();
    until(
        || {
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .rendering_suspended()
        },
        "injected renderer failure",
    );
    switch(&w, first);
    assert!(state(&w).document_file.epoch > epoch);
    assert!(state(&w).document_file.modified);
    switch(&w, second);
    assert!(
        !w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
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
#[ignore = "isolated Wayland and recovery directory"]
fn native_document_tabs_multiple_recovery_offers() {
    let (app, windows) = crate::application("art.capycanvas.TabRecovery");
    let app = NativeTestApp(app);
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    crate::open_workspace(
        &app,
        &windows,
        Some((new_drawing(64, 64).unwrap(), None)),
        None,
    );
    let w = windows.borrow()[0].clone();
    new_photo::ready(&w);
    let dir = std::path::PathBuf::from(std::env::var_os("CAPY_RECOVERY_DIR").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let paths: Vec<_> = [96, 144]
        .into_iter()
        .enumerate()
        .map(|(i, width)| {
            let path = dir.join(format!("999999999-tab-{i}.capy"));
            new_drawing(width, width)
                .unwrap()
                .write(std::fs::File::create(&path).unwrap())
                .unwrap();
            path
        })
        .collect();
    crate::recovery::offer_stale(&w);
    for (i, width) in [96, 144].into_iter().enumerate() {
        until(|| w.window.visible_dialog().is_some(), "recovery offered");
        new_photo::response(&w, "recover");
        until(
            || w.documents.len() == i + 2 && !w.documents.changing.get(),
            "recovery opens a tab before next offer",
        );
        assert_eq!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .document()
                .width,
            width
        );
        assert!(state(&w).document_file.modified);
        assert!(
            paths[i].exists(),
            "keep original until explicit save/discard"
        );
    }
    assert_eq!(app.windows().len(), 1);
    assert_eq!(windows.borrow().len(), 1);
    for _ in 0..2 {
        w.documents.select(&w, w.documents.selected(), true);
        until(
            || w.window.visible_dialog().is_some(),
            "recovered drawing close decision",
        );
        let count = w.documents.len();
        new_photo::response(&w, "discard");
        until(
            || w.documents.len() == count - 1 && !w.documents.changing.get(),
            "recovered tab discarded",
        );
        new_photo::ready(&w);
    }
    until(
        || paths.iter().all(|p| !p.exists()),
        "discard retires recovery copies",
    );
    assert_eq!(w.documents.selected(), 1);
    assert!(!state(&w).document_file.modified);
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Wayland and GPU"]
fn native_document_tabs_immediate_stroke_and_undo() {
    let app = native_test_app("art.capycanvas.TabPendingStroke");
    let w = Workspace::with_project(&app, Some((new_drawing(256, 256).unwrap(), None)));
    w.window.present();
    new_photo::ready(&w);
    w.documents.ram_budget.set(0);
    glib::MainContext::default()
        .block_on(
            w.documents
                .open(&w, (new_drawing(96, 96).unwrap(), None, None)),
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
            let m = camera.document_to_surface();
            let now = glib::monotonic_time() as u64 * 1000;
            for (i, (phase, x)) in [(PenPhase::Down, 50.), (PenPhase::Up, 180.)]
                .into_iter()
                .enumerate()
            {
                session
                    .pen(PenEvent {
                        device_id: 71,
                        sequence: now + i as u64,
                        timestamp_ns: now + i as u64,
                        view_revision: camera.revision,
                        surface_position: Point {
                            x: m[0] * x + m[2] * 120. + m[4],
                            y: m[1] * x + m[3] * 120. + m[5],
                        },
                        pressure: 0.7,
                        tilt_radians: [0.; 2],
                        twist_radians: 0.,
                        distance: 0.,
                        phase,
                        tool: ToolKind::Pen,
                        flags: SampleFlags::PRIMARY,
                    })
                    .unwrap();
            }
            if undo {
                // Submit, then immediately move its still-pending capture to redo.
                session.frame(now + 2, now + 2).unwrap();
                let root = session.engine().document().layers[0].raster.clone();
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
        let document = w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .clone();
        let raster = &document.layers[0].raster;
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
#[ignore = "isolated Wayland; CAPY_TAB_CACHE_DIR=/dev/null/capy-tabs"]
fn native_document_tabs_disk_failure_keeps_data() {
    assert_eq!(
        std::env::var("CAPY_TAB_CACHE_DIR").unwrap(),
        "/dev/null/capy-tabs"
    );
    let app = native_test_app("art.capycanvas.TabDiskFailure");
    let mut project = new_drawing(256, 256).unwrap();
    let blob = std::sync::Arc::new(
        TileBlob::encode(
            project.document.color.paint_descriptor(),
            &vec![128; 256 * 256 * 4],
        )
        .unwrap(),
    );
    project.document.layers[0].raster = RasterRevision::backed(RasterData {
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
    w.documents.ram_budget.set(0);
    glib::MainContext::default()
        .block_on(
            w.documents
                .open(&w, (new_drawing(96, 96).unwrap(), None, None)),
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
                .open(&w, (new_drawing(80, 80).unwrap(), None, None)),
        )
        .unwrap_err();
    assert!(error.contains("Free disk space"));
    assert_eq!(w.documents.len(), 2);
    switch(&w, 1);
    let captured = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .capture_project_recovery()
        .unwrap();
    let mut saved = Vec::new();
    captured.write(&mut saved).unwrap();
    Project::read(saved.as_slice(), Default::default()).unwrap();
    w.documents.select(&w, 1, true);
    until(
        || w.documents.len() == 1 && !w.documents.changing.get(),
        "storage failure does not prevent closing",
    );
    w.window.destroy();
    pump(100);
}
