//! Pixel clipboard journeys: Copy and Paste within a drawing, between
//! drawings and with other applications, from the keyboard and the bar.
use super::canvas_bar_tests::{Device, choose_from_bar_menu, document, filled_selection, remote_input, until_some};
use super::*;
use crate::files::clipboard::{CLIP_MIME, current};
use layer_core::PaintBasePolicy;
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
fn external_copy(png: &[u8]) { external_copy_type(png, "image/png"); }

fn external_copy_type(bytes: &[u8], mime: &str) {
    let mut child = std::process::Command::new("wl-copy")
        .args(["--type", mime])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("wl-copy");
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), bytes).unwrap();
    until(|| child.try_wait().unwrap().is_some(), "wl-copy offers the image");
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
    let paint = document(&w).working.occurrence.unwrap();
    filled_selection(&w, layer_ui::occurrence_token(paint));
    let mut native = remote_input();

    let clip = {
        let before = nonce();
        chord(&mut native, &[CONTROL], 0x63);
        copied(&w, before, "Ctrl+C")
    };
    let selection = document(&w).working.selection.clone().unwrap().coverage_bounds();
    assert_eq!(clip.origin, [selection.min.x.floor() as i64, selection.min.y.floor() as i64]);
    assert_eq!(clip.policy, PaintBasePolicy::WorkingPixels);
    let formats = clipboard_formats(&w);
    assert!(formats.iter().any(|m| m == "image/png") && formats.iter().any(|m| m == CLIP_MIME), "{formats:?}");
    let png = external_png();
    let photo = layer_color::photo::read_photo(std::io::Cursor::new(&png), Default::default()).unwrap();
    assert_eq!(photo.extent, clip.source.extent, "another application reads the copy as a PNG");
    w.window.present();
    pump(300);

    let layers = document(&w).scene().order().len();
    chord(&mut native, &[CONTROL], 0x76);
    until(|| document(&w).scene().order().len() == layers + 1 && idle(&w), "Ctrl+V pastes a new layer");
    let pasted = document(&w);
    assert_source_samples(active_paint(&pasted).base.as_ref().map(|base|base.image.as_ref()).unwrap(), clip.source_for(document(&w).composition().color).image.as_ref());
    assert_eq!(active_occurrence(&pasted).offset, clip.origin.map(i64::from), "at the copied position");
    assert!(state(&w).canvas_bar.is_none_or(|b| b.context.kind != layer_ui::CanvasBarKind::Placement), "no handles");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).scene().order().len() == layers, "one undo step");

    let merged = {
        let before = nonce();
        chord(&mut native, &[CONTROL, SHIFT], 0x63);
        copied(&w, before, "Ctrl+Shift+C")
    };
    assert_eq!(merged.name, "Merged copy");

    let erased = document(&w).scene().paint_source(paint).unwrap().raster.identity();
    let cut = {
        let before = nonce();
        chord(&mut native, &[CONTROL], 0x78);
        copied(&w, before, "Ctrl+X")
    };
    until(|| document(&w).scene().paint_source(paint).unwrap().raster.identity() != erased, "Cut erases the copied pixels");
    assert_eq!(cut.source.extent, clip.source.extent);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });

    for device in [Device::Mouse, Device::Touch] {
        filled_selection(&w, layer_ui::occurrence_token(paint));
        let before = nonce();
        choose_from_bar_menu(&w, &mut native, device, CanvasBarMenu::Copy, &["Copy Merged"]);
        assert_eq!(copied(&w, before, &format!("{device:?}: Copy ▾ › Copy Merged")).name, "Merged copy");
        let layers = document(&w).scene().order().len();
        w.dispatch(UiAction::Invoke { command: CommandId::PasteInto });
        until(|| document(&w).scene().order().len() == layers + 1 && idle(&w), &format!("{device:?}: Paste Into"));
        let mask = |into: &layer_core::Document| into.artwork.coverage.get(active_occurrence(into).mask.as_ref().expect("Paste Into adds a mask").source).unwrap().raster.clone();
        until(|| mask(&document(&w)).try_data().is_some(), &format!("{device:?}: the mask's pixels are stored"));
        assert!(mask(&document(&w)).try_data().is_some_and(|data| data.is_ok_and(|data| !data.tiles.is_empty())), "a mask from the selection");
        assert!(document(&w).working.selection.is_none());
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        until(|| document(&w).scene().order().len() == layers && document(&w).working.selection.is_some(), "Paste Into undoes in one step");
    }

    let mut other = new_drawing(640, 480, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    composition_mut(&mut other).color = layer_core::color::DocumentColor {
        space: layer_core::color::RgbSpace::DisplayP3,
        depth: layer_core::color::SampleDepth::U16,
    };
    let second = Workspace::with_project(&app, Some((other, None)));
    apply_fixture_theme(&second);
    second.window.present();
    until(|| second.gpu.borrow().as_ref().is_some_and(|g| g.session.require_document_idle().is_ok()), "the second drawing");
    pump(600);
    let copy = current().unwrap();
    chord(&mut native, &[CONTROL, SHIFT], 0x76);
    until(|| document(&second).scene().order().len() == 3 && idle(&second), "Paste to Shown Position into another drawing");
    let pasted = document(&second);
    assert_eq!(active_paint(&pasted).base.as_ref().unwrap().policy, PaintBasePolicy::SourceProfile, "another colour mode keeps an explicit profile");
    let camera = state(&second).camera;
    let centre = camera.surface_to_document64(camera.work_area_center().map(f64::from));
    assert_eq!(active_occurrence(&pasted).offset, std::array::from_fn(|i| (centre[i] - f64::from(copy.source.extent[i]) / 2.).round() as i64));
    second.window.destroy();
    w.window.present();
    pump(400);

    external_copy(&png);
    until(|| !clipboard_formats(&w).iter().any(|m| m == CLIP_MIME), "the clipboard now holds another application's image");
    let layers = document(&w).scene().order().len();
    chord(&mut native, &[CONTROL], 0x76);
    until(|| document(&w).scene().order().len() == layers + 1, "an image from another app pastes");
    until(|| state(&w).canvas_bar.is_some_and(|b| b.context.kind == layer_ui::CanvasBarKind::Placement), "with placement handles");
    w.dispatch(UiAction::Invoke { command: CommandId::CancelTransform });
    until(|| document(&w).scene().order().len() == layers && idle(&w), "cancelling the placement removes it");
    chord(&mut native, &[CONTROL, SHIFT], 0x76);
    until(|| document(&w).scene().order().len() == layers + 1 && idle(&w), "Paste to Shown Position of another app's image");
    assert!(state(&w).canvas_bar.is_none_or(|b| b.context.kind != layer_ui::CanvasBarKind::Placement), "centred without handles");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).scene().order().len() == layers, "one undo step");

    w.dispatch(UiAction::Layer { action: LayerAction::BeginRename { id: layer_ui::occurrence_token(paint) } });
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

    let path = layer_core::temp_files::directory().unwrap().join(format!("clipboard-file-{}.png", layer_core::PortableId::random()));
    std::fs::write(&path, &png).unwrap();
    external_copy_type(format!("{}\r\n", gtk::gio::File::for_path(&path).uri()).as_bytes(), "text/uri-list");
    until(|| clipboard_formats(&w).iter().any(|mime| mime == "text/uri-list"), "a copied file is offered");
    let owner = document(&w).owner;
    w.dispatch(UiAction::Invoke { command: CommandId::PasteAsNewImage });
    until(|| w.gpu.borrow().is_some() && !w.documents.changing.get() && document(&w).owner != owner && idle(&w), "Paste as New Image opens a copied file in another tab");
    assert_eq!(document(&w).composition().size, photo.extent);
    assert!(state(&w).document_file.modified);
    assert!(state(&w).document_file.location.is_none());
    assert_eq!(w.documents.model.borrow().order().len(), 2);
    let before = nonce();
    chord(&mut native, &[CONTROL], 0x63);
    let clip = copied(&w, before, "copy the newly opened image");
    let owner = document(&w).owner;
    w.dispatch(UiAction::Invoke { command: CommandId::PasteAsNewImage });
    until(|| w.gpu.borrow().is_some() && !w.documents.changing.get() && document(&w).owner != owner && idle(&w), "Paste as New Image opens the retained copy");
    assert_eq!(document(&w).composition().size, clip.source.extent);
    assert_source_samples(active_paint(&document(&w)).base.as_ref().unwrap().image.as_ref(), &clip.source);
    assert_eq!(w.documents.model.borrow().order().len(), 3);
    std::fs::remove_file(path).unwrap();

    let original = document(&w).working.occurrence.unwrap();
    w.dispatch(UiAction::SetLayerOpacity { id: Some(layer_ui::occurrence_token(original)), opacity: 0.37 });
    let original_doc = document(&w);
    let original_layer = active_occurrence(&original_doc).clone();
    let layers = original_doc.scene().order().len();
    assert!(original_doc.working.selection.is_none());
    let before = nonce();
    chord(&mut native, &[CONTROL], 0x63);
    let whole = copied(&w, before, "whole-layer Copy without a selection");
    let retained = whole.layers.as_ref().unwrap();
    assert_eq!(retained.roots.len(), 1);
    assert_eq!(retained.scene.view().order().len(), 1);
    let retained_layer = retained.scene.view().occurrence(retained.roots[0]).unwrap();
    assert_eq!((&retained_layer.name, retained_layer.opacity, retained_layer.blend, retained_layer.offset),
        (&original_layer.name, original_layer.opacity, original_layer.blend, original_layer.offset));
    chord(&mut native, &[CONTROL], 0x76);
    until(|| document(&w).scene().order().len() == layers + 1 && idle(&w), "whole-layer Paste");
    let pasted = document(&w);
    let pasted_layer = active_occurrence(&pasted);
    assert_eq!((&pasted_layer.name, pasted_layer.opacity, pasted_layer.blend, pasted_layer.offset),
        (&original_layer.name, original_layer.opacity, original_layer.blend, original_layer.offset));
    assert_source_samples(active_paint(&pasted).base.as_ref().unwrap().image.as_ref(), active_paint(&original_doc).base.as_ref().unwrap().image.as_ref());
    let pasted_id = pasted.working.occurrence.unwrap();
    let before = nonce();
    chord(&mut native, &[CONTROL], 0x78);
    assert!(copied(&w, before, "whole-layer Cut without a selection").layers.is_some());
    until(|| document(&w).scene().order().len() == layers && idle(&w), "Cut removes the copied layer");
    assert!(document(&w).scene().occurrence(pasted_id).is_none());
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).scene().order().len() == layers + 1, "Undo restores the cut layer");
    assert_eq!(document(&w).scene().occurrence(pasted_id).unwrap(), pasted_layer);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).scene().order().len() == layers, "Undo removes the pasted layer");

    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    let point = screen_point(w.area.upcast_ref(), &w.window, [0.5, 0.5]);
    native.perform(json!([{"point": point}, {"wheel": [0, 1]}]));
    let camera = state(&w).camera;
    let centre = camera.surface_to_document64(camera.work_area_center().map(f64::from));
    let expected: [i64; 2] = std::array::from_fn(|i| (centre[i] - f64::from(whole.source.extent[i]) / 2.).round() as i64 + original_layer.offset[i] - whole.origin[i]);
    assert_ne!(expected, original_layer.offset, "the view has panned away from the copied position");
    chord(&mut native, &[CONTROL, SHIFT], 0x76);
    until(|| document(&w).scene().order().len() == layers + 1 && idle(&w), "whole layers paste to the shown position");
    assert_eq!(active_occurrence(&document(&w)).offset, expected);
    assert!(state(&w).canvas_bar.is_none_or(|b| b.context.kind != layer_ui::CanvasBarKind::Placement));
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).scene().order().len() == layers, "centred Paste undoes in one step");

    let extent = document(&w).composition().size.map(|v| v + 64);
    let source = layer_core::color::source::rgba8_source(extent, |_, _| [224, 64, 16, 255]);
    let mut oversized = Vec::new();
    layer_color::photo::write_png(&mut oversized, &source).unwrap();
    external_copy(&oversized);
    until(|| !clipboard_formats(&w).iter().any(|m| m == CLIP_MIME), "an oversized external image is offered");
    chord(&mut native, &[CONTROL], 0x76);
    until(|| state(&w).canvas_bar.is_some_and(|b| b.context.kind == layer_ui::CanvasBarKind::Placement), "oversized external Paste shows handles");
    let pasted = document(&w);
    let object = pasted.object_layer_children(pasted.working.occurrence.unwrap()).unwrap()[0];
    assert_eq!(pasted.scene().object(object).unwrap().image.extent, extent);
    assert_eq!(pasted.object_document_affine(object).unwrap().0[..4], [1., 0., 0., 1.], "external Paste keeps full pixel size beyond the canvas");
    w.dispatch(UiAction::Invoke { command: CommandId::CancelTransform });
    until(|| document(&w).scene().order().len() == layers && idle(&w), "oversized placement cancels completely");
}

#[test]
#[ignore = "isolated compositor, GPU and native keyboard delivery; prints copy latency"]
fn native_clipboard_copy_latency_24mp() {
    let [width, height] = [6000u32, 4000];
    let mut project = new_drawing(width, height, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
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
    paint_at_mut(&mut project, 0).base = Some(layer_core::PaintBase::new((std::sync::Arc::new(builder.finish().unwrap())).into()));
    let app = native_test_app("art.capycanvas.ClipboardLatency");
    let w = Workspace::with_project(&app, Some((project, None)));
    apply_fixture_theme(&w);
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
    let photo = paint_at(&document(&w), 0).base.as_ref().unwrap().image.storage().clone();
    assert!(std::sync::Arc::ptr_eq(&whole.source, &photo), "an untouched photo keeps its original samples");
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    native_pen_path(&w, &[[100., 100.], [5900., 150.], [5850., 3900.], [150., 3850.], [100., 100.]]);
    let composed = timed(&mut native, "24 MP photo, lasso selection");
    assert_eq!(composed.policy, PaintBasePolicy::WorkingPixels);
    w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
    let layers = timed(&mut native, "24 MP photo, whole layer");
    assert!(layers.layers.is_some());
    assert_eq!(layers.source.extent, [width, height]);
}
