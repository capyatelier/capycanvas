use super::super::curve_reference::evaluate as curve_reference;
use super::*;

#[test]
fn native_tagged_effect_and_gradient_colors_match_document_rgb_in_both_depths() {
    use layer_core::{GradientStop, color::RgbColor};
    let color = RgbColor::new(RgbSpace::DisplayP3, [0.9, 0.2, 0.15, 0.37]).unwrap();
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            for image in [false, true] {
                for alpha in [0., 1. / 65535., 0.37, 1.] {
                    let mut ink = effect(2, "halftone", image);
                    set(&mut ink, "ink", EffectValue::Color(color));
                    // Halftone's opaque ink/paper semantics retain input coverage.
                    let expected = color.linear_in(space).unwrap();
                    close(frame(&mut r, &[ink, source([0.; 3], alpha)]),
                        expected[..3].try_into().unwrap(), alpha, &format!("ink {space:?} {depth:?} {image}"));
                    let mut gradient = effect(3, "gradient_map", image);
                    set(&mut gradient, "gradient", EffectValue::Gradient(vec![
                        GradientStop { position: 0., color }, GradientStop { position: 1., color },
                    ]));
                    // Gradient alpha is mapping strength; RGB interpolation occurs
                    // before document decoding, as declared by the table shader.
                    let encoded = color.encoded_in(space).unwrap();
                    let expected = std::array::from_fn(|c| space.decode(f64::from(encoded[c] * color.rgba[3])) as f32);
                    close(frame(&mut r, &[gradient, source([0.; 3], alpha)]), expected, alpha,
                        &format!("gradient {space:?} {depth:?} {image}"));
                }
            }
        }
    }
}

#[test]
fn native_neutral_tone_chains_preserve_extended_rgb_and_low_alpha_exactly() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let mut r =
                WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            for image in [false, true] {
                for alpha in [0., 0.00000008, 1. / 65535., 0.37, 1.] {
                    let source = source([-0.125, 0.234567, 1.5], alpha);
                    let expected = frame(&mut r, std::slice::from_ref(&source));
                    let mut layers = vec![source];
                    for i in 0..24 {
                        let mut e = effect(
                            2 + i,
                            [
                                "levels",
                                "curves",
                                "hue_saturation",
                                "color_balance",
                                "brightness_contrast",
                                "vibrance",
                                "gradient_map",
                            ][i as usize % 7],
                            image,
                        );
                        if e.name.as_ref() == "gradient_map" {
                            set(&mut e, "amount", EffectValue::Number(0.));
                        }
                        layers.insert(0, e);
                    }
                    assert_eq!(
                        frame(&mut r, &layers),
                        expected,
                        "{space:?} {depth:?} image={image}, alpha={alpha}"
                    );
                }
            }
        }
    }
}

#[test]
fn native_levels_clipping_is_explicit_and_matches_profiled_signed_gamma_reference() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
            space,
            depth: SampleDepth::U16,
        })
        .unwrap();
        let encoded: [f64; 3] = [-0.125, 0.41, 1.125];
        let rgb = encoded.map(|v| space.decode(v) as f32);
        for image in [false, true] {
            for gamma in [1., 1.7] {
                for clamp_input in [false, true] {
                    for clamp_output in [false, true] {
                        let mut fx = effect(2, "levels", image);
                        for (key, value) in [
                            ("black", 0.125),
                            ("white", 0.875),
                            ("gamma", gamma),
                            ("output_black", 0.1),
                            ("output_white", 0.9),
                        ] {
                            set(&mut fx, key, EffectValue::Number(value));
                        }
                        set(&mut fx, "clamp_input", EffectValue::Toggle(clamp_input));
                        set(&mut fx, "clamp_output", EffectValue::Toggle(clamp_output));
                        let expected = encoded.map(|v| {
                            let mut x = (v - 0.125) / 0.75;
                            if clamp_input {
                                x = x.clamp(0., 1.);
                            }
                            x = x.signum() * x.abs().powf(1. / f64::from(gamma));
                            x = 0.1 + 0.8 * x;
                            if clamp_output {
                                x = x.clamp(0., 1.);
                            }
                            space.decode(x) as f32
                        });
                        for alpha in [0., 0.00000008, 1. / 65535., 0.37, 1.] {
                            close(
                                frame(&mut r, &[fx.clone(), source(rgb, alpha)]),
                                expected,
                                alpha,
                                &format!(
                                    "levels {space:?}, image={image}, gamma={gamma}, clamps={clamp_input}/{clamp_output}"
                                ),
                            );
                        }
                    }
                }
            }
        }
    }
}

fn table_probe(name: &str, image: bool, component: usize) -> Layer {
    let mut l = effect(1, name, false);
    let p = Arc::make_mut(&mut Arc::make_mut(l.effect.as_mut().unwrap()).program);
    p.kind = EffectKind::Generator;
    p.entry = "table_probe".into();
    p.wgsl = format!(
        "fn table_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{
        let code=u32(p.y)*256u+u32(p.x);let x=f32(code)/65535.;
        let v=fx_lut(b,0u,x)[{component}u];return vec4<f32>(v,v,v,1.);}} "
    )
    .into();
    if image {
        p.passes = vec![EffectPass {
            entry: p.entry.clone(),
            sampling: EffectSampling::Neighborhood { radius: 0 },
        }]
        .into();
    }
    l
}
fn samples(r: &mut WgpuRasterizer, layer: &Layer) -> Vec<f32> {
    frame(r, std::slice::from_ref(layer));
    crate::layer_tests::page_bytes(r, r.composite_texture.as_ref().unwrap())
        .chunks_exact(16)
        .map(|p| f32::from_le_bytes(p[..4].try_into().unwrap()))
        .collect()
}

#[test]
fn native_analytic_curve_and_gradient_tables_resolve_every_code_and_narrow_knots() {
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    })
    .unwrap();
    for image in [false, true] {
        for points in [
            vec![[0., 0.], [1., 1.]],
            vec![[0., 0.], [0.213, 0.73], [0.617, 0.19], [1., 1.]],
            vec![[0., 0.], [0.40003, 0.1], [0.40007, 0.9], [1., 1.]],
            (0..32)
                .map(|i| [i as f32 / 31., if i % 2 == 0 { 0.1 } else { 0.9 }])
                .collect(),
        ] {
            let mut probe = table_probe("curves", image, 0);
            set(&mut probe, "curve_0", EffectValue::Curve(points.clone()));
            for (code, actual) in samples(&mut r, &probe).into_iter().enumerate() {
                let x = code as f32 / 65535.;
                let expected = curve_reference(&points, f64::from(x));
                assert!(
                    (f64::from(actual) - expected).abs() <= 1. / 65535.,
                    "curve {points:?}, image={image}, {code}: {actual} vs {expected}"
                );
                assert!(
                    ((f64::from(actual) * 65535.).round() - (expected * 65535.).round()).abs()
                        <= 1.
                );
            }
        }
        let stops: Vec<_> = [0., 0.40003, 0.40007, 1.]
            .into_iter()
            .enumerate()
            .map(|(i, position)| layer_core::GradientStop {
                position,
                color: layer_core::color::RgbColor::new(layer_core::color::RgbSpace::ProPhoto, [0.1, 0.3, 0.7, 0.9].map(|v| if i % 2 == 0 { v } else { 1. - v })).unwrap(),
            })
            .collect();
        for component in 0..4 {
            let mut probe = table_probe("gradient_map", image, component);
            set(&mut probe, "gradient", EffectValue::Gradient(stops.clone()));
            for (code, actual) in samples(&mut r, &probe).into_iter().enumerate() {
                let x = f64::from(code as f32 / 65535.);
                let i = stops
                    .partition_point(|s| f64::from(s.position) < x)
                    .saturating_sub(1)
                    .min(stops.len() - 2);
                let t = (x - f64::from(stops[i].position))
                    / (f64::from(stops[i + 1].position) - f64::from(stops[i].position));
                let expected = f64::from(stops[i].color.rgba[component]) * (1. - t)
                    + f64::from(stops[i + 1].color.rgba[component]) * t;
                assert!(
                    (f64::from(actual) - expected).abs() <= 1. / 65535.,
                    "gradient image={image}, {component}, {code}: {actual} vs {expected}"
                );
                assert!(
                    ((f64::from(actual) * 65535.).round() - (expected * 65535.).round()).abs()
                        <= 1.
                );
            }
        }
    }
}

#[test]
fn native_hue_vibrance_and_balance_preserve_extended_color_with_document_tone_coordinates() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
            space,
            depth: SampleDepth::U16,
        })
        .unwrap();
        for image in [false, true] {
            let encoded = [-0.2, 1.2, 0.4];
            let rgb = encoded.map(|v| space.decode(v) as f32);
            let mut hue = effect(2, "hue_saturation", image);
            set(&mut hue, "hue", EffectValue::Number(180.));
            let expected = [1.2, -0.2, 0.6].map(|v| space.decode(v) as f32);
            close(
                frame(&mut r, &[hue, source(rgb, 1. / 65535.)]),
                expected,
                1. / 65535.,
                "extended hue",
            );
            let mut vib = effect(2, "vibrance", image);
            set(&mut vib, "saturation", EffectValue::Number(-100.));
            close(
                frame(&mut r, &[vib, source(rgb, 0.37)]),
                [space.decode(0.5) as f32; 3],
                0.37,
                "extended desaturation",
            );
            let mut balance = effect(2, "color_balance", image);
            for key in ["shadows_red", "midtones_red", "highlights_red"] {
                set(&mut balance, key, EffectValue::Number(20.));
            }
            let y = space.to_xyz()[1];
            let correction = -y[0] * 0.1;
            let expected = std::array::from_fn(|c| {
                space.decode(encoded[c] + if c == 0 { 0.1 + correction } else { correction }) as f32
            });
            close(
                frame(&mut r, &[balance, source(rgb, 0.37)]),
                expected,
                0.37,
                "extended color balance",
            );
        }
    }
}

#[test]
fn native_profiled_curves_match_integer16_reference_through_fused_and_physical_passes() {
    let points = vec![[0., 0.05], [0.213, 0.31], [0.617, 0.73], [1., 0.95]];
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
            space,
            depth: SampleDepth::U16,
        })
        .unwrap();
        for image in [false, true] {
            let alpha = 1. / 65535.;
            let mut input = source([0.; 3], alpha);
            let p = Arc::make_mut(&mut Arc::make_mut(input.effect.as_mut().unwrap()).program);
            p.wgsl = format!(
                "fn fixture(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{
                let code=u32(p.y)*256u+u32(p.x);let x=f32(code)/65535.;
                return vec4<f32>(sdr_decode(vec3<f32>(x),FX_SPACE)*{alpha:?},{alpha:?});}}"
            )
            .into();
            let mut curves = effect(2, "curves", image);
            set(&mut curves, "curve_0", EffectValue::Curve(points.clone()));
            frame(&mut r, &[curves, input]);
            let bytes = crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap());
            for (code, pixel) in bytes.chunks_exact(16).enumerate() {
                let value = f32::from_le_bytes(pixel[..4].try_into().unwrap());
                let coverage = f32::from_le_bytes(pixel[12..].try_into().unwrap());
                assert_eq!(coverage, alpha);
                let actual = f64::from(value) / f64::from(alpha);
                let expected = curve_reference(&points, f64::from(code as f32 / 65535.));
                assert!(
                    (actual - space.decode(expected)).abs() <= 2e-6,
                    "{space:?} image={image} {code}: {actual} vs {}",
                    space.decode(expected)
                );
                let encoded = space.encode(actual);
                assert!(
                    ((encoded * 65535.).round() - (expected * 65535.).round()).abs() <= 1.,
                    "encoded {space:?} image={image} {code}: {encoded} vs {expected}"
                );
            }
        }
    }
}

#[test]
fn hdr_linear_curves_and_exposure_retain_range_across_physical_passes() {
    let points=vec![[0.,0.05],[0.25,0.2],[0.75,0.85],[1.,0.95]];
    for space in RgbSpace::ALL {
        let mut r=WgpuRasterizer::new_native_headless(DocumentColor{space,depth:SampleDepth::F16}).unwrap();
        for image in [false,true] {
            for alpha in [0.,1./16777216.,0.5,1.] {
                let rgb=[-0.25,4.,32768.];
                let mut curve=effect(2,"curves",image);
                set(&mut curve,"domain",EffectValue::Choice(1));set(&mut curve,"hdr_stops",EffectValue::Number(4.));
                set(&mut curve,"curve_0",EffectValue::Curve(points.clone()));
                let mut exposure=effect(3,"exposure",image);set(&mut exposure,"exposure",EffectValue::Number(1.));
                let actual=frame(&mut r,&[exposure,curve,source(rgb,alpha)]);
                assert_eq!(actual[3],alpha);
                for c in 0..3 {let expected=if alpha==0.{0.}else{curve_reference(&points,f64::from(rgb[c])/16.)*32.};
                    let actual=if alpha==0.{actual[c]}else{actual[c]/alpha} as f64;
                    assert!((actual-expected).abs()<=2e-6+expected.abs()*2e-5,"HDR curve {space:?}, physical={image}, alpha={alpha}: {actual} != {expected}");
                }
            }
        }
    }
}

#[test]
fn final_effect_covers_partial_document_tiles_without_overwriting_neighbors() {
    for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16] {
        let mut r=WgpuRasterizer::new_native_headless(DocumentColor{space:RgbSpace::Srgb,depth}).unwrap();
        let layers=[effect(2,"exposure",false),source([0.25,2.,-0.125],1.)];
        for extent in [[512,384],[513,385],[385,513]] {
            r.submit(FramePacket{view:ViewState{width_px:extent[0],height_px:extent[1],background_rgba_linear:[0.;4],..test_view()},document_extent:extent,layers:&layers,dabs:&[],dab_batches:&[],restore_rasters:&[],reset_layers:true,composite_all:true,time_seconds:0.}).unwrap();
            let pixels=crate::layer_tests::page_bytes(&r,r.composite_texture.as_ref().unwrap());
            for (i,p) in pixels.chunks_exact(16).enumerate() {
                let actual: [f32;4]=std::array::from_fn(|c| f32::from_le_bytes(p[c*4..c*4+4].try_into().unwrap()));
                assert_eq!(actual,[0.25,2.,-0.125,1.],"{depth:?} {extent:?} {},{}",i%extent[0] as usize,i/extent[0] as usize);
            }
        }
    }
}
