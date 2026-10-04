//! Fill generators and vignette removal against CPU oracles at every depth.
use super::*;
use layer_core::{GradientStop, color::RgbColor};

const EXTENT: [u32; 2] = [256, 256];
const DEPTHS: [SampleDepth; 4] = [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32];

fn composite(r: &mut WgpuRasterizer, layers: &[EffectInstance]) -> Vec<[f32; 4]> {
    let document=effect_document(layers,EXTENT,r.document_color);
    r.submit(packet(document.scene().with_owner(0,0),EXTENT)).unwrap();
    crate::layer_tests::page_bytes(r, crate::test_support::document_texture(r))
        .chunks_exact(16)
        .map(|p| std::array::from_fn(|c| f32::from_le_bytes(p[c * 4..c * 4 + 4].try_into().unwrap())))
        .collect()
}
fn at(pixels: &[[f32; 4]], [x, y]: [u32; 2]) -> [f32; 4] {
    pixels[(y * EXTENT[0] + x) as usize]
}

struct Gradient {
    radial: bool,
    reverse: bool,
    angle: f32,
    scale: f32,
    center: [f32; 2],
}
impl Gradient {
    fn layer(&self, space: RgbSpace, end: [f32; 3]) -> EffectInstance {
        let mut layer = effect(3, "gradient_fill", false);
        let stop = |position, rgb: [f32; 3]| GradientStop {
            position,
            color: RgbColor::new(space, [rgb[0], rgb[1], rgb[2], 1.]).unwrap(),
        };
        set(&mut layer, "gradient", EffectValue::Gradient(vec![stop(0., [0.; 3]), stop(1., end)]));
        set(&mut layer, "style", EffectValue::Choice(u32::from(self.radial)));
        set(&mut layer, "reverse", EffectValue::Toggle(self.reverse));
        set(&mut layer, "angle", EffectValue::Number(self.angle));
        set(&mut layer, "scale", EffectValue::Number(self.scale));
        set(&mut layer, "center_x", EffectValue::Number(self.center[0]));
        set(&mut layer, "center_y", EffectValue::Number(self.center[1]));
        layer
    }
    /// Position along the gradient of the pixel centre at `[x, y]`.
    fn t(&self, [x, y]: [u32; 2]) -> f64 {
        self.position([f64::from(x) + 0.5, f64::from(y) + 0.5])
    }
    fn position(&self, [x, y]: [f64; 2]) -> f64 {
        let extent = EXTENT.map(f64::from);
        let d = [
            x - extent[0] * f64::from(self.center[0]) / 100.,
            y - extent[1] * f64::from(self.center[1]) / 100.,
        ];
        let scale = f64::from(self.scale) / 100.;
        let t = if self.radial {
            d[0].hypot(d[1]) / (0.5 * extent[0].hypot(extent[1]) * scale)
        } else {
            let angle = f64::from(self.angle).to_radians();
            let axis = [angle.cos(), -angle.sin()];
            (d[0] * axis[0] + d[1] * axis[1]) / ((axis[0].abs() * extent[0] + axis[1].abs() * extent[1]) * scale) + 0.5
        };
        (if self.reverse { 1. - t } else { t }).clamp(0., 1.)
    }
}

#[test]
fn fill_thumbnails_render_gradient_parameters_and_future_multipass_generators() {
    let points = [[4usize, 8usize], [15, 16], [27, 23]];
    for depth in DEPTHS {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth }).unwrap();
        r.set_ui_rendition(None).unwrap();
        let mut previous = Vec::new();
        for gradient in [
            Gradient { radial: false, reverse: false, angle: 0., scale: 100., center: [50.; 2] },
            Gradient { radial: false, reverse: true, angle: 90., scale: 60., center: [40., 60.] },
            Gradient { radial: true, reverse: false, angle: 0., scale: 80., center: [30., 70.] },
        ] {
            let layer = gradient.layer(RgbSpace::Srgb, [1.; 3]);
            let mut document = effect_document(&[layer], EXTENT, r.document_color);
            let owner = document.scene().order()[0];
            document.artwork.occurrences.get_mut(owner).unwrap().opacity = 0.1;
            mask(&mut document, owner, 0.);
            r.submit(packet(document.scene().with_owner(0,0), EXTENT)).unwrap();
            let bytes = crate::source_thumbnails::tests::thumbnail(&mut r, layer_render::ThumbnailTarget::Occurrence(owner));
            assert_ne!(bytes, previous, "edited parameters must replace the gradient thumbnail");
            for [x, y] in points {
                let expected = (gradient.position([(x as f64 + 0.5) * 8., (y as f64 + 0.5) * 8.]) * 255.).round() as i32;
                let pixel = &bytes[(y * 32 + x) * 4..][..4];
                assert_eq!(pixel[3], 255);
                for value in &pixel[..3] { assert!((i32::from(*value) - expected).abs() <= 2, "{depth:?} {x},{y}: {pixel:?} expected {expected}"); }
            }
            previous = bytes;
        }
        let mut layer = effect(5, "solid_color", false);
        let program = Arc::make_mut(&mut layer.program);
        program.id = "future_fill".into();
        program.constant_color = None;
        program.entry = "future_first".into();
        program.wgsl = "fn future_first(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(p/fx_extent(),.25,1.)*.5;}\nfn future_second(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(c.bgr,c.a);}".into();
        program.passes = vec![
            EffectPass { entry: "future_first".into(), sampling: EffectSampling::Neighborhood { radius: 0 } },
            EffectPass { entry: "future_second".into(), sampling: EffectSampling::Document },
        ].into();
        for extent in [[96, 64], [2040, 1360]] {
            let mut document = effect_document(std::slice::from_ref(&layer), extent, r.document_color);
            let owner = document.scene().order()[0];
            let occurrence = document.artwork.occurrences.get_mut(owner).unwrap();
            occurrence.visible = false;
            occurrence.opacity = 0.1;
            r.submit(packet(document.scene().with_owner(0,0), extent)).unwrap();
            let bytes = crate::source_thumbnails::tests::thumbnail(&mut r, layer_render::ThumbnailTarget::Occurrence(owner));
            for [x, y] in points {
                let checker = if (x / 4 + y / 4) % 2 == 0 { 0.855 } else { 0.497 };
                let expected = [0.25, (y as f64 + 0.5 - 16. / 3.) / (64. / 3.), (x as f64 + 0.5) / 32.]
                    .map(|value| (RgbSpace::Srgb.encode(value * 0.5 + checker * 0.5) * 255.).round() as i32);
                let pixel = &bytes[(y * 32 + x) * 4..][..4];
                for c in 0..3 { assert!((i32::from(pixel[c]) - expected[c]).abs() <= 2, "{depth:?} {extent:?} {x},{y}: {pixel:?} expected {expected:?}"); }
            }
            assert!(r.thumbnails.storage_bytes() < 1024 * 1024, "fill thumbnail storage is bounded independently of canvas extent");
        }
    }
}

#[test]
fn native_fill_generators_match_color_and_gradient_oracles_at_every_depth() {
    let color = RgbColor::new(RgbSpace::DisplayP3, [0.9, 0.2, 0.15, 0.37]).unwrap();
    let end = [1., 0.5, 0.25];
    let linear = Gradient { radial: false, reverse: false, angle: 0., scale: 100., center: [50.; 2] };
    let gradients = [
        Gradient { angle: 90., ..linear },
        Gradient { reverse: true, center: [25., 50.], ..linear },
        Gradient { angle: 30., scale: 60., ..linear },
        Gradient { radial: true, ..linear },
        Gradient { radial: true, scale: 50., center: [30., 70.], reverse: true, ..linear },
        linear,
    ];
    let points = [[0, 0], [37, 200], [128, 128], [191, 64], [255, 255]];
    for space in [RgbSpace::Srgb, RgbSpace::ProPhoto] {
        for depth in DEPTHS {
            let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            let mut solid = effect(2, "solid_color", false);
            set(&mut solid, "color", EffectValue::Color(color));
            let pixels = composite(&mut r, &[solid]);
            let expected = color.linear_in(space).unwrap();
            for point in points {
                close(at(&pixels, point), expected[..3].try_into().unwrap(), color.rgba[3],
                    &format!("solid {space:?} {depth:?} {point:?}"));
            }
            for gradient in &gradients {
                let pixels = composite(&mut r, &[gradient.layer(space, end)]);
                for point in points {
                    let t = gradient.t(point);
                    let actual = at(&pixels, point);
                    assert_eq!(actual[3], 1., "{space:?} {depth:?} {point:?}");
                    for c in 0..3 {
                        let expected = space.decode(t * f64::from(end[c]));
                        assert!(
                            (f64::from(actual[c]) - expected).abs() <= 2e-5,
                            "gradient radial={} {space:?} {depth:?} {point:?} t={t} channel {c}: {} vs {expected}",
                            gradient.radial,
                            actual[c]
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn native_negative_vignette_brightens_corners_within_alpha_unless_hdr() {
    for depth in DEPTHS {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth }).unwrap();
        let mut vignette = effect(2, "vignette", false);
        set(&mut vignette, "strength", EffectValue::Number(-50.));
        let pixels = composite(&mut r, &[vignette.clone(), source([0.6; 3], 0.8)]);
        close(at(&pixels, [128, 128]), [0.6; 3], 0.8, &format!("{depth:?}: the centre is untouched"));
        let corner = at(&pixels, [0, 0]);
        assert_eq!(corner[3], 0.8, "{depth:?}");
        let straight = corner[0] / corner[3];
        if depth.is_float() {
            assert!((straight - 1.2).abs() <= 1e-5, "{depth:?}: +1 EV in the corner: {straight}");
        } else {
            assert!(corner[..3].iter().all(|v| *v <= corner[3]), "{depth:?}: SDR keeps rgb within alpha: {corner:?}");
            assert!((straight - 1.).abs() <= 1e-6, "{depth:?}: brightened to white: {straight}");
        }
        set(&mut vignette, "strength", EffectValue::Number(40.));
        let corner = at(&composite(&mut r, &[vignette, source([0.6; 3], 0.8)]), [0, 0]);
        assert!(corner[0] / corner[3] < 0.6 * 0.6, "{depth:?}: positive strength still darkens");
    }
}
