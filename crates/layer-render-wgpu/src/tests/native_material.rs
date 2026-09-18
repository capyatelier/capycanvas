//! Connected native brush/composite precision checks with declared scalar oracles.
use super::*;
use layer_core::color::{DocumentColor, IntegerDepth, RgbSpace, rgb};

fn frame(r: &mut WgpuRasterizer, layer: &Layer, dab: Dab, batch: &DabBatch, reset: bool) {
    r.submit(FramePacket {
        view: ViewState {
            width_px: 256,
            height_px: 256,
            background_rgba_linear: [0.; 4],
            ..test_view()
        },
        document_extent: [256; 2],
        layers: std::slice::from_ref(layer),
        dabs: &[dab],
        dab_batches: std::slice::from_ref(batch),
        restore_rasters: &[],
        reset_layers: reset,
        composite_all: true,
        time_seconds: 0.,
    })
    .unwrap();
}
fn batch(style: DabStyle) -> DabBatch {
    DabBatch {
        material_update: 0,
        stroke_id: StrokeId(1),
        layer_id: LayerId(1),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style,
        damage: Rect {
            min: Point { x: 0., y: 0. },
            max: Point { x: 256., y: 256. },
        },
    }
}
fn prime(r: &mut WgpuRasterizer, layer: &Layer, color: [f32; 3], alpha: f32) {
    let dab = test_dab([128., 128.], [0.; 4], 0.);
    frame(r, layer, dab, &batch(test_style(BrushExecution::Dry)), true);
    let texture = &r.paint_layers[0].pages[0].active().texture;
    let pixel = [color[0] * alpha, color[1] * alpha, color[2] * alpha, alpha];
    r.queue.write_texture(
        texture.as_image_copy(),
        &pixel
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
            .repeat(65536),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4096),
            rows_per_image: Some(256),
        },
        texture.size(),
    );
}
fn pixel(r: &WgpuRasterizer) -> [f32; 4] {
    let bytes = crate::layer_tests::page_bytes(r, &r.paint_layers[0].pages[0].active().texture);
    let bytes = &bytes[(128 * 256 + 128) * 16..];
    std::array::from_fn(|c| f32::from_le_bytes(bytes[c * 4..c * 4 + 4].try_into().unwrap()))
}
fn close(actual: [f32; 4], premultiplied: [f64; 3], alpha: f64, tolerance: f64, context: &str) {
    assert!(
        (f64::from(actual[3]) - alpha).abs() <= 3e-7 * alpha.abs(),
        "alpha {context}: {actual:?} vs {alpha}"
    );
    for c in 0..3 {
        let (value, expected) = if alpha > 0. {
            (
                f64::from(actual[c]) / f64::from(actual[3]),
                premultiplied[c] / alpha,
            )
        } else {
            (f64::from(actual[c]), 0.)
        };
        assert!(
            (value - expected).abs() <= tolerance,
            "{context}: {c}: {value} vs {expected}, rgba={actual:?}"
        );
    }
}
fn blended(mode: BrushBlendMode, d: f64, s: f64) -> f64 {
    match mode {
        BrushBlendMode::Normal => s,
        BrushBlendMode::Multiply => s * d,
        BrushBlendMode::Screen => s + d - s * d,
        BrushBlendMode::Add => (s + d).min(1.),
        BrushBlendMode::Subtract => (d - s).max(0.),
        BrushBlendMode::Darken => s.min(d),
        BrushBlendMode::Lighten => s.max(d),
        BrushBlendMode::Overlay => {
            if d >= 0.5 {
                1. - 2. * (1. - d) * (1. - s)
            } else {
                2. * s * d
            }
        }
    }
}

#[test]
fn native_brush_blends_preserve_extended_color_and_tiny_locked_coverage() {
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let mut r =
                WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            let layer = Layer::paint(LayerId(1), "reference brush");
            for mode in [
                BrushBlendMode::Normal,
                BrushBlendMode::Multiply,
                BrushBlendMode::Screen,
                BrushBlendMode::Add,
                BrushBlendMode::Subtract,
                BrushBlendMode::Darken,
                BrushBlendMode::Lighten,
                BrushBlendMode::Overlay,
            ] {
                for alpha in [0., 0.00000008, 1. / 65535.] {
                    for locked in [false, true] {
                        let d = [-0.125, 0.234567, 1.5];
                        let s = [0.6, 0.125, 0.8, 0.7];
                        prime(&mut r, &layer, d, alpha);
                        let mut style = test_style(BrushExecution::Dry);
                        style.alpha_locked = locked;
                        style.rendering.blend_mode = mode;
                        style.rendering.accumulation = BrushAccumulation::Uniform;
                        let dab = test_dab([128., 128.], s, 0.3);
                        let mut stroke = batch(style);
                        stroke.damage = Rect {
                            min: Point { x: 100., y: 100. },
                            max: Point { x: 156., y: 156. },
                        };
                        frame(&mut r, &layer, dab, &stroke, false);
                        let pixels = crate::layer_tests::page_bytes(
                            &r, &r.paint_layers[0].pages[0].active().texture,
                        );
                        let original: Vec<_> = [d[0] * alpha, d[1] * alpha, d[2] * alpha, alpha]
                            .into_iter().flat_map(f32::to_le_bytes).collect();
                        for (index, pixel) in pixels.chunks_exact(16).enumerate() {
                            let [x, y] = [index % 256, index / 256];
                            if !(100..156).contains(&x) || !(100..156).contains(&y) {
                                assert_eq!(pixel, original, "untouched pixel {x},{y}");
                            }
                        }
                        let da = f64::from(alpha);
                        let sa = f64::from(s[3]) * f64::from(dab.flow);
                        let out_alpha = if locked { da } else { sa + da * (1. - sa) };
                        let rgb = std::array::from_fn(|c| {
                            let d = f64::from(d[c]);
                            let s = f64::from(s[c]);
                            let b = blended(mode, d, s);
                            if locked {
                                (1. - sa) * d * da + sa * b * da
                            } else {
                                (1. - sa) * d * da + (1. - da) * s * sa + sa * da * b
                            }
                        });
                        close(
                            std::array::from_fn(|c| {
                                let offset = (128 * 256 + 128) * 16 + c * 4;
                                f32::from_le_bytes(pixels[offset..offset + 4].try_into().unwrap())
                            }),
                            rgb,
                            out_alpha,
                            2e-6,
                            &format!("{space:?}/{depth:?} {mode:?} locked={locked} a={alpha}"),
                        );
                    }
                }
            }
        }
    }
}

// Float64 evaluation of Ottosson's published linear-sRGB/Oklab equations, with
// the independently checked document-primary matrices/adaptation on each side.
fn lab(c: [f64; 3]) -> [f64; 3] {
    let lms = rgb::apply(
        [
            [0.4122214708, 0.5363325363, 0.0514459929],
            [0.2119034982, 0.6806995451, 0.1073969566],
            [0.0883024619, 0.2817188376, 0.6299787005],
        ],
        c,
    )
    .map(f64::cbrt);
    rgb::apply(
        [
            [0.2104542553, 0.7936177850, -0.0040720468],
            [1.9779984951, -2.4285922050, 0.4505937099],
            [0.0259040371, 0.7827717662, -0.8086757660],
        ],
        lms,
    )
}
fn unlab(c: [f64; 3]) -> [f64; 3] {
    let lms = rgb::apply(
        [
            [1., 0.3963377774, 0.2158037573],
            [1., -0.1055613458, -0.0638541728],
            [1., -0.0894841775, -1.2914855480],
        ],
        c,
    )
    .map(|v| v * v * v);
    rgb::apply(
        [
            [4.0767416621, -3.3077115913, 0.2309699292],
            [-1.2684380046, 2.6097574011, -0.3413193965],
            [-0.0041960863, -0.7034186147, 1.7076147010],
        ],
        lms,
    )
}
fn mixed(space: RgbSpace, a: [f32; 3], b: [f32; 3], weight: f64) -> [f64; 3] {
    if weight == 0. {
        return a.map(f64::from);
    }
    if weight == 1. {
        return b.map(f64::from);
    }
    let a = lab(rgb::apply(
        space.linear_transform(RgbSpace::Srgb),
        a.map(f64::from),
    ));
    let b = lab(rgb::apply(
        space.linear_transform(RgbSpace::Srgb),
        b.map(f64::from),
    ));
    rgb::apply(
        RgbSpace::Srgb.linear_transform(space),
        unlab(std::array::from_fn(|c| {
            a[c] * (1. - weight) + b[c] * weight
        })),
    )
}
#[test]
fn native_wet_brush_oklab_mixing_uses_document_primaries_and_preserves_extended_endpoints() {
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let mut r =
                WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            let layer = Layer::paint(LayerId(1), "wet reference");
            for amount in [0., 0.37, 1.] {
                let d = [-0.125, 0.25, 1.25];
                let s = [0.8, 0.1, 0.3, 0.7];
                let alpha = 0.6;
                prime(&mut r, &layer, d, alpha);
                let mut style = test_style(BrushExecution::Wet);
                style.wet_mix.amount_of_paint = amount;
                style.wet_mix.mix_space = ColorMixSpace::Oklab;
                let dab = test_dab([128., 128.], s, 0.35);
                frame(&mut r, &layer, dab, &batch(style), false);
                let paint = mixed(space, d, [s[0], s[1], s[2]], f64::from(amount));
                let sa = f64::from(dab.flow) * f64::from(s[3]) * f64::from(alpha.max(amount));
                let da = f64::from(alpha);
                let expected =
                    std::array::from_fn(|c| f64::from(d[c]) * da * (1. - sa) + paint[c] * sa);
                close(
                    pixel(&r),
                    expected,
                    sa + da * (1. - sa),
                    5e-6,
                    &format!("Oklab {space:?}/{depth:?} amount={amount}"),
                );
            }
        }
    }
}

#[test]
fn native_wet_and_watercolor_deposit_sub_epsilon_pigment_and_transport_extended_values() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
            space,
            depth: IntegerDepth::U16,
        })
        .unwrap();
        let layer = Layer::paint(LayerId(1), "tiny pigment");
        let color = [-0.125, 0.25, 1.25, 1.];
        for execution in [BrushExecution::Wet, BrushExecution::Watercolor] {
            prime(&mut r, &layer, [0.; 3], 0.);
            let mut style = test_style(execution);
            style.rendering.accumulation = BrushAccumulation::Uniform;
            let dab = test_dab([128., 128.], color, 0.00000008);
            frame(&mut r, &layer, dab, &batch(style), false);
            let alpha = f64::from(dab.flow);
            assert!(pixel(&r)[3] > 0., "{execution:?} discarded tiny pigment");
            close(
                pixel(&r),
                [color[0], color[1], color[2]].map(|v| f64::from(v) * alpha),
                alpha,
                2e-6,
                "tiny pigment",
            );
        }
        // A constant interior is a transport fixed point. The live wetness and
        // real transport pass must not clip RGB to [0,alpha] between native commits.
        prime(&mut r, &layer, [color[0], color[1], color[2]], 0.5);
        let mut style = test_style(BrushExecution::Watercolor);
        style.wet_mix.wetness = 0.8;
        style.rendering.accumulation = BrushAccumulation::Uniform;
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
        let mut dab = test_dab([128., 128.], color, 0.3);
        dab.radii = [120.; 2];
        frame(&mut r, &layer, dab, &batch(style), false);
        let actual = pixel(&r);
        assert!(actual[3] > 0.5);
        close(
            actual,
            [color[0], color[1], color[2]].map(|v| f64::from(v) * f64::from(actual[3])),
            f64::from(actual[3]),
            2e-6,
            "transport constant color",
        );
        assert!(actual[0] < 0. && actual[2] > actual[3]);
    }
}

#[test]
fn native_capillary_front_transports_faint_pigment_with_independent_water_coverage() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
            space,
            depth: IntegerDepth::U16,
        })
        .unwrap();
        let layer = Layer::paint(LayerId(1), "faint transport");
        let mut baseline = None;
        for alpha in [0.5, 1. / 65535.] {
            prime(&mut r, &layer, [0.; 3], 0.);
            let mut style = test_style(BrushExecution::Watercolor);
            style.rendering.accumulation = BrushAccumulation::Uniform;
            let mut dab = test_dab([128., 128.], [0.; 4], 0.);
            dab.radii = [120.; 2];
            frame(&mut r, &layer, dab, &batch(style.clone()), false);
            // Water coverage and pigment coverage have separate ownership. Keep
            // the wet field identical while varying only the pigment amount.
            for (texture, channels) in [
                (&r.paint_layers[0].pages[0].active().texture, 4),
                (
                    &r.paint_layers[0].watercolor_wetness_pages[0]
                        .active()
                        .texture,
                    1,
                ),
            ] {
                let data: Vec<_> = (0..65536)
                    .flat_map(|i| {
                        let left = i % 256 < 128;
                        let pixel = if channels == 4 {
                            [-0.125 * alpha, 0.25 * alpha, 1.25 * alpha, alpha]
                        } else {
                            [0.8, 0., 0., 0.]
                        };
                        pixel
                            .into_iter()
                            .take(channels)
                            .flat_map(move |v| f32::to_le_bytes(if left { v } else { 0. }))
                    })
                    .collect();
                r.queue.write_texture(
                    texture.as_image_copy(),
                    &data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256 * channels as u32 * 4),
                        rows_per_image: Some(256),
                    },
                    texture.size(),
                );
            }
            style.transport = Some(BrushTransport {
                distance: 4.,
                wet_flow: 0.4,
                dry_flow: 0.6,
                conductance: AssetId::from(WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET),
                scale: 1.,
                rotation_radians: 0.,
                contrast: 0.,
                water_load: 1.,
            });
            frame(&mut r, &layer, dab, &batch(style), false);
            let actual = pixel(&r);
            assert!(actual[3] > 0., "{space:?} front discarded alpha={alpha}");
            close(
                actual,
                [-0.125, 0.25, 1.25].map(|v| v * f64::from(actual[3])),
                f64::from(actual[3]),
                2e-6,
                "front color",
            );
            if let Some(high) = baseline {
                let expected: f64 = f64::from(high) * f64::from(alpha) / 0.5;
                assert!(
                    (f64::from(actual[3]) / expected - 1.).abs() < 3e-6,
                    "faint coverage must scale independently of water"
                );
            } else {
                baseline = Some(actual[3]);
            }
        }
    }
}
