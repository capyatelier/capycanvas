//! Canvas action bar journeys with real Mutter mouse and touch delivery.
use super::*;
use serde_json::json;

pub(super) fn remote_input() -> RemoteInput {
    let input = RemoteInput::new().settle_ms(150).timeout_secs(30);
    input.ready();
    pump(300);
    input
}

pub(super) fn until_some<T>(mut find: impl FnMut() -> Option<T>, message: &str) -> T {
    let mut found = None;
    until(|| {
        found = find();
        found.is_some()
    }, message);
    found.unwrap()
}

pub(super) fn center(w: &Workspace, widget: &gtk::Widget) -> [f32; 2] {
    let b = widget.compute_bounds(&w.window).expect("mapped widget");
    [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]
}

pub(super) fn bar_widget(w: &Workspace, name: &str) -> gtk::Widget {
    find_named(w.canvas_bar.root.upcast_ref(), name).unwrap_or_else(|| panic!("{name}"))
}

/// Document bounds of the transform anchor in window coordinates.
pub(super) fn anchor_in_window(w: &Workspace) -> [f32; 4] {
    let s = state(w);
    let [x0, y0, x1, y1] = s.canvas_bar.expect("canvas bar").anchor.expect("anchor");
    let m = s.camera.document_to_surface();
    let scale = w.area.scale_factor() as f32;
    let map = |x: f32, y: f32| {
        let p = gtk::graphene::Point::new(
            (m[0] * x + m[2] * y + m[4]) / scale,
            (m[1] * x + m[3] * y + m[5]) / scale,
        );
        let p = w.area.compute_point(&w.window, &p).unwrap();
        [p.x(), p.y()]
    };
    let corners = [map(x0, y0), map(x1, y0), map(x1, y1), map(x0, y1)];
    let xs = corners.map(|c| c[0]);
    let ys = corners.map(|c| c[1]);
    [
        xs.iter().copied().fold(f32::INFINITY, f32::min),
        ys.iter().copied().fold(f32::INFINITY, f32::min),
        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    ]
}

/// Press a bar command, through More when it does not fit.
fn press_bar_command(w: &Workspace, native: &mut RemoteInput, name: &str, label: &str) {
    let widget = bar_widget(w, name);
    if widget.is_mapped() {
        native.click(center(w, &widget));
    } else {
        native.click(center(w, &bar_widget(w, "canvas-bar-more")));
        let item = until_some(|| mapped_label(w.canvas_bar.root.upcast_ref(), label), label);
        native.click(center(w, &item));
    }
}

pub(super) fn shown(w: &Workspace) -> bool {
    w.canvas_bar.root.is_mapped() && w.canvas_bar.visible_bounds().is_some()
}

pub(super) use super::mapped_label;

fn transforming(w: &Workspace) -> bool {
    state(w).layer_tools.tool == LayerCanvasTool::Transform
}

fn aspect(w: &Workspace) -> bool {
    state(w)
        .commands
        .iter()
        .any(|c| c.id == CommandId::TransformUniform && c.selected)
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse/touch delivery"]
fn native_canvas_bar_input() {
    let app = native_test_app("art.capycanvas.CanvasBar");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke {
        command: CommandId::FitCanvas,
    });
    w.dispatch(UiAction::SetColor {
        rgba: [0.12, 0.38, 0.72, 1.],
    });
    let id = ui_session(&w).engine().document().working.occurrence.map(layer_ui::occurrence_token).unwrap();
    w.dispatch(UiAction::Layer { action: LayerAction::AddMask { id, replace: false } });
    w.dispatch(UiAction::Layer { action: LayerAction::Select { id, mask: false } });
    pump(200);
    w.dispatch(UiAction::Invoke {
        command: CommandId::Lasso,
    });
    native_pen_path(
        &w,
        &[[650., 500.], [1150., 500.], [1150., 850.], [650., 850.], [650., 500.]],
    );
    w.dispatch(UiAction::Layer {
        action: LayerAction::FillSelection,
    });
    pump(200);
    let revision = || {
        ui_session(&w)
            .engine()
            .document()
            .revision
    };
    let kind = |w: &Workspace| state(w).canvas_bar.map(|b| b.context.kind);
    until(
        || kind(&w) == Some(layer_ui::CanvasBarKind::Selection) && shown(&w),
        "the selection bar appears beside the new selection",
    );
    let mut native = remote_input();
    press_bar_command(&w, &mut native, "canvas-bar-ScaleRotate", &CommandId::ScaleRotate.label());
    until(
        || kind(&w) == Some(layer_ui::CanvasBarKind::Transform) && shown(&w),
        "Transform on the selection bar opens the transform bar",
    );
    w.dispatch(UiAction::Invoke { command: CommandId::CancelTransform });
    until(|| kind(&w) == Some(layer_ui::CanvasBarKind::Selection), "Cancel keeps the linked selection");
    let cached_started = std::time::Instant::now();
    w.dispatch(UiAction::Invoke { command: CommandId::ScaleRotate });
    until(|| kind(&w) == Some(layer_ui::CanvasBarKind::Transform), "Cached linked Transform is ready");
    println!("Linked cached Transform dispatch-to-shared-ready ms={:.3}", cached_started.elapsed().as_secs_f64() * 1000.);
    pump(200);
    let bar = w.canvas_bar.root.compute_bounds(&w.window).unwrap();
    let anchor = anchor_in_window(&w);
    assert!(bar.y() > anchor[3], "the bar sits below the transform box");
    let centre = (anchor[0] + anchor[2]) * 0.5;
    assert!(
        bar.x() < centre && centre < bar.x() + bar.width(),
        "the bar spans the centre of the transform box: {bar:?} {anchor:?}"
    );
    let scale = w.area.scale_factor() as f32;
    until(
        || {
            w.glass.borrow().iter().any(|r| {
                (r.bounds[0] / scale - bar.x()).abs() < 2.
                    && (r.bounds[1] / scale - bar.y()).abs() < 2.
            })
        },
        "the bar registers its glass region",
    );
    let dir = artifact_dir("../../artifacts/canvas-action-bar");
    let before = revision();
    let modes = bar_widget(&w, "canvas-bar-choice-transform-mode");
    let segment = |index: usize| {
        let mut child = modes.first_child();
        for _ in 0..index {
            child = child.and_then(|c| c.next_sibling());
        }
        child.expect("mode segment")
    };
    assert!(!aspect(&w));
    native.click(center(&w, &segment(1)));
    until(|| aspect(&w), "a mouse click on Uniform keeps proportions");
    let point = center(&w, &segment(0));
    native.perform(json!([
        {"touch": "down", "point": point}, {"wait_ms": 40}, {"touch": "up"}
    ]));
    until(|| !aspect(&w), "a finger tap on Free releases them");
    assert!(transforming(&w));
    assert_eq!(revision(), before, "bar taps never paint or commit");
    let perspective = |w: &Workspace| find_named(w.canvas_bar.root.upcast_ref(), "canvas-bar-TransformPerspective").is_some();
    native.click(center(&w, &segment(2)));
    until(|| perspective(&w), "Distort offers Perspective");
    let corner = [anchor[2], anchor[3]];
    native.perform(json!([
        {"point": corner}, {"down": true}, {"wait_ms": 40},
        {"point": [corner[0] + 30., corner[1] + 15.]}, {"wait_ms": 20},
        {"point": [corner[0] + 60., corner[1] + 30.]}, {"wait_ms": 20}, {"down": false}
    ]));
    until(|| w.canvas_bar.visible_bounds().is_some(), "the bar returns after the corner drag");
    let distorted = anchor_in_window(&w);
    assert!(
        (distorted[2] - anchor[2] - 60.).abs() < 3. && (distorted[3] - anchor[3] - 30.).abs() < 3.,
        "a Distort corner drag moves that corner: {anchor:?} {distorted:?}"
    );
    assert!((distorted[0] - anchor[0]).abs() < 1. && (distorted[1] - anchor[1]).abs() < 1., "the opposite corner stays");
    capture_reference(&w, &format!("{dir}/distort.png"), 1.);
    let selected = |w: &Workspace, id: CommandId| state(w).commands.iter().any(|c| c.id == id && c.selected);
    assert!(selected(&w, CommandId::TransformBicubic), "Distort resamples with Bicubic");
    let press = |native: &mut RemoteInput, widget: &gtk::Widget| {
        native.click(screen_point(widget, &w.window, [0.5, 0.5]));
    };
    let from_more = |native: &mut RemoteInput, labels: &[&str]| {
        press(native, &bar_widget(&w, "canvas-bar-more"));
        for label in labels {
            let item = until_some(|| mapped_label(w.canvas_bar.root.upcast_ref(), label), label);
            press(native, &item);
        }
    };
    let interpolation = bar_widget(&w, "canvas-bar-choice-transform-interpolation");
    if interpolation.is_mapped() {
        press(&mut native, &interpolation);
        let nearest = until_some(|| mapped_label(w.window.upcast_ref(), "Nearest"), "the dropdown lists the filters");
        press(&mut native, &nearest);
    } else {
        from_more(&mut native, &["Interpolation", "Nearest"]);
    }
    until(|| selected(&w, CommandId::TransformNearest), "choosing Nearest resamples with hard edges");
    let reset = bar_widget(&w, "canvas-bar-ResetTransform");
    if reset.is_mapped() {
        press(&mut native, &reset);
    } else {
        from_more(&mut native, &["Reset transform"]);
    }
    until(
        || !perspective(&w) && anchor_in_window(&w).iter().zip(anchor).all(|(a, b)| (a - b).abs() < 1.),
        "Reset returns to Free and the starting box",
    );
    assert_eq!(revision(), before, "distorting and resetting never commit");
    let inside = [anchor[0] + (anchor[2] - anchor[0]) * 0.35, anchor[1] + (anchor[3] - anchor[1]) * 0.4];
    native.perform(json!([
        {"point": inside}, {"down": true}, {"wait_ms": 40},
        {"point": [inside[0] + 40., inside[1] + 20.]}
    ]));
    assert!(w.canvas_bar.visible_bounds().is_none(), "the bar hides during a canvas drag");
    native.perform(json!([{"down": false}]));
    until(|| w.canvas_bar.visible_bounds().is_some(), "the bar returns after the drag");
    let moved = w.canvas_bar.root.compute_bounds(&w.window).unwrap();
    let shift = moved.x() - bar.x();
    assert!(
        (moved.y() - bar.y() - 20.).abs() < 3. && (-1. ..=43.).contains(&shift),
        "the bar follows the moved box, clamped to the work area: {bar:?} -> {moved:?}"
    );
    let more = bar_widget(&w, "canvas-bar-more");
    native.click(center(&w, &more));
    until(|| w.canvas_bar.menu_open(), "More opens its menu");
    native.key(0xff1b);
    until(|| !w.canvas_bar.menu_open(), "Escape closes the menu first");
    assert!(transforming(&w), "opening and closing More keeps the transform");
    w.interact(UiInput::Blur);
    pump(100);
    assert!(transforming(&w), "losing window focus keeps the transform");
    w.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    pump(300);
    assert!(shown(&w), "the bar stays visible in Zen");
    assert!(!w.canvas_bar.root.has_css_class("zen-hidden"));
    w.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    pump(300);
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(300);
        capture_reference(&w, &format!("{dir}/transform-{theme:?}.png"), 1.);
    }
    let apply = bar_widget(&w, "canvas-bar-ApplyTransform");
    native.click(center(&w, &apply));
    until(
        || state(&w).canvas_bar.is_some_and(|b| b.context.kind == layer_ui::CanvasBarKind::Selection),
        "Apply hands the bar to the moved selection",
    );
    assert!(!transforming(&w));
    assert!(revision() > before, "Apply commits the transform");
    w.dispatch(UiAction::Invoke {
        command: CommandId::ShowCanvasActionBar,
    });
    pump(100);
    assert!(state(&w).canvas_bar.is_none(), "the selection bar respects the toggle");
    w.dispatch(UiAction::Invoke {
        command: CommandId::ScaleRotate,
    });
    until(|| shown(&w), "completion stays available while the bar is off");
    assert!(find_named(w.canvas_bar.root.upcast_ref(), "canvas-bar-choice-transform-mode").is_none());
    let cancel = bar_widget(&w, "canvas-bar-CancelTransform");
    native.click(center(&w, &cancel));
    until(|| !transforming(&w), "Cancel from the completion-only bar");
}

pub(super) fn canvas_point(w: &Workspace, document: [f32; 2]) -> [f32; 2] {
    let m = state(w).camera.document_to_surface();
    let scale = w.area.scale_factor() as f32;
    let p = gtk::graphene::Point::new(
        (m[0] * document[0] + m[2] * document[1] + m[4]) / scale,
        (m[1] * document[0] + m[3] * document[1] + m[5]) / scale,
    );
    let p = w.area.compute_point(&w.window, &p).unwrap();
    [p.x(), p.y()]
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_canvas_bar_polygon_input() {
    let app = native_test_app("art.capycanvas.CanvasBarPolygon");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke {
        command: CommandId::FitCanvas,
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::PolygonSelect,
    });
    pump(200);
    let mut native = remote_input();
    let click = |native: &mut RemoteInput, point: [f32; 2]| {
        native.perform(json!([{"point": point}, {"down": true}, {"wait_ms": 30}, {"down": false}]));
    };
    for p in [[600., 400.], [1200., 400.], [1200., 900.]] {
        click(&mut native, canvas_point(&w, p));
    }
    until(|| shown(&w), "the polygon bar appears");
    let bar = w.canvas_bar.root.compute_bounds(&w.window).unwrap();
    let area = w.area.compute_bounds(&w.window).unwrap();
    assert!(
        bar.y() + bar.height() > area.y() + area.height() - 80.,
        "the polygon bar sits at the bottom edge"
    );
    let remove = bar_widget(&w, "canvas-bar-RemoveSelectionPoint");
    native.click(center(&w, &remove));
    until(|| ui_session(&w).state().canvas_bar.as_ref().is_some_and(|b| {
        b.completion.iter().any(|i| matches!(&i.option, ToolOption::Action { state, .. } if state.id == CommandId::CompleteSelection && !state.enabled))
    }), "removing a point disables Finish");
    click(&mut native, canvas_point(&w, [700., 950.]));
    let finish = bar_widget(&w, "canvas-bar-CompleteSelection");
    until(|| finish.is_sensitive(), "Finish is available with three points");
    native.click(center(&w, &finish));
    until(
        || ui_session(&w).engine().document().working.selection.is_some(),
        "Finish creates the selection",
    );
    until(
        || state(&w).canvas_bar.is_some_and(|b| b.context.kind == layer_ui::CanvasBarKind::Selection),
        "the finished polygon hands the bar to its selection",
    );
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_canvas_bar_distorts_a_pixel_selection() {
    let app = native_test_app("art.capycanvas.CanvasBarPixels");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    w.dispatch(UiAction::SetColor { rgba: [0.12, 0.38, 0.72, 1.] });
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    native_pen_path(&w, &[[650., 500.], [1150., 500.], [1150., 850.], [650., 850.], [650., 500.]]);
    w.dispatch(UiAction::Layer { action: LayerAction::FillSelection });
    w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
    w.dispatch(UiAction::Invoke { command: CommandId::ColorSelect });
    pump(200);
    let pixels = |w: &Workspace| {
        ui_session(&w).engine().document().working.selection.as_ref()
            .is_some_and(|s| matches!(s.shape, layer_core::SelectionShape::Pixels(_)))
    };
    let mut native = remote_input();
    native.perform(json!([{"point": [900., 675.]}, {"down": true}, {"wait_ms": 30}, {"down": false}]));
    until(|| pixels(&w), "Color Select makes a pixel selection");
    w.dispatch(UiAction::Invoke { command: CommandId::ScaleRotate });
    w.dispatch(UiAction::Invoke { command: CommandId::TransformDistort });
    until(|| shown(&w) && transforming(&w), "the transform bar appears");
    let revision = ui_session(&w).engine().document().revision;
    let anchor = anchor_in_window(&w);
    let corner = [anchor[2], anchor[1]];
    native.perform(json!([
        {"point": corner}, {"down": true}, {"wait_ms": 40},
        {"point": [corner[0] + 40., corner[1] - 20.]}, {"wait_ms": 20},
        {"point": [corner[0] + 80., corner[1] - 40.]}, {"wait_ms": 20}, {"down": false}
    ]));
    until(|| shown(&w), "the bar returns after the corner drag");
    let apply = bar_widget(&w, "canvas-bar-ApplyTransform");
    native.click(center(&w, &apply));
    until(|| !transforming(&w), "Apply finishes once the resampled coverage returns");
    let document = ui_session(&w).engine().document().clone();
    assert!(document.revision > revision, "Apply commits the distorted pixels");
    assert!(pixels(&w), "the selection follows the distortion as pixel coverage");
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_canvas_bar_warps_a_selection --tablet"]
fn native_canvas_bar_warps_a_selection() {
    let app = native_test_app("art.capycanvas.CanvasBarWarp");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    w.dispatch(UiAction::SetColor { rgba: [0.12, 0.38, 0.72, 1.] });
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    native_pen_path(&w, &[[650., 500.], [1150., 500.], [1150., 850.], [650., 850.], [650., 500.]]);
    w.dispatch(UiAction::Layer { action: LayerAction::FillSelection });
    let kind = |w: &Workspace| state(w).canvas_bar.map(|b| b.context.kind);
    until(|| kind(&w) == Some(layer_ui::CanvasBarKind::Selection) && shown(&w), "the selection bar appears");
    let mut native = remote_input();
    let click = |native: &mut RemoteInput, widget: &gtk::Widget| {
        native.click(center(&w, widget));
    };
    press_bar_command(&w, &mut native, "canvas-bar-ScaleRotate", &CommandId::ScaleRotate.label());
    until(|| kind(&w) == Some(layer_ui::CanvasBarKind::Transform) && shown(&w), "Transform opens the transform bar");
    let modes = bar_widget(&w, "canvas-bar-choice-transform-mode");
    let mut warp = modes.first_child();
    for _ in 0..3 {
        warp = warp.and_then(|c| c.next_sibling());
    }
    click(&mut native, &warp.expect("the Warp segment"));
    until(
        || state(&w).commands.iter().any(|c| c.id == CommandId::TransformWarp && c.selected)
            && find_named(w.canvas_bar.root.upcast_ref(), "canvas-bar-choice-transform-warp-grid").is_some(),
        "Warp shows its grid choice",
    );
    let mesh = |w: &Workspace| {
        let g = w.gpu.borrow();
        let engine = g.as_ref().unwrap().session.engine();
        engine.transform_preview().unwrap().transform.placement.mesh.clone().unwrap_or_else(|| {
            let selected = engine.document().working.selection.as_ref().unwrap().bounds();
            std::sync::Arc::new(layer_core::MeshMap::identity(selected, layer_core::MeshMap::PRESETS[0]).unwrap())
        })
    };
    let revision = ui_session(&w).engine().document().revision;
    let node = |w: &Workspace, index: u32| {
        let p = mesh(w).node(index).unwrap();
        canvas_point(w, [p.x, p.y])
    };
    let drags: [(u32, [f32; 2], &str); 3] = [(5, [40., 30.], "mouse"), (6, [-30., 25.], "touch"), (1, [10., -45.], "pen")];
    for (index, delta, device) in drags {
        let from = node(&w, index);
        let to = [from[0] + delta[0], from[1] + delta[1]];
        let middle = [from[0] + delta[0] * 0.5, from[1] + delta[1] * 0.5];
        let events = match device {
            "mouse" => json!([
                {"point": from}, {"down": true}, {"wait_ms": 40},
                {"point": middle}, {"wait_ms": 20}, {"point": to}, {"wait_ms": 20}, {"down": false}
            ]),
            "touch" => json!([
                {"touch": "down", "point": from}, {"wait_ms": 40},
                {"touch": "move", "point": middle}, {"wait_ms": 20},
                {"touch": "move", "point": to}, {"wait_ms": 20}, {"touch": "up"}
            ]),
            _ => json!([
                {"pen": "down", "point": from}, {"wait_ms": 40},
                {"pen": "move", "point": middle}, {"wait_ms": 20},
                {"pen": "move", "point": to}, {"wait_ms": 20}, {"pen": "up"}, {"pen": "leave"}
            ]),
        };
        native.perform(events);
        until(
            || {
                let now = node(&w, index);
                (now[0] - to[0]).hypot(now[1] - to[1]) < 3.
            },
            &format!("a {device} drag moves mesh node {index}"),
        );
        until(|| shown(&w), "the bar returns after the drag");
    }
    let dir = artifact_dir("../../artifacts/canvas-action-bar");
    pump(300);
    capture_reference(&w, &format!("{dir}/warp.png"), 1.);
    click(&mut native, &bar_widget(&w, "canvas-bar-ApplyTransform"));
    until(|| !transforming(&w), "Apply ends the warp");
    let document = ui_session(&w).engine().document().revision;
    assert!(document > revision, "Apply commits the warped pixels");
    w.dispatch(UiAction::Invoke { command: CommandId::RectangleSelect });
    w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
    w.dispatch(UiAction::Layer { action: LayerAction::FillSelection });
    pump(300);
    w.dispatch(UiAction::Invoke { command: CommandId::ScaleRotate });
    until(|| kind(&w)==Some(layer_ui::CanvasBarKind::Transform) && shown(&w),"the full-canvas transform opens");
    w.dispatch(UiAction::Invoke { command: CommandId::TransformWarp });
    until(|| state(&w).commands.iter().any(|c|c.id==CommandId::TransformWarp && c.selected)
        && shown(&w),"the full canvas is ready to warp");
    let from = node(&w,5);
    let destination = mesh(&w).node(5).unwrap();
    let to = canvas_point(&w,[destination.x-800.,destination.y-800.]);
    native.perform(json!([
        {"point":from},{"down":true},{"wait_ms":40},
        {"point":[(from[0]+to[0])*0.5,(from[1]+to[1])*0.5]},{"wait_ms":100},
        {"point":to},{"wait_ms":100},{"down":false}
    ]));
    until(|| {
        let now = node(&w,5);
        (now[0]-to[0]).hypot(now[1]-to[1])<3.
    },"the full-canvas node crosses the neighboring patches");
    until(|| shown(&w), "the folded warp survives the drag");
    click(&mut native,&bar_widget(&w,"canvas-bar-ApplyTransform"));
    until(|| !transforming(&w), "the folded full-canvas warp applies");
    assert!(ui_session(&w).engine().document().revision>document);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    w.dispatch(UiAction::Invoke { command: CommandId::Redo });
    pump(300);
}

#[test]
#[ignore = "isolated compositor, GPU and native touch delivery"]
fn native_canvas_bar_finger_moves_a_transform() {
    let app = native_test_app("art.capycanvas.CanvasBarFinger");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    native_pen_path(&w, &[[650., 500.], [1150., 500.], [1150., 850.], [650., 850.], [650., 500.]]);
    w.dispatch(UiAction::Layer { action: LayerAction::FillSelection });
    w.dispatch(UiAction::Invoke { command: CommandId::ScaleRotate });
    until(|| transforming(&w) && shown(&w), "the transform bar appears");
    let mut native = remote_input();
    let anchor = anchor_in_window(&w);
    let inside = [anchor[0] + (anchor[2] - anchor[0]) * 0.35, anchor[1] + (anchor[3] - anchor[1]) * 0.35];
    native.perform(json!([
        {"touch": "down", "point": inside}, {"wait_ms": 40},
        {"touch": "move", "point": [inside[0] + 15., inside[1] + 10.]}, {"wait_ms": 20},
        {"touch": "move", "point": [inside[0] + 30., inside[1] + 20.]}, {"wait_ms": 20},
        {"touch": "up"}
    ]));
    until(
        || {
            let moved = anchor_in_window(&w);
            (moved[0] - anchor[0] - 30.).abs() < 2. && (moved[1] - anchor[1] - 20.).abs() < 2.
        },
        "a finger inside the box moves the transform",
    );
    assert!(transforming(&w), "the finger keeps the transform open");
    until(|| shown(&w), "the bar returns after the finger drag");
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let group = state(&w).workspace.layout.panel_group(Panel::Layers).unwrap();
    w.dispatch(UiAction::MoveGroup { group, target: DockTarget::Float { position: [80., 480.] }, viewport });
    pump(300);
    let panel = w.groups.borrow().iter().find(|g| g.id == group).unwrap().root.clone();
    let from = center(&w, &find_css(panel.upcast_ref(), "panel-grip").unwrap());
    native.perform(json!([
        {"point": from}, {"down": true}, {"wait_ms": 40},
        {"point": [from[0] + 30., from[1] + 20.]}, {"wait_ms": 40}
    ]));
    assert!(w.canvas_bar.visible_bounds().is_none(), "the bar hides while a floating panel moves");
    native.perform(json!([{"point": [from[0] + 60., from[1] + 40.]}, {"down": false}]));
    until(|| shown(&w), "the bar returns after the panel drop");
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Device {
    Mouse,
    Touch,
    Pen,
}

pub(super) fn tap(native: &mut RemoteInput, device: Device, point: [f32; 2]) {
    native.perform(match device {
        Device::Mouse => json!([{"point": point}, {"down": true}, {"wait_ms": 30}, {"down": false}]),
        Device::Touch => json!([{"touch": "down", "point": point}, {"wait_ms": 40}, {"touch": "up"}]),
        Device::Pen => json!([{"pen": "down", "point": point}, {"wait_ms": 40}, {"pen": "up"}, {"pen": "leave"}]),
    });
}

/// The action of the item reached through `labels` in a native menu model.
fn menu_action(model: &gtk::gio::MenuModel, labels: &[&str]) -> Option<String> {
    let (label, rest) = labels.split_first()?;
    (0..model.n_items()).find_map(|i| {
        if let Some(section) = model.item_link(i, "section") {
            return menu_action(&section, labels);
        }
        let text = model.item_attribute_value(i, "label", None)?.get::<String>()?;
        if text != *label {
            return None;
        }
        match model.item_link(i, "submenu") {
            Some(submenu) => menu_action(&submenu, rest),
            None if rest.is_empty() => model.item_attribute_value(i, "action", None)?.get::<String>(),
            None => None,
        }
    })
}

/// Open a bar menu, or its submenu in More when it does not fit, and choose
/// `labels` in turn. The isolated tablet's synthetic serials cannot grab a
/// popup, so a pen opens the menu and its item runs through the menu's action.
pub(super) fn choose_from_bar_menu(w: &Workspace, native: &mut RemoteInput, device: Device, menu: layer_ui::CanvasBarMenu, labels: &[&str]) {
    let button = bar_widget(w, &format!("canvas-bar-menu-{}", menu.id()));
    let menu_label = menu.label(ui_session(w).localization());
    let mut path = labels.to_vec();
    let opener = if button.is_mapped() {
        button
    } else {
        path.insert(0, &menu_label);
        bar_widget(w, "canvas-bar-more")
    };
    tap(native, device, center(w, &opener));
    let popover = until_some(
        || opener.downcast_ref::<gtk::MenuButton>()?.popover()?.downcast::<gtk::PopoverMenu>().ok().filter(|p| p.is_visible()),
        &format!("{device:?} opens {}", menu_label),
    );
    if let Device::Pen = device {
        let action = until_some(|| popover.menu_model().and_then(|m| menu_action(&m, &path)), &path.join(" › "));
        popover.activate_action(&action, None).unwrap();
        return;
    }
    for (index, label) in path.iter().enumerate() {
        let item = until_some(|| mapped_label(w.canvas_bar.root.upcast_ref(), label), label);
        tap(native, device, center(w, &item));
        if index + 1 < path.len() {
            until(|| !item.is_mapped(), &format!("{label} opens its submenu"));
        }
    }
}

pub(super) fn document(w: &Workspace) -> layer_core::Document {
    ui_session(&w).engine().document().clone()
}

/// Fill a lasso selection on `layer` and wait for its bar.
pub(super) fn filled_selection(w: &Rc<Workspace>, layer: u64) {
    w.dispatch(UiAction::SelectLayer { id: layer });
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    native_pen_path(w, &[[650., 500.], [1150., 500.], [1150., 850.], [650., 850.], [650., 500.]]);
    w.dispatch(UiAction::Layer { action: LayerAction::FillSelection });
    until(
        || state(w).canvas_bar.is_some_and(|b| b.context.kind == layer_ui::CanvasBarKind::Selection) && shown(w),
        "the selection bar appears beside the new selection",
    );
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_canvas_bar_selection_menus --tablet"]
fn native_canvas_bar_selection_menus() {
    use layer_ui::CanvasBarMenu;
    let app = native_test_app("art.capycanvas.CanvasBarMenus");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    w.dispatch(UiAction::SetColor { rgba: [0.12, 0.38, 0.72, 1.] });
    let paint = document(&w).working.occurrence.map(layer_ui::occurrence_token).unwrap();
    let curves = {
        let g = w.gpu.borrow();
        let filters = g.as_ref().unwrap().session.application_menu(layer_ui::ApplicationMenu::Filter);
        filters.sections[0]
            .iter()
            .find(|category| category.sections.iter().flatten().any(|i| i.label == "Curves"))
            .expect("Curves has a category")
            .label
            .clone()
    };
    let mut native = remote_input();
    for device in [Device::Mouse, Device::Touch, Device::Pen] {
        filled_selection(&w, paint);
        let layers = document(&w).scene().order().len();
        choose_from_bar_menu(&w, &mut native, device, CanvasBarMenu::CopyToLayer, &["Copy Selection to New Layer"]);
        until(
            || {
                let doc = document(&w);
                doc.scene().order().len() == layers + 1 && doc.working.selection.is_none() && doc.working.occurrence.map(layer_ui::occurrence_token).unwrap() != paint
            },
            &format!("{device:?}: Copy to Layer puts the selection on a new layer"),
        );

        filled_selection(&w, paint);
        let pixels = document(&w).scene().paint_source(layer_ui::occurrence_handle(paint).unwrap()).unwrap().raster.identity();
        choose_from_bar_menu(&w, &mut native, device, CanvasBarMenu::Clear, &["Clear Outside Selection"]);
        until(
            || {
                let doc = document(&w);
                doc.scene().paint_source(layer_ui::occurrence_handle(paint).unwrap()).unwrap().raster.identity() != pixels && doc.working.selection.is_some()
            },
            &format!("{device:?}: Clear Outside erases around the kept selection"),
        );

        filled_selection(&w, paint);
        choose_from_bar_menu(&w, &mut native, device, CanvasBarMenu::Adjust, &[&curves, "Curves"]);
        until(
            || {
                let doc = document(&w);
                let effect = doc.scene().occurrence(doc.working.occurrence.unwrap()).unwrap();
                effect.kind() == layer_core::LayerKind::Effect && effect.mask.is_some() && doc.working.selection.is_none()
            },
            &format!("{device:?}: Adjust › Curves masks a new Curves layer to the selection"),
        );
    }
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse and keyboard delivery"]
fn native_delete_clears_pixels_unless_a_guide_is_selected() {
    let app = native_test_app("art.capycanvas.DeleteKey");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    let paint = document(&w).working.occurrence.unwrap();
    filled_selection(&w, layer_ui::occurrence_token(paint));
    let mut native = remote_input();
    let pixels = |w: &Workspace| document(w).scene().paint_source(paint).unwrap().raster.identity();
    let filled = pixels(&w);
    native.key(0xffff);
    until(|| pixels(&w) != filled, "Delete clears the selected pixels");
    assert!(document(&w).working.selection.is_some(), "clearing keeps the selection");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| pixels(&w) == filled, "one undo step restores them");

    w.dispatch(UiAction::Invoke { command: CommandId::Ruler });
    let [from, to] = [canvas_point(&w, [300., 300.]), canvas_point(&w, [500., 380.])];
    native.perform(json!([
        {"point": from}, {"down": true}, {"wait_ms": 40},
        {"point": [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5]}, {"wait_ms": 20},
        {"point": to}, {"wait_ms": 20}, {"down": false}
    ]));
    until(|| document(&w).artwork.guides.len() == 1, "the drag draws a guide");
    native.key(0xffff);
    until(|| document(&w).artwork.guides.is_empty(), "Delete removes the selected guide under the Ruler tool");
    pump(200);
    assert_eq!(pixels(&w), filled, "and leaves the pixels alone");
}

fn bar_kind(w: &Workspace) -> Option<layer_ui::CanvasBarKind> {
    state(w).canvas_bar.map(|b| b.context.kind)
}

/// Tap a bar button once it is shown and enabled, then wait for `done`.
fn tap_bar(w: &Workspace, native: &mut RemoteInput, device: Device, command: CommandId, done: impl Fn() -> bool, message: &str) {
    let name = format!("canvas-bar-{command:?}");
    let button = until_some(|| find_named(w.canvas_bar.root.upcast_ref(), &name).filter(|b| b.is_mapped() && b.is_sensitive()), &name);
    tap(native, device, center(w, &button));
    until(done, &format!("{device:?}: {message}"));
}

/// The mode bar's label is shown and the bar sits on the bottom edge of the canvas.
fn assert_mode_bar(w: &Workspace, kind: layer_ui::CanvasBarKind, label: &str) {
    until(|| bar_kind(w) == Some(kind) && shown(w), &format!("the {kind:?} bar appears"));
    let text = find_css(w.canvas_bar.root.upcast_ref(), "canvas-action-bar-label")
        .and_downcast::<gtk::Label>()
        .expect("the bar label");
    until(|| text.is_mapped() && text.text() == label, &format!("the bar reads {label}"));
    let bar = w.canvas_bar.root.compute_bounds(&w.window).unwrap();
    let area = w.area.compute_bounds(&w.window).unwrap();
    assert!(
        bar.y() + bar.height() > area.y() + area.height() - 80.,
        "{kind:?} sits on the bottom edge: {bar:?} {area:?}"
    );
}

/// Mode bars with Mutter's mouse and touch. The tablet proxy loses its
/// Wayland connection when Quick Mask or Selection Layer rows change, so the
/// guide journey covers the pen.
#[test]
#[ignore = "isolated compositor, GPU and native mouse and touch delivery"]
fn native_canvas_bar_modes() {
    let app = native_test_app("art.capycanvas.CanvasBarModes");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    let mut native = remote_input();
    let paint = document(&w).working.occurrence.unwrap();
    for device in [Device::Mouse, Device::Touch] {
        w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
        w.dispatch(UiAction::Invoke { command: CommandId::QuickMask });
        assert_mode_bar(&w, layer_ui::CanvasBarKind::QuickMask, "Quick Mask");
        let inverted = || document(&w).working.selection.as_ref().is_some_and(|s| s.inverted);
        tap_bar(&w, &mut native, device, CommandId::InvertSelection, inverted, "Invert inverts the Quick Mask");
        assert_eq!(bar_kind(&w), Some(layer_ui::CanvasBarKind::QuickMask), "Invert stays in Quick Mask");
        tap_bar(&w, &mut native, device, CommandId::ReturnToArtwork, || !state(&w).layer_tools.quick_mask, "Exit leaves Quick Mask");

        w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
        w.dispatch(UiAction::Invoke { command: CommandId::SaveSelectionLayer });
        let saved = document(&w).working.occurrence.unwrap();
        let name = document(&w).scene().occurrence(saved).unwrap().name.to_string();
        assert_mode_bar(&w, layer_ui::CanvasBarKind::SelectionLayer, &format!("Editing {name}"));
        let stored = document(&w).saved_selection(saved).unwrap();
        tap_bar(
            &w,
            &mut native,
            device,
            CommandId::InvertSelectionLayer,
            || document(&w).saved_selection(saved).is_ok_and(|s| s.inverted != stored.inverted),
            "Invert inverts the stored coverage",
        );
        assert_eq!(document(&w).working.occurrence.unwrap(), saved, "Invert stays on the Selection Layer");
        tap_bar(
            &w,
            &mut native,
            device,
            CommandId::ReturnToArtwork,
            || document(&w).working.occurrence.unwrap() == paint && bar_kind(&w) != Some(layer_ui::CanvasBarKind::SelectionLayer),
            "Return to Artwork leaves Selection Layer editing",
        );

        w.dispatch(UiAction::Invoke { command: CommandId::MaskSelection });
        let name = document(&w).scene().occurrence(paint).unwrap().name.to_string();
        assert_mode_bar(&w, layer_ui::CanvasBarKind::LayerMask, &format!("Editing {name} mask"));
        let enabled = || document(&w).scene().occurrence(paint).unwrap().mask.as_ref().is_some_and(|m| m.enabled);
        tap_bar(&w, &mut native, device, CommandId::LayerMaskEnabled, || !enabled(), "Disable turns the mask off");
        let offers_enable = || {
            find_named(w.canvas_bar.root.upcast_ref(), "canvas-bar-LayerMaskEnabled")
                .and_then(|b| mapped_label(&b, "Enable"))
                .is_some()
        };
        until(offers_enable, "the button now offers Enable");
        tap_bar(
            &w,
            &mut native,
            device,
            CommandId::EditLayerContent,
            || !document(&w).working.target.is_some_and(|t| t.is_coverage()) && state(&w).canvas_bar.is_none(),
            "Edit Content leaves mask editing",
        );
        w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::DeleteMask { id: layer_ui::occurrence_token(paint) } });
        pump(100);
    }

    w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
    w.dispatch(UiAction::Invoke { command: CommandId::MaskSelection });
    assert_mode_bar(&w, layer_ui::CanvasBarKind::LayerMask, &format!("Editing {} mask", document(&w).scene().occurrence(paint).unwrap().name));
    w.dispatch(UiAction::Invoke { command: CommandId::Move });
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Lock { id: layer_ui::occurrence_token(paint), value: true } });
    let area = w.area.compute_bounds(&w.window).unwrap();
    native.click([area.x() + area.width() * 0.5, area.y() + area.height() * 0.4]);
    until(|| w.notice.root.is_visible() && state(&w).notice.is_some(), "Move on the locked mask shows a notice");
    until(|| shown(&w), "the mode bar stays through the contact");
    let notice = w.notice.root.compute_bounds(&w.window).unwrap();
    let bar = w.canvas_bar.root.compute_bounds(&w.window).unwrap();
    assert!(notice.y() + notice.height() <= bar.y(), "the notice sits above the bottom-edge bar: {notice:?} {bar:?}");
    let dir = artifact_dir("../../artifacts/canvas-action-bar");
    capture_reference(&w, &format!("{dir}/mask-mode-notice.png"), 1.);
    native.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_canvas_bar_guide --tablet"]
fn native_canvas_bar_guide() {
    let app = native_test_app("art.capycanvas.CanvasBarGuide");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    w.dispatch(UiAction::Invoke { command: CommandId::Ruler });
    let mut native = remote_input();
    for device in [Device::Mouse, Device::Touch, Device::Pen] {
        let [from, to] = [canvas_point(&w, [300., 300.]), canvas_point(&w, [500., 380.])];
        native.perform(json!([
            {"point": from}, {"down": true}, {"wait_ms": 40},
            {"point": [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5]}, {"wait_ms": 20},
            {"point": to}, {"wait_ms": 20}, {"down": false}
        ]));
        until(|| document(&w).artwork.guides.len() == 1, "the drag draws a guide");
        until(|| bar_kind(&w) == Some(layer_ui::CanvasBarKind::Guide) && shown(&w), "the guide bar appears");
        let bar = w.canvas_bar.root.compute_bounds(&w.window).unwrap();
        let lowest = from[1].max(to[1]);
        assert!(bar.y() > lowest + 12., "the bar clears the guide's handles: {bar:?} {from:?} {to:?}");
        tap_bar(&w, &mut native, device, CommandId::DeleteRuler, || document(&w).artwork.guides.is_empty(), "Delete removes the guide");
        until(|| bar_kind(&w).is_none(), "the bar leaves with the guide");
    }
    native.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_crop_on_the_selection_bar_then_canvas_size_shows_the_hidden_pixels() {
    use super::photo_edit::{apply_canvas_size, canvas_size_number, shown as pixel};
    let app = native_test_app("art.capycanvas.CropToSelection");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    w.dispatch(UiAction::SetColor { rgba: [0.1, 0.3, 0.8, 1.] });
    let before = document(&w);
    let [width, height] = [before.composition().size[0] as f32, before.composition().size[1] as f32];
    let paint = before.working.occurrence.unwrap();
    let rectangle = |[x0, y0, x1, y1]: [f32; 4]| [[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]];
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    native_pen_path(&w, &rectangle([width * 0.2, height * 0.2, width * 0.8, height * 0.8]));
    w.dispatch(UiAction::Layer { action: LayerAction::FillSelection });
    until(|| !document(&w).scene().paint_source(paint).unwrap().raster.is_empty(), "the fill paints the selection");
    let filled = document(&w).scene().paint_source(paint).unwrap().raster.clone();
    native_pen_path(&w, &rectangle([width * 0.4, height * 0.4, width * 0.6, height * 0.6]));
    until(|| state(&w).canvas_bar.is_some_and(|b| b.context.kind == layer_ui::CanvasBarKind::Selection) && shown(&w), "the selection bar");
    let mut native = remote_input();
    press_bar_command(&w, &mut native, "canvas-bar-CropCanvasToSelection", "Crop Canvas to Selection");
    until(|| document(&w).composition().size[0] < before.composition().size[0], "Crop on the selection bar crops the canvas");
    let cropped = document(&w);
    assert!((cropped.composition().size[0] as f32 - width * 0.2).abs() <= 2. && (cropped.composition().size[1] as f32 - height * 0.2).abs() <= 2.);
    assert!(cropped.scene().paint_source(paint).unwrap().raster == filled, "a crop changes only metadata");
    w.dispatch(UiAction::Invoke { command: CommandId::CanvasSize });
    until(|| state(&w).layer_tools.canvas_size.is_some(), "Canvas Size opens");
    for (axis, value) in [("width", before.composition().size[0]), ("height", before.composition().size[1])] {
        edit_number(&canvas_size_number(&w, axis), &value.to_string());
    }
    apply_canvas_size(&w, &mut native);
    let grown = document(&w);
    assert_eq!([grown.composition().size[0], grown.composition().size[1]], [before.composition().size[0], before.composition().size[1]]);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(400);
    let hidden = pixel(&w, [width * 0.3, height * 0.5]);
    assert!(hidden[2] > 150 && hidden[0] < 100, "the pixels hidden by the crop show again: {hidden:?}");
    let paper = pixel(&w, [width * 0.1, height * 0.1]);
    assert!(paper.iter().take(3).all(|v| *v > 200), "outside the fill the paper shows: {paper:?}");
    native.finish();
    w.window.close();
    pump(50);
}

fn drag(native: &mut RemoteInput, device: Device, from: [f32; 2], to: [f32; 2]) {
    let middle = [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5];
    native.perform(match device {
        Device::Mouse => json!([
            {"point": from}, {"down": true}, {"wait_ms": 40},
            {"point": middle}, {"wait_ms": 20}, {"point": to}, {"wait_ms": 20}, {"down": false}
        ]),
        Device::Touch => json!([
            {"touch": "down", "point": from}, {"wait_ms": 40},
            {"touch": "move", "point": middle}, {"wait_ms": 20},
            {"touch": "move", "point": to}, {"wait_ms": 20}, {"touch": "up"}
        ]),
        Device::Pen => json!([
            {"pen": "down", "point": from}, {"wait_ms": 40},
            {"pen": "move", "point": middle}, {"wait_ms": 20},
            {"pen": "move", "point": to}, {"wait_ms": 20}, {"pen": "up"}, {"pen": "leave"}
        ]),
    });
}

fn soft_edged(selection: &layer_core::Selection) -> bool {
    matches!(&selection.shape, layer_core::SelectionShape::Pixels(p) if p.coverage_format() == 2
        && p.words().iter().any(|w| (0..4).any(|i| !matches!((w >> (8 * i)) & 255, 0 | 255))))
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_canvas_bar_refine --tablet"]
fn native_canvas_bar_refine() {
    use layer_ui::CanvasBarMenu;
    let app = native_test_app("art.capycanvas.CanvasBarRefine");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    w.dispatch(UiAction::SetColor { rgba: [0.12, 0.38, 0.72, 1.] });
    let paint = document(&w).working.occurrence.unwrap();
    let selection = |w: &Workspace| document(w).working.selection;
    let dir = artifact_dir("../../artifacts/canvas-action-bar");
    let mut native = remote_input();
    for device in [Device::Mouse, Device::Touch, Device::Pen] {
        filled_selection(&w, layer_ui::occurrence_token(paint));
        let hard = selection(&w).unwrap();
        choose_from_bar_menu(&w, &mut native, device, CanvasBarMenu::Refine, &["Feather…"]);
        let field = until_some(
            || find_named(w.window.upcast_ref(), "selection-refine-value").filter(|f| f.is_mapped()),
            &format!("{device:?}: Feather… opens the Refine dialog"),
        );
        assert!(mapped_label(&field, "Feather radius").is_some(), "the dialog names the value");
        assert!(w.window.has_css_class(crate::preview_dialog::PreviewDialog::PREVIEW_CLASS), "the canvas stays undimmed");
        until(|| selection(&w).is_some_and(|s| soft_edged(&s)), "the default radius previews live");
        let previewed = selection(&w);
        let slider = find_css(&field, "number-track").and_then(|t| descendant::<gtk::Scale>(&t)).expect("the value slider");
        let track = until_some(
            || {
                let before = slider.compute_bounds(&w.window)?;
                pump(120);
                slider.compute_bounds(&w.window).filter(|after| after == &before)
            },
            "the dialog finishes sliding in",
        );
        let along = |f: f32| [track.x() + track.width() * f, track.y() + track.height() * 0.5];
        drag(&mut native, device, along(0.2), along(0.15));
        until(
            || state(&w).layer_tools.selection_resize.is_some_and(|v| v.radius > 8.),
            &format!("{device:?}: dragging the slider changes the radius"),
        );
        until(
            || {
                let now = selection(&w);
                now != previewed && now.is_some_and(|s| soft_edged(&s))
            },
            "the new radius previews without Apply",
        );
        if let Device::Mouse = device {
            pump(300);
            capture_reference(&w, &format!("{dir}/refine-feather.png"), 1.);
        }
        let apply = until_some(|| mapped_label(w.window.upcast_ref(), "Apply"), "the dialog's Apply");
        tap(&mut native, device, center(&w, &apply));
        until(
            || state(&w).layer_tools.selection_resize.is_none() && find_named(w.window.upcast_ref(), "selection-refine-value").is_none_or(|f| !f.is_mapped()),
            &format!("{device:?}: Apply closes the dialog"),
        );
        assert!(!w.window.has_css_class(crate::preview_dialog::PreviewDialog::PREVIEW_CLASS));
        let feathered = selection(&w).unwrap();
        assert!(soft_edged(&feathered));
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        until(|| selection(&w).as_ref() == Some(&hard), &format!("{device:?}: one Undo restores the hard edge"));
        w.dispatch(UiAction::Invoke { command: CommandId::Redo });
        until(|| selection(&w).as_ref() == Some(&feathered), "Redo feathers it again");

        filled_selection(&w, layer_ui::occurrence_token(paint));
        let (outline, pixels) = {
            let doc = document(&w);
            (doc.working.selection.clone().unwrap(), doc.scene().paint_source(paint).unwrap().raster.identity())
        };
        choose_from_bar_menu(&w, &mut native, device, CanvasBarMenu::Refine, &["Transform Outline"]);
        until(
            || {
                state(&w).canvas_bar.is_some_and(|b| {
                    b.context.kind == layer_ui::CanvasBarKind::Transform && b.label.as_deref() == Some("Transform Outline")
                }) && shown(&w)
            },
            &format!("{device:?}: Transform Outline opens its bar"),
        );
        assert!(find_named(w.canvas_bar.root.upcast_ref(), "canvas-bar-choice-transform-interpolation").is_none());
        let anchor = anchor_in_window(&w);
        let handle = [anchor[2], (anchor[1] + anchor[3]) * 0.5];
        drag(&mut native, device, handle, [handle[0] + 80., handle[1]]);
        until(
            || (anchor_in_window(&w)[2] - anchor[2] - 80.).abs() < 3.,
            &format!("{device:?}: dragging the edge handle scales the outline"),
        );
        assert_eq!(selection(&w).as_ref(), Some(&outline), "only Apply edits the selection");
        until(|| shown(&w), "the bar returns after the drag");
        if let Device::Mouse = device {
            pump(300);
            capture_reference(&w, &format!("{dir}/transform-outline.png"), 1.);
        }
        tap(&mut native, device, center(&w, &bar_widget(&w, "canvas-bar-ApplyTransform")));
        until(|| !transforming(&w) && selection(&w).as_ref() != Some(&outline), "Apply moves the outline");
        let doc = document(&w);
        let applied = doc.working.selection.clone().unwrap();
        assert_eq!(applied.shape, outline.shape, "the outline keeps its shape");
        assert!(applied.bounds().max.x > outline.bounds().max.x);
        assert_eq!(doc.scene().paint_source(paint).unwrap().raster.identity(), pixels, "{device:?}: the pixels stay where they are");
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        until(|| selection(&w).as_ref() == Some(&outline), "one Undo restores the outline");
    }
}

/// The document pixel at `p`, as the readback's bytes.
fn document_pixel(image: &layer_render::ReadbackImage, p: [f32; 2]) -> [u8; 4] {
    assert!(p[0] >= 0. && p[1] >= 0. && p[0] < image.width as f32 && p[1] < image.height as f32,
        "pixel {p:?} outside {}×{} document", image.width, image.height);
    let at = p[1] as usize * image.stride as usize + p[0] as usize * 4;
    image.bytes[at..at + 4].try_into().unwrap()
}

/// Journey 26: select, then drag the selected pixels with Move, with and
/// without Leave Copy, and with Alt held at the press.
#[test]
#[ignore = "isolated native-input.js --native-test=native_move_drags_selected_pixels --tablet"]
fn native_move_drags_selected_pixels() {
    let app = native_test_app("art.capycanvas.MoveSelection");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    w.dispatch(UiAction::SetColor { rgba: [0.12, 0.38, 0.72, 1.] });
    let paint = document(&w).working.occurrence.unwrap();
    let context = glib::MainContext::default();
    let mut readback = 9700;
    let mut pixels = |w: &Rc<Workspace>| {
        readback += 1;
        context.block_on(read_canvas_pixels(w, readback)).unwrap()
    };
    let mut native = remote_input();
    let runs = [Device::Mouse, Device::Touch, Device::Pen]
        .into_iter()
        .flat_map(|device| [(device, false, false), (device, true, false)])
        .chain([(Device::Mouse, false, true)]);
    for (device, leave_copy, alt) in runs {
        filled_selection(&w, layer_ui::occurrence_token(paint));
        w.dispatch(UiAction::Invoke { command: CommandId::Move });
        until(
            || {
                let offered = state(&w).canvas_bar.is_some_and(|b| {
                    b.items.iter().any(|i| matches!(&i.option, layer_ui::ToolOption::Action { state, .. } if state.id == CommandId::MoveLeaveCopy))
                });
                offered && shown(&w)
            },
            "Move offers Leave Copy on the selection bar",
        );
        pump(300);
        let leave = |w: &Workspace| state(w).commands.iter().any(|c| c.id == CommandId::MoveLeaveCopy && c.selected);
        if leave(&w) != leave_copy {
            tap_bar(&w, &mut native, device, CommandId::MoveLeaveCopy, || leave(&w) == leave_copy, "Leave Copy toggles on the bar");
        }
        if let (Device::Mouse, true) = (device, leave_copy) {
            let dir = artifact_dir("../../artifacts/move-selection");
            for theme in [layer_ui::Theme::Light, layer_ui::Theme::Dark] {
                w.dispatch(UiAction::SetTheme { theme: Some(theme) });
                pump(300);
                capture_reference(&w, &format!("{dir}/leave-copy-{theme:?}.png"), 1.);
            }
        }
        let selection = document(&w).working.selection.clone().unwrap();
        let bounds = selection.coverage_bounds();
        let kept = [bounds.min.x + 6., bounds.min.y + 6.];
        let original = pixels(&w);
        let center = [(bounds.min.x + bounds.max.x) * 0.5, (bounds.min.y + bounds.max.y) * 0.5];
        let from = canvas_point(&w, center);
        let to = canvas_point(&w, [center[0] + 400., center[1] + 300.]);
        if alt {
            native.perform(json!([{"key": 0xffe9, "down": true}]));
        }
        drag(&mut native, device, from, to);
        if alt {
            native.perform(json!([{"key": 0xffe9, "down": false}]));
        }
        let moved = until_some(
            || {
                let doc = document(&w);
                let placed = doc.working.selection.as_ref()?.affine;
                (placed != selection.affine).then_some(placed)
            },
            &format!("{device:?}: the selection follows the dragged pixels"),
        );
        let [_, _, _, _, dx, dy] = moved.0;
        assert!(dx > 50. && dy > 30. && dx.fract() == 0. && dy.fract() == 0., "{device:?}: whole pixels, {dx} {dy}");
        assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::Move, "Move stays the tool");
        until(|| bar_kind(&w) == Some(layer_ui::CanvasBarKind::Selection) && shown(&w), "the selection bar returns beside it");
        let after = pixels(&w);
        let copy = [kept[0] + dx, kept[1] + dy];
        assert_eq!(document_pixel(&after, copy), document_pixel(&original, kept), "{device:?}: the pixels arrive");
        let keeps = leave_copy != alt;
        assert_eq!(
            document_pixel(&after, kept) == document_pixel(&original, kept),
            keeps,
            "{device:?} Leave Copy {leave_copy} Alt {alt}: the original stays only with a copy"
        );
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        until(|| document(&w).working.selection.as_ref() == Some(&selection), &format!("{device:?}: one Undo restores the selection"));
        let undone = pixels(&w);
        for p in [kept, copy] {
            assert_eq!(document_pixel(&undone, p), document_pixel(&original, p), "{device:?}: and the pixels");
        }
    }
}
