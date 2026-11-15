mod support;
use layer_core::{default_brush, DefaultBrushPreset, Document, DocumentNames, Point, authored::PortableId};
use support::*;

fn turn_across_page_corners(preset: DefaultBrushPreset) {
    let document = Document::new(PortableId::random(), 1024, 768, DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let (mut engine, mut input) = engine(document);
    draw(&mut engine, &mut input, [0.6, 0.02, 0.02, 1.], Point { x: 64., y: 250. }, Point { x: 960., y: 250. }, 16_000_000);
    engine.set_brush(layer_core::BrushSnapshot { diameter: 40., ..default_brush(preset) }).unwrap();
    let corners = [[200., 100.], [400., 100.], [400., 400.], [100., 400.], [100., 600.]];
    let mut time = 400_000_000;
    for (index, [x, y]) in corners.into_iter().enumerate() {
        input.push(pen_sample(index as u64, corners.len() as u64 - 1, time, Point { x, y })).unwrap();
        drain_input(&mut engine, time, false);
        time += 8_000_000;
    }
    settle(&mut engine, time);
}

fn blend_over_bands(presets: &[(f32, DefaultBrushPreset)]) {
    let document = Document::new(PortableId::random(), 1600, 1000, DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let (mut engine, mut input) = engine(document);
    let mut time = 16_000_000;
    for (band, color) in [[0.85, 0.55, 0.02, 1.], [0.6, 0.02, 0.02, 1.], [0.02, 0.12, 0.6, 1.], [0.02, 0.35, 0.18, 1.]].into_iter().enumerate() {
        for row in 0..7 {
            let y = 140. + 170. * band as f32 + 24. * row as f32;
            draw(&mut engine, &mut input, color, Point { x: 64., y }, Point { x: 1536., y }, time);
            time += 200_000_000;
        }
    }
    for &(x, preset) in presets {
        engine.set_brush(layer_core::BrushSnapshot { diameter: 54., ..default_brush(preset) }).unwrap();
        draw_with_brush(&mut engine, &mut input, Point { x, y: 100. }, Point { x: x + 40., y: 850. }, time);
        time += 400_000_000;
    }
}

#[test]
fn a_stable_blender_prediction_reads_the_committed_stroke_start_it_selected() {
    blend_over_bands(&[(693., DefaultBrushPreset::ScatterBlender)]);
}

#[test]
fn a_stable_blender_after_another_blender_continues_its_own_coverage() {
    blend_over_bands(&[(626., DefaultBrushPreset::HueBlender), (693., DefaultBrushPreset::ScatterBlender)]);
}

#[test]
fn a_wet_stroke_whose_prediction_turns_across_page_corners_paints() {
    turn_across_page_corners(DefaultBrushPreset::WetRound);
}

#[test]
fn a_smudge_stroke_whose_prediction_turns_across_page_corners_paints() {
    turn_across_page_corners(DefaultBrushPreset::Smudge);
}
