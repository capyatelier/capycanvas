use super::*;
use layer_core::{LayerBlend, color::RgbSpace};

const SIDE: u32 = 256;
const PATCH: u32 = 16;

fn document(space: RgbSpace, blend: LayerBlend, hidden_effect: bool, backdrop: bool) -> layer_core::Document {
    let extent = [SIDE; 2];
    let mut artwork = layer_core::authored::Artwork::new(extent).unwrap();
    let (upper, _) = crate::test_support::add_paint(&mut artwork, "Sweep", extent);
    let lower = backdrop.then(|| crate::test_support::add_paint(&mut artwork, "Backdrop", extent).0);
    let composition = artwork.compositions.get_mut(artwork.root).unwrap();
    composition.color.depth = layer_core::color::SampleDepth::U8;
    composition.color.space = space;
    artwork.occurrences.get_mut(upper).unwrap().blend = blend;
    let lower_pixels = layer_core::color::source::rgba8_source(extent, |x, y| {
        let patch = (y / PATCH) * (SIDE / PATCH) + x / PATCH;
        [(16 + patch * 7 % 224) as u8, (24 + patch * 13 % 208) as u8, (32 + patch * 29 % 192) as u8, 255]
    });
    let sweep = layer_core::color::source::rgba8_source(extent, |x, y| {
        let patch = (y / PATCH) * (SIDE / PATCH) + x / PATCH;
        let alpha = [1, 2, 17, 64, 128, 191, 238, 254, 255][patch as usize % 9];
        [(3 + patch * 37 % 250) as u8, (5 + patch * 59 % 248) as u8, (7 + patch * 83 % 246) as u8, alpha]
    });
    for (occurrence, source) in lower.map(|lower| (lower, lower_pixels)).into_iter().chain([(upper, sweep)]) {
        let layer_core::authored::OccurrenceContent::Paint(handle) = artwork.occurrences.get(occurrence).unwrap().content else { unreachable!() };
        artwork.paint.get_mut(handle).unwrap().base = Some(layer_core::authored::PaintBase::new(source.into()));
    }
    let mut doc = layer_core::Document::from_artwork(artwork).unwrap();
    if hidden_effect {
        let effect = crate::artwork_sample_tests::insert_effect(&mut doc,
            layer_core::EffectInstance::new(crate::tests::fixture("invert").program()), 0);
        doc.artwork.occurrences.get_mut(effect).unwrap().visible = false;
    }
    doc
}

fn sweep(doc: &layer_core::Document) -> layer_core::authored::OccurrenceHandle {
    doc.scene().order().iter().copied().find(|&handle| doc.scene().paint_source(handle).is_some()).unwrap()
}

fn transform(doc: &layer_core::Document) -> layer_render::TransformPreview {
    layer_render::TransformPreview { transaction: 1, target: doc.scene().source_target(sweep(doc)).unwrap(), moving: false, selection: None,
        transform: layer_core::ImageTransform::affine(layer_core::Affine::around(layer_core::Point { x: 128., y: 128. }, [0.83, 0.91], 0.27,
            layer_core::Point { x: 7.25, y: -3.5 })) }
}

fn settled(doc: &layer_core::Document, view: layer_render::ViewState, preview: Option<&layer_render::TransformPreview>) -> (WgpuRasterizer, crate::test_support::PassCounts) {
    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut frame = crate::test_support::packet(doc.scene(), [SIDE; 2]);
    frame.view = view;
    let mut before = crate::test_support::PassCounts::snapshot();
    crate::test_support::submit_settled(&mut renderer, frame);
    if let Some(preview) = preview {
        before = crate::test_support::PassCounts::snapshot();
        renderer.set_transform_preview(Some(preview)).unwrap();
        frame.composite_all = false;
        crate::test_support::submit_settled(&mut renderer, frame);
    }
    let passes = crate::test_support::PassCounts::snapshot().delta(before);
    (renderer, passes)
}

fn presented(doc: &layer_core::Document, view: layer_render::ViewState, preview: Option<&layer_render::TransformPreview>) -> Vec<u8> {
    let (renderer, _) = settled(doc, view, preview);
    let mut capture = ViewportPresenter::for_surface(&renderer, wgpu::TextureFormat::Rgba8UnormSrgb, SdrSurfaceColor::Srgb).unwrap();
    capture.readback_rgba8(&renderer, view, [0.5, 0.5, 0.5, 1.]).unwrap().bytes.to_vec()
}

fn compare(half: &[u8], full: &[u8], width: u32, patch: u32, label: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let patches = width / patch;
    for py in 0..patches {
        for px in 0..patches {
            for channel in 0..3 {
                let (mut worst, mut sum, mut count) = (0i32, 0i64, 0i64);
                for y in py * patch..(py + 1) * patch {
                    for x in px * patch..(px + 1) * patch {
                        let index = ((y * width + x) * 4 + channel) as usize;
                        let difference = i32::from(half[index]) - i32::from(full[index]);
                        worst = worst.max(difference.abs());
                        sum += i64::from(difference);
                        count += 1;
                    }
                }
                let mean = sum as f64 / count as f64;
                if worst > 1 || mean.abs() >= 0.1 {
                    failures.push(format!("{label} patch ({px},{py}) channel {channel}: worst {worst}, mean {mean:.3}"));
                }
            }
        }
    }
    failures
}

fn views() -> [(layer_render::ViewState, u32, &'static str); 2] {
    let fit = layer_render::ViewState { width_px: SIDE / 2, height_px: SIDE / 2, document_to_surface: [0.5, 0., 0., 0.5, 0., 0.] };
    [(crate::test_support::view([SIDE; 2]), PATCH, "100%"), (fit, PATCH / 2, "Fit")]
}

#[test]
fn a_hidden_effect_changes_only_display_precision_not_presented_pixels() {
    for (view, _, label) in views() {
        let plain = presented(&document(RgbSpace::DisplayP3, LayerBlend::Normal, false, true), view, None);
        let marked = presented(&document(RgbSpace::DisplayP3, LayerBlend::Normal, true, true), view, None);
        assert_eq!(plain, marked, "{label}: a hidden effect is the corpus's full-precision reference and must not change pixels");
    }
}

#[test]
fn half_precision_display_delivers_the_full_precision_codes_within_one_code() {
    let mut failures = Vec::new();
    for blend in [LayerBlend::Normal, LayerBlend::Multiply, LayerBlend::Screen, LayerBlend::Difference, LayerBlend::HardMix, LayerBlend::ColorDodge] {
        let half = document(RgbSpace::Srgb, blend, false, true);
        let full = document(RgbSpace::Srgb, blend, true, true);
        assert_eq!(crate::display_precision::DisplayPrecision::for_scene(half.scene(), false), crate::display_precision::DisplayPrecision::Half);
        assert_eq!(crate::display_precision::DisplayPrecision::for_scene(full.scene(), false), crate::display_precision::DisplayPrecision::Full);
        for (view, patch, label) in views() {
            failures.extend(compare(&presented(&half, view, None), &presented(&full, view, None), view.width_px, patch, &format!("{blend:?} {label}")));
        }
    }
    assert!(failures.is_empty(), "{} patch channels exceed one code or shift on average:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn a_transform_preview_at_fit_delivers_the_full_precision_codes_within_one_code() {
    let mut failures = Vec::new();
    let [_, (fit, patch, _)] = views();
    for backdrop in [false, true] {
        let [half, full] = [false, true].map(|hidden| document(RgbSpace::Srgb, LayerBlend::Normal, hidden, backdrop));
        failures.extend(compare(&presented(&half, fit, Some(&transform(&half))), &presented(&full, fit, Some(&transform(&full))),
            fit.width_px, patch, &format!("Transform backdrop={backdrop} Fit")));
    }
    assert!(failures.is_empty(), "{} patch channels exceed one code or shift on average:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn half_precision_cells_take_as_many_composition_dispatches_as_full_precision_cells() {
    for (view, _, label) in views() {
        for (writer, backdrop, offset, preview) in [("Combine", true, [0; 2], false), ("Transform", false, [0; 2], true), ("Placed", false, [37, 11], false)] {
            let [half, full] = [false, true].map(|hidden| {
                let mut doc = document(RgbSpace::Srgb, LayerBlend::Normal, hidden, backdrop);
                let upper = sweep(&doc);
                doc.artwork.occurrences.get_mut(upper).unwrap().offset = offset;
                let preview = preview.then(|| transform(&doc));
                settled(&doc, view, preview.as_ref()).1
            });
            assert_eq!([half.scene_dispatches, half.composed_texels], [full.scene_dispatches, full.composed_texels], "{writer} {label}: dispatches and composed texels");
        }
    }
}
