use super::*;
use layer_core::{LayerBlend, color::RgbSpace};

const SIDE: u32 = 256;
const PATCH: u32 = 16;

fn document(space: RgbSpace, blend: LayerBlend, hidden_effect: bool) -> layer_core::Document {
    let extent = [SIDE; 2];
    let mut artwork = layer_core::authored::Artwork::new(extent).unwrap();
    let (upper, _) = crate::test_support::add_paint(&mut artwork, "Sweep", extent);
    let (lower, _) = crate::test_support::add_paint(&mut artwork, "Backdrop", extent);
    let composition = artwork.compositions.get_mut(artwork.root).unwrap();
    composition.color.depth = layer_core::color::SampleDepth::U8;
    composition.color.space = space;
    artwork.occurrences.get_mut(upper).unwrap().blend = blend;
    let backdrop = layer_core::color::source::rgba8_source(extent, |x, y| {
        let patch = (y / PATCH) * (SIDE / PATCH) + x / PATCH;
        [(16 + patch * 7 % 224) as u8, (24 + patch * 13 % 208) as u8, (32 + patch * 29 % 192) as u8, 255]
    });
    let sweep = layer_core::color::source::rgba8_source(extent, |x, y| {
        let patch = (y / PATCH) * (SIDE / PATCH) + x / PATCH;
        let alpha = [1, 2, 17, 64, 128, 191, 238, 254, 255][patch as usize % 9];
        [(3 + patch * 37 % 250) as u8, (5 + patch * 59 % 248) as u8, (7 + patch * 83 % 246) as u8, alpha]
    });
    for (occurrence, source) in [(lower, backdrop), (upper, sweep)] {
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

fn presented(doc: &layer_core::Document, view: layer_render::ViewState) -> Vec<u8> {
    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut frame = crate::test_support::packet(doc.scene(), [SIDE; 2]);
    frame.view = view;
    crate::test_support::submit_settled(&mut renderer, frame);
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
        let plain = presented(&document(RgbSpace::DisplayP3, LayerBlend::Normal, false), view);
        let marked = presented(&document(RgbSpace::DisplayP3, LayerBlend::Normal, true), view);
        assert_eq!(plain, marked, "{label}: a hidden effect is the corpus's full-precision reference and must not change pixels");
    }
}

#[test]
fn half_precision_display_delivers_the_full_precision_codes_within_one_code() {
    let mut failures = Vec::new();
    for blend in [LayerBlend::Normal, LayerBlend::Multiply, LayerBlend::Screen, LayerBlend::Difference, LayerBlend::HardMix, LayerBlend::ColorDodge] {
        let half = document(RgbSpace::Srgb, blend, false);
        let full = document(RgbSpace::Srgb, blend, true);
        assert_eq!(crate::display_precision::DisplayPrecision::for_scene(half.scene(), false), crate::display_precision::DisplayPrecision::Half);
        assert_eq!(crate::display_precision::DisplayPrecision::for_scene(full.scene(), false), crate::display_precision::DisplayPrecision::Full);
        for (view, patch, label) in views() {
            failures.extend(compare(&presented(&half, view), &presented(&full, view), view.width_px, patch, &format!("{blend:?} {label}")));
        }
    }
    assert!(failures.is_empty(), "{} patch channels exceed one code or shift on average:\n{}", failures.len(), failures.join("\n"));
}
