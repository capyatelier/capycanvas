//! Canvas action bar journeys with real Mutter mouse and touch delivery.
use super::*;
use serde_json::json;

fn remote_input() -> RemoteInput {
    let input = RemoteInput::new().settle_ms(150).timeout_secs(30);
    input.ready();
    pump(300);
    input
}

fn until_some<T>(mut find: impl FnMut() -> Option<T>, message: &str) -> T {
    let mut found = None;
    until(|| {
        found = find();
        found.is_some()
    }, message);
    found.unwrap()
}

fn center(w: &Workspace, widget: &gtk::Widget) -> [f32; 2] {
    let b = widget.compute_bounds(&w.window).expect("mapped widget");
    [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]
}

fn bar_widget(w: &Workspace, name: &str) -> gtk::Widget {
    find_named(w.canvas_bar.root.upcast_ref(), name).unwrap_or_else(|| panic!("{name}"))
}

/// Document bounds of the transform anchor in window coordinates.
fn anchor_in_window(w: &Workspace) -> [f32; 4] {
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

fn shown(w: &Workspace) -> bool {
    w.canvas_bar.root.is_mapped() && w.canvas_bar.visible_bounds().is_some()
}

fn mapped_label(root: &gtk::Widget, text: &str) -> Option<gtk::Widget> {
    if root.is_mapped() && root.downcast_ref::<gtk::Label>().is_some_and(|l| l.text() == text) {
        return Some(root.clone());
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Some(found) = mapped_label(&widget, text) {
            return Some(found);
        }
    }
    None
}

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
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
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
    let transform = bar_widget(&w, "canvas-bar-ScaleRotate");
    native.click(center(&w, &transform));
    until(
        || kind(&w) == Some(layer_ui::CanvasBarKind::Transform) && shown(&w),
        "Transform on the selection bar opens the transform bar",
    );
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
    let dir = "../../artifacts/canvas-action-bar";
    std::fs::create_dir_all(dir).unwrap();
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
        native.click(center(&w, widget));
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
    let inside = [(anchor[0] + anchor[2]) * 0.5, (anchor[1] + anchor[3]) * 0.5];
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

fn canvas_point(w: &Workspace, document: [f32; 2]) -> [f32; 2] {
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
    until(|| w.gpu.borrow().as_ref().unwrap().session.state().canvas_bar.as_ref().is_some_and(|b| {
        b.completion.iter().any(|i| matches!(&i.option, ToolOption::Action { state, .. } if state.id == CommandId::CompleteSelection && !state.enabled))
    }), "removing a point disables Finish");
    click(&mut native, canvas_point(&w, [700., 950.]));
    let finish = bar_widget(&w, "canvas-bar-CompleteSelection");
    until(|| finish.is_sensitive(), "Finish is available with three points");
    native.click(center(&w, &finish));
    until(
        || w.gpu.borrow().as_ref().unwrap().session.engine().document().selection.is_some(),
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
        w.gpu.borrow().as_ref().unwrap().session.engine().document().selection.as_ref()
            .is_some_and(|s| matches!(s.shape, layer_core::SelectionShape::Pixels(_)))
    };
    let mut native = remote_input();
    native.perform(json!([{"point": [900., 675.]}, {"down": true}, {"wait_ms": 30}, {"down": false}]));
    until(|| pixels(&w), "Color Select makes a pixel selection");
    w.dispatch(UiAction::Invoke { command: CommandId::ScaleRotate });
    w.dispatch(UiAction::Invoke { command: CommandId::TransformDistort });
    until(|| shown(&w) && transforming(&w), "the transform bar appears");
    let revision = w.gpu.borrow().as_ref().unwrap().session.engine().document().revision;
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
    let document = w.gpu.borrow().as_ref().unwrap().session.engine().document().clone();
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
    click(&mut native, &bar_widget(&w, "canvas-bar-ScaleRotate"));
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
        match &g.as_ref().unwrap().session.engine().transform_preview().unwrap().transform.map {
            layer_core::TransformMap::Mesh(mesh) => mesh.clone(),
            other => panic!("Warp previews a mesh, not {other:?}"),
        }
    };
    let revision = w.gpu.borrow().as_ref().unwrap().session.engine().document().revision;
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
    let dir = "../../artifacts/canvas-action-bar";
    std::fs::create_dir_all(dir).unwrap();
    pump(300);
    capture_reference(&w, &format!("{dir}/warp.png"), 1.);
    click(&mut native, &bar_widget(&w, "canvas-bar-ApplyTransform"));
    until(|| !transforming(&w), "Apply ends the warp");
    let document = w.gpu.borrow().as_ref().unwrap().session.engine().document().revision;
    assert!(document > revision, "Apply commits the warped pixels");
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
    let inside = [(anchor[0] + anchor[2]) * 0.5, (anchor[1] + anchor[3]) * 0.5];
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

