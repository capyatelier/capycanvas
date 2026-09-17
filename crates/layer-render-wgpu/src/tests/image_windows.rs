//! Cropped captures must be the same document, including halos and clipping.
use super::*;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
use layer_core::{EffectInstance, EffectKind, EffectPass, EffectSampling, LayerMask, Selection};

pub(crate) fn effect(id: u64, generator: bool, global: bool) -> Layer {
    let mut p = (*fixture("exposure").program()).clone();
    p.kind = if generator {
        EffectKind::Generator
    } else {
        EffectKind::Adjustment
    };
    if generator {
        p.entry = "pattern".into();
        p.wgsl = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{let a=select(.31,.73,(u32(p.x)/13u+u32(p.y)/11u)%2u==0u);return vec4<f32>(vec3<f32>(fract(p.x/37.),fract(p.y/29.),.27)*a,a);}".into();
        p.passes = Arc::from([]);
    } else {
        p.entry = "horizontal".into();
        p.wgsl = if global {
            "fn horizontal(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return fx_sample(fx_extent()-p);}".into()
        } else {
            "fn horizontal(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p-vec2<f32>(3.,0.))+c+fx_sample(p+vec2<f32>(3.,0.)))/3.;} fn vertical(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p-vec2<f32>(0.,2.))+c+fx_sample(p+vec2<f32>(0.,2.)))/3.;}".into()
        };
        p.passes = if global {
            vec![EffectPass {
                entry: "horizontal".into(),
                sampling: EffectSampling::Document,
            }]
        } else {
            vec![
                EffectPass {
                    entry: "horizontal".into(),
                    sampling: EffectSampling::Neighborhood { radius: 3 },
                },
                EffectPass {
                    entry: "vertical".into(),
                    sampling: EffectSampling::Neighborhood { radius: 2 },
                },
            ]
        }
        .into();
    }
    let mut layer = Layer::paint(LayerId(id), "window fixture");
    layer.kind = LayerKind::Effect;
    layer.effect = Some(Arc::new(EffectInstance::new(Arc::new(p))));
    layer
}

fn capture(
    r: &mut WgpuRasterizer,
    scene: &mut scene::Scene,
    packet: FramePacket<'_>,
    crop: PixelRect,
) -> Vec<u8> {
    let (target, _) =
        create_color_target(&r.device, [crop.width(), crop.height()], "window oracle");
    let mut encoder = crate::submission::CommandEncoder::new(
        &r.device,
        &wgpu::CommandEncoderDescriptor::default(),
    );
    scene
        .capture_region(r, packet, &target, crop, None, &mut encoder)
        .unwrap();
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    crate::layer_tests::page_bytes(r, &target)
}

#[test]
fn image_windows_match_full_composition_with_halos_masks_and_clipping() {
    let extent = [777, 533];
    for space in [
        None,
        Some(RgbSpace::Srgb),
        Some(RgbSpace::DisplayP3),
        Some(RgbSpace::AdobeRgb),
        Some(RgbSpace::ProPhoto),
    ] {
        let mut r = match space {
            None => WgpuRasterizer::new_headless().unwrap(),
            Some(space) => WgpuRasterizer::new_native_headless(DocumentColor {
                space,
                depth: SampleDepth::U16,
            })
            .unwrap(),
        };
        for clipped in [false, true] {
            let mut group = Layer::paint(LayerId(10), "isolated");
            group.kind = LayerKind::Group;
            group.opacity = 0.79;
            let mut first = effect(2, false, false);
            first.opacity = 0.63;
            first.properties.clipped = clipped;
            let mut second = first.clone();
            second.id = LayerId(3);
            let mut mask = LayerMask::reveal_all(LayerId(20), Point { x: 7., y: -9. });
            mask.default_coverage = 0.;
            mask.initial = Some(
                Selection::polygon(vec![
                    Point { x: 0., y: 0. },
                    Point { x: 760., y: 99. },
                    Point { x: 440., y: 533. },
                ])
                .unwrap(),
            );
            first.mask = Some(mask);
            let mut layers = vec![
                group,
                first,
                second,
                effect(1, true, false),
                effect(4, true, false),
            ];
            for l in &mut layers[1..4] {
                l.properties.parent = Some(LayerId(10));
            }
            let packet = FramePacket {
                document_extent: extent,
                layers: &layers,
                view: ViewState {
                    width_px: extent[0],
                    height_px: extent[1],
                    background_rgba_linear: [0.; 4],
                    ..test_view()
                },
                time_seconds: 0.,
                dabs: &[],
                dab_batches: &[],
                restore_rasters: &[],
                reset_layers: false,
                composite_all: true,
            };
            r.submit(packet).unwrap(); // Initializes real mask pages and renderer metadata.
            let mut scene = scene::Scene::new(&r);
            let full = capture(&mut r, &mut scene, packet, PixelRect::full(extent));
            let full_cache = scene.image_cache_bytes();
            for crop in [
                PixelRect::new(249, 251, 279, 281),
                PixelRect::new(17, 33, 49, 89),
                PixelRect::new(752, 511, 777, 533),
                PixelRect::new(0, 0, 23, 27),
            ] {
                let pixels = capture(&mut r, &mut scene, packet, crop);
                let bytes = if space.is_some() { 16 } else { 4 };
                for y in 0..crop.height() as usize {
                    for x in 0..crop.width() as usize {
                        let src = ((y + crop.min_y() as usize) * extent[0] as usize
                            + x
                            + crop.min_x() as usize)
                            * bytes;
                        let dst = (y * crop.width() as usize + x) * bytes;
                        for c in 0..4 {
                            let (a, b, tolerance) = if space.is_some() {
                                (
                                    f32::from_le_bytes(
                                        pixels[dst + c * 4..dst + c * 4 + 4].try_into().unwrap(),
                                    ),
                                    f32::from_le_bytes(
                                        full[src + c * 4..src + c * 4 + 4].try_into().unwrap(),
                                    ),
                                    2e-6,
                                )
                            } else {
                                (pixels[dst + c] as f32, full[src + c] as f32, 1.)
                            };
                            assert!(
                                (a - b).abs() <= tolerance,
                                "{space:?} clipped={clipped} crop={crop:?} ({x},{y}) c={c}: {a} != {b}"
                            );
                        }
                    }
                }
                assert!(
                    scene.image_cache_bytes() < full_cache / 20,
                    "cropped boundaries must allocate only their window"
                );
            }
            // Reusing the same Scene must restore full bounds, without stale pixels.
            assert_eq!(
                capture(&mut r, &mut scene, packet, PixelRect::full(extent)),
                full
            );
        }
    }
}

#[test]
fn image_windows_keep_document_sampler_dependencies_complete() {
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    })
    .unwrap();
    let extent = [333, 291];
    let layers = [
        effect(3, false, true),
        effect(2, false, false),
        effect(1, true, false),
    ];
    let packet = FramePacket {
        document_extent: extent,
        layers: &layers,
        view: test_view(),
        time_seconds: 0.,
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: false,
        composite_all: true,
    };
    r.submit(packet).unwrap();
    let mut scene = scene::Scene::new(&r);
    let full = capture(&mut r, &mut scene, packet, PixelRect::full(extent));
    let cache = scene.image_cache_bytes();
    let crop = PixelRect::new(271, 3, 295, 19);
    let pixels = capture(&mut r, &mut scene, packet, crop);
    assert_eq!(
        scene.image_cache_bytes(),
        cache,
        "global remapping must retain the declared full input"
    );
    for y in 0..crop.height() as usize {
        let a = y * crop.width() as usize * 16;
        let b = ((y + crop.min_y() as usize) * extent[0] as usize + crop.min_x() as usize) * 16;
        assert_eq!(
            &pixels[a..a + crop.width() as usize * 16],
            &full[b..b + crop.width() as usize * 16]
        );
    }
}
