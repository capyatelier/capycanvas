use super::*;
use layer_core::color::{DocumentColor, IntegerDepth, RgbSpace};
use layer_core::{EffectInstance, EffectKind, EffectPass, EffectSampling, EffectValue};

fn effect(id: u64, name: &str, image: bool) -> Layer {
    let mut program = (*fixture(name).program()).clone();
    if image {
        program.passes = vec![EffectPass {
            entry: program.entry.clone(),
            sampling: EffectSampling::Neighborhood { radius: 0 },
        }]
        .into();
    }
    let mut layer = Layer::paint(LayerId(id), name);
    layer.kind = LayerKind::Effect;
    layer.effect = Some(Arc::new(EffectInstance::new(Arc::new(program))));
    layer
}
fn set(layer: &mut Layer, name: &str, value: EffectValue) {
    Arc::make_mut(layer.effect.as_mut().unwrap())
        .set(name, value)
        .unwrap();
}
fn source(rgb: [f32; 3], alpha: f32) -> Layer {
    let mut layer = effect(1, "exposure", false);
    let p = Arc::make_mut(&mut Arc::make_mut(layer.effect.as_mut().unwrap()).program);
    p.kind = EffectKind::Generator;
    p.entry = "fixture".into();
    p.wgsl = format!("fn fixture(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{return vec4<f32>({:?},{:?},{:?},{:?});}}",
        rgb[0]*alpha, rgb[1]*alpha, rgb[2]*alpha, alpha).into();
    layer
}
fn frame(r: &mut WgpuRasterizer, layers: &[Layer]) -> [f32; 4] {
    r.submit(FramePacket {
        view: ViewState {
            width_px: 256,
            height_px: 256,
            background_rgba_linear: [0.; 4],
            ..test_view()
        },
        document_extent: [256; 2],
        layers,
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: false,
        composite_all: true,
        time_seconds: 0.,
    })
    .unwrap();
    let bytes = crate::layer_tests::page_bytes(r, r.composite_texture.as_ref().unwrap());
    std::array::from_fn(|c| f32::from_le_bytes(bytes[c * 4..c * 4 + 4].try_into().unwrap()))
}
fn close(actual: [f32; 4], rgb: [f32; 3], alpha: f32, context: &str) {
    assert_eq!(actual[3], alpha, "coverage {context}");
    for c in 0..3 {
        let straight = if alpha == 0. {
            actual[c]
        } else {
            actual[c] / alpha
        };
        let expected = if alpha == 0. { 0. } else { rgb[c] };
        assert!(
            (straight - expected).abs() <= 2e-6,
            "{context}: channel {c}: {straight} vs {expected}"
        );
    }
}

#[test]
fn native_exposure_retains_extended_low_alpha_through_fused_and_physical_chains() {
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let mut r =
                WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            for image in [false, true] {
                for alpha in [0., 0.00000008, 1. / 65535., 0.5, 1.] {
                    let rgb = [-0.125, 0.234567, 1.5];
                    let mut layers = vec![source(rgb, alpha)];
                    for i in 0..24 {
                        let mut ev = effect(2 + i, "exposure", image);
                        set(
                            &mut ev,
                            "exposure",
                            EffectValue::Number(if i % 2 == 0 { 5. } else { -5. }),
                        );
                        layers.insert(0, ev);
                    }
                    let out = frame(&mut r, &layers);
                    close(
                        out,
                        rgb,
                        alpha,
                        &format!("{space:?} {depth:?} image={image}"),
                    );
                    // A slider evaluates the retained input, and restoring it
                    // recovers the same result without baking prior outputs.
                    set(&mut layers[0], "exposure", EffectValue::Number(-4.));
                    close(
                        frame(&mut r, &layers),
                        rgb.map(|v| v * 2.),
                        alpha,
                        &format!("slider {space:?} {depth:?} image={image} alpha={alpha}"),
                    );
                    set(&mut layers[0], "exposure", EffectValue::Number(-5.));
                    assert_eq!(frame(&mut r, &layers), out);
                }
            }
        }
    }
}

#[test]
fn native_white_balance_and_encoded_tone_helpers_use_document_primaries_and_transfer() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
            space,
            depth: IntegerDepth::U16,
        })
        .unwrap();
        for image in [false, true] {
            let alpha = 0.00000008;
            let rgb = [0.75, 0.2, 0.05];
            let mut wb = effect(2, "white_balance", image);
            set(&mut wb, "temperature", EffectValue::Number(50.));
            set(&mut wb, "tint", EffectValue::Number(-30.));
            set(&mut wb, "preserve_luminance", EffectValue::Toggle(false));
            let mut layers = vec![wb, source(rgb, alpha)];
            let adjusted = [
                0.5f64 * 0.8 - 0.3 * 0.25,
                0.3 * 0.5,
                -0.5 * 0.8 - 0.3 * 0.25,
            ];
            let raw: [f64; 3] = std::array::from_fn(|c| f64::from(rgb[c]) * adjusted[c].exp2());
            close(
                frame(&mut r, &layers),
                raw.map(|v| v as f32),
                alpha,
                "white balance gains",
            );
            set(
                &mut layers[0],
                "preserve_luminance",
                EffectValue::Toggle(true),
            );
            let w = space.to_xyz()[1];
            let before: f64 = (0..3).map(|c| w[c] * f64::from(rgb[c])).sum();
            let after: f64 = (0..3).map(|c| w[c] * raw[c]).sum();
            close(
                frame(&mut r, &layers),
                raw.map(|v| (v * before / after) as f32),
                alpha,
                "profile luminance",
            );
            // Brightness is deliberately encoded-domain; its +10 control adds
            // 0.1 in this document's transfer curve, without an SDR range clamp.
            layers[0] = effect(2, "brightness_contrast", image);
            set(&mut layers[0], "brightness", EffectValue::Number(10.));
            let expected = rgb.map(|v| space.decode(space.encode(f64::from(v)) + 0.1) as f32);
            close(
                frame(&mut r, &layers),
                expected,
                alpha,
                "profile tone curve",
            );
        }
    }
}

#[test]
fn native_photo_adjustments_and_masks_remain_editable_after_save_reopen() {
    use layer_core::color::{ColorProfile, source::*};
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let color = DocumentColor { space, depth };
            let mut document = layer_core::Document::new("editable photo", 256, 256);
            document.color = color;
            let mut builder = SourceBuilder::new(
                [256; 2],
                SourceInterpretation {
                    channels: SourceChannels::Rgba,
                    depth,
                    profile: ColorProfile::Builtin(space),
                    profile_assumed: false,
                },
                4 * 1024 * 1024,
            )
            .unwrap();
            for y in 0..256u32 {
                let bytes: Vec<_> = (0..256u32)
                    .flat_map(|x| {
                        [x * 257, y * 257, (x * 101 + y * 237) % 65536, 65535]
                            .into_iter()
                            .flat_map(move |v| {
                                let value = if depth == IntegerDepth::U8 {
                                    v / 257
                                } else {
                                    v
                                };
                                (value as u16).to_le_bytes().into_iter().take(depth.bytes())
                            })
                    })
                    .collect();
                builder.push_row(&bytes).unwrap();
            }
            let source_id = document.allocate_layer_id();
            let mut photo = Layer::paint(source_id, "retained original");
            photo.source = Some(Arc::new(builder.finish().unwrap()));
            document.active_layer = source_id;
            document.layers = vec![photo];
            for name in [
                "exposure",
                "white_balance",
                "levels",
                "curves",
                "hue_saturation",
                "color_balance",
            ] {
                let mut layer = effect(document.allocate_layer_id().0, name, false);
                if name == "exposure" {
                    set(&mut layer, "exposure", EffectValue::Number(0.75));
                }
                if name == "white_balance" {
                    set(&mut layer, "temperature", EffectValue::Number(25.));
                }
                if name == "hue_saturation" {
                    set(&mut layer, "hue", EffectValue::Number(10.));
                }
                let mut mask = layer_core::LayerMask::reveal_all(
                    document.allocate_layer_id(),
                    Point::default(),
                );
                mask.default_coverage = 0.5;
                layer.mask = Some(mask);
                document.layers.insert(0, layer);
            }
            let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
            frame(&mut r, &document.layers);
            let before = crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap());
            let project = layer_core::Project {
                document,
                assets: Default::default(),
            };
            let mut archive = Vec::new();
            project.write(&mut archive).unwrap();
            let mut loaded =
                layer_core::Project::read(archive.as_slice(), Default::default()).unwrap();
            assert_eq!(loaded.document.color, color);
            assert!(
                loaded.document.layers[..6]
                    .iter()
                    .all(|l| l.effect.is_some() && l.mask.is_some())
            );
            let mut fresh = WgpuRasterizer::new_native_headless(color).unwrap();
            frame(&mut fresh, &loaded.document.layers);
            assert_eq!(
                crate::layer_tests::page_bytes(&fresh, fresh.composite_texture.as_ref().unwrap()),
                before
            );
            let exposure = loaded
                .document
                .layers
                .iter()
                .position(|l| l.name.as_ref() == "exposure")
                .unwrap();
            set(
                &mut loaded.document.layers[exposure],
                "exposure",
                EffectValue::Number(-1.),
            );
            frame(&mut fresh, &loaded.document.layers);
            assert_ne!(
                crate::layer_tests::page_bytes(&fresh, fresh.composite_texture.as_ref().unwrap()),
                before
            );
            set(
                &mut loaded.document.layers[exposure],
                "exposure",
                EffectValue::Number(0.75),
            );
            frame(&mut fresh, &loaded.document.layers);
            assert_eq!(
                crate::layer_tests::page_bytes(&fresh, fresh.composite_texture.as_ref().unwrap()),
                before
            );
            let source = loaded
                .document
                .layers
                .last()
                .unwrap()
                .source
                .as_ref()
                .unwrap();
            assert_eq!(source.interpretation.profile, ColorProfile::Builtin(space));
            assert_eq!(source.interpretation.depth, depth);
            assert!(loaded.document.layers.last().unwrap().raster.is_empty());
        }
    }
}
