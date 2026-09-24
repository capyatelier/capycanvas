use super::*;
use layer_render::{SelectionPaint, SelectionPaintMode};

fn request(id: u64, before: Selection, mode: SelectionPaintMode, opacity: f32) -> SelectionPaint {
    let mut contact = dab([1.; 4]);
    contact.radii = [12.; 2];
    contact.flow = 1.;
    contact.hardness = 1.;
    SelectionPaint {
        id,
        before: Arc::new(before),
        mode,
        opacity,
        gray: 0.5,
        gradient: None,
        style: crate::tests::test_style(BrushExecution::Dry),
        dabs: vec![contact],
        enclosed: None,
        finish: false,
        restart: false,
    }
}
fn receive(r: &mut WgpuRasterizer, mut request: SelectionPaint) -> Selection {
    request.dabs.clear();
    request.enclosed = None;
    request.finish = true;
    assert!(r.paint_selection(&request).unwrap());
    let until = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        if let Some(result) = r.take_selection_paint() {
            return Selection::pixels(result.unwrap().pixels);
        }
        assert!(
            std::time::Instant::now() < until,
            "selection paint capture timed out"
        );
        std::thread::yield_now();
    }
}
fn value(s: &Selection, x: u32, y: u32) -> u8 {
    let layer_core::SelectionShape::Pixels(p) = &s.shape else {
        panic!("pixels")
    };
    ((p.words()[(y * p.extent()[0].div_ceil(4) + x / 4) as usize] >> ((x % 4) * 8)) & 255) as u8
}
fn renderer() -> WgpuRasterizer {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    submit(
        &mut r,
        &[Layer::paint(LayerId(1), "artwork")],
        &[],
        &[],
        true,
    );
    r
}

#[test]
fn selection_paint_opacity_accumulates_between_contacts_but_not_within_one() {
    let mut r = renderer();
    let first = request(1, Selection::empty(), SelectionPaintMode::Add, 0.5);
    assert!(r.paint_selection(&first).unwrap());
    assert!(r.paint_selection(&first).unwrap());
    let first = receive(&mut r, first);
    assert_eq!(value(&first, 64, 64), 128);
    assert_eq!(value(&first, 2, 2), 0);
    let second = request(2, first, SelectionPaintMode::Add, 0.5);
    assert!(r.paint_selection(&second).unwrap());
    let second = receive(&mut r, second);
    assert_eq!(value(&second, 64, 64), 192);
    let subtract = request(3, second, SelectionPaintMode::Subtract, 0.5);
    assert!(r.paint_selection(&subtract).unwrap());
    let subtract = receive(&mut r, subtract);
    assert_eq!(value(&subtract, 64, 64), 96);
}

#[test]
fn selection_paint_gray_uses_float_working_coverage_and_cancel_discards_preview() {
    let mut r = renderer();
    let mut paint = request(1, Selection::full(), SelectionPaintMode::Gray, 0.5);
    paint.gray = 0.;
    assert!(r.paint_selection(&paint).unwrap());
    assert!(r.paint_selection(&paint).unwrap());
    let selected = receive(&mut r, paint);
    assert_eq!(value(&selected, 64, 64), 64);
    assert_eq!(value(&selected, 2, 2), 255);
    let paint = request(2, selected, SelectionPaintMode::Gray, 1.);
    assert!(r.paint_selection(&paint).unwrap());
    r.cancel_selection_paint();
    assert!(r.display_selection.is_none());
    assert!(r.take_selection_paint().is_none());
    let paint = request(3, Selection::empty(), SelectionPaintMode::Gray, 1.);
    assert!(r.paint_selection(&paint).unwrap());
    assert_eq!(value(&receive(&mut r, paint), 64, 64), 128);
}

#[test]
fn selection_paint_coherent_brush_sweeps_between_contacts_and_respects_uniform_opacity() {
    let mut r = renderer();
    let mut paint = request(1, Selection::full(), SelectionPaintMode::Gray, 0.5);
    paint.style = preset_style(layer_core::DefaultBrushPreset::GPen);
    paint.gray = 0.;
    let d = &mut paint.dabs[0];
    d.center = Point { x: 100., y: 64. };
    d.radii = [4.; 2];
    d.motion = [72., 0.];
    d.previous = [4., 4., 1., 0.];
    d.contact = [1., 0., 0., 0.];
    d.previous_contact = d.contact;
    assert!(r.paint_selection(&paint).unwrap());
    let first = receive(&mut r, paint.clone());
    assert!(
        value(&first, 64, 64) < 255,
        "the real swept segment is covered"
    );
    paint.id = 2;
    assert!(r.paint_selection(&paint).unwrap());
    assert!(r.paint_selection(&paint).unwrap());
    let repeated = receive(&mut r, paint);
    assert_eq!(
        value(&first, 64, 64),
        value(&repeated, 64, 64),
        "uniform dry paint does not build opacity within a contact"
    );
}

#[test]
fn selection_paint_gradient_blends_scalar_values_and_transparency() {
    let mut r = renderer();
    let mut paint = request(1, Selection::full(), SelectionPaintMode::Gray, 1.);
    paint.dabs.clear();
    paint.gray = 0.;
    paint.enclosed = Some(Arc::new(Selection::full()));
    paint.gradient = Some(layer_render::SelectionGradient {
        start: Point { x: 0., y: 64. },
        end: Point { x: 128., y: 64. },
        background: 1.,
        radial: false,
        transparent: false,
    });
    assert!(r.paint_selection(&paint).unwrap());
    let s = receive(&mut r, paint.clone());
    assert!((126..=130).contains(&value(&s, 64, 64)));
    assert!(value(&s, 0, 64) < 3 && value(&s, 127, 64) > 252);
    paint.id = 2;
    paint.before = Arc::new(Selection::pixels(match &s.shape {
        layer_core::SelectionShape::Pixels(p) => p.clone(),
        _ => unreachable!(),
    }));
    paint.gray = 1.;
    paint.gradient.as_mut().unwrap().transparent = true;
    assert!(r.paint_selection(&paint).unwrap());
    let s = receive(&mut r, paint);
    assert!((189..=194).contains(&value(&s, 64, 64)));
}

#[test]
fn selection_paint_loads_raw_alpha_and_disabled_mask_with_independent_placement() {
    use layer_core::{Affine, LayerMask};
    use layer_render::{RegionRequest, RegionSource, SelectionRefinement};
    let mut r = renderer();
    let mut layer = Layer::paint(LayerId(1), "hidden source");
    layer.visible = false;
    layer.opacity = 0.2;
    let mut mask = LayerMask::reveal_all(LayerId(3), Point::default());
    mask.default_coverage = 0.25;
    mask.enabled = false;
    mask.inverted = true;
    layer.mask = Some(mask);
    let mut d = dab([1., 1., 1., 0.5]);
    d.flow = 1.;
    d.hardness = 1.;
    let batch = batch(1);
    let mut initial_layer = Layer::paint(LayerId(2), "hidden initial mask");
    initial_layer.visible = false;
    let mut initial = LayerMask::reveal_all(LayerId(4), Point::default());
    initial.enabled = false;
    initial.default_coverage = 0.;
    initial.initial = Some(
        Selection::polygon(vec![
            Point { x: 32., y: 32. },
            Point { x: 96., y: 32. },
            Point { x: 96., y: 96. },
            Point { x: 32., y: 96. },
        ])
        .unwrap(),
    );
    initial_layer.mask = Some(initial);
    submit(&mut r, &[layer, initial_layer], &[d], &[batch], true);
    let mut load = |id, offset| {
        assert!(
            r.request_region(RegionRequest {
                request_id: id,
                source: RegionSource::Coverage(LayerId(id)),
                contiguous: false,
                position: [0, 0],
                tolerance: 0.,
                refinement: Default::default(),
                limit: None,
                selection: Some(SelectionRefinement {
                        resize: 0,
                    mode: layer_core::SelectionMode::New,
                    previous: None,
                    antialias: true,
                    feather: 0.,
                    source_to_document: Affine::translation(Point { x: offset, y: 0. })
                }),
            })
            .unwrap()
        );
        let until = std::time::Instant::now() + READBACK_TIMEOUT;
        loop {
            if let Some(result) = r.take_region() {
                break Selection::pixels(result.unwrap().pixels);
            }
            assert!(std::time::Instant::now() < until);
            std::thread::yield_now();
        }
    };
    let alpha = load(1, 10.);
    assert!(
        (126..=129).contains(&value(&alpha, 74, 64)),
        "layer opacity, visibility and attached mask do not alter raw alpha"
    );
    assert_eq!(value(&alpha, 2, 2), 0);
    let mask = load(3, 0.);
    assert!(
        (190..=193).contains(&value(&mask, 2, 2)),
        "disabled inverted mask loads its own scalar coverage"
    );
    assert_eq!(value(&mask, 2, 2), value(&mask, 64, 64));
    let initial = load(4, 0.);
    assert_eq!(value(&initial, 2, 2), 0);
    assert_eq!(value(&initial, 64, 64), 255);
}

#[test]
fn selection_paint_enclosed_area_and_footprint_share_one_opacity_application() {
    let mut r = renderer();
    let mut paint = request(1, Selection::empty(), SelectionPaintMode::Add, 0.5);
    paint.enclosed = Some(Arc::new(
        Selection::polygon(vec![
            Point { x: 40., y: 40. },
            Point { x: 90., y: 40. },
            Point { x: 90., y: 90. },
            Point { x: 40., y: 90. },
        ])
        .unwrap(),
    ));
    assert!(r.paint_selection(&paint).unwrap());
    let selected = receive(&mut r, paint);
    assert_eq!(value(&selected, 64, 64), 128);
    assert_eq!(value(&selected, 42, 42), 128);
    assert_eq!(value(&selected, 10, 10), 0);
    assert!(r.selection_painter.as_ref().unwrap().active.is_none());
}

#[test]
fn selection_paint_crosses_pages_and_contact_blocks_with_partial_last_words() {
    let mut r = renderer();
    r.document_extent = [513, 257];
    let mut paint = request(1, Selection::full(), SelectionPaintMode::Subtract, 0.5);
    let mut contact = paint.dabs[0];
    contact.center = Point { x: 256., y: 256. };
    contact.radii = [8.; 2];
    paint.dabs = vec![contact; 513];
    assert!(r.paint_selection(&paint).unwrap());
    assert!(r.selection_painter.as_ref().unwrap().storage_bytes() >= 4 * 256 * 256 * 4);
    let result = receive(&mut r, paint);
    for (x, y) in [(255, 255), (256, 255), (255, 256), (256, 256)] {
        assert_eq!(value(&result, x, y), 128);
    }
    assert_eq!(value(&result, 512, 256), 255);
    assert_eq!(value(&result, 0, 0), 255);
}

#[test]
fn selection_paint_soft_tip_keeps_faint_coverage_until_final_quantization() {
    let mut r = renderer();
    let mut paint = request(1, Selection::empty(), SelectionPaintMode::Add, 0.01);
    paint.dabs[0].hardness = 0.;
    assert!(r.paint_selection(&paint).unwrap());
    let first = receive(&mut r, paint);
    assert!((1..=3).contains(&value(&first, 64, 64)));
    assert!(value(&first, 70, 64) < value(&first, 64, 64));
    assert_eq!(value(&first, 90, 90), 0);
}

#[test]
fn selection_paint_reports_unchanged_coverage_for_zero_opacity_and_saturated_strokes() {
    let mut r = renderer();
    for (id, before, opacity) in [(1, Selection::empty(), 0.), (2, Selection::full(), 1.)] {
        let mut paint = request(id, before, SelectionPaintMode::Add, opacity);
        paint.finish = true;
        assert!(r.paint_selection(&paint).unwrap());
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        loop {
            if let Some(result) = r.take_selection_paint() {
                assert!(!result.unwrap().changed);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}

#[test]
fn selection_paint_overlay_is_coverage_scaled_and_excluded_from_artwork() {
    let mut r = renderer();
    let artwork = r.readback_srgb_rgba8().unwrap();
    let paint = request(1, Selection::empty(), SelectionPaintMode::Add, 0.5);
    assert!(r.paint_selection(&paint).unwrap());
    r.set_selection_overlay(Some(layer_render::SelectionOverlay {
        active: true,
        editing: None,
        color: [1., 0., 0., 0.5],
        protected: false,
    }));
    let target = r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("selection overlay reference"),
        size: wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut presenter = crate::ViewportPresenter::for_surface(
        &r,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        crate::SdrSurfaceColor::Srgb,
    )
    .unwrap();
    presenter
        .present(
            &r,
            &target.create_view(&Default::default()),
            ViewState {
                background_rgba_linear: [1.; 4],
                ..view()
            },
            [1.; 4],
        )
        .unwrap();
    let pixels = page_bytes(&r, &target);
    let center = &pixels[(64 * 128 + 64) * 4..][..4];
    assert!(
        center[0] > center[1] + 15 && center[1] > 180,
        "soft overlay {center:?}"
    );
    r.set_selection_overlay(None);
    presenter
        .present(
            &r,
            &target.create_view(&Default::default()),
            ViewState {
                background_rgba_linear: [1.; 4],
                ..view()
            },
            [1.; 4],
        )
        .unwrap();
    let baseline = page_bytes(&r, &target);
    assert_eq!(
        &pixels[(10 * 128 + 10) * 4..][..4],
        &baseline[(10 * 128 + 10) * 4..][..4]
    );
    assert_eq!(r.readback_srgb_rgba8().unwrap(), artwork);
}

#[test]
#[ignore = "hardware selection painting timing; serial release run"]
fn selection_paint_latency() {
    use layer_engine::{PenEvent, PenPhase, PressureCurve, SampleFlags, SelectionStroke, ToolKind, ViewTransform};
    use std::time::Instant;
    let mut r = renderer();
    r.document_extent = [4096, 4096];
    for diameter in [64., 800.] {
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        brush.diameter = diameter;
        let mut stroke = SelectionStroke::new(diameter as u64, brush, ViewTransform::IDENTITY, PressureCurve::default(), None);
        let mut paint = request(diameter as u64, Selection::empty(), SelectionPaintMode::Gray, 0.5);
        paint.style = stroke.style();
        let mut submit = Vec::new();
        let mut complete = Vec::new();
        for i in 0..120 {
            paint.dabs.clear();
            let start = Instant::now();
            for j in 0..8 {
                let n = i*8+j;
                stroke.push(PenEvent {
                    device_id: 1, sequence: n, timestamp_ns: n*1_000_000, view_revision: 0,
                    surface_position: Point { x: 600.+n as f32*2., y: 1000. }, pressure: 0.7,
                    tilt_radians: [0.;2], twist_radians: 0., distance: 0.,
                    phase: if n==0 { PenPhase::Down } else { PenPhase::Move },
                    tool: ToolKind::Pen, flags: SampleFlags::PRIMARY,
                }, &mut paint.dabs);
            }
            r.paint_selection(&paint).unwrap();
            submit.push(start.elapsed().as_secs_f64()*1000.);
            r.wait_idle().unwrap();
            complete.push(start.elapsed().as_secs_f64()*1000.);
        }
        submit.sort_by(f64::total_cmp);
        complete.sort_by(f64::total_cmp);
        eprintln!("selection G-Pen {diameter}px, 8 real samples/update on 4096²: p50/p95 sample+submit_ms={:.3}/{:.3}, complete_ms={:.3}/{:.3}",submit[60],submit[114],complete[60],complete[114]);
        let start = Instant::now();
        let _ = receive(&mut r, paint);
        eprintln!("selection finish capture_ms={:.3}",start.elapsed().as_secs_f64()*1000.);
    }
}

#[test]
fn selection_paint_retained_preview_repaints_strokes_without_artwork_or_cursor_damage() {
    let mut r = renderer();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let make = || {
        r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("retained selection overlay"),
            size: wgpu::Extent3d {
                width: 128,
                height: 128,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    };
    let target = make();
    let reference = make();
    let mut presenter =
        crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb).unwrap();
    presenter.set_target_retention(true);
    r.set_selection_overlay(Some(layer_render::SelectionOverlay {
        active: true,
        editing: None,
        color: [1., 0., 0., 0.5],
        protected: false,
    }));
    let mut paint = request(1, Selection::empty(), SelectionPaintMode::Add, 0.5);
    for x in [25., 65., 105.] {
        paint.dabs[0].center.x = x;
        assert!(r.paint_selection(&paint).unwrap());
        presenter
            .present(
                &r,
                &target.create_view(&Default::default()),
                view(),
                [0.; 4],
            )
            .unwrap();
        let mut full =
            crate::ViewportPresenter::for_surface(&r, format, crate::SdrSurfaceColor::Srgb)
                .unwrap();
        full.present(
            &r,
            &reference.create_view(&Default::default()),
            view(),
            [0.; 4],
        )
        .unwrap();
        assert_eq!(
            page_bytes(&r, &target),
            page_bytes(&r, &reference),
            "update at {x}"
        );
    }
}

#[test]
fn selection_paint_saved_previews_color_visibility_thumbnails_and_export_isolation() {
    let mut r = renderer();
    let artwork = r.readback_srgb_rgba8().unwrap();
    let rect = |x| {
        Selection::polygon(vec![
            Point { x, y: 20. },
            Point { x: x + 25., y: 20. },
            Point { x: x + 25., y: 80. },
            Point { x, y: 80. },
        ])
        .unwrap()
    };
    let mut layers = vec![
        Layer::paint(LayerId(1), "art"),
        Layer::selection(LayerId(2), "left", rect(15.)),
        Layer::selection(LayerId(3), "right", rect(80.)),
    ];
    r.set_selection_overlay(Some(layer_render::SelectionOverlay {
        active: false,
        editing: None,
        color: [1., 0., 0., 0.5],
        protected: false,
    }));
    submit(&mut r, &layers, &[], &[], false);
    let first = r.selection_previews.buffer.clone().unwrap();
    submit(&mut r, &layers, &[], &[], false);
    assert_eq!(
        r.selection_previews.buffer.as_ref(),
        Some(&first),
        "unchanged previews must reuse their GPU overlay"
    );
    let target = r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("saved mask display test"),
        size: wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut presenter = crate::ViewportPresenter::for_surface(
        &r,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        crate::SdrSurfaceColor::Srgb,
    )
    .unwrap();
    let render = |r: &WgpuRasterizer, presenter: &mut crate::ViewportPresenter| {
        presenter
            .present(
                r,
                &target.create_view(&Default::default()),
                ViewState {
                    background_rgba_linear: [1.; 4],
                    ..view()
                },
                [1.; 4],
            )
            .unwrap();
        page_bytes(r, &target)
    };
    let pixels = render(&r, &mut presenter);
    for x in [25, 90] {
        let p = &pixels[(50 * 128 + x) * 4..][..4];
        assert!(p[0] > p[1] + 40, "{p:?}");
    }
    layers[2].properties.selection_mask = Some(layer_core::SelectionMaskProperties { color: layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb,[0.,0.,1.,1.]).unwrap(), ..Default::default() });
    submit(&mut r, &layers, &[], &[], false);
    let pixels = render(&r, &mut presenter);
    let blue = &pixels[(50 * 128 + 90) * 4..][..4];
    assert!(blue[2] > blue[0]+40 && blue[2] > blue[1]+40, "per-layer overlay color: {blue:?}");
    layers[1].visible = false;
    submit(&mut r, &layers, &[], &[], false);
    let pixels = render(&r, &mut presenter);
    let p = &pixels[(50 * 128 + 25) * 4..][..4];
    assert_eq!(p[0], p[1]);
    let p = &pixels[(50 * 128 + 90) * 4..][..4];
    assert!(p[2] > p[1] + 40);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), artwork);
    r.set_quick_mask_thumbnail(Some(&rect(15.)));
    r.request_thumbnail(42, LayerId(0)).unwrap();
    let until = std::time::Instant::now() + READBACK_TIMEOUT;
    let image = loop {
        let _ = r.device.poll(wgpu::PollType::Poll);
        if let Some(image) = r.take_thumbnail() {
            break image.unwrap();
        };
        assert!(std::time::Instant::now() < until);
        std::thread::yield_now();
    };
    assert_eq!(image.request_id, 42);
    let inside = &image.bytes[(12 * image.stride + 6 * 4) as usize..][..4];
    let outside = &image.bytes[(12 * image.stride + 25 * 4) as usize..][..4];
    assert_eq!(inside, [255, 255, 255, 255]);
    assert_eq!(outside, [0, 0, 0, 255]);
}
