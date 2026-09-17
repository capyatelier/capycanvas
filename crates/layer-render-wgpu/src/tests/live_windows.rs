//! Live window seams, partial damage and resource ceilings against full-image
//! composition. Edited images use an arithmetic tolerance, not byte parity.
use super::image_windows::effect;
use super::*;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
use layer_core::{LayerMask, Selection};

fn packet(layers: &[Layer], extent: [u32; 2]) -> FramePacket<'_> {
    FramePacket {
        document_extent: extent,
        layers,
        view: ViewState {
            background_rgba_linear: [0.; 4],
            ..test_view()
        },
        time_seconds: 0.,
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: false,
        composite_all: true,
    }
}
fn pixels(r: &WgpuRasterizer) -> Vec<u8> {
    crate::layer_tests::page_bytes(r, r.composite_texture.as_ref().unwrap())
}
fn close(actual: &[u8], reference: &[u8]) {
    assert_eq!(actual.len(), reference.len());
    let mut maximum = 0f32;
    for (i, (a, b)) in actual
        .chunks_exact(4)
        .zip(reference.chunks_exact(4))
        .enumerate()
    {
        let a = f32::from_le_bytes(a.try_into().unwrap());
        let b = f32::from_le_bytes(b.try_into().unwrap());
        let error = (a - b).abs();
        assert!(error <= 3e-6, "sample {i}: {a} != {b}");
        maximum = maximum.max(error);
    }
    eprintln!("window composite maximum absolute channel error: {maximum}");
}

#[test]
fn native_live_windows_match_full_filters_masks_clips_and_reconfiguration() {
    let extent = [777, 533];
    const CAP: u64 = 16 * 1024 * 1024;
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let mut r =
                WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            for clipped in [false, true] {
                let mut first = effect(2, false, false);
                first.opacity = 0.63;
                first.properties.clipped = clipped;
                let mut second = effect(3, false, false);
                second.properties.clipped = clipped;
                let mut mask = LayerMask::reveal_all(LayerId(20), Point { x: 7., y: -9. });
                mask.default_coverage = 0.;
                mask.initial = Some(
                    Selection::polygon(vec![
                        Point { x: 0., y: 0. },
                        Point { x: 760., y: 99. },
                        Point { x: 440., y: 533. },
                    ])
                    .unwrap(),
                );
                first.mask = Some(mask);
                let mut layers = vec![
                    first,
                    second,
                    effect(1, true, false),
                    effect(4, true, false),
                ];
                for group in [false, true] {
                    if group {
                        let mut g = Layer::paint(LayerId(10), "isolated group");
                        g.kind = LayerKind::Group;
                        g.opacity = 0.79;
                        for l in &mut layers[..3] {
                            l.properties.parent = Some(g.id);
                        }
                        layers.insert(0, g);
                        layers[1].mask.as_mut().unwrap().show_area = true;
                    }
                    r.native_edit.as_mut().unwrap().image_pixel_bytes = u64::MAX;
                    r.submit(packet(&layers, extent)).unwrap();
                    let expected = pixels(&r);
                    let full_cache = r.scene.as_ref().unwrap().image_cache_bytes();
                    let before = r.metrics().image_window_submissions;
                    r.native_edit.as_mut().unwrap().image_pixel_bytes = CAP;
                    r.submit(packet(&layers, extent)).unwrap();
                    close(&pixels(&r), &expected);
                    assert!(r.metrics().image_window_submissions > before + 1);
                    assert!(r.metrics().image_window_peak_bytes <= CAP + 96 * layers.len() as u64);
                    assert_eq!(r.scene.as_ref().unwrap().image_cache_bytes(), 0,
                        "completed filter windows must release temporary pixels");
                    eprintln!(
                        "{space:?} {depth:?} clipped={clipped} group={group}: full cache={full_cache}, retained window={}, peak window={}, cap={CAP}",
                        r.scene.as_ref().unwrap().image_cache_bytes(),
                        r.metrics().image_window_peak_bytes
                    );
                    // A second partial window frame cannot reuse a previous
                    // window as if it held the entire document.
                    let mut scene = r.scene.take().unwrap();
                    let composed = r.metrics().composited_pixels;
                    let mut encoder =
                        submission::CommandEncoder::new(&r.device, &Default::default());
                    scene
                        .compose(
                            &mut r,
                            FramePacket {
                                composite_all: false,
                                ..packet(&layers, extent)
                            },
                            PixelRect::new(253, 251, 279, 283),
                            &mut encoder,
                            true,
                            None,
                        )
                        .unwrap();
                    r.uploads.finish(&encoder);
                    encoder.submit(&r.queue);
                    r.scene = Some(scene);
                    assert!(r.metrics().composited_pixels - composed < u64::from(extent[0]) * u64::from(extent[1]),
                        "releasing temporary pixels must preserve damage metadata");
                    close(&pixels(&r), &expected);
                    // Returning to the ordinary cache must repopulate all of it.
                    r.native_edit.as_mut().unwrap().image_pixel_bytes = u64::MAX;
                    r.submit(packet(&layers, extent)).unwrap();
                    close(&pixels(&r), &expected);
                }
            }
        }
    }
}

#[test]
fn native_live_global_limit_rejects_before_document_or_submission_changes() {
    let extent = [333, 291];
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    })
    .unwrap();
    let mut layers = vec![effect(2, false, true), effect(1, true, false)];
    r.submit(packet(&layers, extent)).unwrap();
    let before = pixels(&r);
    let texture = r.composite_texture.clone();
    let metrics = r.metrics();
    r.native_edit.as_mut().unwrap().image_pixel_bytes = 1024;
    // Includes a resize and reset: rejection must precede both.
    let error = r
        .submit(FramePacket {
            reset_layers: true,
            ..packet(&layers, [6000, 4000])
        })
        .unwrap_err();
    assert!(error.to_string().contains("Document-wide"));
    assert_eq!(r.document_extent, extent);
    assert_eq!(r.composite_texture, texture);
    assert_eq!(r.metrics(), metrics);
    assert_eq!(
        pixels(&r),
        before,
        "rejection must preserve the exact current composite"
    );
    // Removing the unsupported adjustment leaves the renderer usable.
    layers.remove(0);
    r.submit(packet(&layers, extent)).unwrap();
    assert!(pixels(&r).iter().any(|b| *b != 0));
}

#[test]
fn native_live_window_halos_follow_paint_undo_redo_and_recreated_renderer() {
    use layer_core::raster::RasterRevision;
    let extent = [777, 533];
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    const CAP: u64 = 8 * 1024 * 1024;
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    r.native_edit.as_mut().unwrap().image_pixel_bytes = CAP;
    let mut layers = vec![
        effect(3, false, false),
        effect(2, false, false),
        Layer::paint(LayerId(1), "retouch"),
    ];
    let original = layers[2].raster.clone();
    r.submit(packet(&layers, extent)).unwrap();
    let before = pixels(&r);
    layers[2].raster = RasterRevision::pending();
    let dabs = [test_dab([255., 256.], [0.13, 0.72, 0.41, 0.37], 0.5)];
    let batches = [DabBatch {
        material_update: 0,
        stroke_id: StrokeId(1),
        layer_id: LayerId(1),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style: test_style(BrushExecution::Dry),
        damage: Rect {
            min: Point { x: 235., y: 236. },
            max: Point { x: 275., y: 276. },
        },
    }];
    r.submit(FramePacket {
        dabs: &dabs,
        dab_batches: &batches,
        composite_all: false,
        ..packet(&layers, extent)
    })
    .unwrap();
    let edited = layers[2].raster.clone();
    let backing = edited.wait_data().unwrap();
    assert!(!backing.tiles.is_empty());
    let incremental = pixels(&r);
    assert_ne!(incremental, before);
    r.native_edit.as_mut().unwrap().image_pixel_bytes = u64::MAX;
    r.submit(packet(&layers, extent)).unwrap();
    close(&incremental, &pixels(&r));
    r.native_edit.as_mut().unwrap().image_pixel_bytes = CAP;
    layers[2].raster = original;
    r.submit(FramePacket {
        composite_all: false,
        ..packet(&layers, extent)
    })
    .unwrap();
    assert_eq!(pixels(&r), before, "undo restores the exact empty artwork");
    layers[2].raster = edited;
    r.submit(FramePacket {
        composite_all: false,
        ..packet(&layers, extent)
    })
    .unwrap();
    close(&pixels(&r), &incremental);
    assert!(Arc::ptr_eq(
        &layers[2].raster.wait_data().unwrap(),
        &backing
    ));
    let mut replacement = WgpuRasterizer::new_native_headless(color).unwrap();
    replacement.native_edit.as_mut().unwrap().image_pixel_bytes = CAP;
    replacement.submit(packet(&layers, extent)).unwrap();
    close(&pixels(&replacement), &incremental);
    assert!(Arc::ptr_eq(
        &layers[2].raster.wait_data().unwrap(),
        &backing
    ));
    // Metadata changes still invalidate the complete adjustment, even when a
    // caller supplies only a small paint rectangle in the same frame.
    layers[0].opacity = 0.13;
    let mut scene = r.scene.take().unwrap();
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    scene
        .compose(
            &mut r,
            FramePacket {
                composite_all: false,
                ..packet(&layers, extent)
            },
            PixelRect::new(259, 261, 263, 265),
            &mut encoder,
            true,
            None,
        )
        .unwrap();
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    r.scene = Some(scene);
    let changed = pixels(&r);
    r.native_edit.as_mut().unwrap().image_pixel_bytes = u64::MAX;
    r.submit(packet(&layers, extent)).unwrap();
    close(&changed, &pixels(&r));
}

#[test]
fn native_live_animated_windows_refresh_with_empty_paint_damage_and_keep_frozen_time() {
    let extent = [777, 533];
    let mut generator = effect(1, true, false);
    let mut program = (*generator.effect.as_ref().unwrap().program)
        .clone()
        .with_time_controls();
    program.wgsl = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{let a=.37;return vec4<f32>(vec3<f32>(fract(p.x/37.+fx_time(b)/7.),fract(p.y/29.),.27)*a,a);}".into();
    generator.effect = Some(Arc::new(layer_core::EffectInstance::new(Arc::new(program))));
    let mut layers = vec![effect(2, false, false), generator];
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    })
    .unwrap();
    const CAP: u64 = 8 * 1024 * 1024;
    r.native_edit.as_mut().unwrap().image_pixel_bytes = CAP;
    r.submit(packet(&layers, extent)).unwrap();
    let before = pixels(&r);
    r.submit(FramePacket {
        time_seconds: 3.,
        composite_all: false,
        ..packet(&layers, extent)
    })
    .unwrap();
    let animated = pixels(&r);
    assert_ne!(animated, before);
    r.native_edit.as_mut().unwrap().image_pixel_bytes = u64::MAX;
    r.submit(FramePacket {
        time_seconds: 3.,
        ..packet(&layers, extent)
    })
    .unwrap();
    close(&pixels(&r), &animated);
    let generator = Arc::make_mut(layers[1].effect.as_mut().unwrap());
    generator
        .set("animate", layer_core::EffectValue::Toggle(false))
        .unwrap();
    generator
        .set("time", layer_core::EffectValue::Number(3.))
        .unwrap();
    r.native_edit.as_mut().unwrap().image_pixel_bytes = CAP;
    r.submit(FramePacket {
        time_seconds: 7.,
        ..packet(&layers, extent)
    })
    .unwrap();
    close(&pixels(&r), &animated);
    let submissions = r.metrics().image_window_submissions;
    r.submit(FramePacket {
        time_seconds: 8.,
        composite_all: false,
        ..packet(&layers, extent)
    })
    .unwrap();
    assert_eq!(r.metrics().image_window_submissions, submissions);
    close(&pixels(&r), &animated);
}
