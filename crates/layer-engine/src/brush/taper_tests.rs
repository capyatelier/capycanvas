use super::*;
use layer_core::{BrushTaper, DefaultBrushPreset, TaperMode};

const MATERIALS: [DefaultBrushPreset; 5] = [
    DefaultBrushPreset::Paintbrush,
    DefaultBrushPreset::Marker,
    DefaultBrushPreset::TexturedFlat,
    DefaultBrushPreset::DryScumble,
    DefaultBrushPreset::TransparentGlaze,
];

const INK: [DefaultBrushPreset; 9] = [
    DefaultBrushPreset::GPen,
    DefaultBrushPreset::Monoline,
    DefaultBrushPreset::RoughGPen,
    DefaultBrushPreset::CalligraphyPen,
    DefaultBrushPreset::AntiquePen,
    DefaultBrushPreset::RealisticPen,
    DefaultBrushPreset::WetInk,
    DefaultBrushPreset::BlottyInk,
    DefaultBrushPreset::BrushedInk,
];

fn taper(mode: TaperMode, lengths: [f32; 2]) -> BrushTaper {
    let mut taper = BrushTaper { mode, ..BrushTaper::default() };
    if mode == TaperMode::Proportional { taper.proportional_shares = lengths } else { taper.fixed_widths = lengths }
    taper
}

fn pen(taper: BrushTaper) -> BrushSnapshot {
    let mut brush = layer_core::default_brush(DefaultBrushPreset::GPen);
    brush.stabilization.strength = 0.;
    brush.taper = taper;
    brush
}

fn line(length: f32, pressure: impl Fn(f32) -> f32) -> Vec<StrokePoint> {
    let steps = (length / 2.).ceil().max(1.) as u32;
    (0..=steps)
        .map(|i| {
            let x = length * i as f32 / steps as f32;
            crate::test_support::point(x, 0., pressure(x / length.max(1.)), i * 4_000)
        })
        .collect()
}

fn draw(brush: &BrushSnapshot, points: Vec<StrokePoint>) -> Vec<Dab> {
    let stroke = Stroke::new(
        StrokeId(31),
        layer_core::SourceTarget::Paint(layer_core::authored::PaintHandle::from_index(1)),
        layer_core::StrokeTool::Brush,
        brush.clone(),
        points,
    )
    .unwrap();
    let mut dabs = Vec::new();
    DabGenerator::generate(&stroke, RgbSpace::Srgb, &mut dabs);
    dabs
}

fn width(dab: &Dab) -> f32 {
    dab.radii[0] * 2.
}

fn full(dab: &Dab) -> bool {
    (width(dab) - 18.).abs() < 1e-3
}

#[test]
fn only_uniform_contact_brushes_without_dry_media_taper() {
    for preset in layer_core::DefaultBrushPreset::ALL {
        let mut brush = layer_core::default_brush(preset);
        brush.taper = taper(TaperMode::Fixed, [2., 2.]);
        assert_eq!(tapers(&brush), INK.contains(&preset) || MATERIALS.contains(&preset), "{preset:?}");
        brush.taper.mode = TaperMode::Off;
        assert!(!tapers(&brush), "{preset:?}");
    }
    let mut brush = pen(taper(TaperMode::Proportional, [0., 0.]));
    assert!(!tapers(&brush), "zero lengths are Off");
    brush.taper.proportional_shares = [0., 0.2];
    assert!(tapers(&brush));
    let mut mappings = brush.mappings.to_vec();
    (mappings[0].input_min, mappings[0].input_max) = (1., 0.);
    brush.mappings = mappings.into();
    assert!(!tapers(&brush), "a reversed pressure range is rejected");
}

#[test]
fn an_overflowing_mapping_bounds_widths_by_the_largest_size() {
    let mut brush = pen(taper(TaperMode::Fixed, [1., 1.]));
    let mut mappings = brush.mappings.to_vec();
    mappings[0].output_scale = 1e38;
    mappings[0].output_bias = 0.;
    brush.mappings = mappings.into();
    assert_eq!(TaperPlan::new(&brush).unwrap().full_width, tip_diameter(&brush, brush.bounds.maximum_size));
}

#[test]
fn taper_leaves_placement_and_middle_contacts_untouched() {
    for preset in INK {
        let mut off = layer_core::default_brush(preset);
        off.stabilization.strength = 0.;
        let mut on = off.clone();
        on.taper = taper(TaperMode::Fixed, [2., 2.]);
        let points = line(600., |t| 0.6 + 0.3 * (t * 9.).sin());
        let (plain, shaped) = (draw(&off, points.clone()), draw(&on, points));
        assert_eq!(plain.len(), shaped.len(), "{preset:?}");
        let zone = 2. * off.diameter * 1.2;
        for (index, (a, b)) in plain.iter().zip(&shaped).enumerate() {
            assert_eq!(a.center, b.center, "{preset:?} {index}");
            let middle = |dab: &Dab| dab.center.x > zone && dab.center.x < 600. - zone;
            if index > 0 && middle(&plain[index - 1]) && middle(a) {
                assert_eq!(a, b, "{preset:?} {index}");
            }
        }
        assert_ne!(plain.first(), shaped.first(), "{preset:?}");
        assert_ne!(plain.last(), shaped.last(), "{preset:?}");
    }
}

#[test]
fn fixed_ends_come_to_points_over_their_widths() {
    let brush = pen(taper(TaperMode::Fixed, [2., 3.]));
    let dabs = draw(&brush, line(400., |_| 1.));
    assert!(width(&dabs[0]) < 0.2 * 18. && width(dabs.last().unwrap()) < 0.2 * 18.);
    let start: Vec<_> = dabs.iter().filter(|dab| dab.center.x <= 36.).map(width).collect();
    let end: Vec<_> = dabs.iter().filter(|dab| dab.center.x >= 400. - 54.).map(width).collect();
    assert!(start.windows(2).all(|pair| pair[1] >= pair[0]), "{start:?}");
    assert!(end.windows(2).all(|pair| pair[1] <= pair[0]), "{end:?}");
    for dab in &dabs {
        let x = dab.center.x;
        assert_eq!(full(dab), (36.0..=346.0).contains(&x), "{x}");
    }
}

#[test]
fn proportional_zones_follow_the_stroke_and_each_stays_within_a_third() {
    for (mode, lengths, stroke, zone) in [
        (TaperMode::Proportional, [0.3, 0.3], 300., 90.),
        (TaperMode::Proportional, [0.5, 0.5], 300., 100.),
        (TaperMode::Fixed, [10., 10.], 60., 20.),
    ] {
        let dabs = draw(&pen(taper(mode, lengths)), line(stroke, |_| 1.));
        let first = dabs.iter().find(|dab| full(dab)).unwrap().center.x;
        let last = dabs.iter().rev().find(|dab| full(dab)).unwrap().center.x;
        assert!((zone..zone + 10.).contains(&first), "{mode:?} {lengths:?}: start {first}");
        assert!((stroke - zone - 10.0..=stroke - zone).contains(&last), "{mode:?} {lengths:?}: end {last}");
    }
}

#[test]
fn a_tap_stays_a_full_dot() {
    let points = vec![
        crate::test_support::point(50., 50., 0.7, 0),
        crate::test_support::point(50., 50., 0.9, 4_000),
        crate::test_support::point(50., 50., 0.8, 8_000),
    ];
    let off = draw(&pen(BrushTaper::default()), points.clone());
    assert_eq!(draw(&pen(taper(TaperMode::Fixed, [2., 2.])), points), off);
}

#[test]
fn each_end_measures_from_its_own_peak_pressure() {
    let brush = pen(taper(TaperMode::Proportional, [0.3, 0.3]));
    let plan = TaperPlan::new(&brush).unwrap();
    let pose = |d: f32, pressure: f32| DynamicPoint {
        point: crate::test_support::point(d, 0., pressure, d as u32),
        speed: 0.,
        direction_turns: 0.,
        stroke_distance: d,
    };
    let records: Vec<_> = [(0., 0.2, 0.2), (1., 0.3, 0.9), (2., 0.5, 0.5), (8., 0.6, 0.6), (9., 0.4, 0.7), (10., 0.1, 0.4)]
        .into_iter()
        .scan(0f32, |peak, (d, pressure, interval)| {
            *peak = peak.max(interval);
            Some(ContactRecord { pose: pose(d, pressure), variant: 0, index: 0, copy_seed: 0, copies: 1, interval, peak: *peak })
        })
        .collect();
    let transfers = plan.transfers(&brush, &records, 0., 10., 0.35);
    let pressures: Vec<_> = transfers.iter().map(|transfer| transfer.map(|transfer| transfer.pressure)).collect();
    assert_eq!(pressures, [Some(0.2), Some(0.9), Some(0.9), Some(0.7), Some(0.4), Some(0.35)]);
}
