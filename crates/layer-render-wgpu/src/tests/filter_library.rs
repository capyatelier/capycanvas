//! Catalog-wide filter rendering, animation and performance gates.
use super::*;
use layer_core::{EffectAlpha, EffectValue};
const EXTENT: [u32; 2] = [384, 256];

#[path = "filter_investigation.rs"]
mod investigation;
#[path = "../../../../tools/performance/filter-history-benchmark.rs"]
mod historical;

fn artwork([width, height]: [u32; 2]) -> Vec<u8> {
    // Original test artwork: gradients, curved silhouettes, bright highlights,
    // fine texture and transparent edges. No external image/licensing inputs.
    (0..width * height)
        .flat_map(|i| {
            let x = i % width;
            let y = i / width;
            let u = x as f32 / width as f32;
            let v = y as f32 / height as f32;
            let mut color = [
                0.15 + 0.5 * u,
                0.25 + 0.45 * (1. - v),
                0.65 + 0.2 * (1. - u),
            ];
            if (u - 0.72).hypot(v - 0.22) < 0.095 {
                color = [1., 0.94, 0.67];
            }
            let ridge = 0.52 + 0.08 * (u * 11.).sin() + 0.04 * (u * 29.).cos();
            if v > ridge {
                color = [0.12 + 0.15 * u, 0.30 + 0.3 * (1. - v), 0.22 + 0.15 * u];
            }
            if v > 0.76 + 0.05 * (u * 8.).sin() {
                color = [0.4 + 0.25 * u, 0.20 + 0.1 * v, 0.09];
            }
            if (u - 0.25).abs() < 0.09 && (0.44..0.78).contains(&v) {
                color = [0.82, 0.23, 0.14];
            }
            if (x / 5 + y / 7) % 13 == 0 {
                for c in &mut color {
                    *c = (*c * 0.75 + 0.1).min(1.);
                }
            }
            let noise =
                ((x.wrapping_mul(1664525) ^ y.wrapping_mul(1013904223)) & 31) as f32 / 255. - 0.06;
            let alpha = if x < 8 || y < 8 || x + 8 >= width || y + 8 >= height {
                0
            } else {
                255
            };
            [
                (255. * (color[0] + noise).clamp(0., 1.)) as u8,
                (255. * (color[1] + noise).clamp(0., 1.)) as u8,
                (255. * (color[2] + noise).clamp(0., 1.)) as u8,
                alpha,
            ]
        })
        .collect()
}
fn setup(extent: [u32; 2]) -> Layer {
    let bytes = artwork(extent);
    let mut layer = Layer::paint(LayerId(1), "Artwork");
    layer.source = Some(layer_core::color::source::rgba8_source(extent, |x, y| {
        let i = (y * extent[0] + x) as usize * 4;
        bytes[i..i + 4].try_into().unwrap()
    }));
    layer
}
fn filter(id: &layer_core::EffectDefinition) -> Layer {
    let mut layer = Layer::paint(LayerId(2), id.label());
    layer.kind = LayerKind::Effect;
    layer.effect = Some(Arc::new(id.preview().unwrap()));
    layer
}
fn submit(
    r: &mut WgpuRasterizer,
    extent: [u32; 2],
    layers: &[Layer],
    time: f32,
    reset: bool,
    all: bool,
    paint: Option<([f32; 2], f32)>,
) {
    let mut dabs = Vec::new();
    let mut batches = Vec::new();
    if let Some((center, radius)) = paint {
        let mut dab = test_dab(center, [0.75, 0.06, 0.8, 1.], 1.);
        dab.radii = [radius; 2];
        dabs.push(dab);
        batches.push(DabBatch {
            stroke_id: StrokeId(77),
            ..crate::test_support::dab_batch(
                LayerId(1),
                test_style(BrushExecution::Dry),
                Rect { min: Point { x: center[0] - radius - 1., y: center[1] - radius - 1. }, max: Point { x: center[0] + radius + 1., y: center[1] + radius + 1. } },
            )
        });
    }
    let view = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        background_rgba_linear: [0.; 4],
        ..test_view()
    };
    r.submit(FramePacket {
        view,
        time_seconds: time,
        dabs: &dabs,
        dab_batches: &batches,
        reset_layers: reset,
        composite_all: all,
        ..packet(layers, extent)
    })
    .unwrap();
}
fn image(r: &mut WgpuRasterizer) -> Vec<u8> {
    r.readback_srgb_rgba8().unwrap()
}

#[test]
fn runtime_manifest_loads_a_new_filter_and_its_preparation() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/filters/tent-blur");
    let manifest = std::fs::read_to_string(directory.join("manifest.json")).unwrap();
    let catalog = layer_core::EffectPackage::parse(&manifest)
        .unwrap()
        .resolve(|name| {
            std::fs::read_to_string(directory.join(name))
                .map(Arc::from)
                .map_err(|e| e.to_string())
        })
        .unwrap();
    let definition = catalog.get("example:tent_blur").unwrap();
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let base = setup(EXTENT);
    submit(
        &mut r,
        EXTENT,
        std::slice::from_ref(&base),
        0.,
        true,
        true,
        None,
    );
    let original = image(&mut r);
    let mut layer = Layer::paint(LayerId(2), definition.label());
    layer.kind = LayerKind::Effect;
    layer.effect = Some(Arc::new(definition.preview().unwrap()));
    let mut layers = vec![layer, base];
    submit(&mut r, EXTENT, &layers, 0., false, true, None);
    assert_ne!(image(&mut r), original);
    assert_eq!(r.scene.as_ref().unwrap().effects.preparation_count(), 1);
    Arc::make_mut(layers[0].effect.as_mut().unwrap())
        .set("radius", EffectValue::Number(0.))
        .unwrap();
    submit(&mut r, EXTENT, &layers, 0., false, true, None);
    assert!(
        image(&mut r)
            .iter()
            .zip(original)
            .all(|(a, b)| a.abs_diff(b) <= 1)
    );
    assert_eq!(r.scene.as_ref().unwrap().effects.preparation_count(), 2);
}

#[test]
fn entire_filter_catalog_renders_masks_freezes_and_animates() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).expect("physical GPU required");
    let base = setup(EXTENT);
    submit(
        &mut r,
        EXTENT,
        std::slice::from_ref(&base),
        0.,
        true,
        true,
        None,
    );
    let original = image(&mut r);
    for id in fixtures() {
        let mut layers = vec![filter(id), base.clone()];
        submit(&mut r, EXTENT, &layers, 0., false, true, None);
        let output = image(&mut r);
        assert_ne!(
            output,
            original,
            "{} preview must demonstrate its effect",
            id.label()
        );
        assert!(
            output.chunks_exact(4).any(|p| p[3] > 0),
            "{} must not erase the image",
            id.label()
        );
        if layers[0].effect.as_ref().unwrap().program.alpha == EffectAlpha::Preserve {
            assert!(
                output
                    .chunks_exact(4)
                    .zip(original.chunks_exact(4))
                    .all(|(a, b)| a[3] == b[3]),
                "{} preserves alpha",
                id.label()
            );
        }
        let before = r.scene.as_ref().map_or([0, 0], |s| s.image_work());
        submit(&mut r, EXTENT, &layers, 20., false, false, None);
        assert_eq!(image(&mut r), output, "{} frozen result", id.label());
        assert_eq!(
            r.scene.as_ref().map_or([0, 0], |s| s.image_work()),
            before,
            "{} frozen frame does no image work",
            id.label()
        );
        layers[0].opacity = 0.;
        submit(&mut r, EXTENT, &layers, 20., false, true, None);
        assert_eq!(
            image(&mut r),
            original,
            "{} zero opacity is identity",
            id.label()
        );
        layers[0].opacity = 1.;
        layers[0].mask = Some(layer_core::LayerMask::reveal_all(
            LayerId(100),
            Point::default(),
        ));
        layers[0].mask.as_mut().unwrap().default_coverage = 0.;
        submit(&mut r, EXTENT, &layers, 20., false, true, None);
        assert_eq!(
            image(&mut r),
            original,
            "{} zero mask is identity",
            id.label()
        );
        layers[0].mask = None;
        layers[0].properties.clipped = true;
        submit(&mut r, EXTENT, &layers, 20., false, true, None);
        assert!(
            image(&mut r)
                .chunks_exact(4)
                .zip(original.chunks_exact(4))
                .all(|(a, b)| a[3] == b[3]),
            "{} clipping preserves base coverage",
            id.label()
        );
        if layers[0].effect.as_ref().unwrap().program.time {
            Arc::make_mut(layers[0].effect.as_mut().unwrap())
                .set("animate", EffectValue::Toggle(true))
                .unwrap();
            submit(&mut r, EXTENT, &layers, 0., false, true, None);
            let first = image(&mut r);
            submit(&mut r, EXTENT, &layers, 1., false, false, None);
            let second = image(&mut r);
            assert_ne!(first, second, "{} animation must change pixels", id.label());
        }
    }
}
