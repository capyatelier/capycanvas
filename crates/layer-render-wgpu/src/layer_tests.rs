//! Pixel assertions and bounded GPU-completion timings for layer composition.
use super::*;
use layer_core::{
    BrushDeform, BrushRendering, BrushWetMix, LayerMask, LayerOperation, LayerOperationKind, Point,
    Rect, Selection, StrokeId,
};
use layer_render::{DabStyle, ViewState};

fn view() -> ViewState {
    ViewState {
        width_px: 128,
        height_px: 128,
        document_to_surface: [1., 0., 0., 1., 0., 0.],
        background_rgba_linear: [0.; 4],
    }
}
fn dab(color: [f32; 4]) -> Dab {
    Dab {
        center: Point { x: 64., y: 64. },
        radii: [60., 60.],
        rotation: [1., 0.],
        motion: [0.; 2],
        color_rgba_linear: color,
        flow: 1.,
        hardness: 1.,
        texture_sign: [1.; 2],
        material: [0.; 4],
    }
}
fn batch(id: u64) -> DabBatch {
    DabBatch {
        stroke_id: StrokeId(1),
        layer_id: LayerId(id),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style: DabStyle {
            alpha_locked: false,
            tip: BrushTip::AnalyticEllipse,
            mode: DabMode::Paint,
            execution: BrushExecution::Dry,
            grain: None,
            dual: None,
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix::default(),
            transport: None,
            deform: BrushDeform::default(),
        },
        damage: Rect {
            min: Point { x: 0., y: 0. },
            max: Point { x: 128., y: 128. },
        },
    }
}
fn submit(
    r: &mut WgpuRasterizer,
    layers: &[Layer],
    dabs: &[Dab],
    batches: &[DabBatch],
    reset: bool,
) {
    r.submit(FramePacket {
        view: view(),
        document_extent: [128, 128],
        layers,
        dabs,
        dab_batches: batches,
        reset_layers: reset,
        time_seconds: 0.,
        composite_all: true,
    })
    .unwrap();
}
fn pixel(r: &mut WgpuRasterizer, x: usize, y: usize) -> [u8; 4] {
    r.readback_srgb_rgba8().unwrap()[(y * 128 + x) * 4..][..4]
        .try_into()
        .unwrap()
}
fn left_mask(id: u64) -> LayerMask {
    let mut m = LayerMask::reveal_all(LayerId(id), Point::default());
    m.default_coverage = 0.;
    m.initial = Some(
        Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 64., y: 0. },
            Point { x: 64., y: 128. },
            Point { x: 0., y: 128. },
        ])
        .unwrap(),
    );
    m
}

#[test]
fn clipping_stack_keeps_soft_base_alpha_and_group_opacity_once() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut base = Layer::paint(LayerId(1), "base");
    let mut a = Layer::paint(LayerId(2), "clip a");
    a.properties.clipped = true;
    let mut b = Layer::paint(LayerId(3), "clip b");
    b.properties.clipped = true;
    let mut batches = vec![batch(1), batch(2), batch(3)];
    for (i, b) in batches.iter_mut().enumerate() {
        b.first_dab = i as u32;
    }
    let dabs = [
        dab([1., 0., 0., 0.4]),
        dab([0., 1., 0., 1.]),
        dab([0., 0., 1., 1.]),
    ];
    let mut layers = vec![b.clone(), a.clone(), base.clone()];
    submit(&mut r, &layers, &dabs, &batches, true);
    assert_eq!(pixel(&mut r, 64, 64), [0, 0, 255, 102]); // export is straight-alpha sRGB
    let mut group = Layer::paint(LayerId(4), "group");
    group.kind = LayerKind::Group;
    group.opacity = 0.5;
    for l in [&mut base, &mut a, &mut b] {
        l.properties.parent = Some(group.id);
    }
    layers = vec![group, b, a, base];
    submit(&mut r, &layers, &[], &[], false);
    let p = pixel(&mut r, 64, 64);
    assert!((p[3] as i32 - 51).abs() <= 1, "{p:?}");
    layers[0].visible = false;
    submit(&mut r, &layers, &[], &[], false);
    assert_eq!(pixel(&mut r, 64, 64), [0; 4]);
}

#[test]
fn apply_mask_preserves_pixels_and_does_not_remain_a_live_mask() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let before = r.readback_srgb_rgba8().unwrap();
    let mut mask = l.mask.take().unwrap();
    mask.show_area = false;
    l.operations.push(LayerOperation {
        after_stroke: 1,
        coverage: mask,
        kind: LayerOperationKind::ApplyMask,
    });
    let mut op = batch(1);
    op.dab_count = 0;
    op.kind = DabBatchKind::LayerOperation(0);
    submit(&mut r, &[l.clone()], &[], &[op], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), before);
    submit(&mut r, &[l], &[dab([0., 1., 0., 1.])], &[batch(1)], false);
    assert_eq!(
        pixel(&mut r, 90, 64),
        [0, 255, 0, 255],
        "new paint can extend the baked silhouette"
    );
}

#[test]
fn inspection_is_not_exported_and_translated_mask_keeps_source() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let before = r.readback_srgb_rgba8().unwrap();
    l.mask.as_mut().unwrap().show_area = true;
    submit(&mut r, &[l.clone()], &[], &[], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), before);
    l.mask.as_mut().unwrap().offset.x = 40.;
    submit(&mut r, &[l], &[], &[], false);
    assert_eq!(pixel(&mut r, 20, 64), [0; 4]);
    assert_eq!(pixel(&mut r, 80, 64), [255, 0, 0, 255]);
}

#[test]
fn imported_texture_is_linearized_premultiplied_and_masked_on_gpu() {
    let mut r = WgpuRasterizer::new().unwrap();
    let id = AssetId::from("test:image");
    let bytes = [128, 0, 255, 128].repeat(128 * 128);
    r.prepare_asset(
        &id,
        HostImage {
            width: 128,
            height: 128,
            stride: 512,
            format: PixelFormat::Rgba8Srgb,
            bytes: &bytes,
        },
    )
    .unwrap();
    let mut l = Layer::paint(LayerId(1), "texture");
    l.asset = Some(id);
    l.mask = Some(left_mask(9));
    submit(&mut r, &[l.clone()], &[], &[], true);
    let p = pixel(&mut r, 30, 64);
    assert!(
        (p[0] as i32 - 128).abs() <= 2 && p[2] == 255 && p[3] == 128,
        "{p:?}"
    );
    assert_eq!(pixel(&mut r, 90, 64), [0; 4]);
    l.mask = None;
    submit(&mut r, &[l], &[], &[], true);
    assert_eq!(pixel(&mut r, 90, 64), p);
}

#[test]
fn alpha_lock_preserves_partial_alpha_and_eraser_is_noop() {
    let mut r = WgpuRasterizer::new().unwrap();
    let l = Layer::paint(LayerId(1), "paint");
    submit(
        &mut r,
        std::slice::from_ref(&l),
        &[dab([1., 0., 0., 0.4])],
        &[batch(1)],
        true,
    );
    let mut locked = batch(1);
    locked.style.alpha_locked = true;
    submit(
        &mut r,
        std::slice::from_ref(&l),
        &[dab([0., 0., 1., 0.5])],
        &[locked.clone()],
        false,
    );
    let p = pixel(&mut r, 64, 64);
    assert!(
        (p[0] as i32 - p[2] as i32).abs() <= 1,
        "equal red/blue contributions: {p:?}"
    );
    assert_eq!(p[3], 102);
    locked.style.mode = DabMode::Erase;
    submit(&mut r, &[l], &[dab([1.; 4])], &[locked], false);
    assert_eq!(pixel(&mut r, 64, 64), p);
}

#[test]
fn mask_scene_preview_keeps_pixels_outside_preview_damage() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(LayerMask::reveal_all(LayerId(9), Point::default()));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let mut preview = batch(1);
    preview.kind = DabBatchKind::Preview;
    preview.style.mode = DabMode::Erase;
    preview.damage = Rect {
        min: Point { x: 50., y: 50. },
        max: Point { x: 78., y: 78. },
    };
    let mut d = dab([1.; 4]);
    d.radii = [12.; 2];
    submit(&mut r, &[l.clone()], &[d], &[preview], false);
    assert_eq!(pixel(&mut r, 64, 64), [0; 4]);
    assert_eq!(pixel(&mut r, 30, 64), [255, 0, 0, 255]);
    submit(&mut r, &[l], &[], &[], false);
    assert_eq!(pixel(&mut r, 64, 64), [255, 0, 0, 255]);
}

#[test]
fn mask_scene_destination_preview_preserves_untouched_color() {
    let mut r = WgpuRasterizer::new().unwrap();
    let mut l = Layer::paint(LayerId(1), "paint");
    l.mask = Some(left_mask(9));
    submit(
        &mut r,
        &[l.clone()],
        &[dab([1., 0., 0., 1.])],
        &[batch(1)],
        true,
    );
    let mut b = batch(1);
    b.kind = DabBatchKind::Preview;
    b.style.alpha_locked = true;
    b.damage = Rect {
        min: Point { x: 20., y: 50. },
        max: Point { x: 44., y: 78. },
    };
    let mut d = dab([0., 0., 1., 1.]);
    d.center.x = 32.;
    d.radii = [10.; 2];
    submit(&mut r, &[l], &[d], &[b], false);
    assert_eq!(pixel(&mut r, 32, 64), [0, 0, 255, 255]);
    assert_eq!(pixel(&mut r, 50, 64), [255, 0, 0, 255]);
}

#[test]
#[ignore = "hardware GPU latency benchmark; run serially in release mode"]
fn layer_composition_latency() {
    let mut r = WgpuRasterizer::new().unwrap();
    for (name, count, masked) in [
        ("plain", 1, false),
        ("masked", 1, true),
        ("24 masks", 24, true),
        ("100 masks", 100, true),
    ] {
        let layers: Vec<_> = (1..=count)
            .map(|id| {
                let mut l = Layer::paint(LayerId(id), "layer");
                if masked {
                    l.mask = Some(left_mask(id + 1000));
                }
                l
            })
            .collect();
        let dabs: Vec<_> = (1..=count).map(|_| dab([0.2, 0.3, 0.4, 0.2])).collect();
        let batches: Vec<_> = (1..=count)
            .map(|id| {
                let mut b = batch(id);
                b.first_dab = (id - 1) as u32;
                b
            })
            .collect();
        submit(&mut r, &layers, &dabs, &batches, true);
        r.wait_idle().unwrap();
        let mut times = Vec::new();
        for i in 0..140 {
            let start = std::time::Instant::now();
            submit(&mut r, &layers, &dabs[..1], &[batch(1)], false);
            r.wait_idle().unwrap();
            if i >= 20 {
                times.push(start.elapsed().as_secs_f64() * 1000.);
            }
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "{name}: completed GPU frame ms p50={:.3} p95={:.3} p99={:.3}",
            times[60], times[114], times[118]
        );
    }
}
