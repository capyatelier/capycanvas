//! Image layers on the private display: list rows, canvas picking, transform
//! handles, ordering, visibility, duplication, deletion and painting refusals
//! with real mouse, keyboard and touch input.
use super::canvas_bar_tests::{center, document, remote_input};
use super::new_photo::{capture_ui, invoke, ready};
use super::*;
use layer_core::authored::{Affine64, ImageObjectHandle, OccurrenceHandle};
use layer_ui::{LayerCanvasTool, NoticeActionId};
use serde_json::json;

const SHIFT: u32 = 0xffe1;
const CONTROL: u32 = 0xffe3;
const RIGHT: u32 = 273;

fn solid(extent: [u32; 2], rgba: [u8; 4]) -> layer_core::authored::Image {
    layer_core::color::source::rgba8_source(extent, move |_, _| rgba).into()
}

/// A raster layer below separate blue and red Object layers.
fn fixture() -> (layer_core::Document, OccurrenceHandle, [ImageObjectHandle; 2]) {
    let mut doc = new_drawing(640, 480, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let mut handles = Vec::new();
    let mut layers = Vec::new();
    for (index, (name, rgba, offset)) in [("Blue", [30, 60, 220, 255], [180., 140.]), ("Red", [220, 40, 30, 255], [100., 100.])].into_iter().enumerate() {
        let mut object = layer_core::ImageObject::new(solid([200, 150], rgba));
        object.affine = Affine64([1., 0., 0., 1., offset[0], offset[1]]);
        let (layer, edit) = doc.create_object_layer_edit(name, object, None, index).unwrap();
        doc.apply(edit).unwrap();
        handles.push(doc.scene().object_handle(layer).unwrap()); layers.push(layer);
    }
    let layer = layers[1];
    doc.working.occurrence = Some(layer); doc.working.target = None;
    doc.working.layer_selection = [layer].into(); doc.working.layer_anchor = Some(layer);
    (doc, layer, [handles[1], handles[0]])
}

fn selected(w: &Workspace) -> Vec<ImageObjectHandle> { document(w).selected_objects().into_iter().collect() }
fn affine(w: &Workspace, object: ImageObjectHandle) -> Affine64 { document(w).scene().object(object).unwrap().affine }
fn image_order(w: &Workspace) -> Vec<ImageObjectHandle> { let doc = document(w); doc.scene().order().iter().filter_map(|&id| doc.scene().object_handle(id)).collect() }

fn mapped(w: &Workspace, name: &str) -> gtk::Widget {
    super::canvas_bar_tests::until_some(
        || widgets(w.window.upcast_ref()).find(|widget| widget.widget_name() == name && widget.is_mapped()),
        name,
    )
}

fn object_row(w: &Workspace, object: ImageObjectHandle) -> gtk::Widget {
    mapped(w, &format!("art-layer-{}", layer_ui::occurrence_token(document(w).scene().object_owner(object).unwrap())))
}

fn row_child(row: &gtk::Widget, class: &str) -> gtk::Widget {
    widgets(row).find(|widget| widget.has_css_class(class) && widget.is_mapped()).unwrap_or_else(|| panic!("{class}"))
}

fn doc_point(w: &Workspace, point: [f64; 2]) -> [f32; 2] {
    super::canvas_bar_tests::canvas_point(w, [point[0] as f32, point[1] as f32])
}

fn settle(w: &Rc<Workspace>) {
    ready(w);
    pump(100);
}

fn capture(w: &Rc<Workspace>, name: &str) {
    let Some(output) = std::env::var_os("LAYER_TEST_ARTIFACTS") else { return };
    let output = std::path::PathBuf::from(output);
    std::fs::create_dir_all(&output).unwrap();
    let theme = std::env::var("CAPY_NATIVE_TEST_THEME").unwrap_or_else(|_| "default".into());
    capture_ui(w, &output, &format!("{name}-{theme}-{}.png", w.window.width()));
}

#[test]
#[ignore = "isolated compositor, GPU and native keyboard, mouse and touch delivery"]
fn native_image_object_rows_picking_and_transforms() {
    let app = native_test_app("art.capycanvas.ImageObjects");
    let (project, _layer, [red, blue]) = fixture();
    let w = Workspace::with_project(&app, Some((project, None)));
    apply_fixture_theme(&w);
    w.window.present();
    w.window.maximize();
    settle(&w);
    invoke(&w, CommandId::FitCanvas);
    if state(&w).commands.iter().any(|c| c.id == CommandId::TransformSnapping && c.selected) { invoke(&w, CommandId::TransformSnapping); }
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Tool { tool: LayerCanvasTool::Move } });
    settle(&w);
    let mut native = remote_input();

    let row = object_row(&w, red);
    assert!(widgets(&row).filter_map(|widget| widget.downcast::<gtk::Label>().ok()).any(|label| label.text() == "Red"));
    assert_eq!(state(&w).layers.iter().filter(|row| row.object).count(), 2);
    until(|| widgets(&object_row(&w, blue)).filter_map(|widget| widget.downcast::<gtk::Picture>().ok()).any(|p| p.paintable().is_some()), "Object layers show their own previews");
    capture(&w, "image-rows");

    native.click(doc_point(&w, [120., 120.]));
    until(|| selected(&w) == vec![red], "clicking the red image selects it");
    native.perform(json!([{"key":SHIFT,"down":true},{"point":doc_point(&w, [360., 270.])},{"down":true},{"down":false},{"key":SHIFT,"down":false}]));
    until(|| selected(&w).len() == 2, "Shift-click adds the blue image");
    capture(&w, "image-selection-handles");
    native.click(doc_point(&w, [600., 450.]));
    assert_eq!(image_order(&w).len(), 2, "empty canvas leaves both Object layers intact");


    native.click(center(&w, &object_row(&w, red)));
    until(|| selected(&w) == vec![red], "the list selects the obscured image");
    native.perform(json!([{"key":SHIFT,"down":true},{"point":center(&w, &object_row(&w, blue))},{"down":true},{"down":false},{"key":SHIFT,"down":false}]));
    until(|| selected(&w).len() == 2, "Shift-click on a row adds to the selection");
    native.click(center(&w, &row_child(&object_row(&w, red), "layer-thumbnail")));
    until(|| selected(&w) == vec![red], "the content thumbnail selects one Object layer");

    let before = affine(&w, red);
    let checkpoint = ui_session(&w).engine().checkpoint();
    let from = doc_point(&w, [140., 120.]);
    let to = doc_point(&w, [180., 140.]);
    native.perform(json!([{"point":from,"down":true},{"point":[(from[0]+to[0])*0.5,(from[1]+to[1])*0.5]},{"point":to},{"down":false}]));
    settle(&w);
    let moved = affine(&w, red);
    assert!((moved.0[4] - before.0[4] - 40.).abs() < 1.5 && (moved.0[5] - before.0[5] - 20.).abs() < 1.5, "dragging moves the image: {moved:?}");
    assert_eq!(moved.0[..4], before.0[..4]);
    assert_ne!(ui_session(&w).engine().checkpoint(), checkpoint);
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(affine(&w, red), before, "one undo step restores the start of the drag");
    invoke(&w, CommandId::Redo); settle(&w);
    assert_eq!(affine(&w, red), moved);

    let corner = doc_point(&w, moved.map([200., 150.]));
    native.perform(json!([{"point":corner,"down":true},{"point":[corner[0]+20.,corner[1]+15.]},{"point":[corner[0]+40.,corner[1]+30.]},{"down":false}]));
    settle(&w);
    let scaled = affine(&w, red);
    assert!(scaled.0[0] > 1.05 && scaled.0[3] > 1.05, "the corner handle resizes: {scaled:?}");
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(affine(&w, red), moved);

    native.click(center(&w, &row_child(&object_row(&w, red), "layer-column")));
    until(|| !document(&w).scene().occurrence(document(&w).scene().object_owner(red).unwrap()).unwrap().visible, "the row eye hides the image");
    native.click(center(&w, &row_child(&object_row(&w, red), "layer-column")));
    until(|| document(&w).scene().occurrence(document(&w).scene().object_owner(red).unwrap()).unwrap().visible, "and shows it again");

    assert_eq!(image_order(&w), vec![blue, red]);
    let start = center(&w, &row_child(&object_row(&w, red), "layer-name"));
    let target = object_row(&w, blue).compute_bounds(&w.window).unwrap();
    let end = [start[0], target.y() + target.height() * 0.2];
    native.perform(json!([{"point":start,"down":true},{"point":[start[0],start[1]-4.]},{"point":[start[0],start[1]-12.]},{"wait_ms":60},{"point":end},{"wait_ms":120},{"point":end},{"down":false}]));
    until(|| image_order(&w) == vec![red, blue], "a mouse drag on the red row body above the blue row brings it forward");
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(image_order(&w), vec![blue, red], "reordering is one undo step");

    let start = center(&w, &row_child(&object_row(&w, red), "layer-name"));
    let target = object_row(&w, blue).compute_bounds(&w.window).unwrap();
    let end = [start[0], target.y() + target.height() * 0.2];
    native.perform(json!([{"touch":"down","point":start},{"touch":"move","point":[start[0],start[1]-14.]},{"touch":"up"}]));
    pump(200);
    assert_eq!(image_order(&w), vec![blue, red], "an unheld touch on a row does not reorder");
    native.perform(json!([{"touch":"down","point":start},{"wait_ms":900},{"touch":"move","point":[start[0],start[1]-6.]},
        {"touch":"move","point":[start[0],start[1]-14.]},{"wait_ms":60},{"touch":"move","point":end},{"wait_ms":120},{"touch":"move","point":end},{"touch":"up"}]));
    until(|| image_order(&w) == vec![red, blue], "a held touch drag reorders the rows");
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(image_order(&w), vec![blue, red]);
    w.dispatch(UiAction::SelectLayer { id: layer_ui::occurrence_token(document(&w).scene().object_owner(red).unwrap()) });
    settle(&w);

    let layers = document(&w).scene().order().len();
    native.perform(json!([{"key":CONTROL,"down":true},{"key":0x6a,"down":true},{"key":0x6a,"down":false},{"key":CONTROL,"down":false}]));
    until(|| image_order(&w).len() == 3, "Ctrl+J duplicates the selected Object layer");
    assert_eq!(document(&w).scene().order().len(), layers + 1, "duplication adds a sibling layer");
    let copy = selected(&w);
    assert_eq!(copy.len(), 1);
    assert!(!copy.contains(&red), "the copy becomes the selection");
    w.area.grab_focus(); pump(50);
    invoke(&w, CommandId::DeleteLayer);
    until(|| image_order(&w).len() == 2, "Delete Layer removes the duplicate");
    assert_eq!(document(&w).scene().order().len(), layers);
    native.perform(json!([{"touch":"down","point":doc_point(&w, [360., 270.])},{"wait_ms":40},{"touch":"up"}]));
    until(|| selected(&w).contains(&blue), "a touch selects an unselected image");
    let camera = state(&w).camera.document_to_surface();
    let picked = selected(&w);
    let [a, b] = [doc_point(&w, [560., 40.]), doc_point(&w, [620., 60.])];
    let shifted = |p: [f32; 2], step: f32| [p[0] - 20. * step, p[1] + 15. * step];
    native.perform(json!([{"touch":"down","slot":0,"point":a},{"touch":"down","slot":1,"point":b},
        {"touch":"move","slot":0,"point":shifted(a, 1.)},{"touch":"move","slot":1,"point":shifted(b, 1.)},
        {"touch":"move","slot":0,"point":shifted(a, 2.)},{"touch":"move","slot":1,"point":shifted(b, 2.)},
        {"touch":"up","slot":0},{"touch":"up","slot":1}]));
    until(|| state(&w).camera.document_to_surface() != camera, "two fingers on empty canvas navigate");
    assert_eq!(selected(&w), picked, "navigation keeps the image selection");

    let paint_layers = document(&w).scene().order().len();
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Tool { tool: LayerCanvasTool::Paint } });
    settle(&w);
    let stroke = [doc_point(&w, [150., 300.]), doc_point(&w, [250., 320.])];
    native.perform(json!([{"point":stroke[0],"down":true},{"point":stroke[1]},{"down":false}]));
    let notice = super::canvas_bar_tests::until_some(|| state(&w).notice.filter(|n| n.actions.len() == 3), "painting on images raises the refusal");
    assert_eq!(notice.actions.iter().map(|a| a.id).collect::<Vec<_>>(), vec![NoticeActionId::AddMask, NoticeActionId::NewPaintLayer, NoticeActionId::RasterizeLayer]);
    let buttons: Vec<gtk::Button> = descendants::<gtk::Button>(w.window.upcast_ref()).into_iter().filter(|b| b.widget_name() == "canvas-notice-action" && b.is_mapped()).collect();
    assert_eq!(buttons.len(), 3);
    capture(&w, "image-paint-refusal");
    native.click(center(&w, buttons[1].upcast_ref()));
    until(|| document(&w).scene().order().len() == paint_layers + 1, "New Paint Layer adds a layer");
    let doc = document(&w);
    let active = doc.working.occurrence.unwrap();
    assert!(doc.scene().paint_source(active).is_some_and(|p| p.raster.is_empty()), "the refused stroke is not replayed");
    assert_eq!(image_order(&w).len(), 2);

    native.finish();
    w.window.destroy();
    pump(100);
}

fn kind(w: &Workspace, layer: OccurrenceHandle) -> layer_core::LayerKind {
    document(w).scene().occurrence(layer).unwrap().kind()
}

fn shown_points(w: &Workspace, points: &[[f32; 2]]) -> Vec<[u8; 4]> {
    points.iter().map(|point| super::photo_edit::shown(w, *point)).collect()
}

fn near_pixels(actual: &[[u8; 4]], expected: &[[u8; 4]], what: &str) {
    for (a, e) in actual.iter().zip(expected) {
        assert!(a.iter().zip(e).all(|(a, e)| a.abs_diff(*e) <= 2), "{what}: {actual:?} != {expected:?}");
    }
}

fn layer_menu(w: &Workspace, native: &mut RemoteInput, layer: OccurrenceHandle, item: &str) {
    let row = row_child(&mapped(w, &format!("art-layer-{}", layer_ui::occurrence_token(layer))), "layer-name");
    native.perform(json!([{"point":center(w, &row)},{"button":RIGHT,"down":true},{"button":RIGHT,"down":false}]));
    let entry = super::canvas_bar_tests::until_some(|| mapped_label(w.layer_panel.root.upcast_ref(), item), item);
    native.click(center(w, &entry));
}

#[test]
#[ignore = "isolated compositor, GPU and native keyboard and mouse delivery"]
fn native_image_layer_conversions_merges_and_alpha_selection() {
    let app = native_test_app("art.capycanvas.ImageConversions");
    let (mut project, layer, [red, blue]) = fixture();
    let ink = *project.scene().order().iter().find(|h| project.scene().paint_source(**h).is_some_and(|p| p.base.is_none())
        && project.scene().occurrence(**h).unwrap().name.as_ref() != "Paper").unwrap();
    let paint = project.scene().paint_source(ink).map(|_| ()).and_then(|_| project.scene().source_target(ink)).unwrap();
    let layer_core::authored::SourceTarget::Paint(paint) = paint else { unreachable!() };
    project.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::PaintBase::new(solid([120, 90], [40, 200, 60, 255])));
    project.artwork.occurrences.get_mut(ink).unwrap().offset = [60, 300];
    let w = Workspace::with_project(&app, Some((project, None)));
    apply_fixture_theme(&w);
    w.window.present();
    w.window.maximize();
    settle(&w);
    invoke(&w, CommandId::FitCanvas);
    settle(&w);
    let mut native = remote_input();
    let points = [[120., 120.], [360., 270.], [100., 340.], [600., 450.]];
    let original = shown_points(&w, &points);
    assert!(original[0][0] > 150 && original[1][2] > 150 && original[2][1] > 150, "{original:?}");

    layer_menu(&w, &mut native, layer, "Rasterize Layer");
    until(|| kind(&w, layer) == layer_core::LayerKind::Paint, "Rasterize Layer makes paint");
    settle(&w);
    near_pixels(&shown_points(&w, &points), &original, "rasterized images keep their appearance");
    assert!(document(&w).scene().object(red).is_none(), "the rasterized layer consumes its object");
    capture(&w, "image-rasterized");
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(kind(&w, layer), layer_core::LayerKind::Object);
    assert_eq!(image_order(&w), vec![blue, red], "undo restores the same images");

    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Select { id: layer_ui::occurrence_token(ink), mask: false } });
    settle(&w);
    layer_menu(&w, &mut native, ink, "Convert to Object Layer");
    until(|| kind(&w, ink) == layer_core::LayerKind::Object, "Convert to Image Layer makes images");
    settle(&w);
    assert_eq!(usize::from(document(&w).scene().object_handle(ink).is_some()), 1);
    near_pixels(&shown_points(&w, &points), &original, "converted paint keeps its appearance");
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(kind(&w, ink), layer_core::LayerKind::Paint);
    near_pixels(&shown_points(&w, &points), &original, "undo restores the paint");

    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Select { id: layer_ui::occurrence_token(layer), mask: false } });
    settle(&w);
    let layers = document(&w).scene().order().len();
    w.area.grab_focus(); pump(50);
    native.perform(json!([{"key":CONTROL,"down":true},{"key":0x65,"down":true},{"key":0x65,"down":false},{"key":CONTROL,"down":false}]));
    until(|| document(&w).scene().order().len() == layers - 1, "Ctrl+E merges the image layer down");
    settle(&w);
    near_pixels(&shown_points(&w, &points), &original, "merging keeps the composite");
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(document(&w).scene().order().len(), layers);
    assert_eq!(image_order(&w), vec![blue, red]);

    let thumbnail = row_child(&mapped(&w, &format!("art-layer-{}", layer_ui::occurrence_token(layer))), "layer-thumbnail");
    native.perform(json!([{"key":CONTROL,"down":true},{"point":center(&w, &thumbnail)},{"down":true},{"down":false},{"key":CONTROL,"down":false}]));
    until(|| document(&w).working.selection.is_some(), "Ctrl-click on the image layer preview selects its opacity");
    let bounds = document(&w).working.selection.clone().unwrap().coverage_bounds();
    assert!((bounds.min.x - 100.).abs() < 1. && (bounds.min.y - 100.).abs() < 1. && (bounds.max.x - 300.).abs() < 1. && (bounds.max.y - 250.).abs() < 1., "{bounds:?}");
    assert_eq!(kind(&w, layer), layer_core::LayerKind::Object, "selecting opacity keeps the images");
    invoke(&w, CommandId::Deselect); settle(&w);

    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Tool { tool: LayerCanvasTool::Paint } });
    settle(&w);
    let stroke = [doc_point(&w, [150., 200.]), doc_point(&w, [250., 220.])];
    native.perform(json!([{"point":stroke[0],"down":true},{"point":stroke[1]},{"down":false}]));
    super::canvas_bar_tests::until_some(|| state(&w).notice.filter(|n| n.actions.len() == 3), "the refusal offers Rasterize Layer");
    let rasterize = descendants::<gtk::Button>(w.window.upcast_ref()).into_iter().filter(|b| b.widget_name() == "canvas-notice-action" && b.is_mapped()).nth(2).unwrap();
    native.click(center(&w, rasterize.upcast_ref()));
    until(|| kind(&w, layer) == layer_core::LayerKind::Paint, "the notice rasterizes the image layer");
    settle(&w);
    let painted = document(&w).scene().paint_source(layer).unwrap().raster.identity();
    native.perform(json!([{"point":stroke[0],"down":true},{"point":stroke[1]},{"down":false}]));
    until(|| document(&w).scene().paint_source(layer).unwrap().raster.identity() != painted, "the next stroke paints the rasterized layer");
    native.finish();
    w.window.destroy();
    pump(100);
}

fn clip_nonce() -> Option<String> { crate::files::clipboard::current().map(|clip| clip.nonce) }

fn chord(native: &mut RemoteInput, key: u32) {
    native.perform(json!([{"key":CONTROL,"down":true},{"key":key,"down":true},{"key":key,"down":false},{"key":CONTROL,"down":false}]));
}

fn copied(w: &Workspace, previous: Option<String>) -> layer_ui::PixelClip {
    until(|| clip_nonce() != previous && state(w).requests.is_empty() && !state(w).document_file.busy, "the copy reaches the clipboard");
    crate::files::clipboard::current().unwrap()
}

fn mouse_path(native: &mut RemoteInput, points: &[[f32; 2]]) {
    let mut events = vec![json!({"point": points[0]}), json!({"down": true})];
    events.extend(points[1..].iter().map(|p| json!({"point": p})));
    events.push(json!({"down": false}));
    native.perform(serde_json::Value::Array(events));
}

#[test]
#[ignore = "isolated compositor, GPU, wl-clipboard and native keyboard and mouse delivery"]
fn native_image_object_clipboard_and_paste_into() {
    let app = native_test_app("art.capycanvas.ImageClipboard");
    let (project, _layer, [red, blue]) = fixture();
    let w = Workspace::with_project(&app, Some((project, None)));
    apply_fixture_theme(&w);
    w.window.present();
    w.window.maximize();
    settle(&w);
    invoke(&w, CommandId::FitCanvas);
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Tool { tool: LayerCanvasTool::Move } });
    settle(&w);
    let mut native = remote_input();
    let red_image = document(&w).scene().object(red).unwrap().image.id();
    native.click(doc_point(&w, [140., 120.]));
    until(|| selected(&w) == vec![red], "the red image is selected");

    let clip = copied(&w, { let before = clip_nonce(); chord(&mut native, 0x63); before });
    assert_eq!(clip.layers.as_ref().expect("Ctrl+C retains the selected Object layer").roots.len(), 1);
    assert_eq!(clip.origin, [100, 100], "the picture fallback covers the image's own bounds");
    let formats: Vec<String> = w.window.clipboard().formats().mime_types().iter().map(|m| m.to_string()).collect();
    assert!(formats.iter().any(|m| m == "image/png"), "{formats:?}");
    assert_eq!(image_order(&w), vec![blue, red], "Copy leaves the drawing unchanged");

    chord(&mut native, 0x76);
    until(|| image_order(&w).len() == 3, "Ctrl+V pastes a sibling Object layer");
    let pasted = selected(&w);
    assert_eq!(pasted.len(), 1);
    assert!(!pasted.contains(&red), "the paste is a new image object");
    let copy = document(&w).scene().object(pasted[0]).unwrap().clone();
    assert_eq!(copy.affine, affine(&w, red), "pasting keeps the copied document position");
    assert_eq!(copy.image.id(), red_image, "the paste shares the immutable image");
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(image_order(&w), vec![blue, red], "pasting is one undo step");

    native.click(doc_point(&w, [140., 120.]));
    until(|| selected(&w) == vec![red], "reselect red");
    let place = affine(&w, red);
    let cut = copied(&w, { let before = clip_nonce(); chord(&mut native, 0x78); before });
    assert!(cut.layers.is_some());
    until(|| image_order(&w) == vec![blue], "Cut removes the image after the clipboard has it");
    chord(&mut native, 0x76);
    until(|| image_order(&w).len() == 2, "pasting the cut image restores it");
    let restored = selected(&w)[0];
    assert_eq!(affine(&w, restored), place);
    capture(&w, "image-cut-paste");

    let order = document(&w).scene().order().len();
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    settle(&w);
    let frame = [[400., 60.], [600., 60.], [600., 220.], [400., 220.], [400., 60.]].map(|p| doc_point(&w, p));
    mouse_path(&mut native, &frame);
    until(|| document(&w).working.selection.is_some(), "a lasso selection");
    super::photo_edit::choose(&w, &mut native, "Edit", &["Paste Into"]);
    until(|| document(&w).scene().order().len() == order + 1 && state(&w).requests.is_empty(), "Paste Into adds a masked image layer");
    let doc = document(&w);
    let frame_layer = doc.working.occurrence.unwrap();
    assert_eq!(doc.scene().occurrence(frame_layer).unwrap().kind(), layer_core::LayerKind::Object);
    let mask = doc.scene().occurrence(frame_layer).unwrap().mask.clone().expect("a mask from the selection");
    assert!(doc.working.selection.is_none(), "the selection became the mask");
    let inner = vec![doc.scene().object_handle(frame_layer).unwrap()];
    assert_eq!(inner.len(), 1);
    settle(&w);
    capture(&w, "image-paste-into");
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Tool { tool: LayerCanvasTool::Move } });
    settle(&w);
    let start = affine(&w, inner[0]);
    let grab = doc_point(&w, start.map([50., 40.]));
    mouse_path(&mut native, &[grab, [grab[0] + 20., grab[1] + 10.], [grab[0] + 40., grab[1] + 20.]]);
    settle(&w);
    let after = document(&w);
    assert_ne!(after.scene().object(inner[0]).unwrap().affine, start, "the image moves behind its mask");
    assert_eq!(after.scene().occurrence(frame_layer).unwrap().mask, Some(mask), "the mask frame stays fixed");
    invoke(&w, CommandId::Undo); settle(&w);
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(document(&w).scene().order().len(), order, "Paste Into undoes in one step");
    native.finish();
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated compositor, GPU and native keyboard and mouse delivery"]
fn native_image_and_pixel_targets_and_crop_keep_images() {
    let app = native_test_app("art.capycanvas.ImageTargets");
    let (project, _layer, [red, blue]) = fixture();
    let w = Workspace::with_project(&app, Some((project, None)));
    apply_fixture_theme(&w);
    w.window.present();
    w.window.maximize();
    settle(&w);
    invoke(&w, CommandId::FitCanvas);
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Tool { tool: LayerCanvasTool::Move } });
    settle(&w);
    let mut native = remote_input();
    native.click(doc_point(&w, [140., 120.]));
    until(|| selected(&w) == vec![red], "the red image is selected");

    w.dispatch(UiAction::Invoke { command: CommandId::RectangleSelect });
    settle(&w);
    mouse_path(&mut native, &[doc_point(&w, [80., 80.]), doc_point(&w, [200., 160.]), doc_point(&w, [320., 260.])]);
    until(|| document(&w).working.selection.is_some(), "a rectangle selection");
    assert_eq!(selected(&w), vec![red], "a pixel selection keeps the image selection");
    let clear = state(&w).commands.into_iter().find(|c| c.id == CommandId::ClearSelected).unwrap();
    assert!(clear.enabled && clear.label.as_ref() == "Clear Selected Pixels", "the pixel target names pixels: {clear:?}");
    w.area.grab_focus(); pump(50);
    native.key(0xffff);
    until(|| w.notice.root.is_visible() && state(&w).notice.is_some_and(|notice| notice.actions.len() == 3), "Delete on images offers the image actions");
    assert_eq!(image_order(&w), vec![blue, red], "the pixel target never deletes images");
    capture(&w, "image-pixel-target-refusal");

    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Tool { tool: LayerCanvasTool::Move } });
    settle(&w);
    assert!(document(&w).working.selection.is_some(), "returning to Move keeps the pixel selection");
    w.area.grab_focus(); pump(50);
    invoke(&w, CommandId::DeleteLayer);
    until(|| image_order(&w) == vec![blue], "Delete Layer removes the selected Object occurrence");
    assert!(document(&w).working.selection.is_some(), "and leaves the pixel selection");
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(image_order(&w), vec![blue, red]);

    let before = [red, blue].map(|h| document(&w).object_document_affine(h).unwrap());
    invoke(&w, CommandId::CropCanvasToSelection);
    until(|| document(&w).composition().size != [640, 480], "Crop to Selection crops the canvas");
    settle(&w);
    let doc = document(&w);
    let size = doc.composition().size;
    assert_eq!(image_order(&w), vec![blue, red], "cropping keeps every image, inside the frame or not");
    let after = [red, blue].map(|h| doc.object_document_affine(h).unwrap());
    let shift = [after[0].0[4] - before[0].0[4], after[0].0[5] - before[0].0[5]];
    assert!(shift[0] < 0. && shift[1] < 0., "content moves by the crop origin: {shift:?}");
    for (a, b) in after.iter().zip(&before) {
        assert_eq!(a.0[..4], b.0[..4], "cropping never resamples an image");
        assert_eq!([a.0[4] - b.0[4], a.0[5] - b.0[5]], shift, "every image keeps its place relative to the artwork");
    }
    capture(&w, "image-crop-keeps-images");
    invoke(&w, CommandId::Undo); settle(&w);
    assert_ne!(document(&w).composition().size, size);
    assert_eq!([red, blue].map(|h| document(&w).object_document_affine(h).unwrap()), before, "one undo step restores the frame and positions");
    native.finish();
    w.window.destroy();
    pump(100);
}

/// `(id, mask, revision)` of every layer-panel preview request still in flight.
fn pending_previews(w: &Workspace) -> Vec<(u64, bool, u64)> {
    let debug = w.layer_panel.preview_debug(&[]);
    debug["pending"].as_array().unwrap().iter()
        .map(|entry| serde_json::from_value::<(u64, bool, u64)>(entry[1].clone()).unwrap()).collect()
}

fn cached_preview(w: &Workspace, (id, mask, revision): (u64, bool, u64)) -> bool {
    let debug = w.layer_panel.preview_debug(&[]);
    debug["cached"].as_array().unwrap().iter()
        .any(|entry| serde_json::from_value::<((u64, bool), u64)>(entry.clone()).unwrap() == ((id, mask), revision))
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_previews_in_flight_during_a_renderer_replacement_still_arrive() {
    let app = native_test_app("art.capycanvas.PreviewReplacement");
    let (project, layer, [red, blue]) = fixture();
    let w = Workspace::with_project(&app, Some((project, None)));
    apply_fixture_theme(&w);
    w.window.present();
    settle(&w);
    let images = [red, blue].map(|id| layer_ui::occurrence_token(document(&w).scene().object_owner(id).unwrap()));
    let mut lost = Vec::new();
    until(|| {
        pump(1);
        lost = pending_previews(&w);
        lost.iter().any(|(id, _, _)| images.contains(id))
    }, "image row previews are requested");
    let generation = ui_session(&w).renderer_generation();
    w.restart_gpu();
    assert_ne!(ui_session(&w).renderer_generation(), generation, "the canvas has a new renderer");
    until(|| lost.iter().all(|request| cached_preview(&w, *request)) && pending_previews(&w).is_empty(),
        "previews in flight on the replaced renderer arrive from the new one");
    for object in [red, blue] {
        until(|| widgets(&object_row(&w, object)).filter_map(|widget| widget.downcast::<gtk::Picture>().ok()).any(|p| p.paintable().is_some()),
            "each image row shows its preview");
    }
    let layer_preview = row_child(&mapped(&w, &format!("art-layer-{}", layer_ui::occurrence_token(layer))), "layer-thumbnail");
    until(|| widgets(&layer_preview).filter_map(|widget| widget.downcast::<gtk::Picture>().ok()).any(|p| p.paintable().is_some()),
        "the image layer row shows its preview");
    w.window.destroy();
    pump(100);
}

fn pen_hold(native: &mut RemoteInput, at: [f32; 2]) {
    native.perform(json!([{"pen":"down","point":at},{"wait_ms":900}]));
}

fn pen_release(native: &mut RemoteInput) {
    native.perform(json!([{"pen":"up"},{"pen":"leave"}]));
}

/// Run the item labelled `label` in the open popover menu. The isolated
/// tablet's synthetic serials cannot grab a popup, so a pen-opened menu runs
/// its item through the menu model's action.
fn run_open_menu_item(w: &Workspace, label: &str) {
    fn action(model: &gtk::gio::MenuModel, label: &str) -> Option<String> {
        (0..model.n_items()).find_map(|i| {
            for link in ["section", "submenu"] {
                if let Some(found) = model.item_link(i, link).and_then(|inner| action(&inner, label)) { return Some(found); }
            }
            (model.item_attribute_value(i, "label", None)?.get::<String>()? == label)
                .then(|| model.item_attribute_value(i, "action", None)?.get::<String>()).flatten()
        })
    }
    let (popover, name) = super::canvas_bar_tests::until_some(|| widgets(w.window.upcast_ref())
        .filter_map(|widget| widget.downcast::<gtk::PopoverMenu>().ok())
        .filter(|popover| popover.is_visible())
        .find_map(|popover| { let name = action(&popover.menu_model()?, label)?; Some((popover, name)) }), label);
    popover.activate_action(&name, None).unwrap();
    popover.popdown();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_image_objects_with_the_pen --tablet"]
fn native_image_objects_with_the_pen() {
    use super::crop::{Device, drag, tap};
    let app = native_test_app("art.capycanvas.ImageObjectsPen");
    let (project, layer, [red, blue]) = fixture();
    let w = Workspace::with_project(&app, Some((project, None)));
    apply_fixture_theme(&w);
    w.window.present();
    w.window.maximize();
    settle(&w);
    invoke(&w, CommandId::FitCanvas);
    if state(&w).commands.iter().any(|c| c.id == CommandId::TransformSnapping && c.selected) { invoke(&w, CommandId::TransformSnapping); }
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Tool { tool: LayerCanvasTool::Move } });
    settle(&w);
    let mut native = remote_input();
    let token = layer_ui::occurrence_token(layer);
    tap(&mut native, Device::Pen, doc_point(&w, [140., 120.]));
    until(|| selected(&w) == vec![red], "a pen tap selects the red image");
    let before = affine(&w, red);
    drag(&mut native, Device::Pen, doc_point(&w, [140., 120.]), doc_point(&w, [180., 140.]));
    settle(&w);
    let moved = affine(&w, red);
    assert!((moved.0[4] - before.0[4] - 40.).abs() < 1.5 && (moved.0[5] - before.0[5] - 20.).abs() < 1.5, "a pen drag moves the image: {moved:?}");
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(affine(&w, red), before, "one undo step");
    let corner = doc_point(&w, before.map([200., 150.]));
    drag(&mut native, Device::Pen, corner, [corner[0] + 40., corner[1] + 30.]);
    settle(&w);
    let scaled = affine(&w, red);
    assert!(scaled.0[0] > 1.05 && scaled.0[3] > 1.05, "the pen resizes from the corner handle: {scaled:?}");
    invoke(&w, CommandId::Undo); settle(&w);

    tap(&mut native, Device::Pen, center(&w, &row_child(&object_row(&w, blue), "layer-name")));
    until(|| selected(&w) == vec![blue], "a pen tap on a row selects its image");
    tap(&mut native, Device::Pen, center(&w, &row_child(&object_row(&w, red), "layer-name")));
    until(|| selected(&w) == vec![red], "and selects the obscured image");
    tap(&mut native, Device::Pen, center(&w, &row_child(&object_row(&w, red), "layer-column")));
    until(|| !document(&w).scene().occurrence(document(&w).scene().object_owner(red).unwrap()).unwrap().visible, "a pen tap on the eye hides the image");
    tap(&mut native, Device::Pen, center(&w, &row_child(&object_row(&w, red), "layer-column")));
    until(|| document(&w).scene().occurrence(document(&w).scene().object_owner(red).unwrap()).unwrap().visible, "and shows it");

    assert_eq!(image_order(&w), vec![blue, red]);
    let order = document(&w).scene().order().len();
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Tool { tool: LayerCanvasTool::Paint } });
    settle(&w);
    drag(&mut native, Device::Pen, doc_point(&w, [150., 300.]), doc_point(&w, [250., 320.]));
    super::canvas_bar_tests::until_some(|| state(&w).notice.filter(|n| n.actions.len() == 3), "a pen stroke on images is refused with actions");
    let buttons: Vec<gtk::Button> = descendants::<gtk::Button>(w.window.upcast_ref()).into_iter().filter(|b| b.widget_name() == "canvas-notice-action" && b.is_mapped()).collect();
    tap(&mut native, Device::Pen, center(&w, buttons[1].upcast_ref()));
    until(|| document(&w).scene().order().len() == order + 1, "a pen tap on New Paint Layer adds paint");
    let doc = document(&w);
    assert!(doc.scene().paint_source(doc.working.occurrence.unwrap()).is_some_and(|p| p.raster.is_empty()), "the refused stroke is not replayed");
    invoke(&w, CommandId::Undo); settle(&w);

    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Select { id: token, mask: false } });
    w.dispatch(UiAction::Invoke { command: CommandId::RectangleSelect });
    settle(&w);
    drag(&mut native, Device::Pen, doc_point(&w, [80., 80.]), doc_point(&w, [320., 260.]));
    until(|| document(&w).working.selection.is_some(), "a pen rectangle selection");
    w.area.grab_focus(); pump(50);
    native.key(0xffff);
    pump(300);
    assert_eq!(image_order(&w), vec![blue, red], "pixel Delete never removes images");
    invoke(&w, CommandId::Deselect); settle(&w);

    let row = row_child(&mapped(&w, &format!("art-layer-{token}")), "layer-name");
    pen_hold(&mut native, center(&w, &row));
    pen_release(&mut native);
    run_open_menu_item(&w, "Rasterize Layer");
    until(|| kind(&w, layer) == layer_core::LayerKind::Paint, "a pen-opened layer menu rasterizes the image layer");
    invoke(&w, CommandId::Undo); settle(&w);
    assert_eq!(image_order(&w), vec![blue, red], "undo restores the images");
    capture(&w, "image-pen");
    native.finish();
    w.window.destroy();
    pump(100);
}
