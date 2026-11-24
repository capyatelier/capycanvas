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
        assert!(shaped.len() > plain.len(), "{preset:?}: the zones are subdivided");
        let zone = 2. * off.diameter * 1.2;
        let mut originals = shaped.iter();
        for (index, a) in plain.iter().enumerate() {
            let b = originals.find(|b| b.center == a.center).unwrap_or_else(|| panic!("{preset:?} {index}: a contact moved"));
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
    assert!(width(&dabs[0]) <= 0.15 * 18. && width(dabs.last().unwrap()) <= 0.15 * 18.);
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
fn proportional_zones_follow_the_stroke() {
    for (mode, lengths, stroke, zones) in [
        (TaperMode::Proportional, [0.3, 0.3], 300., [90., 90.]),
        (TaperMode::Proportional, [0.2, 0.4], 600., [120., 240.]),
        (TaperMode::Proportional, [0.5, 0.5], 4000., [900., 900.]),
        (TaperMode::Fixed, [2., 3.], 300., [36., 54.]),
    ] {
        let measured = taper_lengths(&pen(taper(mode, lengths)), line(stroke, |_| 1.), stroke);
        for (measured, zone) in measured.into_iter().zip(zones) {
            assert!((zone - 10.0..zone).contains(&measured), "{mode:?} {lengths:?}: {measured} for {zone}");
        }
    }
}

#[test]
fn ends_longer_than_the_line_meet_at_full_width_in_the_middle() {
    let dabs = draw(&pen(taper(TaperMode::Fixed, [10., 10.])), line(120., |_| 1.));
    let widest = dabs.iter().max_by(|a, b| width(a).total_cmp(&width(b))).unwrap();
    assert!((widest.center.x - 60.).abs() <= 2. && width(widest) > 0.99 * 18., "{} at {}", width(widest), widest.center.x);
    let rising: Vec<_> = dabs.iter().filter(|dab| dab.center.x <= 60.).map(width).collect();
    assert!(rising.windows(2).all(|pair| pair[1] >= pair[0]), "{rising:?}");
}

#[test]
fn a_short_fixed_line_tapers_over_its_whole_length() {
    for diameter in [18., 60.] {
        let mut brush = pen(taper(TaperMode::Fixed, [50., 50.]));
        brush.diameter = diameter;
        for widths in [5., 10., 25.] {
            let length = widths * diameter;
            let dabs = draw(&brush, line(length, |_| 1.));
            let tips = [width(&dabs[0]), width(dabs.last().unwrap())];
            assert!(tips.iter().all(|tip| *tip <= 0.15 * diameter), "{diameter} px, {widths} widths: tips {tips:?}");
            let widest = dabs.iter().max_by(|a, b| width(a).total_cmp(&width(b))).unwrap();
            assert!((widest.center.x - 0.5 * length).abs() <= 0.1 * length, "{diameter} px, {widths} widths: widest at {}", widest.center.x);
        }
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
    for lengths in [[2., 2.], [2., 0.], [0., 2.], [50., 50.]] {
        assert_eq!(draw(&pen(taper(TaperMode::Fixed, lengths)), points.clone()), off, "{lengths:?}");
    }
    let brush = pen(taper(TaperMode::Fixed, [50., 50.]));
    for length in [0.5, 2., 18.] {
        let widest = draw(&brush, line(length, |_| 1.)).iter().map(width).fold(0., f32::max);
        assert!(widest >= 0.9 * 18., "a {length} px line keeps {widest}");
    }
}

#[test]
fn the_start_follows_its_prefix_peak_and_the_end_its_running_peak() {
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
            Some(ContactRecord { pose: pose(d, pressure), variant: 0, index: 0, copy_seed: 0, copies: 1, interval, peak: *peak, reach: 0. })
        })
        .collect();
    let (profile, records) = HeldStroke::holding(&plan, records, &[]).profiled(&plan, 10.);
    let pressures: Vec<_> = records
        .iter()
        .map(|record| profile.transfer(record.pose.stroke_distance, record.peak, record.reach).map(|transfer| transfer.pressure))
        .collect();
    assert_eq!(pressures, [Some(0.2), Some(0.9), Some(0.9), Some(0.6), Some(0.7), Some(0.7)]);
}

#[test]
fn an_end_press_raises_only_the_contacts_from_the_press_to_the_tip() {
    let brush = pen(taper(TaperMode::Fixed, [10., 10.]));
    let press = |after: fn(f32) -> f32| line(400., move |t| match 400. * t {
        x if x < 380. => 0.3,
        x if x <= 384. => 1.,
        x => after(x),
    });
    let along = |dabs: &[Dab]| -> Vec<(f32, f32)> { dabs.iter().map(|dab| (dab.contact[2] * 18., width(dab))).collect() };
    let light = draw(&brush, line(400., |_| 0.3));
    let pressed = along(&draw(&brush, press(|_| 1.)));
    let at = |distance: f32| {
        let index = pressed.partition_point(|(d, _)| *d < distance).clamp(1, pressed.len() - 1);
        let ((d0, w0), (d1, w1)) = (pressed[index - 1], pressed[index]);
        w0 + (w1 - w0) * ((distance - d0) / (d1 - d0).max(f32::EPSILON)).clamp(0., 1.)
    };
    let before = |dabs: &[Dab]| dabs.iter().filter(|dab| dab.center.x < 360.).copied().collect::<Vec<_>>();
    for (label, after) in [("a light end", (|_| 0.3) as fn(f32) -> f32), ("a fade", |x| 1. - 0.95 * (x - 384.) / 16.)] {
        let dabs = draw(&brush, press(after));
        assert_eq!(before(&dabs), before(&light), "{label}: contacts before the press keep their own width");
        for (distance, width) in along(&dabs).into_iter().filter(|(distance, _)| *distance >= 384.) {
            let expected = at(distance);
            assert!((width - expected).abs() <= 0.04 * expected + 0.1, "{label} at {distance}: {width} against the press height {expected}");
        }
    }
}

#[test]
fn the_preview_chain_drops_straight_runs_and_keeps_bends() {
    let template = draw(&pen(BrushTaper::default()), line(10., |_| 1.))[0];
    let dab = |x: f32, y: f32| Dab { center: Point { x, y }, radii: [9.; 2], rotation: [1., 0.], ..template };
    let straight: Vec<_> = (0..200).map(|i| dab(i as f32 * 2., 0.)).collect();
    let kept = super::taper::selection(&straight, 1., |_| 0.04);
    assert!(kept.len() <= 200 / 32 + 2, "{}", kept.len());
    assert_eq!((kept.first(), kept.last()), (Some(&0), Some(&199)));
    let bent: Vec<_> = (0..200).map(|i| if i < 100 { dab(i as f32 * 2., 0.) } else { dab(198., (i - 99) as f32 * 2.) }).collect();
    assert!(super::taper::selection(&bent, 1., |_| 0.04).contains(&99), "the corner stays");
}

#[test]
fn the_preview_chain_keeps_its_points_when_the_held_start_moves() {
    let template = draw(&pen(BrushTaper::default()), line(10., |_| 1.))[0];
    let dab = |i: usize| {
        let x = i as f32 * 0.5;
        Dab { center: Point { x, y: 0. }, radii: [9.; 2], rotation: [1., 0.], contact: [1., 0., x / 18., 0.], ..template }
    };
    let chain: Vec<_> = (0..400).map(dab).collect();
    let centers = |start: usize| -> Vec<_> {
        super::taper::selection(&chain[start..], 1., |_| 0.02).into_iter().map(|index| chain[start + index].center.x).filter(|x| *x >= 36.).collect()
    };
    let whole = centers(0);
    for start in [1, 7, 30, 71] {
        assert_eq!(centers(start), whole, "held from {start}");
    }
}

fn ramped(length: f32, fade: f32) -> Vec<StrokePoint> {
    line(length, |t| (0.1 + 0.9 * (t * length / fade).min((1. - t) * length / fade)).min(1.))
}

fn taper_lengths(brush: &BrushSnapshot, points: Vec<StrokePoint>, length: f32) -> [f32; 2] {
    let mut off = brush.clone();
    off.taper = BrushTaper::default();
    let (plain, shaped) = (draw(&off, points.clone()), draw(brush, points));
    let changed: Vec<_> = plain
        .iter()
        .filter(|a| shaped.iter().find(|b| b.center == a.center).is_some_and(|b| b.radii != a.radii))
        .map(|a| a.center.x)
        .collect();
    let start = changed.iter().copied().filter(|x| *x < length / 2.).fold(0f32, f32::max);
    let end = changed.iter().copied().filter(|x| *x >= length / 2.).fold(length, f32::min);
    [start, length - end]
}

#[test]
fn a_light_touch_down_and_a_slow_lift_keep_full_length_tapers() {
    let brush = pen(taper(TaperMode::Fixed, [3., 3.]));
    for fade in [20., 80., 200.] {
        let lengths = taper_lengths(&brush, ramped(1200., fade), 1200.);
        assert!(lengths.iter().all(|length| *length >= 3. * 18. - 10.), "fade {fade}: {lengths:?}");
    }
}

fn end_profile(brush: &BrushSnapshot) -> f32 {
    let dabs = draw(brush, line(400., |_| 1.));
    let exponent = 0.5 + 0.5 * brush.taper.pointiness;
    let zone = brush.taper.fixed_widths[1] * 18.;
    let expected = |x: f32| 18. * (0.99 * super::taper::shaped(1., 1., 1., x / zone, exponent) + 0.01);
    let drawn = |x: f32| {
        let d = 400. - x;
        let pair = dabs.windows(2).find(|pair| pair[0].center.x <= d && d <= pair[1].center.x).unwrap();
        let t = (d - pair[0].center.x) / (pair[1].center.x - pair[0].center.x).max(f32::EPSILON);
        width(&pair[0]) + (width(&pair[1]) - width(&pair[0])) * t
    };
    (1..=72).map(|step| step as f32 * 0.25).map(|x| (drawn(x) - expected(x)).abs() / expected(x)).fold(0., f32::max)
}

#[test]
fn a_round_end_follows_its_profile_over_the_last_width() {
    let mut brush = pen(taper(TaperMode::Fixed, [3., 3.]));
    brush.taper.pointiness = 0.;
    let error = end_profile(&brush);
    assert!(error <= 0.1, "{error}");
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pressure {
    Constant(f32),
    RampIn,
    LiftFade(f32),
    Noisy,
}

#[derive(Clone, Copy, Debug)]
struct Situation {
    preset: DefaultBrushPreset,
    diameter: f32,
    widths: f32,
    pointiness: f32,
    pressure: Pressure,
    curved: bool,
}

struct Shape {
    shaped: Vec<(f32, f32)>,
    samples: Vec<(f32, f32)>,
    brush: BrushSnapshot,
    length: f32,
    zone: f32,
    raw: bool,
}

impl Situation {
    fn points(&self, length: f32) -> Vec<StrokePoint> {
        let step = (self.diameter / 9.).clamp(1., 8.);
        let count = (length / step).ceil() as u32;
        let radius = length / std::f32::consts::PI;
        (0..=count)
            .map(|i| {
                let d = (i as f32 * step).min(length);
                let pressure = match self.pressure {
                    Pressure::Constant(pressure) => pressure,
                    Pressure::RampIn => (0.1 + 0.9 * d / 20.).min(1.),
                    Pressure::LiftFade(fade) => if fade > 0. { (0.1 + 0.9 * (length - d) / fade).min(1.) } else { 1. },
                    Pressure::Noisy => 0.7 + 0.2 * signed_unit(hash32(i)),
                };
                let (x, y) = if self.curved { (radius * (d / radius).sin(), radius * (1. - (d / radius).cos())) } else { (d, 0.) };
                crate::test_support::point(x + 50., y + 50., pressure, i * 4_000)
            })
            .collect()
    }

    fn brush(&self, taper: BrushTaper) -> BrushSnapshot {
        let mut brush = layer_core::default_brush(self.preset);
        brush.stabilization.strength = 0.;
        brush.diameter = self.diameter;
        brush.taper = taper;
        brush
    }

    fn shape(&self, length: f32) -> Shape {
        let points = self.points(length);
        let mut on = taper(TaperMode::Fixed, [self.widths; 2]);
        on.pointiness = self.pointiness;
        let chain = |brush: BrushSnapshot| -> Vec<(f32, f32)> {
            draw(&brush, points.clone()).iter().map(|dab| (dab.contact[2] * self.diameter, width(dab))).collect()
        };
        let shaped = chain(self.brush(on));
        let mut travelled = 0.;
        let samples = points
            .iter()
            .enumerate()
            .map(|(index, point)| {
                if index > 0 {
                    let before = points[index - 1].position;
                    travelled += (point.position.x - before.x).hypot(point.position.y - before.y);
                }
                (travelled, point.pressure)
            })
            .collect();
        Shape { shaped, samples, brush: self.brush(BrushTaper::default()), length: travelled, zone: self.widths * self.diameter, raw: false }
    }
}

impl Shape {
    fn width_at(&self, distance: f32) -> f32 {
        let index = self.shaped.partition_point(|(d, _)| *d < distance).clamp(1, self.shaped.len() - 1);
        let ((d0, w0), (d1, w1)) = (self.shaped[index - 1], self.shaped[index]);
        w0 + (w1 - w0) * ((distance - d0) / (d1 - d0).max(f32::EPSILON)).clamp(0., 1.)
    }

    fn natural(&self, output: f32) -> f32 {
        let mapping = &self.brush.mappings[0];
        tip_diameter(&self.brush, super::taper::combine(self.brush.diameter, output.mul_add(mapping.output_scale, mapping.output_bias), mapping.combine))
    }

    fn span(&self, peak: f32) -> [f32; 2] {
        let mapping = &self.brush.mappings[0];
        let reference = self.natural(super::taper::mapping_output(mapping, peak, None).mul_add(1. / mapping.output_scale, -mapping.output_bias / mapping.output_scale));
        let floor = if self.raw { 0. } else { self.natural(0.) };
        [floor, reference - floor]
    }

    fn normalized(&self, distance: f32, peak: f32) -> f32 {
        let [floor, span] = self.span(peak);
        (self.width_at(distance) - floor) / span
    }

    fn slack(&self, peak: f32) -> f32 {
        let [_, span] = self.span(peak);
        (0.02 * span).max(MIN_RESOLVED_WIDTH) / span
    }

    fn dip(&self, from: f32, toward: f32) -> f32 {
        let widths: Vec<_> = (0..=1000).map(|i| self.width_at(from + (toward - from) * i as f32 / 1000.)).collect();
        let fall = widths.windows(2).map(|pair| pair[0] - pair[1]).fold(0., f32::max);
        let step = (self.width_at(toward + (toward - from) * 0.01) - self.width_at(toward - (toward - from) * 0.01)).abs();
        fall.max(step - 0.02 * widths[1000])
    }

    fn settles(&self) -> [bool; 2] {
        let inside = |from: f32, to: f32| self.samples.iter().filter(move |(d, _)| (from..=to).contains(d)).map(|(_, p)| *p);
        let start: Vec<_> = inside(0., self.zone).collect();
        let end: Vec<_> = inside(self.length - self.zone, self.length).collect();
        [start.windows(2).all(|pair| pair[1] >= pair[0]), end.windows(2).all(|pair| pair[1] <= pair[0])]
    }

    fn pressure_at(&self, distance: f32) -> f32 {
        let index = self.samples.partition_point(|(d, _)| *d < distance).clamp(1, self.samples.len() - 1);
        let ((d0, p0), (d1, p1)) = (self.samples[index - 1], self.samples[index]);
        p0 + (p1 - p0) * ((distance - d0) / (d1 - d0).max(f32::EPSILON)).clamp(0., 1.)
    }

    fn end_peak(&self) -> f32 {
        let edge = self.length - self.zone;
        self.samples.iter().filter(|(d, _)| *d >= edge).map(|(_, p)| *p).fold(self.pressure_at(edge), f32::max)
    }

    fn start_peak(&self, u: f32) -> f32 {
        let distance = u * self.zone;
        self.samples.iter().filter(|(d, _)| *d <= distance).map(|(_, p)| *p).fold(self.pressure_at(distance), f32::max)
    }

    fn end(&self, u: f32) -> f32 {
        self.normalized(self.length - u * self.zone, self.end_peak())
    }

    fn start(&self, u: f32) -> f32 {
        self.normalized(u * self.zone, self.start_peak(u))
    }
}

const MIN_RESOLVED_WIDTH: f32 = 0.05;

fn ellipse(u: f32) -> f32 {
    (1. - (1. - u) * (1. - u)).sqrt()
}

fn failures(situation: Situation, raw: bool) -> Vec<&'static str> {
    let length = 2. * situation.widths * situation.diameter + 6. * situation.diameter;
    let shape = Shape { raw, ..situation.shape(length) };
    let mut failed = Vec::new();
    let steady = matches!(situation.pressure, Pressure::Constant(_));
    let ends: Vec<Box<dyn Fn(f32) -> f32>> = match situation.pressure {
        Pressure::RampIn => vec![Box::new(|u| shape.start(u))],
        Pressure::LiftFade(_) => vec![Box::new(|u| shape.end(u))],
        _ => vec![Box::new(|u| shape.start(u)), Box::new(|u| shape.end(u))],
    };
    for profile in &ends {
        let reach = if steady { 1. } else { 0.5 };
        let grid = |to: f32| (1..=(to * 500.) as u32).map(|i| i as f32 / 500.);
        match situation.pointiness {
            0. => {
                if grid(reach).filter(|u| *u >= 0.02).any(|u| (profile(u) - ellipse(u)).abs() > 0.1 * ellipse(u)) { failed.push("a ellipse"); }
                if profile(0.25) < 0.6 { failed.push("a W(0.25)"); }
            }
            0.5 => {
                if !(0.08..=0.2).contains(&profile(0.02)) { failed.push("b W(0.02)"); }
                if profile(0.25) > 0.65 { failed.push("b W(0.25)"); }
            }
            _ => {
                if profile(0.02) > 0.06 { failed.push("c W(0.02)"); }
                if grid(1.).filter(|u| profile(*u) < 0.15).count() as f32 / 500. > 0.12 { failed.push("c thin share"); }
                if profile(0.25) < 0.4 { failed.push("c W(0.25)"); }
            }
        }
        if steady {
            let inside = (profile(1.) - profile(0.95)) / 0.05;
            let outside = (profile(1.05) - profile(1.)) / 0.05;
            if (inside - outside).abs() > 0.1 { failed.push("d entry"); }
            let samples: Vec<_> = (0..=500).map(|i| profile(i as f32 / 500.)).collect();
            if samples.windows(2).any(|pair| pair[1] < pair[0] - 1e-3) { failed.push("e monotone"); }
        }
    }
    if let Pressure::LiftFade(_) = situation.pressure {
        let reference = Shape { raw, ..Situation { pressure: Pressure::LiftFade(0.), ..situation }.shape(length) };
        if [0.1, 0.25, 0.5].iter().any(|u| (shape.end(*u) - reference.end(*u)).abs() > 0.08) { failed.push("f lift"); }
    }
    let outline = |u: f32| super::taper::outline(u, 0.5 + 0.5 * situation.pointiness);
    let grid = (0..=500).map(|i| i as f32 / 500.);
    let [start, end] = shape.settles();
    let dip = 2e-3 * situation.diameter;
    if start
        && (shape.dip(0., shape.zone) > dip
            || grid.clone().any(|u| shape.start(u) < outline(u) - shape.slack(shape.start_peak(u))))
    {
        failed.push("g start");
    }
    if end
        && (shape.dip(shape.length, shape.length - shape.zone) > dip
            || grid.clone().any(|u| shape.end(u) < outline(u) - shape.slack(shape.end_peak())))
    {
        failed.push("g end");
    }
    failed
}

fn situations() -> Vec<Situation> {
    let mut all = Vec::new();
    for preset in [DefaultBrushPreset::GPen, DefaultBrushPreset::Monoline] {
        for diameter in [6., 18., 100.] {
            for widths in [1., 3., 10., 50.] {
                for pointiness in [0., 0.5, 1.] {
                    for pressure in [
                        Pressure::Constant(1.),
                        Pressure::Constant(0.5),
                        Pressure::RampIn,
                        Pressure::LiftFade(20.),
                        Pressure::LiftFade(60.),
                        Pressure::LiftFade(200.),
                        Pressure::Noisy,
                    ] {
                        for curved in [false, true] {
                            all.push(Situation { preset, diameter, widths, pointiness, pressure, curved });
                        }
                    }
                }
            }
        }
    }
    all
}

#[test]
fn taper_shapes_hold_across_pens_sizes_lengths_and_pressures() {
    let failed: Vec<_> = situations()
        .into_iter()
        .filter_map(|situation| {
            let failed = failures(situation, false);
            (!failed.is_empty()).then(|| format!("{situation:?}: {failed:?}"))
        })
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

fn fixed_pen(diameter: f32, pointiness: f32) -> BrushSnapshot {
    let mut brush = layer_core::default_brush(DefaultBrushPreset::GPen);
    brush.stabilization.strength = 0.;
    brush.diameter = diameter;
    brush.taper = BrushTaper { mode: TaperMode::Fixed, pointiness, ..BrushTaper::default() };
    brush
}

fn held_preview(brush: &BrushSnapshot, period: f32) -> (DabGenerator, HeldStroke, u32) {
    let diameter = brush.diameter;
    let mut generator = DabGenerator::new(RgbSpace::Srgb);
    generator.reset_for_stroke(StrokeId(7), brush);
    generator.begin_taper(brush);
    let length = 25. * diameter;
    let radius = length / std::f32::consts::PI;
    let count = (length / 2.) as u32;
    let mut held = HeldStroke::default();
    let mut dabs = Vec::new();
    for index in 0..=count {
        let d = 2. * index as f32;
        let pressure = 0.7 + 0.3 * (d / period * std::f32::consts::TAU).sin();
        let point = crate::test_support::point(50. + radius * (d / radius).sin(), 50. + radius * (1. - (d / radius).cos()), pressure, index * 4_000);
        generator.append(point, brush, &mut dabs);
        held.stage(&mut generator, dabs.drain(..), index + 1);
        if index % 4 == 3 {
            held.seal();
            held.release(&generator, brush, Release::Settled, |_| {});
        }
    }
    held.seal();
    (generator, held, count)
}

fn preview_against_release(brush: &BrushSnapshot) -> (f32, usize, usize) {
    let (generator, held, count) = held_preview(brush, 250.);
    let diameter = brush.diameter;
    let mut tip = generator.preview();
    let mut output = Vec::new();
    tip.flush_contact(brush, &mut output);
    let (mut last, mut release, mut released) = (tip.clone(), held.clone(), Vec::new());
    release.stage(&mut last, output.iter().copied(), count + 2);
    release.seal();
    release.release(&last, brush, Release::All, |slice| released.extend_from_slice(slice));
    held.present(&mut tip, brush, 1., None, 0, &mut output);
    let along = |dabs: &[Dab]| -> Vec<(f32, f32)> { dabs.iter().map(|dab| (dab.contact[2] * diameter, width(dab))).collect() };
    let (preview, release) = (along(&output), along(&released));
    let swept = |pair: &[(f32, f32)], distance: f32| {
        let ((d0, w0), (d1, w1)) = (pair[0], pair[1]);
        let (r0, grow, step, offset) = (0.5 * w0, 0.5 * (w1 - w0), d1 - d0, distance - d0);
        let covered = |t: f32| (r0 + grow * t).powi(2) - (offset - step * t).powi(2);
        let widest = (step * step > grow * grow).then(|| covered(((grow * r0 + step * offset) / (step * step - grow * grow)).clamp(0., 1.)));
        covered(0.).max(covered(1.)).max(widest.unwrap_or(0.))
    };
    let at = |chain: &[(f32, f32)], distance: f32| {
        if brush.shape.size_jitter > 0. {
            return 2. * chain.windows(2).map(|pair| swept(pair, distance)).fold(0., f32::max).sqrt();
        }
        let index = chain.partition_point(|(d, _)| *d < distance).clamp(1, chain.len() - 1);
        let ((d0, w0), (d1, w1)) = (chain[index - 1], chain[index]);
        w0 + (w1 - w0) * ((distance - d0) / (d1 - d0).max(f32::EPSILON)).clamp(0., 1.)
    };
    let (first, end) = (preview[0].0, preview.last().unwrap().0);
    let worst = (0..=((end - first) / 0.25) as usize)
        .map(|step| first + step as f32 * 0.25)
        .map(|distance| 0.5 * (at(&preview, distance) - at(&release, distance)).abs() / (0.04 * at(&release, distance)).max(0.5))
        .fold(0., f32::max);
    let zone = 10. * diameter;
    let simplified = super::taper::selection(&released, 1., |dab| if dab.contact[2] * diameter >= release.last().unwrap().0 - zone { 0.04 } else { 0.08 }).len();
    (worst, output.len(), simplified)
}

#[test]
fn the_preview_outline_follows_the_saved_shape_without_evaluating_dropped_contacts() {
    let mut jittered = fixed_pen(18., 0.5);
    jittered.shape.size_jitter = 0.1;
    let mut copies = fixed_pen(18., 0.5);
    (copies.shape.count, copies.path.jitter_across) = (2, 0.2);
    let pens = [18., 100.].into_iter().flat_map(|diameter| [0., 0.5, 0.75, 1.].map(|pointiness| fixed_pen(diameter, pointiness)));
    for brush in pens.chain([jittered, copies]) {
        let (worst, drawn, simplified) = preview_against_release(&brush);
        let label = format!("{} px at {} with jitter {} and {} copies", brush.diameter, brush.taper.pointiness, brush.shape.size_jitter, brush.shape.count);
        assert!(worst <= 1., "{label}: outline off by {worst} of its tolerance");
        assert!(drawn <= 2 * simplified, "{label}: {drawn} preview dabs for {simplified} kept");
    }
}

#[test]
fn the_proportional_preview_outline_follows_the_saved_shape() {
    for diameter in [18., 60.] {
        for pointiness in [0., 0.5, 1.] {
            let mut brush = fixed_pen(diameter, pointiness);
            brush.taper.mode = TaperMode::Proportional;
            let (worst, drawn, simplified) = preview_against_release(&brush);
            assert!(worst <= 1., "{diameter} px at {pointiness}: outline off by {worst} of its tolerance");
            assert!(drawn <= 2 * simplified, "{diameter} px at {pointiness}: {drawn} preview dabs for {simplified} kept");
        }
    }
}

#[test]
fn the_preview_does_not_grow_with_the_held_length() {
    let drawn = [18., 100.].map(|diameter| {
        let brush = fixed_pen(diameter, 0.5);
        let (generator, held, _) = held_preview(&brush, 2.5 * diameter);
        let mut tip = generator.preview();
        let mut output = Vec::new();
        tip.flush_contact(&brush, &mut output);
        held.present(&mut tip, &brush, 1., None, 0, &mut output);
        output.len()
    });
    let held = [18f32, 100.].map(|diameter| (10. * diameter / 2.) as usize);
    assert!(held[1] >= 5 * held[0], "{held:?}");
    assert!(drawn[1] <= 2 * drawn[0], "{drawn:?} preview dabs for {held:?} held contacts");
}
