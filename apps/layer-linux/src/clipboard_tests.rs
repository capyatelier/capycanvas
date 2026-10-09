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
const INSERT: u32 = 0xff63;
const DELETE: u32 = 0xffff;

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
        chord(&mut native, &[CONTROL], INSERT);
        copied(&w, before, "Ctrl+Insert")
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
        chord(&mut native, &[SHIFT], DELETE);
        copied(&w, before, "Shift+Delete")
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

    let missing = layer_core::temp_files::directory().unwrap().join(format!("missing-clipboard-{}.png", layer_core::PortableId::random()));
    let uri = format!("{}\r\n", gtk::gio::File::for_path(missing).uri());
    let provider = gdk::ContentProvider::new_union(&[
        gdk::ContentProvider::for_bytes("text/uri-list", &glib::Bytes::from(uri.as_bytes())),
        gdk::ContentProvider::for_bytes("image/tiff", &glib::Bytes::from_static(b"broken preferred TIFF")),
        gdk::ContentProvider::for_bytes("image/png", &glib::Bytes::from(&png[..])),
    ]);
    w.window.clipboard().set_content(Some(&provider)).unwrap();
    until(|| !clipboard_formats(&w).iter().any(|m| m == CLIP_MIME), "the clipboard now holds another application's image");
    let layers = document(&w).scene().order().len();
    chord(&mut native, &[CONTROL], 0x76);
    until(|| document(&w).scene().order().len() == layers + 1, "an image from another app pastes");
    until(|| state(&w).canvas_bar.is_some_and(|b| b.context.kind == layer_ui::CanvasBarKind::Placement), "failed file list and TIFF fall back to PNG with placement handles");
    assert!(state(&w).host_error.is_none());
    w.dispatch(UiAction::Invoke { command: CommandId::CancelTransform });
    until(|| document(&w).scene().order().len() == layers && idle(&w), "cancelling the placement removes it");
    chord(&mut native, &[CONTROL, SHIFT], 0x76);
    until(|| document(&w).scene().order().len() == layers + 1 && idle(&w), "Paste to Shown Position of another app's image");
    assert!(state(&w).canvas_bar.is_none_or(|b| b.context.kind != layer_ui::CanvasBarKind::Placement), "centred without handles");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).scene().order().len() == layers, "one undo step");

    let before_dialog = document(&w).artwork.clone();
    let previous = nonce();
    let dialog = adw::AlertDialog::builder().heading("Clipboard focus").build();
    dialog.add_response("close", "Close");
    dialog.present(Some(&w.window));
    pump(150);
    for key in [0x63, 0x78, 0x76] { chord(&mut native, &[CONTROL], key); }
    pump(300);
    assert_eq!(document(&w).artwork, before_dialog, "a native dialog owns Copy, Cut and Paste");
    assert_eq!(nonce(), previous);
    assert!(state(&w).requests.is_empty());
    dialog.close();
    pump(150);
    w.area.grab_focus();

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
    chord(&mut native, &[CONTROL], INSERT);
    let text = glib::MainContext::default().block_on(w.window.clipboard().read_text_future()).unwrap();
    assert_eq!(text.as_deref(), Some("Typed name"), "the focused field copies its text");
    assert!(state(&w).requests.is_empty() && nonce() == before, "Ctrl+Insert in a text field copies no pixels");
    entry.set_text("");
    chord(&mut native, &[SHIFT], INSERT);
    until(|| entry.text() == "Typed name", "Shift+Insert pastes text into the field");
    assert!(state(&w).requests.is_empty());
    entry.select_region(0, -1);
    chord(&mut native, &[SHIFT], DELETE);
    until(|| entry.text().is_empty(), "Shift+Delete cuts only focused text");
    assert!(state(&w).requests.is_empty() && nonce() == before);
    chord(&mut native, &[SHIFT], INSERT);
    until(|| entry.text() == "Typed name", "Shift+Insert pastes the cut text");
    entry.emit_activate();
    pump(200);

    let path = layer_core::temp_files::directory().unwrap().join(format!("clipboard-file-{}.png", layer_core::PortableId::random()));
    std::fs::write(&path, &png).unwrap();
    external_copy_type(format!("{}\r\n", gtk::gio::File::for_path(&path).uri()).as_bytes(), "text/uri-list");
    until(|| clipboard_formats(&w).iter().any(|mime| mime == "text/uri-list"), "a copied file is offered");
    let count = document(&w).scene().order().len();
    chord(&mut native, &[SHIFT], INSERT);
    until(|| document(&w).scene().order().len() == count + 1 && !state(&w).document_file.busy, "Shift+Insert pastes a copied image file");
    assert!(state(&w).canvas_bar.is_some_and(|bar| bar.context.kind == layer_ui::CanvasBarKind::Placement));
    w.dispatch(UiAction::Invoke { command: CommandId::CancelTransform });
    until(|| document(&w).scene().order().len() == count && idle(&w), "copied-file placement cancels");
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

    let before = nonce();
    chord(&mut native, &[CONTROL], 0x63);
    assert!(copied(&w, before, "retain a whole layer for another colour mode").layers.is_some());
    let copied_layer = active_occurrence(&document(&w)).clone();
    let color = layer_core::color::DocumentColor { space: layer_core::color::RgbSpace::DisplayP3, depth: layer_core::color::SampleDepth::U16 };
    let mut project = new_drawing(64, 48, &w.localization()).unwrap();
    composition_mut(&mut project).color = color;
    let destination = Workspace::with_project(&app, Some((project, None)));
    apply_fixture_theme(&destination);
    destination.window.present();
    until(|| destination.gpu.borrow().as_ref().is_some_and(|g| g.session.require_document_idle().is_ok()), "the retained-layer colour destination");
    pump(600);
    destination.area.grab_focus();
    let count = document(&destination).scene().order().len();
    chord(&mut native, &[CONTROL], 0x76);
    until(|| document(&destination).scene().order().len() == count + 1 && idle(&destination), "retained whole-layer paste across colour and depth");
    let pasted = document(&destination);
    assert_eq!(pasted.composition().color, color);
    let layer = active_occurrence(&pasted);
    assert_eq!((&layer.name, layer.opacity, layer.blend, layer.offset), (&copied_layer.name, copied_layer.opacity, copied_layer.blend, copied_layer.offset));
    assert!(state(&destination).canvas_bar.is_none_or(|b| b.context.kind != layer_ui::CanvasBarKind::Placement));
    destination.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&destination).scene().order().len() == count, "cross-colour retained Paste is one undo step");
    clipboard_regions_and_masks(&destination, &mut native);
    destination.window.destroy();
}

fn clipboard_regions_and_masks(w: &Rc<Workspace>, native: &mut RemoteInput) {
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    let first = document(w).working.occurrence.unwrap();
    w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
    w.dispatch(UiAction::SetColor { rgba: [0.8, 0.1, 0.2, 1.] });
    w.dispatch(UiAction::Invoke { command: CommandId::FillSelection });
    w.dispatch(UiAction::Invoke { command: CommandId::AddLayer });
    let second = document(w).working.occurrence.unwrap();
    w.dispatch(UiAction::SetColor { rgba: [0.1, 0.7, 0.2, 1.] });
    w.dispatch(UiAction::Invoke { command: CommandId::FillSelection });
    w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    native_pen_path(w, &[[8., 8.], [40., 8.], [40., 32.], [8., 32.], [8., 8.]]);
    w.dispatch(UiAction::Layer { action: LayerAction::ToggleSelection { id: layer_ui::occurrence_token(first) } });
    let before = document(w);
    assert_eq!(before.working.layer_selection.len(), 2);
    let previous = nonce();
    chord(native, &[CONTROL], 0x63);
    let clip = copied(w, previous, "selected regions Copy");
    let retained = clip.layers.as_ref().expect("separate selected layers");
    assert_eq!(retained.roots.len(), 2);
    for &id in &retained.roots {
        assert_eq!(retained.scene.view().paint_source(id).unwrap().base.as_ref().unwrap().image.extent, clip.source.extent);
    }
    let count = before.scene().order().len();
    chord(native, &[CONTROL], 0x76);
    until(|| document(w).scene().order().len() == count + 2 && idle(w), "selected regions Paste preserves separate layers");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(w).scene().order().len() == count, "region Paste is one undo");
    let previous = nonce();
    chord(native, &[CONTROL], 0x78);
    copied(w, previous, "selected regions Cut");
    until(|| [first, second].iter().all(|&id| document(w).scene().paint_source(id).unwrap().raster.identity() != before.scene().paint_source(id).unwrap().raster.identity()) && idle(w), "Cut erases both regions");
    assert_eq!(document(w).scene().order().len(), count);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| [first, second].iter().all(|&id| document(w).scene().paint_source(id) == before.scene().paint_source(id) && document(w).scene().occurrence(id) == before.scene().occurrence(id)), "one Undo restores both cut regions");
    assert_eq!(document(w).working.layer_selection, before.working.layer_selection);
    assert_eq!(document(w).working.occurrence, before.working.occurrence);
    assert_eq!(document(w).working.target, before.working.target);
    w.dispatch(UiAction::Layer { action: LayerAction::GroupSelected });
    let grouped = document(w);
    let previous = nonce();
    chord(native, &[CONTROL], 0x63);
    let clip = copied(w, previous, "group region Copy");
    let retained = clip.layers.as_ref().unwrap();
    assert_eq!(retained.roots.len(), 1);
    assert_eq!(retained.scene.view().occurrence(retained.roots[0]).unwrap().kind(), layer_core::LayerKind::Group);
    assert_eq!(clip.layers.as_ref().unwrap().scene.view().order().len(), 3);
    chord(native, &[CONTROL], 0x76);
    until(|| document(w).scene().order().len() == count + 4 && idle(w), "group region Paste preserves hierarchy");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| [first, second].iter().all(|&id| document(w).scene().paint_source(id) == grouped.scene().paint_source(id) && document(w).scene().occurrence(id) == grouped.scene().occurrence(id)), "group region Paste is one undo");
    let previous = nonce();
    chord(native, &[CONTROL], 0x78);
    copied(w, previous, "group region Cut");
    until(|| [first, second].iter().all(|&id| document(w).scene().paint_source(id).unwrap().raster.identity() != grouped.scene().paint_source(id).unwrap().raster.identity()) && idle(w), "group Cut erases both child regions");
    assert_eq!(document(w).scene().order().len(), count + 1);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| [first, second].iter().all(|&id| document(w).scene().paint_source(id) == grouped.scene().paint_source(id) && document(w).scene().occurrence(id) == grouped.scene().occurrence(id)), "group region Cut is one undo");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(w).scene().order().len() == count, "restore ungrouped source");
    w.dispatch(UiAction::Layer { action: LayerAction::Select { id: layer_ui::occurrence_token(second), mask: false } });
    w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
    w.dispatch(UiAction::Layer { action: LayerAction::AddMask { id: layer_ui::occurrence_token(second), replace: false } });
    w.dispatch(UiAction::Layer { action: LayerAction::Select { id: layer_ui::occurrence_token(second), mask: true } });
    let masked = document(w);
    let mask = masked.scene().occurrence(second).unwrap().mask.as_ref().unwrap().source;
    let previous = nonce();
    chord(native, &[CONTROL], 0x63);
    let clip = copied(w, previous, "focused mask Copy");
    assert!(clip.layers.is_none());
    chord(native, &[CONTROL], 0x78);
    copied(w, Some(clip.nonce), "focused mask Cut");
    until(|| document(w).artwork.coverage.get(mask).unwrap().raster.identity() != masked.artwork.coverage.get(mask).unwrap().raster.identity() && idle(w), "Cut edits the focused mask");
    assert_eq!(document(w).scene().paint_source(second), masked.scene().paint_source(second));
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(w).artwork.coverage.get(mask).unwrap().raster.identity() == masked.artwork.coverage.get(mask).unwrap().raster.identity(), "mask Cut is one undo");
    chord(native, &[CONTROL], 0x76);
    until(|| document(w).artwork.coverage.get(mask).unwrap().raster.identity() != masked.artwork.coverage.get(mask).unwrap().raster.identity() && idle(w), "retained Paste edits the focused mask");
    assert_eq!(document(w).scene().order().len(), count);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(w).artwork.coverage.get(mask).unwrap().raster.identity() == masked.artwork.coverage.get(mask).unwrap().raster.identity(), "mask Paste is one undo");
    let source = layer_core::color::source::rgba8_source([32, 24], |_, _| [128, 128, 128, 255]);
    let mut png = Vec::new();
    layer_color::photo::write_png(&mut png, &source).unwrap();
    external_copy(&png);
    chord(native, &[CONTROL], 0x76);
    until(|| document(w).artwork.coverage.get(mask).unwrap().raster.identity() != masked.artwork.coverage.get(mask).unwrap().raster.identity() && idle(w), "external PNG Paste edits the focused mask");
    assert_eq!(document(w).scene().order().len(), count);
    assert_eq!(document(w).scene().paint_source(second), masked.scene().paint_source(second));
    assert!(state(w).canvas_bar.is_none_or(|b| b.context.kind != layer_ui::CanvasBarKind::Placement));
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(w).artwork.coverage.get(mask).unwrap().raster.identity() == masked.artwork.coverage.get(mask).unwrap().raster.identity(), "external mask Paste is one undo");
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
