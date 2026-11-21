//! Erase through selection coverage: bounded pages, soft coverage, inverted
//! coverage, alpha lock, watercolor settling and inserted copies.
use super::*;
use layer_core::raster::{RasterPlane, RasterRevision, TileKey};
use std::collections::BTreeMap;

const EXTENT: [u32; 2] = [600, 300];

fn frame(r: &mut WgpuRasterizer, scene: SceneView<'_>, dabs: &[Dab], batches: &[DabBatch], restore: &[(SourceTarget, RasterRevision)]) {
    crate::test_support::submit_ready(r, FramePacket {
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
    erase_within(EXTENT, selection, alpha_locked)
}

fn erase_within(extent: [u32; 2], selection: Selection, alpha_locked: bool) -> RasterOperation {
    let mut coverage = reveal_all(extent, [0, 0]);
    coverage.source.default_coverage = f32::from(selection.inverted);
    coverage.selection = Some(selection);
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
fn a_whole_layer_erase_settles_watercolor_pages_in_any_order() {
    let settled = |reverse: bool| {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut layer = paint_document(EXTENT, "watercolor erase order");
        let mut stroke = batch(target(&layer));
        stroke.style = preset_style(layer_core::DefaultBrushPreset::WatercolorWash);
        stroke.damage = Rect { min: Point { x: 156., y: 40. }, max: Point { x: 356., y: 240. } };
        let mut ink = dab([0.2, 0.3, 0.8, 0.8]);
        ink.center = Point { x: 256., y: 140. };
        ink.radii = [100.; 2];
        ink.material = [0.5, 0.8, 1., 0.8];
        paint_mut(&mut layer).raster = RasterRevision::pending();
        frame(&mut r, layer.scene(), &[ink], &[stroke], &[]);
        paint_mut(&mut layer).raster.wait_data().unwrap();
        if reverse { r.paint_layers[0].pages.reverse(); }
        let full = Rect { min: Point::default(), max: Point { x: 600., y: 300. } };
        run(&mut r, &mut layer, erase_operation(rectangle([560., 260., 600., 300.]), false), full, &[]);
        assert!(r.paint_layers[0].watercolor.is_none());
        image(&mut r)
    };
    let (forward, backward) = (settled(false), settled(true));
    let differing = (0..600).flat_map(|x| (0..300).map(move |y| [x, y])).filter(|&point| at(&forward, point) != at(&backward, point)).collect::<Vec<_>>();
    assert!(differing.is_empty(), "{} pixels depend on page order, first {:?}", differing.len(), differing.first());
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
fn mask_coverage_operations_store_selection_pages_and_keep_the_default_elsewhere() {
    for inverted in [false, true] {
        let mut selection = Selection::polygon(vec![Point { x: 20., y: 30. }, Point { x: 300., y: 30. }, Point { x: 160., y: 200. }]).unwrap();
        selection.inverted = inverted;
        let mut source = reveal_all(EXTENT, [0, 0]).source;
        source.default_coverage = f32::from(inverted);
        crate::test_support::materialize_mask(&mut source, selection, Default::default());
        let data = source.raster.wait_data().unwrap();
        assert_eq!(data.tiles.keys().map(|key| key.coordinate).collect::<Vec<_>>(), [[0, 0], [1, 0]], "only pages the selection reaches");
        let bytes = data.tiles[&TileKey { plane: RasterPlane::Mask, coordinate: [0, 0] }].wait_backing().unwrap().decode_on_worker().unwrap();
        let at = |x: usize, y: usize| bytes[y * PAGE_SIZE as usize + x];
        assert_eq!(at(160, 100), if inverted { 0 } else { 255 });
        assert_eq!(at(20, 190), if inverted { 255 } else { 0 });
        assert!(bytes.iter().any(|v| *v != 0 && *v != 255), "edges keep the selection's antialiasing");
    }
}

#[test]
fn command_coverage_is_independent_of_authored_masks_and_other_operations() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = painted(&mut r);
    let mut mask = reveal_all(EXTENT, [0, 0]);
    mask.source.default_coverage = 0.;
    crate::test_support::materialize_mask(&mut mask.source, rectangle([0., 0., 300., 300.]), document.composition().color);
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
    set_mask(&mut document, reveal_all(EXTENT, [0, 0]));
    let owner = placement::occurrence_id(&document);
    let mask = document.scene().mask(owner).unwrap().0.source;
    let mask_target = SourceTarget::Coverage(mask);
    let revision = |coverage: u8| RasterRevision::backed(RasterData {
        tiles: [(TileKey { plane: RasterPlane::Mask, coordinate: [0; 2] },
            RasterTile::backed(TileBlob::encode(RasterPlane::Mask.descriptor(Default::default()),
                &vec![coverage; (PAGE_SIZE * PAGE_SIZE) as usize]).unwrap()))].into(),
        ..Default::default()
    });
    {
        let color = document.composition().color;
        let source = document.artwork.coverage.get_mut(mask).unwrap();
        source.default_coverage = 0.;
        source.raster = revision(64);
        crate::test_support::materialize_mask(source, rectangle([0., 0., 300., 300.]), color);
    }
    let first = document.artwork.coverage.get(mask).unwrap().raster.clone();
    frame(&mut r, document.scene(), &[], &[], &[(mask_target, first)]);
    let first_scene = document.snapshot();
    {
        let color = document.composition().color;
        let source = document.artwork.coverage.get_mut(mask).unwrap();
        source.raster = revision(192);
        crate::test_support::materialize_mask(source, rectangle([300., 0., 600., 300.]), color);
    }
    let second = document.artwork.coverage.get(mask).unwrap().raster.clone();
    frame(&mut r, document.scene(), &[], &[], &[(mask_target, second)]);
    let second_scene = document.snapshot();
    let live = revision(255);
    {
        let source = document.artwork.coverage.get_mut(mask).unwrap();
        source.default_coverage = 1.;
        source.raster = live.clone();
    }
    frame(&mut r, document.scene(), &[], &[], &[(mask_target, live)]);
    let live_source = document.artwork.coverage.get(mask).unwrap().clone();
    let live_pixels = crate::test_support::mask_page_bytes(&r,mask_target,[0;2]);
    document.artwork.occurrences.get_mut(owner).unwrap().visible = false;
    let mut outputs = Vec::new();
    let mut batches = Vec::new();
    for (name, scene) in [("First captured mask", first_scene), ("Second captured mask", second_scene)] {
        let operation = RasterOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage: reveal_all(EXTENT, [0, 0]),
            kind: RasterOperationKind::Bake { scene, scope: SceneScope::Members(Arc::from([owner])), offset: Point::default() },
        };
        let damage = operation.bounds(EXTENT);
        let (handle, target) = placement::append_paint(&mut document, name, layer_core::PaintSource { color_mode: Default::default(),
            domain: EXTENT, raster: RasterRevision::pending(), base: None, operations: Arc::new(vec![operation]),
        });
        outputs.push((handle, target));
        batches.push(DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0, damage, ..batch(target) });
    }
    crate::test_support::wait_until("captured mask bakes did not settle", || {
        if outputs.iter().all(|(_, target)| document.target_raster(*target).unwrap().try_data().is_some()) { return Ok(()); }
        let packet = FramePacket { dab_batches: &batches, ..packet(document.scene(), EXTENT) };
        if r.raster_dependencies_ready(packet, None) { r.submit(packet).unwrap(); }
        Err(())
    });
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
    assert_eq!(crate::test_support::mask_page_bytes(&r,mask_target,[0;2]), live_pixels);
    assert!(r.layer_masks.snapshots.is_empty(), "completed bakes release their private mask pages");
}

fn backing(layer: &Document) -> BTreeMap<TileKey, Vec<u8>> {
    paint(layer).raster.wait_data().unwrap().tiles.iter()
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode_on_worker().unwrap())).collect()
}

fn wash(extent: [u32; 2], center: [f32; 2], radius: f32) -> (WgpuRasterizer, Document) {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut layer = paint_document(extent, "watercolor wash");
    let mut ink = dab([0.2, 0.3, 0.8, 0.8]);
    ink.center = Point { x: center[0], y: center[1] };
    ink.radii = [radius; 2];
    ink.material = [0.5, 0.8, 1., 0.8];
    let mut stroke = batch(target(&layer));
    stroke.style = preset_style(layer_core::DefaultBrushPreset::WatercolorWash);
    stroke.damage = Rect { min: Point { x: center[0] - radius, y: center[1] - radius }, max: Point { x: center[0] + radius, y: center[1] + radius } };
    paint_mut(&mut layer).raster = RasterRevision::pending();
    crate::test_support::submit_ready(&mut r, FramePacket { dabs: &[ink], dab_batches: &[stroke], ..packet(layer.scene(), extent) }).unwrap();
    paint(&layer).raster.wait_data().unwrap();
    assert!(r.paint_layers[0].watercolor.is_some());
    (r, layer)
}

fn queue(layer: &mut Document, extent: [u32; 2], op: RasterOperation) -> DabBatch {
    paint_mut(layer).operations = Arc::new(vec![op]);
    paint_mut(layer).raster = RasterRevision::pending();
    DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0, damage: Rect::from_extent(extent), ..batch(target(layer)) }
}

fn settle(r: &mut WgpuRasterizer, layer: &mut Document, extent: [u32; 2], op: RasterOperation) -> BTreeMap<TileKey, Vec<u8>> {
    let command = queue(layer, extent, op);
    crate::test_support::submit_ready(r, FramePacket { dab_batches: &[command], ..packet(layer.scene(), extent) }).unwrap();
    let tiles = backing(layer);
    assert!(r.paint_layers[0].watercolor.is_none());
    assert!(paint(layer).raster.wait_data().unwrap().watercolor.is_none());
    tiles
}

fn settles_in_any_order(extent: [u32; 2], center: [f32; 2], radius: f32, pages: usize, orders: &[fn(&mut Vec<LayerPage>)],
    operation: impl Fn(&mut WgpuRasterizer, &mut Document) -> RasterOperation,
) {
    let settled: Vec<_> = orders.iter().map(|order| {
        let (mut r, mut layer) = wash(extent, center, radius);
        assert_eq!(r.paint_layers[0].pages.len(), pages);
        let before = backing(&layer);
        order(&mut r.paint_layers[0].pages);
        let op = operation(&mut r, &mut layer);
        (before, settle(&mut r, &mut layer, extent, op))
    }).collect();
    let (wet, first) = &settled[0];
    for (before, tiles) in &settled[1..] {
        assert!(before == wet, "the washes differ before they settle");
        let seams: Vec<_> = first.iter().filter(|(key, tile)| tiles.get(key) != Some(tile)).map(|(key, _)| key.coordinate).collect();
        assert!(seams.is_empty() && tiles.len() == first.len(), "pages {seams:?} depend on the order their neighbours settled in");
    }
}

#[test]
fn a_watercolor_erase_bakes_both_pages_from_one_pigment_revision() {
    let extent = [384, 256];
    settles_in_any_order(extent, [256., 120.], 64., 2, &[|_| {}, |pages| pages.reverse()],
        |_, _| erase_within(extent, rectangle([368., 240., 384., 256.]), false));
}

#[test]
fn applying_a_mask_bakes_both_watercolor_pages_from_one_pigment_revision() {
    let extent = [384, 256];
    settles_in_any_order(extent, [256., 120.], 64., 2, &[|_| {}, |pages| pages.reverse()], |r, layer| {
        let mut mask = reveal_all(extent, [0, 0]);
        mask.source.default_coverage = 0.;
        crate::test_support::materialize_mask(&mut mask.source, rectangle([0., 0., 384., 240.]), layer.composition().color);
        set_mask(layer, mask);
        crate::test_support::submit_ready(r, packet(layer.scene(), extent)).unwrap();
        let coverage = mask_snapshot(layer);
        occurrence_mut(layer).mask = None;
        RasterOperation { placement: layer_core::Affine::IDENTITY, coverage, kind: RasterOperationKind::ApplyMask }
    });
}

#[test]
fn a_watercolor_erase_settles_three_rows_of_pages_in_any_order() {
    let extent = [768; 2];
    settles_in_any_order(extent, [384.; 2], 200., 9,
        &[|_| {}, |pages| pages.reverse(), |pages| pages.sort_by_key(|page| (page.coordinate[0] + 3 * page.coordinate[1]) * 4 % 9)],
        |_, _| erase_within(extent, rectangle([752., 752., 768., 768.]), false));
}

#[test]
fn a_watercolor_erase_settles_a_page_the_same_whatever_it_erases_on_its_neighbour() {
    let extent = [384, 256];
    for (page, beside, apart) in [([1, 0], [0., 0., 255., 256.], [0., 240., 16., 256.]), ([0, 0], [257., 0., 384., 256.], [368., 240., 384., 256.])] {
        let [beside, apart] = [beside, apart].map(|selection| {
            let (mut r, mut layer) = wash(extent, [256., 120.], 64.);
            settle(&mut r, &mut layer, extent, erase_within(extent, rectangle(selection), false))
                .remove(&TileKey { plane: RasterPlane::Color, coordinate: page }).unwrap()
        });
        assert!(beside == apart, "page {page:?} settled from pigment the erase had already taken from its neighbour");
    }
}

#[test]
fn a_watercolor_erase_holds_its_shorter_side_of_outputs_and_frees_its_scratch() {
    let extent = [1024, 512];
    let (mut r, mut layer) = wash(extent, [512., 256.], 300.);
    assert_eq!(r.paint_layers[0].pages.len(), 8, "four columns of two pages");
    drop(r.scene.as_mut().unwrap().trim_scratch(0));
    assert_eq!(r.scene.as_ref().unwrap().pool_bytes(), 0);
    settle(&mut r, &mut layer, extent, erase_within(extent, rectangle([1008., 496., 1024., 512.]), false));
    let page = u64::from(PAGE_SIZE).pow(2) * u64::from(r.device.working_format().block_copy_size(None).unwrap());
    let scene = r.scene.as_mut().unwrap();
    assert!(scene.pool_bytes() <= 3 * page, "more than two outputs and one resolve at once: {} bytes", scene.pool_bytes());
    drop(scene.trim_scratch(0));
    assert_eq!(scene.pool_bytes(), 0, "the erase kept scratch after it finished");
}

#[test]
fn a_watercolor_erase_waits_for_its_scratch_and_then_settles_the_same() {
    let extent = [768; 2];
    let erase = || erase_within(extent, rectangle([752., 752., 768., 768.]), false);
    let (mut roomy, mut control) = wash(extent, [384.; 2], 200.);
    let expected = settle(&mut roomy, &mut control, extent, erase());
    let (mut r, mut layer) = wash(extent, [384.; 2], 200.);
    drop(r.scene.as_mut().unwrap().trim_scratch(0));
    let page = u64::from(PAGE_SIZE).pow(2) * u64::from(r.device.working_format().block_copy_size(None).unwrap());
    r.scene_tiers.gpu.update_limit(r.scene_tiers.gpu.used() + 3 * page);
    let written = |r: &WgpuRasterizer| (r.paint_layers[0].pages.iter().map(|page| page.active().name()).collect::<Vec<_>>(), r.metrics.submissions);
    let before = written(&r);
    let command = queue(&mut layer, extent, erase());
    let frame = FramePacket { dab_batches: &[command], ..packet(layer.scene(), extent) };
    crate::test_support::wait_until("the erase did not reserve the scratch there was", || {
        assert!(!r.raster_dependencies_ready(frame, None), "an erase without its scratch is not ready");
        r.wait_idle().unwrap();
        if r.scene.as_ref().unwrap().pool_bytes() == 3 * page { Ok(()) } else { Err(()) }
    });
    assert!(!r.raster_dependencies_ready(frame, None), "an erase holding part of its scratch is not ready");
    assert!(written(&r) == before, "a waiting erase wrote a page or submitted its frame");
    assert!(r.paint_layers[0].watercolor.is_some() && paint(&layer).raster.try_data().is_none(), "a waiting erase settled or published");
    r.scene_tiers.gpu.update_limit(u64::MAX);
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    assert!(backing(&layer) == expected, "the erase settled differently after waiting for its scratch");
    assert_eq!(r.scene.as_ref().unwrap().pool_bytes(), 4 * page, "the erase used other scratch than its preflight reserved");
    drop(r.scene.as_mut().unwrap().trim_scratch(0));
    assert_eq!(r.scene.as_ref().unwrap().pool_bytes(), 0, "the erase kept scratch after it finished");
}
