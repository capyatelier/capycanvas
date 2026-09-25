use super::*;
use crate::{ViewportPresenter, WgpuRasterizer};
use layer_core::{Layer, LayerId};
use layer_render::{CanvasRenderer, FramePacket, ViewState};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn texture(r: &WgpuRasterizer, size: [u32; 2]) -> wgpu::Texture {
    r.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("backdrop test surface"),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn view(size: [u32; 2], x: f32) -> ViewState {
    ViewState {
        width_px: size[0],
        height_px: size[1],
        document_to_surface: [1., 0., 0., 1., x, 0.],
        background_rgba_linear: [0.; 4],
    }
}

fn document(size: [u32; 2]) -> WgpuRasterizer {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    r.submit(FramePacket {
        view: view(size, 0.),
        document_extent: [size[0] / 2, size[1]],
        layers: &[Layer::paint(LayerId(1), "backdrop")],
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: true,
        time_seconds: 0.,
        composite_all: true,
    })
    .unwrap();
    r
}

fn present(r: &WgpuRasterizer, presenter: &mut ViewportPresenter, surface: &wgpu::Texture, x: f32) -> Vec<u8> {
    let size = [surface.width(), surface.height()];
    presenter.present(r, &surface.create_view(&Default::default()), view(size, x), [0., 0., 0., 1.]).unwrap();
    crate::layer_tests::page_bytes(r, surface)
}

fn region(bounds: [f32; 4], radii: [f32; 4]) -> BackdropRegion {
    BackdropRegion { bounds, radii, shape: BackdropRegion::SQUIRCLE }
}

#[test]
fn glass_blurs_only_inside_rounded_regions() {
    let size = [256, 128];
    let r = document(size);
    let surface = texture(&r, size);
    let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
    let raw = present(&r, &mut presenter, &surface, 0.);
    presenter.set_backdrop(&r, &[region([64., 32., 128., 64.], [16.; 4])], Default::default(), false);
    let glass = present(&r, &mut presenter, &surface, 0.);
    let at = |bytes: &[u8], x: u32, y: u32| bytes[((y * 256 + x) * 4) as usize..][..4].to_vec();
    for (x, y) in [(10, 10), (200, 10), (70, 100), (65, 33), (190, 94)] {
        assert_eq!(at(&glass, x, y), at(&raw, x, y), "{x},{y}");
    }
    let row: Vec<u8> = (70..186).map(|x| at(&glass, x, 64)[0]).collect();
    assert!(row.windows(2).all(|w| w[0].abs_diff(w[1]) < 12), "smooth across the document edge: {row:?}");
    assert!(at(&glass, 100, 64)[0] < at(&raw, 100, 64)[0] && at(&glass, 160, 64)[0] > 0);
    assert!((70..186).all(|x| at(&glass, x, 64)[3] == 255), "glass keeps the surface opaque");
}

#[test]
fn glass_larger_than_the_surface_fills_it() {
    let size = [256, 128];
    let r = document(size);
    let surface = texture(&r, size);
    let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
    let raw = present(&r, &mut presenter, &surface, 0.);
    presenter.set_backdrop(&r, &[region([-32., -32., 320., 192.], [16.; 4])], Default::default(), false);
    let glass = present(&r, &mut presenter, &surface, 0.);
    let row = |bytes: &[u8]| (16..64).map(|x| bytes[((64 * 256 + x) * 4) as usize]).collect::<Vec<u8>>();
    let spread = |row: &[u8]| row.iter().max().unwrap() - row.iter().min().unwrap();
    let (checkers, frosted) = (row(&raw), row(&glass));
    assert!(spread(&checkers) > 12, "the document is checkered: {checkers:?}");
    assert!(spread(&frosted) < 6 && frosted.iter().all(|v| *v > 200), "the checkers are frosted: {frosted:?}");
}

#[test]
fn concave_corners_leave_the_fillet_sharp() {
    let size = [256, 128];
    let r = document(size);
    let surface = texture(&r, size);
    let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
    let raw = present(&r, &mut presenter, &surface, 0.);
    presenter.set_backdrop(&r, &[region([96., 32., 64., 64.], [-32., 0., 0., 0.])], Default::default(), false);
    let glass = present(&r, &mut presenter, &surface, 0.);
    let at = |bytes: &[u8], x: u32, y: u32| bytes[((y * 256 + x) * 4) as usize];
    assert_eq!(at(&glass, 100, 36), at(&raw, 100, 36), "inside the scooped disk");
    assert!(at(&raw, 148, 92) == 0 && at(&glass, 148, 92) > 16, "the far corner is frosted");
}

#[test]
fn navigator_overviews_draw_above_the_glass() {
    let size = [256, 128];
    let r = document(size);
    let surface = texture(&r, size);
    let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
    presenter.prepare_overviews(&r);
    presenter.set_overviews(
        &r,
        &[crate::OverviewPlacement {
            bounds: [80., 40., 32., 32.],
            clip: None,
            work_area: [[0.; 2]; 4],
            outline_linear: [0.; 3],
            background_linear: [1., 0., 0.],
            scale: 1.,
            opacity: 1.,
        }],
    );
    presenter.set_backdrop(&r, &[region([64., 32., 128., 64.], [16.; 4])], Default::default(), false);
    let bytes = present(&r, &mut presenter, &surface, 0.);
    let pixel = &bytes[((56 * 256 + 96) * 4) as usize..][..4];
    assert!(pixel[0] > 150 && pixel[1] < 64, "overview background: {pixel:?}");
}

#[test]
fn cached_blur_is_reused_until_a_long_move() {
    let size = [512, 256];
    let r = document(size);
    let surface = texture(&r, size);
    let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
    let place = |presenter: &mut ViewportPresenter, x: f32| {
        presenter.set_backdrop(&r, &[region([x, 16., 64., 64.], [8.; 4])], Default::default(), false);
    };
    place(&mut presenter, 16.);
    present(&r, &mut presenter, &surface, 0.);
    assert_eq!(presenter.backdrop_frames(), [1, 0]);
    present(&r, &mut presenter, &surface, 0.);
    assert_eq!(presenter.backdrop_frames(), [1, 1], "an unchanged frame keeps the blur");
    place(&mut presenter, 40.);
    present(&r, &mut presenter, &surface, 0.);
    assert_eq!(presenter.backdrop_frames(), [1, 2], "a short move stays inside the slack");
    place(&mut presenter, 300.);
    present(&r, &mut presenter, &surface, 0.);
    assert_eq!(presenter.backdrop_frames(), [2, 2], "a long move recomputes");
}

#[test]
fn camera_motion_moves_the_cached_blur_between_refreshes() {
    let size = [512, 256];
    let r = document(size);
    let surface = texture(&r, size);
    let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
    presenter.set_backdrop(&r, &[region([200., 16., 64., 64.], [8.; 4])], Default::default(), false);
    present(&r, &mut presenter, &surface, 0.);
    for (x, frames) in [(4., [1, 1]), (8., [1, 2]), (12., [1, 3]), (16., [2, 3]), (20., [2, 4])] {
        present(&r, &mut presenter, &surface, x);
        assert_eq!(presenter.backdrop_frames(), frames, "camera frames reuse the moved blur and refresh every fourth at {x}");
    }
    present(&r, &mut presenter, &surface, 20.);
    assert_eq!(presenter.backdrop_frames(), [3, 4], "the first still frame recomputes the exact blur");
    present(&r, &mut presenter, &surface, 20.);
    assert_eq!(presenter.backdrop_frames(), [3, 5]);
    present(&r, &mut presenter, &surface, 180.);
    assert_eq!(presenter.backdrop_frames(), [4, 5], "a pan beyond the cached slack recomputes");
}

#[test]
fn strokes_after_a_gesture_keep_the_moved_blur_until_they_end() {
    let size = [256, 128];
    let r = document(size);
    let surface = texture(&r, size);
    let glass = [region([140., 32., 96., 64.], [12.; 4])];
    let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
    presenter.set_backdrop(&r, &glass, Default::default(), false);
    present(&r, &mut presenter, &surface, 0.);
    let moved = present(&r, &mut presenter, &surface, 6.);
    presenter.set_backdrop(&r, &glass, Default::default(), true);
    let stroke = present(&r, &mut presenter, &surface, 6.);
    assert_eq!(presenter.backdrop_frames(), [1, 2], "a stroke keeps the moved blur");
    assert_eq!(stroke, moved);
    presenter.set_backdrop(&r, &glass, Default::default(), false);
    present(&r, &mut presenter, &surface, 6.);
    assert_eq!(presenter.backdrop_frames(), [2, 2], "the stroke's end recomputes the exact blur");
}

#[test]
fn retained_targets_repaint_only_changed_glass() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let mut blur = BackdropBlur::new(r.device(), FORMAT);
    let frame = |blur: &mut BackdropBlur, damage: Option<&[PixelRect]>| {
        let mut encoder = r.device().create_command_encoder(&Default::default());
        let mut repaint = Vec::new();
        blur.encode(&r, &mut encoder, [512, 256], 0, layer_core::Affine::IDENTITY, false, damage, |_| {}, None, &mut repaint);
        r.queue().submit([encoder.finish()]);
        repaint
    };
    let bounds = PixelRect::new(16, 16, 80, 80);
    blur.set_regions(&[region([16., 16., 64., 64.], [8.; 4])]);
    assert_eq!(frame(&mut blur, None), [bounds]);
    assert_eq!(frame(&mut blur, Some(&[])), []);
    assert_eq!(frame(&mut blur, Some(&[PixelRect::new(400, 200, 410, 210)])), [], "distant paint");
    assert_eq!(blur.frames(), [1, 2]);
    let reach = blur.style.reach();
    assert_eq!(
        frame(&mut blur, Some(&[PixelRect::new(110, 20, 120, 30)])),
        [PixelRect::new(110 - reach, 16, 80, 80)],
        "nearby paint repaints only its blurred neighborhood"
    );
    assert_eq!(blur.frames(), [2, 2]);
    blur.set_regions(&[region([40., 16., 64., 64.], [8.; 4])]);
    assert_eq!(frame(&mut blur, Some(&[])), [bounds, PixelRect::new(40, 16, 104, 80)], "moves repaint both places");
    assert_eq!(blur.frames(), [2, 3]);
    blur.set_regions(&[]);
    assert_eq!(frame(&mut blur, Some(&[])), [PixelRect::new(40, 16, 104, 80)], "removed glass repaints the artwork");
}

fn paint_near_glass(r: &mut WgpuRasterizer, size: [u32; 2]) {
    let mut dab = crate::layer_tests::dab([0., 0., 0., 1.]);
    dab.center = layer_core::Point { x: 110., y: 64. };
    dab.radii = [8.; 2];
    let mut batch = crate::layer_tests::batch(1);
    batch.damage = layer_core::Rect { min: layer_core::Point { x: 100., y: 54. }, max: layer_core::Point { x: 120., y: 74. } };
    r.submit(FramePacket {
        view: view(size, 0.),
        document_extent: [size[0] / 2, size[1]],
        layers: &[Layer::paint(LayerId(1), "backdrop")],
        dabs: &[dab],
        dab_batches: &[batch],
        restore_rasters: &[],
        reset_layers: false,
        time_seconds: 0.,
        composite_all: false,
    })
    .unwrap();
}

#[test]
fn local_refresh_matches_a_full_blur() {
    let size = [256, 128];
    let glass = [region([140., 32., 96., 64.], [12.; 4])];
    for style in [BackdropBlurStyle { levels: 3, offset: 2.9 }, BackdropBlurStyle::default(), BackdropBlurStyle { levels: 4, offset: 2.5 }] {
        let mut r = document(size);
        let surface = texture(&r, size);
        let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
        presenter.set_backdrop(&r, &glass, style, false);
        let before = present(&r, &mut presenter, &surface, 0.);
        paint_near_glass(&mut r, size);
        let local = present(&r, &mut presenter, &surface, 0.);
        assert_eq!(presenter.backdrop_frames(), [2, 0], "paint within reach refreshes the cached blur");
        let mut fresh = ViewportPresenter::for_renderer(&r, FORMAT);
        fresh.set_backdrop(&r, &glass, style, false);
        let full = present(&r, &mut fresh, &surface, 0.);
        let worst = local.iter().zip(&full).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 1, "{style:?}: local refresh differs from a full blur by {worst}");
        let at = |bytes: &[u8]| bytes[((64 * 256 + 144) * 4) as usize];
        assert!(at(&local) < at(&before), "{style:?}: the nearby dab darkens the glass");
    }
}

#[test]
fn strokes_keep_the_cached_glass_until_they_end() {
    let size = [256, 128];
    let mut r = document(size);
    let surface = texture(&r, size);
    let glass = [region([140., 32., 96., 64.], [12.; 4])];
    let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
    presenter.set_backdrop(&r, &glass, Default::default(), false);
    let before = present(&r, &mut presenter, &surface, 0.);
    paint_near_glass(&mut r, size);
    presenter.set_backdrop(&r, &glass, Default::default(), true);
    let during = present(&r, &mut presenter, &surface, 0.);
    assert_eq!(presenter.backdrop_frames(), [1, 1], "a stroke keeps the cached blur");
    let at = |bytes: &[u8]| bytes[((64 * 256 + 150) * 4) as usize];
    assert_eq!(at(&during), at(&before));
    presenter.set_backdrop(&r, &glass, Default::default(), false);
    let after = present(&r, &mut presenter, &surface, 0.);
    assert_eq!(presenter.backdrop_frames(), [2, 1], "the stroke's end refreshes the glass");
    let mut fresh = ViewportPresenter::for_renderer(&r, FORMAT);
    fresh.set_backdrop(&r, &glass, Default::default(), false);
    let full = present(&r, &mut fresh, &surface, 0.);
    let worst = after.iter().zip(&full).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    assert!(worst <= 1, "the held refresh differs from a full blur by {worst}");
}

#[test]
fn regions_follow_the_surface_rotation() {
    let logical = region([10., 5., 20., 10.], [1., 2., 3., 4.]);
    let size = [100., 50.];
    assert_eq!(logical.rotated(0, size), logical);
    assert_eq!(logical.rotated(1, size), region([35., 10., 10., 20.], [4., 1., 2., 3.]));
    assert_eq!(logical.rotated(2, size), region([70., 35., 20., 10.], [3., 4., 1., 2.]));
    assert_eq!(logical.rotated(3, size), region([5., 70., 10., 20.], [2., 3., 4., 1.]));
}

fn measure(r: &WgpuRasterizer, iterations: u32, mut encode: impl FnMut(&mut wgpu::CommandEncoder, u32)) -> Vec<f64> {
    let queries = r.device().create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("backdrop timing"),
        ty: wgpu::QueryType::Timestamp,
        count: 2 * iterations,
    });
    let resolve = r.device().create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16 * iterations as u64,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = r.device().create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16 * iterations as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let marker = texture(r, [1, 1]).create_view(&Default::default());
    let stamp = |encoder: &mut wgpu::CommandEncoder, index: u32, begin: bool| {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &marker,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
                query_set: &queries,
                beginning_of_pass_write_index: begin.then_some(index),
                end_of_pass_write_index: (!begin).then_some(index),
            }),
            occlusion_query_set: None,
            multiview_mask: None,
        });
    };
    for i in 0..iterations {
        let mut encoder = r.device().create_command_encoder(&Default::default());
        stamp(&mut encoder, 2 * i, true);
        encode(&mut encoder, i);
        stamp(&mut encoder, 2 * i + 1, false);
        r.queue().submit([encoder.finish()]);
    }
    let mut encoder = r.device().create_command_encoder(&Default::default());
    encoder.resolve_query_set(&queries, 0..2 * iterations, &resolve, 0);
    encoder.copy_buffer_to_buffer(&resolve, 0, &readback, 0, 16 * iterations as u64);
    r.queue().submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    r.device().poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
    let period = r.queue().get_timestamp_period() as f64;
    let data = readback.slice(..).get_mapped_range().unwrap();
    let stamps: Vec<u64> = data.chunks_exact(8).map(|c| u64::from_ne_bytes(c.try_into().unwrap())).collect();
    let mut ms: Vec<f64> = stamps.chunks_exact(2).skip((iterations as usize / 10).min(10)).map(|p| (p[1] - p[0]) as f64 * period / 1e6).collect();
    ms.sort_by(f64::total_cmp);
    ms
}

fn layout(scale: f32, extent: [u32; 2]) -> Vec<BackdropRegion> {
    let [sx, sy] = [extent[0] as f32 / scale / 1600., extent[1] as f32 / scale / 1000.];
    [
        [1510., 7., 36., 34., 9.72], [6., 7., 36., 34., 9.72], [1564., 12., 24., 24., 12.],
        [48., 7., 448., 34., 9.18], [710., 6., 180., 36., 9.72], [1360., 6., 144., 36., 9.72],
        [1243., 970., 87., 24., 6.48], [6., 48., 36., 946., 9.72], [48., 48., 242., 432., 9.72],
        [48., 485.6, 242., 180., 9.72], [48., 671.5, 242., 323., 9.72], [1340., 48., 254., 232., 9.72],
        [1340., 286.3, 254., 279., 9.72], [1340., 571., 254., 423., 9.72], [296., 48., 1038., 36., 9.72],
    ]
    .map(|[x, y, w, h, r]| {
        let right = x > 800.;
        let bottom = y > 500.;
        let x = if right { x + 1600. * (sx - 1.) } else { x };
        let y = if bottom { y + 1000. * (sy - 1.) } else { y };
        let h = if h > 400. { h + 1000. * (sy - 1.) } else { h };
        let w = if w > 1000. { w + 1600. * (sx - 1.) } else { w };
        region([x * scale, y * scale, w * scale, h * scale], [r / 0.54 * scale; 4])
    })
    .to_vec()
}

#[test]
#[ignore = "hardware GPU timing: cargo test --release -p layer-render-wgpu backdrop_blur_cost -- --ignored --nocapture"]
fn backdrop_blur_cost() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    eprintln!("adapter: {:?}", r.adapter().get_info());
    let iterations = std::env::var("LAYER_BLUR_BENCH_ITERATIONS").map_or(200, |v| v.parse().unwrap());
    for (scale, extent) in [(1., [1600, 1000]), (2., [3200, 2000]), (1., [3840, 2160]), (2., [5120, 2880])] {
        let camera = |x: f32| ViewState {
            width_px: extent[0],
            height_px: extent[1],
            document_to_surface: [0.8, 0.1, -0.1, 0.8, x, 100.],
            background_rgba_linear: [0.5, 0.5, 0.5, 1.],
        };
        r.submit(FramePacket {
            view: camera(200.),
            document_extent: [4096, 3072],
            layers: &[Layer::paint(LayerId(1), "bench")],
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &[],
            reset_layers: true,
            time_seconds: 0.,
            composite_all: true,
        })
        .unwrap();
        let target = texture(&r, extent).create_view(&Default::default());
        let surround = [0.2, 0.2, 0.2, 1.];
        let median = |v: &[f64]| v[v.len() / 2];
        let p95 = |v: &[f64]| v[v.len() * 95 / 100];
        let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
        let present = measure(&r, iterations, |encoder, i| {
            presenter.encode(&r, encoder, &target, camera(200. + (i % 2) as f32), surround).unwrap();
        });
        let regions = layout(scale, extent);
        let area: f32 = regions.iter().map(|r| r.bounds[2] * r.bounds[3]).sum();
        for style in [BackdropBlurStyle { levels: 3, offset: 2.9 }, BackdropBlurStyle { levels: 3, offset: 3.4 }, BackdropBlurStyle { levels: 4, offset: 2.5 }] {
            let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
            presenter.set_backdrop(&r, &regions, style, false);
            let mut cpu = Vec::new();
            let moving = measure(&r, iterations, |encoder, i| {
                let start = std::time::Instant::now();
                presenter.encode(&r, encoder, &target, camera(200. + 400. * (i % 2) as f32), surround).unwrap();
                cpu.push(start.elapsed().as_secs_f64() * 1000.);
            });
            cpu.sort_by(f64::total_cmp);
            let moved = measure(&r, iterations, |encoder, i| {
                presenter.encode(&r, encoder, &target, camera(200. + i as f32), surround).unwrap();
            });
            let cached = measure(&r, iterations, |encoder, _| {
                presenter.encode(&r, encoder, &target, camera(200.), surround).unwrap();
            });
            eprintln!(
                "{extent:?} @{scale}x glass {:.0}% of surface, reach {}px, levels {} offset {}: viewport median {:.4} ms p95 {:.4}; with glass, recomputing camera median {:.4} ms p95 {:.4} (CPU encode {:.4} ms), moved blur median {:.4} ms, cached median {:.4} ms p95 {:.4}",
                100. * area / (extent[0] * extent[1]) as f32,
                style.reach(),
                style.levels,
                style.offset,
                median(&present),
                p95(&present),
                median(&moving),
                p95(&moving),
                median(&cpu),
                median(&moved),
                median(&cached),
                p95(&cached),
            );
        }
    }
}

#[test]
fn reprojected_glass_tracks_the_camera() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut dabs = Vec::new();
    let mut seed = 7u32;
    let mut next = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    for _ in 0..200 {
        let mut dab = crate::layer_tests::dab([next(), next(), next(), 1.]);
        dab.center = layer_core::Point { x: next() * 512., y: next() * 256. };
        dab.radii = [3. + next() * 20.; 2];
        dabs.push(dab);
    }
    let mut batch = crate::layer_tests::batch(1);
    batch.dab_count = dabs.len() as u32;
    batch.damage = layer_core::Rect { min: layer_core::Point { x: 0., y: 0. }, max: layer_core::Point { x: 512., y: 256. } };
    let view = |matrix: [f32; 6]| ViewState { width_px: 512, height_px: 256, document_to_surface: matrix, background_rgba_linear: [0.; 4] };
    r.submit(FramePacket {
        view: view([1., 0., 0., 1., 0., 0.]),
        document_extent: [512, 256],
        layers: &[Layer::paint(LayerId(1), "reprojection")],
        dabs: &dabs,
        dab_batches: &[batch],
        restore_rasters: &[],
        reset_layers: true,
        time_seconds: 0.,
        composite_all: true,
    })
    .unwrap();
    let glass = [region([40., 40., 160., 176.], [12.; 4]), region([320., 30., 150., 190.], [12.; 4])];
    for turns in 0..4 {
        let size = if turns % 2 == 0 { [512, 256] } else { [256, 512] };
        let surface = texture(&r, size);
        let present = |presenter: &mut ViewportPresenter, matrix| {
            presenter.present(&r, &surface.create_view(&Default::default()), view(matrix), [0.2, 0.2, 0.2, 1.]).unwrap();
            crate::layer_tests::page_bytes(&r, &surface)
        };
        for (motion, moved) in [("pan", [1.2, 0., 0., 1.2, -22.7, -31.4]), ("zoom", [1.236, 0., 0., 1.236, -47.7, -23.8])] {
            let mut presenter = ViewportPresenter::for_renderer(&r, FORMAT);
            presenter.set_surface_rotation(turns);
            presenter.set_backdrop(&r, &glass, BackdropBlurStyle::default(), false);
            present(&mut presenter, [1.2, 0., 0., 1.2, -40., -20.]);
            present(&mut presenter, [1.2, 0., 0., 1.2, -41., -20.]);
            let reused = present(&mut presenter, moved);
            assert_eq!(presenter.backdrop_frames(), [1, 2], "turns {turns} {motion}");
            let mut fresh = ViewportPresenter::for_renderer(&r, FORMAT);
            fresh.set_surface_rotation(turns);
            fresh.set_backdrop(&r, &glass, BackdropBlurStyle::default(), false);
            let full = present(&mut fresh, moved);
            let mut diffs = Vec::new();
            for g in &glass {
                let [x, y, w, h] = g.rotated(turns, [512., 256.]).bounds.map(|v| v as u32);
                for py in y + 12..y + h - 12 {
                    for px in x + 12..x + w - 12 {
                        let i = ((py * size[0] + px) * 4) as usize;
                        diffs.push((0..3).map(|c| reused[i + c].abs_diff(full[i + c])).max().unwrap());
                    }
                }
            }
            let mean = diffs.iter().map(|d| f64::from(*d)).sum::<f64>() / diffs.len() as f64;
            let worst = diffs.iter().max().unwrap();
            assert!(mean < 3. && *worst < 20, "turns {turns} {motion}: moved blur differs from a fresh blur by {mean:.2} on average, {worst} at worst");
        }
    }
}
