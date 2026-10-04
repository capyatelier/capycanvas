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
        paint_document([128; 2], "artwork").scene(),
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
            view()
            ,
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
            view()
            ,
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
fn saved_selection_overlay_uses_working_visibility_without_changing_authored_artwork() {
    use layer_core::{Edit, SavedSelection};
    let mut document = paint_document([128; 2], "artwork");
    let saved = RecordChange::insert(&document.artwork.selections, SavedSelection {
        selection: Selection::full(), display: Default::default(),
    });
    let mut value = Occurrence::new(OccurrenceContent::Selection(saved.handle), "Saved coverage");
    value.visible = false;
    let occurrence = RecordChange::insert(&document.artwork.occurrences, value);
    let handle = occurrence.handle;
    let stack = document.composition().result;
    let mut membership = document.artwork.stacks.get(stack).unwrap().clone();
    membership.entries.insert(0, handle);
    let membership = RecordChange::replace(&document.artwork.stacks, stack, Some(membership)).unwrap();
    document.apply(Edit::Batch(vec![Edit::SavedSelection(saved), Edit::Occurrence(occurrence), Edit::Stack(membership)])).unwrap();
    let authored = document.artwork.clone();
    let mut r = renderer();
    r.set_selection_overlay(Some(layer_render::SelectionOverlay {
        active: false, editing: None, color: [1., 0., 0., 0.5], protected: false, saved_protected: false,
    }));
    let base = packet(document.scene(), [128; 2]);
    r.prepare_selection_previews(base).unwrap();
    assert!(r.selection_previews.texture.is_none());
    let mut visibility = std::collections::BTreeMap::from([(handle, true)]);
    r.prepare_selection_previews(FramePacket { selection_visibility: Some(&visibility), ..base }).unwrap();
    assert!(r.selection_previews.texture.is_some());
    visibility.insert(handle, false);
    r.prepare_selection_previews(FramePacket { selection_visibility: Some(&visibility), ..base }).unwrap();
    assert!(r.selection_previews.texture.is_none());
    r.prepare_selection_previews(base).unwrap();
    assert!(r.selection_previews.texture.is_none());
    assert_eq!(document.artwork, authored);
}

#[test]
fn selection_gradient_shapes_preserve_scalar_coverage_and_contact_opacity() {
    use layer_core::{GradientShape, ScalarGradient, ScalarGradientStop};
    for shape in GradientShape::ALL {
        for reverse in [false,true] {
            let mut r=renderer();
            let mut paint=request(1,Selection::empty(),SelectionPaintMode::Gray,0.5);
            paint.dabs.clear();
            paint.style.rendering.accumulation=layer_core::BrushAccumulation::Uniform;
            paint.enclosed=Some(Arc::new(Selection::polygon(vec![Point{x:52.,y:52.},Point{x:76.,y:52.},Point{x:76.,y:76.},Point{x:52.,y:76.}]).unwrap()));
            paint.gradient=Some(layer_render::SelectionGradient {
                start:Point {x:64.,y:64.},end:Point {x:80.,y:64.},shape,reverse,
                gradient:ScalarGradient {stops:vec![
                    ScalarGradientStop {position:0.,value:0.2,opacity:0.5},
                    ScalarGradientStop {position:1.,value:0.8,opacity:0.25},
                ]},
            });
            assert!(r.paint_selection(&paint).unwrap());
            assert!(r.paint_selection(&paint).unwrap());
            let result=receive(&mut r,paint);
            for (x,y) in [(56,64),(64,64),(72,64),(64,70)] {
                let dx=f64::from(x)+0.5-64.;let dy=f64::from(y)+0.5-64.;
                let t=match shape {GradientShape::Linear=>dx/16.,GradientShape::Radial=>dx.hypot(dy)/16.,GradientShape::Reflected=>dx.abs()/16.}.clamp(0.,1.);
                let t=if reverse {1.-t}else{t};
                let expected=((0.2*0.5*(1.-t)+0.8*0.25*t)*0.5*255.).round();
                assert!((f64::from(value(&result,x,y))-expected).abs()<=1.,"{shape:?}/{reverse}/{x},{y}: {} vs {expected}",value(&result,x,y));
            }
            assert_eq!(value(&result,2,2),0);
        }
    }
}
