//! Erase through selection coverage: bounded pages, soft coverage, inverted
//! coverage, alpha lock, watercolor settling and inserted copies.
use super::*;
use layer_core::raster::{RasterRevision, TileKey};
use std::collections::BTreeMap;

const EXTENT: [u32; 2] = [600, 300];

fn frame(r: &mut WgpuRasterizer, layers: &[Layer], dabs: &[Dab], batches: &[DabBatch], restore: &[(LayerId, RasterRevision)]) {
    r.submit(FramePacket {
        dabs,
        dab_batches: batches,
        restore_rasters: restore,
        reset_layers: false,
        ..packet(layers, EXTENT)
    })
    .unwrap();
}

fn image(r: &mut WgpuRasterizer) -> Vec<u8> {
    r.readback_srgb_rgba8().unwrap()
}

fn at(image: &[u8], [x, y]: [usize; 2]) -> [u8; 4] {
    image[(y * EXTENT[0] as usize + x) * 4..][..4].try_into().unwrap()
}

fn rectangle([x0, y0, x1, y1]: [f32; 4]) -> Selection {
    Selection::polygon(vec![
        Point { x: x0, y: y0 },
        Point { x: x1, y: y0 },
        Point { x: x1, y: y1 },
        Point { x: x0, y: y1 },
    ])
    .unwrap()
}

/// Opaque white paint over the whole layer.
fn painted(r: &mut WgpuRasterizer, id: u64) -> Layer {
    let mut layer = Layer::paint(LayerId(id), "erased paint");
    let mut ink = dab([1.; 4]);
    ink.center = Point { x: 300., y: 150. };
    ink.radii = [400.; 2];
    let mut stroke = batch(id);
    stroke.damage = Rect { min: Point::default(), max: Point { x: 600., y: 300. } };
    layer.raster = RasterRevision::pending();
    frame(r, std::slice::from_ref(&layer), &[ink], &[stroke], &[]);
    layer.raster.wait_data().unwrap();
    layer
}

fn erase_operation(selection: Selection, alpha_locked: bool) -> LayerOperation {
    let mut coverage = LayerMask::reveal_all(LayerId(90), Point::default());
    coverage.default_coverage = f32::from(selection.inverted);
    coverage.initial = Some(selection);
    LayerOperation {
        placement: layer_core::Affine::IDENTITY,
        coverage,
        kind: LayerOperationKind::Erase { alpha_locked },
    }
}

/// Run `op` on `layer` with the given damage and return the published tiles.
fn run(
    r: &mut WgpuRasterizer,
    layer: &mut Layer,
    op: LayerOperation,
    damage: Rect,
    restore: &[(LayerId, RasterRevision)],
) -> BTreeMap<TileKey, u64> {
    let command = DabBatch {
        kind: DabBatchKind::LayerOperation(0),
        dab_count: 0,
        damage,
        ..batch(layer.id.0)
    };
    layer.pending_operations.push(op);
    layer.raster = RasterRevision::pending();
    frame(r, std::slice::from_ref(layer), &[], &[command], restore);
    let data = layer.raster.wait_data().unwrap();
    layer.pending_operations.clear();
    frame(r, std::slice::from_ref(layer), &[], &[], &[]);
    data.tiles.iter().map(|(key, tile)| (*key, tile.identity())).collect()
}

fn changed(before: &BTreeMap<TileKey, u64>, after: &BTreeMap<TileKey, u64>) -> Vec<[u32; 2]> {
    after.iter().filter(|(key, id)| before.get(key) != Some(id)).map(|(key, _)| key.coordinate).collect()
}

#[test]
fn erase_follows_hard_soft_and_inverted_coverage_and_rewrites_only_covered_pages() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = painted(&mut r, 1);
    let tiles = |layer: &Layer| -> BTreeMap<TileKey, u64> {
        layer.raster.wait_data().unwrap().tiles.iter().map(|(k, t)| (*k, t.identity())).collect()
    };
    let start = tiles(&layer);
    assert_eq!(start.len(), 6, "three by two pages of paint");
    assert_eq!(at(&image(&mut r), [60, 60]), [255; 4]);

    let hard = erase_operation(rectangle([20., 20., 120., 120.]), false);
    let bounds = hard.bounds(EXTENT);
    let after = run(&mut r, &mut layer, hard, bounds, &[]);
    let pixels = image(&mut r);
    assert_eq!(at(&pixels, [60, 60])[3], 0, "inside the selection is erased");
    assert_eq!(at(&pixels, [200, 60]), [255; 4]);
    assert_eq!(at(&pixels, [60, 200]), [255; 4]);
    assert_eq!(changed(&start, &after), [[0, 0]], "only the selected page is rewritten");

    let row = EXTENT[0].div_ceil(4) as usize;
    let mut words = vec![0u32; row * EXTENT[1] as usize];
    for y in 20..120 {
        for x in (300..400).step_by(4) {
            words[y * row + x / 4] = 0x8080_8080;
        }
    }
    let soft = Selection::pixels(Arc::new(
        layer_core::SelectionPixels::bytes(EXTENT, [300, 20, 400, 120], words).unwrap(),
    ));
    let op = erase_operation(soft, false);
    let bounds = op.bounds(EXTENT);
    let next = run(&mut r, &mut layer, op, bounds, &[]);
    let pixels = image(&mut r);
    let alpha = at(&pixels, [340, 60])[3];
    assert!(alpha.abs_diff(127) <= 2, "half coverage erases half: {alpha}");
    assert_eq!(at(&pixels, [340, 200]), [255; 4]);
    assert_eq!(changed(&after, &next), [[1, 0]]);

    let locked = erase_operation(rectangle([220., 150., 300., 250.]), true);
    let bounds = locked.bounds(EXTENT);
    run(&mut r, &mut layer, locked, bounds, &[]);
    assert_eq!(at(&image(&mut r), [260, 200]), [255; 4], "alpha lock keeps the pixels");

    let mut outside = rectangle([400., 150., 560., 280.]);
    outside.inverted = true;
    let op = erase_operation(outside, false);
    let bounds = op.bounds(EXTENT);
    assert_eq!(bounds, Rect { min: Point::default(), max: Point { x: 600., y: 300. } });
    run(&mut r, &mut layer, op, bounds, &[]);
    let pixels = image(&mut r);
    assert_eq!(at(&pixels, [480, 200]), [255; 4], "inside the kept area stays");
    for point in [[300, 200], [580, 20], [10, 290]] {
        assert_eq!(at(&pixels, point)[3], 0, "{point:?} outside is erased");
    }
}

#[test]
fn a_whole_layer_erase_settles_watercolor_before_erasing() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = Layer::paint(LayerId(1), "watercolor erase");
    let mut stroke = batch(1);
    stroke.style = preset_style(layer_core::DefaultBrushPreset::WatercolorWash);
    stroke.damage = Rect { min: Point { x: 40., y: 40. }, max: Point { x: 260., y: 260. } };
    let mut ink = dab([0.2, 0.3, 0.8, 0.8]);
    ink.center = Point { x: 150., y: 150. };
    ink.radii = [100.; 2];
    ink.material = [0.5, 0.8, 1., 0.8];
    layer.raster = RasterRevision::pending();
    frame(&mut r, std::slice::from_ref(&layer), &[ink], &[stroke], &[]);
    layer.raster.wait_data().unwrap();
    assert!(r.paint_layers[0].watercolor.is_some());
    let before = image(&mut r);
    let full = Rect { min: Point::default(), max: Point { x: 600., y: 300. } };
    run(&mut r, &mut layer, erase_operation(rectangle([140., 20., 300., 300.]), false), full, &[]);
    assert!(r.paint_layers[0].watercolor.is_none(), "the layer's wet state is baked");
    assert!(layer.raster.wait_data().unwrap().watercolor.is_none());
    let after = image(&mut r);
    for x in (0..600).step_by(7) {
        for y in (0..300).step_by(7) {
            let (a, b) = (at(&before, [x, y]), at(&after, [x, y]));
            if x >= 140 {
                assert_eq!(b[3], 0, "{x},{y} is erased");
            } else {
                let premultiplied = |p: [u8; 4]| p.map(|c| u32::from(c) * u32::from(p[3]) / 255);
                let (pa, pb) = (premultiplied(a), premultiplied(b));
                assert!(
                    a[3].abs_diff(b[3]) <= 2 && pa.iter().zip(pb).all(|(a, b)| a.abs_diff(b) <= 2),
                    "{x},{y} keeps its settled look: {a:?} {b:?}"
                );
            }
        }
    }
}

#[test]
fn an_inserted_copy_starts_from_its_restore_source() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut source = painted(&mut r, 1);
    let pixels = source.raster.clone();
    source.visible = false;
    let mut copy = source.clone();
    copy.id = LayerId(2);
    copy.visible = true;
    let mut outside = rectangle([100., 50., 250., 200.]);
    outside.inverted = true;
    let op = erase_operation(outside, false);
    let bounds = op.bounds(EXTENT);
    copy.pending_operations.push(op);
    copy.raster = RasterRevision::pending();
    let command = DabBatch { kind: DabBatchKind::LayerOperation(0), dab_count: 0, damage: bounds, ..batch(2) };
    let layers = [copy.clone(), source.clone()];
    frame(&mut r, &layers, &[], &[command], &[(copy.id, pixels)]);
    copy.raster.wait_data().unwrap();
    copy.pending_operations.clear();
    frame(&mut r, &[copy, source], &[], &[], &[]);
    let pixels = image(&mut r);
    assert_eq!(at(&pixels, [175, 125]), [255; 4], "the copy holds the source's selected pixels");
    assert_eq!(at(&pixels, [400, 125])[3], 0, "and nothing outside the selection");
}
