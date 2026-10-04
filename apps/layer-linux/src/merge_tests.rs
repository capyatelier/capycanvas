//! Merges with actual Mutter delivery: Merge Down from Ctrl+E and the Layer
//! menu, Flatten Image confirmed from its notice, and Stamp Visible.
use super::*;
use photo_edit::{choose, document, shown, start};

/// Two painted layers crossing at the canvas centre over the paper.
fn painted(w: &Rc<Workspace>) -> [f32; 2] {
    let (width, height) = {
        let doc = document(w);
        (doc.width as f32, doc.height as f32)
    };
    w.dispatch(UiAction::Invoke { command: CommandId::Pen });
    w.dispatch(UiAction::SetBrushSize { value: 60. });
    w.dispatch(UiAction::SetColor { rgba: [0.1, 0.3, 0.85, 1.] });
    native_pen_path(w, &(0..=16).map(|i| [width * (0.2 + 0.0375 * i as f32), height * 0.5]).collect::<Vec<_>>());
    w.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } });
    w.dispatch(UiAction::SetColor { rgba: [0.9, 0.6, 0.05, 0.7] });
    native_pen_path(w, &(0..=16).map(|i| [width * 0.5, height * (0.2 + 0.0375 * i as f32)]).collect::<Vec<_>>());
    let count = document(w).layers.len();
    until(|| document(w).layers.iter().all(|l| l.raster.try_data().is_some()), "both strokes are captured");
    pump(300);
    assert!(count >= 3);
    [width * 0.5, height * 0.5]
}

fn command(w: &Workspace, id: CommandId) -> layer_ui::CommandState {
    state(w).commands.into_iter().find(|c| c.id == id).unwrap()
}

fn close(w: &Workspace, input: &RemoteInput) {
    input.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse and keyboard delivery"]
fn native_merge_down_from_ctrl_e_and_the_layer_menu() {
    let (_app, w, mut input) = start("art.capycanvas.MergeDown");
    let center = painted(&w);
    let before = document(&w);
    let crossing = shown(&w, center);
    assert!(command(&w, CommandId::MergeDown).enabled);
    input.perform(serde_json::json!([
        {"key": 0xffe3, "down": true}, {"key": 0x65, "down": true},
        {"key": 0x65, "down": false}, {"key": 0xffe3, "down": false}
    ]));
    until(|| document(&w).layers.len() + 1 == before.layers.len(), "Ctrl+E merges down");
    until(|| !ui_session(&w).engine().has_pending_document_edits(), "the merge runs");
    pump(300);
    let merged = shown(&w, center);
    assert!(crossing.iter().zip(merged).all(|(a, b)| a.abs_diff(b) <= 1), "{crossing:?} {merged:?}");
    let doc = document(&w);
    let result = doc.layer(doc.active_layer).unwrap();
    assert!(result.mask.is_none() && result.opacity == 1.);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).layers.len() == before.layers.len(), "one undo step restores both layers");
    choose(&w, &mut input, "Layer", &["Merge Down"]);
    until(|| document(&w).layers.len() + 1 == before.layers.len(), "Layer › Merge Down merges with the mouse");
    close(&w, &input);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_flatten_image_confirms_discarding_hidden_layers() {
    let (_app, w, mut input) = start("art.capycanvas.Flatten");
    let center = painted(&w);
    let hidden = document(&w).active_layer;
    w.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: hidden.0, value: false } });
    pump(300);
    let visible = shown(&w, center);
    choose(&w, &mut input, "Layer", &["Flatten Image"]);
    until(|| w.notice.root.is_visible(), "Flatten Image asks first");
    assert_eq!(state(&w).notice.unwrap().text, "Flattening discards 1 hidden layer");
    assert!(document(&w).layer(hidden).is_some(), "nothing changes before it is accepted");
    let button = find_named(w.notice.root.upcast_ref(), "canvas-notice-action").and_downcast::<gtk::Button>().unwrap();
    assert_eq!(button.label().as_deref(), Some("Flatten"));
    input.click(screen_point(button.upcast_ref(), &w.window, [0.5, 0.5]));
    until(
        || document(&w).layers.iter().filter(|l| l.id != layer_core::LayerId(2)).count() == 1,
        "Flatten leaves one layer over the paper",
    );
    assert!(document(&w).layer(hidden).is_none(), "the hidden layer is discarded");
    pump(300);
    let flat = shown(&w, center);
    assert!(visible.iter().zip(flat).all(|(a, b)| a.abs_diff(b) <= 1), "{visible:?} {flat:?}");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).layer(hidden).is_some(), "one undo step restores every layer");
    close(&w, &input);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_stamp_visible_adds_the_visible_image_on_top() {
    let (_app, w, mut input) = start("art.capycanvas.StampVisible");
    let center = painted(&w);
    let before = document(&w);
    let crossing = shown(&w, center);
    choose(&w, &mut input, "Layer", &["Stamp Visible"]);
    until(|| document(&w).layers.len() == before.layers.len() + 1, "Stamp Visible adds a layer");
    let doc = document(&w);
    assert_eq!((doc.layers[0].id, doc.layers[0].name.as_ref()), (doc.active_layer, "Visible"));
    for layer in before.layers.iter().filter(|l| l.id != layer_core::LayerId(2)) {
        w.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: layer.id.0, value: false } });
    }
    pump(300);
    let stamp = shown(&w, center);
    assert!(crossing.iter().zip(stamp).all(|(a, b)| a.abs_diff(b) <= 1), "the stamp alone: {crossing:?} {stamp:?}");
    close(&w, &input);
}

/// Timing, not a gate: Flatten Image and Merge Visible on a 24 MP photo with
/// nine painted layers above it, and the longest main-loop stall meanwhile.
#[test]
#[ignore = "hardware 24 MP merge timing: workspace-motion.sh gtk --native-test=native_merge_timing"]
fn native_merge_timing() {
    let app = native_test_app("art.capycanvas.MergeTiming");
    let mut project = native_navigation::photo([6000, 4000]);
    project.document.layers.retain(|layer| layer.source.is_some() || layer.id == layer_core::LayerId(2));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize();
    w.window.present();
    pump(1500);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    w.dispatch(UiAction::Invoke { command: CommandId::Pen });
    w.dispatch(UiAction::SetBrushSize { value: 120. });
    for layer in 0..9 {
        w.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } });
        w.dispatch(UiAction::SetColor { rgba: [0.1 * layer as f32, 0.9 - 0.08 * layer as f32, 0.5, 0.8] });
        let y = 400. + 380. * layer as f32;
        native_pen_path(&w, &(0..=24).map(|i| [300. + 225. * i as f32, y]).collect::<Vec<_>>());
    }
    let idle = |w: &Workspace| {
        let gpu = w.gpu.borrow();
        let engine = gpu.as_ref().unwrap().session.engine();
        !engine.has_pending_document_edits()
            && engine.document().layers.iter().all(|l| l.raster.try_data().is_some_and(|d| d.is_ok_and(|d| d.host_backed())))
    };
    until(|| idle(&w), "the strokes are captured");
    assert_eq!(document(&w).layers.len(), 11, "ten layers over the paper");
    for command in [CommandId::MergeVisible, CommandId::FlattenImage] {
        pump(500);
        let stall = Rc::new(Cell::new(Duration::ZERO));
        let last = Rc::new(Cell::new(Instant::now()));
        let tick = glib::timeout_add_local(Duration::from_millis(1), glib::clone!(#[strong] stall, #[strong] last, move || {
            let now = Instant::now();
            stall.set(stall.get().max(now - last.replace(now)));
            glib::ControlFlow::Continue
        }));
        let start = Instant::now();
        w.dispatch(UiAction::Invoke { command });
        let dispatch = start.elapsed();
        assert_eq!(document(&w).layers.len(), 2, "the merge replaces the layers");
        until(|| idle(&w), "the merged layer is captured");
        let captured = start.elapsed();
        tick.remove();
        eprintln!(
            "{command:?} on 24 MP with 10 layers: dispatch {:.1} ms, result captured {:.0} ms, longest main-loop gap {:.1} ms",
            dispatch.as_secs_f64() * 1e3,
            captured.as_secs_f64() * 1e3,
            stall.get().as_secs_f64() * 1e3,
        );
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        until(|| document(&w).layers.len() == 11, "undo restores the layers");
        until(|| idle(&w), "the restored layers are ready");
    }
    w.window.close();
    pump(50);
}
