//! Canvas action bar journeys with real Mutter mouse and touch delivery.
use super::*;
use serde_json::{Value, json};
use std::path::PathBuf;

struct Native {
    dir: PathBuf,
    step: usize,
}
impl Native {
    fn start() -> Self {
        let dir = PathBuf::from(std::env::var_os("LAYER_NATIVE_INPUT_DIR").unwrap());
        std::fs::write(dir.join("ready"), b"ready").unwrap();
        pump(300);
        Self { dir, step: 0 }
    }
    fn events(&mut self, events: Value) {
        std::fs::write(
            self.dir.join(format!("step-{}.json", self.step)),
            serde_json::to_vec(&events).unwrap(),
        )
        .unwrap();
        until(
            || self.dir.join(format!("done-{}", self.step)).exists(),
            "native input acknowledgement",
        );
        self.step += 1;
        pump(150);
    }
}

fn until(mut predicate: impl FnMut() -> bool, message: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !predicate() {
        assert!(Instant::now() < deadline, "{message}");
        pump(10);
    }
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
    let mut native = Native::start();
    let transform = bar_widget(&w, "canvas-bar-ScaleRotate");
    native.events(json!([{"point": center(&w, &transform)}, {"down": true}, {"down": false}]));
    until(
        || kind(&w) == Some(layer_ui::CanvasBarKind::Transform) && shown(&w),
        "Transform on the selection bar opens the transform bar",
    );
    let bar = w.canvas_bar.root.compute_bounds(&w.window).unwrap();
    let anchor = anchor_in_window(&w);
    assert!(bar.y() > anchor[3], "the bar sits below the transform box");
    assert!(
        (bar.x() + bar.width() * 0.5 - (anchor[0] + anchor[2]) * 0.5).abs() < 2.,
        "the bar is centred on the transform box"
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
    native.events(json!([{"point": center(&w, &segment(1))}, {"down": true}, {"down": false}]));
    until(|| aspect(&w), "a mouse click on Uniform keeps proportions");
    let point = center(&w, &segment(0));
    native.events(json!([
        {"touch": "down", "point": point}, {"wait_ms": 40}, {"touch": "up"}
    ]));
    until(|| !aspect(&w), "a finger tap on Free releases them");
    assert!(transforming(&w));
    assert_eq!(revision(), before, "bar taps never paint or commit");
    let perspective = |w: &Workspace| find_named(w.canvas_bar.root.upcast_ref(), "canvas-bar-TransformPerspective").is_some();
    native.events(json!([{"point": center(&w, &segment(2))}, {"down": true}, {"down": false}]));
    until(|| perspective(&w), "Distort offers Perspective");
    let corner = [anchor[2], anchor[3]];
    native.events(json!([
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
    assert!(!bar_widget(&w, "canvas-bar-ResetTransform").is_mapped(), "Reset overflows while distorting");
    native.events(json!([{"point": center(&w, &bar_widget(&w, "canvas-bar-more"))}, {"down": true}, {"down": false}]));
    until(|| w.canvas_bar.menu_open(), "More opens with the overflowed items");
    let reset = mapped_label(w.canvas_bar.root.upcast_ref(), "Reset transform").expect("More lists Reset");
    native.events(json!([{"point": center(&w, &reset)}, {"down": true}, {"down": false}]));
    until(
        || !perspective(&w) && anchor_in_window(&w).iter().zip(anchor).all(|(a, b)| (a - b).abs() < 1.),
        "Reset returns to Free and the starting box",
    );
    assert_eq!(revision(), before, "distorting and resetting never commit");
    let inside = [(anchor[0] + anchor[2]) * 0.5, (anchor[1] + anchor[3]) * 0.5];
    native.events(json!([
        {"point": inside}, {"down": true}, {"wait_ms": 40},
        {"point": [inside[0] + 40., inside[1] + 20.]}
    ]));
    assert!(w.canvas_bar.visible_bounds().is_none(), "the bar hides during a canvas drag");
    native.events(json!([{"down": false}]));
    until(|| w.canvas_bar.visible_bounds().is_some(), "the bar returns after the drag");
    let moved = w.canvas_bar.root.compute_bounds(&w.window).unwrap();
    assert!((moved.x() - bar.x() - 40.).abs() < 3., "the bar follows the moved box");
    let more = bar_widget(&w, "canvas-bar-more");
    native.events(json!([{"point": center(&w, &more)}, {"down": true}, {"down": false}]));
    until(|| w.canvas_bar.menu_open(), "More opens its menu");
    native.events(json!([{"key": 0xff1b, "down": true}, {"key": 0xff1b, "down": false}]));
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
    native.events(json!([{"point": center(&w, &apply)}, {"down": true}, {"down": false}]));
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
    native.events(json!([{"point": center(&w, &cancel)}, {"down": true}, {"down": false}]));
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
    let mut native = Native::start();
    let click = |native: &mut Native, point: [f32; 2]| {
        native.events(json!([{"point": point}, {"down": true}, {"wait_ms": 30}, {"down": false}]));
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
    native.events(json!([{"point": center(&w, &remove)}, {"down": true}, {"down": false}]));
    until(|| w.gpu.borrow().as_ref().unwrap().session.state().canvas_bar.as_ref().is_some_and(|b| {
        b.completion.iter().any(|i| matches!(&i.option, ToolOption::Action { state, .. } if state.id == CommandId::CompleteSelection && !state.enabled))
    }), "removing a point disables Finish");
    click(&mut native, canvas_point(&w, [700., 950.]));
    let finish = bar_widget(&w, "canvas-bar-CompleteSelection");
    until(|| finish.is_sensitive(), "Finish is available with three points");
    native.events(json!([{"point": center(&w, &finish)}, {"down": true}, {"down": false}]));
    until(
        || w.gpu.borrow().as_ref().unwrap().session.engine().document().selection.is_some(),
        "Finish creates the selection",
    );
    until(
        || state(&w).canvas_bar.is_some_and(|b| b.context.kind == layer_ui::CanvasBarKind::Selection),
        "the finished polygon hands the bar to its selection",
    );
}
