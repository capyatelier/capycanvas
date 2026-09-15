use super::*;

fn until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "GPU lifecycle timed out");
        pump(20);
    }
}

fn snapshot_pixels(w: &Workspace) -> Vec<[f32; 4]> {
    let gpu = w.snapshot_gpu().unwrap();
    let (project, background, time) = {
        let canvas = w.gpu.borrow();
        let session = &canvas.as_ref().unwrap().session;
        (session.capture_project_recovery().unwrap(),
         session.engine().view().background_rgba_linear,
         session.engine().animation_time())
    };
    glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
        let extent = [project.document.width, project.document.height];
        let mut renderer = gpu.capture(project, background, time, Default::default(), Default::default()).unwrap();
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
    use layer_core::color::{DocumentColor, IntegerDepth, RgbSpace};
    let app = native_test_app("art.capycanvas.WideColorGpuRecovery");
    for color in [
        DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: IntegerDepth::U8,
        },
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: IntegerDepth::U16,
        },
    ] {
        check_gpu_failure_recovery(&app, color);
    }
}

fn check_gpu_failure_recovery(app: &adw::Application, color: layer_core::color::DocumentColor) {
    use layer_render::CanvasRenderer;
    let mut project = new_drawing(384, 256).unwrap();
    project.document.color = color;
    let w = Workspace::with_project(app, Some((project, None)));
    w.window.present();
    until(|| {
        w.gpu.borrow().as_ref().is_some_and(|g| {
            g.session.engine().backend().startup.complete && !g.session.state().filter_load.pending
        })
    });
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
    until(|| {
        let stats = w.gpu.borrow().as_ref().unwrap().session.renderer_stats();
        !stats.samples.is_empty() && !["Unavailable", "—"].contains(&stats.rows[1].value.as_str())
    });
    w.dispatch(UiAction::SetBrushSize { value: 70. });
    let stroke = |fail: bool| {
        let mut gpu = w.gpu.borrow_mut();
        let g = gpu.as_mut().unwrap();
        if fail {
            g.session.renderer_mut().fail_next_frame();
        }
        let camera = g.session.state().camera.clone();
        let m = camera.view().document_to_surface;
        let now = glib::monotonic_time() as u64 * 1000;
        for (i, (phase, x)) in [(PenPhase::Down, 80.), (PenPhase::Up, 240.)]
            .into_iter()
            .enumerate()
        {
            g.session
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
        g.session.frame(now + 2, now + 2).unwrap();
        g.session.engine().document().layers[0].raster.clone()
    };
    let saved_root = stroke(false);
    w.wake();
    until(|| saved_root.host_backed());
    let before = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 8001))
        .unwrap();
    let snapshot_before = snapshot_pixels(&w);
    let checkpoint = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .checkpoint();
    let failed_root = stroke(true);
    w.wake();
    until(|| {
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .rendering_suspended()
    });
    assert!(
        matches!(failed_root.try_data(), Some(Err(_))),
        "failed producer resolves immediately"
    );
    assert!(w.snapshot_gpu().is_err(), "failed owners cannot start new capture jobs");
    {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().unwrap().session;
        assert_eq!(session.engine().checkpoint(), checkpoint);
        assert_eq!(session.engine().document().layers[0].raster, saved_root);
        assert!(session.command(CommandId::SaveDocumentAs).enabled);
        assert!(session.state().document_file.modified);
        let mut bytes = Vec::new();
        session
            .capture_project_recovery()
            .unwrap()
            .write(&mut bytes)
            .unwrap();
        let project =
            layer_core::Project::read(std::io::Cursor::new(bytes), Default::default()).unwrap();
        assert_eq!(project.document.color, color);
    }
    for _ in 0..5 {
        w.wake();
        pump(20);
    }
    assert!(w.frame_timer.borrow().is_none(), "failed workers must not be rescheduled");
    assert!(w.restart_canvas.is_visible());
    click(&w.restart_canvas);
    until(|| {
        w.gpu.borrow().as_ref().is_some_and(|g| {
            !g.session.rendering_suspended() && g.session.engine().backend().startup.complete
        })
    });
    let after = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 8002))
        .unwrap();
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
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
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .checkpoint(),
        checkpoint
    );
    assert!(!w.restart_canvas.is_visible());
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
    until(|| next.host_backed());
    w.window.destroy();
    pump(100);
}
