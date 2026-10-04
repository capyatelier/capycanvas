//! Filters that follow the document's Blending read and write encoded values
//! in Perceptual documents, as in Photoshop, checked against CPU references;
//! undeclared filters keep reading linear values.
use super::*;
use layer_core::color::RgbSpace;
use layer_core::{BlendSpace, EffectInstance, EffectSpace, EffectValue};
use super::native_effects::{effect_document,insert_source};

const EXTENT: [u32; 2] = [96, 64];
type Image = Vec<[f64; 4]>;

fn photo(pixel: impl Fn(u32,u32)->[u8;4]) -> Arc<layer_core::color::source::SourceImage> {
    layer_core::color::source::rgba8_source(EXTENT,pixel)
}

fn edge(x: u32, _: u32) -> [u8; 4] {
    if x < EXTENT[0] / 2 { [0, 0, 0, 255] } else { [255; 4] }
}

fn texture(x: u32, y: u32) -> [u8; 4] {
    let wave = (x as f32 * 0.37).sin() * (y as f32 * 0.29).cos();
    [if x < 48 { 40 } else { 215 }, (128. + 100. * wave) as u8, ((x * 7 + y * 13) % 256) as u8, 255]
}

fn filter(id: &str, values: &[(&str, f32)], space: EffectSpace) -> EffectInstance {
    let mut program = (*fixture(id).program()).clone();
    program.space = space;
    let mut effect = EffectInstance::new(Arc::new(program));
    for (key, value) in values {
        effect.set(key, EffectValue::Number(*value)).unwrap();
    }
    effect
}

/// The composite, as the document's blend space holds it.
fn composite(space: BlendSpace, effects: &[EffectInstance], source: Arc<layer_core::color::source::SourceImage>) -> Image {
    let mut document=effect_document(effects,EXTENT,Default::default());
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().blend=space;
    insert_source(&mut document,"Photo",source);
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let view = crate::test_support::view(EXTENT);
    let packet = FramePacket { view, reset_layers: true, blend_space: space, ..packet(document.scene(), EXTENT) };
    r.submit(packet).unwrap();
    while layer_render::CanvasRenderer::has_pending_work(&r) {
        r.submit(FramePacket { reset_layers: false, composite_all: false, ..packet }).unwrap();
    }
    crate::layer_tests::page_bytes(&r, crate::test_support::document_texture(&r))
        .chunks_exact(16)
        .map(|p| std::array::from_fn(|c| f64::from(f32::from_le_bytes(p[c * 4..][..4].try_into().unwrap()))))
        .collect()
}

fn at(image: &Image, x: i64, y: i64) -> [f64; 4] {
    let [w, h] = EXTENT.map(i64::from);
    image[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize]
}

/// The discrete Gaussian the filter prepares, applied along x, then y.
fn gaussian(image: &Image, sigma: f64) -> Image {
    let radius = (sigma * 3.).ceil() as i64;
    let weights: Vec<f64> = (0..=radius).map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp()).collect();
    let sum = weights[0] + 2. * weights[1..].iter().sum::<f64>();
    let pass = |image: &Image, step: [i64; 2]| -> Image {
        (0..image.len() as i64)
            .map(|i| {
                let [x, y] = [i % i64::from(EXTENT[0]), i / i64::from(EXTENT[0])];
                (-radius..=radius).fold([0.; 4], |mut total, k| {
                    let p = at(image, x + k * step[0], y + k * step[1]);
                    let w = weights[k.unsigned_abs() as usize] / sum;
                    for c in 0..4 {
                        total[c] += w * p[c];
                    }
                    total
                })
            })
            .collect()
    };
    pass(&pass(image, [1, 0]), [0, 1])
}

fn map(image: &Image, f: impl Fn(usize, [f64; 4]) -> [f64; 4]) -> Image {
    image.iter().enumerate().map(|(i, p)| f(i, *p)).collect()
}

/// Every channel of `actual` within `tolerance` of `expected`, away from the
/// document's edges by `margin`.
fn assert_matches(label: &str, actual: &Image, expected: &Image, margin: i64, tolerance: f64) {
    let [w, h] = EXTENT.map(i64::from);
    for y in margin..h - margin {
        for x in margin..w - margin {
            let (a, e) = (at(actual, x, y), at(expected, x, y));
            for c in 0..4 {
                assert!((a[c] - e[c]).abs() <= tolerance, "{label} at {x},{y} channel {c}: {} != {}", a[c], e[c]);
            }
        }
    }
}

#[test]
fn gaussian_blur_gives_the_photoshop_midpoint_in_perceptual_documents() {
    let encode = |v: f64| RgbSpace::Srgb.encode(v);
    for (space, expected) in [(BlendSpace::Perceptual, 127.5), (BlendSpace::Linear, 188.)] {
        let layers = [filter("gaussian_blur", &[("sigma", 3.)], EffectSpace::Blending)];
        let blurred = composite(space, &layers, photo(edge));
        let row = i64::from(EXTENT[1] / 2);
        let middle = i64::from(EXTENT[0] / 2);
        let [left, right] = [at(&blurred, middle - 1, row)[1], at(&blurred, middle, row)[1]];
        let code = 255. * if space == BlendSpace::Perceptual { (left + right) / 2. } else { encode((left + right) / 2.) };
        assert!((code - expected).abs() <= 1., "{space:?}: the edge's midpoint is {code}, not {expected}");
        let original = composite(space, &[], photo(edge));
        assert_matches(&format!("{space:?} blur"), &blurred, &gaussian(&original, 3.), 12, 1e-4);
    }
}

fn straight(p: [f64; 4]) -> [f64; 3] {
    [p[0] / p[3], p[1] / p[3], p[2] / p[3]]
}

/// Unsharp Mask at sigma 1.5, amount 100 and threshold 2.
fn unsharp(original: &Image) -> Image {
    let blurred = gaussian(original, 1.5);
    map(original, |i, p| {
        let detail: [f64; 3] = std::array::from_fn(|c| straight(p)[c] - straight(blurred[i])[c]);
        let length = detail.iter().map(|d| d * d).sum::<f64>().sqrt();
        let t = ((length - 0.02) / 0.02).clamp(0., 1.);
        let gate = t * t * (3. - 2. * t);
        let rgb: [f64; 3] = std::array::from_fn(|c| (straight(p)[c] + detail[c] * gate).clamp(0., 1.));
        [rgb[0] * p[3], rgb[1] * p[3], rgb[2] * p[3], p[3]]
    })
}

const UNSHARP: [(&str, f32); 3] = [("sigma", 1.5), ("amount", 100.), ("threshold", 2.)];

#[test]
fn declared_filters_match_encoded_references() {
    let space = BlendSpace::Perceptual;
    let original = composite(space, &[], photo(texture));
    let blur = |sigma| gaussian(&original, sigma);
    let unsharp = unsharp(&original);
    let high_pass = {
        let blurred = blur(4.);
        map(&original, |i, p| {
            let rgb: [f64; 3] = std::array::from_fn(|c| 0.5 + straight(p)[c] - straight(blurred[i])[c]);
            [rgb[0] * p[3], rgb[1] * p[3], rgb[2] * p[3], p[3]]
        })
    };
    let soft_focus = {
        let blurred = blur(5.);
        map(&original, |i, p| {
            let soft = straight(blurred[i]);
            std::array::from_fn(|c| if c == 3 { p[3] } else { p[c] + 0.4 * (p[c].max(soft[c] * p[3]) - p[c]) })
        })
    };
    let denoise = map(&original, |i, p| {
        let [x, y] = [i as i64 % i64::from(EXTENT[0]), i as i64 / i64::from(EXTENT[0])];
        let center = straight(p);
        let range = 0.01 + 0.25 * 0.35;
        let (mut sum, mut total) = ([0.; 3], 0.);
        for dy in -2..=2i64 {
            for dx in -2..=2i64 {
                let v = at(&original, x + dx, y + dy);
                let diff: f64 = (0..3).map(|c| (straight(v)[c] - center[c]).powi(2)).sum();
                let weight = (-diff / (range * range)).exp2() / (1. + (dx * dx + dy * dy) as f64) * v[3];
                for (sum, value) in sum.iter_mut().zip(straight(v)) {
                    *sum += value * weight;
                }
                total += weight;
            }
        }
        [sum[0] / total * p[3], sum[1] / total * p[3], sum[2] / total * p[3], p[3]]
    });
    for (id, values, expected, margin) in [
        ("gaussian_blur", &[("sigma", 3.)][..], blur(3.), 12),
        ("unsharp_mask", &UNSHARP[..], unsharp, 8),
        ("high_pass", &[("sigma", 4.), ("amount", 100.)], high_pass, 15),
        ("soft_focus", &[("sigma", 5.), ("amount", 40.)], soft_focus, 18),
        ("denoise", &[("radius", 2.), ("strength", 25.)], denoise, 4),
    ] {
        assert_eq!(fixture(id).program().space, EffectSpace::Blending, "{id} follows the document's Blending");
        let actual = composite(space, &[filter(id, values, EffectSpace::Blending)], photo(texture));
        assert_matches(id, &actual, &expected, margin, 1e-4);
    }
}

#[test]
fn adjacent_filters_that_follow_the_documents_blending_share_their_image() {
    let space = BlendSpace::Perceptual;
    let original = composite(space, &[], photo(texture));
    let blur = filter("gaussian_blur", &[("sigma", 2.)], EffectSpace::Blending);
    let layers = [blur, filter("unsharp_mask", &UNSHARP, EffectSpace::Blending)];
    assert_matches("blur over unsharp mask", &composite(space, &layers, photo(texture)), &gaussian(&unsharp(&original), 2.), 16, 1e-4);
}

#[test]
fn light_filters_and_undeclared_filters_read_linear_values() {
    for id in ["vignette", "bloom", "motion_blur", "exposure"] {
        assert_eq!(fixture(id).program().space, EffectSpace::Linear, "{id} models light");
    }
    let halve = |space: EffectSpace| {
        let mut program = (*fixture("gaussian_blur").program()).clone();
        program.id = "halve".into();
        program.label = "Halve".into();
        program.space = space;
        program.lookups = Arc::from([]);
        program.parameters = Arc::from([]);
        program.entry = "halve".into();
        program.wgsl = "fn halve(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{let s=fx_sample(p);return vec4<f32>(s.rgb*.5,s.a);}".into();
        program.passes = vec![layer_core::EffectPass { entry: "halve".into(), sampling: layer_core::EffectSampling::Neighborhood { radius: 0 } }].into();
        EffectInstance::new(Arc::new(program))
    };
    let decode = |image: &Image| map(image, |_, p| {
        if p[3] <= 0. { return p; }
        std::array::from_fn(|c| if c == 3 { p[3] } else { RgbSpace::Srgb.decode(p[c] / p[3]) * p[3] })
    });
    let linear = composite(BlendSpace::Linear, &[halve(EffectSpace::Linear)], photo(texture));
    let perceptual = composite(BlendSpace::Perceptual, &[halve(EffectSpace::Linear)], photo(texture));
    assert_matches("undeclared", &decode(&perceptual), &linear, 0, 2e-6);
    let original = composite(BlendSpace::Perceptual, &[], photo(texture));
    let encoded = composite(BlendSpace::Perceptual, &[halve(EffectSpace::Blending)], photo(texture));
    assert_matches("declared", &encoded, &map(&original, |_, p| [p[0] / 2., p[1] / 2., p[2] / 2., p[3]]), 0, 1e-6);
    let blurred = composite(BlendSpace::Perceptual, &[filter("gaussian_blur", &[("sigma", 3.)], EffectSpace::Linear)], photo(edge));
    let row = i64::from(EXTENT[1] / 2);
    let middle = i64::from(EXTENT[0] / 2);
    let code = 255. * RgbSpace::Srgb.encode((RgbSpace::Srgb.decode(at(&blurred, middle - 1, row)[1]) + RgbSpace::Srgb.decode(at(&blurred, middle, row)[1])) / 2.);
    assert!((code - 188.).abs() <= 1., "an undeclared blur blends light: {code}");
}

#[test]
fn a_pointwise_filter_cannot_follow_the_documents_blending() {
    let mut program = (*fixture("curves").program()).clone();
    program.space = EffectSpace::Blending;
    assert!(EffectInstance::new(Arc::new(program)).validate().is_err());
}
