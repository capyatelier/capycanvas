use super::*;
use layer_core::{Edit, EffectInstance, EffectValue, paper};

pub(crate) fn set_paper(document: &mut Document, values: &[(&str, EffectValue)]) {
    let sheet = document.bottom_paper().unwrap();
    let handle = document.scene().effect_handle(sheet).unwrap();
    let mut application = document.artwork.effects.get(handle).unwrap().clone();
    let mut draft = EffectInstance { program: application.program.clone(), values: application.values };
    for (key, value) in values {
        draft.set(key, value.clone()).unwrap();
    }
    application.values = draft.values;
    document.apply(Edit::Effect(RecordChange::replace(&document.artwork.effects, handle, Some(application)).unwrap())).unwrap();
}

pub(crate) fn rough_gray_paper() -> Document {
    let mut document = Document::new(layer_core::authored::PortableId::random(), 128, 128,
        layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let rough = paper::built_in_paper("rough").unwrap().texture.clone();
    let sheet = document.bottom_paper().unwrap();
    let handle = document.scene().effect_handle(sheet).unwrap();
    for value in &mut document.artwork.effects.get_mut(handle).unwrap().values {
        if let EffectValue::Texture(texture) = value { *texture = rough.clone(); }
    }
    let gray = layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb, [0.6, 0.6, 0.6, 1.]).unwrap();
    set_paper(&mut document, &[("color", EffectValue::Color(gray))]);
    document
}

fn linear(pixels: &[u8]) -> Vec<f32> {
    pixels.as_chunks::<4>().0.iter().map(|p| layer_core::color::srgb_decode(f32::from(p[1]) / 255.)).collect()
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len() as f32
}

#[test]
fn paper_without_texture_keeps_its_color_with_raised_lighting_values() {
    let mut document = rough_gray_paper();
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let texture = crate::create_target(&r.device, [128; 2], wgpu::TextureFormat::Rgba8UnormSrgb, "plain paper").0;
    let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, SdrSurfaceColor::Srgb).unwrap();
    let mut render = |r: &mut WgpuRasterizer, document: &Document| {
        crate::test_support::submit_settled(r, packet(document.scene(), [128; 2]));
        crate::test_support::prepare_lit_presenter(r, &mut presenter);
        presenter.present(r, &texture.create_view(&Default::default()), view(), [0.2, 0.3, 0.4, 1.]).unwrap();
        let canvas = page_bytes(r, &texture);
        let mut snapshot = r.snapshot_gpu().capture_scene(document.snapshot(), layer_core::authored::SceneScope::All, Default::default()).unwrap();
        let mut png = Vec::new();
        snapshot.write_png(&mut png, &layer_core::color::source::SourceInterpretation { channels: layer_core::color::source::SourceChannels::Rgba, depth: layer_core::color::SampleDepth::U8, profile: Default::default(), profile_assumed: false }, Default::default(), None).unwrap();
        (canvas, png)
    };
    set_paper(&mut document, &[(paper::RELIEF, EffectValue::Number(150.)), (paper::THROUGH_PAINT, EffectValue::Number(40.))]);
    let rough = render(&mut r, &document);
    set_paper(&mut document, &[("texture", EffectValue::Texture(None))]);
    let raised = render(&mut r, &document);
    assert_ne!(raised, rough);
    assert!(r.paper_shade().is_none());
    assert!(raised.0.chunks_exact(4).all(|pixel| pixel[..3].iter().all(|value| value.abs_diff(153) <= 1) && pixel[3] == 255));
    set_paper(&mut document, &[(paper::RELIEF, EffectValue::Number(0.))]);
    assert_eq!(render(&mut r, &document), raised);
}

#[test]
fn visible_paper_slope_prepares_while_optional_work_is_parked() {
    let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = rough_gray_paper();
    set_paper(&mut document, &[(paper::RELIEF, EffectValue::Number(150.))]);
    let mut r = crate::test_support::staged_renderer(&reference, document.composition().color);
    r.shader_idle(false, false);
    let batch = r.compilation.batch();
    r.update_paper_lighting(document.scene()).unwrap();
    assert!(r.paper_lighting.is_some() && r.paper_shade().is_none());
    drop(batch);
    crate::test_support::wait_until("visible slope must prepare without optional admission", || {
        if r.paper_shade().is_some() { return Ok(()); }
        Err(r.poll_startup().unwrap())
    });
    assert!(!r.paper_shade_work_pending());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn first_ready_paper_frame_matches_eager_pixels_without_lighting_unused_surfaces() {
    let mut reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    for (overview, raised) in [(false, true), (false, false), (true, true)] {
        let mut document = rough_gray_paper();
        set_paper(&mut document, &[(paper::RELIEF, EffectValue::Number(if raised {150.} else {0.}))]);
        crate::test_support::submit_settled(&mut reference, packet(document.scene(), [128; 2]));
        assert_eq!(reference.paper_shade().is_some(), raised);
        let mut r = crate::test_support::staged_renderer(&reference, document.composition().color);
        r.shader_idle(false, false);
        r.prepare_startup(&document, &layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), false).unwrap();
        crate::test_support::submit_settled(&mut r, packet(document.scene(), [128; 2]));
        let batch = r.compilation.batch();
        let create = if overview {ViewportPresenter::for_overview_surface} else {ViewportPresenter::for_surface};
        let mut presenter = create(&r, format, SdrSurfaceColor::Srgb).unwrap();
        if overview {
            let placement = crate::OverviewPlacement {
                bounds: [0., 0., 128., 128.], clip: None, work_area: [[-1000.; 2]; 4],
                outline_linear: [0.; 3], scale: 1., opacity: 1.,
            };
            presenter.set_overviews(&r, &[placement]);
        }
        let lit = raised && !overview;
        drop(batch);
        crate::test_support::wait_until(format_args!("first frame readiness, overview={overview}, raised={raised}"), || {
            let ready = presenter.pipelines_ready(&r);
            let progress = r.poll_startup().unwrap();
            if progress.complete && ready { Ok(()) } else { Err((ready, progress)) }
        });
        let composited = r.metrics.composited_pixels;
        let (texture, target) = crate::create_target(&r.device, [128; 2], format, "first ready paper frame");
        let draw = |presenter: &mut ViewportPresenter, renderer: &WgpuRasterizer| {
            if overview { presenter.present_overviews(renderer, &target, [128; 2]) }
            else { presenter.present(renderer, &target, view(), [0.2, 0.3, 0.4, 1.]).map(|_| ()) }.unwrap();
        };
        draw(&mut presenter, &r);
        assert_eq!(presenter.paper_lighting_drawn(), lit);
        let first = page_bytes(&r, &texture);
        let mut expected = create(&reference, format, SdrSurfaceColor::Srgb).unwrap();
        if overview {
            expected.set_overviews(&reference, &[crate::OverviewPlacement {
                bounds: [0., 0., 128., 128.], clip: None, work_area: [[-1000.; 2]; 4],
                outline_linear: [0.; 3], scale: 1., opacity: 1.,
            }]);
        }
        expected.prepare_paper_lighting(&reference);
        pollster::block_on(expected.prepare(&reference)).unwrap();
        draw(&mut expected, &reference);
        assert_eq!(expected.paper_lighting_drawn(), lit);
        assert_eq!(first, page_bytes(&reference, &texture),
            "first staged and eager pixels agree, overview={overview}, raised={raised}");
        assert_eq!(r.metrics.composited_pixels, composited, "presentation lighting does not recompose artwork");
    }
}

#[test]
fn paper_relief_lights_the_canvas_keeps_its_tone_and_turns_with_the_light() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = rough_gray_paper();
    let texture = crate::create_target(&r.device, [128, 128], wgpu::TextureFormat::Rgba8UnormSrgb, "paper relief").0;
    let mut presenter = crate::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    let mut first = true;
    let mut present = |r: &mut WgpuRasterizer, document: &Document, camera: ViewState| {
        crate::test_support::submit_settled(r,FramePacket { view: camera, composite_all: std::mem::take(&mut first), ..packet(document.scene(), [128, 128]) });
        crate::test_support::prepare_lit_presenter(r,&mut presenter);
        presenter.present(r, &texture.create_view(&Default::default()), camera, [0.2, 0.3, 0.4, 1.]).unwrap();
        linear(&page_bytes(r, &texture))
    };
    let flat = present(&mut r, &document, view());
    assert!(r.paper_shade().is_none(), "a flat paper is not shaded");
    set_paper(&mut document, &[(paper::RELIEF, EffectValue::Number(150.))]);
    let composited = r.metrics.composited_pixels;
    let lit = present(&mut r, &document, view());
    assert_eq!(r.metrics.composited_pixels, composited, "lighting changes no composited pixel");
    let changed = lit.iter().zip(&flat).filter(|(a, b)| (*a - *b).abs() > 0.004).count();
    assert!(changed > flat.len() / 2, "relief shows across the sheet: {changed} pixels");
    assert!(lit.iter().zip(&flat).any(|(a, b)| a > b) && lit.iter().zip(&flat).any(|(a, b)| a < b));
    assert!((mean(&lit) - mean(&flat)).abs() < 0.01 * mean(&flat), "the sheet keeps its tone: {} {}", mean(&lit), mean(&flat));
    set_paper(&mut document, &[(paper::LIGHT, EffectValue::Number(45.))]);
    let opposite = present(&mut r, &document, view());
    let agreement: f32 = lit.iter().zip(&opposite).zip(&flat).map(|((a, b), f)| (a - f) * (b - f)).sum();
    assert!(agreement < 0., "light from the opposite side inverts the shading");
    let far = ViewState { document_to_surface: [1. / 16., 0., 0., 1. / 16., 60., 60.], ..view() };
    let distant = present(&mut r, &document, far);
    let unshaded = { set_paper(&mut document, &[(paper::RELIEF, EffectValue::Number(0.))]); present(&mut r, &document, far) };
    let sheet = |pixels: &[f32]| (61..67).flat_map(|y| (61..67).map(move |x| y * 128 + x)).map(|i| pixels[i]).collect::<Vec<_>>();
    let deviation = |shaded: &[f32], plain: &[f32]| shaded.iter().zip(plain).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
    let (far_deviation, near_deviation) = (deviation(&sheet(&distant), &sheet(&unshaded)), deviation(&opposite, &flat));
    assert!(far_deviation < 0.3 * near_deviation, "far away the relief averages out: {far_deviation} against {near_deviation}");
    assert_eq!(present(&mut r, &document, view()), flat, "a flattened sheet presents as before");
}

#[test]
fn separated_paper_and_the_paper_preview_light_the_artwork_in_its_own_encoding() {
    for blend in [layer_core::BlendSpace::Linear, layer_core::BlendSpace::Perceptual] {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut document = rough_gray_paper();
        let root = document.artwork.root;
        document.artwork.compositions.get_mut(root).unwrap().blend = blend;
        super::placement::set_source(&mut document, crate::test_support::depth_source([128, 128], layer_core::color::SampleDepth::F32,
            layer_core::color::RgbSpace::Srgb, 128 * 128 * 16 + 4096, |x, _| if x < 64 { [0.8, 0.1, 0.1, 0.9] } else { [0.; 4] }));
        let texture = crate::create_target(&r.device, [128, 128], wgpu::TextureFormat::Rgba8UnormSrgb, "paper states").0;
        let mut presenter = crate::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
        let mut first = true;
        let mut present = |r: &mut WgpuRasterizer, document: &Document| {
            crate::test_support::submit_settled(r, FramePacket {
                composite_all: std::mem::take(&mut first), blend_space: blend, ..packet(document.scene(), [128, 128])
            });
            crate::test_support::prepare_lit_presenter(r,&mut presenter);
            presenter.present(r, &texture.create_view(&Default::default()), view(), [0.2, 0.3, 0.4, 1.]).unwrap();
            page_bytes(r, &texture)
        };
        let plain = present(&mut r, &document);
        assert!(r.paper_shade().is_none());
        set_paper(&mut document, &[(paper::RELIEF, EffectValue::Number(150.)), (paper::THROUGH_PAINT, EffectValue::Number(0.5))]);
        let lit = present(&mut r, &document);
        assert!(r.paper_shade().is_some_and(|(_, state, _)| state[13] != 0.), "the lit sheet is separated from the artwork");
        set_paper(&mut document, &[(paper::RELIEF, EffectValue::Number(0.))]);
        let binding = paper::PaperBinding { surface: Arc::new(paper::PaperSurface {
            texture: paper::built_in_paper("rough").unwrap().texture.clone().unwrap(),
            mapping: paper::reference([128; 2], [300.; 2]).mapping }), target_offset: [0; 2] };
        r.set_paper_overlay(Some(layer_render::PaperOverlay { paper: binding.clone(), opacity: 1., moving: false }));
        let previewed = present(&mut r, &document);
        assert!(r.paper_overlay_ready(&binding) && r.paper_shade().is_some_and(|(_, state, _)| state[13] == 0.), "the preview lights the composite");
        let channel = |pixels: &[u8], left: bool, c: usize| {
            let values: Vec<f32> = (0..128 * 128).filter(|i| (i % 128 < 64) == left && i % 128 != 63 && i % 128 != 64)
                .map(|i| layer_core::color::srgb_decode(f32::from(pixels[i * 4 + c]) / 255.)).collect();
            mean(&values)
        };
        for (name, shaded) in [("separated", &lit), ("preview", &previewed)] {
            assert!(shaded.iter().zip(&plain).filter(|(a, b)| a != b).count() > plain.len() / 8, "{name} relief shows");
            for (left, c) in [(true, 0), (true, 1), (false, 0), (false, 2)] {
                let (a, b) = (channel(shaded, left, c), channel(&plain, left, c));
                assert!((a - b).abs() < 0.03 * b.max(0.05), "{blend:?} {name} keeps the tone of channel {c} on the {} side: {a} against {b}", if left { "painted" } else { "bare" });
            }
        }
    }
}
