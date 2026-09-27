//! Fill generators and vignette removal against CPU oracles at every depth.
use super::*;
use layer_core::{GradientStop, color::RgbColor};

const EXTENT: [u32; 2] = [256, 256];
const DEPTHS: [SampleDepth; 4] = [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32];

fn composite(r: &mut WgpuRasterizer, layers: &[Layer]) -> Vec<[f32; 4]> {
    r.submit(packet(layers, EXTENT)).unwrap();
    crate::layer_tests::page_bytes(r, r.composite_texture.as_ref().unwrap())
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
    fn layer(&self, space: RgbSpace, end: [f32; 3]) -> Layer {
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
        let extent = EXTENT.map(f64::from);
        let d = [
            f64::from(x) + 0.5 - extent[0] * f64::from(self.center[0]) / 100.,
            f64::from(y) + 0.5 - extent[1] * f64::from(self.center[1]) / 100.,
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
