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
        tip: BrushTip::AnalyticEllipse,
        dabs: vec![contact],
        enclosed: None,
        finish: false,
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
    use std::time::Instant;
    let mut r = renderer();
    r.document_extent = [4096, 4096];
    let mut paint = request(1, Selection::empty(), SelectionPaintMode::Add, 0.5);
    let start = Instant::now();
    r.paint_selection(&paint).unwrap();
    r.wait_idle().unwrap();
    eprintln!(
        "selection 4096² initialize complete_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
    let mut submit = Vec::new();
    let mut complete = Vec::new();
    for i in 0..120 {
        paint.dabs = (0..8)
            .map(|j| {
                let mut d = paint.dabs[0];
                d.center = Point {
                    x: 200. + (i * 8 + j) as f32 * 2.,
                    y: 1000.,
                };
                d.radii = [16.; 2];
                d
            })
            .collect();
        let start = Instant::now();
        r.paint_selection(&paint).unwrap();
        submit.push(start.elapsed().as_secs_f64() * 1000.);
        r.wait_idle().unwrap();
        complete.push(start.elapsed().as_secs_f64() * 1000.);
    }
    submit.sort_by(f64::total_cmp);
    complete.sort_by(f64::total_cmp);
    eprintln!(
        "selection 8 contacts update p50/p95 submit_ms={:.3}/{:.3} complete_ms={:.3}/{:.3}",
        submit[60], submit[114], complete[60], complete[114]
    );
    let start = Instant::now();
    let _ = receive(&mut r, paint);
    eprintln!(
        "selection finish capture_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
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
