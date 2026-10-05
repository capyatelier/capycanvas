//! Erase through selection coverage: bounded pages, soft coverage, inverted
//! coverage, alpha lock, watercolor settling and inserted copies.
use super::*;
use layer_core::raster::{RasterRevision, TileKey};
use std::collections::BTreeMap;

const EXTENT: [u32; 2] = [600, 300];

fn frame(r: &mut WgpuRasterizer, scene: SceneView<'_>, dabs: &[Dab], batches: &[DabBatch], restore: &[(SourceTarget, RasterRevision)]) {
    r.submit(FramePacket {
        dabs,
        dab_batches: batches,
        restore_rasters: restore,
        reset_layers: false,
        ..packet(scene, EXTENT)
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
fn painted(r: &mut WgpuRasterizer) -> Document {
    let mut layer = paint_document(EXTENT, "erased paint");
    let mut ink = dab([1.; 4]);
    ink.center = Point { x: 300., y: 150. };
    ink.radii = [400.; 2];
    let mut stroke = batch(target(&layer));
    stroke.damage = Rect { min: Point::default(), max: Point { x: 600., y: 300. } };
    paint_mut(&mut layer).raster = RasterRevision::pending();
    frame(r, layer.scene(), &[ink], &[stroke], &[]);
    paint_mut(&mut layer).raster.wait_data().unwrap();
    layer
}

fn erase_operation(selection: Selection, alpha_locked: bool) -> RasterOperation {
    let mut coverage = reveal_all(EXTENT, Point::default());
    coverage.source.default_coverage = f32::from(selection.inverted);
    coverage.source.initial = Some(selection);
    RasterOperation {
        placement: layer_core::Affine::IDENTITY,
        coverage,
        kind: RasterOperationKind::Erase { alpha_locked },
    }
}

/// Run `op` on `layer` with the given damage and return the published tiles.
fn run(
    r: &mut WgpuRasterizer,
    layer: &mut Document,
    op: RasterOperation,
    damage: Rect,
    restore: &[(SourceTarget, RasterRevision)],
) -> BTreeMap<TileKey, u64> {
    let command = DabBatch {
        kind: DabBatchKind::RasterOperation(0),
        dab_count: 0,
        damage,
        ..batch(target(layer))
    };
    paint_mut(layer).operations = Arc::new(vec![op]);
    paint_mut(layer).raster = RasterRevision::pending();
    frame(r, layer.scene(), &[], &[command], restore);
    let data = paint(layer).raster.wait_data().unwrap();
    paint_mut(layer).operations = Arc::default();
    frame(r, layer.scene(), &[], &[], &[]);
    data.tiles.iter().map(|(key, tile)| (*key, tile.identity())).collect()
}

fn changed(before: &BTreeMap<TileKey, u64>, after: &BTreeMap<TileKey, u64>) -> Vec<[u32; 2]> {
    after.iter().filter(|(key, id)| before.get(key) != Some(id)).map(|(key, _)| key.coordinate).collect()
}

#[test]
fn erase_follows_hard_soft_and_inverted_coverage_and_rewrites_only_covered_pages() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = painted(&mut r);
    let tiles = |layer: &Document| -> BTreeMap<TileKey, u64> {
        paint(layer).raster.wait_data().unwrap().tiles.iter().map(|(k, t)| (*k, t.identity())).collect()
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
    let mut layer = paint_document(EXTENT, "watercolor erase");
    let mut stroke = batch(target(&layer));
    stroke.style = preset_style(layer_core::DefaultBrushPreset::WatercolorWash);
    stroke.damage = Rect { min: Point { x: 40., y: 40. }, max: Point { x: 260., y: 260. } };
    let mut ink = dab([0.2, 0.3, 0.8, 0.8]);
    ink.center = Point { x: 150., y: 150. };
    ink.radii = [100.; 2];
    ink.material = [0.5, 0.8, 1., 0.8];
    paint_mut(&mut layer).raster = RasterRevision::pending();
    frame(&mut r, layer.scene(), &[ink], &[stroke], &[]);
    paint_mut(&mut layer).raster.wait_data().unwrap();
    assert!(r.paint_layers[0].watercolor.is_some());
    let before = image(&mut r);
    let full = Rect { min: Point::default(), max: Point { x: 600., y: 300. } };
    run(&mut r, &mut layer, erase_operation(rectangle([140., 20., 300., 300.]), false), full, &[]);
    assert!(r.paint_layers[0].watercolor.is_none(), "the layer's wet state is baked");
    assert!(paint_mut(&mut layer).raster.wait_data().unwrap().watercolor.is_none());
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
    let mut document = painted(&mut r);
    let source_owner = placement::occurrence_id(&document);
    let pixels = paint(&document).raster.clone();
    let copied = paint(&document).clone();
    occurrence_mut(&mut document).visible = false;
    let (copy, copy_target) = placement::append_paint(&mut document, "erased copy", copied);
    let root = document.composition().result;
    let stack = RecordChange::replace(&document.artwork.stacks,root,Some(layer_core::authored::Stack {entries:vec![copy,source_owner]})).unwrap();
    document.apply(layer_core::Edit::Stack(stack)).unwrap();
    let SourceTarget::Paint(copy_source) = copy_target else { unreachable!() };
    let mut outside = rectangle([100., 50., 250., 200.]);
    outside.inverted = true;
    let op = erase_operation(outside, false);
    let bounds = op.bounds(EXTENT);
    let source = document.artwork.paint.get_mut(copy_source).unwrap();
    source.operations = Arc::new(vec![op]);
    source.raster = RasterRevision::pending();
    let command = DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0, damage: bounds, ..batch(copy_target) };
    frame(&mut r, document.scene(), &[], &[command], &[(copy_target, pixels)]);
    document.artwork.paint.get(copy_source).unwrap().raster.wait_data().unwrap();
    document.artwork.paint.get_mut(copy_source).unwrap().operations = Arc::default();
    frame(&mut r, document.scene(), &[], &[], &[]);
    let pixels = image(&mut r);
    assert_eq!(at(&pixels, [175, 125]), [255; 4], "the copy holds the source's selected pixels");
    assert_eq!(at(&pixels, [400, 125])[3], 0, "and nothing outside the selection");
}

#[test]
fn command_coverage_is_independent_of_authored_masks_and_other_operations() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = painted(&mut r);
    let mut mask = reveal_all(EXTENT, Point::default());
    mask.source.default_coverage = 0.;
    mask.source.initial = Some(rectangle([0., 0., 300., 300.]));
    set_mask(&mut document, mask);
    frame(&mut r, document.scene(), &[], &[], &[]);
    let owner = placement::occurrence_id(&document);
    let authored = document.scene().occurrence(owner).unwrap().mask.as_ref().unwrap().source;
    let operations = [
        erase_operation(rectangle([20., 20., 120., 120.]), false),
        erase_operation(rectangle([180., 20., 220., 120.]), false),
    ];
    assert!(operations.iter().all(|op| op.coverage.target == authored));
    let batches: Vec<_> = operations.iter().enumerate().map(|(index, op)| DabBatch {
        kind: DabBatchKind::RasterOperation(index as u32), dab_count: 0,
        damage: op.bounds(EXTENT), ..batch(target(&document))
    }).collect();
    paint_mut(&mut document).operations = Arc::new(operations.into());
    paint_mut(&mut document).raster = RasterRevision::pending();
    frame(&mut r, document.scene(), &[], &batches, &[]);
    paint(&document).raster.wait_data().unwrap();
    paint_mut(&mut document).operations = Arc::default();
    frame(&mut r, document.scene(), &[], &[], &[]);
    let pixels = image(&mut r);
    assert_eq!(at(&pixels, [60, 60])[3], 0);
    assert_eq!(at(&pixels, [200, 60])[3], 0);
    assert_eq!(at(&pixels, [150, 60]), [255; 4]);
    assert_eq!(at(&pixels, [400, 60])[3], 0, "authored mask still clips the unedited paint");
    let mut occurrence = document.scene().occurrence(owner).unwrap().clone();
    occurrence.mask = None;
    let edit = layer_core::Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences, owner, Some(occurrence)).unwrap());
    document.apply(edit).unwrap();
    frame(&mut r, document.scene(), &[], &[], &[]);
    assert_eq!(at(&image(&mut r), [400, 60]), [255; 4], "commands did not rewrite the authored mask or its hidden paint");
    assert!(r.layer_masks.command_pages.is_empty(), "finished commands release transient coverage");
}

#[test]
fn bakes_freeze_mask_versions_and_keep_live_mask_pages_unchanged() {
    use layer_core::{SceneScope, raster::{RasterData, RasterPlane, RasterTile, TileBlob}};
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = painted(&mut r);
    set_mask(&mut document, reveal_all(EXTENT, Point::default()));
    let owner = placement::occurrence_id(&document);
    let mask = document.scene().mask(owner).unwrap().0.source;
    let mask_target = SourceTarget::Coverage(mask);
    let revision = |coverage: u8| RasterRevision::backed(RasterData {
        tiles: [(TileKey { plane: RasterPlane::Mask, coordinate: [0; 2] },
            RasterTile::backed(TileBlob::encode(RasterPlane::Mask.descriptor(Default::default()),
                &vec![coverage; (PAGE_SIZE * PAGE_SIZE) as usize]).unwrap()))].into(),
        ..Default::default()
    });
    let first = revision(64);
    {
        let source = document.artwork.coverage.get_mut(mask).unwrap();
        source.default_coverage = 0.;
        source.initial = Some(rectangle([0., 0., 300., 300.]));
        source.raster = first.clone();
    }
    frame(&mut r, document.scene(), &[], &[], &[(mask_target, first)]);
    let first_scene = document.snapshot();
    let second = revision(192);
    {
        let source = document.artwork.coverage.get_mut(mask).unwrap();
        source.initial = Some(rectangle([300., 0., 600., 300.]));
        source.raster = second.clone();
    }
    frame(&mut r, document.scene(), &[], &[], &[(mask_target, second)]);
    let second_scene = document.snapshot();
    let live = revision(255);
    {
        let source = document.artwork.coverage.get_mut(mask).unwrap();
        source.default_coverage = 1.;
        source.initial = None;
        source.raster = live.clone();
    }
    frame(&mut r, document.scene(), &[], &[], &[(mask_target, live)]);
    let live_source = document.artwork.coverage.get(mask).unwrap().clone();
    let live_pixels = page_bytes(&r, &r.layer_masks.pages[&(mask_target, [0; 2])].texture);
    document.artwork.occurrences.get_mut(owner).unwrap().visible = false;
    let mut outputs = Vec::new();
    let mut batches = Vec::new();
    for (name, scene) in [("First captured mask", first_scene), ("Second captured mask", second_scene)] {
        let operation = RasterOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage: reveal_all(EXTENT, Point::default()),
            kind: RasterOperationKind::Bake { scene, scope: SceneScope::Members(Arc::from([owner])), offset: Point::default() },
        };
        let damage = operation.bounds(EXTENT);
        let (handle, target) = placement::append_paint(&mut document, name, layer_core::PaintSource { color_mode: Default::default(),
            domain: EXTENT, raster: RasterRevision::pending(), base: None, operations: Arc::new(vec![operation]),
        });
        outputs.push((handle, target));
        batches.push(DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0, damage, ..batch(target) });
    }
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    while outputs.iter().any(|(_, target)| document.target_raster(*target).unwrap().try_data().is_none()) {
        let packet = FramePacket { dab_batches: &batches, ..packet(document.scene(), EXTENT) };
        if r.raster_dependencies_ready(packet) { r.submit(packet).unwrap(); }
        assert!(std::time::Instant::now() < deadline, "captured mask bakes did not settle");
        std::thread::yield_now();
    }
    for (_, target) in &outputs {
        let SourceTarget::Paint(handle) = target else { unreachable!() };
        document.artwork.paint.get_mut(*handle).unwrap().operations = Arc::default();
    }
    for (index, (handle, _)) in outputs.iter().enumerate() {
        for (other, _) in &outputs { document.artwork.occurrences.get_mut(*other).unwrap().visible = other == handle; }
        frame(&mut r, document.scene(), &[], &[], &[]);
        let pixels = image(&mut r);
        let alpha = at(&pixels, [60, 60])[3];
        assert!(alpha.abs_diff([64, 192][index]) <= 1, "bake {index} keeps its painted mask revision: {alpha}");
        assert_eq!(at(&pixels, [280, 60])[3], [255, 0][index], "bake {index} keeps its initial coverage");
        assert_eq!(at(&pixels, [400, 60])[3], [0, 255][index], "bake {index} has a distinct captured context");
    }
    assert_eq!(document.artwork.coverage.get(mask).unwrap(), &live_source);
    assert_eq!(page_bytes(&r, &r.layer_masks.pages[&(mask_target, [0; 2])].texture), live_pixels);
    assert!(r.layer_masks.snapshots.is_empty(), "completed bakes release their private mask pages");
}
