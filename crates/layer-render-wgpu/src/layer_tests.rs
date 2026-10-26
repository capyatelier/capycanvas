//! Pixel assertions and bounded GPU-completion timings for layer composition.
use super::*;
use crate::test_support::packet;
use layer_core::{Document, SceneView, CoverageSnapshot, RasterOperation, RasterOperationKind, Point, Rect, Selection, StrokeId,
    authored::{Occurrence, OccurrenceContent, RecordChange, SourceTarget, Stack}};
use placement::{paint_document, target, paint, paint_mut, occurrence_mut, set_mask, mask_snapshot, reveal_all};
use layer_render::{DabStyle, ViewState};
#[path = "tonal_tests.rs"]
mod tonal_selection;
#[path = "selection_option_tests.rs"]
mod selection_options;
#[path = "enclose_fill_tests.rs"]
mod enclose_fill;
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
pub(crate) mod placement;
#[path = "placement_material_tests.rs"]
mod placement_material;
#[path = "erase_tests.rs"]
mod erase;
#[path = "liquify_tests.rs"]
mod liquify;
#[path = "crop_overlay_tests.rs"]
mod crop_overlay;
#[path = "paper_shade_tests.rs"]
mod paper_shade;
#[path = "paint_mixing_tests.rs"]
mod paint_mixing;

fn limited_renderer(storage_binding_bytes: u64) -> WgpuRasterizer {
    let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let required_limits = wgpu::Limits { max_storage_buffer_binding_size: storage_binding_bytes, ..reference.device().limits() };
    let (device, queue) = pollster::block_on(reference.adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("limited storage bindings"), required_features: reference.device().features(), required_limits,
        memory_hints: crate::memory_hints(), ..Default::default() })).unwrap();
    let mut renderer = *WgpuRasterizer::from_wgpu_inner(reference.adapter.clone(), device.into(), queue, Initialization::Headless).unwrap();
    renderer.initialize_native(Default::default()).unwrap();
    renderer
}

fn view() -> ViewState {
    ViewState {
        width_px: 128,
        height_px: 128,
        document_to_surface: [1., 0., 0., 1., 0., 0.],
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
pub(super) fn batch(target: SourceTarget) -> DabBatch {
    let damage = Rect { min: Point { x: 0., y: 0. }, max: Point { x: 128., y: 128. } };
    crate::test_support::dab_batch(target, crate::tests::test_style(BrushExecution::Dry), damage)
}
fn submit(
    r: &mut WgpuRasterizer,
    scene: SceneView<'_>,
    dabs: &[Dab],
    batches: &[DabBatch],
    reset: bool,
) {
    crate::test_support::submit_ready(r, FramePacket { dabs, dab_batches: batches, reset_layers: reset, ..packet(scene, [128, 128]) })
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
    let make_target = || crate::create_target(&r.device, [512, 512], format, "retained scene viewport regression").0;
    let target = make_target();
    let reference = make_target();
    let buffered = make_target();
    let shared_again = make_target();
    let mut document = paint_document([1024; 2], "scene paint");
    set_mask(&mut document, reveal_all([1024; 2], [0, 0]));
    let camera = ViewState { width_px: 512, height_px: 512,
        ..view() };
    let mut retained = crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb).unwrap();
    retained.set_target_retention(true);
    for (i, (x, y, preview)) in [(85., 90., false), (365., 330., true),
        (95., 370., true), (360., 100., false)].into_iter().enumerate() {
        let mut ink = dab([0.8, 0.1, 0.2, 0.7]);
        ink.center = Point { x, y };
        ink.radii = [24.; 2];
        ink.previous = [24., 24., 1., 0.];
        ink.contact = [1., 0., 0., 0.];
        let mut stroke = batch(placement::target(&document));
        stroke.kind = if preview { DabBatchKind::Preview } else { DabBatchKind::Persistent };
        stroke.style = preset_style(layer_core::DefaultBrushPreset::Pencil);
        stroke.damage = ink.bounds();
        r.submit(FramePacket {
            view: camera,
            dabs: &[ink],
            dab_batches: &[stroke],
            reset_layers: i == 0,
            composite_all: i == 0,
            ..packet(document.scene(), [1024; 2])
        }).unwrap();
        if i > 0 { assert!(r.composite_damage.area() < 1024 * 1024); }
        let cursor = [layer_render::CursorSegment { from: [x + 24., y - 24.],
            to: [x + 140., y + 24.], distance: 0., marker: 2., scale: 1. }];
        retained.set_cursor(r.device(), &cursor, 1.);
        retained.present(&r, &target.create_view(&Default::default()), camera, [0.2; 4]).unwrap();
        let mut full = crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb).unwrap();
        full.set_cursor(r.device(), &cursor, 1.);
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

#[test]
fn oversized_surface_preserves_viewport_pixels_and_overlays_at_every_rotation() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document = paint_document([128; 2], "viewport artwork");
    submit(&mut r, document.scene(), &[dab([0.8, 0.1, 0.2, 1.])], &[batch(placement::target(&document))], true);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let camera = ViewState { width_px: 173, height_px: 117,
        document_to_surface: [0.65, 0., 0., 0.65, 23., 17.] };
    let crop = layer_core::Affine([78., 0., 0., 86., 10., 4.]);
    let cursor = [layer_render::CursorSegment { from: [14., 80.], to: [150., 100.],
        distance: 0., marker: 2., scale: 1. }];
    for turns in 0u32..4 {
        let extent = if turns.is_multiple_of(2) { [camera.width_px, camera.height_px] }
            else { [camera.height_px, camera.width_px] };
        let capacity = [extent[0] + 95, extent[1] + 139];
        let exact = crate::create_target(&r.device, extent, format, "exact viewport target").0;
        let oversized = crate::create_target(&r.device, capacity, format, "oversized viewport target").0;
        let mut presenters = std::array::from_fn::<_, 2, _>(|_| {
            let mut presenter = crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb).unwrap();
            presenter.set_surface_rotation(turns);
            presenter
        });
        let mut previous = None;
        for overlays in 0..3 {
            r.set_crop_overlay((overlays > 0).then(|| layer_render::CropOverlay {
                to_crop: crop.inverse().unwrap(), dim: 0.5,
            }));
            for (presenter, target) in presenters.iter_mut().zip([&exact, &oversized]) {
                presenter.set_cursor(r.device(), if overlays == 2 { &cursor } else { &[] }, 1.);
                presenter.present(&r, &target.create_view(&Default::default()), camera, [0.2, 0.3, 0.4, 1.]).unwrap();
            }
            let expected = page_bytes(&r, &exact);
            let actual = page_bytes(&r, &oversized);
            let row = extent[0] as usize * 4;
            let mismatch = expected.chunks_exact(row).zip(actual.chunks_exact(capacity[0] as usize * 4))
                .enumerate().find_map(|(y, (a, b))| a.iter().zip(b).position(|(a, b)| a != b).map(|x| (x / 4, y)));
            assert!(mismatch.is_none(), "rotation {turns}, overlay stage {overlays}: different pixel at {mismatch:?}");
            if let Some(before) = previous {
                assert!(expected != before, "rotation {turns}, overlay stage {overlays}: the overlay must be visible");
            }
            previous = Some(expected);
        }
    }
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
fn left_mask() -> CoverageSnapshot {
    let mut m = reveal_all([128; 2], [0, 0]);
    m.source.default_coverage = 0.;
    crate::test_support::materialize_mask(&mut m.source,
        Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 64., y: 0. },
            Point { x: 64., y: 128. },
            Point { x: 0., y: 128. },
        ])
        .unwrap(),
        Default::default(),
    );
    m
}

pub(crate) fn preset_style(preset: layer_core::DefaultBrushPreset) -> DabStyle {
    let brush = layer_core::default_brush(preset);
    DabStyle {
        diameter: brush.diameter,
        opacity: brush.opacity,
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
        paper: None,
        rendering: brush.rendering,
        wet_mix: brush.wet_mix,
        transport: brush.transport,
        deform: brush.deform,
        blender: brush.blender,
        liquify: None,
        contact: brush.contact,
        retouch: None,
        blend_space: layer_core::BlendSpace::Linear,
    }
}

#[test]
fn scanline_selection_handles_holes_crossings_offcanvas_and_wide_rows() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
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
    let render = |r: &mut WgpuRasterizer, document: &Document, brush: &DabBatch| {
        let mut d = dab([1.; 4]);
        d.center = Point { x: 1024., y: 64. };
        d.radii = [3000.; 2];
        crate::test_support::submit_ready(r, FramePacket {
            view: ViewState { width_px: 2048, ..view() },
            dabs: &[d],
            dab_batches: std::slice::from_ref(brush),
            reset_layers: true,
            ..packet(document.scene(), extent)
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
            let mut document = paint_document(extent, "scanline reference");
            let mut mask = reveal_all(extent, [0, 0]);
            mask.source.default_coverage = f32::from(inverted);
    crate::test_support::materialize_mask(&mut mask.source, geometry.clone(), document.composition().color);
            set_mask(&mut document, mask);
            let mut b = batch(target(&document));
            b.damage = Rect {
                min: Point::default(),
                max: Point { x: 2048., y: 128. },
            };
            let reference = render(&mut r, &document, &b);
            occurrence_mut(&mut document).mask = None;
            b.style.selection = Some(std::sync::Arc::new(geometry));
            let actual = render(&mut r, &document, &b);
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
fn connected_and_global_regions_select_painted_disks() {
    use layer_render::{RegionRequest, RegionSource};
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document = paint_document([128; 2], "disks");
    let mut left = dab([1., 0., 0., 1.]);
    left.center = Point { x: 32., y: 64. };
    left.radii = [16.; 2];
    let mut right = left;
    right.center.x = 96.;
    let mut disks = batch(target(&document));
    disks.dab_count = 2;
    submit(&mut r, document.scene(), &[left, right], &[disks], true);
    for contiguous in [true, false] {
        let result = crate::test_support::receive_request(&mut r, RegionRequest {
            enclosure: None, contiguous,
            selection: None,
            request_id: 1,
            source: RegionSource::Source(target(&document)),
            position: [32, 64],
            tolerance: 0.,
            refinement: Default::default(),
            limit: None,
        });
        let [x0, y0, x1, y1] = result.pixels.bounds();
        assert!(x0 <= 20 && y0 <= 52 && y1 >= 76, "{contiguous}: {:?}", result.pixels.bounds());
        assert_eq!(x1 < 64, contiguous, "{:?}", result.pixels.bounds());
    }
}

#[test]
fn clipping_stack_keeps_soft_base_alpha_and_group_opacity_once() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = paint_document([128; 2], "base");
    let base = placement::occurrence_id(&document);
    let base_target = target(&document);
    let source = paint(&document).clone();
    let (a, a_target) = placement::append_paint(&mut document, "clip a", source.clone());
    let (b, b_target) = placement::append_paint(&mut document, "clip b", source);
    document.artwork.occurrences.get_mut(a).unwrap().attachment = layer_core::Attachment::Clip;
    document.artwork.occurrences.get_mut(b).unwrap().attachment = layer_core::Attachment::Clip;
    let root = document.composition().result;
    let stack = RecordChange::replace(&document.artwork.stacks, root, Some(Stack { entries: vec![b, a, base] })).unwrap();
    document.apply(layer_core::Edit::Stack(stack)).unwrap();
    let mut batches = vec![batch(base_target), batch(a_target), batch(b_target)];
    for (i, b) in batches.iter_mut().enumerate() {
        b.first_dab = i as u32;
    }
    let dabs = [
        dab([1., 0., 0., 0.4]),
        dab([0., 1., 0., 1.]),
        dab([0., 0., 1., 1.]),
    ];
    submit(&mut r, document.scene(), &dabs, &batches, true);
    assert_eq!(pixel(&mut r, 64, 64), [0, 0, 255, 102]); // export is straight-alpha sRGB
    let children = RecordChange::insert(&document.artwork.stacks, Stack { entries: vec![b, a, base] });
    let mut group = Occurrence::new(OccurrenceContent::Stack(children.handle), "group");
    group.opacity = 0.5;
    let group = RecordChange::insert(&document.artwork.occurrences, group);
    let group_id = group.handle;
    let stack = RecordChange::replace(&document.artwork.stacks, root, Some(Stack { entries: vec![group_id] })).unwrap();
    document.apply(layer_core::Edit::Batch(vec![layer_core::Edit::Stack(children), layer_core::Edit::Occurrence(group), layer_core::Edit::Stack(stack)])).unwrap();
    submit(&mut r, document.scene(), &[], &[], false);
    let p = pixel(&mut r, 64, 64);
    assert!((p[3] as i32 - 51).abs() <= 1, "{p:?}");
    document.artwork.occurrences.get_mut(group_id).unwrap().visible = false;
    submit(&mut r, document.scene(), &[], &[], false);
    assert_eq!(pixel(&mut r, 64, 64), [0; 4]);
}

#[test]
fn apply_mask_preserves_pixels_and_does_not_remain_a_live_mask() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = paint_document([128; 2], "paint");
    set_mask(&mut document, left_mask());
    submit(&mut r, document.scene(), &[dab([1., 0., 0., 1.])], &[batch(target(&document))], true);
    let before = r.readback_srgb_rgba8().unwrap();
    let coverage = mask_snapshot(&document);
    occurrence_mut(&mut document).mask = None;
    paint_mut(&mut document).operations = Arc::new(vec![RasterOperation {
        placement: layer_core::Affine::IDENTITY,
        coverage,
        kind: RasterOperationKind::ApplyMask,
    }]);
    let mut op = batch(target(&document));
    op.dab_count = 0;
    op.kind = DabBatchKind::RasterOperation(0);
    submit(&mut r, document.scene(), &[], &[op], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), before);
    submit(&mut r, document.scene(), &[dab([0., 1., 0., 1.])], &[batch(target(&document))], false);
    assert_eq!(
        pixel(&mut r, 90, 64),
        [0, 255, 0, 255],
        "new paint can extend the baked silhouette"
    );
}

#[test]
fn alpha_lock_preserves_partial_alpha_and_eraser_is_noop() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document = paint_document([128; 2], "paint");
    submit(
        &mut r,
        document.scene(),
        &[dab([1., 0., 0., 0.4])],
        &[batch(target(&document))],
        true,
    );
    let mut locked = batch(target(&document));
    locked.style.alpha_locked = true;
    submit(
        &mut r,
        document.scene(),
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
    submit(&mut r, document.scene(), &[dab([1.; 4])], &[locked], false);
    assert_eq!(pixel(&mut r, 64, 64), p);
}

#[test]
fn small_swept_contact_preview_matches_commit_and_preserves_distant_pixels() {
    swept_contact_preview(layer_core::color::LayerColorMode::FullColor);
}

#[test]
fn in_place_dry_strokes_match_ping_pong_pages() {
    let extent = [1280; 2];
    let render = |in_place: bool| {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        if !in_place { crate::test_support::write_only_dry_material(&mut r); }
        let document = paint_document(extent, "wide dry stroke");
        let style = preset_style(layer_core::DefaultBrushPreset::Pencil);
        let relief = style.contact.unwrap().dry.unwrap().pose(0.5).relief;
        let mut ink = dab([0.2, 0.3, 0.4, 0.9]);
        ink.center = Point { x: 640., y: 640. };
        ink.radii = [600.; 2];
        ink.previous = [600., 600., 1., 0.];
        ink.contact = [0.7, relief[0], relief[1], relief[2]];
        ink.previous_contact = ink.contact;
        let mut stroke = batch(target(&document));
        stroke.style = style;
        stroke.damage = ink.bounds();
        r.submit(FramePacket { dabs: &[ink], dab_batches: &[stroke], ..packet(document.scene(), extent) }).unwrap();
        let mut pages: Vec<_> = r.paint_layers.iter().find(|l| l.id == target(&document)).unwrap().pages.iter()
            .map(|p| (p.coordinate, page_bytes(&r, &p.active().texture))).collect();
        pages.sort_by_key(|(coordinate, _)| *coordinate);
        pages
    };
    let in_place = render(true);
    assert!(in_place.iter().any(|(_, bytes)| bytes.as_chunks::<16>().0.iter().any(|p| p[12..] != [0; 4])), "the stroke paints");
    assert!(in_place == render(false), "in-place pages match the ping-pong pages exactly");
}

#[test]
fn display_predictions_wider_than_the_input_sets_match_single_page_predictions() {
    let extent = [1024; 2];
    let document = paint_document(extent, "wide display prediction");
    let pages: Vec<[u32; 2]> = (0..4).flat_map(|y| (0..4).map(move |x| [x, y])).collect();
    let at = |page: [u32; 2], offset: f32| Point { x: page[0] as f32 * 256. + 128. + offset, y: page[1] as f32 * 256. + 128. + offset };
    let predict = |predicted: &[[u32; 2]]| {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut frame = packet(document.scene(), extent);
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        frame.view.width_px = 128;
        frame.view.height_px = 128;
        let base: Vec<_> = pages.iter().enumerate().map(|(i, &page)| {
            let mut paint = dab([0.25 * (i % 4) as f32, 0.125 * (i / 4) as f32, 0.5, 1.]);
            paint.center = at(page, 0.);
            paint.radii = [100.; 2];
            paint
        }).collect();
        let mut committed = batch(target(&document));
        committed.dab_count = base.len() as u32;
        committed.damage = Rect::from_extent(extent);
        r.submit(FramePacket { dabs: &base, dab_batches: &[committed], ..frame }).unwrap();
        let ink: Vec<_> = predicted.iter().map(|&page| {
            let mut ink = dab([0.9, 0.2, 0.1, 1.]);
            ink.center = at(page, 40.);
            ink.radii = [24.; 2];
            ink
        }).collect();
        let mut stroke = batch(target(&document));
        stroke.style = preset_style(layer_core::DefaultBrushPreset::GPen);
        stroke.style.opacity = 0.5;
        stroke.dab_count = ink.len() as u32;
        stroke.damage = Rect::from_extent(extent);
        stroke.kind = DabBatchKind::Preview;
        stroke.stroke_end = false;
        r.submit(FramePacket { dabs: &ink, dab_batches: std::slice::from_ref(&stroke), composite_all: false, ..frame }).unwrap();
        assert!(r.dry_display_gathers(&stroke), "the prediction averages its inputs at display resolution");
        let predicted: Vec<_> = predicted.iter().map(|&page| page_bytes(&r, &r.preview_page(page).unwrap().active().texture)).collect();
        (r.dry_display_inputs.len(), predicted)
    };
    let (sets, together) = predict(&pages);
    assert!(pages.len() > dry_material::DISPLAY_INPUT_SETS, "the prediction covers more pages than the input sets");
    assert_eq!(sets, dry_material::DISPLAY_INPUT_SETS, "input sets stay bounded by the set, not the prediction");
    for index in [0, 7, 8, 15] {
        let (_, alone) = predict(&[pages[index]]);
        assert!(together[index] == alone[0], "page {:?} matches its prediction alone", pages[index]);
    }
}

#[test]
fn reduced_color_contact_previews_match_committed_pixels() {
    for mode in [layer_core::color::LayerColorMode::Grayscale, layer_core::color::LayerColorMode::TwoTone] {
        swept_contact_preview(mode);
    }
}

#[test]
fn reduced_color_live_strokes_constrain_pixels_before_publication() {
    for mode in [layer_core::color::LayerColorMode::Grayscale, layer_core::color::LayerColorMode::TwoTone] {
        for preset in [layer_core::DefaultBrushPreset::GPen, layer_core::DefaultBrushPreset::Airbrush, layer_core::DefaultBrushPreset::WatercolorWash] {
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            let mut document = paint_document([256; 2], "live reduced color stroke");
            paint_mut(&mut document).color_mode = mode;
            let mut ink = dab([0.03, 0.04, 0.3, 0.9]);
            ink.center = Point { x: 128., y: 128. }; ink.radii = [80.; 2];
            let mut stroke = batch(target(&document));
            stroke.style = preset_style(preset); stroke.damage = ink.bounds(); stroke.stroke_end = false;
            r.submit(FramePacket { dabs: &[ink], dab_batches: &[stroke], ..packet(document.scene(), [256; 2]) }).unwrap();
            assert!(paint(&document).raster.is_empty());
            let pages = &r.paint_layers.iter().find(|p| p.id == target(&document)).unwrap().pages;
            assert!(!pages.is_empty());
            for page in pages {
                for pixel in page_bytes(&r, &page.active().texture).as_chunks::<16>().0 {
                    let channels = std::array::from_fn::<_, 4, _>(|c| f32::from_le_bytes(pixel[c * 4..c * 4 + 4].try_into().unwrap()));
                    assert_eq!(channels[0], channels[1], "{mode:?} {preset:?}");
                    assert_eq!(channels[1], channels[2], "{mode:?} {preset:?}");
                    if mode == layer_core::color::LayerColorMode::TwoTone {
                        assert!(channels[0] == 0. || channels[0] == 1.);
                        assert!(channels[3] == 0. || channels[3] == 1.);
                    }
                }
            }
        }
    }
}

pub(crate) fn reduced_color_scaled_preview(mut r: WgpuRasterizer, mode: layer_core::color::LayerColorMode) {
    let mut document = paint_document([1024; 2], "reduced color scaled preview");
    paint_mut(&mut document).color_mode = mode;
    let send = |r: &mut WgpuRasterizer, document: &mut Document, dabs: &[Dab], batches: &[DabBatch], reset| {
        if batches.iter().any(|b| b.kind != DabBatchKind::Preview) {
            paint_mut(document).raster = layer_core::raster::RasterRevision::pending();
        }
        let mut frame = packet(document.scene(), [1024; 2]);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        frame.view.width_px = 128; frame.view.height_px = 128;
        crate::test_support::submit_ready(r, FramePacket { dabs, dab_batches: batches, reset_layers: reset, ..frame }).unwrap();
    };
    let mut base = dab([1.; 4]);
    base.center = Point { x: 512., y: 512. }; base.radii = [900.; 2];
    let mut background = batch(target(&document)); background.damage = base.bounds();
    send(&mut r, &mut document, &[base], &[background], true);
    let original = crate::scene::scale::tests::display_pixels(&r);
    let mut ink = dab([0.03, 0.04, 0.3, 1.]);
    ink.center = Point { x: 512., y: 512. }; ink.radii = [180.; 2];
    let mut stroke = batch(target(&document));
    stroke.style = preset_style(layer_core::DefaultBrushPreset::GPen);
    stroke.damage = ink.bounds(); stroke.kind = DabBatchKind::Preview; stroke.stroke_end = false;
    send(&mut r, &mut document, &[ink], &[stroke.clone()], false);
    assert!(r.preview_level > 0);
    assert!(r.preview_pages.iter().all(|p| p.active().texture.width() < 256));
    let prediction = crate::scene::scale::tests::display_pixels(&r);
    assert_ne!(prediction, original);
    for pixel in &prediction {
        assert!((pixel[0] - pixel[1]).abs() < 0.0001 && (pixel[1] - pixel[2]).abs() < 0.0001);
    }
    send(&mut r, &mut document, &[], &[], false);
    assert_eq!(crate::scene::scale::tests::display_pixels(&r), original);
    stroke.kind = DabBatchKind::Persistent; stroke.stroke_end = true;
    send(&mut r, &mut document, &[ink], &[stroke], false);
    let accepted = crate::scene::scale::tests::display_pixels(&r);
    assert_ne!(accepted, original);
    send(&mut r, &mut document, &[], &[], true);
    assert_eq!(crate::scene::scale::tests::display_pixels(&r), accepted);
}

fn swept_contact_preview(mode: layer_core::color::LayerColorMode) {
    use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::Srgb, depth: SampleDepth::U8,
    }).unwrap();
    let mut document = paint_document([1024; 2], "small swept preview");
    let SourceTarget::Paint(handle) = target(&document) else { unreachable!() };
    document.artwork.paint.get_mut(handle).unwrap().color_mode = mode;
    let frames = std::cell::RefCell::new(document.clone());
    let render = |r: &mut WgpuRasterizer, dabs: &[Dab], batches: &[DabBatch], reset| {
        let mut frame = frames.borrow_mut();
        if mode != layer_core::color::LayerColorMode::FullColor && batches.iter().any(|b| b.kind != DabBatchKind::Preview) {
            frame.artwork.paint.get_mut(handle).unwrap().raster = layer_core::raster::RasterRevision::pending();
        }
        crate::test_support::submit_ready(r, FramePacket {
            view: view(),
            dabs,
            dab_batches: batches,
            reset_layers: reset,
            ..packet(frame.scene(), [1024; 2])
        }).unwrap();
    };
    for preset in layer_core::CONTACT_BRUSH_PRESETS {
        if mode != layer_core::color::LayerColorMode::FullColor { r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U8 }).unwrap(); }
        let mut base = dab(if mode == layer_core::color::LayerColorMode::TwoTone { [0.03, 0.01, 0.02, 0.8] } else { [0.7, 0.1, 0.2, 0.8] });
        base.center = Point { x: 512., y: 512. };
        base.radii = [900.; 2];
        let mut base_batch = batch(target(&document));
        base_batch.damage = base.bounds();
        let mut first = dab(if mode == layer_core::color::LayerColorMode::TwoTone { [1., 1., 1., 0.7] } else { [0.1, 0.3, 0.8, 0.7] });
        first.center = Point { x: 260., y: 255. };
        first.radii = [9., 4.];
        first.previous = [0.6, 1.2, 0.8, 0.6];
        first.motion = [29., -17.];
        first.contact = [0.9, 0.7, 1., 0.3];
        first.previous_contact = [0.1, 0.2, 0., 0.3];
        let mut stroke = batch(target(&document));
        stroke.stroke_id = StrokeId(2);
        stroke.style = preset_style(preset);
        if let Some(dry) = stroke.style.contact.and_then(|c| c.dry) {
            let current = dry.pose(0.7).relief;
            let previous = dry.pose(0.2).relief;
            first.contact = [0.9, current[0], current[1], current[2]];
            first.previous_contact = [0.1, previous[0], previous[1], previous[2]];
        }
        let mut last = first;
        last.center = Point { x: 770., y: 790. };
        last.motion = [-21., 31.];
        stroke.dab_count = 2;
        stroke.damage = first.bounds().union(last.bounds());
        render(&mut r, &[base], &[base_batch.clone()], true);
        let original = r.readback_srgb_rgba8().unwrap();
        render(&mut r, &[first, last], &[stroke.clone()], false);
        let committed = r.readback_srgb_rgba8().unwrap();
        assert!(original != committed, "{mode:?} {preset:?} must deposit");
        if mode != layer_core::color::LayerColorMode::FullColor { r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U8 }).unwrap(); }
        render(&mut r, &[base], &[base_batch], true);
        stroke.kind = DabBatchKind::Preview;
        stroke.stroke_end = false;
        render(&mut r, &[first, last], &[stroke.clone()], false);
        let predicted = r.readback_srgb_rgba8().unwrap();
        let maximum = predicted.iter().zip(&committed).map(|(a,b)|a.abs_diff(*b)).max().unwrap();
        assert!(maximum <= 1, "{mode:?} {preset:?}: predicted vs committed maximum error {maximum}");
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
