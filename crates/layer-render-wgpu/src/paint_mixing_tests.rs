//! Paint color mixing oracles: one Smudge pickup and one Wet pickup of pure
//! red and pure green at 50%, checked against CPU references for each space.
use super::*;
use layer_core::color::{DocumentColor, RgbSpace, oklab};
use layer_core::{BlendSpace, ColorMixSpace, Point, StrokeId};

const RED: [f32; 4] = [1., 0., 0., 1.];
const GREEN: [f32; 4] = [0., 1., 0., 1.];
const PROBE: [usize; 2] = [70, 64];

fn mixed(space: RgbSpace, mix: ColorMixSpace, a: [f32; 3], b: [f32; 3]) -> [f64; 3] {
    let [a, b] = [a, b].map(|c| c.map(f64::from));
    match mix {
        ColorMixSpace::LinearRgb => std::array::from_fn(|i| (a[i] + b[i]) / 2.),
        ColorMixSpace::Oklab => {
            let to_srgb = space.linear_transform(RgbSpace::Srgb);
            let [a, b] = [a, b].map(|c| oklab::to_lab(layer_core::color::rgb::apply(to_srgb, c)));
            let lab = std::array::from_fn(|i| (a[i] + b[i]) / 2.);
            layer_core::color::rgb::apply(RgbSpace::Srgb.linear_transform(space), oklab::from_lab(lab))
        }
        ColorMixSpace::Classic => std::array::from_fn(|i| space.decode((space.encode(a[i]) + space.encode(b[i])) / 2.)),
    }
}

fn encoded8(space: RgbSpace, linear: [f64; 3]) -> [u8; 3] {
    linear.map(|v| (space.encode(v).clamp(0., 1.) * 255.).round() as u8)
}

fn submit(r: &mut WgpuRasterizer, scene: SceneView<'_>, dabs: &[Dab], batches: &[DabBatch], reset: bool, space: BlendSpace) {
    let batches: Vec<_> = batches.iter().cloned().map(|mut b| {
        b.style.blend_space = space;
        b
    }).collect();
    r.submit(FramePacket { dabs, dab_batches: &batches, reset_layers: reset, blend_space: space, ..crate::test_support::packet(scene, [128, 128]) })
        .unwrap();
}

fn paint(r: &mut WgpuRasterizer, document: &Document, color: [f32; 4], center: [f32; 2], radius: f32, reset: bool, space: BlendSpace) {
    let mut dab = dab(color);
    dab.center = Point { x: center[0], y: center[1] };
    dab.radii = [radius; 2];
    submit(r, document.scene(), &[dab], &[batch(target(document))], reset, space);
}

/// Smudge drags the green left half 8 px into the red right half with an
/// influence of exactly one half; Wet deposits red carrying half its load over
/// green. Both leave the 50% mix at [`PROBE`].
fn pickup(color: DocumentColor, execution: BrushExecution, mix: ColorMixSpace, space: BlendSpace) -> WgpuRasterizer {
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let mut document = paint_document([128; 2], "mixing");
    let root = document.artwork.root;
    let composition = document.artwork.compositions.get_mut(root).unwrap();
    composition.color = color;
    composition.blend = space;
    paint(&mut r, &document, GREEN, [64., 64.], 200., true, space);
    let mut style = crate::tests::test_style(execution);
    style.wet_mix = layer_core::BrushWetMix { blur: 0., mix_space: mix, ..Default::default() };
    let mut contact = dab(RED);
    contact.radii = [200.; 2];
    if execution == BrushExecution::Smudge {
        paint(&mut r, &document, RED, [1066., 64.], 1000., false, space);
        style.wet_mix.attack = 0.5;
        contact.motion = [16., 0.];
        contact.material = [0., 0.5, 1., 0.];
    } else {
        style.wet_mix.amount_of_paint = 0.5;
        contact.material = [0., 0., 1., 0.];
    }
    let pickup = DabBatch { stroke_id: StrokeId(2), ..crate::test_support::dab_batch(target(&document), style, batch(target(&document)).damage) };
    submit(&mut r, document.scene(), &[contact], &[pickup], false, space);
    r
}

fn composite(r: &WgpuRasterizer) -> [f32; 3] {
    let bytes = page_bytes(r, crate::test_support::document_texture(r));
    let texel = &bytes[(PROBE[1] * 128 + PROBE[0]) * 16..][..12];
    std::array::from_fn(|i| f32::from_le_bytes(texel[i * 4..][..4].try_into().unwrap()))
}

#[test]
fn smudge_and_wet_pickups_mix_red_and_green_in_each_space() {
    for execution in [BrushExecution::Smudge, BrushExecution::Wet] {
        for mix in [ColorMixSpace::Oklab, ColorMixSpace::LinearRgb, ColorMixSpace::Classic] {
            let mut r = pickup(DocumentColor::default(), execution, mix, BlendSpace::Linear);
            let expected = encoded8(RgbSpace::Srgb, mixed(RgbSpace::Srgb, mix, [0., 1., 0.], [1., 0., 0.]));
            let actual = pixel(&mut r, PROBE[0], PROBE[1]);
            assert_eq!(actual[3], 255, "{execution:?} {mix:?}");
            for c in 0..3 {
                assert!(actual[c].abs_diff(expected[c]) <= 1, "{execution:?} {mix:?}: {actual:?} != {expected:?}");
            }
        }
    }
}

#[test]
fn classic_mixing_follows_the_document_transfer_curve() {
    for space in RgbSpace::ALL {
        let r = pickup(DocumentColor { space, ..Default::default() }, BrushExecution::Wet, ColorMixSpace::Classic, BlendSpace::Linear);
        let expected = mixed(space, ColorMixSpace::Classic, [0., 1., 0.], [1., 0., 0.]);
        for (actual, expected) in composite(&r).into_iter().zip(expected) {
            let error = (space.encode(f64::from(actual)) - space.encode(expected)).abs();
            assert!(error < 1e-4, "{space:?}: {actual} != {expected}");
        }
    }
}

#[test]
fn color_mixing_is_the_same_in_both_document_spaces() {
    for execution in [BrushExecution::Smudge, BrushExecution::Wet] {
        for mix in ColorMixSpace::ALL {
            let [linear, perceptual] = [BlendSpace::Linear, BlendSpace::Perceptual]
                .map(|space| pixel(&mut pickup(DocumentColor::default(), execution, mix, space), PROBE[0], PROBE[1]));
            assert!(linear.iter().zip(perceptual).all(|(a, b)| a.abs_diff(b) <= 1), "{execution:?} {mix:?}: {linear:?} != {perceptual:?}");
        }
    }
}

#[test]
fn every_mixing_space_shares_one_pass_plan() {
    use layer_core::DefaultBrushPreset as P;
    for preset in [P::Smudge, P::NaturalBlender, P::WetRound, P::LoadedOil, P::WatercolorWash] {
        let plans = ColorMixSpace::ALL.map(|mix| {
            let mut style = preset_style(preset);
            style.wet_mix.mix_space = mix;
            BrushPassPlan::for_style(&style)
        });
        assert!(plans.iter().all(|plan| *plan == plans[0]), "{preset:?}");
    }
}
