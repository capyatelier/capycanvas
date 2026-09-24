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
                point: true
            }),
            None
        )
        .tonal_sample
        .is_none()
    );
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
