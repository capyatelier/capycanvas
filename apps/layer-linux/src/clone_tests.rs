//! Retouching journeys with real Mutter delivery. Clone Stamp: Alt-click and a
//! bound side button set the source, the source disc drags at once with the
//! mouse, a finger and the pen, a tap shows its bar, and each stroke is one undo
//! step. Healing Brush and Spot Healing Brush: the stroke previews as a clone or
//! a tint, heals into its surroundings when the pen lifts, and undoes in one
//! step, with the mouse and with the pen.
use super::photo_edit::{document, shown, start, window_point};
use super::*;
use layer_render::CanvasRenderer;
use serde_json::json;

const ALT: u32 = 0xffe9;

fn session_source(w: &Workspace) -> layer_core::CloneSource {
    ui_session(&w).engine().clone_source()
}

fn strokes(w: &Workspace) -> u64 {
    ui_session(&w).engine().metrics().committed_strokes
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

const BLUE: [f32; 4] = [0.1, 0.3, 0.8, 1.];

/// Fill the rectangle `[x0, y0, x1, y1]`, as fractions of the canvas, with
/// `rgba`.
pub(super) fn fill(w: &Rc<Workspace>, rgba: [f32; 4], [x0, y0, x1, y1]: [f32; 4]) {
    w.dispatch(UiAction::SetColor { rgba });
    let revision = document(w).revision;
    let doc = document(w);
    let [width, height] = [doc.width as f32, doc.height as f32];
    let paint = doc.active_layer;
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    let [x0, y0, x1, y1] = [width * x0, height * y0, width * x1, height * y1];
    native_pen_path(w, &[[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]);
    w.dispatch(UiAction::Layer { action: LayerAction::FillSelection });
    until(
        || !document(w).layer(paint).unwrap().raster.is_empty() && document(w).revision > revision,
        "the fill paints the selection",
    );
    w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
}

/// Filled `rectangles`, then `tool` copying from the editing layer, ready before
/// pen-down.
fn retouch_ready(id: &str, tool: CommandId, rectangles: &[([f32; 4], [f32; 4])]) -> (NativeTestApp, Rc<Workspace>, RemoteInput) {
    let (app, w, input) = start(id);
    for &(rgba, rectangle) in rectangles {
        fill(&w, rgba, rectangle);
    }
    w.dispatch(UiAction::Invoke { command: tool });
    w.dispatch(UiAction::Invoke { command: CommandId::SelectionEditing });
    until(|| state(&w).commands.iter().any(|c| c.id == tool && c.selected), "the retouching tool is selected");
    until(
        || {
            w.gpu.borrow().as_ref().is_some_and(|g| {
                let engine = g.session.engine();
                engine.backend().paint_ready(engine.document(), engine.brush(), false)
            })
        },
        "the retouching brush is ready before pen-down",
    );
    (app, w, input)
}

fn clone_ready(id: &str) -> (NativeTestApp, Rc<Workspace>, RemoteInput) {
    let ready = retouch_ready(id, CommandId::Clone, &[(BLUE, [0.1, 0.2, 0.35, 0.8])]);
    until(|| session_source(&ready.1).point.is_some(), "Clone Stamp has a source");
    ready
}

pub(super) fn press(device: &str, from: [f32; 2], to: [f32; 2]) -> Vec<serde_json::Value> {
    let mut events = vec![contact(device, "down", from), json!({"wait_ms": 30})];
    for i in 1..=12 {
        let t = i as f32 / 12.;
        events.push(contact(device, "move", [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t]));
        events.push(json!({"wait_ms": 16}));
    }
    events
}

pub(super) fn lift(device: &str, at: [f32; 2]) -> Vec<serde_json::Value> {
    let mut events = vec![contact(device, "up", at)];
    if device == "pen" {
        events.push(json!({"pen": "leave"}));
    }
    events
}

pub(super) fn stroke(input: &mut RemoteInput, device: &str, from: [f32; 2], to: [f32; 2]) {
    let mut events = press(device, from, to);
    events.extend(lift(device, to));
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

pub(super) fn finish(w: &Workspace, input: &RemoteInput) {
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

fn raster(w: &Workspace) -> layer_core::raster::RasterRevision {
    document(w).layer(document(w).active_layer).unwrap().raster.clone()
}

fn blueish(pixel: [u8; 4]) -> bool {
    pixel[2] > pixel[0] + 60
}

/// Heal from the blue rectangle into the paper with `device`: while the pen is
/// down the stroke is the blue clone, and when it lifts the stroke takes on the
/// paper around it, as one undo step.
fn heal_journey(id: &str, device: &str) {
    let (_app, w, mut input) = retouch_ready(id, CommandId::Heal, &[(BLUE, [0.1, 0.2, 0.35, 0.8])]);
    let doc = document(&w);
    let [width, height] = [doc.width as f32, doc.height as f32];
    let source = [width * 0.2, height * 0.5];
    input.perform(json!([{ "key": ALT, "down": true }]));
    until(|| state(&w).commands.iter().any(|c| c.id == CommandId::CloneSourceArm && c.selected), "Alt arms Set Source");
    tap(&mut input, device, window_point(&w, source));
    input.perform(json!([{ "key": ALT, "down": false }]));
    until(|| close(source_point(&w), source, 1.5), "Alt and a click set the healing source");
    let before = raster(&w);
    let [from, to] = [[width * 0.6, height * 0.5], [width * 0.75, height * 0.5]];
    let middle = [width * 0.67, height * 0.5];
    input.perform(json!(press(device, window_point(&w, from), window_point(&w, to))));
    until(|| blueish(shown(&w, middle)), "the live stroke is the clone");
    input.perform(json!(lift(device, window_point(&w, to))));
    until(|| strokes(&w) == 1, "the stroke ends");
    until(|| paper(shown(&w, middle)), "the healed stroke takes on the paper around it");
    assert_ne!(raster(&w), before, "healing painted the layer");
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(300);
        if let Some(dir) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
            crate::snapshot(&w).save_to_png(std::path::Path::new(&dir).join(format!("heal-{device}-{theme:?}.png"))).unwrap();
        }
    }
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| raster(&w) == before, "one undo removes the heal");
    finish(&w, &input);
}

/// Spot heal a blue spot on a pale fill with `device`: when the stroke lifts
/// the spot is gone, as one undo step.
fn spot_heal_journey(id: &str, device: &str) {
    let pale = [0.9, 0.88, 0.85, 1.];
    let (_app, w, mut input) =
        retouch_ready(id, CommandId::SpotHeal, &[(pale, [0.2, 0.2, 0.8, 0.8]), (BLUE, [0.49, 0.49, 0.51, 0.51])]);
    let doc = document(&w);
    let [width, height] = [doc.width as f32, doc.height as f32];
    let spot = [width * 0.5, height * 0.5];
    assert!(blueish(shown(&w, spot)), "the spot is blue");
    assert!(state(&w).commands.iter().any(|c| c.id == CommandId::CloneSourceArm && !c.enabled), "spot healing has no source to set");
    w.dispatch(UiAction::SetBrushSize { value: width * 0.05 });
    let before = raster(&w);
    let [from, to] = [[width * 0.495, height * 0.5], [width * 0.505, height * 0.5]];
    let paper_beside = [width * 0.5, height * 0.4];
    let beside = shown(&w, paper_beside);
    input.perform(json!(press(device, window_point(&w, from), window_point(&w, to))));
    input.perform(json!(lift(device, window_point(&w, to))));
    until(|| strokes(&w) == 1, "the stroke ends");
    until(|| paper(shown(&w, spot)), "the spot is healed away");
    assert_eq!(shown(&w, paper_beside), beside, "the paper around it is untouched");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| raster(&w) == before && blueish(shown(&w, spot)), "one undo brings the spot back");
    finish(&w, &input);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_heal_stroke_with_the_mouse() {
    heal_journey("art.capycanvas.HealMouse", "mouse");
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_heal_stroke_with_the_pen --tablet"]
fn native_heal_stroke_with_the_pen() {
    heal_journey("art.capycanvas.HealPen", "pen");
}

#[test]
#[ignore = "isolated compositor, GPU and native pen and touch delivery"]
fn native_navigation_and_queued_paint_during_healing() {
    let (_app, w, input) = retouch_ready("art.capycanvas.HealNavigation", CommandId::SpotHeal, &[(BLUE, [0.1, 0.2, 0.35, 0.8])]);
    let mut input = input.settle_ms(0);
    let doc = document(&w);
    let [width, height] = [doc.width as f32, doc.height as f32];
    let before = raster(&w);
    w.dispatch(UiAction::SetBrushSize { value: width * 0.15 });
    stroke(&mut input, "pen", window_point(&w, [width * 0.4, height * 0.5]), window_point(&w, [width * 0.8, height * 0.5]));
    let pending = || ui_session(&w).engine().backend().has_pending_submission();
    until(pending, "the lifted stroke starts healing");
    w.dispatch(UiAction::Invoke { command: CommandId::Brush });
    w.dispatch(UiAction::SetBrushSize { value: 30. });
    tap(&mut input, "pen", window_point(&w, [width * 0.5, height * 0.7]));
    assert!(pending(), "the next contact arrives during healing");
    w.dispatch(UiAction::Invoke { command: CommandId::Hand });
    let camera = state(&w).camera.view();
    let at = window_point(&w, [width * 0.5, height * 0.5]);
    let to = [at[0] + 80., at[1] + 40.];
    input.perform(json!([contact("touch", "down", at), {"wait_ms": 16}, contact("touch", "move", to)]));
    until(|| state(&w).camera.view() != camera, "touch navigation moves during healing");
    assert_ne!(state(&w).camera.view(), camera, "touch navigation moves during healing");
    assert!(pending(), "navigation starts before healing finishes");
    assert!(blueish(shown(&w, [width * 0.33, height * 0.5])), "the presented canvas follows the camera");
    input.perform(json!(lift("touch", to)));
    until(|| !pending() && strokes(&w) == 2, "the queued contact paints after healing");
    let after = raster(&w);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| raster(&w) != after && !pending(), "the queued contact undoes");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| raster(&w) == before, "both contacts remain separate undo steps");
    finish(&w, &input);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_spot_heal_with_the_mouse() {
    spot_heal_journey("art.capycanvas.SpotHealMouse", "mouse");
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_spot_heal_with_the_pen --tablet"]
fn native_spot_heal_with_the_pen() {
    spot_heal_journey("art.capycanvas.SpotHealPen", "pen");
}
