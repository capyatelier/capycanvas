//! Whole-image journeys with real Mutter delivery: Image Size down with
//! Constrain proportions then Undo, Rotate Image 90° Right on a non-square
//! canvas, Reveal All after a crop and Trim, all from Edit › Image; and Image
//! Size timing on a 24 MP photo.
use super::crop::{Device, bar_widget, center, crop_ready, drag, fill_rect, setting, tap};
use super::photo_edit::{choose, document, labelled, shown, start, until_some_widget, window_point};
use super::*;
use serde_json::json;

const CONTROL: u32 = 0xffe3;

/// Every raster is captured and the canvas has nothing left to do.
fn settled(w: &Workspace) -> bool {
    let doc = document(w);
    let published = doc.layers.iter().all(|l| l.raster.try_data().is_some() && l.masks().all(|m| m.raster.try_data().is_some()));
    published && !ui_session(&w).wants_continuous_frames()
}

fn undo(input: &mut RemoteInput) {
    input.perform(json!([
        {"key": CONTROL, "down": true}, {"key": 0x7a, "down": true},
        {"key": 0x7a, "down": false}, {"key": CONTROL, "down": false}
    ]));
}

fn blue(pixel: [u8; 4]) -> bool {
    pixel[2] > 150 && pixel[0] < 100
}

/// Type `text` into a number field of the Image Size dialog with the keyboard,
/// then press Enter.
fn type_number(w: &Workspace, input: &mut RemoteInput, field: &str, text: &str) {
    let dialog = w.image_size.dialog.clone();
    let field = until_some_widget(|| find_named(dialog.upcast_ref(), &format!("image-size-{field}")), field);
    let number: crate::number_control::NumberControl = field.first_child().and_downcast().expect("a number control");
    let spin = number.first_child().and_then(|header| header.last_child()).and_downcast::<gtk::SpinButton>().expect("a spin button");
    input.click(screen_point(spin.upcast_ref(), &w.window, [0.5, 0.5]));
    let mut keys = vec![json!({"key": CONTROL, "down": true}), json!({"key": 0x61, "down": true}), json!({"key": 0x61, "down": false}), json!({"key": CONTROL, "down": false})];
    for c in text.chars() {
        let key = c as u32;
        keys.extend([json!({"key": key, "down": true}), json!({"key": key, "down": false})]);
    }
    keys.extend([json!({"key": 0xff0d, "down": true}), json!({"key": 0xff0d, "down": false})]);
    input.perform(json!(keys));
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse and keyboard delivery"]
fn native_image_size_down_with_constrain_then_undo() {
    let (_app, w, mut input) = start("art.capycanvas.ImageSize");
    let paint = fill_rect(&w, [0.2, 0.2, 0.8, 0.8]);
    let before = document(&w);
    choose(&w, &mut input, "Edit", &["Image", "Image Size…"]);
    until(|| state(&w).layer_tools.image_size.is_some(), "Edit › Image › Image Size… opens the dialog");
    let view = state(&w).layer_tools.image_size.unwrap();
    assert!(view.constrain, "Constrain proportions starts on");
    let half = [before.width / 2, before.height / 2];
    type_number(&w, &mut input, "width", &half[0].to_string());
    until(
        || state(&w).layer_tools.image_size.is_some_and(|v| v.values == half.map(f64::from)),
        "the typed width halves the height too",
    );
    if let Some(dir) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
        for theme in [Theme::Dark, Theme::Light] {
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            pump(300);
            crate::snapshot(&w).save_to_png(std::path::Path::new(&dir).join(format!("image-size-dialog-{theme:?}.png"))).unwrap();
        }
    }
    let dialog = w.image_size.dialog.clone();
    let apply = until_some_widget(|| labelled(dialog.upcast_ref(), "Apply"), "the Apply button");
    input.click(screen_point(&apply, &w.window, [0.5, 0.5]));
    until(|| state(&w).layer_tools.image_size.is_none(), "Apply closes Image Size");
    until(|| [document(&w).width, document(&w).height] == half, "the image is half the size");
    until(|| settled(&w), "the resampled pixels are captured");
    assert!(state(&w).host_error.is_none(), "{:?}", state(&w).host_error);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(400);
    assert!(blue(shown(&w, [half[0] as f32 * 0.5, half[1] as f32 * 0.5])), "the fill scales with the image");
    let scaled = document(&w);
    let tiles = scaled.layer(paint).unwrap().raster.try_data().unwrap().unwrap().tiles.len();
    assert!(tiles <= (half[0].div_ceil(256) * half[1].div_ceil(256)) as usize, "{tiles} tiles: none left in the vacated area");
    undo(&mut input);
    until(|| document(&w).width == before.width, "Ctrl+Z undoes Image Size in one step");
    assert_eq!(document(&w).height, before.height);
    input.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse and keyboard delivery"]
fn native_rotate_image_right_on_a_non_square_canvas() {
    let (_app, w, mut input) = start("art.capycanvas.RotateImage");
    let before = document(&w);
    let [width, height] = [before.width as f32, before.height as f32];
    assert_ne!(before.width, before.height, "a non-square canvas");
    fill_rect(&w, [0.05, 0.05, 0.3, 0.3]);
    let spot = [width * 0.15, height * 0.15];
    pump(300);
    assert!(blue(shown(&w, spot)), "the fill sits top left");
    choose(&w, &mut input, "Edit", &["Image", "Rotate Image 90° Right"]);
    until(|| [document(&w).width, document(&w).height] == [before.height, before.width], "the canvas turns");
    until(|| settled(&w), "the turned pixels are captured");
    assert!(state(&w).host_error.is_none(), "{:?}", state(&w).host_error);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(400);
    assert!(blue(shown(&w, [height - spot[1], spot[0]])), "the fill is now top right");
    assert!(!blue(shown(&w, [spot[1], spot[0]])), "and gone from the top left");
    undo(&mut input);
    until(|| document(&w).width == before.width, "Ctrl+Z turns it back in one step");
    input.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_reveal_all_after_a_crop() {
    let (_app, w, mut input) = crop_ready("art.capycanvas.RevealAll");
    let before = document(&w);
    let [width, height] = [before.width as f32, before.height as f32];
    drag(&mut input, Device::Mouse, window_point(&w, [0., 0.]), window_point(&w, [width * 0.4, height * 0.4]));
    until(|| setting(&w, "crop_width") < width * 0.7, "the top-left handle moves into the fill");
    tap(&mut input, Device::Mouse, center(&w, &bar_widget(&w, "canvas-bar-ApplyTransform")));
    until(|| state(&w).layer_tools.tool != LayerCanvasTool::Crop, "Apply crops");
    let cropped = document(&w);
    assert!(cropped.width < before.width);
    choose(&w, &mut input, "Edit", &["Image", "Reveal All"]);
    until(|| document(&w).width > cropped.width, "Reveal All grows the canvas");
    let revealed = document(&w);
    let expected = [(width * 0.8).round(), (height * 0.8).round()];
    for (axis, size) in [revealed.width, revealed.height].into_iter().enumerate() {
        assert!((size as f32 - expected[axis]).abs() <= 2., "the canvas reaches the fill's hidden edge: {size} vs {}", expected[axis]);
    }
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(400);
    assert!(blue(shown(&w, [4., 4.])), "the hidden fill shows again at the new top left");
    assert!(state(&w).host_error.is_none(), "{:?}", state(&w).host_error);
    undo(&mut input);
    until(|| document(&w).width == cropped.width, "one undo step");
    input.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_crop_fit_content_from_the_bar() {
    let (_app, w, mut input) = crop_ready("art.capycanvas.FitContent");
    let before = document(&w);
    let [width, height] = [before.width as f32, before.height as f32];
    let paper = before.layers.iter().find(|l| l.id == layer_core::LayerId(2)).expect("paper").id;
    w.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: paper.0, value: false } });
    until(|| !document(&w).layer(paper).unwrap().visible, "the paper hides");
    tap(&mut input, Device::Mouse, center(&w, &bar_widget(&w, "canvas-bar-CropFitContent")));
    until(|| setting(&w, "crop_width") < width * 0.7, "Fit Content frames the fill");
    for (id, expected) in [("crop_width", width * 0.6), ("crop_height", height * 0.6)] {
        assert!((setting(&w, id) - expected).abs() <= 3., "{id}: {} vs {expected}", setting(&w, id));
    }
    if let Some(dir) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
        pump(200);
        crate::snapshot(&w).save_to_png(std::path::Path::new(&dir).join("crop-fit-content.png")).unwrap();
    }
    tap(&mut input, Device::Mouse, center(&w, &bar_widget(&w, "canvas-bar-ApplyTransform")));
    until(|| state(&w).layer_tools.tool != LayerCanvasTool::Crop, "Apply crops to the fill");
    assert!((document(&w).width as f32 - width * 0.6).abs() <= 3.);
    assert!(state(&w).host_error.is_none(), "{:?}", state(&w).host_error);
    input.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_trim_to_the_visible_pixels() {
    let (_app, w, mut input) = start("art.capycanvas.Trim");
    fill_rect(&w, [0.2, 0.25, 0.7, 0.8]);
    let before = document(&w);
    let [width, height] = [before.width as f32, before.height as f32];
    let paper = before.layers.iter().find(|l| l.id == layer_core::LayerId(2)).expect("paper").id;
    choose(&w, &mut input, "Edit", &["Image", "Trim"]);
    until(|| state(&w).notice.is_some(), "with the paper showing, Trim explains that nothing changes");
    assert_eq!(document(&w).width, before.width);
    w.dispatch(UiAction::Layer { action: LayerAction::Visibility { id: paper.0, value: false } });
    until(|| !document(&w).layer(paper).unwrap().visible, "the paper hides");
    choose(&w, &mut input, "Edit", &["Image", "Trim"]);
    until(|| document(&w).width < before.width, "Trim shrinks the canvas to the fill");
    let trimmed = document(&w);
    let expected = [(width * 0.5).round(), (height * 0.55).round()];
    for (axis, size) in [trimmed.width, trimmed.height].into_iter().enumerate() {
        assert!((size as f32 - expected[axis]).abs() <= 2., "{size} vs {}", expected[axis]);
    }
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(400);
    for at in [[2., 2.], [trimmed.width as f32 - 3., trimmed.height as f32 - 3.]] {
        assert!(blue(shown(&w, at)), "the fill reaches the trimmed edges at {at:?}");
    }
    assert!(state(&w).host_error.is_none(), "{:?}", state(&w).host_error);
    undo(&mut input);
    until(|| document(&w).width == before.width, "one undo step");
    input.finish();
    w.window.close();
    pump(50);
}

/// A 6000 × 4000 16-bit drawing whose only layer is painted everywhere.
fn painted_24_mp() -> layer_core::Project {
    use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TILE_SIZE, TileBlob, TileKey};
    let extent = [6000u32, 4000];
    let mut project = layer_ui::new_drawing(1, 1, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let doc = &mut project.document;
    [doc.width, doc.height] = extent;
    doc.color = layer_core::color::DocumentColor {
        space: layer_core::color::RgbSpace::ProPhoto,
        depth: layer_core::color::SampleDepth::U16,
    };
    doc.layers.retain(|l| l.id != layer_core::LayerId(2));
    let descriptor = RasterPlane::Color.descriptor(doc.color);
    let tiles = (0..extent[1].div_ceil(TILE_SIZE))
        .flat_map(|ty| (0..extent[0].div_ceil(TILE_SIZE)).map(move |tx| [tx, ty]))
        .map(|coordinate| {
            let mut bytes = Vec::with_capacity(descriptor.byte_len([TILE_SIZE; 2]).unwrap());
            for y in 0..TILE_SIZE {
                for x in 0..TILE_SIZE {
                    let [gx, gy] = [coordinate[0] * TILE_SIZE + x, coordinate[1] * TILE_SIZE + y];
                    for code in [(gx * 10) as u16, (gy * 16) as u16, ((gx ^ gy) * 97) as u16, 65535] {
                        bytes.extend_from_slice(&code.to_le_bytes());
                    }
                }
            }
            (TileKey { plane: RasterPlane::Color, coordinate }, RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()))
        })
        .collect();
    doc.layers[0].raster = RasterRevision::backed(RasterData { tiles, watercolor: None });
    project.validate(Default::default()).unwrap();
    project
}

/// Image Size from 6000 × 4000 to `size`, timed from the command to settled
/// pixels.
fn time_image_size(w: &Rc<Workspace>, size: [u32; 2], label: &str) {
    until(|| settled(w), "the drawing is ready");
    let started = Instant::now();
    w.dispatch(UiAction::Invoke { command: CommandId::ImageSize });
    for action in [
        layer_ui::ImageSizeAction::Constrain { constrain: false },
        layer_ui::ImageSizeAction::Width { value: f64::from(size[0]) },
        layer_ui::ImageSizeAction::Height { value: f64::from(size[1]) },
        layer_ui::ImageSizeAction::Apply,
    ] {
        w.dispatch(UiAction::ImageSize { action });
    }
    let applied = started.elapsed();
    until(|| [document(w).width, document(w).height] == size && settled(w), label);
    let finished = started.elapsed();
    let tiles: usize = document(w).layers.iter().map(|l| l.raster.try_data().unwrap().unwrap().tiles.len()).sum();
    eprintln!(
        "image size timing: {label}: {:.1} ms on the UI thread, settled after {:.0} ms, {tiles} tiles",
        applied.as_secs_f64() * 1e3,
        finished.as_secs_f64() * 1e3
    );
    assert!(state(w).host_error.is_none(), "{:?}", state(w).host_error);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(w).width == 6000 && settled(w), "undo");
}

/// Image Size on a 24 MP photo, which scales its placement losslessly, and on
/// a 24 MP 16-bit paint layer, which resamples on the GPU. Prints the times.
#[test]
#[ignore = "hardware Wayland benchmark: run separately in release with --ignored --test-threads=1"]
fn native_image_size_timing_on_a_24_mp_photo() {
    let app = native_test_app("dev.layer.ImageSizeTiming");
    let mut photo = native_navigation::photo([6000, 4000]);
    photo.document.layers.retain(|layer| layer.source.is_some() || layer.id == layer_core::LayerId(2));
    for (project, name) in [(photo, "placed photo"), (painted_24_mp(), "16-bit paint layer")] {
        let w = Workspace::with_project(&app, Some((project, None)));
        w.window.maximize();
        w.window.present();
        pump(1500);
        for size in [[3000, 2000], [4000, 2667]] {
            time_image_size(&w, size, &format!("{name} 6000x4000 to {}x{}", size[0], size[1]));
        }
        w.window.close();
        pump(200);
    }
}
