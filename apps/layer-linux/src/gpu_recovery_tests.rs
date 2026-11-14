use super::*;

fn snapshot_pixels(w: &Workspace) -> Vec<[f32; 4]> {
    let gpu = w.snapshot_gpu().unwrap();
    let project = {
        let canvas = w.gpu.borrow();
        let session = &canvas.as_ref().unwrap().session;
        session.capture_artwork().unwrap()
    };
    glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
        let extent = project.composition().size;
        let mut renderer = gpu.capture(project, Default::default()).unwrap();
        renderer.read_region([0, 0, extent[0], extent[1]]).unwrap()
    })).unwrap()
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_diagnostics_and_gpu_failure_recovery() {
    let app = native_test_app("art.capycanvas.GpuRecovery");
    check_gpu_failure_recovery(&app, Default::default());
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_wide_color_gpu_failure_recovery() {
    use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
    let app = native_test_app("art.capycanvas.WideColorGpuRecovery");
    for (space, depth) in [
        (RgbSpace::DisplayP3, SampleDepth::U8),
        (RgbSpace::ProPhoto, SampleDepth::U16),
        (RgbSpace::Srgb, SampleDepth::F16),
        (RgbSpace::DisplayP3, SampleDepth::F32),
    ] {
        check_gpu_failure_recovery(&app, DocumentColor { space, depth });
    }
}

fn check_gpu_failure_recovery(app: &adw::Application, color: layer_core::color::DocumentColor) {
    use layer_render::CanvasRenderer;
    let mut project = new_drawing(384, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let composition = composition_mut(&mut project);
    composition.color = color;
    composition.blend = composition.blend.for_depth(color.depth);
    let w = Workspace::with_project(app, Some((project, None)));
    apply_fixture_theme(&w);
    w.window.present();
    until(
        || {
            w.gpu.borrow().as_ref().is_some_and(|g| {
                g.session.engine().backend().startup.complete
                    && !g.session.state().filter_load.pending
            })
        },
        "canvas startup",
    );
    if ui_session(&w).engine().document().composition().color.depth.is_float() {
        w.dispatch(UiAction::Color { action: layer_ui::ColorAction::Definition {
            color: layer_core::color::RgbColor::from_linear(
                layer_core::color::RgbSpace::ProPhoto, [8., -0.125, 2., 1.]).unwrap(),
        }});
    }
    let proof = super::proof::benchmark_proof(&w);
    let proof_cache = w.proof.cache_info();
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetPanelVisible {
            panel: Panel::Stats,
            visible: true,
        },
    });
    let group = state(&w)
        .workspace
        .layout
        .panel_group(Panel::Stats)
        .unwrap();
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetColumnCollapsed {
            group,
            collapsed: true,
        },
    });
    let column = state(&w)
        .workspace
        .layout
        .collapsed_column_for_group(group)
        .unwrap();
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetColumnDrawers {
            column,
            drawers: false,
        },
    });
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::ToggleColumnDrawer {
            group,
            panel: Panel::Stats,
        },
    });
    assert!(w.effects.stats.is_mapped());
    for opacity in [0.9, 0.8, 1.] {
        w.dispatch(UiAction::SetLayerOpacity { id: None, opacity });
        pump(40);
    }
    until(
        || {
            let stats = ui_session(&w).renderer_stats();
            !stats.samples.is_empty()
                && !["Unavailable", "—"].contains(&stats.rows[1].value.as_str())
        },
        "renderer statistics",
    );
    let fps_title = w.localization().text(layer_ui::MessageId::DIAGNOSTICS_FPS).to_string();
    let stat_rows = || widgets(w.effects.stats.upcast_ref()).filter(|row| row.has_css_class("meter-row")).collect::<Vec<_>>();
    let caption = |row: &gtk::Widget| row.first_child().and_then(|line| line.first_child()).and_then(|lead| lead.first_child())
        .and_downcast::<gtk::Label>().map(|label| label.text().to_string());
    let displayed_fps = || stat_rows().into_iter().find(|row| caption(row).as_deref() == Some(fps_title.as_str()))
        .and_then(|row| row.first_child()?.last_child().and_downcast::<gtk::Label>()).and_then(|label| label.text().parse::<f32>().ok());
    until(|| displayed_fps().is_some_and(|fps| fps > 0.), "FPS shows canvas presentation feedback");
    assert!(ui_session(&w).engine().backend().stats.lock().unwrap().presented.iter().any(|frame| frame[3] == 1));
    until(|| displayed_fps() == Some(0.) && w.frame_timer.borrow().is_none(), "idle FPS falls to zero");
    let idle_frames = ui_session(&w).engine().backend().stats.lock().unwrap().cpu.len();
    let idle_presentations = ui_session(&w).engine().backend().stats.lock().unwrap().presented.len();
    let idle_memory = || {
        let session = ui_session(&w);
        let backend = session.engine().backend();
        let stats = backend.stats.lock().unwrap();
        assert_eq!(stats.cpu.len(), idle_frames, "Memory sampling does not render canvas frames");
        assert_eq!(stats.presented.len(), idle_presentations, "Memory sampling does not present canvas frames");
        assert!(w.frame_timer.borrow().is_none());
        drop(stats);
        backend.telemetry().memory
    };
    until(|| idle_memory().is_some_and(|sample| sample.process.is_some_and(|process| process.bytes > 0)), "process memory sample while canvas is idle");
    let sampled_at = idle_memory().unwrap().sampled_at;
    until(|| idle_memory().is_some_and(|sample| sample.sampled_at > sampled_at && sample.process.is_some_and(|process| process.bytes > 0)), "memory sample refreshes without canvas frames");
    let metrics = ui_session(&w).renderer_stats();
    let captions: Vec<_> = stat_rows().iter().filter_map(caption).collect();
    assert_eq!(&captions[..metrics.rows.len()], metrics.rows.iter().map(|row| row.label.clone()).collect::<Vec<_>>());
    assert!(metrics.memory.rows.first().is_some_and(|row| ["RAM", "Memory"].contains(&row.label.as_str()) && row.meter.is_some()), "{:?}", metrics.memory);
    until(|| descendants::<crate::meter::Meter>(w.effects.stats.upcast_ref()).iter().any(|meter| meter.is_mapped() && meter.width() > 0), "the memory bar is drawn");
    assert_eq!(displayed_fps(), Some(0.));
    assert_eq!(ui_session(&w).engine().backend().stats.lock().unwrap().cpu.len(), idle_frames, "Diagnostics does not keep the canvas rendering");
    assert!(w.frame_timer.borrow().is_none());
    if let Some(scroller) = w.effects.stats.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>() {
        let rows = scroller.vadjustment();
        rows.set_value(rows.upper() - rows.page_size());
    }
    pump(60);
    let artifacts = std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from);
    if let Some(output) = &artifacts {
        crate::capture(&w, output.join("stats-memory.png").to_str().unwrap());
    }
    let section = widgets(w.effects.stats.upcast_ref()).find(|widget| widget.has_css_class("meter-section")).expect("memory section");
    let heading = section.first_child().expect("memory heading");
    for (index, row) in stat_rows().into_iter().chain([heading.clone()]).enumerate() {
        if let Some(scroller) = row.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>() {
            let bounds = row.compute_bounds(&scroller).unwrap();
            let rows = scroller.vadjustment();
            rows.set_value(rows.value() + f64::from(bounds.y()) - 8.);
            pump(60);
        }
        let bounds = row.compute_bounds(&w.window).unwrap();
        w.tooltips.hover(w.window.upcast_ref(), [f64::from(bounds.x() + bounds.width() / 2.), f64::from(bounds.y() + bounds.height() / 2.)]);
        pump(620);
        let tip = find_named(w.window.upcast_ref(), "pen-tooltip").and_downcast::<gtk::Popover>().expect("Diagnostics tooltip");
        let text: Vec<_> = descendants::<gtk::Label>(tip.upcast_ref()).iter().map(|label| label.text().to_string()).collect();
        let expected = if row == heading { Some("Rest of app") } else if row.is_ancestor(&section) { Some("In use") }
            else if caption(&row).as_deref() == Some("CPU · ms") { Some("Typical") } else { None };
        assert!(text.iter().any(|line| !line.is_empty() && expected.is_none_or(|expected| line == expected)), "{:?} tooltip: {text:?}", caption(&row));
        if let Some(output) = &artifacts {
            let picture = gtk::Snapshot::new();
            gtk::WidgetPaintable::new(Some(&tip)).snapshot(&picture, f64::from(tip.width()), f64::from(tip.height()));
            w.window.renderer().unwrap().render_texture(picture.to_node().unwrap(), None)
                .save_to_png(output.join(format!("stats-tip-{index}.png"))).unwrap();
        }
        w.tooltips.hide();
    }
    let navigation_document = ui_session(&w).engine().document().clone();
    let navigation_checkpoint = ui_session(&w).engine().checkpoint();
    let presentations = ui_session(&w).engine().backend().stats.lock().unwrap().presented.iter().filter(|frame| frame[3] == 1).count();
    let camera = state(&w).camera;
    for step in 0..8 {
        let current = state(&w).camera;
        let change = ui_session_mut(&w).gesture(current.translation, [camera.translation[0] + step as f32 * 3., camera.translation[1]], 1., 0.);
        w.changed(change);
        pump(40);
    }
    until(|| displayed_fps().is_some_and(|fps| fps > 0.), "camera navigation updates displayed FPS");
    assert_eq!(ui_session(&w).engine().document(), &navigation_document);
    assert_eq!(ui_session(&w).engine().checkpoint(), navigation_checkpoint);
    assert!(ui_session(&w).engine().backend().stats.lock().unwrap().presented.iter().filter(|frame| frame[3] == 1).count() > presentations);
    let current = state(&w).camera;
    let change = ui_session_mut(&w).gesture(current.translation, camera.translation, 1., 0.);
    w.changed(change);
    w.dispatch(UiAction::SetBrushSize { value: 70. });
    until(|| ui_session(&w).paint_ready(), "the resized brush is ready to paint");
    let stroke = |fail: bool| {
        let mut gpu = w.gpu.borrow_mut();
        let g = gpu.as_mut().unwrap();
        if fail {
            g.session.renderer_mut().fail_next_frame();
        }
        let camera = g.session.state().camera.clone();
        let now = glib::monotonic_time() as u64 * 1000;
        for (i, (phase, x)) in [(PenPhase::Down, 80.), (PenPhase::Up, 240.)]
            .into_iter()
            .enumerate()
        {
            g.session
                .pen(PenEvent {
                    device_id: 71,
                    timestamp_ns: now + i as u64,
                    pressure: 0.7,
                    ..pen_event(&camera, [x, 120.], phase, now + i as u64)
                });
        }
        g.session.frame(now + 2, now + 2).unwrap();
        active_raster(g.session.engine().document()).clone()
    };
    let saved_root = stroke(false);
    w.wake();
    until(|| saved_root.host_backed(), "saved stroke backing");
    let before = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 8001))
        .unwrap();
    let snapshot_before = snapshot_pixels(&w);
    let checkpoint = ui_session(&w)
        .engine()
        .checkpoint();
    if color.depth.is_float() {
        until(|| w.local_tone.ready_count().is_some(), "local tone guide");
    }
    let local_before=w.local_tone.ready_count();
    let failed_root = stroke(true);
    w.wake();
    until(
        || {
            ui_session(&w)
                .rendering_suspended()
        },
        "rendering suspended",
    );
    assert!(
        matches!(failed_root.try_data(), Some(Err(_))),
        "failed producer resolves immediately"
    );
    assert!(w.snapshot_gpu().is_err(), "failed owners cannot start new capture jobs");
    {
        let mut gpu = w.gpu.borrow_mut();
        let renderer = gpu.as_mut().unwrap().session.renderer_mut();
        let cause = renderer.ready().unwrap_err();
        assert!(cause.contains("Validation Error"), "lost worker cause: {cause}");
        assert_eq!(renderer.ready().unwrap_err(), cause,
            "optional result polls must not consume the worker failure");
    }
    {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().unwrap().session;
        assert_eq!(session.engine().checkpoint(), checkpoint);
        assert_eq!(active_raster(session.engine().document()), &saved_root);
        assert!(session.command(CommandId::SaveDocumentAs).enabled);
        assert!(session.state().document_file.modified);
        let mut bytes = Vec::new();
        write_capture(&session.capture_artwork().unwrap(), &mut bytes).unwrap();
        let project = open_native_document(std::io::Cursor::new(bytes));
        assert_eq!(project.composition().color, color);
    }
    for _ in 0..5 {
        w.wake();
        pump(20);
    }
    assert!(w.frame_timer.borrow().is_none(), "failed workers must not be rescheduled");
    assert!(w.restart_canvas.is_visible());
    click(&w.restart_canvas);
    until(
        || {
            w.gpu.borrow().as_ref().is_some_and(|g| {
                !g.session.rendering_suspended() && g.session.engine().backend().startup.complete
            })
        },
        "canvas restart",
    );
    let after = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 8002))
        .unwrap();
    if proof.is_some() {
        super::proof::wait_proof(&w, "Proof:");
        assert_eq!(w.proof.cache_info(), proof_cache, "GPU recovery reuses the validated CPU transform");
    }
    assert_eq!(
        ui_session(&w)
            .engine()
            .backend()
            .document_color(),
        color
    );
    assert_eq!(
        after.bytes, before.bytes,
        "restart restores exact surviving pixels"
    );
    assert_eq!(snapshot_pixels(&w), snapshot_before,
        "snapshot workers use the replacement canvas device after recovery");
    assert_eq!(
        ui_session(&w)
            .engine()
            .checkpoint(),
        checkpoint
    );
    assert!(!w.restart_canvas.is_visible());
    if color.depth.is_float() {
        until(
            || {
                w.local_tone
                    .ready_count()
                    .is_some_and(|n| n > local_before.unwrap())
            },
            "local tone refresh",
        );
    }
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    pump(100);
    w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    pump(100);
    let redone = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 8003))
        .unwrap();
    assert_eq!(
        redone.bytes, before.bytes,
        "earlier undo/redo survives recovery"
    );
    let next = stroke(false);
    w.wake();
    until(|| next.host_backed(), "next stroke backing");
    w.window.destroy();
    pump(100);
}
