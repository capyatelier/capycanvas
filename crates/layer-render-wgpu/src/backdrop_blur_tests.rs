use super::*;
use crate::{ViewportPresenter, WgpuRasterizer};
use layer_core::{Layer, LayerId};
use layer_render::{CanvasRenderer, FramePacket, ViewState};

fn texture(r: &WgpuRasterizer, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("backdrop test surface"),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn stripes(r: &WgpuRasterizer, target: &wgpu::Texture) {
    let size = target.size();
    let bytes: Vec<u8> = (0..size.height)
        .flat_map(|_| (0..size.width).flat_map(|x| if (x / 4) % 2 == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 255] }))
        .collect();
    r.queue().write_texture(
        target.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.width * 4), rows_per_image: None },
        size,
    );
}

fn stripe(x: u32) -> [u8; 4] {
    if (x / 4) % 2 == 0 { [255; 4] } else { [0, 0, 0, 255] }
}

fn run(r: &WgpuRasterizer, blur: &mut BackdropBlur, surface: &wgpu::Texture, damage: Option<&[[u32; 4]]>) -> Vec<u8> {
    stripes(r, surface);
    let view = surface.create_view(&Default::default());
    let mut encoder = r.device().create_command_encoder(&Default::default());
    let size = surface.size();
    blur.encode(r, &mut encoder, &view, &view, [size.width, size.height], damage);
    r.queue().submit([encoder.finish()]);
    crate::layer_tests::page_bytes(r, surface)
}

#[test]
fn blur_writes_only_inside_rounded_regions() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let surface = texture(&r, [256, 128], wgpu::TextureFormat::Rgba8Unorm);
    let mut blur = BackdropBlur::new(r.device(), wgpu::TextureFormat::Rgba8Unorm);
    blur.set_regions(&[BackdropRegion { bounds: [64., 32., 128., 64.], radii: [16.; 4], shape: BackdropRegion::SQUIRCLE }]);
    let bytes = run(&r, &mut blur, &surface, None);
    let at = |x: u32, y: u32| &bytes[((y * 256 + x) * 4) as usize..][..4];
    for (x, y) in [(10, 10), (70, 10), (70, 100), (65, 33), (200, 60)] {
        assert_eq!(at(x, y), stripe(x), "{x},{y}");
    }
    for x in 90..150 {
        let p = at(x, 64);
        assert!(p[0].abs_diff(128) < 12 && p[3] == 255, "{x}: {p:?}");
    }
}

#[test]
fn concave_corners_leave_the_fillet_sharp() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let surface = texture(&r, [128, 128], wgpu::TextureFormat::Rgba8Unorm);
    let mut blur = BackdropBlur::new(r.device(), wgpu::TextureFormat::Rgba8Unorm);
    blur.set_regions(&[BackdropRegion { bounds: [32., 32., 32., 32.], radii: [-32., 0., 0., 0.], shape: BackdropRegion::SQUIRCLE }]);
    let bytes = run(&r, &mut blur, &surface, None);
    let at = |x: u32, y: u32| &bytes[((y * 128 + x) * 4) as usize..][..4];
    assert_eq!(at(36, 36), stripe(36), "inside the scooped disk");
    assert!(at(62, 62)[0].abs_diff(128) < 24, "the far corner is frosted");
}

#[test]
fn unchanged_backdrops_reuse_the_previous_blur() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let surface = texture(&r, [512, 256], wgpu::TextureFormat::Rgba8Unorm);
    let mut blur = BackdropBlur::new(r.device(), wgpu::TextureFormat::Rgba8Unorm);
    let region = |x: f32| BackdropRegion { bounds: [x, 16., 64., 64.], radii: [8.; 4], shape: BackdropRegion::SQUIRCLE };
    blur.set_regions(&[region(16.)]);
    run(&r, &mut blur, &surface, Some(&[]));
    assert_eq!(blur.frames(), [1, 0]);
    run(&r, &mut blur, &surface, Some(&[[400, 200, 410, 210]]));
    assert_eq!(blur.frames(), [1, 1], "distant paint keeps the blur");
    blur.set_regions(&[region(40.)]);
    run(&r, &mut blur, &surface, Some(&[]));
    assert_eq!(blur.frames(), [1, 2], "a short move stays inside the slack");
    run(&r, &mut blur, &surface, Some(&[[110, 20, 120, 30]]));
    assert_eq!(blur.frames(), [2, 2], "nearby paint recomputes");
    blur.set_regions(&[region(300.)]);
    run(&r, &mut blur, &surface, Some(&[]));
    assert_eq!(blur.frames(), [3, 2], "a long move recomputes");
    run(&r, &mut blur, &surface, None);
    assert_eq!(blur.frames(), [4, 2], "camera changes recompute");
}

fn measure(r: &WgpuRasterizer, iterations: u32, mut encode: impl FnMut(&mut wgpu::CommandEncoder)) -> Vec<f64> {
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
    let marker = texture(r, [1, 1], wgpu::TextureFormat::Rgba8Unorm).create_view(&Default::default());
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
        encode(&mut encoder);
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
        BackdropRegion { bounds: [x * scale, y * scale, w * scale, h * scale], radii: [r / 0.54 * scale; 4], shape: BackdropRegion::SQUIRCLE }
    })
    .to_vec()
}

#[test]
#[ignore = "hardware GPU timing: cargo test --release -p layer-render-wgpu backdrop_blur_cost -- --ignored --nocapture"]
fn backdrop_blur_cost() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    eprintln!("adapter: {:?}", r.adapter().get_info());
    let format = wgpu::TextureFormat::Rgba8Unorm;
    for (scale, extent) in [(1., [1600, 1000]), (2., [3200, 2000]), (1., [3840, 2160]), (2., [5120, 2880])] {
        let view = ViewState {
            width_px: extent[0],
            height_px: extent[1],
            document_to_surface: [0.8, 0.1, -0.1, 0.8, 200., 100.],
            background_rgba_linear: [0.5, 0.5, 0.5, 1.],
        };
        r.submit(FramePacket {
            view,
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
        let surface = texture(&r, extent, format);
        stripes(&r, &surface);
        let target = surface.create_view(&Default::default());
        let mut presenter = ViewportPresenter::for_renderer(&r, format);
        presenter.set_corner_radius(12. * scale);
        let iterations = std::env::var("LAYER_BLUR_BENCH_ITERATIONS").map_or(200, |v| v.parse().unwrap());
        let present = measure(&r, iterations, |encoder| {
            presenter.encode(&r, encoder, &target, view, [0.2, 0.2, 0.2, 1.]).unwrap();
        });
        let regions = layout(scale, extent);
        let area: f32 = regions.iter().map(|r| r.bounds[2] * r.bounds[3]).sum();
        for style in [
            BackdropBlurStyle { levels: 3, offset: 1.5 },
            BackdropBlurStyle { levels: 3, offset: 3. },
            BackdropBlurStyle { levels: 4, offset: 2. },
            BackdropBlurStyle { levels: 5, offset: 2. },
        ] {
            let mut blur = BackdropBlur::new(r.device(), format);
            blur.set_style(style);
            blur.set_regions(&regions);
            let mut cpu = Vec::new();
            let cost = measure(&r, iterations, |encoder| {
                let start = std::time::Instant::now();
                blur.encode(&r, encoder, &target, &target, extent, None);
                cpu.push(start.elapsed().as_secs_f64() * 1000.);
            });
            cpu.sort_by(f64::total_cmp);
            let cached = measure(&r, iterations, |encoder| {
                blur.encode(&r, encoder, &target, &target, extent, Some(&[]));
            });
            let work: u64 = blur.work().iter().map(|w| w.area()).sum();
            let median = |v: &[f64]| v[v.len() / 2];
            let p95 = |v: &[f64]| v[v.len() * 95 / 100];
            eprintln!(
                "{extent:?} @{scale}x panels {:.0}% work {:.0}% ({} rects) of surface, reach {}px, levels {} offset {}: blur CPU encode median {:.4} ms, GPU median {:.4} ms p95 {:.4} ms, cached GPU median {:.4} ms; viewport present median {:.4} ms p95 {:.4} ms",
                100. * area / (extent[0] * extent[1]) as f32,
                100. * work as f32 / (extent[0] * extent[1]) as f32,
                blur.work().len(),
                style.reach(),
                style.levels,
                style.offset,
                median(&cpu),
                median(&cost),
                p95(&cost),
                median(&cached),
                median(&present),
                p95(&present),
            );
        }
    }
}
