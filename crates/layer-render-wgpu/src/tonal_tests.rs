use super::*;
use layer_core::{
    Affine, SelectionMode,
    color::{
        ColorProfile, DocumentColor, RgbSpace, SampleDepth,
        source::{SourceBuilder, SourceChannels, SourceInterpretation},
    },
    tonal::TonalBand,
};
use layer_render::{
    RegionRequest, RegionResult, RegionSource, SelectionRefinement, TonalProbe, TonalRequest,
};
use std::sync::Arc;

fn receive(
    r: &mut WgpuRasterizer,
    source: RegionSource,
    bands: Vec<TonalBand>,
    invert: bool,
    probe: Option<TonalProbe>,
    previous: Option<Selection>,
) -> RegionResult {
    assert!(
        r.request_region(RegionRequest {
            request_id: 73,
            contiguous: false,
            position: [0, 0],
            tolerance: 0.,
            refinement: Default::default(),
            limit: None,
            source: RegionSource::Tonal(Box::new(TonalRequest {
                source,
                bands,
                invert,
                probe
            })),
            selection: Some(SelectionRefinement {
                resize: 0,
                mode: if previous.is_some() {
                    SelectionMode::Intersect
                } else {
                    SelectionMode::New
                },
                antialias: true,
                feather: 0.,
                previous: previous.map(Arc::new),
                source_to_document: Affine::IDENTITY
            }),
        })
        .unwrap()
    );
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        if let Some(result) = r.take_region() {
            return result.unwrap();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "tonal readback timed out"
        );
        std::thread::yield_now();
    }
}
fn byte(p: &layer_core::SelectionPixels, x: u32, y: u32) -> u8 {
    (p.words()[(y * p.extent()[0].div_ceil(4) + x / 4) as usize] >> ((x % 4) * 8)) as u8
}
#[test]
fn tonal_hdr_masks_and_probes_match_luminance_reference() {
    let extent = [259, 17]; // crosses a source tile and ends in a partial packed word
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::F32,
    };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let pixel = |x: u32, y: u32| {
        let v = if x == 0 {
            0.
        } else {
            2f32.powf(x as f32 / 16. - 10.)
        };
        let alpha = match y {
            0 => 0.,
            1 => 0.5,
            _ => 1.,
        };
        if y >= 12 {
            [v * 0.25, v, v * 2., alpha]
        } else {
            [v, v, v, alpha]
        }
    };
    let mut source = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::F32,
            profile: ColorProfile::Builtin(color.space),
            profile_assumed: false,
        },
        1_000_000,
    )
    .unwrap();
    for y in 0..extent[1] {
        source
            .push_row(
                &(0..extent[0])
                    .flat_map(|x| pixel(x, y))
                    .flat_map(f32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    let mut layer = Layer::paint(LayerId(1), "HDR ramp");
    layer.source = Some(Arc::new(source.finish().unwrap()));
    r.submit(FramePacket {
        view: view(),
        document_extent: extent,
        layers: &[layer],
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: true,
        time_seconds: 0.,
        composite_all: true,
    })
    .unwrap();
    let bands = vec![
        TonalBand::defaults()[0].clone(),
        TonalBand {
            name: "Custom HDR".into(),
            lower: Some(1.),
            upper: Some(3.),
            falloff: [1., 2.],
        },
    ];
    for source in [RegionSource::Layer(LayerId(1)), RegionSource::Composite] {
        for invert in [false, true] {
            let p = receive(&mut r, source.clone(), bands.clone(), invert, None, None).pixels;
            let weights = color.space.to_xyz()[1];
            for y in 0..extent[1] {
                for x in 0..extent[0] {
                    let rgb = pixel(x, y);
                    let luminance = (0..3).map(|i| weights[i] * f64::from(rgb[i])).sum();
                    let c = bands
                        .iter()
                        .map(|b| b.coverage(luminance))
                        .fold(0., f64::max);
                    let expected = ((if invert { 1. - c } else { c }) * f64::from(rgb[3]) * 255.)
                        .round() as u8;
                    assert!(
                        byte(&p, x, y).abs_diff(expected) <= 1,
                        "{source:?} invert={invert} {x},{y}: {} expected {expected}",
                        byte(&p, x, y)
                    );
                }
            }
        }
    }
    let sample = receive(
        &mut r,
        RegionSource::Layer(LayerId(1)),
        bands.clone(),
        false,
        Some(TonalProbe {
            bounds: [160, 3, 165, 8],
            point: true,
            quad: None,
        }),
        None,
    )
    .tonal_sample
    .unwrap();
    let expected = ((160..165).map(|x| f64::from(pixel(x, 3)[0])).sum::<f64>() / 5.).log2();
    assert!((f64::from(sample.stops[0]) - expected).abs() < 1e-5);
    assert_eq!(sample.count, 25);
    let area = receive(
        &mut r,
        RegionSource::Layer(LayerId(1)),
        bands.clone(),
        false,
        Some(TonalProbe {
            bounds: [16, 3, 240, 8],
            point: false,
            quad: None,
        }),
        None,
    )
    .tonal_sample
    .unwrap();
    assert!((area.stops[0] - (-8.3125)).abs() < 0.07, "{area:?}");
    assert!((area.stops[1] - 4.25).abs() < 0.07, "{area:?}");
    assert!(
        receive(
            &mut r,
            RegionSource::Layer(LayerId(1)),
            bands.clone(),
            false,
            Some(TonalProbe {
                bounds: [10, 0, 15, 1],
                point: true,
                quad: None,
            }),
            None
        )
        .tonal_sample
        .is_none()
    );
    // A document rectangle becomes a quad in a rotated raw layer. Pixels
    // inside its bounding box but outside the actual footprint must not sample.
    let quad = [
        Point { x: 128., y: 3. },
        Point { x: 140., y: 7. },
        Point { x: 128., y: 11. },
        Point { x: 116., y: 7. },
    ];
    let mut values = Vec::new();
    for y in 3..11 {
        for x in 116..140 {
            let p = Point {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
            };
            if (0..4).all(|i| {
                let a = quad[i];
                let b = quad[(i + 1) % 4];
                (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x) >= 0.
            }) {
                values.push((x as f32 / 16. - 10.) as f64);
            }
        }
    }
    values.sort_by(f64::total_cmp);
    for q in [quad, [quad[3], quad[2], quad[1], quad[0]]] {
        let sample = receive(
            &mut r,
            RegionSource::Layer(LayerId(1)),
            bands.clone(),
            false,
            Some(TonalProbe {
                bounds: [116, 3, 140, 11],
                point: false,
                quad: Some(q),
            }),
            None,
        )
        .tonal_sample
        .unwrap();
        assert_eq!(sample.count as usize, values.len());
        for (i, fraction) in [0.05, 0.95].into_iter().enumerate() {
            let rank = (fraction * (values.len() - 1) as f64).round() as usize;
            assert!(
                (f64::from(sample.stops[i]) - values[rank]).abs() < 0.1,
                "{sample:?}"
            );
        }
    }
    let previous = Selection::polygon(vec![
        Point { x: 128., y: 0. },
        Point { x: 259., y: 0. },
        Point { x: 259., y: 17. },
        Point { x: 128., y: 17. },
    ])
    .unwrap();
    let p = receive(
        &mut r,
        RegionSource::Layer(LayerId(1)),
        bands,
        false,
        None,
        Some(previous),
    )
    .pixels;
    assert_eq!(byte(&p, 10, 8), 0);
    assert_eq!(byte(&p, 192, 8), 255);
}

#[test]
fn tonal_sdr_native_painted_source_and_composite() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut white = Layer::paint(LayerId(2), "Paper");
    white.kind = layer_core::LayerKind::Background;
    let gray = 0.007f32;
    let mut ink = dab([gray, gray, gray, 1.]);
    ink.center = Point { x: 300., y: 300. };
    let mut stroke = batch(1);
    stroke.damage = layer_core::Rect {
        min: Point::default(),
        max: Point { x: 512., y: 512. },
    };
    r.submit(FramePacket {
        view: view(),
        document_extent: [512, 512],
        layers: &[Layer::paint(LayerId(1), "Paint"), white],
        dabs: &[ink],
        dab_batches: &[stroke],
        restore_rasters: &[],
        reset_layers: true,
        time_seconds: 0.,
        composite_all: true,
    })
    .unwrap();
    let _ = receive(
        &mut r,
        RegionSource::Composite,
        vec![TonalBand::defaults()[4].clone()],
        false,
        None,
        None,
    );
    let _ = receive(&mut r, RegionSource::Composite, vec![], false, None, None);
    let bands = vec![TonalBand::defaults()[0].clone()];
    for source in [RegionSource::Layer(LayerId(1)), RegionSource::Composite] {
        let result = receive(
            &mut r,
            source.clone(),
            bands.clone(),
            false,
            Some(TonalProbe {
                bounds: [298, 298, 303, 303],
                point: true,
                quad: None,
            }),
            None,
        );
        assert!(
            byte(&result.pixels, 300, 300) > 240,
            "{source:?}: sample {:?} coverage {}",
            result.tonal_sample,
            byte(&result.pixels, 300, 300)
        );
    }
}

#[test]
#[ignore = "24MP hardware tonal preview benchmark; release, serial"]
fn tonal_preview_latency() {
    let extent = [6000, 4000];
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::F32,
    })
    .unwrap();
    let mut paper = Layer::paint(LayerId(1), "Uniform photo benchmark");
    paper.kind = layer_core::LayerKind::Background;
    r.submit(FramePacket {
        view: ViewState {
            background_rgba_linear: [1.; 4],
            ..view()
        },
        document_extent: extent,
        layers: &[paper],
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: true,
        time_seconds: 0.,
        composite_all: true,
    })
    .unwrap();
    for probe in [false, true] {
        let mut ms = Vec::new();
        for _ in 0..6 {
            let start = std::time::Instant::now();
            let result = receive(
                &mut r,
                RegionSource::Composite,
                TonalBand::defaults(),
                false,
                probe.then_some(TonalProbe {
                    bounds: [0, 0, 6000, 4000],
                    point: false,
                    quad: None,
                }),
                None,
            );
            ms.push(start.elapsed().as_secs_f64() * 1000.);
            assert_eq!(byte(&result.pixels, 3000, 2000), 255);
            if probe {
                assert_eq!(result.tonal_sample.unwrap().count, 24_000_000);
            }
        }
        let first = ms.remove(0);
        ms.sort_by(f64::total_cmp);
        eprintln!(
            "tonal 24MP seven bands probe={probe}: first={first:.2}ms warm_median={:.2}ms warm_max={:.2}ms",
            ms[2], ms[4]
        );
    }
}
