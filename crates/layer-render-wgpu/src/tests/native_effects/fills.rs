//! Fill generators and vignette removal against CPU oracles at every depth.
use super::*;
use layer_core::{GradientDefinition, GradientStop, ColorMixSpace, GradientShape, color::RgbColor};

const EXTENT: [u32; 2] = [256, 256];
const DEPTHS: [SampleDepth; 4] = [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32];

fn triangular_noise([x,y]:[u32;2])->f64 {
    let seed=x.wrapping_mul(0x9e3779b9).wrapping_add(y.wrapping_mul(0x85ebca6b)).wrapping_add(0x632be59b);
    let hash=|mut v:u32| {v=(v^(v>>16)).wrapping_mul(0x7feb352d);v=(v^(v>>15)).wrapping_mul(0x846ca68b);v^(v>>16)};
    (f64::from(hash(seed)&65535)+f64::from(hash(seed^0xa511e9b3)&65535)+1.)/65536.-1.
}


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
    shape: GradientShape,
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
        let mut gradient=GradientDefinition { stops: vec![stop(0., [0.; 3]), stop(1., end)], interpolation: ColorMixSpace::Classic };
        if self.reverse {gradient.reverse();}
        set(&mut layer, "gradient", EffectValue::Gradient(gradient));
        set(&mut layer, "style", EffectValue::Choice(self.shape as u32));
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
        let t = if self.shape == GradientShape::Radial {
            d[0].hypot(d[1]) / (0.5 * extent[0].hypot(extent[1]) * scale)
        } else {
            let angle = f64::from(self.angle).to_radians();
            let axis = [angle.cos(), -angle.sin()];
            let progress = (d[0] * axis[0] + d[1] * axis[1]) / ((axis[0].abs() * extent[0] + axis[1].abs() * extent[1]) * scale);
            if self.shape == GradientShape::Reflected { progress.abs() } else { progress + 0.5 }
        };
        (if self.reverse { 1. - t } else { t }).clamp(0., 1.)
    }
}

#[test]
fn fill_thumbnails_fill_their_square_for_gradients_and_future_multipass_generators() {
    let points = [[4usize, 8usize], [15, 16], [27, 23]];
    for depth in DEPTHS {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth }).unwrap();
        r.set_ui_rendition(None).unwrap();
        let mut previous = Vec::new();
        for gradient in [
            Gradient { shape: GradientShape::Linear, reverse: false, angle: 0., scale: 100., center: [50.; 2] },
            Gradient { shape: GradientShape::Linear, reverse: true, angle: 90., scale: 60., center: [40., 60.] },
            Gradient { shape: GradientShape::Radial, reverse: false, angle: 0., scale: 80., center: [30., 70.] },
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
                let expected = [0.25, (y as f64 + 0.5) / 32., (x as f64 + 8.5) / 48.]
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
    let linear = Gradient { shape: GradientShape::Linear, reverse: false, angle: 0., scale: 100., center: [50.; 2] };
    let gradients = [
        Gradient { angle: 90., ..linear },
        Gradient { reverse: true, center: [25., 50.], ..linear },
        Gradient { angle: 30., scale: 60., ..linear },
        Gradient { shape: GradientShape::Radial, ..linear },
        Gradient { shape: GradientShape::Radial, scale: 50., center: [30., 70.], reverse: true, ..linear },
        Gradient { shape: GradientShape::Reflected, ..linear },
        Gradient { shape: GradientShape::Reflected, reverse: true, angle: 30., ..linear },
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
                        let mut encoded=t * f64::from(end[c]);
                        if !depth.is_float() && t>0. && t<1. {
                            let noise=triangular_noise(point);
                            encoded+=noise/f64::from((1u32<<depth.bits())-1);
                        }
                        let expected = space.decode(encoded);
                        assert!(
                            (f64::from(actual[c]) - expected).abs() <= 2e-5,
                            "gradient shape={:?} {space:?} {depth:?} {point:?} t={t} channel {c}: {} vs {expected}",
                            gradient.shape,
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

fn gradient_reference(a: [f32; 4], b: [f32; 4], t: f64, space: RgbSpace, mix: ColorMixSpace) -> [f64; 4] {
    let lab = |rgb: [f64; 3]| {
        let rgb = layer_core::color::rgb::apply(space.linear_transform(RgbSpace::Srgb), rgb);
        let l = (0.4122214708*rgb[0]+0.5363325363*rgb[1]+0.0514459929*rgb[2]).cbrt();
        let m = (0.2119034982*rgb[0]+0.6806995451*rgb[1]+0.1073969566*rgb[2]).cbrt();
        let s = (0.0883024619*rgb[0]+0.2817188376*rgb[1]+0.6299787005*rgb[2]).cbrt();
        [0.2104542553*l+0.7936177850*m-0.0040720468*s,
            1.9779984951*l-2.4285922050*m+0.4505937099*s,
            0.0259040371*l+0.7827717662*m-0.8086757660*s]
    };
    let coordinates = |p: [f32; 4]| {
        let rgb = [p[0], p[1], p[2]].map(f64::from);
        match mix { ColorMixSpace::LinearRgb => rgb, ColorMixSpace::Classic => rgb.map(|v| space.encode(v)), ColorMixSpace::Oklab => lab(rgb) }
    };
    let alpha = f64::from(a[3])*(1.-t)+f64::from(b[3])*t;
    let ca = coordinates(a); let cb = coordinates(b);
    let v: [f64; 3] = std::array::from_fn(|c| if alpha == 0. { 0. } else {
        (ca[c]*f64::from(a[3])*(1.-t)+cb[c]*f64::from(b[3])*t)/alpha
    });
    let rgb = match mix {
        ColorMixSpace::LinearRgb => v,
        ColorMixSpace::Classic => v.map(|v| space.decode(v)),
        ColorMixSpace::Oklab => {
            let l=(v[0]+0.3963377774*v[1]+0.2158037573*v[2]).powi(3);
            let m=(v[0]-0.1055613458*v[1]-0.0638541728*v[2]).powi(3);
            let s=(v[0]-0.0894841775*v[1]-1.2914855480*v[2]).powi(3);
            layer_core::color::rgb::apply(RgbSpace::Srgb.linear_transform(space),
                [4.0767416621*l-3.3077115913*m+0.2309699292*s,
                    -1.2684380046*l+2.6097574011*m-0.3413193965*s,
                    -0.0041960863*l-0.7034186147*m+1.7076147010*s])
        }
    };
    [rgb[0]*alpha,rgb[1]*alpha,rgb[2]*alpha,alpha]
}

#[test]
fn native_gradient_mixes_weight_alpha_and_preserve_hidden_endpoint_color_independently() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth: SampleDepth::F32 }).unwrap();
        for mix in ColorMixSpace::ALL {
            for (a, b) in [([1.,0.,0.,1.],[0.,1.,0.,0.]),
                ([-0.125,2.,0.3,0.2],[1.,0.25,-0.1,0.8])] {
                let mut layer = Gradient { shape: GradientShape::Linear, reverse: false, angle: 0., scale: 100., center: [50.; 2] }.layer(space, [1.; 3]);
                let gradient = GradientDefinition { stops: [a,b].into_iter().enumerate().map(|(i,p)| GradientStop {
                    position: i as f32, color: RgbColor::from_linear(space,p).unwrap(),
                }).collect(), interpolation: mix };
                set(&mut layer,"gradient",EffectValue::Gradient(gradient));
                let pixels = composite(&mut r,&[layer]);
                for point in [[0,0],[37,200],[128,128],[191,64],[255,255]] {
                    let expected = gradient_reference(a,b,(f64::from(point[0])+0.5)/256.,space,mix);
                    let actual = at(&pixels,point);
                    for c in 0..4 { assert!((f64::from(actual[c])-expected[c]).abs()<=2e-5,
                        "{space:?}/{mix:?}/{point:?}/{c}: {} vs {}",actual[c],expected[c]); }
                }
            }
        }
    }
}

#[test]
fn native_gradient_integer_triangular_noise_is_bounded_and_float_depth_is_unchanged() {
    for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F32] {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth }).unwrap();
        let layer = Gradient { shape: GradientShape::Linear, reverse: false, angle: 0., scale: 100., center: [50.;2] }.layer(RgbSpace::Srgb,[1.;3]);
        let actual = composite(&mut r,&[layer]);
        let maximum = if depth == SampleDepth::U16 {65535.} else {255.};
        let mut sum=0.; let mut residual=0.; let mut changed=0;
        let mut phases:[Vec<f64>;8]=std::array::from_fn(|_|Vec::new());
        for (index,pixel) in actual.iter().enumerate() {
            let expected=(index % 256) as f64 / 256. + 0.5 / 256.;
            let [x,y]=[(index%256) as u32,(index/256) as u32];
            let noise=if depth.is_float() {0.} else {triangular_noise([x,y])};
            assert!(noise.abs()<1.);
            let noised=expected+noise/maximum;
            assert_eq!(pixel[3],1.);
            for value in &pixel[..3] {
                assert!((f64::from(*value)-RgbSpace::Srgb.decode(noised)).abs()<2e-6,
                    "{depth:?} pixel{index}: {} vs {}",*value,RgbSpace::Srgb.decode(noised));
                let encoded=RgbSpace::Srgb.encode(f64::from(*value));
                let error=(encoded-expected)*maximum;
                if index%256>2 && index%256<253 {
                    let code=expected*maximum;
                    let phase=((code.fract()*8.).floor() as usize).min(7);
                    phases[phase].push((encoded*maximum).round()-code);
                }
                residual+=(error-noise).abs();
                sum+=error; changed+=usize::from(error.abs()>0.01);
            }
        }
        let count=(actual.len()*3) as f64;
        if depth!=SampleDepth::F32 {
            assert!(changed>actual.len());assert!((sum/count).abs()<0.01);
            assert!(residual/count<0.01,"{depth:?}: mean floating transfer residue {} codes",residual/count);
            for (phase,errors) in phases.iter().enumerate() {
                let mean=errors.iter().sum::<f64>()/errors.len() as f64;
                let variance=errors.iter().map(|v|(v-mean).powi(2)).sum::<f64>()/errors.len() as f64;
                println!("TPDF_PHASE {depth:?} {phase} mean={mean} variance={variance}");
                assert!(mean.abs()<0.035,"{depth:?} phase{phase}: bias{mean}");
                assert!((0.20..0.30).contains(&variance),"{depth:?} phase{phase}: variance{variance}");
            }
        }
    }
}

#[test]
fn native_gradient_triangular_noise_preserves_constants_endpoints_and_alpha() {
    for depth in DEPTHS {
        let mut r=WgpuRasterizer::new_native_headless(DocumentColor {space:RgbSpace::Srgb,depth}).unwrap();
        for endpoints in [([0.3,0.3,0.3,0.625],[0.3,0.3,0.3,0.625]),([1.,0.,0.,0.625],[0.,0.,1.,0.])] {
            let mut layer=Gradient {shape:GradientShape::Linear,reverse:false,angle:0.,scale:10.,center:[50.;2]}.layer(RgbSpace::Srgb,[1.;3]);
            set(&mut layer,"gradient",EffectValue::Gradient(GradientDefinition {stops:[endpoints.0,endpoints.1].into_iter().enumerate().map(|(i,rgba)|GradientStop {position:i as f32,color:RgbColor::new(RgbSpace::Srgb,rgba).unwrap()}).collect(),interpolation:ColorMixSpace::Classic}));
            let pixels=composite(&mut r,&[layer]);
            for (point,rgba) in [([0,0],endpoints.0),([255,255],endpoints.1)] {
                let p=at(&pixels,point);assert_eq!(p[3],rgba[3]);
                for c in 0..3 {let expected=if rgba[3]==0. {0.}else{RgbSpace::Srgb.decode(f64::from(rgba[c]))*f64::from(rgba[3])};assert!((f64::from(p[c])-expected).abs()<2e-6);}
            }
            if endpoints.0==endpoints.1 {assert!(pixels.iter().all(|p|*p==pixels[0]));}
        }
    }
}
