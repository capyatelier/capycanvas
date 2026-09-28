//! Clone Stamp journeys with real Mutter delivery: Alt-click and a bound side
//! button set the source, the source disc drags at once with the mouse, a
//! finger and the pen, a tap shows its bar, and each stroke is one undo step.
use super::photo_edit::{document, shown, start, window_point};
use super::*;
use serde_json::json;

const ALT: u32 = 0xffe9;

fn session_source(w: &Workspace) -> layer_core::CloneSource {
    w.gpu.borrow().as_ref().unwrap().session.engine().clone_source()
}

fn strokes(w: &Workspace) -> u64 {
    w.gpu.borrow().as_ref().unwrap().session.engine().metrics().committed_strokes
}

fn source_point(w: &Workspace) -> [f32; 2] {
    let p = session_source(w).point.expect("a clone source");
    [p.x, p.y]
}

fn close(a: [f32; 2], b: [f32; 2], within: f32) -> bool {
    (a[0] - b[0]).abs() <= within && (a[1] - b[1]).abs() <= within
}

fn blue(pixel: [u8; 4]) -> bool {
    pixel[2] > 150 && pixel[0] < 100
}

fn paper(pixel: [u8; 4]) -> bool {
    pixel.iter().take(3).all(|v| *v > 200)
}

/// A blue rectangle in the left third of the canvas, then Clone Stamp copying
/// from the editing layer.
fn clone_ready(id: &str) -> (NativeTestApp, Rc<Workspace>, RemoteInput) {
    let (app, w, input) = start(id);
    w.dispatch(UiAction::SetColor { rgba: [0.1, 0.3, 0.8, 1.] });
    let doc = document(&w);
    let [width, height] = [doc.width as f32, doc.height as f32];
    let paint = doc.active_layer;
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    let [x0, y0, x1, y1] = [width * 0.1, height * 0.2, width * 0.35, height * 0.8];
    native_pen_path(&w, &[[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]);
    w.dispatch(UiAction::Layer { action: LayerAction::FillSelection });
    until(|| !document(&w).layer(paint).unwrap().raster.is_empty(), "the fill paints the selection");
    w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
    w.dispatch(UiAction::Invoke { command: CommandId::Clone });
    w.dispatch(UiAction::Invoke { command: CommandId::SelectionEditing });
    until(|| state(&w).brush.tool == layer_ui::Tool::Clone && session_source(&w).point.is_some(), "Clone Stamp has a source");
    until(
        || {
            w.gpu.borrow().as_ref().is_some_and(|g| {
                let engine = g.session.engine();
                engine.backend().paint_ready(engine.document(), engine.brush(), false)
            })
        },
        "the clone brush is ready before pen-down",
    );
    (app, w, input)
}

fn stroke(input: &mut RemoteInput, device: &str, from: [f32; 2], to: [f32; 2]) {
    let mut events = vec![contact(device, "down", from), json!({"wait_ms": 30})];
    for i in 1..=12 {
        let t = i as f32 / 12.;
        events.push(contact(device, "move", [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t]));
        events.push(json!({"wait_ms": 16}));
    }
    events.push(contact(device, "up", to));
    if device == "pen" {
        events.push(json!({"pen": "leave"}));
    }
    input.perform(json!(events));
}

fn tap(input: &mut RemoteInput, device: &str, at: [f32; 2]) {
    let mut events = vec![contact(device, "down", at), json!({"wait_ms": 40}), contact(device, "up", at)];
    if device == "pen" {
        events.push(json!({"pen": "leave"}));
    }
    input.perform(json!(events));
}

fn disc_bar(w: &Workspace) -> bool {
    state(w).canvas_bar.as_ref().is_some_and(|b| b.context.kind == layer_ui::CanvasBarKind::CloneSource)
        && w.canvas_bar.root.is_mapped()
}

fn bar_button(w: &Workspace, command: CommandId) -> [f32; 2] {
    let name = format!("canvas-bar-{command:?}");
    let mut found = None;
    until(
        || {
            found = find_named(w.canvas_bar.root.upcast_ref(), &name).filter(|b| b.is_mapped());
            found.is_some()
        },
        &name,
    );
    let b = found.unwrap().compute_bounds(&w.window).expect("mapped bar button");
    [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]
}

/// Drag the disc by `by` window pixels with `device`; the source moves by
/// the same distance on the canvas and nothing is painted.
fn drag_disc(w: &Workspace, input: &mut RemoteInput, device: &str, by: [f32; 2]) {
    let before = source_point(w);
    let revision = document(w).revision;
    let from = window_point(w, before);
    let to = [from[0] + by[0], from[1] + by[1]];
    stroke(input, device, from, to);
    until(|| close(window_point(w, source_point(w)), to, 2.), &format!("the {device} drags the disc"));
    assert_eq!(document(w).revision, revision, "dragging the disc paints nothing");
    assert!(!disc_bar(w), "a drag is not a tap");
}

fn finish(w: &Workspace, input: &RemoteInput) {
    assert!(state(w).host_error.is_none(), "{:?}", state(w).host_error);
    input.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_clone_alt_click_stroke_disc_and_bar_with_the_mouse() {
    let (_app, w, mut input) = clone_ready("art.capycanvas.CloneMouse");
    let doc = document(&w);
    let [width, height] = [doc.width as f32, doc.height as f32];
    let source = [width * 0.2, height * 0.5];
    input.perform(json!([{ "key": ALT, "down": true }]));
    until(|| state(&w).commands.iter().any(|c| c.id == CommandId::CloneSourceArm && c.selected), "Alt arms Set Source");
    input.click(window_point(&w, source));
    input.perform(json!([{ "key": ALT, "down": false }]));
    until(|| close(source_point(&w), source, 1.5), "Alt-click sets the source");
    assert_eq!(strokes(&w), 0, "Alt-click paints nothing");

    let [from, to] = [[width * 0.6, height * 0.5], [width * 0.75, height * 0.5]];
    stroke(&mut input, "mouse", window_point(&w, from), window_point(&w, to));
    until(|| strokes(&w) == 1, "the mouse clones a stroke");
    pump(300);
    assert!(blue(shown(&w, [width * 0.65, height * 0.5])), "the stroke copies the blue fill");
    assert!(close(source_point(&w), [source[0] + to[0] - from[0], source[1]], 3.), "an aligned source follows the stroke");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    pump(300);
    assert!(paper(shown(&w, [width * 0.65, height * 0.5])), "one undo removes the stroke");

    drag_disc(&w, &mut input, "mouse", [60., 40.]);
    tap(&mut input, "mouse", window_point(&w, source_point(&w)));
    until(|| disc_bar(&w), "a click on the disc shows its bar");
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(300);
        if let Some(dir) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
            crate::snapshot(&w).save_to_png(std::path::Path::new(&dir).join(format!("clone-bar-{theme:?}.png"))).unwrap();
        }
    }
    input.click(bar_button(&w, CommandId::CloneFlipHorizontal));
    until(|| session_source(&w).flip == [true, false], "the bar flips the source");
    input.click(bar_button(&w, CommandId::CloneAligned));
    until(|| !session_source(&w).aligned, "the bar turns Aligned off");
    finish(&w, &input);
}

#[test]
#[ignore = "isolated compositor, GPU and native touch delivery"]
fn native_clone_disc_drags_and_taps_with_a_finger() {
    let (_app, w, mut input) = clone_ready("art.capycanvas.CloneTouch");
    let camera = state(&w).camera.clone();
    drag_disc(&w, &mut input, "touch", [-70., 50.]);
    assert_eq!(state(&w).camera.view(), camera.view(), "the disc never pans the canvas");
    let at = window_point(&w, source_point(&w));
    tap(&mut input, "touch", at);
    until(|| disc_bar(&w), "a finger tap on the disc shows its bar");
    let before = session_source(&w);
    stroke(&mut input, "touch", [at[0] + 250., at[1] + 120.], [at[0] + 350., at[1] + 160.]);
    pump(200);
    assert_eq!(session_source(&w), before, "a finger elsewhere never moves the source");
    assert_eq!(strokes(&w), 0, "and never paints");
    finish(&w, &input);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_clone_side_button_disc_and_strokes_with_the_pen --tablet"]
fn native_clone_side_button_disc_and_strokes_with_the_pen() {
    let (_app, w, mut input) = clone_ready("art.capycanvas.ClonePen");
    let doc = document(&w);
    let [width, height] = [doc.width as f32, doc.height as f32];
    w.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts });
    for action in [
        PreferenceAction::EditPenButton { trigger: "pen.button.primary".into() },
        PreferenceAction::PenButtonPerTool { trigger: "pen.button.primary".into(), per_tool: true },
        PreferenceAction::OpenPenButtonPicker {
            trigger: "pen.button.primary".into(),
            category: Some(layer_ui::ToolCategory::Retouching),
        },
        PreferenceAction::ChooseAction { id: "command.CloneSourceArm".into() },
    ] {
        w.dispatch(UiAction::Preferences { action });
    }
    w.dispatch(UiAction::CloseSettings);
    pump(300);
    let source = [width * 0.25, height * 0.4];
    let at = window_point(&w, source);
    input.perform(json!([{"pen": "move", "point": at}, {"pen": "button", "button": 331, "down": true}]));
    until(|| state(&w).commands.iter().any(|c| c.id == CommandId::CloneSourceArm && c.selected), "the side button arms Set Source");
    tap(&mut input, "pen", at);
    input.perform(json!([{"pen": "move", "point": at}, {"pen": "button", "button": 331, "down": false}, {"pen": "leave"}]));
    until(|| close(source_point(&w), source, 1.5), "side button and tap set the source");
    assert_eq!(strokes(&w), 0);

    drag_disc(&w, &mut input, "pen", [40., -30.]);
    let moved = source_point(&w);
    let before = document(&w).layer(document(&w).active_layer).unwrap().raster.clone();
    for (i, y) in [0.45f32, 0.6].into_iter().enumerate() {
        stroke(&mut input, "pen", window_point(&w, [width * 0.6, height * y]), window_point(&w, [width * 0.72, height * y]));
        until(|| strokes(&w) == i as u64 + 1, "the pen clones a stroke");
    }
    pump(300);
    assert!(blue(shown(&w, [width * 0.62, height * 0.45])), "the first stroke copies the fill");
    assert!(!close(source_point(&w), moved, 1.), "an aligned source follows the strokes");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    pump(200);
    assert!(blue(shown(&w, [width * 0.62, height * 0.45])), "undo removes only the last stroke");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).layer(document(&w).active_layer).unwrap().raster == before, "each stroke is one undo step");
    tap(&mut input, "pen", window_point(&w, source_point(&w)));
    until(|| disc_bar(&w), "a pen tap on the disc shows its bar");
    finish(&w, &input);
}
