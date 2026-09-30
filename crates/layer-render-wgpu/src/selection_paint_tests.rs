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
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
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
        saved_protected: false,
    }));
    let target = crate::create_target(&r.device, [128, 128], wgpu::TextureFormat::Rgba8UnormSrgb, "selection overlay reference").0;
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

