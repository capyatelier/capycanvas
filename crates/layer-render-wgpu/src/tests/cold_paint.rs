//! A zero-sized mutable cache forces native artwork through its lossless backing.
//! Compare complete consumers, not just allocation counts or the decoded cache.
use super::*;
use crate::test_support::complete;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
use layer_core::{Document, authored::{PortableId,SourceTarget,PaintHandle,PaintSource}};
use layer_render::{ColorSampleArea, ColorSampleRequest, ColorSampleSource, ThumbnailTarget};

const EXTENT: [u32; 2] = [4352, 512]; // 33 backed tiles and one transparent hole.
const RESIDENT: u64 = 1 << 40;
fn document(color: DocumentColor) -> Document {
    let mut document = Document::new(PortableId::random(), EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let root = document.artwork.root;
    document.artwork.compositions.get_mut(root).unwrap().color = color;
    let paper = document.scene().order()[1];
    document.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    let mut data = RasterData::default();
    for y in 0..2 {
        for x in 0..17 {
            if [x, y] == [3, 0] {
                continue;
            }
            let pixel = [((x * 2351 + y * 71) % 65536) as u16, 32123, 51007, 65535];
            let pixel: Vec<_> = match color.depth {
                SampleDepth::F16 | SampleDepth::F32 => unreachable!("SDR-only fixture"),
                SampleDepth::U16 => pixel.into_iter().flat_map(u16::to_le_bytes).collect(),
                SampleDepth::U8 => pixel.map(|v| (v >> 8) as u8).to_vec(),
            };
            data.tiles.insert(
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [x, y],
                },
                RasterTile::backed(
                    TileBlob::encode(color.paint_descriptor(), &pixel.repeat(256 * 256)).unwrap(),
                ),
            );
        }
    }
    paint_mut(&mut document).raster = RasterRevision::backed(data);
    document
}
fn paint_handle(document: &Document) -> PaintHandle { document.artwork.paint.iter().next().unwrap().0 }
fn target(document: &Document) -> SourceTarget { SourceTarget::Paint(paint_handle(document)) }
fn paint(document: &Document) -> &PaintSource { document.artwork.paint.get(paint_handle(document)).unwrap() }
fn paint_mut(document: &mut Document) -> &mut PaintSource { let target = paint_handle(document); document.artwork.paint.get_mut(target).unwrap() }

fn packet(document: &Document, all: bool) -> FramePacket<'_> {
    FramePacket {
        view: crate::test_support::view(EXTENT),
        composite_all: all,
        ..crate::test_support::packet(document.scene(), EXTENT)
    }
}
struct ColdRenderer {
    pressure: bool,
    renderer: WgpuRasterizer,
}
impl std::ops::Deref for ColdRenderer {
    type Target = WgpuRasterizer;
    fn deref(&self) -> &Self::Target { &self.renderer }
}
impl std::ops::DerefMut for ColdRenderer {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.renderer }
}
impl ColdRenderer {
    fn new(color: DocumentColor) -> Self {
        let renderer = crate::memory::memory_tests::renderer(color, layer_core::DeviceMemoryFacts {
            budget_bytes: Some(4 << 30), unified: true, ..Default::default()
        }).unwrap();
        Self { pressure: false, renderer }
    }
    fn begin_pressure(&mut self) {
        complete(&self.renderer);
        crate::memory::memory_tests::cold_color_pressure(&mut self.renderer);
        self.pressure = true;
        self.idle();
    }
    fn idle(&mut self) {
        if !self.pressure { return; }
        complete(&self.renderer);
        self.renderer.publish_memory();
        self.factory.memory_budget().flush_reports();
        self.renderer.refresh_memory_limits();
        self.renderer.shader_idle(true, true);
        self.renderer.after_present();
        complete(&self.renderer);
        trim_cache(&mut self.renderer, &[], &[]);
    }
    fn assert_cold_capacity(&self) {
        assert!(self.memory.raster_grant() <= self.raster_memory_report().pinned_gpu_bytes + self.raster_buffers.retained.used(),
            "headroom={}, pinned paint={}", self.factory.memory_budget().import_headroom_bytes(), self.pinned_paint_bytes());
    }
    fn submit(&mut self, frame: FramePacket<'_>) -> Result<(), GpuRasterError> {
        crate::test_support::submit_published(&mut self.renderer, frame);
        if frame.dabs.is_empty() && frame.dab_batches.is_empty() && self.transform_preview.is_none() { self.idle(); }
        Ok(())
    }
}
fn renderer(document: &Document, limit: u64) -> ColdRenderer { renderer_with(document, limit, packet(document, true)) }
fn renderer_with(document: &Document, limit: u64, frame: FramePacket<'_>) -> ColdRenderer {
    let mut r = ColdRenderer::new(document.composition().color);
    crate::memory::memory_tests::declare_source_budget(&mut r.renderer, frame, 64, 16 << 20, 44 << 20);
    crate::test_support::submit_settled(&mut r.renderer, frame);
    if limit == 0 {
        r.begin_pressure();
        settle(&mut r, FramePacket { composite_all: true, ..frame });
        r.assert_cold_capacity();
    }
    r
}
fn settle(r: &mut ColdRenderer, frame: FramePacket<'_>) {
    crate::test_support::submit_settled(&mut r.renderer, frame);
    if frame.dabs.is_empty() && frame.dab_batches.is_empty() && r.transform_preview.is_none() { r.idle(); }
}
fn trim_color(r: &mut WgpuRasterizer, bytes: u64, batches: &[DabBatch], tiles: &[Vec<BrushTile>]) {
    complete(r);
    let excess = r.color_memory_bytes().saturating_sub(bytes);
    if excess != 0 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        r.evict_raster(excess + r.raster_buffers.retained.used(), batches, tiles, &mut encoder);
        encoder.submit(&r.queue);
    }
    r.refresh_storage_metrics();
}
fn trim_cache(r: &mut WgpuRasterizer, batches: &[DabBatch], tiles: &[Vec<BrushTile>]) {
    complete(r);
    crate::memory::memory_tests::evict_to_grant(r, batches, tiles);
    r.refresh_storage_metrics();
}
fn image(r: &WgpuRasterizer) -> Vec<u8> {
    crate::layer_tests::page_bytes(r, crate::test_support::document_texture(r))
}
fn close(a: &[u8], b: &[u8]) {
    assert_eq!(a.len(), b.len());
    for (i, (a, b)) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0.iter()).enumerate() {
        let a = f32::from_le_bytes(*a);
        let b = f32::from_le_bytes(*b);
        assert!((a - b).abs() <= 3e-6, "working sample {i}: {a} != {b}");
    }
}
fn thumbnail(r: &mut WgpuRasterizer, id: SourceTarget) -> Vec<u8> {
    crate::test_support::thumbnail(r, 7, ThumbnailTarget::Source(id)).bytes
}
fn sample(
    r: &mut WgpuRasterizer,
    id: SourceTarget,
    position: [u32; 2],
    area: ColorSampleArea,
) -> [f32; 4] {
    crate::test_support::color_sample(r, ColorSampleRequest {
        request_id: 8, source: ColorSampleSource::Source(id), position, area,
    }).rgba
}
fn backing(root: &RasterRevision) -> std::collections::BTreeMap<TileKey, Vec<u8>> {
    root.wait_data()
        .unwrap()
        .tiles
        .iter()
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode_on_worker().unwrap()))
        .collect()
}

#[test]
fn stale_thumbnail_batches_cancel_without_publishing_and_leave_live_artwork_available() {
    use layer_core::authored::*;
    let original = document(DocumentColor::default());
    let stale_source = target(&original);
    let stale_occurrence = original.scene().source_owner(stale_source).unwrap();
    let mut r = renderer(&original, 0);
    let stale = ThumbnailTarget::Occurrence(stale_occurrence);
    r.prepare_thumbnail_batch(stale).unwrap();
    assert!(!r.thumbnails_pending());

    let mut artwork = original.artwork.clone();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.retain(|handle| *handle != stale_occurrence);
    artwork.occurrences.remove(stale_occurrence).unwrap();
    let SourceTarget::Paint(handle) = stale_source else { unreachable!() };
    artwork.paint.remove(handle).unwrap();
    let (live_occurrence, live_source) = crate::test_support::add_paint(&mut artwork, "Live ink", EXTENT);
    let SourceTarget::Paint(live_handle) = live_source else { unreachable!() };
    let bytes = [0, 0, 255, 255].repeat(256 * 256);
    let root = RasterRevision::backed(RasterData {tiles: [(TileKey {plane: RasterPlane::Color, coordinate: [0, 0]},
        RasterTile::backed(TileBlob::encode(original.composition().color.paint_descriptor(), &bytes).unwrap()))].into(), ..Default::default()});
    artwork.paint.get_mut(live_handle).unwrap().raster = root.clone();
    let current = Document::from_artwork(artwork).unwrap();
    crate::test_support::submit_ready(&mut r, packet(&current, true)).unwrap();
    complete(&r);
    let before = backing(&root);
    for target in [stale, ThumbnailTarget::Source(stale_source)] {
        assert_eq!(r.prepare_thumbnail_batch(target), Err(GpuRasterError::ThumbnailUnavailable(target)));
        assert_eq!(r.request_thumbnail(99, target), Err(GpuRasterError::ThumbnailUnavailable(target)));
        assert!(r.take_thumbnail().is_none());
        assert!(!r.thumbnails_pending());
    }
    let live = ThumbnailTarget::Occurrence(live_occurrence);
    let image = crate::test_support::thumbnail(&mut r, 77, live);
    assert_eq!((image.request_id, image.width, image.height), (77, 32, 32));
    assert!(image.bytes.as_chunks::<4>().0.iter().all(|pixel| *pixel == [0, 0, 255, 255]));
    assert!(!r.thumbnails_pending());
    assert_eq!(backing(&root), before);
    r.submit(packet(&current, true)).unwrap();
    complete(&r);
    assert_eq!(backing(&root), before);
}

#[test]
fn drained_headroom_keeps_a_decoded_batch_for_native_thumbnails() {
    let document = document(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
    let id = target(&document);
    let mut resident = renderer(&document, RESIDENT);
    let expected = thumbnail(&mut resident, id);
    let mut cold = renderer(&document, 0);
    let budget = cold.factory.memory_budget().clone();
    let _import = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    cold.renderer.refresh_memory_limits();
    cold.idle();
    cold.publish_memory();
    budget.flush_reports();
    cold.renderer.refresh_memory_limits();
    assert!(cold.source_tiles.borrow().free_slots() >= SOURCE_SLOTS + 4);
    assert_eq!(thumbnail(&mut cold, id), expected);
    assert_eq!(cold.metrics.thumbnail_pages, 33);
    assert!(cold.paint_layers[0].pages.is_empty());
}

#[test]
fn native_thumbnail_preparation_reduces_pages_in_bounded_batches() {
    let p = document(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
    let id = target(&p);
    let mut cold = renderer(&p, 0);
    let (polls, reduced) = polls_until_ready(&mut cold, id);
    assert_eq!((polls, reduced), (33usize.div_ceil(crate::thumbnails::BATCH_PAGES), 33),
        "each backed page is reduced once, a batch per poll");
    let mut resident = renderer(&p, RESIDENT);
    assert_eq!(polls_until_ready(&mut resident, id), (33usize.div_ceil(crate::thumbnails::BATCH_PAGES), 33),
        "resident pages reduce one bounded batch per poll");
    assert_eq!(thumbnail(&mut cold, id), thumbnail(&mut resident, id));
}

#[test]
fn cold_native_color_composition_sampling_and_thumbnails_match_resident_tiles() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let p = document(DocumentColor { space, depth });
            let id = target(&p);
            let mut resident = renderer(&p, RESIDENT);
            let mut cold = renderer(&p, 0);
            assert_eq!(resident.paint_layers[0].pages.len(), 33);
            assert!(cold.paint_layers[0].pages.is_empty());
            close(&image(&cold), &image(&resident));
            let a = thumbnail(&mut cold, id);
            let b = thumbnail(&mut resident, id);
            assert!(
                a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1),
                "thumbnail {space:?} {depth:?}"
            );
            for point in [[257, 257], [4331, 510], [3 * 256 + 80, 80]] {
                for area in [ColorSampleArea::Point, ColorSampleArea::Average5] {
                    let a = sample(&mut cold, id, point, area);
                    let b = sample(&mut resident, id, point, area);
                    assert!(
                        a.into_iter().zip(b).all(|(a, b)| (a - b).abs() <= 3e-6),
                        "sample {space:?} {depth:?} {point:?}"
                    );
                }
            }
            let region = |r: &mut WgpuRasterizer| {
                crate::test_support::receive_request(r, layer_render::RegionRequest { fill_areas: None, colour_mode: Default::default(), fill: None, reference_count: None, paper: None,
                        enclosure: None, contiguous: true,
                        selection: None,
                        request_id: 9,
                        source: layer_render::RegionSource::Source(id),
                        position: [259, 258],
                        tolerance: 0.,
                        refinement: Default::default(),
                        limit: None,
                    }).pixels
            };
            assert_eq!(
                region(&mut cold),
                region(&mut resident),
                "raw region {space:?} {depth:?}"
            );
            assert!(
                cold.paint_layers[0].pages.is_empty(),
                "read-only consumers must not materialize mutable tiles"
            );
            let unique_samples: std::collections::BTreeSet<_> = paint(&p).raster
                .wait_data().unwrap().tiles.values()
                .map(|tile| tile.wait_backing().unwrap().content_digest().unwrap()).collect();
            assert_eq!(cold.source_cache_work()[1], unique_samples.len() as u64);
            assert_eq!(cold.source_tiles.borrow().evictions, 0, "resident capacity holds every unique sample");
        }
    }
}

#[test]
fn cold_native_paint_rehydrates_only_damage_and_keeps_other_tiles_in_saved_revisions() {
    let mut p = document(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    });
    let original = paint(&p).raster.clone();
    let original_bytes = backing(&original);
    let id = target(&p);
    let mut r = renderer(&p, 0);
    let before = image(&r);
    let dabs = [test_dab([275., 275.], [0.13, 0.72, 0.41, 0.37], 0.5)];
    let batches = [crate::test_support::dab_batch(
        id,
        test_style(BrushExecution::Dry),
        Rect { min: Point { x: 257., y: 257. }, max: Point { x: 295., y: 295. } },
    )];
    paint_mut(&mut p).raster = RasterRevision::pending();
    crate::test_support::submit_ready(&mut r, FramePacket {
        dabs: &dabs,
        dab_batches: &batches,
        ..packet(&p, false)
    })
    .unwrap();
    let edited = paint(&p).raster.clone();
    let edited_bytes = backing(&edited);
    assert_eq!(
        edited_bytes.len(),
        original_bytes.len(),
        "cold tiles must not disappear at publication"
    );
    for (key, data) in &original_bytes {
        if key.coordinate != [1, 1] {
            assert_eq!(&edited_bytes[key], data);
        }
    }
    assert_ne!(
        edited_bytes[&TileKey {
            plane: RasterPlane::Color,
            coordinate: [1, 1]
        }],
        original_bytes[&TileKey {
            plane: RasterPlane::Color,
            coordinate: [1, 1]
        }]
    );
    assert_eq!(r.paint_layers[0].pages.len(), 1);
    let changed = image(&r);
    assert_ne!(changed, before);
    settle(&mut r, packet(&p, true));
    assert!(
        r.paint_layers[0].pages.is_empty(),
        "completed color may leave the mutable cache"
    );
    close(&image(&r), &changed);
    let reopened = super::native_effects::roundtrip(p.clone());
    assert_eq!(backing(&paint(&reopened).raster), edited_bytes);
    let replacement = renderer(&reopened, 0);
    close(&image(&replacement), &changed);
    paint_mut(&mut p).raster = original;
    settle(&mut r, packet(&p, false));
    assert_eq!(backing(&paint(&p).raster), original_bytes);
    close(&image(&r), &before);
    paint_mut(&mut p).raster = edited;
    settle(&mut r, packet(&p, false));
    close(&image(&r), &changed);
}

#[test]
fn cold_native_transform_snapshots_keep_original_tiles_through_preview_and_cancel() {
    let p = document(DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    });
    let id = target(&p);
    let mut resident = renderer(&p, RESIDENT);
    let mut cold = renderer(&p, 0);
    let original = image(&cold);
    for offset in [Point { x: 17., y: -11. }, Point { x: -31., y: 9. }] {
        let preview = layer_render::TransformPreview {
            transaction: 1,
            moving: false,
            target: id,
            selection: None,
            transform: layer_core::ImageTransform::affine(layer_core::Affine::translation(offset)),
        };
        for r in [&mut resident, &mut cold] {
            r.set_transform_preview(Some(&preview)).unwrap();
            settle(r, packet(&p, false));
        }
        close(&image(&cold), &image(&resident));
    }
    for r in [&mut resident, &mut cold] {
        r.set_transform_preview(None).unwrap();
        settle(r, packet(&p, false));
    }
    close(&image(&cold), &original);
    close(&image(&cold), &image(&resident));
    cold.submit(packet(&p, true)).unwrap();
    assert!(cold.paint_layers[0].pages.is_empty());
    close(&image(&cold), &original);
}

#[test]
fn cold_native_overrides_keep_the_original_photo_and_its_thumbnail_contributions() {
    use layer_core::color::{ColorProfile, source::*};
    let mut p = document(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    });
    let mut source = SourceBuilder::new(
        EXTENT,
        SourceInterpretation {
            channels: SourceChannels::Rgb,
            depth: SampleDepth::U8,
            profile: ColorProfile::Builtin(RgbSpace::AdobeRgb),
            profile_assumed: false,
        },
        16 * 1024 * 1024,
    )
    .unwrap();
    for _ in 0..EXTENT[1] {
        source
            .push_row(&[31, 201, 133].repeat(EXTENT[0] as usize))
            .unwrap();
    }
    paint_mut(&mut p).base = Some(layer_core::authored::PaintBase::new((Arc::new(source.finish().unwrap())).into()));
    let id = target(&p);
    let mut resident = renderer(&p, RESIDENT);
    let mut cold = renderer(&p, 0);
    close(&image(&cold), &image(&resident));
    pollster::block_on(crate::thumbnails::prepare_pipelines(&mut cold));
    let mut preparations = 1;
    while !cold.prepare_thumbnail_batch(ThumbnailTarget::Source(id)).unwrap() {
        preparations += 1;
        assert!(preparations < 100);
        complete(&cold);
        pollster::block_on(cold.await_image_decode()).unwrap();
    }
    assert!(preparations >= (34usize + 33).div_ceil(4), "photo overrides must share the four-tile preparation budget: {preparations}");
    let a = crate::test_support::thumbnail(&mut cold, 7, ThumbnailTarget::Source(id)).bytes;
    let b = crate::test_support::thumbnail(&mut resident, 7, ThumbnailTarget::Source(id)).bytes;
    assert!(a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1));
    for point in [[257, 257], [3 * 256 + 80, 80]] {
        let a = sample(&mut cold, id, point, ColorSampleArea::Average5);
        let b = sample(&mut resident, id, point, ColorSampleArea::Average5);
        assert!(a.into_iter().zip(b).all(|(a, b)| (a - b).abs() <= 3e-6));
        assert_eq!(a[3], 1., "holes in paint must reveal the original photo");
    }
    assert!(cold.paint_layers[0].pages.is_empty());
}

#[test]
fn cold_native_neighborhood_brushes_keep_prediction_and_terminal_backing() {
    for execution in [
        BrushExecution::Dry,
        BrushExecution::Smudge,
        BrushExecution::Wet,
        BrushExecution::Liquify,
        BrushExecution::Watercolor,
    ] {
        let mut a = document(DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        });
        let mut b = a.clone();
        let id = target(&a);
        let mut resident = renderer(&a, RESIDENT);
        let mut cold = renderer(&b, 0);
        let mut style = test_style(execution);
        style.wet_mix.amount_of_paint = 0.3;
        style.wet_mix.dilution = 0.4;
        style.wet_mix.pull = 0.9;
        style.wet_mix.blur = 1.;
        style.wet_mix.wetness = 0.7;
        let mut dab = test_dab([257., 255.], [0.1, 0.2, 0.8, 0.7], 0.6);
        dab.radii = [14., 10.];
        dab.motion = [13.25, -24.5];
        dab.material = [0., 0.8, 0.9, 0.7];
        let mut batch = DabBatch {
            stroke_end: false,
            ..crate::test_support::dab_batch(
                id,
                style,
                Rect { min: Point { x: 225., y: 225. }, max: Point { x: 288., y: 284. } },
            )
        };
        for phase in 0..4 {
            if phase == 1 {
                batch.kind = DabBatchKind::Preview;
                batch.stroke_start = false;
                dab.center.x += 9.;
            }
            if phase == 2 {
                batch.kind = DabBatchKind::Persistent;
                batch.material_update = 1;
            }
            if phase == 3 {
                batch.dab_count = 0;
                batch.stroke_end = true;
                batch.material_update = 2;
                paint_mut(&mut a).raster = RasterRevision::pending();
                paint_mut(&mut b).raster = RasterRevision::pending();
            }
            for (r, p) in [(&mut resident, &a), (&mut cold, &b)] {
                r.submit(FramePacket {
                    dabs: &[dab],
                    dab_batches: std::slice::from_ref(&batch),
                    ..packet(p, false)
                })
                .unwrap();
            }
            close(&image(&cold), &image(&resident));
        }
        assert_eq!(
            backing(&paint(&a).raster),
            backing(&paint(&b).raster),
            "{execution:?}"
        );
        cold.submit(packet(&b, true)).unwrap();
        resident.submit(packet(&a, true)).unwrap();
        assert!(cold.paint_layers[0].pages.is_empty());
        close(&image(&cold), &image(&resident));
    }
}

#[test]
fn native_cache_keeps_prepared_brush_pages() {
    for execution in [BrushExecution::Dry, BrushExecution::Wet] {
        let mut a = document(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
        let mut b = a.clone();
        let id = target(&a);
        let mut resident = renderer(&a, 256 * 1024 * 1024);
        let mut bounded = renderer(&b, 256 * 1024 * 1024);
        let data = paint(&b).raster.wait_data().unwrap();
        paint_mut(&mut a).raster = RasterRevision::pending();
        paint_mut(&mut b).raster = RasterRevision::pending();
        let style = if execution == BrushExecution::Dry {
            DabStyle::for_brush(&layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), layer_core::StrokeTool::Brush)
        } else { test_style(execution) };
        let mut dab = test_dab([257., 255.], [0.1, 0.2, 0.8, 0.7], 0.6);
        dab.radii = [14., 10.];
        dab.motion = [13.25, -24.5];
        let batch = crate::test_support::dab_batch(id, style, dab.bounds());
        let frame = FramePacket { dabs: &[dab], dab_batches: std::slice::from_ref(&batch), ..packet(&b, false) };
        let mut needs=compile::Needs::default();
        needs.style(&bounded,&batch.style,dry_material::Reach::of_dabs(frame.dabs),false,false);
        needs.compute.extend(bounded.native_edit.as_ref().unwrap().required_pipelines(b.composition().color.depth,false).cloned());
        pollster::block_on(needs.prepare(&bounded.compilation)).unwrap();
        assert!(bounded.raster_dependencies_ready(frame, None));
        let required = bounded.restore_preflight.as_ref().unwrap().dependencies().to_vec();
        assert!(!required.is_empty());
        bounded.begin_pressure();
        let tiles = brush_tiles::plan_packet(frame, |target| b.scene().target_extent(target)).unwrap();
        trim_cache(&mut bounded, frame.dab_batches, &tiles);
        for (target, coordinates) in &required {
            let layer = bounded.paint_layers.iter().find(|layer| layer.id == *target).unwrap();
            for coordinate in coordinates {
                if data.tiles.contains_key(&TileKey {plane: RasterPlane::Color, coordinate: *coordinate}) {
                    assert!(layer.pages.iter().any(|page| page.coordinate == *coordinate), "{execution:?}: required {coordinate:?}");
                }
            }
        }
        assert!(bounded.paint_layers[0].pages.len() < resident.paint_layers[0].pages.len());
        crate::test_support::submit_published(&mut bounded, frame);
        crate::test_support::submit_published(&mut resident, FramePacket {scene: a.scene(), ..frame});
        complete(&bounded);
        complete(&resident);
        close(&image(&bounded), &image(&resident));
        bounded.retire_stroke_sources();
        bounded.idle();
        trim_cache(&mut bounded, &[], &[]);
        assert!(bounded.paint_layers[0].pages.is_empty());
    }
}

#[test]
fn a_finished_strokes_coverage_yields_to_raster_eviction_without_changing_pixels() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U8 };
    let (mut a, mut b) = (document(color), document(color));
    let id = target(&a);
    let mut resident = renderer(&a, RESIDENT);
    let mut evicting = renderer(&b, RESIDENT);
    let style = DabStyle::for_brush(&layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), layer_core::StrokeTool::Brush);
    let owned = |r: &WgpuRasterizer, stroke| r.paint_layers.iter().flat_map(|l| &l.coverage_pages).filter(|p| p.owner == Some(layer_core::StrokeId(stroke))).count();
    let evict = |r: &mut WgpuRasterizer| {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        r.evict_raster(u64::MAX, &[], &[], &mut encoder);
        encoder.submit(&r.queue);
        complete(r);
    };
    let mut revisions = Vec::new();
    for (stroke, x) in [(1, 300.), (2, 2860.), (3, 310.)] {
        let mut dab = test_dab([x, 255.], [0.1, 0.2, 0.8, 0.7], 0.6);
        dab.radii = [14., 10.];
        let batch = DabBatch { stroke_id: layer_core::StrokeId(stroke), stroke_start: true, stroke_end: true,
            ..crate::test_support::dab_batch(id, style.clone(), dab.bounds()) };
        for (r, p) in [(&mut resident, &mut a), (&mut evicting, &mut b)] {
            paint_mut(p).raster = RasterRevision::pending();
            r.submit(FramePacket { dabs: &[dab], dab_batches: std::slice::from_ref(&batch), ..packet(p, false) }).unwrap();
            complete(r);
            paint(p).raster.wait_data().unwrap();
        }
        if stroke == 2 {
            assert!(owned(&evicting, 1) > 0 && owned(&evicting, 2) > 0, "both strokes leave coverage pages");
            evict(&mut evicting);
            assert_eq!(owned(&evicting, 1), 0, "a finished stroke's coverage is scratch the raster tier can evict");
            assert!(owned(&evicting, 2) > 0, "the live stroke keeps its coverage");
        }
        revisions.push([paint(&a).raster.clone(), paint(&b).raster.clone()]);
        close(&image(&evicting), &image(&resident));
    }
    for step in [1, 2] {
        for (index, (r, p)) in [(&mut resident, &mut a), (&mut evicting, &mut b)].into_iter().enumerate() {
            let revision = revisions[step][index].clone();
            paint_mut(p).raster = revision.clone();
            r.submit(FramePacket { restore_rasters: &[(id, revision)], ..packet(p, false) }).unwrap();
            complete(r);
        }
        evict(&mut evicting);
        close(&image(&evicting), &image(&resident));
        close(&image(&renderer(&b, RESIDENT)), &image(&evicting));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_stroke_frame_refused_raster_room_waits_for_a_release() {
    let mut document = document(DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U8 });
    let id = target(&document);
    let mut r = renderer(&document, RESIDENT);
    let style = DabStyle::for_brush(&layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), layer_core::StrokeTool::Brush);
    let stroke = |stroke, x| {
        let mut dab = test_dab([x, 255.], [0.1, 0.2, 0.8, 0.7], 0.6);
        dab.radii = [14., 10.];
        (dab, DabBatch { stroke_id: layer_core::StrokeId(stroke), stroke_start: true, stroke_end: true,
            ..crate::test_support::dab_batch(id, style.clone(), dab.bounds()) })
    };
    let (dab, batch) = stroke(1, 300.);
    paint_mut(&mut document).raster = RasterRevision::pending();
    r.submit(FramePacket { dabs: &[dab], dab_batches: std::slice::from_ref(&batch), ..packet(&document, false) }).unwrap();
    complete(&r);
    paint(&document).raster.wait_data().unwrap();
    r.preparation_wake = Some(Arc::new(|| {}));
    r.retire_stroke_sources();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    r.evict_raster(u64::MAX, &[], &[], &mut encoder);
    encoder.submit(&r.queue);
    complete(&r);
    let room = r.raster_tiers.raster.limit() - r.raster_tiers.raster.used();
    let held = r.admit_raster_working(room).expect("the test holds the raster's room");
    let (dab, batch) = stroke(2, 2860.);
    paint_mut(&mut document).raster = RasterRevision::pending();
    let frame = FramePacket { dabs: &[dab], dab_batches: std::slice::from_ref(&batch), ..packet(&document, false) };
    assert!(!r.raster_dependencies_ready(frame, None), "the held room refuses the stroke frame");
    complete(&r);
    let _ = r.frame_demand();
    assert_eq!(r.frame_demand(), layer_render::FrameDemand::Wait, "a refused stroke frame waits for a release instead of spinning");
    r.commit_raster_working(held);
    assert_eq!(r.frame_demand(), layer_render::FrameDemand::Work, "the released room makes the refused frame runnable");
    assert!((0..64).any(|_| { let ready = r.raster_dependencies_ready(frame, None); complete(&r); ready }), "the released room admits the stroke frame");
}

#[test]
fn native_cache_keeps_dither_originals_after_publication_and_revisits() {
    let mut a = document(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
    let mut b = a.clone();
    let id = target(&a);
    let mut resident = renderer(&a, RESIDENT);
    let mut bounded = renderer(&b, 0);
    let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::DitherBlender);
    brush.diameter = 128.;
    brush.blender.reach = 0.4;
    let style = DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
    for (phase, x) in [257., 1281., 257.].into_iter().enumerate() {
        let mut dab = test_dab([x, 255.], [0.; 4], 1.);
        dab.radii = [64.; 2];
        dab.motion = [48., 0.];
        let batch = DabBatch { stroke_id: layer_core::StrokeId(100), stroke_start: phase == 0,
            stroke_end: phase == 2, ..crate::test_support::dab_batch(id, style.clone(), dab.bounds()) };
        for (r, p) in [(&mut resident, &mut a), (&mut bounded, &mut b)] {
            paint_mut(p).raster = RasterRevision::pending();
            r.submit(FramePacket { dabs: &[dab], dab_batches: std::slice::from_ref(&batch), ..packet(p, false) }).unwrap();
            complete(r);
            paint(p).raster.wait_data().unwrap();
            trim_cache(r, &[], &[]);
            assert!(r.paint_layers.iter().flat_map(|l| &l.pages).any(|p| p.stroke_start.is_some()),
                "published originals survive cache pressure during the same stroke");
        }
        close(&image(&bounded), &image(&resident));
    }
    let state_bytes = bounded.raster_tiers.paint.used();
    bounded.retire_stroke_sources();
    assert!(bounded.paint_layers.iter().flat_map(|l| &l.pages).all(|p| p.stroke_start.is_none()));
    assert!(bounded.paint_layers.iter().all(|l| l.coverage_pages.is_empty()), "completed Dither exposure no longer occupies stroke scratch");
    assert!(bounded.raster_tiers.paint.used() < state_bytes, "retirement releases the scratch charges");
    bounded.idle();
    trim_cache(&mut bounded, &[], &[]);
    assert!(bounded.paint_layers.iter().all(|l| l.pages.is_empty()), "completed originals no longer pin the native cache");
}

#[test]
fn native_cache_retires_blend_scratch_before_artwork_and_recreates_stroke_edges() {
    for (depth, execution) in [SampleDepth::U8, SampleDepth::U16].into_iter()
        .flat_map(|depth| [BrushExecution::Dry, BrushExecution::Wet].map(|execution| (depth, execution))) {
        let mut a = document(DocumentColor { space: RgbSpace::ProPhoto, depth });
        let mut b = a.clone();
        let id = target(&a);
        let original = paint(&a).raster.clone();
        let mut resident = renderer(&a, RESIDENT);
        let mut bounded = renderer(&b, RESIDENT);
        let page_count = bounded.paint_layers[0].pages.len();
        let mut style = test_style(execution);
        style.wet_mix.wetness = 0.8;
        style.rendering.accumulation = BrushAccumulation::Uniform;
        style.rendering.edge_after_stroke = true;
        style.rendering.wet_edge = 0.8;
        style.rendering.burnt_edge = 0.4;
        style.rendering.edge_width = 4.;
        let mut batch = DabBatch {
            stroke_end: false,
            ..crate::test_support::dab_batch(id, style, Rect::default())
        };
        // Consecutive and returning writes exercise both ping-pong roles. The
        // final edge pass must recreate destinations at the distant earlier mark.
        for (phase, x) in [257., 257., 1281., 257., 1281.].into_iter().enumerate() {
            let mut dab = test_dab([x, 255.], [0.1, 0.2, 0.8, 0.7], 0.6);
            dab.radii = [14., 10.];
            dab.motion = [13.25, -24.5];
            dab.material = [0., 0.8, 0.9, 0.7];
            batch.material_update = phase as u32;
            batch.stroke_start = phase == 0;
            batch.damage = Rect {
                min: Point { x: x - 32., y: 223. },
                max: Point { x: x + 32., y: 287. },
            };
            let frame = FramePacket {
                dabs: std::slice::from_ref(&dab),
                dab_batches: std::slice::from_ref(&batch),
                ..packet(&b, false)
            };
            for (r, p) in [(&mut resident, &a), (&mut bounded, &b)] {
                r.submit(FramePacket { scene: p.scene(), ..frame }).unwrap();
            }
            let tiles = brush_tiles::plan_packet(frame, |target| b.scene().target_extent(target)).unwrap();
            trim_color(&mut bounded, (page_count as u64 + 4) * 256 * 256 * 16, frame.dab_batches, &tiles);
            close(&image(&bounded), &image(&resident));
            assert_eq!(bounded.paint_layers[0].pages.len(), page_count,
                "artwork should survive pressure from disposable blend scratch");
            assert!(bounded.paint_layers[0].pages.iter()
                .filter(|p| p.secondary.is_some()).count() <= 8);
        }
        // An idle redraw can retire every companion while the stroke is still
        // active. Pen-up must recreate both distant sets of edge destinations.
        bounded.submit(packet(&b, false)).unwrap();
        trim_color(&mut bounded, page_count as u64 * 256 * 256 * 16, &[], &[]);
        assert_eq!(bounded.paint_layers[0].pages.len(), page_count);
        assert!(bounded.paint_layers[0].pages.iter().all(|p| p.secondary.is_none()));
        close(&image(&bounded), &image(&resident));
        batch.stroke_start = false;
        batch.stroke_end = true;
        batch.dab_count = 0;
        batch.material_update += 1;
        paint_mut(&mut a).raster = RasterRevision::pending();
        paint_mut(&mut b).raster = RasterRevision::pending();
        for (r, p) in [(&mut resident, &a), (&mut bounded, &b)] {
            r.submit(FramePacket {
                dab_batches: std::slice::from_ref(&batch),
                ..packet(p, false)
            }).unwrap();
        }
        trim_color(&mut bounded, page_count as u64 * 256 * 256 * 16, &[], &[]);
        close(&image(&bounded), &image(&resident));
        assert_eq!(backing(&paint(&a).raster), backing(&paint(&b).raster));
        bounded.submit(packet(&b, false)).unwrap();
        trim_color(&mut bounded, page_count as u64 * 256 * 256 * 16, &[], &[]);
        assert_eq!(bounded.paint_layers[0].pages.len(), page_count);
        assert!(bounded.paint_layers[0].pages.iter().all(|p| p.secondary.is_none()));
        let edited = paint(&b).raster.clone();
        paint_mut(&mut b).raster = original;
        settle(&mut bounded, packet(&b, true));
        paint_mut(&mut b).raster = edited;
        settle(&mut bounded, packet(&b, true));
        close(&image(&bounded), &image(&resident));
    }
}

#[test]
fn cold_native_color_feeds_bounded_live_filter_windows() {
    let mut p = document(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    });
    for _ in 0..2 { super::image_windows::insert_effect(&mut p, super::image_windows::program(false,false)); }
    let resident = renderer(&p, RESIDENT);
    let mut cold = ColdRenderer::new(p.composition().color);
    cold.native_edit.as_mut().unwrap().image_pixel_bytes = Some(8 * 1024 * 1024);
    let frame = packet(&p, true);
    crate::memory::memory_tests::declare_source_budget(&mut cold.renderer, frame, 64, 16 << 20, 44 << 20);
    crate::test_support::submit_settled(&mut cold.renderer, frame);
    cold.begin_pressure();
    settle(&mut cold, FramePacket { composite_all: true, ..frame });
    cold.assert_cold_capacity();
    close(&image(&cold), &image(&resident));
    assert!(cold.paint_layers[0].pages.is_empty());
    assert!(cold.metrics().image_window_submissions > 1);
    assert!(cold.metrics().image_window_peak_bytes <= 8 * 1024 * 1024);
}

#[test]
fn cold_native_operations_publish_complete_color_and_restore_exact_history() {
    use layer_core::{Affine, ImageTransform, CoverageSnapshot, RasterOperation, RasterOperationKind};
    for kind in [
        RasterOperationKind::Fill { edges: None, underfill: None, texture: Default::default(),
            color: [0.12, 0.38, 0.73, 0.42],
            alpha_locked: true,
        },
        RasterOperationKind::ApplyMask,
        RasterOperationKind::Erase { alpha_locked: false },
        RasterOperationKind::Transform(ImageTransform::affine(Affine::translation(Point { x: 83.25, y: 127.5 }))),
    ] {
        let mut a = document(DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: SampleDepth::U16,
        });
        let mut b = a.clone();
        let id = target(&a);
        let original = paint(&a).raster.clone();
        let mut resident = renderer(&a, RESIDENT);
        let mut cold = renderer(&b, 0);
        let before = image(&cold);
        let mut coverage = CoverageSnapshot::reveal_all(a.artwork.coverage.next_handle(), EXTENT, [0, 0]);
        if matches!(kind, RasterOperationKind::ApplyMask | RasterOperationKind::Erase { .. }) {
            coverage.source.default_coverage = 0.5;
        }
        let operation = RasterOperation { placement: layer_core::Affine::IDENTITY, coverage, kind };
        let batch = DabBatch {
            stroke_id: StrokeId(8),
            kind: DabBatchKind::RasterOperation(0),
            dab_count: 0,
            ..crate::test_support::dab_batch(
                id,
                test_style(BrushExecution::Dry),
                operation.bounds(EXTENT),
            )
        };
        for (r, p) in [(&mut resident, &mut a), (&mut cold, &mut b)] {
            if let RasterOperationKind::Transform(transform) = &operation.kind {
                r.set_transform_preview(Some(&layer_render::TransformPreview {
                    transaction: 8,
                    moving: false,
                    target: id,
                    selection: None,
                    transform: transform.clone(),
                }))
                .unwrap();
                crate::test_support::submit_ready(r, packet(p, false)).unwrap();
                r.set_transform_preview(None).unwrap();
            }
            Arc::make_mut(&mut paint_mut(p).operations).push(operation.clone());
            paint_mut(p).raster = RasterRevision::pending();
            settle(r, FramePacket {
                dab_batches: std::slice::from_ref(&batch),
                ..packet(p, false)
            });
            Arc::make_mut(&mut paint_mut(p).operations).clear();
        }
        assert_eq!(
            backing(&paint(&a).raster),
            backing(&paint(&b).raster),
            "{:?}",
            operation.kind
        );
        close(&image(&cold), &image(&resident));
        let after = paint(&b).raster.clone();
        let changed = image(&cold);
        assert_ne!(changed, before);
        settle(&mut cold, packet(&b, true));
        assert!(cold.paint_layers[0].pages.is_empty());
        close(&image(&cold), &changed);
        let replacement = renderer(&b, 0);
        close(&image(&replacement), &changed);
        paint_mut(&mut b).raster = original;
        settle(&mut cold, packet(&b, false));
        close(&image(&cold), &before);
        paint_mut(&mut b).raster = after;
        settle(&mut cold, packet(&b, false));
        close(&image(&cold), &changed);
    }
}

const TIER: [u32; 2] = [6000, 4000];
fn filled(pixel: [u8; 4]) -> Document {
    let mut document = Document::new(PortableId::random(), TIER[0], TIER[1], layer_core::DocumentNames { paint: "Fill".into(), paper: "Paper".into() });
    let paper = document.scene().order()[1];
    document.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    let tile = RasterTile::backed(TileBlob::encode(document.composition().color.paint_descriptor(), &pixel.repeat(256 * 256)).unwrap());
    let data = RasterData { tiles: page_coordinates(PixelRect::full(TIER))
        .map(|coordinate| (TileKey { plane: RasterPlane::Color, coordinate }, tile.clone())).collect(), ..Default::default() };
    paint_mut(&mut document).raster = RasterRevision::backed(data);
    document
}
fn retiled(document: &Document, coordinate: [u32; 2], pixel: [u8; 4]) -> Document {
    let mut edited = document.clone();
    let mut data = (*paint(document).raster.wait_data().unwrap()).clone();
    data.tiles.insert(TileKey { plane: RasterPlane::Color, coordinate },
        RasterTile::backed(TileBlob::encode(document.composition().color.paint_descriptor(), &pixel.repeat(256 * 256)).unwrap()));
    paint_mut(&mut edited).raster = RasterRevision::backed(data);
    edited
}
fn tier_frame(document: &Document, all: bool) -> FramePacket<'_> {
    FramePacket { view: crate::test_support::view(TIER), composite_all: all, ..crate::test_support::packet(document.scene(), TIER) }
}
fn polls_until_ready(r: &mut WgpuRasterizer, id: SourceTarget) -> (usize, u64) {
    pollster::block_on(crate::thumbnails::prepare_pipelines(r));
    let (mut polls, before) = (0, r.metrics.thumbnail_pages);
    loop {
        let pages = r.metrics.thumbnail_pages;
        polls += 1;
        let ready = r.prepare_thumbnail_batch(ThumbnailTarget::Source(id)).unwrap();
        assert!(r.metrics.thumbnail_pages - pages <= crate::thumbnails::BATCH_PAGES as u64, "one poll reduces at most a batch of pages");
        if ready { return (polls, r.metrics.thumbnail_pages - before); }
        assert!(polls < 1000);
        complete(r);
        pollster::block_on(r.await_image_decode()).unwrap();
    }
}

#[test]
fn edited_layer_thumbnails_reduce_only_their_changed_pages_in_bounded_batches() {
    let blue = [40, 90, 200, 255];
    let document = filled(blue);
    let id = target(&document);
    let pages = page_coordinates(PixelRect::full(TIER)).count();
    for residency in [0, 256 << 20] {
        let mut r = renderer_with(&document, residency, tier_frame(&document, true));
        let (polls, reduced) = polls_until_ready(&mut r, id);
        assert!(polls <= pages.div_ceil(crate::thumbnails::BATCH_PAGES), "a filled 24 MP layer is ready after {polls} polls");
        assert_eq!(reduced, pages as u64, "a filled layer reduces each page once");
        let filled_thumbnail = thumbnail(&mut r, id);
        assert!(filled_thumbnail[5 * 128..27 * 128].as_chunks::<4>().0.iter().all(|pixel| *pixel == blue), "the framed 3:2 layer shows the fill");
        let edited = retiled(&document, [0, 0], [250, 20, 20, 255]);
        settle(&mut r, tier_frame(&edited, false));
        assert_eq!(polls_until_ready(&mut r, id), (1, 1), "a one-page edit reduces one page");
        let edited_thumbnail = thumbnail(&mut r, id);
        let corner = &edited_thumbnail[5 * 128..5 * 128 + 4];
        assert!(corner[0] > 150 && corner[2] < 150, "the edited corner shows: {corner:?}");
        assert_eq!(&edited_thumbnail[7 * 128..], &filled_thumbnail[7 * 128..], "the rest stays");
    }
}

#[test]
fn reduced_thumbnails_frame_and_fill_content_that_leaves_most_of_its_pages_transparent() {
    let extent = [2048, 2048];
    let (content, ink) = (PixelRect::new(517, 303, 853, 805), [0u8, 0, 0, 255]);
    let mut document = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Kept".into(), paper: "Paper".into() });
    let paper = document.scene().order()[1];
    document.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    let descriptor = document.composition().color.paint_descriptor();
    let tiles = page_coordinates(PixelRect::full(extent)).map(|c| {
        let page = page_rect(c);
        let pixels: Vec<u8> = (0..PAGE_SIZE * PAGE_SIZE).flat_map(|i| {
            let [x, y] = [page.min_x() + i % PAGE_SIZE, page.min_y() + i / PAGE_SIZE];
            if (content.min_x()..content.max_x()).contains(&x) && (content.min_y()..content.max_y()).contains(&y) { ink } else { [0; 4] }
        }).collect();
        (TileKey { plane: RasterPlane::Color, coordinate: c }, RasterTile::backed(TileBlob::encode(descriptor, &pixels).unwrap()))
    }).collect();
    paint_mut(&mut document).raster = RasterRevision::backed(RasterData { tiles, ..Default::default() });
    let id = target(&document);
    let frame = FramePacket { view: crate::test_support::view(extent), composite_all: true, ..crate::test_support::packet(document.scene(), extent) };
    let mut r = renderer_with(&document, RESIDENT, frame);
    let image = thumbnail(&mut r, id);
    let size = [content.width(), content.height()].map(|v| v as f32 * 32. / content.width().max(content.height()) as f32);
    let low = size.map(|v| (32. - v) / 2.);
    let rows: Vec<String> = image.as_chunks::<{ 32 * 4 }>().0.iter().map(|row| row.as_chunks::<4>().0.iter().map(|p| if p[0] < 100 { '#' } else { 'o' }).collect()).collect();
    for (i, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
        let p = [i % 32, i / 32].map(|v| v as f32);
        let inside = (0..2).all(|a| p[a] >= low[a] && p[a] + 1. <= low[a] + size[a]);
        let outside = (0..2).any(|a| p[a] + 1. <= low[a] || p[a] >= low[a] + size[a]);
        assert!(!inside || pixel[0] < 32, "thumbnail pixel {p:?} inside the content shows the ink, got {pixel:?}:\n{}", rows.join("\n"));
        assert!(!outside || pixel[0] > 200, "thumbnail pixel {p:?} outside the content shows the checkerboard, got {pixel:?}:\n{}", rows.join("\n"));
    }
}

#[test]
#[ignore = "workstation thumbnail latency and frame cost benchmark"]
fn thumbnail_latency_and_frames_with_thumbnails_pending() {
    let document = filled([40, 90, 200, 255]);
    let id = target(&document);
    let quantile = |values: &mut Vec<f64>, q: f64| { values.sort_by(f64::total_cmp); values[((values.len() - 1) as f64 * q).round() as usize] };
    for residency in [0, 256 << 20] {
        let mut r = renderer_with(&document, residency, tier_frame(&document, true));
        complete(&r);
        let (mut cpu, mut gpu, mut polls) = (Vec::new(), Vec::new(), 0);
        loop {
            polls += 1;
            let started = std::time::Instant::now();
            let ready = r.prepare_thumbnail_batch(ThumbnailTarget::Source(id)).unwrap();
            cpu.push(started.elapsed().as_secs_f64() * 1e3);
            let submitted = std::time::Instant::now();
            complete(&r);
            gpu.push(submitted.elapsed().as_secs_f64() * 1e3);
            if ready { break; }
        }
        eprintln!("residency {residency}: {polls} polls ({:.1} s at 120 ms); poll CPU p50 {:.2} max {:.2} ms; GPU wait p50 {:.2} max {:.2} ms",
            polls as f64 * 0.12, quantile(&mut cpu, 0.5), quantile(&mut cpu, 1.), quantile(&mut gpu, 0.5), quantile(&mut gpu, 1.));
        for pending in [false, true] {
            let mut edited = retiled(&document, [7, 5], [250, 20, 20, 255]);
            r.submit(tier_frame(&edited, false)).unwrap();
            complete(&r);
            let mut frames = Vec::new();
            for frame in 0..60u32 {
                let mut view = crate::test_support::view(TIER);
                view.width_px = 2560; view.height_px = 1600;
                view.document_to_surface = [0.4, 0., 0., 0.4, -(frame as f32 * 7.), 0.];
                if frame % 3 == 0 { edited = retiled(&edited, [frame / 3 % 23, 3], [250, (frame * 4) as u8, 20, 255]); }
                let started = std::time::Instant::now();
                r.submit(FramePacket { view, composite_all: false, ..tier_frame(&edited, false) }).unwrap();
                if pending { r.prepare_thumbnail_batch(ThumbnailTarget::Source(id)).unwrap(); }
                complete(&r);
                if frame >= 5 { frames.push(started.elapsed().as_secs_f64() * 1e3); }
            }
            eprintln!("  pan and edit frames with thumbnails {}: wall p50 {:.2} p95 {:.2} max {:.2} ms", if pending { "pending" } else { "idle" },
                quantile(&mut frames, 0.5), quantile(&mut frames, 0.95), quantile(&mut frames, 1.));
        }
    }
}
