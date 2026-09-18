use super::*;
use layer_core::color::{IntegerDepth, source::*};

#[test]
fn source_neighborhood_brushes_match_materialized_pixels_across_cache_and_prediction() {
    let extent = [2305, 1793]; // Eighty original tiles exceed the 64-tile cache.
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            profile: Default::default(),
            profile_assumed: false,
        },
        32 * 1024 * 1024,
    )
    .unwrap();
    let mut bytes = Vec::new();
    for y in 0..extent[1] {
        let mut row = Vec::new();
        for x in 0..extent[0] {
            let rgb = match (x / 256 + y / 256 * 3) % 4 {
                0 => [255u8, 0, 0],
                1 => [0, 255, 0],
                2 => [0, 0, 255],
                _ => [255; 3],
            };
            for code in [rgb[0], rgb[1], rgb[2], 255] {
                bytes.push(code);
                row.extend_from_slice(&(u16::from(code) * 257).to_le_bytes());
            }
        }
        builder.push_row(&row).unwrap();
    }
    let source = Arc::new(builder.finish().unwrap());
    let original_digests: Vec<_> = source.tiles.values().map(|t| t.digest).collect();
    let asset = AssetId::from("test:materialized-source-neighborhood");
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut reference = WgpuRasterizer::from_wgpu_inner(
        r.adapter.clone(),
        r.device.clone(),
        r.queue.clone(),
        Initialization::Warm,
    )
    .unwrap();
    reference
        .prepare_asset(
            &asset,
            HostImage {
                width: extent[0],
                height: extent[1],
                stride: extent[0] * 4,
                format: PixelFormat::Rgba8Srgb,
                bytes: &bytes,
            },
        )
        .unwrap();
    let view = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        background_rgba_linear: [0.; 4],
        ..test_view()
    };
    let damage = Rect {
        min: Point { x: 0., y: 505. },
        max: Point { x: 2304., y: 512. },
    };
    for execution in [
        BrushExecution::Dry,
        BrushExecution::Smudge,
        BrushExecution::Wet,
        BrushExecution::Liquify,
        BrushExecution::Watercolor,
    ] {
        let mut layer = Layer::paint(LayerId(1), "tiled original");
        layer.source = Some(source.clone());
        let mut baked = Layer::paint(layer.id, "materialized original");
        baked.asset = Some(asset.clone());
        let mut style = test_style(execution);
        style.wet_mix.amount_of_paint = 0.3;
        style.wet_mix.dilution = 0.4;
        style.wet_mix.pull = 0.9;
        style.wet_mix.blur = 1.;
        style.wet_mix.wetness = 0.7;
        if execution == BrushExecution::Watercolor {
            style.transport = Some(BrushTransport {
                distance: 4.,
                wet_flow: 0.4,
                dry_flow: 0.1,
                conductance: AssetId::from(WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET),
                scale: 1.,
                rotation_radians: 0.,
                contrast: 0.,
                water_load: 1.,
            });
        }
        let mut first = test_dab([1152., 508.5], [0.1, 0.2, 0.8, 0.7], 0.6);
        first.radii = [1120., 2.];
        first.motion = [13.25, -24.5];
        first.material = [0., 0.8, 0.9, 0.7];
        let mut second = first;
        second.center.y += 0.75;
        let dabs = [first, second];
        let mut base = DabBatch {
            material_update: 0,
            stroke_id: StrokeId(1),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: false,
            first_dab: 0,
            dab_count: 2,
            style,
            damage,
        };
        let mut before = Vec::new();
        for phase in 0..5 {
            let mut contacts = dabs;
            let mut batches = Vec::new();
            if phase > 0 {
                base.kind = if phase <= 2 {
                    DabBatchKind::Preview
                } else {
                    DabBatchKind::Persistent
                };
                batches.push(base.clone());
                if phase == 2 || phase == 3 {
                    batches[0].dab_count = 1;
                    let mut next = batches[0].clone();
                    next.first_dab = 1;
                    next.stroke_start = false;
                    next.material_update = 1;
                    batches.push(next);
                }
                if phase == 4 {
                    batches[0].stroke_start = false;
                    batches[0].stroke_end = true;
                    batches[0].dab_count = 0;
                    batches[0].material_update = 2;
                }
                if phase == 2 {
                    // Shrinking the tail retains off-tail preview allocations.
                    // They must never replace the original in neighbor reads.
                    for (i, contact) in contacts.iter_mut().enumerate() {
                        contact.center.x = 254. + i as f32;
                        contact.radii[0] = 3.;
                    }
                    for batch in &mut batches {
                        batch.damage.min.x = 250.;
                        batch.damage.max.x = 260.;
                    }
                }
            }
            for (renderer, layer) in [(&mut r, &layer), (&mut reference, &baked)] {
                renderer
                    .submit(FramePacket {
                        view,
                        document_extent: extent,
                        layers: std::slice::from_ref(layer),
                        dabs: if phase == 0 { &[] } else { &contacts },
                        dab_batches: &batches,
                        restore_rasters: &[],
                        reset_layers: phase == 0,
                        time_seconds: 0.,
                        composite_all: phase == 0,
                    })
                    .unwrap();
            }
            let actual = crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap());
            let expected = crate::layer_tests::page_bytes(
                &reference,
                reference.composite_texture.as_ref().unwrap(),
            );
            let (at, error) = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .map(|(i, (a, b))| (i, a.abs_diff(*b)))
                .max_by_key(|v| v.1)
                .unwrap();
            assert!(
                error <= 1,
                "{execution:?} phase={phase} at=({}, {}) channel={} actual={} expected={} error={error}",
                at / 4 % extent[0] as usize,
                at / 4 / extent[0] as usize,
                at % 4,
                actual[at],
                expected[at]
            );
            if phase == 0 {
                before = actual;
            } else if phase == 3 {
                assert!(actual != before, "each brush edits the reference");
            }
            assert!(
                r.paint_layers[0].pages.len() < source.tiles.len(),
                "read-only neighbors must stay source-backed"
            );
            assert!(r.metrics.source_upload_peak_bytes <= 16 * 1024 * 1024);
        }
        assert!(r.scene.as_ref().unwrap().source_cache_work()[1] > source.tiles.len() as u64,
            "brush and prediction reads must exercise cache eviction");
    }
    assert_eq!(
        original_digests,
        source.tiles.values().map(|t| t.digest).collect::<Vec<_>>()
    );
}
