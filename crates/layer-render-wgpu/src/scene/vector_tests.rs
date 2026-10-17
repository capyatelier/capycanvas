use super::*;
use crate::test_support::{packet, float_pixels, max_error};
use layer_core::authored::vector::*;
use layer_core::color::RgbColor;
use crate::scene::Output;

fn paint(rgba: [f32; 4]) -> Paint { Paint::color(RgbColor::new(Default::default(), rgba).unwrap()) }
fn stroke(id: u64, points: &[[f64; 2]], width: f64, rgba: [f32; 4]) -> Record {
    let anchors: Vec<_> = points.iter().map(|p| Anchor::corner(*p)).collect();
    let mut stroke = Stroke::new(paint(rgba), width);
    stroke.cap = Cap::Round; stroke.join = Join::Round;
    Record::Path(PathItem::new(ItemId(id), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)]))))
}
fn shape(id: u64, rect: [f64; 4], rgba: [f32; 4]) -> Record {
    let [x0, y0, x1, y1] = rect;
    let anchors = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(Anchor::corner);
    Record::Path(PathItem::new(ItemId(id), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, true)]).unwrap()),
        Arc::new(Style::new(vec![PaintEntry::Fill(Fill { paint: paint(rgba), unknown: None })]))))
}
fn document(extent: [u32; 2], records: Vec<Record>) -> (layer_core::Editor, OccurrenceHandle) {
    let artwork = layer_core::authored::Artwork::new(extent).unwrap();
    let mut editor = layer_core::Editor::new(layer_core::Document::from_artwork(artwork).unwrap());
    let (layer, edit) = editor.document().create_vector_layer_edit("Ink", VectorObject::from_records(records), None, 0).unwrap();
    editor.perform(edit).unwrap();
    (editor, layer)
}
fn records() -> Vec<Record> {
    let mut list: Vec<Record> = (0..40).map(|i| stroke(i + 1, &[[10. + i as f64 * 7., 20.], [60. + i as f64 * 5., 150.], [20. + i as f64 * 6., 190.]], 2. + (i % 4) as f64, [0.05, 0.1, 0.3, 1.])).collect();
    list.push(shape(100, [40., 60., 260., 120.], [0.9, 0.5, 0.2, 0.8]));
    list
}
fn submit_ready(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    r.submit(frame).unwrap();
    for _ in 0..200 {
        r.wait_idle().unwrap();
        if !r.has_pending_work() { return; }
        r.submit(FramePacket { composite_all: false, reset_layers: false, ..frame }).unwrap();
    }
    panic!("vector display must settle");
}
fn native(r: &mut WgpuRasterizer, doc: &layer_core::Document) -> Vec<[f32; 4]> {
    let extent = doc.composition().size;
    let (texture, _) = create_color_target(&r.device, extent, "vector native reference");
    let mut scene = r.scene.take().unwrap_or_else(|| Scene::new(r));
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    scene.capture_region_prepared(r, packet(doc.scene(), extent), &texture, PixelRect::full(extent), Output::Artwork(None), &mut encoder).unwrap();
    r.uploads.finish(&encoder); encoder.submit(&r.queue);
    r.scene = Some(scene);
    float_pixels(r, &texture)
}
fn display(r: &WgpuRasterizer) -> Vec<[f32; 4]> { float_pixels(r, r.scale_display.as_ref().unwrap().texture()) }

#[test]
fn vector_layers_render_the_same_through_display_and_native_paths() {
    let (editor, _) = document([300, 200], records());
    let doc = editor.document();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    submit_ready(&mut r, packet(doc.scene(), doc.composition().size));
    let shown = display(&r);
    let exact = native(&mut r, doc);
    let covered = exact.iter().filter(|p| p[3] > 0.5).count();
    eprintln!("covered {covered}, display vs native {}", max_error(&shown, &exact));
    assert!(covered > 5000, "vector content must be visible");
    assert!(max_error(&shown, &exact) < 1e-5);
}

#[test]
fn vector_edits_damage_only_changed_items_and_match_a_fresh_render() {
    let (mut editor, layer) = document([300, 200], records());
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut r, packet(editor.document().scene(), editor.document().composition().size));
    let before = editor.document().clone();
    let object = editor.document().scene().vector_layer(layer).unwrap().clone();
    let drawn = object.insert_front(vec![stroke(500, &[[250., 10.], [290., 40.]], 4., [1., 0., 0., 1.])]);
    editor.perform(editor.document().vector_object_edit(layer, drawn).unwrap()).unwrap();
    let damage = objects::edited_damage(before.scene(), editor.document().scene(), [300, 200]).expect("vector edits must not composite everything");
    let area: u64 = damage.regions.iter().map(|r| u64::from(r.width()) * u64::from(r.height())).sum();
    eprintln!("drawing damage {area} px over {:?}", damage.regions);
    assert!(area < 300 * 200 / 4, "a new stroke damages only its own bounds");
    submit_ready(&mut r, FramePacket { composite_all: true, ..packet(editor.document().scene(), [300, 200]) });
    let incremental = display(&r);
    let mut fresh = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut fresh, packet(editor.document().scene(), [300, 200]));
    assert!(max_error(&incremental, &display(&fresh)) < 1e-5);
    let target = editor.document().scene().vector_layer(layer).unwrap().find(ItemId(7)).unwrap();
    let recolored = editor.document().scene().vector_layer(layer).unwrap().replace([(target, Some(stroke(7, &[[59., 20.], [95., 150.], [62., 190.]], 5., [0., 0.8, 0., 1.])))]);
    let before = editor.document().clone();
    editor.perform(editor.document().vector_object_edit(layer, recolored).unwrap()).unwrap();
    assert!(objects::edited_damage(before.scene(), editor.document().scene(), [300, 200]).is_some());
    submit_ready(&mut r, FramePacket { composite_all: true, ..packet(editor.document().scene(), [300, 200]) });
    let mut fresh = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut fresh, packet(editor.document().scene(), [300, 200]));
    assert!(max_error(&display(&r), &display(&fresh)) < 1e-5);
}
