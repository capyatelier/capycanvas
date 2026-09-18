//! A zero-sized mutable cache forces native artwork through its lossless backing.
//! Compare complete consumers, not just allocation counts or the decoded cache.
use super::*;
use layer_core::color::{DocumentColor, IntegerDepth, RgbSpace};
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
use layer_core::{Document, Project};
use layer_render::{ColorSampleArea, ColorSampleRequest, ColorSampleSource};
use std::io::Cursor;

const EXTENT: [u32; 2] = [4352, 512]; // 33 backed tiles and one transparent hole.
fn project(color: DocumentColor) -> Project {
    let mut document = Document::new("cold paint", EXTENT[0], EXTENT[1]);
    document.color = color;
    document.layers[1].visible = false;
    let mut data = RasterData::default();
    for y in 0..2 {
        for x in 0..17 {
            if [x, y] == [3, 0] {
                continue;
            }
            let pixel = [((x * 2351 + y * 71) % 65536) as u16, 32123, 51007, 65535];
            let pixel: Vec<_> = match color.depth {
                IntegerDepth::U16 => pixel.into_iter().flat_map(u16::to_le_bytes).collect(),
                IntegerDepth::U8 => pixel.map(|v| (v >> 8) as u8).to_vec(),
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
    document.layers[0].raster = RasterRevision::backed(data);
    Project {
        document,
        assets: Default::default(),
    }
}
fn packet(project: &Project, all: bool) -> FramePacket<'_> {
    FramePacket {
        document_extent: EXTENT,
        layers: &project.document.layers,
        view: ViewState {
            background_rgba_linear: [0.; 4],
            ..test_view()
        },
        time_seconds: 0.,
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: false,
        composite_all: all,
    }
}
fn renderer(project: &Project, limit: u64) -> WgpuRasterizer {
    let mut r = WgpuRasterizer::new_native_headless(project.document.color).unwrap();
    r.native_edit.as_mut().unwrap().color_cache_bytes = limit;
    r.submit(packet(project, true)).unwrap();
    r
}
fn complete(r: &WgpuRasterizer) {
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
}
fn image(r: &WgpuRasterizer) -> Vec<u8> {
    crate::layer_tests::page_bytes(r, r.composite_texture.as_ref().unwrap())
}
fn close(a: &[u8], b: &[u8]) {
    assert_eq!(a.len(), b.len());
    for (i, (a, b)) in a.chunks_exact(4).zip(b.chunks_exact(4)).enumerate() {
        let a = f32::from_le_bytes(a.try_into().unwrap());
        let b = f32::from_le_bytes(b.try_into().unwrap());
        assert!((a - b).abs() <= 3e-6, "working sample {i}: {a} != {b}");
    }
}
fn thumbnail(r: &mut WgpuRasterizer, id: LayerId) -> Vec<u8> {
    r.request_thumbnail(7, id).unwrap();
    complete(r);
    r.take_thumbnail().unwrap().unwrap().bytes
}
fn sample(
    r: &mut WgpuRasterizer,
    id: LayerId,
    position: [u32; 2],
    area: ColorSampleArea,
) -> [f32; 4] {
    assert!(
        r.request_color_sample(ColorSampleRequest {
            request_id: 8,
            source: ColorSampleSource::Layer(id),
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
fn cold_native_color_composition_sampling_and_thumbnails_match_resident_tiles() {
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let p = project(DocumentColor { space, depth });
            let id = p.document.layers[0].id;
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
                        request_id: 9,
                        source: layer_render::RegionSource::Layer(id),
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
            let unique: std::collections::BTreeSet<_> = p.document.layers[0].raster
                .wait_data().unwrap().tiles.values()
                .map(|tile| tile.wait_backing().unwrap().digest).collect();
            assert_eq!(cold.scene.as_ref().unwrap().source_cache_work()[1], unique.len() as u64);
        }
    }
}

#[test]
fn cold_native_paint_rehydrates_only_damage_and_keeps_other_tiles_in_saved_revisions() {
    let mut p = project(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    });
    let original = p.document.layers[0].raster.clone();
    let original_bytes = backing(&original);
    let id = p.document.layers[0].id;
    let mut r = renderer(&p, 0);
    let before = image(&r);
    let dabs = [test_dab([275., 275.], [0.13, 0.72, 0.41, 0.37], 0.5)];
    let batches = [DabBatch {
        material_update: 0,
        stroke_id: StrokeId(1),
        layer_id: id,
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style: test_style(BrushExecution::Dry),
        damage: Rect {
            min: Point { x: 257., y: 257. },
            max: Point { x: 295., y: 295. },
        },
    }];
    p.document.layers[0].raster = RasterRevision::pending();
    r.submit(FramePacket {
        dabs: &dabs,
        dab_batches: &batches,
        ..packet(&p, false)
    })
    .unwrap();
    let edited = p.document.layers[0].raster.clone();
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
    let mut archive = Vec::new();
    p.write(&mut archive).unwrap();
    let reopened = Project::read(Cursor::new(archive), Default::default()).unwrap();
    assert_eq!(backing(&reopened.document.layers[0].raster), edited_bytes);
    let replacement = renderer(&reopened, 0);
    close(&image(&replacement), &changed);
    p.document.layers[0].raster = original;
    r.submit(packet(&p, false)).unwrap();
    assert_eq!(backing(&p.document.layers[0].raster), original_bytes);
    close(&image(&r), &before);
    p.document.layers[0].raster = edited;
    r.submit(packet(&p, false)).unwrap();
    close(&image(&r), &changed);
}

#[test]
fn cold_native_transform_snapshots_keep_original_tiles_through_preview_and_cancel() {
    let p = project(DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: IntegerDepth::U16,
    });
    let id = p.document.layers[0].id;
    let mut resident = renderer(&p, u64::MAX);
    let mut cold = renderer(&p, 0);
    let original = image(&cold);
    for offset in [Point { x: 17., y: -11. }, Point { x: -31., y: 9. }] {
        let preview = layer_render::TransformPreview {
            transaction: 1,
            layer: id,
            selection: None,
            transform: layer_core::ImageTransform {
                affine: layer_core::Affine::translation(offset),
                ..Default::default()
            },
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
    let mut p = project(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    });
    let mut source = SourceBuilder::new(
        EXTENT,
        SourceInterpretation {
            channels: SourceChannels::Rgb,
            depth: IntegerDepth::U8,
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
    p.document.layers[0].source = Some(Arc::new(source.finish().unwrap()));
    let id = p.document.layers[0].id;
    let mut resident = renderer(&p, u64::MAX);
    let mut cold = renderer(&p, 0);
    close(&image(&cold), &image(&resident));
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
        let mut a = project(DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: IntegerDepth::U16,
        });
        let mut b = a.clone();
        let id = a.document.layers[0].id;
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
            material_update: 0,
            stroke_id: StrokeId(1),
            layer_id: id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: false,
            first_dab: 0,
            dab_count: 1,
            style,
            damage: Rect {
                min: Point { x: 225., y: 225. },
                max: Point { x: 288., y: 284. },
            },
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
                a.document.layers[0].raster = RasterRevision::pending();
                b.document.layers[0].raster = RasterRevision::pending();
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
            backing(&a.document.layers[0].raster),
            backing(&b.document.layers[0].raster),
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
    for depth in [IntegerDepth::U8, IntegerDepth::U16] {
        let mut a = project(DocumentColor { space: RgbSpace::ProPhoto, depth });
        let mut b = a.clone();
        let id = a.document.layers[0].id;
        let original = a.document.layers[0].raster.clone();
        let mut resident = renderer(&a, u64::MAX);
        let mut bounded = renderer(&b, u64::MAX);
        let page_count = bounded.paint_layers[0].pages.len();
        // Enough for the canonical pages and this frame's four blend targets,
        // but not for scratch retained at the distant previous mark.
        bounded.native_edit.as_mut().unwrap().color_cache_bytes =
            (page_count as u64 + 4) * 256 * 256 * 16;
        let mut style = test_style(BrushExecution::Wet);
        style.wet_mix.wetness = 0.8;
        style.rendering.accumulation = BrushAccumulation::Uniform;
        style.rendering.edge_after_stroke = true;
        style.rendering.wet_edge = 0.8;
        style.rendering.burnt_edge = 0.4;
        style.rendering.edge_width = 4.;
        let mut batch = DabBatch {
            material_update: 0,
            stroke_id: StrokeId(1),
            layer_id: id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: false,
            first_dab: 0,
            dab_count: 1,
            style,
            damage: Rect::default(),
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
        a.document.layers[0].raster = RasterRevision::pending();
        b.document.layers[0].raster = RasterRevision::pending();
        for (r, p) in [(&mut resident, &a), (&mut bounded, &b)] {
            r.submit(FramePacket {
                dab_batches: std::slice::from_ref(&batch),
                ..packet(p, false)
            }).unwrap();
        }
        close(&image(&bounded), &image(&resident));
        assert_eq!(backing(&a.document.layers[0].raster), backing(&b.document.layers[0].raster));
        bounded.submit(packet(&b, false)).unwrap();
        assert_eq!(bounded.paint_layers[0].pages.len(), page_count);
        assert!(bounded.paint_layers[0].pages.iter().all(|p| p.secondary.is_none()));
        let edited = b.document.layers[0].raster.clone();
        b.document.layers[0].raster = original;
        bounded.submit(packet(&b, true)).unwrap();
        b.document.layers[0].raster = edited;
        bounded.submit(packet(&b, true)).unwrap();
        close(&image(&bounded), &image(&resident));
    }
}

#[test]
fn cold_native_color_feeds_bounded_live_filter_windows() {
    let mut p = project(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    });
    p.document
        .layers
        .insert(0, super::image_windows::effect(3, false, false));
    p.document
        .layers
        .insert(0, super::image_windows::effect(4, false, false));
    let resident = renderer(&p, u64::MAX);
    let mut cold = WgpuRasterizer::new_native_headless(p.document.color).unwrap();
    cold.native_edit.as_mut().unwrap().color_cache_bytes = 0;
    cold.native_edit.as_mut().unwrap().image_pixel_bytes = 8 * 1024 * 1024;
    cold.submit(packet(&p, true)).unwrap();
    close(&image(&cold), &image(&resident));
    assert!(cold.paint_layers[0].pages.is_empty());
    assert!(cold.metrics().image_window_submissions > 1);
    assert!(cold.metrics().image_window_peak_bytes <= 8 * 1024 * 1024);
}

#[test]
fn cold_native_operations_publish_complete_color_and_restore_exact_history() {
    use layer_core::{Affine, ImageTransform, LayerMask, LayerOperation, LayerOperationKind};
    for kind in [
        LayerOperationKind::Fill {
            color: [0.12, 0.38, 0.73, 0.42],
            alpha_locked: true,
        },
        LayerOperationKind::ApplyMask,
        LayerOperationKind::Transform(ImageTransform {
            affine: Affine::translation(Point { x: 83.25, y: 127.5 }),
            ..Default::default()
        }),
    ] {
        let mut a = project(DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: IntegerDepth::U16,
        });
        let mut b = a.clone();
        let id = a.document.layers[0].id;
        let original = a.document.layers[0].raster.clone();
        let mut resident = renderer(&a, u64::MAX);
        let mut cold = renderer(&b, 0);
        let before = image(&cold);
        let mut coverage = LayerMask::reveal_all(LayerId(99), Point::default());
        if kind == LayerOperationKind::ApplyMask {
            coverage.default_coverage = 0.5;
        }
        let operation = LayerOperation { placement: layer_core::Affine::IDENTITY, coverage, kind };
        let batch = DabBatch {
            material_update: 0,
            stroke_id: StrokeId(8),
            layer_id: id,
            kind: DabBatchKind::LayerOperation(0),
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 0,
            style: test_style(BrushExecution::Dry),
            damage: operation.bounds(EXTENT),
        };
        for (r, p) in [(&mut resident, &mut a), (&mut cold, &mut b)] {
            if let LayerOperationKind::Transform(transform) = operation.kind {
                r.set_transform_preview(Some(&layer_render::TransformPreview {
                    transaction: 8,
                    layer: id,
                    selection: None,
                    transform,
                }))
                .unwrap();
                r.submit(packet(p, false)).unwrap();
                r.set_transform_preview(None).unwrap();
            }
            p.document.layers[0]
                .pending_operations
                .push(operation.clone());
            p.document.layers[0].raster = RasterRevision::pending();
            r.submit(FramePacket {
                dab_batches: std::slice::from_ref(&batch),
                ..packet(p, false)
            })
            .unwrap();
            p.document.layers[0].pending_operations.clear();
        }
        assert_eq!(
            backing(&a.document.layers[0].raster),
            backing(&b.document.layers[0].raster),
            "{:?}",
            operation.kind
        );
        close(&image(&cold), &image(&resident));
        let after = b.document.layers[0].raster.clone();
        let changed = image(&cold);
        assert_ne!(changed, before);
        cold.submit(packet(&b, true)).unwrap();
        assert!(cold.paint_layers[0].pages.is_empty());
        close(&image(&cold), &changed);
        let replacement = renderer(&b, 0);
        close(&image(&replacement), &changed);
        b.document.layers[0].raster = original;
        cold.submit(packet(&b, false)).unwrap();
        close(&image(&cold), &before);
        b.document.layers[0].raster = after;
        cold.submit(packet(&b, false)).unwrap();
        close(&image(&cold), &changed);
    }
}
