//! Continuous transform drags over 24-megapixel paint and photo layers: a new
//! transform every frame, with CPU submission and GPU completion kept apart.
use super::*;
use layer_core::color::{ColorProfile, RgbSpace, SampleDepth, source::*};
use layer_core::{Affine, ImageTransform, Interpolation, TransformMap};
use std::time::Instant;

const EXTENT: [u32; 2] = [6000, 4000];
const FRAMES: usize = 200;
const WARMUP: usize = 40;

fn center() -> Point {
    Point {
        x: EXTENT[0] as f32 * 0.5,
        y: EXTENT[1] as f32 * 0.5,
    }
}

fn cases() -> Vec<(&'static str, Box<dyn Fn(f32) -> ImageTransform>)> {
    let affine = |t: f32| {
        Affine::around(
            center(),
            [0.9 + t.sin() * 0.05; 2],
            0.1 + t.cos() * 0.02,
            Point {
                x: t.sin() * 40.,
                y: t.cos() * 30.,
            },
        )
    };
    vec![(
        "affine bilinear",
        Box::new(move |t| ImageTransform {
            map: TransformMap::Affine(affine(t)),
            interpolation: Interpolation::Linear,
        }),
    )]
}

fn submit(r: &mut WgpuRasterizer, layer: &Layer, reset: bool) {
    let zoom = (1920. / EXTENT[0] as f32).min(1080. / EXTENT[1] as f32);
    r.submit(FramePacket {
        view: ViewState {
            width_px: 1920,
            height_px: 1080,
            document_to_surface: [zoom, 0., 0., zoom, 0., 0.],
            ..view()
        },
        document_extent: EXTENT,
        layers: std::slice::from_ref(layer),
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: reset,
        time_seconds: 0.,
        composite_all: reset,
    })
    .unwrap();
}

fn percentiles(mut values: Vec<f64>) -> [f64; 3] {
    values.sort_by(f64::total_cmp);
    [0.5, 0.95, 0.99].map(|p| values[(values.len() as f64 * p).ceil() as usize - 1])
}

/// Drag each case's transform across FRAMES frames of one transaction.
fn drag(r: &mut WgpuRasterizer, layer: &Layer, label: &str) -> f64 {
    let selection = Selection::polygon(vec![
        Point { x: 0., y: 0. },
        Point {
            x: EXTENT[0] as f32,
            y: 0.,
        },
        Point {
            x: EXTENT[0] as f32,
            y: EXTENT[1] as f32,
        },
        Point {
            x: 0.,
            y: EXTENT[1] as f32,
        },
    ])
    .unwrap();
    let mut worst = 0f64;
    for (transaction, (name, transform)) in cases().into_iter().enumerate() {
        let mut cpu = Vec::new();
        let mut completed = Vec::new();
        for i in 0..FRAMES {
            let start = Instant::now();
            r.set_transform_preview(Some(&layer_render::TransformPreview {
                transaction: transaction as u64 + 1,
                layer: layer.id,
                selection: Some(selection.clone()),
                transform: transform(i as f32 * 0.04),
            }))
            .unwrap();
            submit(r, layer, false);
            let submitted = start.elapsed().as_secs_f64() * 1000.;
            r.wait_idle().unwrap();
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            if i == 0 {
                eprintln!("{label} {name}: first frame {elapsed:.3}ms");
            } else if i >= WARMUP {
                cpu.push(submitted);
                completed.push(elapsed);
            }
        }
        let (cpu, completed) = (percentiles(cpu), percentiles(completed));
        eprintln!(
            "{label} {name} 6000x4000, full selection: CPU submit p50/p95/p99 {cpu:.3?}ms, GPU completion {completed:.3?}ms"
        );
        worst = worst.max(completed[2]);
        r.set_transform_preview(None).unwrap();
        submit(r, layer, false);
        r.wait_idle().unwrap();
    }
    worst
}

#[test]
#[ignore = "hardware 24-megapixel paint transform benchmark; release, serial"]
fn large_paint_transform_latency() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let asset = AssetId::from("test:large transform latency");
    let pixels: Vec<_> = (0..EXTENT[0] * EXTENT[1])
        .flat_map(|i| {
            let (x, y) = (i % EXTENT[0], i / EXTENT[0]);
            [(x % 200) as u8, (y % 220) as u8, ((x ^ y) % 256) as u8, 230]
        })
        .collect();
    r.prepare_asset(
        &asset,
        HostImage {
            width: EXTENT[0],
            height: EXTENT[1],
            stride: EXTENT[0] * 4,
            format: PixelFormat::Rgba8Srgb,
            bytes: &pixels,
        },
    )
    .unwrap();
    let mut layer = Layer::paint(LayerId(1), "large paint transform");
    layer.asset = Some(asset);
    submit(&mut r, &layer, true);
    r.wait_idle().unwrap();
    let worst = drag(&mut r, &layer, "paint");
    assert!(worst < 8.333, "a transform drag exceeds the 120 Hz budget");
}

#[test]
#[ignore = "hardware 24-megapixel photo transform benchmark; release, serial"]
fn large_photo_transform_latency() {
    let mut builder = SourceBuilder::new(
        EXTENT,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        512 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..EXTENT[1] {
        let row: Vec<_> = (0..EXTENT[0])
            .flat_map(|x| {
                [
                    ((x * 8191 + y * 31) % 65536) as u16,
                    ((x * 17 + y * 16381) % 65536) as u16,
                    ((x / 173 + y / 111) % 2 * 65535) as u16,
                    65535,
                ]
            })
            .flat_map(u16::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
    }
    let mut layer = Layer::paint(LayerId(1), "large photo transform");
    layer.source = Some(std::sync::Arc::new(builder.finish().unwrap()));
    let mut r = WgpuRasterizer::new_headless().unwrap();
    submit(&mut r, &layer, true);
    r.wait_idle().unwrap();
    let worst = drag(&mut r, &layer, "photo");
    assert!(
        worst < 8.333,
        "a photo transform drag exceeds the 120 Hz budget"
    );
}
