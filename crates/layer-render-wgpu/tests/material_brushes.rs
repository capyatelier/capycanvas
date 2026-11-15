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

#[test]
fn a_wet_stroke_whose_prediction_turns_across_page_corners_paints() {
    turn_across_page_corners(DefaultBrushPreset::WetRound);
}

#[test]
fn a_smudge_stroke_whose_prediction_turns_across_page_corners_paints() {
    turn_across_page_corners(DefaultBrushPreset::Smudge);
}
