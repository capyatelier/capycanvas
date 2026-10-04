//! Canvas notices with actual Mutter mouse delivery: refusals raised by canvas
//! gestures, their timeout, the next canvas contact and the notice action.
use super::*;

fn canvas_center(w: &Workspace) -> [f32; 2] {
    let (width, height) = {
        let gpu = w.gpu.borrow();
        let doc = gpu.as_ref().unwrap().session.engine().document();
        (doc.composition().size[0] as f32, doc.composition().size[1] as f32)
    };
    let m = state(w).camera.document_to_surface();
    let scale = w.area.scale_factor() as f32;
    let [x, y] = [width * 0.5, height * 0.5];
    let p = gtk::graphene::Point::new(
        (m[0] * x + m[2] * y + m[4]) / scale,
        (m[1] * x + m[3] * y + m[5]) / scale,
    );
    let p = w.area.compute_point(&w.window, &p).unwrap();
    [p.x(), p.y()]
}

fn notice_text(w: &Workspace) -> Option<String> {
    w.notice
        .root
        .is_visible()
        .then(|| w.notice.root.first_child().and_downcast::<gtk::Label>().unwrap().text().to_string())
}

fn notice_action(w: &Workspace) -> gtk::Button {
    find_named(w.notice.root.upcast_ref(), "canvas-notice-action")
        .and_downcast::<gtk::Button>()
        .unwrap()
}

fn start(id: &str) -> (NativeTestApp, Rc<Workspace>, RemoteInput) {
    let app = native_test_app(id);
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(200);
    let input = RemoteInput::new().settle_ms(150);
    input.ready();
    pump(300);
    (app, w, input)
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_notice_move_on_locked_layer() {
    let (_app, w, mut input) = start("art.capycanvas.NoticeMove");
    let lock = |value| UiAction::Layer { action: layer_ui::LayerAction::Lock { id: 1, value } };
    w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
    w.dispatch(UiAction::Invoke { command: CommandId::Move });
    w.dispatch(lock(true));
    let fill = || {
        find_named(w.canvas_bar.root.upcast_ref(), "canvas-bar-FillSelection")
            .and_then(|b| b.tooltip_text())
            .map(|t| t.to_string())
    };
    until(
        || fill().as_deref() == Some("The active layer is locked"),
        "a disabled bar item shows its reason as the tooltip",
    );
    let point = canvas_center(&w);
    input.click(point);
    until(
        || notice_text(&w).as_deref() == Some("The active layer is locked"),
        "Move on a locked layer shows the notice",
    );
    assert!(!notice_action(&w).is_visible(), "the refusal has no action");
    assert!(!w.status.is_visible(), "refusals are not host errors");
    let first = state(&w).notice.unwrap().id;
    input.click(point);
    until(
        || state(&w).notice.is_some_and(|n| n.id > first) && w.notice.root.is_visible(),
        "a repeated refusal shows again",
    );
    w.dispatch(lock(false));
    input.click(point);
    until(
        || !w.notice.root.is_visible() && state(&w).notice.is_none(),
        "the next canvas contact dismisses the notice",
    );
    w.dispatch(lock(true));
    input.click(point);
    until(|| w.notice.root.is_visible(), "the refusal shows once more");
    let shown = Instant::now();
    until(|| !w.notice.root.is_visible(), "the notice times out");
    assert!(shown.elapsed() > Duration::from_secs(3), "{:?}", shown.elapsed());
    until(|| state(&w).notice.is_none(), "the timeout dismisses the shared notice");
    input.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_notice_wand_offers_a_reference() {
    let (_app, w, mut input) = start("art.capycanvas.NoticeWand");
    w.dispatch(UiAction::Layer {
        action: layer_ui::LayerAction::New { group: false, clipped: false },
    });
    w.dispatch(UiAction::Invoke { command: CommandId::AutoSelect });
    w.dispatch(UiAction::Invoke { command: CommandId::SelectionReference });
    pump(200);
    input.click(canvas_center(&w));
    until(
        || notice_text(&w).as_deref() == Some("This tool samples reference layers, and none is marked"),
        "a Wand click with no reference shows the notice",
    );
    assert!(!w.restart_canvas.is_visible(), "the canvas keeps running");
    assert!(!w.status.is_visible());
    let button = notice_action(&w);
    assert!(button.is_visible());
    assert_eq!(button.label().as_deref(), Some("Use Current ink as Reference"));
    assert!(w.area.has_focus());
    let b = button.compute_bounds(&w.window).unwrap();
    input.click([b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]);
    until(
        || {
            ui_session(&w).engine().document().scene().references()
                == [layer_core::authored::OccurrenceHandle::from_index(0)].into()
        },
        "the notice action marks the layer below as a reference",
    );
    assert!(!w.notice.root.is_visible());
    assert!(state(&w).notice.is_none());
    assert!(w.area.has_focus(), "the notice never takes focus from the canvas");
    input.click(canvas_center(&w));
    until(|| state(&w).notice.is_none() && !w.notice.root.is_visible(), "the Wand now samples the reference");
    input.finish();
    w.window.close();
    pump(50);
}
