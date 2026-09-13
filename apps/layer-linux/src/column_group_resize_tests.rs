//! Measure painted child allocations, not just the outer drawer or input rate.
use super::*;

pub(super) fn measure(
    w: &Rc<Workspace>,
    perform: &mut impl FnMut(serde_json::Value),
    column: u32,
    after: Option<Panel>,
    (touch, outside): (bool, bool),
    output: &std::path::Path,
    theme: Theme,
) {
    let auto_hide = state(w).workspace.layout.column_settings(column).auto_hide;
    if outside {
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetColumnAutoHide {
                column,
                auto_hide: true,
            },
        });
        pump(100);
    }
    let panel = || {
        w.resolved()
            .collapsed
            .into_iter()
            .find(|c| c.id == column)
            .unwrap()
            .group_panel
            .unwrap()
    };
    let p = panel();
    let vertical = after.is_some();
    let axis = usize::from(vertical);
    let bounds = if outside {
        let resolved = w.resolved();
        resolved
            .dividers
            .iter()
            .find(|d| resolved.column_panel_at_divider(d.id) == Some(column))
            .unwrap()
            .bounds
    } else if vertical {
        p.dividers[0]
    } else {
        p.resize
    };
    let start = [
        bounds.x + bounds.width * 0.5,
        bounds.y + bounds.height * 0.5,
    ];
    let mut origin = start;
    origin[axis] += if !vertical && p.direction == Edge::Left {
        -20.
    } else {
        20.
    };
    let event = |phase: &str, point: [f32; 2]| {
        if touch {
            serde_json::json!({"touch":phase,"point":point})
        } else {
            match phase {
                "down" => serde_json::json!({"point":point,"down":true}),
                "up" => serde_json::json!({"down":false}),
                _ => serde_json::json!({"point":point}),
            }
        }
    };
    let root = find_named(w.surface.upcast_ref(), &format!("column-drawer-{column}")).unwrap();
    let child = root.first_child().unwrap();
    let content = find_named(root.upcast_ref(), "drawer-panel-Brushes").unwrap();
    let handle = find_named(
        root.upcast_ref(),
        &format!("group-panel-resize-{column}-{after:?}"),
    )
    .unwrap();
    perform(serde_json::json!([
        event("down", start),
        event("move", origin)
    ]));
    let refreshes = w.publication.refreshes.get();
    let content_revision = w.publication.content_revision.get();
    w.publication.inputs.borrow_mut().clear();
    let samples = Rc::new(RefCell::new(Vec::new()));
    let clock = w.surface.frame_clock().unwrap();
    let callback = clock.connect_after_paint(glib::clone!(
        #[strong]
        samples,
        #[strong]
        child,
        #[strong]
        content,
        #[strong]
        handle,
        #[weak]
        w,
        move |clock| {
            let Some(timing) = clock.current_timings() else {
                return;
            };
            let actual = if vertical {
                child.height()
            } else {
                child.width()
            };
            let handle_bounds = handle.compute_bounds(&w.surface).unwrap();
            let position = if vertical {
                handle_bounds.y()
            } else {
                handle_bounds.x()
            };
            let p = w
                .resolved()
                .collapsed
                .into_iter()
                .find(|c| c.id == column)
                .unwrap()
                .group_panel
                .unwrap();
            let expected = if vertical {
                p.panels[0].bounds.height
            } else {
                p.bounds.width
            };
            let expected_position = if vertical {
                p.dividers[0].y
            } else {
                p.resize.x
            };
            samples.borrow_mut().push((
                timing,
                actual,
                content.width(),
                position,
                (actual as f32 - expected).abs(),
                (position - expected_position).abs(),
            ));
        }
    ));
    let events: Vec<_> = (0..550)
        .map(|i| {
            let phase = (i as f32 * 0.004 * 5.) % 4.;
            let triangle = if phase < 1. {
                phase
            } else if phase < 3. {
                2. - phase
            } else {
                phase - 4.
            };
            let mut at = origin;
            at[axis] += triangle * 65.;
            event("move", at)
        })
        .collect();
    perform(serde_json::to_value(events).unwrap());
    clock.disconnect(callback);
    let samples = samples.borrow();
    let changing: Vec<_> = samples
        .windows(2)
        .filter(|p| p[0].1 != p[1].1)
        .filter_map(|p| {
            let t = &p[1].0;
            (t.is_complete() && t.presentation_time() > 0).then_some(t.presentation_time())
        })
        .collect();
    let hz = if changing.len() > 1 {
        (changing.len() - 1) as f64 * 1e6 / (changing.last().unwrap() - changing[0]) as f64
    } else {
        0.
    };
    let max_error = samples.iter().map(|s| s.4).fold(0., f32::max);
    let handle_error = samples.iter().map(|s| s.5).fold(0., f32::max);
    let mut cpu = w.publication.inputs.borrow().clone();
    cpu.sort_by(f64::total_cmp);
    assert!(!cpu.is_empty(), "native resize motions must reach Rust");
    let report = serde_json::json!({
        "theme": format!("{theme:?}"), "vertical":vertical, "touch":touch, "outside":outside,
        "geometry_hz":hz, "changed_presentations":changing.len(),
        "max_child_error_px":max_error, "max_handle_error_px":handle_error,
        "content_width_changes":samples.windows(2).filter(|p| p[0].2 != p[1].2).count(),
        "child_range":[samples.iter().map(|s|s.1).min(), samples.iter().map(|s|s.1).max()],
        "initial_width":p.bounds.width, "painted_frames":samples.len(), "inputs":cpu.len(),
        "full_refreshes":w.publication.refreshes.get()-refreshes,
        "dispatch_ms":{"p50":cpu[cpu.len()/2],"p95":cpu[cpu.len()*95/100]},
    });
    eprintln!("GROUP_PANEL_RESIZE {report}");
    std::fs::write(
        output.join(format!(
            "resize-{theme:?}-{vertical}-{touch}-{outside}.json"
        )),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    if std::env::var_os("LAYER_GROUP_RESIZE_BASELINE").is_none() {
        assert!(
            max_error <= 1.,
            "stacked child lags shared geometry: {max_error}px"
        );
        assert!(
            handle_error <= 1.,
            "resize handle lags shared geometry: {handle_error}px"
        );
        assert!(
            changing.len() > 100,
            "painted contents must resize continuously"
        );
        if let Ok(minimum) = std::env::var("LAYER_RESIZE_MIN_HZ") {
            assert!(hz >= minimum.parse::<f64>().unwrap(), "{hz} Hz");
        }
        assert_eq!(w.publication.refreshes.get(), refreshes);
        assert_eq!(w.publication.content_revision.get(), content_revision);
    }
    // Return to the measured origin before releasing, including touch, whose
    // native up event has no position. Keep later sweeps away from size limits.
    perform(serde_json::json!([
        event("move", origin),
        event("up", origin)
    ]));
    assert_eq!(
        root,
        find_named(w.surface.upcast_ref(), &format!("column-drawer-{column}")).unwrap()
    );
    assert_eq!(child, root.first_child().unwrap());
    assert_eq!(
        content,
        find_named(root.upcast_ref(), "drawer-panel-Brushes").unwrap()
    );
    // Cancel actual held input after a visible change, including a queued
    // layout frame. Focus loss restores the gesture without closing the panel.
    let committed = state(w).workspace;
    let p = panel();
    let b = if vertical { p.dividers[0] } else { p.resize };
    let start = [b.x + b.width * 0.5, b.y + b.height * 0.5];
    let mut moved = start;
    moved[axis] += 35.;
    perform(serde_json::json!([
        event("down", start),
        event("move", moved)
    ]));
    assert_ne!(state(w).workspace, committed);
    w.interact(UiInput::Blur);
    perform(serde_json::json!([event("up", moved)]));
    assert_eq!(state(w).workspace, committed, "resize cancellation");
    let p = panel();
    let expected = if vertical {
        p.panels[0].bounds.height
    } else {
        p.bounds.width
    };
    let actual = if vertical {
        child.height()
    } else {
        child.width()
    };
    assert!(
        (actual as f32 - expected).abs() <= 1.,
        "cancelled child geometry"
    );
    if outside {
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetColumnAutoHide { column, auto_hide },
        });
        pump(100);
    }
}
