//! Real mouse/touch edge placement, tear-off sizing and bottom insertion.
use super::*;

#[test]
#[ignore = "isolated native-input.js --workspace-edges"]
fn native_workspace_drag_edges() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let captures = std::env::var("LAYER_TEST_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| dir.join("captures"));
    std::fs::create_dir_all(&captures).unwrap();
    let app = native_test_app("art.capycanvas.DragEdges");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let saved = || layer_ui::durable_layout(&state(&w).workspace.layout);
    let mut step = 0;
    let mut perform = |events: serde_json::Value| {
        let file = dir.join(format!("step-{step}.json"));
        let temporary = file.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec(&events).unwrap()).unwrap();
        std::fs::rename(temporary, file).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.join(format!("done-{step}")).exists() {
            assert!(Instant::now() < deadline, "native input timed out");
            pump(5);
        }
        step += 1;
        pump(100);
    };
    std::fs::write(dir.join("ready"), "ready").unwrap();
    pump(500);
    let mut reports = Vec::new();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for touch in [false, true] {
            for scenario in [
                "floating", "tear-off", "footer", "toolbar", "drawer", "icon",
            ] {
                println!("Checking {theme:?}, touch={touch}, {scenario}");
                let mut fixture = layer_ui::WorkspaceState::default();
                let panel = if scenario == "footer" {
                    Panel::Sizes
                } else if scenario == "toolbar" {
                    Panel::Toolbar
                } else {
                    Panel::Layers
                };
                let source_group = fixture.layout.panel_group(panel).unwrap();
                if scenario == "footer" {
                    fixture
                        .layout
                        .panels
                        .iter_mut()
                        .find(|p| p.id == panel)
                        .unwrap()
                        .hide_tab = true;
                }
                if scenario == "floating" {
                    fixture
                        .layout
                        .move_panel(
                            viewport,
                            panel,
                            DockTarget::Float {
                                position: [650., 300.],
                            },
                        )
                        .unwrap();
                }
                w.dispatch(UiAction::RestoreWorkspace {
                    workspace: Box::new(fixture),
                });
                pump(250);
                if matches!(scenario, "drawer" | "icon") {
                    w.dispatch(UiAction::DoubleClickPanelHandle {
                        group: source_group,
                        viewport,
                    });
                    if scenario == "drawer" {
                        enable_individual_column_panels(&w, source_group);
                        w.dispatch(UiAction::Customize {
                            action: CustomizationAction::ToggleColumnDrawer {
                                group: source_group,
                                panel,
                            },
                        });
                    }
                    pump(400);
                }
                let before = saved();
                let source = if scenario == "drawer" {
                    find_named(
                        w.surface.upcast_ref(),
                        &format!("column-drawer-{source_group}"),
                    )
                    .unwrap()
                } else if scenario == "icon" {
                    // Pick the registered icon body, including its hold rule.
                    let layout = w.resolved();
                    let b = layout
                        .collapsed
                        .iter()
                        .flat_map(|c| &c.groups)
                        .flat_map(|g| &g.icons)
                        .find(|i| i.panel == panel)
                        .unwrap()
                        .bounds;
                    w.drag_source_at([b.x + b.width * 0.5, b.y + b.height * 0.5])
                        .unwrap()
                        .0
                } else {
                    w.groups
                        .borrow()
                        .iter()
                        .find(|g| g.panels.contains(&panel))
                        .unwrap()
                        .root
                        .clone()
                        .upcast()
                };
                let source_bounds = source.compute_bounds(&w.surface).unwrap();
                let handle = if scenario == "drawer" {
                    find_named(&source, "column-drawer-grip").unwrap()
                } else if matches!(scenario, "footer" | "toolbar") {
                    find_css(&source, "panel-grip").unwrap()
                } else if scenario == "icon" {
                    source.clone()
                } else {
                    w.groups
                        .borrow()
                        .iter()
                        .flat_map(|g| &g.tabs)
                        .find(|(p, _)| *p == panel)
                        .unwrap()
                        .1
                        .clone()
                        .upcast()
                };
                let b = handle.compute_bounds(&w.surface).unwrap();
                let start = [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5];
                let event = |phase: &str, p: [f32; 2]| {
                    if touch {
                        serde_json::json!({"touch": phase, "point": p})
                    } else {
                        match phase {
                            "down" => serde_json::json!({"point": p, "down": true}),
                            "up" => serde_json::json!({"down": false}),
                            _ => serde_json::json!({"point": p}),
                        }
                    }
                };
                perform(serde_json::json!([event("down", start)]));
                if scenario == "icon" {
                    pump(700);
                }
                let center = [viewport[0] * 0.5, viewport[1] * 0.5];
                perform(serde_json::json!([event("move", center)]));
                pump(200);
                let moving = || {
                    w.gpu
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .session
                        .workspace_update()
                        .drag
                        .unwrap()
                        .group
                        .unwrap()
                };
                let initial = moving();
                let root = w
                    .groups
                    .borrow()
                    .iter()
                    .find(|g| g.id == initial.id)
                    .unwrap()
                    .root
                    .clone();
                if !matches!(scenario, "toolbar" | "icon") {
                    assert!(
                        (initial.bounds.height - source_bounds.height()).abs() < 1.,
                        "{scenario}: preserve the visible panel height"
                    );
                    assert!(
                        (initial.bounds.width - source_bounds.width()).abs() < 1.,
                        "{scenario}: preserve the visible panel width"
                    );
                }
                let offset = [center[0] - initial.bounds.x, center[1] - initial.bounds.y];
                let refreshes = w.publication.refreshes.get();
                for point in [
                    [center[0], viewport[1] - 2.],
                    [viewport[0] - 2., center[1]],
                    [2., center[1]],
                    [center[0], 2.],
                    [center[0], viewport[1] - 2.],
                ] {
                    perform(serde_json::json!([event("move", point)]));
                    let bounds = moving().bounds;
                    let native = root.compute_bounds(&w.surface).unwrap();
                    assert!(
                        (bounds.x - (point[0] - offset[0])).abs() < 1.
                            && (bounds.y - (point[1] - offset[1])).abs() < 1.,
                        "{scenario}: keep the grabbed point under the contact at {point:?}: {bounds:?}"
                    );
                    assert_eq!(
                        [bounds.width, bounds.height],
                        [initial.bounds.width, initial.bounds.height]
                    );
                    assert!(
                        (native.x() - bounds.x).abs() < 1.
                            && (native.y() - bounds.y).abs() < 1.
                            && (native.width() - bounds.width).abs() < 1.
                            && (native.height() - bounds.height).abs() < 1.,
                        "{scenario}: native allocation {native:?} must match shared {bounds:?}"
                    );
                }
                assert_eq!(
                    w.publication.refreshes.get(),
                    refreshes,
                    "edge motion retains controls"
                );
                assert_eq!(w.surface.overflow(), gtk::Overflow::Hidden);
                let file = captures.join(format!("{theme:?}-{touch}-{scenario}-bottom.png"));
                capture_reference(&w, file.to_str().unwrap(), 1.);
                // A refresh during the held drag must not restore the clamped
                // saved position before the next input sample arrives.
                w.surface.queue_allocate();
                if scenario == "floating" {
                    let mut measurements = state(&w).workspace.layout.measurements;
                    measurements
                        .iter_mut()
                        .find(|m| m.panel == panel)
                        .unwrap()
                        .content_height += 300.;
                    w.dispatch(UiAction::MeasurePanels { measurements });
                }
                pump(150);
                assert!(
                    (root.compute_bounds(&w.surface).unwrap().y() - moving().bounds.y).abs() < 1.
                );
                assert!(
                    (root.compute_bounds(&w.surface).unwrap().height() - initial.bounds.height)
                        .abs()
                        < 1.
                );

                if matches!(scenario, "tear-off" | "drawer" | "icon") {
                    let target = w
                        .resolved()
                        .groups
                        .into_iter()
                        .find(|g| g.active == Panel::Sizes)
                        .unwrap();
                    let point = [
                        target.bounds.x + target.bounds.width * 0.5,
                        target.bounds.y + target.bounds.height - 3.,
                    ];
                    perform(serde_json::json!([event("move", point)]));
                    let hint = w
                        .drop_hint
                        .borrow()
                        .clone()
                        .expect("bottom insertion must be reachable");
                    assert!(
                        matches!(hint.target, DockTarget::Split { group, edge: Edge::Bottom } if group == target.id),
                        "bottom indicator: {:?}",
                        hint.target
                    );
                    capture_reference(
                        &w,
                        captures
                            .join(format!("{theme:?}-{touch}-{scenario}-insert.png"))
                            .to_str()
                            .unwrap(),
                        1.,
                    );
                    perform(serde_json::json!([event("up", point)]));
                    let docked = w
                        .resolved()
                        .groups
                        .into_iter()
                        .find(|g| g.panels.contains(&panel))
                        .unwrap();
                    assert!(!docked.floating);
                    assert!(
                        docked.bounds.height < initial.bounds.height,
                        "drop reflows into the bottom slot"
                    );
                    let native = w
                        .groups
                        .borrow()
                        .iter()
                        .find(|g| g.id == docked.id)
                        .unwrap()
                        .root
                        .compute_bounds(&w.surface)
                        .unwrap();
                    assert!((native.height() - docked.bounds.height).abs() < 1.);
                    capture_reference(
                        &w,
                        captures
                            .join(format!("{theme:?}-{touch}-{scenario}-docked.png"))
                            .to_str()
                            .unwrap(),
                        1.,
                    );
                } else if scenario == "floating" {
                    let point = [center[0], viewport[1] - 2.];
                    assert!(w.drop_hint.borrow().is_none());
                    perform(serde_json::json!([event("up", point)]));
                    let bounds = w
                        .resolved()
                        .groups
                        .into_iter()
                        .find(|g| g.id == initial.id)
                        .unwrap()
                        .bounds;
                    assert!(bounds.y + bounds.height <= viewport[1] - WORKSPACE_SPACING + 1.);
                    assert_eq!(
                        [bounds.width, bounds.height],
                        [initial.bounds.width, initial.bounds.height]
                    );
                    capture_reference(
                        &w,
                        captures
                            .join(format!("{theme:?}-{touch}-placed.png"))
                            .to_str()
                            .unwrap(),
                        1.,
                    );
                } else {
                    let sequence = w
                        .workspace_drag
                        .borrow()
                        .as_ref()
                        .and_then(|d| d.sequence.clone());
                    w.workspace_drag_input(ContactPhase::Cancel, center, sequence);
                    perform(serde_json::json!([event("up", center)]));
                    assert_eq!(saved(), before, "cancel restores the original layout");
                    reports.push(serde_json::json!({"theme": format!("{theme:?}"), "touch": touch,
                        "scenario": scenario, "preview": initial.bounds, "result": "cancelled", "refreshes": 0}));
                    continue;
                }
                assert_ne!(saved(), before);
                assert!(w.workspace_drag.borrow().is_none());
                reports.push(
                    serde_json::json!({"theme": format!("{theme:?}"), "touch": touch,
                    "scenario": scenario, "preview": initial.bounds, "refreshes": 0}),
                );
            }
        }
    }
    std::fs::write(
        captures.join("report.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("finished"), "finished").unwrap();
    w.window.close();
    pump(100);
}
