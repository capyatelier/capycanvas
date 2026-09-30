//! Photo-editing journeys with actual Mutter delivery: a fill layer from
//! Layer › New, a Liquify Pinch stroke, and Revert to Original Photo.
use super::*;
use layer_core::{Document, LayerId, LayerKind};
use serde_json::json;

pub(super) fn start(id: &str) -> (NativeTestApp, Rc<Workspace>, RemoteInput) {
    let app = native_test_app(id);
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(200);
    let input = RemoteInput::new().settle_ms(200).timeout_secs(30);
    input.ready();
    pump(300);
    (app, w, input)
}

pub(super) fn document(w: &Workspace) -> Document {
    ui_session(&w).engine().document().clone()
}

pub(super) fn window_point(w: &Workspace, [x, y]: [f32; 2]) -> [f32; 2] {
    let m = state(w).camera.document_to_surface();
    let scale = w.area.scale_factor() as f32;
    let p = gtk::graphene::Point::new(
        (m[0] * x + m[2] * y + m[4]) / scale,
        (m[1] * x + m[3] * y + m[5]) / scale,
    );
    let p = w.area.compute_point(&w.window, &p).unwrap();
    [p.x(), p.y()]
}

/// RGBA8 of the window pixel over document point `at`.
pub(super) fn shown(w: &Workspace, at: [f32; 2]) -> [u8; 4] {
    let texture = crate::snapshot(w);
    let mut download = gdk::TextureDownloader::new(&texture);
    download.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = download.download_bytes();
    let [x, y] = window_point(w, at);
    let scale = texture.width() as f32 / w.window.width() as f32;
    let offset = (y * scale) as usize * stride + (x * scale) as usize * 4;
    bytes[offset..offset + 4].try_into().unwrap()
}

pub(super) use super::mapped_label as labelled;

/// Open a title bar menu with the mouse and click through `path`.
pub(super) fn choose(w: &Workspace, input: &mut RemoteInput, menu: &str, path: &[&str]) {
    let button = menu_button(w.header.root.upcast_ref(), menu).unwrap_or_else(|| panic!("{menu} menu"));
    let popup = button.popover().unwrap();
    input.click(screen_point(button.upcast_ref(), &w.window, [0.5, 0.5]));
    until(|| popup.is_mapped(), &format!("the {menu} menu opens"));
    for label in path {
        let mut item = None;
        until(|| {
            item = labelled(popup.upcast_ref(), label);
            item.is_some()
        }, label);
        input.click(screen_point(&item.unwrap(), &w.window, [0.5, 0.5]));
    }
    until(|| !popup.is_visible(), &format!("{} closes the menu", path.join(" › ")));
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_layer_new_solid_color_fill_masks_to_the_selection() {
    let (_app, w, mut input) = start("art.capycanvas.SolidColorFill");
    w.dispatch(UiAction::SetColor { rgba: [0.85, 0.08, 0.05, 1.] });
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    let (width, height) = {
        let doc = document(&w);
        (doc.width as f32, doc.height as f32)
    };
    let [x0, y0, x1, y1] = [width * 0.25, height * 0.25, width * 0.55, height * 0.6];
    native_pen_path(&w, &[[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]);
    until(|| document(&w).selection.is_some(), "the lasso makes a selection");
    let base = document(&w).active_layer;
    choose(&w, &mut input, "Layer", &["New", "Solid Color Fill"]);
    until(
        || {
            let doc = document(&w);
            doc.layer(doc.active_layer).is_some_and(|l| l.kind == LayerKind::Effect && l.mask.is_some())
        },
        "Layer › New › Solid Color Fill inserts a fill layer",
    );
    let doc = document(&w);
    let fill = doc.layer(doc.active_layer).unwrap();
    assert_eq!(fill.effect.as_ref().unwrap().program.id.as_ref(), "solid_color");
    assert!(fill.mask.as_ref().unwrap().initial.is_some(), "the selection becomes its mask");
    assert!(doc.selection.is_none());
    assert_eq!(doc.layers.iter().position(|l| l.id == fill.id).unwrap() + 1, doc.layers.iter().position(|l| l.id == base).unwrap());
    pump(300);
    let inside = shown(&w, [(x0 + x1) * 0.5, (y0 + y1) * 0.5]);
    let outside = shown(&w, [width * 0.8, height * 0.8]);
    assert!(inside[0] > 180 && inside[1] < 80 && inside[2] < 80, "the current colour fills the selection: {inside:?}");
    assert!(outside.iter().take(3).all(|v| *v > 200), "the paper shows outside it: {outside:?}");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).layers.len() + 1 == doc.layers.len(), "one undo step removes the fill");
    input.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_liquify_pinch_stroke_on_a_pattern --tablet"]
fn native_liquify_pinch_stroke_on_a_pattern() {
    let (_app, w, mut input) = start("art.capycanvas.LiquifyPinch");
    let (width, height) = {
        let doc = document(&w);
        (doc.width as f32, doc.height as f32)
    };
    w.dispatch(UiAction::SetBrushSize { value: 60. });
    w.dispatch(UiAction::SetColor { rgba: [0.05, 0.1, 0.6, 1.] });
    for row in 0..7 {
        let y = height * (0.2 + 0.1 * row as f32);
        native_pen_path(&w, &(0..=16).map(|i| [width * (0.1 + 0.05 * i as f32), y]).collect::<Vec<_>>());
    }
    let paint = document(&w).active_layer;
    let pattern = document(&w).layer(paint).unwrap().raster.identity();
    w.dispatch(UiAction::Invoke { command: CommandId::Liquify });
    w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::LiquifyPinch as u32 });
    w.dispatch(UiAction::SetBrushSize { value: 400. });
    pump(300);
    let selected = state(&w);
    assert_eq!(selected.brush.preset, layer_core::DefaultBrushPreset::LiquifyPinch as u32);
    assert!(layer_ui::brush_catalog().any(|b| b.id == selected.brush.preset && b.label == "Liquify Pinch"));
    let far = [width * 0.95, height * 0.08];
    let untouched = shown(&w, far);
    let from = window_point(&w, [width * 0.3, height * 0.45]);
    let to = window_point(&w, [width * 0.7, height * 0.45]);
    let mut events = vec![json!({"pen": "down", "point": from})];
    for i in 1..=24 {
        let t = i as f32 / 24.;
        events.push(json!({"wait_ms": 16}));
        events.push(json!({"pen": "move", "point": [from[0] + (to[0] - from[0]) * t, from[1]]}));
    }
    events.extend([json!({"pen": "up"}), json!({"pen": "leave"})]);
    input.perform(json!(events));
    until(|| document(&w).layer(paint).unwrap().raster.identity() != pattern, "the Pinch stroke edits the pattern");
    assert!(state(&w).host_error.is_none(), "{:?}", state(&w).host_error);
    pump(300);
    assert_eq!(shown(&w, far), untouched, "pixels away from the stroke stay");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).layer(paint).unwrap().raster.identity() == pattern, "one undo step restores the pattern");
    input.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_revert_to_original_after_painting_on_a_placed_photo() {
    let (_app, w, mut input) = start("art.capycanvas.RevertPhoto");
    let (width, height) = {
        let doc = document(&w);
        (doc.width, doc.height)
    };
    let photo = layer_core::color::source::rgba8_source([width, height], |x, y| {
        if (x / 64 + y / 64) % 2 == 0 { [40, 170, 90, 255] } else { [230, 220, 60, 255] }
    });
    ui_session_mut(&w)
        .import_layer_source("Photo", std::sync::Arc::unwrap_or_clone(photo))
        .unwrap();
    w.refresh(regions::DOCUMENT | regions::COMMANDS);
    w.wake();
    pump(300);
    let id: LayerId = document(&w).active_layer;
    let original = document(&w).layer(id).unwrap().clone();
    assert!(original.source.as_ref().is_some_and(|s| s.is_original()));
    let revert = |w: &Workspace| state(w).commands.into_iter().find(|c| c.id == CommandId::RevertToOriginal).unwrap();
    assert_eq!(revert(&w).disabled_reason.as_deref(), Some("This photo has no edits"));
    w.dispatch(UiAction::Invoke { command: CommandId::Pen });
    w.dispatch(UiAction::SetColor { rgba: [0.9, 0.05, 0.6, 1.] });
    w.dispatch(UiAction::SetBrushSize { value: 80. });
    let [w_, h_] = [width as f32, height as f32];
    native_pen_path(&w, &(0..=20).map(|i| [w_ * (0.2 + 0.03 * i as f32), h_ * 0.5]).collect::<Vec<_>>());
    until(|| !document(&w).layer(id).unwrap().raster.is_empty(), "painting over the photo creates edits");
    let painted = document(&w).layer(id).unwrap().clone();
    until(|| revert(&w).enabled, "Revert to Original Photo is enabled once the photo has edits");
    let stroke = [w_ * 0.5, h_ * 0.5];
    pump(300);
    let before = shown(&w, stroke);
    choose(&w, &mut input, "Edit", &["Revert to Original Photo"]);
    until(|| document(&w).layer(id).unwrap().raster.is_empty(), "Revert discards the edits");
    let reverted = document(&w).layer(id).unwrap().clone();
    assert!(std::sync::Arc::ptr_eq(reverted.source.as_ref().unwrap(), painted.source.as_ref().unwrap()));
    assert_eq!(reverted.properties, painted.properties);
    assert_eq!((reverted.opacity, &reverted.mask), (painted.opacity, &painted.mask));
    pump(300);
    let after = shown(&w, stroke);
    assert_ne!(before, after, "the stroke disappears: {before:?} {after:?}");
    assert!(after[2] < 120, "the photo shows again: {after:?}");
    assert_eq!(revert(&w).disabled_reason.as_deref(), Some("This photo has no edits"));
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).layer(id).unwrap().raster == painted.raster, "one undo step brings the edits back");
    input.finish();
    w.window.close();
    pump(50);
}

/// Commit Canvas Size by clicking Apply in its dialog with the mouse.
pub(super) fn apply_canvas_size(w: &Workspace, input: &mut RemoteInput) {
    let dialog = w.canvas_size.dialog.clone();
    let apply = until_some_widget(|| labelled(dialog.upcast_ref(), "Apply"), "the Apply button");
    input.click(screen_point(&apply, &w.window, [0.5, 0.5]));
    until(|| state(w).layer_tools.canvas_size.is_none(), "Apply closes Canvas Size");
}

pub(super) fn until_some_widget(mut find: impl FnMut() -> Option<gtk::Widget>, message: &str) -> gtk::Widget {
    let mut found = None;
    until(|| {
        found = find();
        found.is_some()
    }, message);
    found.unwrap()
}

pub(super) fn canvas_size_number(w: &Workspace, axis: &str) -> crate::number_control::NumberControl {
    let dialog = w.canvas_size.dialog.clone();
    let field = until_some_widget(|| find_named(dialog.upcast_ref(), &format!("canvas-size-{axis}")), axis);
    field.first_child().and_downcast().expect("a number control")
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse and keyboard delivery"]
fn native_canvas_size_from_the_top_left_anchor_then_undo() {
    let (_app, w, mut input) = start("art.capycanvas.CanvasSize");
    let before = document(&w);
    let paint = before.active_layer;
    choose(&w, &mut input, "Edit", &["Image", "Canvas Size…"]);
    until(|| state(&w).layer_tools.canvas_size.is_some(), "Edit › Image › Canvas Size… opens the dialog");
    let width = canvas_size_number(&w, "width");
    let spin = width.first_child().and_then(|header| header.last_child()).and_downcast::<gtk::SpinButton>().unwrap();
    spin.set_text("2600");
    let anchor = until_some_widget(|| find_named(w.canvas_size.dialog.upcast_ref(), "canvas-size-anchor-top_left"), "the top-left anchor");
    input.click(screen_point(&anchor, &w.window, [0.5, 0.5]));
    until(|| state(&w).layer_tools.canvas_size.is_some_and(|v| v.anchor == layer_ui::CanvasAnchor::TopLeft), "the anchor picker chooses top left");
    let view = state(&w).layer_tools.canvas_size.unwrap();
    assert_eq!(view.values[0], 2600., "choosing an anchor keeps the typed width");
    assert!(view.can_apply, "{}", view.message);
    assert!(!anchor.has_focus(), "the anchor picker does not take focus");
    if let Some(dir) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
        pump(200);
        crate::snapshot(&w).save_to_png(std::path::Path::new(&dir).join("canvas-size-dialog.png")).unwrap();
    }
    apply_canvas_size(&w, &mut input);
    let grown = document(&w);
    assert_eq!([grown.width, grown.height], [2600, before.height]);
    assert_eq!(grown.layer(paint).unwrap().properties.offset, before.layer(paint).unwrap().properties.offset);
    assert!(state(&w).host_error.is_none(), "{:?}", state(&w).host_error);
    input.perform(json!([
        {"key": 0xffe3, "down": true}, {"key": 0x7a, "down": true},
        {"key": 0x7a, "down": false}, {"key": 0xffe3, "down": false}
    ]));
    until(|| document(&w).width == before.width, "Ctrl+Z on the canvas undoes the canvas size in one step");
    input.finish();
    w.window.close();
    pump(50);
}
