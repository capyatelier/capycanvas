use super::*;

const FAMILIES: [&str; 9] = ["reds", "yellows", "greens", "cyans", "blues", "magentas", "whites", "neutrals", "blacks"];

fn selective(values: [[f64; 4]; 9], relative: bool, image: bool) -> EffectInstance {
    let mut layer = effect(2, "selective_color", image);
    for (family, row) in FAMILIES.into_iter().zip(values) {
        for (component, value) in ["cyan", "magenta", "yellow", "black"].into_iter().zip(row) {
            set(&mut layer, &format!("{family}_{component}"), EffectValue::Number(value as f32));
        }
    }
    set(&mut layer, "mode", EffectValue::Choice(u32::from(!relative)));
    layer
}

fn weights(linear: [f64; 3], space: RgbSpace) -> [f64; 9] {
    let u = linear.map(|v| space.encode(v).clamp(0., 1.));
    let chroma = u.into_iter().fold(f64::NEG_INFINITY, f64::max) - u.into_iter().fold(f64::INFINITY, f64::min);
    let lab = oklab(transform(linear, space, RgbSpace::Srgb));
    let mut hue = lab[2].atan2(lab[1]).to_degrees().rem_euclid(360.);
    if hue < 30. { hue += 360.; }
    let centers = [30., 110., 145., 195., 265., 330., 390.];
    let mut result = [0.; 9];
    for i in 0..6 {
        if hue >= centers[i] && hue < centers[i + 1] {
            let t = (hue - centers[i]) / (centers[i + 1] - centers[i]);
            result[i] = chroma * (1. - t); result[(i + 1) % 6] = chroma * t;
            break;
        }
    }
    let luminance = space.to_xyz()[1].into_iter().zip(u).map(|(a, b)| a * b).sum();
    let shadow = 1. - smooth(0., 0.5, luminance); let high = smooth(0.5, 1., luminance);
    result[6] = (1. - chroma) * high; result[7] = (1. - chroma) * (1. - shadow - high); result[8] = (1. - chroma) * shadow;
    assert!((result.iter().sum::<f64>() - 1.).abs() < 1e-12);
    result
}

fn reference(linear: [f64; 3], space: RgbSpace, parameters: [[f64; 4]; 9], relative: bool) -> [f64; 3] {
    if parameters.iter().flatten().all(|v| *v == 0.) { return linear; }
    let encoded = linear.map(|v| space.encode(v)); let u = encoded.map(|v| v.clamp(0., 1.));
    let membership = weights(linear, space);
    let correction: [f64; 4] = std::array::from_fn(|c| (0..9).map(|p| membership[p] * parameters[p][c] / 100.).sum());
    let black_scale = if relative { 1. - u.into_iter().fold(f64::NEG_INFINITY, f64::max) } else { 1. };
    std::array::from_fn(|c| {
        let ink = 1. - u[c];
        let changed = (ink + correction[c] * if relative { ink } else { 1. }).clamp(0., 1.);
        let mapped = (1. - changed - correction[3] * black_scale).clamp(0., 1.);
        space.decode(encoded[c] + mapped - u[c])
    })
}

#[derive(Default)]
struct ErrorReport { count: usize, maximum: f64, maximum_scaled: f64, sum: f64 }
impl ErrorReport {
    fn check(&mut self, actual: [f32; 4], expected: [f64; 3], alpha: f32, context: &str) {
        assert_color_bound(actual, expected, alpha, 2e-6, context);
        for c in 0..3 {
            let value = if alpha == 0. { 0. } else { f64::from(actual[c]) / f64::from(alpha) };
            let error = (value - expected[c]).abs();
            self.count += 1; self.sum += error;
            self.maximum = self.maximum.max(error);
            self.maximum_scaled = self.maximum_scaled.max(error / expected[c].abs().max(1.));
        }
    }
}

#[test]
fn selective_every_slider_original_black_extended_alpha_and_profiles() {
    let mut errors = ErrorReport::default();
    for space in RgbSpace::ALL {
        let mut renderer = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        for image in [false, true] {
            for relative in [true, false] {
                for slider in 0..36 {
                    for amount in [-65., 65.] {
                        let mut parameters = [[0.; 4]; 9]; parameters[slider / 4][slider % 4] = amount;
                        let adjustment = selective(parameters, relative, image);
                        for (physical, alpha) in [([0.5; 3], 1.), ([1.; 3], 0.37), ([0.03; 3], 0.37),
                            ([0.8, 0.15, 0.02], 0.37), ([-0.2, 0.4, 2.], 8e-8), ([0.1, 0.8, 0.2], 1e-5),
                            (physical_hue(CENTERS[(slider / 4) % 6] + 10., 0.12), 0.37)] {
                            let rgb = transform(physical, RgbSpace::Srgb, space).map(|v| v as f32);
                            let expected = reference(input_reference(rgb, alpha), space, parameters, relative);
                            errors.check(frame(&mut renderer, &[adjustment.clone(), source(rgb, alpha)]), expected, alpha,
                                &format!("Selective {space:?} image={image} relative={relative} slider={slider} amount={amount} physical={physical:?}"));
                        }
                    }
                }
                let mut combined = [[0.; 4]; 9];
                for (i, p) in combined.iter_mut().enumerate() { *p = [40. - i as f64 * 3., -30., 25., 60.]; }
                for rgb in [[0.8, 0.2, 0.1], [-0.125, 0.234567, 1.5]] {
                    let alpha = 8e-8;
                    errors.check(frame(&mut renderer, &[selective(combined, relative, image), source(rgb, alpha)]),
                        reference(input_reference(rgb, alpha), space, combined, relative), alpha, "Selective combined original-K order");
                }
            }
            for alpha in [0., 8e-8, 0.37, 1.] {
                let input = source([-0.125, 0.234567, 1.5], alpha);
                let before = frame(&mut renderer, std::slice::from_ref(&input));
                assert_eq!(frame(&mut renderer, &[selective([[0.; 4]; 9], true, image), input]), before);
            }
        }
    }
    eprintln!("Selective numerical channels={} maximum_absolute={} maximum_scaled={} mean_absolute={}", errors.count, errors.maximum, errors.maximum_scaled, errors.sum / errors.count as f64);
}

#[test]
fn selective_masks_opacity_and_clipping_preserve_original_input() {
    for space in RgbSpace::ALL {
        let mut renderer = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        let parameters = [[25., -40., 15., 35.]; 9];
        for image in [false, true] {
            for clipped in [false, true] {
                for alpha in [1., 0.37, 8e-8] {
                    let rgb = [-0.125, 0.34, 1.25];
                    let original = input_reference(rgb, alpha);
                    let mapped = reference(original, space, parameters, false);
                    let expected = std::array::from_fn(|c| original[c] + (mapped[c] - original[c]) * 0.15);
                    let layer = selective(parameters, false, image);
                    let mut document=effect_document(&[layer,source(rgb,alpha)],[256;2],renderer.document_color);
                    let h=document.scene().order()[0];mask(&mut document,h,0.25);
                    let occurrence=document.artwork.occurrences.get_mut(h).unwrap();occurrence.opacity=0.6;occurrence.clipped=clipped;
                    assert_color_bound(frame_document(&mut renderer,&document), expected, alpha, 3e-6,
                        &format!("Selective mask/opacity {space:?} image={image} clipped={clipped} alpha={alpha}"));
                }
            }
        }
    }
}

fn mixer(rows: [[f64; 4]; 4], monochrome: bool, image: bool) -> EffectInstance {
    let mut layer = effect(2, "channel_mixer", image);
    for (output, row) in ["red", "green", "blue", "gray"].into_iter().zip(rows) {
        for (input, value) in ["red", "green", "blue", "constant"].into_iter().zip(row) {
            set(&mut layer, &format!("{output}_{input}"), EffectValue::Number(value as f32));
        }
    }
    set(&mut layer, "monochrome", EffectValue::Toggle(monochrome));
    layer
}

fn mixer_reference(linear: [f64; 3], space: RgbSpace, rows: [[f64; 4]; 4], monochrome: bool) -> [f64; 3] {
    let encoded = linear.map(|v| space.encode(v));
    std::array::from_fn(|output| {
        let row = rows[if monochrome { 3 } else { output }];
        let value = encoded.into_iter().zip(row).map(|(v, coefficient)| v * coefficient / 100.).sum::<f64>() + row[3] / 100.;
        space.decode(value)
    })
}

const IDENTITY: [[f64; 4]; 4] = [[100., 0., 0., 0.], [0., 100., 0., 0.], [0., 0., 100., 0.], [21.26, 71.52, 7.22, 0.]];

#[test]
fn mixer_matrices_monochrome_offsets_extended_depths_and_profiles() {
    let cases = [
        (IDENTITY, true),
        ([[0., 100., 0., 0.], [0., 0., 100., 0.], [100., 0., 0., 0.], IDENTITY[3]], false),
        ([[-125., 80., 145., 35.], [200., -200., 0., -100.], [0., 0., 0., 75.], [150., -75., 25., -20.]], false),
        ([[0.; 4], [0.; 4], [0.; 4], [150., -75., 25., -20.]], true),
    ];
    let mut errors = ErrorReport::default();
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
            let mut renderer = WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            for image in [false, true] {
                for (rows, monochrome) in cases {
                    for encoded in [[0.2, 0.7, 0.1], [-0.2, 0.4, 1.6], [0.5; 3]] {
                        let rgb = encoded.map(|v| space.decode(v) as f32);
                        for alpha in [8e-8, 0.37, 1.] {
                            let expected = mixer_reference(input_reference(rgb, alpha), space, rows, monochrome);
                            let actual = frame(&mut renderer, &[mixer(rows, monochrome, image), source(rgb, alpha)]);
                            errors.check(actual, expected, alpha, &format!("Mixer {space:?}/{depth:?} image={image} monochrome={monochrome} encoded={encoded:?}"));
                        }
                    }
                }
            }
        }
    }
    eprintln!("Mixer numerical channels={} maximum_absolute={} maximum_scaled={}", errors.count, errors.maximum, errors.maximum_scaled);
}

#[test]
fn defaults_are_exact_identity_and_disabled_mixer_gray_rows_are_ignored() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
            let mut renderer = WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            for image in [false, true] {
                for alpha in [0., 8e-8, 1. / 65535., 0.37, 1.] {
                    let input = source([-0.125, 0.234567, 1.5], alpha);
                    let before = frame(&mut renderer, std::slice::from_ref(&input));
                    let mut rows = IDENTITY; rows[3] = [200., -200., 120., -100.];
                    assert_eq!(frame(&mut renderer, &[mixer(rows, false, image), input.clone()]), before);
                    assert_eq!(frame(&mut renderer, &[selective([[0.; 4]; 9], true, image), input.clone()]), before);
                    let _ = frame(&mut renderer, &[mixer(rows, true, image), input.clone()]);
                    assert_eq!(frame(&mut renderer, &[mixer(IDENTITY, false, image), input]), before);
                }
            }
        }
    }
}

#[test]
fn selective_analytic_colors_and_cyclic_boundaries_use_original_membership() {
    let parameters = [[40., -20., 15., 35.], [-30., 50., -25., -15.], [20., 30., -40., 10.],
        [-10., -30., 60., 25.], [55., -20., 30., -35.], [-45., 20., 35., 15.],
        [15., -35., 25., 30.], [-10., 20., 40., -25.], [40., 10., -30., 60.]];
    for space in RgbSpace::ALL {
        let mut renderer = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        let mut inputs = vec![[0.; 3], [1.; 3], [0.5; 3], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.],
            [1., 1., 0.], [0., 1., 1.], [1., 0., 1.], [1., 0.5, 0.5]]
            .into_iter().map(|e| e.map(|v| space.decode(v) as f32)).collect::<Vec<_>>();
        for center in CENTERS {
            for delta in [-0.001, 0., 0.001] {
                inputs.push(transform(physical_hue(center + delta, 0.12), RgbSpace::Srgb, space).map(|v| v as f32));
            }
        }
        for image in [false, true] {
            for relative in [false, true] {
                for &rgb in &inputs {
                    let expected = reference(input_reference(rgb, 0.37), space, parameters, relative);
                    assert_color_bound(frame(&mut renderer, &[selective(parameters, relative, image), source(rgb, 0.37)]),
                        expected, 0.37, 2e-6, &format!("Selective analytic {space:?} image={image} relative={relative} rgb={rgb:?}"));
                }
            }
        }
    }
}

#[test]
fn mixer_masks_opacity_and_clipping_preserve_original_input() {
    let rows = [[80., -20., 50., 10.], [-30., 120., 20., -15.], [25., 35., 70., 5.], [21.26, 71.52, 7.22, 0.]];
    for space in RgbSpace::ALL {
        let mut renderer = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        for image in [false, true] {
            for monochrome in [false, true] {
                for clipped in [false, true] {
                    for alpha in [1., 0.37, 8e-8] {
                        let rgb = [-0.125, 0.34, 1.25];
                        let original = input_reference(rgb, alpha);
                        let changed = mixer_reference(original, space, rows, monochrome);
                        let expected = std::array::from_fn(|c| original[c] + (changed[c] - original[c]) * 0.15);
                        let layer = mixer(rows, monochrome, image);
                        let mut document=effect_document(&[layer,source(rgb,alpha)],[256;2],renderer.document_color);
                    let h=document.scene().order()[0];mask(&mut document,h,0.25);
                    let occurrence=document.artwork.occurrences.get_mut(h).unwrap();occurrence.opacity=0.6;occurrence.clipped=clipped;
                    assert_color_bound(frame_document(&mut renderer,&document), expected, alpha, 2e-6,
                            &format!("Mixer mask {space:?} image={image} monochrome={monochrome} clipped={clipped}"));
                    }
                }
            }
        }
    }
}

#[test]
fn shared_tonal_weights_preserve_color_balance_output() {
    for space in RgbSpace::ALL {
        let mut renderer = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        for image in [false, true] {
            let mut adjustment = effect(2, "color_balance", image);
            let values = [[25., -20., 40.], [-30., 35., 15.], [45., -15., -20.]];
            for (tone, values) in ["shadows", "midtones", "highlights"].into_iter().zip(values) {
                for (channel, value) in ["red", "green", "blue"].into_iter().zip(values) {
                    set(&mut adjustment, &format!("{tone}_{channel}"), EffectValue::Number(value as f32));
                }
            }
            set(&mut adjustment, "preserve_luminance", EffectValue::Toggle(false));
            for encoded in [[-0.2; 3], [0.; 3], [0.25; 3], [0.5; 3], [0.75; 3], [1.4; 3], [0.1, 0.7, 0.3]] {
                let rgb = encoded.map(|v| space.decode(v) as f32);
                let linear = input_reference(rgb, 8e-8); let encoded = linear.map(|v| space.encode(v));
                let luma: f64 = encoded.into_iter().zip(space.to_xyz()[1]).map(|(a, b)| a * b).sum();
                let shadow = 1. - smooth(0., 0.5, luma); let high = smooth(0.5, 1., luma);
                let weights = [shadow, 1. - shadow - high, high];
                let expected = std::array::from_fn(|c| space.decode(encoded[c] + (0..3).map(|tone| values[tone][c] * weights[tone] / 200.).sum::<f64>()));
                assert_color_bound(frame(&mut renderer, &[adjustment.clone(), source(rgb, 8e-8)]), expected, 8e-8, 2e-6, "factored Color Balance tones");
            }
        }
    }
}

#[test]
fn selective_preparation_tracks_corrections_and_reuses_method_opacity_and_frozen_frames() {
    for image in [false, true] {
        let mut renderer = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F32 }).unwrap();
        let rgb = [0.24, 0.41, 0.69]; let alpha = 0.37;
        let input = source(rgb, alpha);
        let original = frame(&mut renderer, std::slice::from_ref(&input));
        let mut parameters = [[0.; 4]; 9]; parameters[7] = [25., -10., 15., 20.];
        let mut adjustment = selective(parameters, true, image);
        let initial = frame(&mut renderer, &[adjustment.clone(), input.clone()]);
        assert_ne!(initial, original);
        assert_eq!(renderer.scene.as_ref().unwrap().effects.preparation_count(), 1);
        set(&mut adjustment, "neutrals_cyan", EffectValue::Number(40.)); parameters[7][0] = 40.;
        let expected = reference(input_reference(rgb, alpha), RgbSpace::Srgb, parameters, true);
        assert_color_bound(frame(&mut renderer, &[adjustment.clone(), input.clone()]), expected, alpha, 2e-6, "prepared correction edit");
        assert_eq!(renderer.scene.as_ref().unwrap().effects.preparation_count(), 2);
        set(&mut adjustment, "mode", EffectValue::Choice(1));
        let mapped = reference(input_reference(rgb, alpha), RgbSpace::Srgb, parameters, false);
        assert_color_bound(frame(&mut renderer, &[adjustment.clone(), input.clone()]), mapped, alpha, 2e-6, "method reuses prepared corrections");
        assert_eq!(renderer.scene.as_ref().unwrap().effects.preparation_count(), 2);
        let mut document=effect_document(&[adjustment.clone(),input.clone()],[256;2],renderer.document_color);
        let h=document.scene().order()[0];document.artwork.occurrences.get_mut(h).unwrap().opacity=0.4;
        let expected = std::array::from_fn(|c| f64::from(original[c]) / f64::from(alpha) * 0.6 + mapped[c] * 0.4);
        let frozen = frame_document(&mut renderer,&document);
        assert_color_bound(frozen, expected, alpha, 2e-6, "opacity reuses prepared corrections");
        assert_eq!(renderer.scene.as_ref().unwrap().effects.preparation_count(), 2);
        renderer.submit(FramePacket { composite_all: false, ..packet(document.scene().with_owner(0,0),[256;2]) }).unwrap();
        assert_eq!(renderer.scene.as_ref().unwrap().effects.preparation_count(), 2);
        assert_eq!(crate::layer_tests::page_bytes(&renderer, crate::test_support::document_texture(&renderer)),
            frozen.into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>().repeat(256 * 256));
        let zero = selective([[0.; 4]; 9], false, image);
        assert_eq!(frame(&mut renderer, &[zero, input]), original);
        assert_eq!(renderer.scene.as_ref().unwrap().effects.preparation_count(), 3);
    }
}
