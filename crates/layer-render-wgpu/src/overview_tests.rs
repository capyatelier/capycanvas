//! In-surface overviews reuse scene pixels, without a preview target or readback.
use super::*;

fn target(r: &WgpuRasterizer, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("overview presentation test"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
fn placement() -> OverviewPlacement {
    OverviewPlacement {
        bounds: [8., 8., 48., 48.],
        work_area: [[18., 18.], [46., 18.], [46., 46.], [18., 46.]],
        outline_linear: [0.03, 0.04, 0.05],
        background_linear: [0.2, 0.3, 0.4],
        scale: 1.,
        opacity: 1.,
    }
}
fn pixel(bytes: &[u8], x: usize, y: usize) -> &[u8] {
    &bytes[(y * 128 + x) * 4..][..4]
}

#[test]
fn overview_presents_transparency_live_paint_and_camera_without_image_exports() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let layers = [Layer::paint(LayerId(1), "overview")];
    let red = dab([1., 0., 0., 0.5]);
    submit(&mut r, &layers, &[red], &[batch(1)], true);
    let revision = r.composite_revision;
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let target = target(&r, [128, 128], format);
        let surface = target.create_view(&Default::default());
        let mut presenter = ViewportPresenter::for_renderer(&r, format);
        presenter.present(&r, &surface, view(), [0.; 4]);
        let baseline = page_bytes(&r, &target);
        let inset = placement();
        presenter.set_overviews(&r, &[inset]);
        presenter.present(&r, &surface, view(), [0.; 4]);
        let rendered = page_bytes(&r, &target);
        for y in 0..128 {
            for x in 0..128 {
                if !(8..56).contains(&x) || !(8..56).contains(&y) {
                    assert_eq!(
                        pixel(&rendered, x, y),
                        pixel(&baseline, x, y),
                        "outside overview {x},{y}"
                    );
                }
            }
        }
        // Half-alpha red over the requested background, not black or the
        // camera's checkerboard. Allow two code values for RGBA8 quantization.
        for (actual, expected) in pixel(&rendered, 32, 32).iter().zip([203u8, 108, 124, 255]) {
            assert!(
                actual.abs_diff(expected) <= 2,
                "{format:?}: {:?}",
                pixel(&rendered, 32, 32)
            );
        }
        // The edge runs between pixel centers: this sample is 75% dark outline
        // and 25% white, blended in linear light before display conversion.
        assert!(
            pixel(&rendered, 18, 32)[0].abs_diff(142) <= 2,
            "antialiased work-area outline: {:?}",
            pixel(&rendered, 18, 32)
        );
        assert!(
            pixel(&rendered, 16, 32)[0] > pixel(&rendered, 14, 32)[0],
            "white outer contrast edge"
        );
        // The work-area outline follows a rotated/flipped camera. The document
        // image itself remains fixed inside the overview, even during panning.
        let moved = OverviewPlacement {
            work_area: [[32., 12.], [52., 32.], [32., 52.], [12., 32.]],
            ..inset
        };
        let camera = ViewState {
            document_to_surface: [-0.8, 0.3, 0.3, 0.8, 100., -24.],
            ..view()
        };
        presenter.set_overviews(&r, &[moved]);
        presenter.present(&r, &surface, camera, [0.; 4]);
        let moved_bytes = page_bytes(&r, &target);
        assert_eq!(pixel(&moved_bytes, 32, 32), pixel(&rendered, 32, 32));
        assert_ne!(pixel(&moved_bytes, 18, 32), pixel(&rendered, 18, 32));
        assert_eq!(
            r.composite_revision, revision,
            "camera cannot rebuild composition"
        );
        presenter.set_overviews(&r, &[]);
        presenter.present(&r, &surface, view(), [0.; 4]);
        assert_eq!(
            page_bytes(&r, &target),
            baseline,
            "closing overview restores canvas"
        );
        presenter.set_overviews(
            &r,
            &[OverviewPlacement {
                opacity: 0.,
                ..inset
            }],
        );
        presenter.present(&r, &surface, view(), [0.; 4]);
        assert_eq!(
            page_bytes(&r, &target),
            baseline,
            "hidden overview performs no visible work"
        );
        presenter.set_corner_radius(16.);
        presenter.set_overviews(&r, &[]);
        presenter.present(&r, &surface, view(), [0.; 4]);
        let rounded = page_bytes(&r, &target);
        presenter.set_overviews(
            &r,
            &[OverviewPlacement {
                bounds: [0., 0., 48., 48.],
                ..inset
            }],
        );
        presenter.present(&r, &surface, view(), [0.; 4]);
        assert_eq!(
            pixel(&page_bytes(&r, &target), 0, 0),
            [0, 0, 0, 0],
            "overview respects rounded window corners"
        );
        let with_overview = page_bytes(&r, &target);
        for y in 0..16 {
            for x in 0..16 {
                assert_eq!(
                    pixel(&with_overview, x, y)[3],
                    pixel(&rounded, x, y)[3],
                    "overview must not thicken antialiased window coverage {x},{y}"
                );
            }
        }
    }
    let target = target(&r, [128, 128], wgpu::TextureFormat::Rgba8Unorm);
    let surface = target.create_view(&Default::default());
    let mut presenter = ViewportPresenter::for_renderer(&r, wgpu::TextureFormat::Rgba8Unorm);
    presenter.set_overviews(
        &r,
        &[
            placement(),
            OverviewPlacement {
                bounds: [72., 8., 48., 48.],
                work_area: [[0.; 2]; 4],
                ..placement()
            },
        ],
    );
    presenter.present(&r, &surface, view(), [0.; 4]);
    let before = page_bytes(&r, &target);
    let blue = dab([0., 0., 1., 1.]);
    submit(
        &mut r,
        &layers,
        &[blue],
        &[DabBatch {
            stroke_id: StrokeId(2),
            ..batch(1)
        }],
        false,
    );
    presenter.present(&r, &surface, view(), [0.; 4]);
    let after = page_bytes(&r, &target);
    for x in [32, 96] {
        assert!(pixel(&before, x, 32)[0] > pixel(&before, x, 32)[2]);
        assert!(
            pixel(&after, x, 32)[2] > 240 && pixel(&after, x, 32)[0] < 10,
            "live overview {x}"
        );
    }
    assert_eq!(
        r.canvas_preview.storage_bytes(),
        0,
        "no preview image/readback allocation"
    );
    assert!(!r.canvas_preview_pending());
}

#[test]
#[ignore = "hardware presentation benchmark; run release and serial"]
fn in_surface_overview_latency() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let layers = [Layer::paint(LayerId(1), "overview latency")];
    let size = [2048, 1536];
    let camera = ViewState {
        width_px: size[0],
        height_px: size[1],
        ..view()
    };
    let mut paint = dab([0.6, 0.1, 0.3, 0.8]);
    paint.center = Point { x: 1024., y: 768. };
    paint.radii = [700., 600.];
    let mut b = batch(1);
    b.damage = Rect {
        min: Point { x: 0., y: 0. },
        max: Point { x: 2048., y: 1536. },
    };
    r.submit(FramePacket {
        view: camera,
        document_extent: size,
        layers: &layers,
        dabs: &[paint],
        dab_batches: &[b],
        reset_layers: true,
        composite_all: true,
        time_seconds: 0.,
    })
    .unwrap();
    let target = target(&r, size, wgpu::TextureFormat::Rgba8Unorm);
    let surface = target.create_view(&Default::default());
    let mut presenter = ViewportPresenter::for_renderer(&r, wgpu::TextureFormat::Rgba8Unorm);
    let mut timer = GpuFrameTimer::new(&r.device, &r.queue);
    assert!(
        r.device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
    );
    let summary = |mut values: Vec<f64>| {
        values.sort_by(f64::total_cmp);
        [values[60], values[114], values[118]]
    };
    for (label, count, width, moving) in [
        ("baseline", 0, 100., false),
        ("default", 1, 100., false),
        ("large", 1, 256., false),
        ("two", 2, 256., false),
        ("panning", 1, 256., true),
        ("baseline-repeat", 0, 100., false),
    ] {
        let mut cpu = Vec::new();
        let mut gpu = Vec::new();
        for i in 0..160 {
            let mut placements = [OverviewPlacement {
                bounds: [1700., 40., width, width * 0.75],
                work_area: [[1720., 60.], [1760., 60.], [1760., 100.], [1720., 100.]],
                ..placement()
            }; 2];
            placements[1].bounds[0] = 32.;
            if moving {
                for p in &mut placements[0].work_area {
                    p[0] += (i % 24) as f32;
                }
            }
            assert!(timer.begin(&r.device, &r.queue, i));
            let start = std::time::Instant::now();
            presenter.set_overviews(&r, &placements[..count]);
            presenter.present(
                &r,
                &surface,
                ViewState {
                    document_to_surface: [
                        1.,
                        0.,
                        0.,
                        1.,
                        if moving { (i % 24) as f32 } else { 0. },
                        0.,
                    ],
                    ..camera
                },
                [0.03, 0.03, 0.03, 1.],
            );
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            timer.end(&r.device, &r.queue);
            r.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(10)),
                })
                .unwrap();
            timer.poll(&r.device, &r.queue);
            r.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(10)),
                })
                .unwrap();
            let mut result = [GpuFrameSample::default(); 3];
            assert_eq!(timer.take_into(&mut result), 1);
            assert_eq!(result[0].status, 1);
            if i >= 40 {
                cpu.push(elapsed);
                gpu.push(result[0].elapsed_ns as f64 / 1e6);
            }
        }
        eprintln!(
            "overview {label}: CPU median/p95/p99={:.3?}ms GPU={:.3?}ms",
            summary(cpu),
            summary(gpu)
        );
    }
    assert_eq!(r.canvas_preview.storage_bytes(), 0);
}
