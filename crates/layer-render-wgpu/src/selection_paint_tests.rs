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
pub(super) fn paint(r: &mut WgpuRasterizer, request: &SelectionPaint) -> Result<bool, GpuRasterError> {
    pollster::block_on(r.compilation.prepare_all(r.recipes.selection_paint.pipelines.iter().chain(r.selection_clip.pipelines()))).unwrap();
    r.paint_selection(request)
}
pub(super) fn receive(r: &mut WgpuRasterizer, mut request: SelectionPaint) -> Selection {
    request.dabs.clear();
    request.enclosed = None;
    request.finish = true;
    assert!(self::paint(r, &request).unwrap());
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
#[test]
fn a_selection_readback_completing_while_the_host_waits_wakes_the_host() {
    let mut r = renderer();
    let (wake, woken) = std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move || { let _ = wake.send(()); }));
    let mut update = request(1, Selection::empty(), SelectionPaintMode::Add, 1.);
    update.finish = true;
    assert!(self::paint(&mut r, &update).unwrap());
    let until = std::time::Instant::now() + READBACK_TIMEOUT;
    let mut waits = 0;
    loop {
        if let Some(result) = r.take_selection_paint() { result.unwrap(); break; }
        if r.next_frame() == layer_render::FrameDemand::Wait {
            waits += 1;
            woken.recv_timeout(std::time::Duration::from_secs(5)).expect("the selection readback never woke the waiting host");
        }
        assert!(std::time::Instant::now() < until, "selection paint capture timed out");
    }
    assert!(waits > 0 && waits < 64, "the host slept {waits} times for one readback");
}
fn value(s: &Selection, x: u32, y: u32) -> u8 {
    let layer_core::SelectionShape::Pixels(p) = &s.shape else {
        panic!("pixels")
    };
    ((p.words()[(y * p.extent()[0].div_ceil(4) + x / 4) as usize] >> ((x % 4) * 8)) & 255) as u8
}
fn renderer() -> WgpuRasterizer {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    crate::test_support::submit_settled(&mut r, FramePacket {reset_layers:true,
        ..packet(paint_document([128;2],"artwork").scene(),[128;2])});
    r
}

#[test]
fn selection_paint_opacity_accumulates_between_contacts_but_not_within_one() {
    let mut r = renderer();
    let idle = r.raster_tiers.paint.used();
    let first = request(1, Selection::empty(), SelectionPaintMode::Add, 0.5);
    assert!(self::paint(&mut r, &first).unwrap());
    assert!(self::paint(&mut r, &first).unwrap());
    assert_eq!(r.raster_tiers.paint.used() - idle, r.selection_painter.as_ref().unwrap().storage_bytes() + r.selection_clip.storage_bytes(),
        "the painting's footprints and capture are charged to the paint tier");
    let first = receive(&mut r, first);
    assert_eq!(value(&first, 64, 64), 128);
    assert_eq!(value(&first, 2, 2), 0);
    let second = request(2, first, SelectionPaintMode::Add, 0.5);
    assert!(self::paint(&mut r, &second).unwrap());
    let second = receive(&mut r, second);
    assert_eq!(value(&second, 64, 64), 192);
    let subtract = request(3, second, SelectionPaintMode::Subtract, 0.5);
    assert!(self::paint(&mut r, &subtract).unwrap());
    let subtract = receive(&mut r, subtract);
    assert_eq!(value(&subtract, 64, 64), 96);
}

#[test]
fn dry_selection_contact_dose_survives_segment_and_update_partitioning() {
    use layer_core::{BrushAccumulation, BrushDryMedia, BrushDryPose};
    let mut r = renderer();
    let mut id = 100;
    for mode in [SelectionPaintMode::Add, SelectionPaintMode::Subtract, SelectionPaintMode::Gray] {
        for (segments, chunk) in [(1, 1), (8, 8), (8, 1)] {
            id += 1;
            let before = if mode == SelectionPaintMode::Subtract { Selection::full() } else { Selection::empty() };
            let mut paint = request(id, before, mode, 0.6);
            paint.gray = 0.35;
            paint.style = preset_style(layer_core::DefaultBrushPreset::Pencil);
            paint.style.rendering.accumulation = BrushAccumulation::Flow;
            let contact = paint.style.contact.as_mut().unwrap();
            contact.paper = 0.;
            contact.pressure_gain = 1.;
            contact.dry = Some(BrushDryMedia { poses: [BrushDryPose { radii: [1.; 2], relief: [0.; 3] }; 3],
                softness: 0.07, loading: 0., corner: 0., relief_offset: 0. });
            let contacts: Vec<_> = (1..=segments).map(|i| {
                let mut d = paint.dabs[0];
                d.center = Point { x: 20.5+88.*i as f32/segments as f32, y: 64.5 };
                d.motion = [88./segments as f32, 0.];
                d.previous = [12., 12., 1., 0.];
                d.contact = [1., 0., 0., 0.];
                d.previous_contact = d.contact;
                d.flow = 0.4;
                d
            }).collect();
            for contacts in contacts.chunks(chunk) {
                paint.dabs = contacts.to_vec();
                assert!(self::paint(&mut r, &paint).unwrap());
            }
            let result = receive(&mut r, paint);
            let alpha = 1.-(-6_f64*0.4).exp();
            let expected = match mode {
                SelectionPaintMode::Add => alpha*0.6,
                SelectionPaintMode::Subtract => 1.-alpha*0.6,
                SelectionPaintMode::Gray => 0.35*alpha*0.6,
            };
            assert!((f64::from(value(&result, 64, 64))-expected*255.).abs() <= 1.,
                "{mode:?} segments={segments} chunk={chunk}: scalar {} vs {}", value(&result, 64, 64), expected*255.);
            assert_eq!(value(&result, 2, 2), if mode == SelectionPaintMode::Subtract { 255 } else { 0 });
        }
    }
}

#[test]
fn selection_paint_overlay_is_coverage_scaled_and_excluded_from_artwork() {
    let mut r = renderer();
    let artwork = r.readback_srgb_rgba8().unwrap();
    let paint = request(1, Selection::empty(), SelectionPaintMode::Add, 0.5);
    assert!(self::paint(&mut r, &paint).unwrap());
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
    pollster::block_on(presenter.prepare(&r)).unwrap();
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
    pollster::block_on(presenter.prepare(&r)).unwrap();
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
        selection: Selection::full(),
    });
    let value = Occurrence::new(OccurrenceContent::Selection(saved.handle), "Saved coverage");
    let occurrence = RecordChange::insert(&document.artwork.occurrences, value);
    let handle = occurrence.handle;
    let stack = document.composition().result;
    let mut membership = document.artwork.stacks.get(stack).unwrap().clone();
    membership.entries.insert(0, handle);
    let membership = RecordChange::replace(&document.artwork.stacks, stack, Some(membership)).unwrap();
    document.apply(Edit::Batch(vec![Edit::SavedSelection(saved), Edit::Occurrence(occurrence), Edit::Stack(membership)])).unwrap();
    document.working.selection_overlays.visibility.insert(handle, false);
    let authored = document.artwork.clone();
    let mut r = renderer();
    r.set_selection_overlay(Some(layer_render::SelectionOverlay {
        active: false, editing: None, color: [1., 0., 0., 0.5], protected: false, saved_protected: false,
    }));
    let base = FramePacket { selection_overlays: Some(&document.working.selection_overlays), ..packet(document.scene(), [128; 2]) };
    crate::test_support::submit_settled(&mut r, base);
    assert!(r.selection_previews.texture.is_none());
    let mut visibility = layer_core::authored::SelectionOverlays {visibility: [(handle,true)].into(), ..Default::default()};
    crate::test_support::submit_settled(&mut r, FramePacket { selection_overlays: Some(&visibility), ..base });
    assert!(r.selection_previews.texture.is_some());
    visibility.visibility.insert(handle, false);
    crate::test_support::submit_settled(&mut r, FramePacket { selection_overlays: Some(&visibility), ..base });
    assert!(r.selection_previews.texture.is_none());
    crate::test_support::submit_settled(&mut r, base);
    assert!(r.selection_previews.texture.is_none());
    assert_eq!(document.artwork, authored);
}

#[test]
fn saved_selection_overlays_beyond_one_storage_binding_draw_in_row_bands() {
    use layer_core::{Edit, SavedSelection};
    let extent = [1300, 900];
    let mut document = paint_document(extent, "artwork");
    let stack = document.composition().result;
    let mut visibility = layer_core::authored::SelectionOverlays::default();
    for corners in [[[40., 20.], [1260., 270.], [520., 880.]], [[870., 90.], [1210., 860.], [650., 680.]]] {
        let saved = RecordChange::insert(&document.artwork.selections, SavedSelection {
            selection: Selection::polygon(corners.map(|[x, y]| Point { x, y }).to_vec()).unwrap(),
        });
        let occurrence = RecordChange::insert(&document.artwork.occurrences, Occurrence::new(OccurrenceContent::Selection(saved.handle), "Saved coverage"));
        visibility.visibility.insert(occurrence.handle, true);
        let mut membership = document.artwork.stacks.get(stack).unwrap().clone();
        membership.entries.insert(0, occurrence.handle);
        let membership = RecordChange::replace(&document.artwork.stacks, stack, Some(membership)).unwrap();
        document.apply(Edit::Batch(vec![Edit::SavedSelection(saved), Edit::Occurrence(occurrence), Edit::Stack(membership)])).unwrap();
    }
    let limit = 4 << 20;
    assert!(32 + 5376 * u64::from(extent[1]) > limit, "the whole overlay exceeds one binding");
    let shown = |mut r: WgpuRasterizer| {
        r.set_selection_overlay(Some(layer_render::SelectionOverlay {
            active: false, editing: None, color: [1., 0., 0., 0.5], protected: false, saved_protected: false,
        }));
        crate::test_support::submit_settled(&mut r, FramePacket { selection_overlays: Some(&visibility), ..packet(document.scene(), extent) });
        assert!(r.selection_previews.texture.is_some());
        let target = crate::create_target(&r.device, extent, wgpu::TextureFormat::Rgba8UnormSrgb, "banded overlay").0;
        let mut presenter = crate::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
        pollster::block_on(presenter.prepare(&r)).unwrap();
        presenter.present(&r, &target.create_view(&Default::default()), crate::test_support::view(extent), [1.; 4]).unwrap();
        page_bytes(&r, &target)
    };
    let banded = shown(limited_renderer(limit));
    let whole = shown(WgpuRasterizer::new_native_headless(Default::default()).unwrap());
    let at = |x: usize, y: usize| &banded[(y * 1300 + x) * 4..][..4];
    assert!(at(600, 300)[0] > at(600, 300)[1] + 15, "inside a saved selection is tinted: {:?}", at(600, 300));
    assert_eq!(at(20, 860)[0], at(20, 860)[1], "outside stays untinted");
    assert!(banded == whole, "banded overlay matches one whole-canvas pass");
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
            assert!(self::paint(&mut r, &paint).unwrap());
            assert!(self::paint(&mut r, &paint).unwrap());
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
