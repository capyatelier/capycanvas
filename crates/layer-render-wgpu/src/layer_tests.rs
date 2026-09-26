//! Pixel assertions and bounded GPU-completion timings for layer composition.
use super::*;
use layer_core::{
    BrushDeform, BrushRendering, BrushWetMix, LayerMask, LayerOperation, LayerOperationKind, Point,
    Rect, Selection, StrokeId,
};
use layer_render::{DabStyle, ViewState};
#[path = "tonal_tests.rs"]
mod tonal_selection;
#[path = "selection_option_tests.rs"]
mod selection_options;
#[path = "selection_paint_tests.rs"]
mod selection_painting;
#[path = "submission_tests.rs"]
mod submissions;
#[path = "paint_transform_tests.rs"]
mod transforms;
#[path = "transform_latency_tests.rs"]
mod transform_latency;
#[path = "transform_oracle_tests.rs"]
mod transform_oracles;
#[path = "placement_tests.rs"]
mod placement;

fn view() -> ViewState {
    ViewState {
        width_px: 128,
        height_px: 128,
        document_to_surface: [1., 0., 0., 1., 0., 0.],
        background_rgba_linear: [0.; 4],
    }
}
pub(super) fn dab(color: [f32; 4]) -> Dab {
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
        previous: [0.0; 4],
        contact: [0.0; 4],
        previous_contact: [0.0; 4],
    }
}
pub(super) fn batch(id: u64) -> DabBatch {
    DabBatch {
        material_update: 0,
        stroke_id: StrokeId(1),
        layer_id: LayerId(id),
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style: DabStyle {
            brush_to_layer: layer_core::Affine::IDENTITY,
            alpha_locked: false,
            selection: None,
            tip: BrushTip::AnalyticEllipse,
            mode: DabMode::Paint,
            execution: BrushExecution::Dry,
            grain: None,
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix::default(),
            transport: None,
            deform: BrushDeform::default(),
            contact: None,
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
        restore_rasters: &[],
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

#[test]
fn retained_scene_viewport_preserves_pixels_outside_local_paint_and_preview_damage() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let make_target = || r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("retained scene viewport regression"),
        size: wgpu::Extent3d { width: 512, height: 512, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST, view_formats: &[],
    });
    let target = make_target();
    let reference = make_target();
    let buffered = make_target();
    let shared_again = make_target();
    let mut layer = Layer::paint(LayerId(1), "scene paint");
    layer.mask = Some(LayerMask::reveal_all(LayerId(9), Point::default()));
    let layers = [layer];
    let camera = ViewState { width_px: 512, height_px: 512,
        background_rgba_linear: [0.2, 0.3, 0.4, 1.], ..view() };
    let mut retained = crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb).unwrap();
    retained.set_target_retention(true);
    for (i, (x, y, preview)) in [(85., 90., false), (365., 330., true),
        (95., 370., true), (360., 100., false)].into_iter().enumerate() {
        let mut ink = dab([0.8, 0.1, 0.2, 0.7]);
        ink.center = Point { x, y };
        ink.radii = [24.; 2];
        ink.previous = [24., 24., 1., 0.];
        ink.contact = [1., 0., 0., 0.];
        let mut stroke = batch(1);
        stroke.kind = if preview { DabBatchKind::Preview } else { DabBatchKind::Persistent };
        stroke.style = preset_style(layer_core::DefaultBrushPreset::Pencil);
        stroke.damage = ink.bounds();
        r.submit(FramePacket { view: camera, document_extent: [1024; 2], layers: &layers,
            dabs: &[ink], dab_batches: &[stroke], restore_rasters: &[],
            reset_layers: i == 0, composite_all: i == 0, time_seconds: 0., }).unwrap();
        if i > 0 { assert!(r.composite_damage.area() < 1024 * 1024); }
        retained.present(&r, &target.create_view(&Default::default()), camera, [0.2; 4]).unwrap();
        let mut full = crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb).unwrap();
        full.present(&r, &reference.create_view(&Default::default()), camera, [0.2; 4]).unwrap();
        assert_eq!(page_bytes(&r, &target), page_bytes(&r, &reference), "frame {i}");
    }
    // A swapchain mode change replaces the image even when the scene and view
    // are unchanged. Neither transition may inherit old image damage/history.
    retained.set_target_retention(false);
    retained.present(&r, &buffered.create_view(&Default::default()), camera, [0.2; 4]).unwrap();
    assert_eq!(page_bytes(&r, &buffered), page_bytes(&r, &reference), "new buffered image");
    retained.set_target_retention(true);
    retained.present(&r, &shared_again.create_view(&Default::default()), camera, [0.2; 4]).unwrap();
    assert_eq!(page_bytes(&r, &shared_again), page_bytes(&r, &reference), "new shared image");
}

// Inspect persistent pigment/wetness independently of layer-level effects.
// This is test-only readback, never a drawing or selection raster path.
pub(super) fn page_bytes(r: &WgpuRasterizer, texture: &wgpu::Texture) -> Vec<u8> {
    let row = texture.width() * texture.format().block_copy_size(None).unwrap();
    let stride = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test persistent page"),
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    let submission = encoder.submit(&r.queue);
    let (send, receive) = mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap();
        });
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    receive.recv().unwrap().unwrap();
    let bytes = buffer.slice(..).get_mapped_range().unwrap()
        .chunks_exact(stride as usize).flat_map(|line| line[..row as usize].iter().copied()).collect();
    buffer.unmap();
    bytes
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

pub(crate) fn preset_style(preset: layer_core::DefaultBrushPreset) -> DabStyle {
    let brush = layer_core::default_brush(preset);
    DabStyle {
        brush_to_layer: layer_core::Affine::IDENTITY,
        alpha_locked: false,
        selection: None,
        tip: brush.tip,
        mode: if preset == layer_core::DefaultBrushPreset::Eraser {
            DabMode::Erase
        } else {
            DabMode::Paint
        },
        execution: brush.execution,
        grain: brush.grain,
        rendering: brush.rendering,
        wet_mix: brush.wet_mix,
        transport: brush.transport,
        deform: brush.deform,
        contact: brush.contact,
    }
}

#[test]
fn scanline_selection_handles_holes_crossings_offcanvas_and_wide_rows() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let extent = [2048, 128];
    let mut outside = Selection::polygon(vec![
        Point { x: -40., y: -10. },
        Point { x: 2031.5, y: 5. },
        Point { x: 2050., y: 135. },
        Point { x: -1., y: 113. },
    ])
    .unwrap();
    let hole = Selection::polygon(vec![
        Point { x: 7.25, y: 3.75 },
        Point { x: 1950., y: 125. },
        Point { x: 17.25, y: 105. },
        Point { x: 1910., y: 10. },
    ])
    .unwrap();
    outside.shape = layer_core::SelectionShape::Contours(
        vec![outside.contours()[0].clone(), hole.contours()[0].clone()].into(),
    );
    let render = |r: &mut WgpuRasterizer, layer: &Layer, brush: &DabBatch| {
        let mut d = dab([1.; 4]);
        d.center = Point { x: 1024., y: 64. };
        d.radii = [3000.; 2];
        r.submit(FramePacket {
            view: ViewState {
                width_px: 2048,
                ..view()
            },
            document_extent: extent,
            layers: std::slice::from_ref(layer),
            dabs: &[d],
            dab_batches: std::slice::from_ref(brush),
            restore_rasters: &[],
            reset_layers: true,
            time_seconds: 0.,
            composite_all: true,
        })
        .unwrap();
        r.readback_srgb_rgba8().unwrap()
    };
    for inverted in [false, true] {
        for delta in [
            Point::default(),
            Point {
                x: 0.375,
                y: -0.125,
            },
        ] {
            let mut geometry = outside.translated(delta);
            geometry.inverted = inverted;
            let mut layer = Layer::paint(LayerId(1), "scanline reference");
            let mut mask = LayerMask::reveal_all(LayerId(9), Point::default());
            mask.default_coverage = f32::from(inverted);
            mask.initial = Some(geometry.clone());
            layer.mask = Some(mask);
            let mut b = batch(1);
            b.damage = Rect {
                min: Point::default(),
                max: Point { x: 2048., y: 128. },
            };
            let reference = render(&mut r, &layer, &b);
            layer.mask = None;
            b.style.selection = Some(std::sync::Arc::new(geometry));
            let actual = render(&mut r, &layer, &b);
            for (i, (a, b)) in actual.iter().zip(reference).enumerate() {
                assert!(
                    a.abs_diff(b) <= 1,
                    "coverage mismatch at byte {i}: {a} vs {b}, inverted={inverted}"
                );
            }
        }
    }
}

#[test]
fn clipping_stack_keeps_soft_base_alpha_and_group_opacity_once() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
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
    let mut r = WgpuRasterizer::new_headless().unwrap();
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
    l.pending_operations.push(LayerOperation {
        placement: layer_core::Affine::IDENTITY,
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
fn alpha_lock_preserves_partial_alpha_and_eraser_is_noop() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
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
fn small_swept_contact_preview_matches_commit_and_preserves_distant_pixels() {
    use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::Srgb, depth: SampleDepth::U8,
    }).unwrap();
    let layers = [Layer::paint(LayerId(1), "small swept preview")];
    let render = |r: &mut WgpuRasterizer, dabs: &[Dab], batches: &[DabBatch], reset| {
        r.submit(FramePacket {
            view: view(), document_extent: [1024; 2], layers: &layers,
            dabs, dab_batches: batches, restore_rasters: &[], reset_layers: reset,
            time_seconds: 0., composite_all: true,
        }).unwrap();
    };
    for preset in layer_core::CONTACT_BRUSH_PRESETS {
        let mut base = dab([0.7, 0.1, 0.2, 0.8]);
        base.center = Point { x: 512., y: 512. };
        base.radii = [900.; 2];
        let mut base_batch = batch(1);
        base_batch.damage = base.bounds();
        let mut first = dab([0.1, 0.3, 0.8, 0.7]);
        first.center = Point { x: 260., y: 255. };
        first.radii = [9., 4.];
        first.previous = [0.6, 1.2, 0.8, 0.6];
        first.motion = [29., -17.];
        first.contact = [0.9, 0.7, 1., 0.3];
        first.previous_contact = [0.1, 0.2, 0., 0.3];
        let mut last = first;
        last.center = Point { x: 770., y: 790. };
        last.motion = [-21., 31.];
        let mut stroke = batch(1);
        stroke.stroke_id = StrokeId(2);
        stroke.style = preset_style(preset);
        stroke.dab_count = 2;
        stroke.damage = first.bounds().union(last.bounds());
        render(&mut r, &[base], &[base_batch.clone()], true);
        let original = r.readback_srgb_rgba8().unwrap();
        render(&mut r, &[first, last], &[stroke.clone()], false);
        let committed = r.readback_srgb_rgba8().unwrap();
        assert_ne!(original, committed, "{preset:?} must deposit");
        render(&mut r, &[base], &[base_batch], true);
        stroke.kind = DabBatchKind::Preview;
        stroke.stroke_end = false;
        render(&mut r, &[first, last], &[stroke.clone()], false);
        let predicted = r.readback_srgb_rgba8().unwrap();
        let maximum = predicted.iter().zip(&committed).map(|(a,b)|a.abs_diff(*b)).max().unwrap();
        assert!(maximum <= 1, "{preset:?}: predicted vs committed maximum error {maximum}");
        for (x,y) in [(512,512), (100,100), (950,950)] {
            let i = (y*1024+x)*4;
            assert_eq!(&predicted[i..i+4], &original[i..i+4], "{preset:?}: untouched ({x}, {y})");
        }
        // Reuse the prediction pool with the opposite diagonal. The old
        // contacts remain inside the bounding rectangle but outside the new
        // sparse plan; they must resolve to persistent paint, not pooled pixels.
        first.center.y = 790.;
        last.center.y = 255.;
        stroke.damage = first.bounds().union(last.bounds());
        render(&mut r, &[first, last], &[stroke], false);
        let moved = r.readback_srgb_rgba8().unwrap();
        for (x, y) in [(260, 255), (770, 790)] {
            let i = (y * 1024 + x) * 4;
            assert_eq!(&moved[i..i+4], &original[i..i+4], "{preset:?}: retired prediction ({x}, {y})");
        }
        render(&mut r, &[], &[], false);
        assert_eq!(r.readback_srgb_rgba8().unwrap(), original, "{preset:?}: cancel restores pixels");
    }
}

