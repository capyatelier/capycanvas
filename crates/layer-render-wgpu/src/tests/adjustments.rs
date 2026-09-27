use super::*;
use layer_core::{EffectInstance, LayerMask};

// Keep the established ten-filter baseline stable as the catalog grows.
fn pointwise_baseline() -> [&'static layer_core::EffectDefinition; 10] {
    [
        fixture("curves"),
        fixture("levels"),
        fixture("brightness_contrast"),
        fixture("hue_saturation"),
        fixture("color_balance"),
        fixture("exposure"),
        fixture("vibrance"),
        fixture("black_white"),
        fixture("gradient_map"),
        fixture("posterize"),
    ]
}

fn effect(id: u64, kind: &layer_core::EffectDefinition) -> Layer {
    let mut l = Layer::paint(LayerId(id), kind.label());
    l.kind = LayerKind::Effect;
    l.effect = Some(Arc::new(EffectInstance::new(kind.program())));
    l
}

#[test]
fn all_effects_incremental_masks_groups_and_clipping_match_full_recomposition() {
    let mut r =
        WgpuRasterizer::new_native_headless(Default::default()).expect("physical GPU required");
    let mut base = Layer::paint(LayerId(1), "Translucent paint");
    let mut group = Layer::paint(LayerId(20), "Isolated group");
    group.kind = LayerKind::Group;
    base.properties.parent = Some(group.id);
    let mut layers = vec![group];
    for (i, kind) in pointwise_baseline().into_iter().enumerate() {
        let mut fx = effect(i as u64 + 2, kind);
        fx.properties.parent = Some(LayerId(20));
        fx.properties.clipped = true;
        fx.opacity = 0.7;
        if i % 2 == 0 {
            let mut mask = LayerMask::reveal_all(LayerId(40 + i as u64), Point::default());
            mask.default_coverage = 0.4;
            fx.mask = Some(mask);
        }
        layers.push(fx);
    }
    layers.push(base);
    let mut view = test_view();
    view.width_px = 333;
    view.height_px = 291;
    view.background_rgba_linear = [0.; 4];
    let mut dab = test_dab([255., 150.], [0.8, 0.2, 0.1, 0.65], 1.);
    dab.radii = [45.; 2];
    let batch = DabBatch {
        material_update: 0,
        stroke_id: StrokeId(1),
        layer_id: LayerId(1),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style: test_style(BrushExecution::Dry),
        damage: Rect {
            min: Point { x: 209., y: 104. },
            max: Point { x: 301., y: 196. },
        },
    };
    let render = |r: &mut WgpuRasterizer, all, dabs: &[Dab], batches: &[DabBatch]| {
        r.submit(FramePacket {
            view,
            document_extent: [333, 291],
            layers: &layers,
            dabs,
            dab_batches: batches,
            restore_rasters: &[],
            reset_layers: false,
            time_seconds: 0.,
            composite_all: all,
        })
        .unwrap();
        let mut bytes = vec![0; 333 * 291 * 4];
        r.copy_rgba8_srgb(&mut bytes, 333 * 4).unwrap();
        bytes
    };
    render(&mut r, true, &[], &[]);
    let incremental = render(&mut r, false, &[dab], &[batch]);
    let full = render(&mut r, true, &[], &[]);
    assert_eq!(
        incremental, full,
        "damage crossing a tile boundary must match full composite"
    );
    let p = &full[(150 * 333 + 255) * 4..][..4];
    assert!(
        (160..=170).contains(&p[3]),
        "adjustments and masks preserve base alpha: {p:?}"
    );
    assert_eq!(
        &full[..4],
        &[0; 4],
        "clipped adjustments cannot create coverage"
    );
}
