//! A zero-sized mutable cache forces native artwork through its lossless backing.
//! Compare complete consumers, not just allocation counts or the decoded cache.
use super::*;
use crate::test_support::complete;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
use layer_core::{Document, authored::{PortableId,SourceTarget,PaintHandle,PaintSource}};
use layer_render::{ColorSampleArea, ColorSampleRequest, ColorSampleSource, ThumbnailTarget};

const EXTENT: [u32; 2] = [4352, 512]; // 33 backed tiles and one transparent hole.
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
fn renderer(document: &Document, limit: u64) -> WgpuRasterizer {
    let mut r = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    r.native_edit.as_mut().unwrap().color_cache_bytes = limit;
    r.submit(packet(document, true)).unwrap();
    r
}
fn image(r: &WgpuRasterizer) -> Vec<u8> {
    crate::layer_tests::page_bytes(r, crate::test_support::document_texture(r))
}
fn close(a: &[u8], b: &[u8]) {
    assert_eq!(a.len(), b.len());
    for (i, (a, b)) in a.chunks_exact(4).zip(b.chunks_exact(4)).enumerate() {
        let a = f32::from_le_bytes(a.try_into().unwrap());
        let b = f32::from_le_bytes(b.try_into().unwrap());
        assert!((a - b).abs() <= 3e-6, "working sample {i}: {a} != {b}");
    }
}
fn thumbnail(r: &mut WgpuRasterizer, id: SourceTarget) -> Vec<u8> {
    r.request_thumbnail(7, ThumbnailTarget::Source(id)).unwrap();
    complete(r);
    r.take_thumbnail().unwrap().unwrap().bytes
}
fn sample(
    r: &mut WgpuRasterizer,
    id: SourceTarget,
    position: [u32; 2],
    area: ColorSampleArea,
) -> [f32; 4] {
    assert!(
        r.request_color_sample(ColorSampleRequest {
            request_id: 8,
            source: ColorSampleSource::Source(id),
            position,
            area
        })
        .unwrap()
    );
    complete(r);
    r.take_color_sample().unwrap().unwrap().rgba
}
fn backing(root: &RasterRevision) -> std::collections::BTreeMap<TileKey, Vec<u8>> {
    root.wait_data()
        .unwrap()
        .tiles
        .iter()
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode().unwrap()))
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
    assert!(!r.prepare_thumbnail_batch(stale).unwrap());
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
    r.submit(packet(&current, true)).unwrap();
    complete(&r);
    let before = backing(&root);
    for target in [stale, ThumbnailTarget::Source(stale_source)] {
        assert_eq!(r.prepare_thumbnail_batch(target), Err(GpuRasterError::ThumbnailUnavailable(target)));
        assert_eq!(r.request_thumbnail(99, target), Err(GpuRasterError::ThumbnailUnavailable(target)));
        assert!(r.take_thumbnail().is_none());
        assert!(!r.thumbnails_pending());
    }
    let live = ThumbnailTarget::Occurrence(live_occurrence);
    for _ in 0..100 {
        if r.prepare_thumbnail_batch(live).unwrap() { break; }
    }
    r.request_thumbnail(77, live).unwrap();
    complete(&r);
    let image = r.take_thumbnail().unwrap().unwrap();
    assert_eq!((image.request_id, image.width, image.height), (77, 32, 32));
    assert!(image.bytes.chunks_exact(4).all(|pixel| pixel == [0, 0, 255, 255]));
    assert!(!r.thumbnails_pending());
    assert_eq!(backing(&root), before);
    r.submit(packet(&current, true)).unwrap();
    complete(&r);
    assert_eq!(backing(&root), before);
}

#[test]
fn native_thumbnail_preparation_bounds_both_page_passes() {
    let p = document(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
    let id = target(&p);
    let mut cold = renderer(&p, 0);
    let mut preparations = 1;
    while !cold.prepare_thumbnail_batch(ThumbnailTarget::Source(id)).unwrap() {
        preparations += 1;
        assert!(preparations < 100);
    }
    assert!(preparations >= (33usize * 2 + 1).div_ceil(4), "bounds and drawing share the four-page budget: {preparations}");
    let mut resident = renderer(&p, u64::MAX);
    assert_eq!(thumbnail(&mut cold, id), thumbnail(&mut resident, id));
}

#[test]
fn cold_native_color_composition_sampling_and_thumbnails_match_resident_tiles() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let p = document(DocumentColor { space, depth });
            let id = target(&p);
            let mut resident = renderer(&p, u64::MAX);
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
                assert!(
                    r.request_region(layer_render::RegionRequest {
                        enclosure: None, contiguous: true,
                        selection: None,
                        request_id: 9,
                        source: layer_render::RegionSource::Source(id),
                        position: [259, 258],
                        tolerance: 0.,
                        refinement: Default::default(),
                        limit: None,
                    })
                    .unwrap()
                );
                complete(r);
                r.take_region().unwrap().unwrap().pixels
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
    r.submit(FramePacket {
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
    r.submit(packet(&p, true)).unwrap();
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
    r.submit(packet(&p, false)).unwrap();
    assert_eq!(backing(&paint(&p).raster), original_bytes);
    close(&image(&r), &before);
    paint_mut(&mut p).raster = edited;
    r.submit(packet(&p, false)).unwrap();
    close(&image(&r), &changed);
}

#[test]
fn cold_native_transform_snapshots_keep_original_tiles_through_preview_and_cancel() {
    let p = document(DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    });
    let id = target(&p);
    let mut resident = renderer(&p, u64::MAX);
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
            r.submit(packet(&p, false)).unwrap();
        }
        close(&image(&cold), &image(&resident));
    }
    for r in [&mut resident, &mut cold] {
        r.set_transform_preview(None).unwrap();
        r.submit(packet(&p, false)).unwrap();
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
    paint_mut(&mut p).original = Some(Arc::new(source.finish().unwrap()));
    let id = target(&p);
    let mut resident = renderer(&p, u64::MAX);
    let mut cold = renderer(&p, 0);
    close(&image(&cold), &image(&resident));
    let mut preparations = 1;
    while !cold.prepare_thumbnail_batch(ThumbnailTarget::Source(id)).unwrap() {
        preparations += 1;
        assert!(preparations < 100);
    }
    assert!(preparations >= (34usize + 33).div_ceil(4), "photo overrides must share the four-tile preparation budget: {preparations}");
    let a = thumbnail(&mut cold, id);
    let b = thumbnail(&mut resident, id);
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
        let mut resident = renderer(&a, u64::MAX);
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
            eprintln!("cold neighborhood {execution:?} phase={phase}");
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
        eprintln!("cold neighborhood {execution:?} after eviction");
        assert!(cold.paint_layers[0].pages.is_empty());
        close(&image(&cold), &image(&resident));
    }
}

#[test]
fn native_cache_retires_blend_scratch_before_artwork_and_recreates_stroke_edges() {
    for (depth, execution) in [SampleDepth::U8, SampleDepth::U16].into_iter()
        .flat_map(|depth| [BrushExecution::Dry, BrushExecution::Wet].map(|execution| (depth, execution))) {
        let mut a = document(DocumentColor { space: RgbSpace::ProPhoto, depth });
        let mut b = a.clone();
        let id = target(&a);
        let original = paint(&a).raster.clone();
        let mut resident = renderer(&a, u64::MAX);
        let mut bounded = renderer(&b, u64::MAX);
        let page_count = bounded.paint_layers[0].pages.len();
        // Enough for the canonical pages and this frame's four blend targets,
        // but not for scratch retained at the distant previous mark.
        bounded.native_edit.as_mut().unwrap().color_cache_bytes =
            (page_count as u64 + 4) * 256 * 256 * 16;
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
            for (r, p) in [(&mut resident, &a), (&mut bounded, &b)] {
                r.submit(FramePacket {
                    dabs: &[dab],
                    dab_batches: std::slice::from_ref(&batch),
                    ..packet(p, false)
                }).unwrap();
            }
            close(&image(&bounded), &image(&resident));
            assert_eq!(bounded.paint_layers[0].pages.len(), page_count,
                "artwork should survive pressure from disposable blend scratch");
            assert!(bounded.paint_layers[0].pages.iter()
                .filter(|p| p.secondary.is_some()).count() <= 8);
        }
        // An idle redraw can retire every companion while the stroke is still
        // active. Pen-up must recreate both distant sets of edge destinations.
        bounded.native_edit.as_mut().unwrap().color_cache_bytes =
            page_count as u64 * 256 * 256 * 16;
        bounded.submit(packet(&b, false)).unwrap();
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
        close(&image(&bounded), &image(&resident));
        assert_eq!(backing(&paint(&a).raster), backing(&paint(&b).raster));
        bounded.submit(packet(&b, false)).unwrap();
        assert_eq!(bounded.paint_layers[0].pages.len(), page_count);
        assert!(bounded.paint_layers[0].pages.iter().all(|p| p.secondary.is_none()));
        let edited = paint(&b).raster.clone();
        paint_mut(&mut b).raster = original;
        bounded.submit(packet(&b, true)).unwrap();
        paint_mut(&mut b).raster = edited;
        bounded.submit(packet(&b, true)).unwrap();
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
    let resident = renderer(&p, u64::MAX);
    let mut cold = WgpuRasterizer::new_native_headless(p.composition().color).unwrap();
    cold.native_edit.as_mut().unwrap().color_cache_bytes = 0;
    cold.native_edit.as_mut().unwrap().image_pixel_bytes = Some(8 * 1024 * 1024);
    cold.submit(packet(&p, true)).unwrap();
    close(&image(&cold), &image(&resident));
    assert!(cold.paint_layers[0].pages.is_empty());
    assert!(cold.metrics().image_window_submissions > 1);
    assert!(cold.metrics().image_window_peak_bytes <= 8 * 1024 * 1024);
}

#[test]
fn cold_native_operations_publish_complete_color_and_restore_exact_history() {
    use layer_core::{Affine, ImageTransform, CoverageSnapshot, RasterOperation, RasterOperationKind};
    for kind in [
        RasterOperationKind::Fill {
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
        let mut resident = renderer(&a, u64::MAX);
        let mut cold = renderer(&b, 0);
        let before = image(&cold);
        let mut coverage = CoverageSnapshot::reveal_all(a.artwork.coverage.next_handle(), EXTENT, Point::default());
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
                r.submit(packet(p, false)).unwrap();
                r.set_transform_preview(None).unwrap();
            }
            Arc::make_mut(&mut paint_mut(p).operations).push(operation.clone());
            paint_mut(p).raster = RasterRevision::pending();
            r.submit(FramePacket {
                dab_batches: std::slice::from_ref(&batch),
                ..packet(p, false)
            })
            .unwrap();
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
        cold.submit(packet(&b, true)).unwrap();
        assert!(cold.paint_layers[0].pages.is_empty());
        close(&image(&cold), &changed);
        let replacement = renderer(&b, 0);
        close(&image(&replacement), &changed);
        paint_mut(&mut b).raster = original;
        cold.submit(packet(&b, false)).unwrap();
        close(&image(&cold), &before);
        paint_mut(&mut b).raster = after;
        cold.submit(packet(&b, false)).unwrap();
        close(&image(&cold), &changed);
    }
}
