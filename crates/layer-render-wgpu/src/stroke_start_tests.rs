use super::*;
use crate::test_support::{complete, dab_batch, packet, submit_published, submit_settled};
use layer_core::{DefaultBrushPreset, Point};
use layer_core::raster::{RasterData, RasterRevision, RasterTile, TileBlob, TileKey};
use layer_render::DabStyle;
use std::collections::BTreeMap;

const SIDE: u32 = PAGE_SIZE * 3;

struct Canvas {
    document: layer_core::Document,
    r: WgpuRasterizer,
    target: SourceTarget,
}

impl Canvas {
    fn new(transparent_tiles: bool) -> Self {
        let mut document = layer_core::Document::new(layer_core::PortableId::random(), SIDE, SIDE,
            layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let target = document.active_target().unwrap();
        if transparent_tiles {
            let descriptor = document.composition().color.paint_descriptor();
            let blob = Arc::new(TileBlob::encode(descriptor, &vec![0; descriptor.byte_len([PAGE_SIZE; 2]).unwrap()]).unwrap());
            let tiles = (0..3).flat_map(|y| (0..3).map(move |x| [x, y]))
                .map(|coordinate| (TileKey { plane: RasterPlane::Color, coordinate }, RasterTile::backed_shared(blob.clone()))).collect();
            Self::paint(&mut document, target).raster = RasterRevision::backed(RasterData { tiles, watercolor: None });
        }
        let mut r = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        submit_settled(&mut r, packet(document.scene(), [SIDE; 2]));
        Self { document, r, target }
    }

    fn paint(document: &mut layer_core::Document, target: SourceTarget) -> &mut layer_core::authored::PaintSource {
        let SourceTarget::Paint(paint) = target else { unreachable!() };
        document.artwork.paint.get_mut(paint).unwrap()
    }

    fn stroke(&mut self, dabs: &[Dab], style: &DabStyle, stroke: u64, [start, end]: [bool; 2]) {
        let damage = dabs.iter().fold(layer_core::Rect::EMPTY, |damage, dab| damage.union(dab.bounds()));
        let batch = DabBatch { stroke_id: StrokeId(stroke), stroke_start: start, stroke_end: end, dab_count: dabs.len() as u32,
            ..dab_batch(self.target, style.clone(), damage) };
        Self::paint(&mut self.document, self.target).raster = RasterRevision::pending();
        submit_published(&mut self.r, FramePacket { dabs, dab_batches: std::slice::from_ref(&batch), ..packet(self.document.scene(), [SIDE; 2]) });
        complete(&self.r);
    }

    fn starts(&self) -> Vec<([u32; 2], &StrokeStart)> {
        self.r.paint_layers.iter().flat_map(|layer| &layer.pages)
            .filter_map(|page| page.stroke_start.as_ref().map(|start| (page.coordinate, start))).collect()
    }

    fn start_bytes(&self) -> u64 {
        self.starts().into_iter().map(|(_, start)| start.storage_bytes()).sum()
    }

    fn zero_page_starts(&self) -> Vec<([u32; 2], bool)> {
        let mut starts: Vec<_> = self.starts().into_iter().map(|(coordinate, start)| (coordinate, start.surface.holds_texture(&self.r.zero_pages[0]))).collect();
        starts.sort();
        starts
    }

    fn tiles(&mut self) -> BTreeMap<TileKey, [u8; 32]> {
        Self::paint(&mut self.document, self.target).raster.wait_data().unwrap().tiles.iter()
            .map(|(key, tile)| (*key, tile.wait_backing().unwrap().content_digest().unwrap())).collect()
    }
}

fn line(from: [f32; 2], to: [f32; 2], radius: f32, color: [f32; 4]) -> Vec<Dab> {
    (0..=12).map(|step| {
        let t = step as f32 / 12.;
        let mut dab = crate::layer_tests::dab(color);
        dab.center = Point { x: from[0] + (to[0] - from[0]) * t, y: from[1] + (to[1] - from[1]) * t };
        dab.radii = [radius; 2];
        dab
    }).collect()
}

fn assert_matches_copied_starts(shared: &mut Canvas, copied: &mut Canvas, expected: &[([u32; 2], bool)]) {
    let page = u64::from(PAGE_SIZE).pow(2) * u64::from(shared.r.device.working_format().block_copy_size(None).unwrap());
    assert_eq!(shared.zero_page_starts(), expected, "every page empty at stroke start shares the renderer's zero page");
    assert_eq!(copied.zero_page_starts(), expected.iter().map(|(coordinate, _)| (*coordinate, false)).collect::<Vec<_>>());
    let empty = expected.iter().filter(|(_, zero)| *zero).count() as u64;
    assert_eq!(shared.start_bytes(), (expected.len() as u64 - empty) * page, "zero-page starts hold no bytes");
    assert_eq!(copied.start_bytes(), expected.len() as u64 * page);
    assert!(copied.r.paint_memory_bytes() >= shared.r.paint_memory_bytes() + empty * page,
        "the shared zero page is charged once: {} against {}", shared.r.paint_memory_bytes(), copied.r.paint_memory_bytes());
    let authored = shared.tiles();
    let reference = copied.tiles();
    assert!(expected.iter().all(|(coordinate, _)| authored.contains_key(&TileKey { plane: RasterPlane::Color, coordinate: *coordinate })));
    for (key, digest) in &authored {
        assert_eq!(reference.get(key), Some(digest), "{key:?} authors the codes of a copied transparent start");
    }
}

#[test]
fn a_paintbrush_stroke_over_empty_pages_starts_from_the_shared_zero_page() {
    let style = crate::layer_tests::preset_style(DefaultBrushPreset::Paintbrush);
    assert!(style.opacity < 1.);
    let dabs = line([60., 380.], [700., 400.], 70., [0.7, 0.2, 0.1, 1.]);
    let [mut shared, mut copied] = [false, true].map(Canvas::new);
    for canvas in [&mut shared, &mut copied] { canvas.stroke(&dabs, &style, 1, [true, false]); }
    assert_matches_copied_starts(&mut shared, &mut copied, &[([0, 1], true), ([1, 1], true), ([2, 1], true)]);
}

#[test]
fn a_blender_stroke_from_paint_into_empty_pages_starts_them_from_the_shared_zero_page() {
    let paint = crate::tests::test_style(BrushExecution::Dry);
    let blender = crate::layer_tests::preset_style(DefaultBrushPreset::ScatterBlender);
    assert!(stable_blender(&blender));
    let ink = line([80., 320.], [170., 440.], 50., [0.1, 0.3, 0.8, 1.]);
    let smear = line([100., 384.], [660., 384.], 90., [0.; 4]);
    let [mut shared, mut copied] = [false, true].map(Canvas::new);
    for canvas in [&mut shared, &mut copied] {
        canvas.stroke(&ink, &paint, 1, [true, true]);
        canvas.stroke(&smear, &blender, 2, [true, false]);
    }
    assert_matches_copied_starts(&mut shared, &mut copied, &[([0, 1], false), ([1, 1], true), ([2, 1], true)]);
}

#[test]
fn a_page_painted_earlier_in_the_stroke_keeps_reading_its_zero_start() {
    let style = crate::layer_tests::preset_style(DefaultBrushPreset::Paintbrush);
    let out = line([60., 380.], [400., 390.], 70., [0.2, 0.6, 0.3, 1.]);
    let back = line([450., 400.], [330., 370.], 60., [0.2, 0.6, 0.3, 1.]);
    let onward = line([330., 370.], [700., 380.], 60., [0.2, 0.6, 0.3, 1.]);
    let [mut shared, mut copied] = [false, true].map(Canvas::new);
    for canvas in [&mut shared, &mut copied] {
        canvas.stroke(&out, &style, 1, [true, false]);
        canvas.stroke(&back, &style, 1, [false, false]);
        canvas.stroke(&onward, &style, 1, [false, false]);
    }
    assert_matches_copied_starts(&mut shared, &mut copied, &[([0, 1], true), ([1, 1], true), ([2, 1], true)]);
}
