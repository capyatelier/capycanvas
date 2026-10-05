use super::*;
use layer_core::{EffectInstance, EffectResolution, EffectValue};
use layer_core::color::{DocumentColor, RgbSpace};

#[test]
fn native_alpha_signed_capture_matches_native_reference_without_reducing_input() {
    let extent = [32, 32];
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F32 };
    for name in ["brightness_to_opacity", "threshold"] { for alpha in [0.37, 1e-30] {
        let mut doc = crate::tests::native_effects::empty_document(extent, color);
        let mut effect = EffectInstance::new(crate::tests::fixture(name).program());
        if name == "threshold" {
            effect.set("colors", EffectValue::Choice(1)).unwrap();
            effect.set("transparency", EffectValue::Choice(0)).unwrap();
        }
        let handle = crate::tests::native_effects::insert_effect(&mut doc, effect);
        let source = crate::test_support::depth_source([64, 48], SampleDepth::F32, color.space, 1 << 20,
            |x, y| { let gray = if (x + y) % 3 == 0 {0.8} else {0.02}; [gray, gray, gray, alpha] });
        let paint = crate::tests::native_effects::insert_source(&mut doc, "negative source", source);
        doc.artwork.occurrences.get_mut(paint).unwrap().translation = layer_core::Point { x: -24., y: -16. };
        crate::tests::native_effects::refresh(&mut doc);
        let bounds = DocRect { min: [-16, -8], max: [48, 24] };
        let size = bounds.size().unwrap();
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut scene = Scene::new(&r);
        let mut plan = display_mips::Plan::at(size, 2); plan.extent = extent; plan.doc_bounds = bounds;
        let destination = Image::new(&r, plan, "signed native alpha reduction");
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        super::super::effects::capture_native_effect(&mut scene, &mut r, packet(doc.scene(), extent), handle,
            &Target { view: destination.view.clone(), slot: None, plan }, &[bounds], None, &mut encoder).unwrap();
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
        let actual = pixels(&r, &destination.texture);
        let (native, _) = create_color_target(&r.device, size, "signed native alpha reference");
        let mut reference_scene = Scene::new(&r);
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        let frame = packet(doc.scene().with_offset64(bounds.min.map(|v| -(v as f64))), size);
        reference_scene.capture_region(&mut r, frame, &native, PixelRect::full(size), scene::Output::EffectComposite(handle), &mut encoder).unwrap();
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
        let native = pixels(&r, &native);
        for y in 0..plan.size[1] { for x in 0..plan.size[0] {
            let mut expected = [0.; 4];
            for dy in 0..4 { for dx in 0..4 {
                let sample = native[((y * 4 + dy) * size[0] + x * 4 + dx) as usize];
                for channel in 0..4 { expected[channel] += sample[channel] / 16.; }
            }}
            let pixel = actual[(y * plan.size[0] + x) as usize];
            for channel in 0..4 { assert!((pixel[channel] - expected[channel]).abs() <= 2e-5 * expected[channel].abs().max(1e-35),
                "{name} alpha{alpha} signed{x},{y} channel{channel}: {pixel:?} != {expected:?}"); }
        }}
        assert!(actual.iter().any(|pixel| pixel[3] > 0.), "{name} alpha{alpha} signed native reduction must retain dark input");
        assert!(scene.pool.len() <= SOURCE_SLOTS);
        assert!(destination.bytes() <= u64::from(size[0] * size[1]) * 16);
    }}
}

#[test]
fn native_alpha_on_both_sides_of_spatial_effect_preserves_cross_canvas_support() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F32 };
    for name in ["brightness_to_opacity", "threshold"] { for before in [true, false] {
        let source = crate::test_support::depth_source([64, 32], SampleDepth::F32, color.space, 1 << 20,
            |x, y| if (20..36).contains(&x) && (8..24).contains(&y) { let gray = if x < 28 {0.8} else {0.02}; [gray, gray, gray, 0.37] } else { [0.; 4] });
        let make = |extent, translation| {
            let mut doc = crate::tests::native_effects::empty_document(extent, color);
            let mut blur = EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
            Arc::make_mut(&mut blur.program).resolution = EffectResolution::Display;
            blur.set("sigma", EffectValue::Number(4.)).unwrap();
            let mut effect = EffectInstance::new(crate::tests::fixture(name).program());
            if name == "threshold" { effect.set("colors", EffectValue::Choice(1)).unwrap(); effect.set("transparency", EffectValue::Choice(0)).unwrap(); }
            if before {
                crate::tests::native_effects::insert_effect(&mut doc, blur);
                crate::tests::native_effects::insert_effect(&mut doc, effect);
            } else {
                crate::tests::native_effects::insert_effect(&mut doc, effect);
                crate::tests::native_effects::insert_effect(&mut doc, blur);
            }
            let paint = crate::tests::native_effects::insert_source(&mut doc, "cross canvas source", source.clone());
            doc.artwork.occurrences.get_mut(paint).unwrap().translation = translation;
            crate::tests::native_effects::refresh(&mut doc); doc
        };
        let evaluate = |doc: &Document| {
            let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
            let mut frame = packet(doc.scene(), doc.composition().size); frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
            r.submit(frame).unwrap();
            assert_eq!(r.scale_display.as_ref().unwrap().plan.level, 2);
            (r.scale_display.as_ref().unwrap().plan.size, display_pixels(&r))
        };
        let small = make([32, 32], layer_core::Point { x: -32., y: 0. });
        let large = make([96, 64], layer_core::Point { x: 0., y: 16. });
        let (size, actual) = evaluate(&small); let (larger, reference) = evaluate(&large);
        let expected: Vec<_> = (0..size[1]).flat_map(|y| (0..size[0]).map(move |x| ((y + 4) * larger[0] + x + 8) as usize)).map(|i| reference[i]).collect();
        assert!(actual.iter().any(|pixel| pixel[3] > 0.01), "{name} before{before} must retain off-canvas input");
        assert!(crate::test_support::max_error(&actual, &expected) < 3e-5, "{name} before{before} signed support differs from larger reference");
    }}
}
