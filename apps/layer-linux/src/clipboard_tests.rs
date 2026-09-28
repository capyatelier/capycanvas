//! Pixel clipboard journeys: Copy and Paste within a drawing, between
//! drawings and with other applications, from the keyboard and the bar.
use super::canvas_bar_tests::{Device, choose_from_bar_menu, document, filled_selection, remote_input, until_some};
use super::*;
use crate::files::clipboard::{CLIP_MIME, current};
use layer_core::color::source::SourceKind;
use layer_ui::{CanvasBarMenu, LayerAction, PixelClip};
use serde_json::json;
use std::io::Read;

const CONTROL: u32 = 0xffe3;
const SHIFT: u32 = 0xffe1;

/// Press `key` while `modifiers` are held.
fn chord(native: &mut RemoteInput, modifiers: &[u32], key: u32) {
    let mut events: Vec<_> = modifiers.iter().map(|m| json!({"key": m, "down": true})).collect();
    events.extend([json!({"key": key, "down": true}), json!({"key": key, "down": false})]);
    events.extend(modifiers.iter().rev().map(|m| json!({"key": m, "down": false})));
    native.perform(serde_json::Value::Array(events));
}

fn nonce() -> Option<String> {
    current().map(|clip| clip.nonce)
}

fn idle(w: &Workspace) -> bool {
    let s = state(w);
    !s.document_file.busy && s.requests.is_empty()
}

/// A new clip published by the copy that just started.
fn copied(w: &Workspace, previous: Option<String>, label: &str) -> PixelClip {
    let deadline = Instant::now() + Duration::from_secs(30);
    while nonce() == previous || !idle(w) {
        let s = state(w);
        assert!(
            Instant::now() < deadline,
            "{label} reaches the clipboard: status {:?} focus {:?} {:?} {:?} {:?} {:?}",
            w.status.text(),
            gtk::prelude::GtkWindowExt::focus(&w.window).map(|f| f.type_().name()),
            s.requests.iter().map(|r| &r.kind).collect::<Vec<_>>(),
            s.host_error,
            s.notice,
            s.commands.iter().find(|c| c.id == CommandId::Copy),
        );
        pump(10);
    }
    assert!(state(w).host_error.is_none(), "{:?}", state(w).host_error);
    current().unwrap()
}

fn clipboard_formats(w: &Workspace) -> Vec<String> {
    w.window.clipboard().formats().mime_types().iter().map(|m| m.to_string()).collect()
}

/// Read the clipboard's PNG from another Wayland client while GTK serves it.
fn external_png() -> Vec<u8> {
    let mut child = std::process::Command::new("wl-paste")
        .args(["--no-newline", "--type", "image/png"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("wl-paste");
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    until(|| child.try_wait().unwrap().is_some(), "another application reads the PNG");
    reader.join().unwrap().unwrap()
}

/// Put an image on the clipboard from another Wayland client.
fn external_copy(png: &[u8]) {
    let mut child = std::process::Command::new("wl-copy")
        .args(["--type", "image/png"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("wl-copy");
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), png).unwrap();
    until(|| child.try_wait().unwrap().is_some(), "wl-copy offers the image");
}

fn active_layer(w: &Workspace) -> layer_core::Layer {
    let doc = document(w);
    doc.layer(doc.active_layer).unwrap().clone()
}

/// Mouse and touch only: the tablet proxy does not forward clipboard
/// requests, so `--tablet` runs lose their Wayland connection on the first copy.
#[test]
#[ignore = "isolated compositor, GPU, wl-clipboard and native keyboard, mouse and touch delivery"]
fn native_clipboard_copy_paste_round_trips() {
    let app = native_test_app("art.capycanvas.Clipboard");
    let w = fixture_workspace(&app);
    w.window.present();
    w.window.maximize();
    pump(900);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    w.dispatch(UiAction::SetColor { rgba: [0.12, 0.38, 0.72, 1.] });
    let paint = document(&w).active_layer;
    filled_selection(&w, paint.0);
    let mut native = remote_input();

    let clip = {
        let before = nonce();
        chord(&mut native, &[CONTROL], 0x63);
        copied(&w, before, "Ctrl+C")
    };
    let selection = document(&w).selection.clone().unwrap().coverage_bounds();
    assert_eq!(clip.origin, [selection.min.x.floor() as u32, selection.min.y.floor() as u32]);
    assert_eq!(clip.source.kind, SourceKind::Rasterized);
    let formats = clipboard_formats(&w);
    assert!(formats.iter().any(|m| m == "image/png") && formats.iter().any(|m| m == CLIP_MIME), "{formats:?}");
    let png = external_png();
    let photo = layer_color::photo::read_photo(std::io::Cursor::new(&png), Default::default()).unwrap();
    assert_eq!(photo.extent, clip.source.extent, "another application reads the copy as a PNG");
    w.window.present();
    pump(300);

    let layers = document(&w).layers.len();
    chord(&mut native, &[CONTROL], 0x76);
    until(|| document(&w).layers.len() == layers + 1 && idle(&w), "Ctrl+V pastes a new layer");
    let pasted = active_layer(&w);
    assert_eq!(pasted.source.as_deref(), Some(&clip.source_for(document(&w).color)));
    assert_eq!(pasted.properties.placement.0[4..], clip.origin.map(|v| v as f32), "at the copied position");
    assert!(state(&w).canvas_bar.is_none_or(|b| b.context.kind != layer_ui::CanvasBarKind::Placement), "no handles");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).layers.len() == layers, "one undo step");

    let merged = {
        let before = nonce();
        chord(&mut native, &[CONTROL, SHIFT], 0x63);
        copied(&w, before, "Ctrl+Shift+C")
    };
    assert_eq!(merged.name, "Merged copy");

    let erased = document(&w).layer(paint).unwrap().raster.identity();
    let cut = {
        let before = nonce();
        chord(&mut native, &[CONTROL], 0x78);
        copied(&w, before, "Ctrl+X")
    };
    until(|| document(&w).layer(paint).unwrap().raster.identity() != erased, "Cut erases the copied pixels");
    assert_eq!(cut.source.extent, clip.source.extent);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });

    for device in [Device::Mouse, Device::Touch] {
        filled_selection(&w, paint.0);
        let before = nonce();
        choose_from_bar_menu(&w, &mut native, device, CanvasBarMenu::Copy, &["Copy Merged"]);
        assert_eq!(copied(&w, before, &format!("{device:?}: Copy ▾ › Copy Merged")).name, "Merged copy");
        let layers = document(&w).layers.len();
        w.dispatch(UiAction::Invoke { command: CommandId::PasteInto });
        until(|| document(&w).layers.len() == layers + 1 && idle(&w), &format!("{device:?}: Paste Into"));
        let into = active_layer(&w);
        assert!(into.mask.as_ref().is_some_and(|m| m.initial.is_some()), "a mask from the selection");
        assert!(document(&w).selection.is_none());
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        until(|| document(&w).layers.len() == layers && document(&w).selection.is_some(), "Paste Into undoes in one step");
    }

    let mut other = new_drawing(640, 480).unwrap();
    other.document.color = layer_core::color::DocumentColor {
        space: layer_core::color::RgbSpace::DisplayP3,
        depth: layer_core::color::SampleDepth::U16,
    };
    let second = Workspace::with_project(&app, Some((other, None)));
    second.window.present();
    until(|| second.gpu.borrow().as_ref().is_some_and(|g| g.session.require_document_idle().is_ok()), "the second drawing");
    pump(600);
    let copy = current().unwrap();
    chord(&mut native, &[CONTROL, SHIFT], 0x76);
    until(|| document(&second).layers.len() == 3 && idle(&second), "Paste in Place into another drawing");
    let pasted = active_layer(&second);
    assert_eq!(pasted.source.as_ref().unwrap().kind, SourceKind::Original, "another colour mode keeps an explicit profile");
    assert_eq!(pasted.properties.placement.0[4..], copy.origin.map(|v| v as f32));
    second.window.destroy();
    w.window.present();
    pump(400);

    external_copy(&png);
    until(|| !clipboard_formats(&w).iter().any(|m| m == CLIP_MIME), "the clipboard now holds another application's image");
    let layers = document(&w).layers.len();
    chord(&mut native, &[CONTROL], 0x76);
    until(|| document(&w).layers.len() == layers + 1, "an image from another app pastes");
    until(|| state(&w).canvas_bar.is_some_and(|b| b.context.kind == layer_ui::CanvasBarKind::Placement), "with placement handles");
    w.dispatch(UiAction::Invoke { command: CommandId::CancelTransform });
    until(|| document(&w).layers.len() == layers && idle(&w), "cancelling the placement removes it");
    chord(&mut native, &[CONTROL, SHIFT], 0x76);
    until(|| document(&w).layers.len() == layers + 1 && idle(&w), "Paste in Place of another app's image");
    assert!(state(&w).canvas_bar.is_none_or(|b| b.context.kind != layer_ui::CanvasBarKind::Placement), "centred without handles");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).layers.len() == layers, "one undo step");

    w.dispatch(UiAction::Layer { action: LayerAction::BeginRename { id: paint.0 } });
    let entry: gtk::Entry = until_some(
        || find_css(w.layer_panel.root.upcast_ref(), "layer-name-entry").and_then(|e| e.downcast::<gtk::Entry>().ok()).filter(|e| e.is_mapped()),
        "the rename field",
    );
    entry.set_text("Typed name");
    entry.grab_focus();
    entry.select_region(0, -1);
    pump(100);
    let before = nonce();
    chord(&mut native, &[CONTROL], 0x63);
    let text = glib::MainContext::default().block_on(w.window.clipboard().read_text_future()).unwrap();
    assert_eq!(text.as_deref(), Some("Typed name"), "the focused field copies its text");
    assert!(state(&w).requests.is_empty() && nonce() == before, "Ctrl+C in a text field copies no pixels");
    entry.set_text("");
    chord(&mut native, &[CONTROL], 0x76);
    until(|| entry.text() == "Typed name", "Ctrl+V pastes text into the field");
    assert!(state(&w).requests.is_empty());
    entry.emit_activate();
    pump(200);
}

#[test]
#[ignore = "isolated compositor, GPU and native keyboard delivery; prints copy latency"]
fn native_clipboard_copy_latency_24mp() {
    let [width, height] = [6000u32, 4000];
    let mut project = new_drawing(width, height).unwrap();
    let interpretation = layer_core::color::source::SourceInterpretation {
        channels: layer_core::color::source::SourceChannels::Rgb,
        depth: layer_core::color::SampleDepth::U8,
        profile: Default::default(),
        profile_assumed: false,
    };
    let mut builder = layer_core::color::source::SourceBuilder::new([width, height], interpretation, 1 << 30).unwrap();
    for y in 0..height {
        let row: Vec<u8> = (0..width).flat_map(|x| [(x / 24) as u8, (y / 16) as u8, ((x ^ y) & 255) as u8]).collect();
        builder.push_row(&row).unwrap();
    }
    project.document.layers[0].source = Some(std::sync::Arc::new(builder.finish().unwrap()));
    let app = native_test_app("art.capycanvas.ClipboardLatency");
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    w.window.maximize();
    until(|| w.gpu.borrow().as_ref().is_some_and(|g| g.session.require_document_idle().is_ok()), "the 24 MP drawing");
    pump(1500);
    let mut native = remote_input();
    let timed = |native: &mut RemoteInput, label: &str| {
        until(|| state(&w).commands.iter().any(|c| c.id == CommandId::Copy && c.enabled), "Copy is available");
        let before = nonce();
        let started = Instant::now();
        chord(native, &[CONTROL], 0x63);
        let mut progress = false;
        while nonce() == before || !idle(&w) {
            progress |= w.window.visible_dialog().is_some_and(|d| {
                d.widget_name() == "clipboard-progress" && find_button(d.upcast_ref(), "Cancel").is_some()
            });
            assert!(started.elapsed() < Duration::from_secs(30), "{label}: {:?}", state(&w).host_error);
            pump(5);
        }
        let elapsed = started.elapsed();
        let clip = current().unwrap();
        let [width, height] = clip.source.extent;
        assert_eq!(progress, u64::from(width) * u64::from(height) > layer_ui::LARGE_CLIP_PIXELS, "{label}: progress with Cancel");
        println!("{label}: {width} × {height} copied in {:.0} ms", elapsed.as_secs_f64() * 1000.);
        until(|| w.window.visible_dialog().is_none(), "the progress closes");
        pump(300);
        clip
    };
    w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
    let whole = timed(&mut native, "24 MP photo, Select All");
    let photo = document(&w).layers[0].source.clone().unwrap();
    assert!(std::sync::Arc::ptr_eq(&whole.source, &photo), "an untouched photo keeps its original samples");
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    native_pen_path(&w, &[[100., 100.], [5900., 150.], [5850., 3900.], [150., 3850.], [100., 100.]]);
    let composed = timed(&mut native, "24 MP photo, lasso selection");
    assert_eq!(composed.source.kind, SourceKind::Rasterized);
}
