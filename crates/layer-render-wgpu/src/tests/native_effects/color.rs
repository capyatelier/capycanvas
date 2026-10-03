use super::*;
use layer_core::color::RgbColor;

#[path = "mixing.rs"]
mod mixing;

const RANGES: [&str; 6] = ["reds", "yellows", "greens", "cyans", "blues", "magentas"];
const CENTERS: [f64; 6] = [30., 110., 145., 195., 265., 330.];

#[derive(Clone)]
struct Hue {
    master: [f64; 3],
    ranges: [[f64; 6]; 6],
    colorize: Option<[f64; 2]>,
}
impl Default for Hue {
    fn default() -> Self {
        Self { master: [0.; 3], ranges: CENTERS.map(|center| [0., 0., 0., center, 30., 30.]), colorize: None }
    }
}
fn configure(layer: &mut Layer, hue: &Hue) {
    for (key, value) in ["hue", "saturation", "lightness"].into_iter().zip(hue.master) {
        set(layer, key, EffectValue::Number(value as f32));
    }
    for (prefix, range) in RANGES.into_iter().zip(hue.ranges) {
        for (suffix, value) in ["hue", "saturation", "lightness", "center", "width", "feather"].into_iter().zip(range) {
            set(layer, &format!("{prefix}_{suffix}"), EffectValue::Number(value as f32));
        }
    }
    set(layer, "colorize", EffectValue::Toggle(hue.colorize.is_some()));
    if let Some([h, s]) = hue.colorize {
        set(layer, "colorize_hue", EffectValue::Number(h as f32));
        set(layer, "colorize_saturation", EffectValue::Number(s as f32));
    }
}
fn transform(rgb: [f64; 3], from: RgbSpace, to: RgbSpace) -> [f64; 3] {
    from.linear_transform(to).map(|row| row.into_iter().zip(rgb).map(|(a, b)| a * b).sum())
}
fn oklab(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb;
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s]
}
fn physical_hue(degrees: f64, chroma: f64) -> [f64; 3] {
    let a = chroma * degrees.to_radians().cos();
    let b = chroma * degrees.to_radians().sin();
    let l = (0.65 + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (0.65 - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (0.65 - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s]
}
fn smooth(low: f64, high: f64, x: f64) -> f64 {
    let u = ((x - low) / (high - low)).clamp(0., 1.);
    u * u * (3. - 2. * u)
}
fn membership(hue: f64, chroma: f64, center: f64, width: f64, feather: f64) -> f64 {
    let distance = ((hue - center + 180.).rem_euclid(360.) - 180.).abs();
    let angular = if feather == 0. { if distance <= width / 2. { 1. } else { 0. } }
        else { 1. - smooth(width / 2., width / 2. + feather, distance) };
    angular * smooth(0.005, 0.02, chroma)
}
fn hsl(rgb: [f64; 3]) -> [f64; 3] {
    let high = rgb.into_iter().fold(f64::NEG_INFINITY, f64::max);
    let low = rgb.into_iter().fold(f64::INFINITY, f64::min);
    let lightness = (high + low) / 2.;
    if high == low { return [0., 0., lightness]; }
    let delta = high - low;
    let h = if high == rgb[0] { (rgb[1] - rgb[2]) / delta }
        else if high == rgb[1] { (rgb[2] - rgb[0]) / delta + 2. }
        else { (rgb[0] - rgb[1]) / delta + 4. };
    [h.rem_euclid(6.) / 6., delta / (1. - (2. * lightness - 1.).abs()), lightness]
}
fn from_hsl([h, s, l]: [f64; 3]) -> [f64; 3] {
    let chroma = (1. - (2. * l - 1.).abs()) * s;
    let sector = h.rem_euclid(1.) * 6.;
    let x = chroma * (1. - (sector.rem_euclid(2.) - 1.).abs());
    let color = match sector.floor() as u8 {
        0 => [chroma, x, 0.], 1 => [x, chroma, 0.], 2 => [0., chroma, x],
        3 => [0., x, chroma], 4 => [x, 0., chroma], _ => [chroma, 0., x],
    };
    color.map(|v| v + l - chroma / 2.)
}
fn hue_reference(linear: [f64; 3], space: RgbSpace, hue: &Hue) -> [f64; 3] {
    let encoded = linear.map(|v| space.encode(v));
    let low = encoded.into_iter().fold(0., f64::min);
    let high = encoded.into_iter().fold(1., f64::max);
    let span = high - low;
    let mut coordinates = hsl(encoded.map(|v| (v - low) / span));
    let mut correction = hue.master;
    if let Some([h, s]) = hue.colorize {
        coordinates[0] = h / 360.; coordinates[1] = s / 100.;
        correction[0] = 0.; correction[1] = 0.;
    } else {
        let lab = oklab(transform(linear, space, RgbSpace::Srgb));
        let h = lab[2].atan2(lab[1]).to_degrees().rem_euclid(360.);
        let chroma = lab[1].hypot(lab[2]);
        for range in hue.ranges {
            let weight = membership(h, chroma, range[3], range[4], range[5]);
            for i in 0..3 { correction[i] += range[i] * weight; }
        }
        coordinates[0] = (coordinates[0] + correction[0] / 360.).rem_euclid(1.);
        coordinates[1] = (coordinates[1] * (1. + correction[1].clamp(-100., 100.) / 100.)).clamp(0., 1.);
    }
    let lightness = correction[2].clamp(-100., 100.) / 100.;
    coordinates[2] = if lightness < 0. { coordinates[2] * (1. + lightness) }
        else { coordinates[2] + (1. - coordinates[2]) * lightness };
    from_hsl(coordinates).map(|v| space.decode(v * span + low))
}
fn assert_color(actual: [f32; 4], expected: [f64; 3], alpha: f32, context: &str) {
    assert_color_bound(actual, expected, alpha, 6e-6, context);
}
fn assert_color_bound(actual: [f32; 4], expected: [f64; 3], alpha: f32, tolerance: f64, context: &str) {
    assert_eq!(actual[3], alpha, "{context}: alpha");
    for i in 0..3 {
        let value = if alpha == 0. { f64::from(actual[i]) } else { f64::from(actual[i]) / f64::from(alpha) };
        let target = if alpha == 0. { 0. } else { expected[i] };
        assert!((value - target).abs() <= tolerance * target.abs().max(1.), "{context}: {actual:?} expected {expected:?}");
    }
}
fn input_reference(rgb: [f32; 3], alpha: f32) -> [f64; 3] {
    if alpha == 0. { [0.; 3] } else { rgb.map(|v| f64::from(v * alpha) / f64::from(alpha)) }
}
fn threshold(image: bool, value: f32) -> Layer {
    let mut layer = effect(2, "threshold", image);
    let instance = Arc::make_mut(layer.effect.as_mut().unwrap());
    instance.program = instance.program.for_depth(SampleDepth::F32);
    set(&mut layer, "threshold", EffectValue::Number(value)); layer
}

#[test]
fn p21_hue_ranges_match_original_linear_oklab_membership_in_every_profile_and_path() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        for image in [false, true] {
            for (index, center) in CENTERS.into_iter().enumerate() {
                let mut hue = Hue::default(); hue.ranges[index][0] = 40.;
                let mut adjustment = effect(2, "hue_saturation", image); configure(&mut adjustment, &hue);
                for delta in [-45.01, -30., -15.01, -14.99, 0., 14.99, 15.01, 30., 45.01] {
                    let rgb = transform(physical_hue(center + delta, 0.08), RgbSpace::Srgb, space).map(|v| v as f32);
                    let expected = hue_reference(input_reference(rgb, 0.37), space, &hue);
                    assert_color(frame(&mut r, &[adjustment.clone(), source(rgb, 0.37)]), expected, 0.37,
                        &format!("{space:?} image={image} range={index} offset={delta}"));
                }
            }
            for physical in [[0.7, 0.16, 0.025], [-0.05, 0.4, 1.1], [-0.4, 0.02, 0.01]] {
                let mut hue = Hue::default();
                for (i, range) in hue.ranges.iter_mut().enumerate() { range[0] = 12. + i as f64 * 7.; range[1] = -25.; }
                let rgb = transform(physical, RgbSpace::Srgb, space).map(|v| v as f32);
                let mut adjustment = effect(2, "hue_saturation", image); configure(&mut adjustment, &hue);
                assert_color(frame(&mut r, &[adjustment, source(rgb, 8e-8)]), hue_reference(input_reference(rgb, 8e-8), space, &hue), 8e-8,
                    &format!("signed membership {space:?} {image}"));
            }
        }
    }
}

#[test]
fn p21_hue_literal_weights_lock_wrap_closed_edges_support_and_chroma_gate() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for (h, c, center, width, feather, expected) in [
        (15., 0.08, 0., 30., 0., 1.), (15.01, 0.08, 0., 30., 0., 0.),
        (0., 0.08, 0., 0., 0., 1.), (0.01, 0.08, 0., 0., 0., 0.),
        (359., 0.08, 0., 0., 2., 0.5), (1., 0.08, 0., 0., 2., 0.5),
        (358., 0.08, 0., 0., 2., 0.), (2., 0.08, 0., 0., 2., 0.),
        (90., 0.08, 0., 180., 90., 1.), (135., 0.08, 0., 180., 90., 0.5), (180., 0.08, 0., 180., 90., 0.),
        (15., 0.08, 0., 0., 30., 0.5), (30., 0.08, 0., 0., 30., 0.),
        (0., 0.005, 0., 30., 30., 0.), (0., 0.0125, 0., 30., 30., 0.5), (0., 0.02, 0., 30., 30., 1.),
    ] {
        assert!((membership(h, c, center, width, feather) - expected).abs() < 1e-12);
        let mut probe = effect(2, "hue_saturation", false);
        let program = Arc::make_mut(&mut Arc::make_mut(probe.effect.as_mut().unwrap()).program);
        program.entry = "weight_probe".into();
        let shader = program.wgsl.sources().unwrap().iter().map(|s| s.as_ref()).collect::<Vec<_>>().join("\n");
        program.wgsl = format!("{shader}\nfn weight_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{let w=fx_hue_weight(vec2<f32>({h:?},{c:?}),{center:?},{width:?},{feather:?});return vec4<f32>(w,w,w,1.);}}").into();
        let actual = frame(&mut r, &[probe, source([0.; 3], 1.)]);
        for v in &actual[..3] { assert!((f64::from(*v) - expected).abs() <= 2e-6, "weight {h}/{c}/{center}/{width}/{feather}: {actual:?}"); }
    }
}

#[test]
fn p21_hue_coordinates_and_membership_are_physical_across_profiles_at_tiny_alpha() {
    let physical = [physical_hue(29., 0.08), physical_hue(359., 0.0125), [-0.05, 0.4, 1.1], [-0.4, 0.02, 0.01]];
    let mut observed = Vec::new();
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        let mut probe = effect(2, "hue_saturation", false);
        let program = Arc::make_mut(&mut Arc::make_mut(probe.effect.as_mut().unwrap()).program);
        let shader = program.wgsl.sources().unwrap().iter().map(|s| s.as_ref()).collect::<Vec<_>>().join("\n");
        program.entry = "coordinates_probe".into();
        program.wgsl = format!("{shader}\nfn coordinates_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{let h=fx_hue_coordinates(c);let w=fx_hue_weight(h,0.,0.,90.);return vec4<f32>(vec3<f32>(h.x/360.,h.y,w)*c.a,c.a);}}").into();
        let mut weights = Vec::new();
        for rgb in physical {
            let rgb = transform(rgb, RgbSpace::Srgb, space).map(|v| v as f32);
            let actual = frame(&mut r, &[probe.clone(), source(rgb, 8e-8)]);
            let lab = oklab(transform(input_reference(rgb, 8e-8), space, RgbSpace::Srgb));
            let h = lab[2].atan2(lab[1]).to_degrees().rem_euclid(360.); let c = lab[1].hypot(lab[2]);
            let values = actual[..3].try_into().map(|v: [f32; 3]| v.map(|x| f64::from(x) / f64::from(8e-8f32))).unwrap();
            let distance = ((values[0] * 360. - h + 180.).rem_euclid(360.) - 180.).abs();
            assert!(distance < 0.002, "{space:?} physical hue {values:?} vs {h}");
            assert!((values[1] - c).abs() < 2e-6, "{space:?} chroma");
            let weight = membership(h, c, 0., 0., 90.);
            assert!((values[2] - weight).abs() < 3e-5, "{space:?} membership {values:?} vs {weight}");
            weights.push(values[2]);
        }
        observed.push(weights);
    }
    for weights in &observed[1..] {
        for (a, b) in weights.iter().zip(&observed[0]) { assert!((a - b).abs() < 3e-5, "physical profile membership {observed:?}"); }
    }
}

#[test]
fn p21_hue_hard_wrap_and_chroma_falloff_match_full_pixel_oracle() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        for image in [false, true] {
            for (center, width, feather, angles, chroma) in [
                (0., 0., 2., [359., 1., 358., 2.], 0.08),
                (30., 30., 0., [15.01, 14.99, 44.99, 45.01], 0.08),
                (0., 180., 90., [90., 135., 179., 180.], 0.08),
                (30., 30., 30., [20., 25., 30., 40.], 0.0125),
            ] {
                let mut hue = Hue::default(); hue.ranges[0] = [70., 25., -15., center, width, feather];
                let mut adjustment = effect(2, "hue_saturation", image); configure(&mut adjustment, &hue);
                for angle in angles {
                    let rgb = transform(physical_hue(angle, chroma), RgbSpace::Srgb, space).map(|v| v as f32);
                    let tolerance = if feather == 2. || chroma == 0.0125 { 5e-5 } else { 6e-6 };
                    assert_color_bound(frame(&mut r, &[adjustment.clone(), source(rgb, 0.37)]), hue_reference(input_reference(rgb, 0.37), space, &hue), 0.37, tolerance,
                        &format!("{space:?} {image} center{center} width{width} feather{feather} h{angle} c{chroma}"));
                }
            }
        }
    }
}

#[test]
fn p21_hue_overlaps_sum_before_clamping_and_do_not_cascade() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        let rgb = transform(physical_hue(30., 0.08), RgbSpace::Srgb, space).map(|v| v as f32);
        for image in [false, true] {
            let mut hue = Hue::default(); hue.master = [10., -10., 5.];
            hue.ranges[0] = [40., 70., 80., 30., 180., 0.]; hue.ranges[1] = [-15., 60., -20., 30., 180., 0.];
            let mut adjustment = effect(2, "hue_saturation", image); configure(&mut adjustment, &hue);
            let first = frame(&mut r, &[adjustment.clone(), source(rgb, 0.37)]);
            assert_color(first, hue_reference(input_reference(rgb, 0.37), space, &hue), 0.37, "overlap oracle");
            hue.ranges.swap(0, 1); configure(&mut adjustment, &hue);
            assert_eq!(frame(&mut r, &[adjustment.clone(), source(rgb, 0.37)]), first, "range order");
            let equivalent = Hue { master: [35., 100., 65.], ..Default::default() };
            configure(&mut adjustment, &equivalent);
            assert_color(frame(&mut r, &[adjustment, source(rgb, 0.37)]), first[..3].try_into().map(|a: [f32; 3]| a.map(|v| f64::from(v) / 0.37)).unwrap(), 0.37, "combined master");
        }
    }
}

#[test]
fn p21_hue_neutral_extended_and_zero_strength_photo_filter_are_exact() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
            let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            for image in [false, true] {
                for alpha in [0., 8e-8, 1. / 65535., 0.37, 1.] {
                    let rgb = [-0.125, 0.234567, 1.5]; let input = source(rgb, alpha);
                    let original = frame(&mut r, std::slice::from_ref(&input));
                    let mut hue = effect(2, "hue_saturation", image); configure(&mut hue, &Hue::default());
                    set(&mut hue, "colorize_hue", EffectValue::Number(297.));
                    set(&mut hue, "colorize_saturation", EffectValue::Number(100.));
                    assert_eq!(frame(&mut r, &[hue, input.clone()]), original, "neutral hue {space:?} {depth:?} {image} {alpha}");
                    let mut photo = effect(2, "photo_filter", image); set(&mut photo, "density", EffectValue::Number(0.));
                    assert_eq!(frame(&mut r, &[photo.clone(), input.clone()]), original, "density0");
                    set(&mut photo, "density", EffectValue::Number(100.));
                    set(&mut photo, "color", EffectValue::Color(RgbColor::new(RgbSpace::DisplayP3, [0.9, 0.1, 0.8, 0.]).unwrap()));
                    assert_eq!(frame(&mut r, &[photo, input]), original, "color alpha0");
                }
            }
        }
    }
}

#[test]
fn p21_colorize_overrides_hue_saturation_ranges_but_retains_master_lightness() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::U16 }).unwrap();
        for image in [false, true] {
            for (lightness, expected) in [(0., [0.16, 0.64, 0.16]), (25., [0.28, 0.82, 0.28]), (-25., [0.12, 0.48, 0.12])] {
                let mut hue = Hue { master: [120., -100., lightness], colorize: Some([120., 60.]), ..Default::default() };
                for range in &mut hue.ranges { range[..3].copy_from_slice(&[170., 100., -100.]); }
                let mut adjustment = effect(2, "hue_saturation", image); configure(&mut adjustment, &hue);
                let rgb = [space.decode(0.4) as f32; 3];
                assert_color(frame(&mut r, &[adjustment, source(rgb, 0.37)]), expected.map(|v| space.decode(v)), 0.37,
                    &format!("Colorize {space:?} {image} {lightness}"));
            }
            let mut hue = Hue::default(); for range in &mut hue.ranges { range[1] = -100.; range[2] = 100.; }
            let mut adjustment = effect(2, "hue_saturation", image); configure(&mut adjustment, &hue);
            let rgb = [space.decode(0.4) as f32; 3];
            assert_color(frame(&mut r, &[adjustment, source(rgb, 8e-8)]), rgb.map(f64::from), 8e-8, "gray chroma gate");
        }
    }
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        for image in [false, true] {
            for encoded in [[-0.2, 0.8, 0.3], [0.1, 0.4, 1.5], [-0.2, 1.2, 0.4], [-0.125; 3]] {
                for alpha in [8e-8, 0.37] {
                    let rgb = encoded.map(|v| space.decode(v) as f32);
                    let hue = Hue { master: [170., -100., 25.], colorize: Some([240., 60.]), ..Default::default() };
                    let mut adjustment = effect(2, "hue_saturation", image); configure(&mut adjustment, &hue);
                    assert_color(frame(&mut r, &[adjustment, source(rgb, alpha)]),
                        hue_reference(input_reference(rgb, alpha), space, &hue), alpha,
                        &format!("extended Colorize {space:?} image={image} encoded={encoded:?} alpha={alpha}"));
                }
            }
        }
    }
}

#[test]
fn p21_invert_desaturate_and_threshold_follow_document_encoded_coordinates() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        for image in [false, true] {
            for alpha in [0., 8e-8, 0.37, 1.] {
                let encoded = [-0.25, 0.375, 1.25]; let rgb = encoded.map(|v| space.decode(v) as f32);
                assert_color(frame(&mut r, &[effect(2, "invert", image), source(rgb, alpha)]), encoded.map(|v| space.decode(1. - v)), alpha, "Invert");
                let rgb = [-0.2, 1.2, 0.4].map(|v| space.decode(v) as f32);
                let desaturated = frame(&mut r, &[effect(2, "desaturate", image), source(rgb, alpha)]);
                assert_color(desaturated, [space.decode(0.5); 3], alpha, "Desaturate lightness");
                let mut master = effect(2, "hue_saturation", image); set(&mut master, "saturation", EffectValue::Number(-100.));
                assert_color(frame(&mut r, &[master, source(rgb, alpha)]), [space.decode(0.5); 3], alpha, "master desaturation");
            }
            let coefficient = space.to_xyz()[1][1] as f32;
            for (threshold, expected) in [(coefficient * 0.5, 1.), (coefficient * 1.1, 0.)] {
                let adjustment = self::threshold(image, threshold);
                assert_color(frame(&mut r, &[adjustment, source([0., 1., 0.], 1.)]), [expected; 3], 1., "Threshold green luminance");
            }
            for (encoded, threshold, expected) in [(0., 0., 1.), (-0.25, -0.5, 1.), (1.25, 2., 0.)] {
                let adjustment = self::threshold(image, threshold);
                assert_color(frame(&mut r, &[adjustment, source([space.decode(encoded) as f32; 3], 0.37)]), [expected; 3], 0.37, "Threshold extended bounds");
            }
        }
    }
}

#[test]
fn p21_photo_filter_converts_tagged_color_and_preserves_encoded_document_luma() {
    let tag = RgbColor::new(RgbSpace::DisplayP3, [0.9, 0.2, 0.15, 0.37]).unwrap();
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        let rgb = [-0.2, 1.2, 0.4].map(|v| space.decode(v) as f32);
        let tagged = transform(tag.rgba[..3].try_into().map(|v: [f32; 3]| v.map(|x| RgbSpace::DisplayP3.decode(f64::from(x)))).unwrap(), RgbSpace::DisplayP3, space).map(|v| space.encode(v));
        for image in [false, true] {
            for preserve in [false, true] {
                let mut photo = effect(2, "photo_filter", image);
                set(&mut photo, "color", EffectValue::Color(tag)); set(&mut photo, "density", EffectValue::Number(40.));
                set(&mut photo, "preserve_luminance", EffectValue::Toggle(preserve));
                for alpha in [8e-8, 0.37, 1.] {
                    let original = input_reference(rgb, alpha).map(|v| space.encode(v));
                    let strength = 0.4 * f64::from(tag.rgba[3]);
                    let mut result: [f64; 3] = std::array::from_fn(|i| original[i] * (1. - strength) + tagged[i] * strength);
                    if preserve {
                        let weights = space.to_xyz()[1];
                        let correction = (0..3).map(|i| weights[i] * (original[i] - result[i])).sum::<f64>();
                        result = result.map(|v| v + correction);
                    }
                    assert_color(frame(&mut r, &[photo.clone(), source(rgb, alpha)]), result.map(|v| space.decode(v)), alpha,
                        &format!("Photo Filter {space:?} image={image} preserve={preserve} alpha={alpha}"));
                }
            }
        }
    }
}
