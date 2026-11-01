mod support;
use layer_core::*;
use layer_engine::{InstantFeedbackConfig, ViewTransform};
use layer_render::ViewState;
use std::sync::Arc;

const EXTENT: [u32; 2] = [256; 2];
const PRESETS: [DefaultBrushPreset; 7] = [DefaultBrushPreset::Pencil,
    DefaultBrushPreset::PointyPencil, DefaultBrushPreset::ShadingPencil,
    DefaultBrushPreset::Charcoal, DefaultBrushPreset::Chalk,
    DefaultBrushPreset::PastelBlock, DefaultBrushPreset::Crayon];

#[derive(Clone, Copy)]
struct Run { samples: u64, cadence: u64, feedback: bool, scale: f32, mask: bool, offset: [i64; 2] }
impl Default for Run {
    fn default() -> Self { Self { samples: 64, cadence: 4, feedback: false, scale: 1., mask: false, offset: [0; 2] } }
}

fn contact_engine(mut brush: BrushSnapshot, run: Run)
    -> (support::Engine, layer_engine::InputProducer<layer_engine::PenEvent>) {
    let mut document = Document::new(PortableId::random(), EXTENT[0], EXTENT[1],
        DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let paper = support::named_occurrence(&document, "Paper");
    document.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    striped_paper(&mut document, paper);
    let ink = document.working.occurrence.unwrap();
    document.artwork.occurrences.get_mut(ink).unwrap().offset = run.offset;
    if run.mask {
        let owner = document.working.occurrence.unwrap();
        let mut mask = CoverageSnapshot::reveal_all(document.artwork.coverage.next_handle(), EXTENT, [0; 2]);
        mask.source.default_coverage = 0.;
        document.apply(support::mask_edit(&document, owner, mask)).unwrap();
        document.working.target = Some(SourceTarget::Coverage(document.scene().occurrence(owner).unwrap().mask.as_ref().unwrap().source));
        support::paint_mut(&mut document, owner).base = Some(layer_core::authored::PaintBase::new(
            color::source::rgba8_source(EXTENT, |_, _| [255; 4]).into()));
        brush.color_rgba_linear = [1.; 4];
    }
    let (mut engine, input) = support::engine(document);
    engine.set_view(ViewState {
        width_px: EXTENT[0], height_px: EXTENT[1],
        document_to_surface: [run.scale, 0., 0., run.scale, 0., 0.],
    }, ViewTransform::IDENTITY);
    engine.set_instant_feedback(InstantFeedbackConfig { enabled: run.feedback, ..Default::default() }).unwrap();
    engine.set_brush(brush).unwrap();
    engine.render_frame_at(0).unwrap();
    (engine, input)
}

fn paint(brush: BrushSnapshot, run: Run, pose: impl Fn(f32) -> ([f32; 2], f32, [f32; 2])) -> support::Image {
    let (mut engine, mut input) = contact_engine(brush, run);
    let mut time = 1_000_000_000;
    for i in 0..=run.samples {
        time += 4_000_000;
        let (position, pressure, tilt) = pose(i as f32 / run.samples as f32);
        let mut event = support::pen_sample(i, run.samples, time, Point { x: position[0], y: position[1] });
        event.sequence = time;
        event.pressure = pressure;
        event.tilt_radians = tilt;
        input.push(event).unwrap();
        if i % run.cadence == 0 || i == run.samples {
            support::drain_input(&mut engine, time, true);
        }
    }
    support::image(&mut engine, time)
}

fn striped_paper(document: &mut Document, paper: authored::OccurrenceHandle) {
    use color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
    let interpretation = SourceInterpretation { channels: SourceChannels::Gray, depth: color::SampleDepth::U8,
        profile: Default::default(), profile_assumed: false };
    let mut builder = SourceBuilder::new([1, 256], interpretation, 1 << 20).unwrap();
    for y in 0..256 { builder.push_row(&[if y % 16 < 8 { 255 } else { 0 }]).unwrap(); }
    let mut source = builder.finish().unwrap();
    source.resolution = Some(ImageResolution::ppi(300));
    let effect = document.scene().effect_handle(paper).unwrap();
    let application = document.artwork.effects.get_mut(effect).unwrap();
    let index = application.program.parameters.iter().position(|p| p.key.as_ref() == "texture").unwrap();
    application.values[index] = EffectValue::Texture(TextureSource::Image(Image::new(Arc::new(source))));
}

fn line(brush: BrushSnapshot, pressure: f32, tilt: f32, run: Run) -> support::Image {
    paint(brush, run, |t| ([32.5 + 192. * t, 128.5], pressure,
        [tilt * std::f32::consts::FRAC_PI_2, 0.]))
}

fn passes(brush: BrushSnapshot, pressures: &[f32], cadence: u64) -> Vec<support::Image> {
    let (mut engine,mut input)=contact_engine(brush,Run::default());
    let mut time=1_000_000_000;
    pressures.iter().map(|&pressure| pass(&mut engine,&mut input,&mut time,pressure,cadence)).collect()
}

fn pass(engine:&mut support::Engine,input:&mut layer_engine::InputProducer<layer_engine::PenEvent>,
    time:&mut u64,pressure:f32,cadence:u64)->support::Image {
    for i in 0..=64 {
        *time+=4_000_000;
        let mut event=support::pen_sample(i,64,*time,Point {x:32.5+3.*i as f32,y:128.5});
        event.sequence=*time;event.pressure=pressure;
        input.push(event).unwrap();
        if i%cadence==0 || i==64 {support::drain_input(engine,*time,true);}
    }
    support::image(engine,*time)
}

fn flat_dry_brush() -> BrushSnapshot {
    let mut brush=analytic(0.,0.);
    brush.flow=1.;brush.color_rgba_linear[3]=0.2;brush.stabilization.strength=0.;
    let dry=brush.contact.as_mut().unwrap().dry.as_mut().unwrap();
    dry.loading=1.;
    for pose in &mut dry.poses {pose.relief=[0.;3];}
    brush
}

#[test]
fn dry_grip_light_over_heavy_adds_nonzero_dose() {
    let brush=flat_dry_brush();
    let images=passes(brush.clone(),&[1.,0.3],4);
    let heavy=6.*f64::from(brush.color_rgba_linear[3])*reference_exposure(128,1.,&brush);
    let light=6.*f64::from(brush.color_rgba_linear[3])*reference_exposure(128,0.3,&brush);
    let expected=grip_dose(heavy,light,light,material_pressure(0.3,&brush));
    let alpha=|dose:f64| (255.*(1.-(-dose).exp())).round() as u8;
    let actual=images[1].rgba[(128*256+128)*4+3];
    assert!(actual>images[0].rgba[(128*256+128)*4+3],"light contact must add pigment over a heavy pass");
    assert!(actual.abs_diff(alpha(expected))<=1,"light over heavy: {actual}, expected {}",alpha(expected));
    let blank=line(brush.clone(),0.3,0.,Run::default());
    assert!(blank.rgba[(128*256+128)*4+3].abs_diff(alpha(light))<=1);
}

#[test]
fn dry_grip_repeated_light_pass_adds_the_pressure_grip() {
    let brush=flat_dry_brush();
    let images=passes(brush.clone(),&[0.14,0.14],4);
    let density=|image:&support::Image| -(1.-f64::from(image.rgba[(128*256+128)*4+3])/255.).ln();
    let first=density(&images[0]);let second=density(&images[1]);
    let full=6.*f64::from(brush.color_rgba_linear[3])*reference_exposure(128,0.14,&brush);
    let expected=grip_dose(full,full,full,material_pressure(0.14,&brush));
    let ratio=(second-first)/first;
    assert!((ratio-(expected-full)/full).abs()<0.05,"density gain ratio {ratio}, expected {}",(expected-full)/full);
}

#[test]
fn dry_grip_at_full_strength_and_pen_is_additive() {
    let brush=flat_dry_brush();
    let images=passes(brush.clone(),&[1.,1.],4);
    let full=6.*f64::from(brush.color_rgba_linear[3])*reference_exposure(128,1.,&brush);
    let alpha=|dose:f64| (255.*(1.-(-dose).exp())).round() as u8;
    assert!(images[0].rgba[(128*256+128)*4+3].abs_diff(alpha(full))<=1);
    let actual=images[1].rgba[(128*256+128)*4+3];
    assert!(actual.abs_diff(alpha(2.*full))<=1,"full Strength and pen: {actual}, additive {}",alpha(2.*full));
}

#[test]
fn pencil_default_strength_retains_partial_grip() {
    let mut brush=default_brush(DefaultBrushPreset::Pencil);
    brush.color_rgba_linear[3]=0.2;
    for pressure in [1.,0.9] {
        let images=passes(brush.clone(),&[pressure,pressure],4);
        let full=6.*f64::from(brush.color_rgba_linear[3])*reference_exposure(128,f64::from(pressure),&brush);
        let expected=grip_dose(full,full,full,material_pressure(f64::from(pressure),&brush));
        let alpha=|dose:f64| (255.*(1.-(-dose).exp())).round() as u8;
        assert!(images[0].rgba[(128*256+128)*4+3].abs_diff(alpha(full))<=1);
        let actual=images[1].rgba[(128*256+128)*4+3];
        assert!(actual.abs_diff(alpha(expected))<=1,"pen {pressure}: {actual}, expected {}",alpha(expected));
        assert!(actual+2<alpha(2.*full),"pen {pressure}: {actual}, additive {}",alpha(2.*full));
    }
}

#[test]
fn dry_grip_at_half_strength_and_full_pen_is_partial() {
    let mut brush=flat_dry_brush();brush.flow=0.5;
    let images=passes(brush.clone(),&[1.,1.],4);
    let full=6.*f64::from(brush.color_rgba_linear[3])*reference_exposure(128,1.,&brush);
    let expected=grip_dose(full,full,full,material_pressure(1.,&brush));
    let alpha=|dose:f64| (255.*(1.-(-dose).exp())).round() as u8;
    assert!(images[0].rgba[(128*256+128)*4+3].abs_diff(alpha(full))<=1);
    let actual=images[1].rgba[(128*256+128)*4+3];
    assert!(actual.abs_diff(alpha(expected))<=1,"Strength .5: {actual}, expected {}",alpha(expected));
    assert!(actual+2<alpha(2.*full),"Strength .5: {actual}, additive {}",alpha(2.*full));
}

#[test]
fn dry_grip_heavy_over_light_retains_the_light_residue() {
    let mut brush=flat_dry_brush();brush.color_rgba_linear[3]=0.5;
    let images=passes(brush.clone(),&[0.2,0.35],4);
    let light=6.*f64::from(brush.color_rgba_linear[3])*reference_exposure(128,0.2,&brush);
    let heavy=6.*f64::from(brush.color_rgba_linear[3])*reference_exposure(128,0.35,&brush);
    let expected=grip_dose(light,heavy,heavy,material_pressure(0.35,&brush));
    let alpha=|dose:f64| (255.*(1.-(-dose).exp())).round() as u8;
    let actual=images[1].rgba[(128*256+128)*4+3];
    assert!(actual.abs_diff(alpha(expected))<=1,"heavy over light: {actual}, expected {}",alpha(expected));
    assert!(actual>alpha(heavy));
}

#[test]
fn dry_grip_grouped_opacity_preserves_light_over_heavy_across_cadences() {
    let mut brush=analytic(0.,0.);
    brush.flow=1.;brush.opacity=0.5;brush.color_rgba_linear[3]=0.2;brush.stabilization.strength=0.;
    let reference=passes(brush.clone(),&[1.,0.25],1);
    let heavy=6.*f64::from(brush.color_rgba_linear[3])*reference_exposure(128,1.,&brush);
    let light=6.*f64::from(brush.color_rgba_linear[3])*reference_exposure(128,0.25,&brush);
    let first=0.5*(1.-(-heavy).exp());
    let dose=grip_dose(-(1.-first).ln(),light,light,material_pressure(0.25,&brush));
    let expected=(255.*(0.5*first+0.5*(1.-(-dose).exp()))).round() as u8;
    assert!(reference[1].rgba[(128*256+128)*4+3].abs_diff(expected)<=1);
    let batched=passes(brush,&[1.,0.25],17);
    for (a,b) in reference.iter().zip(&batched) {a.assert_near(b,1,"grouped grip cadence");}
}

#[test]
fn dry_full_strength_and_pressure_color_covers_graphite_and_saturates() {
    let mut brush=default_brush(DefaultBrushPreset::Pencil);
    brush.diameter=80.;brush.flow=1.;brush.stabilization.strength=0.;brush.color_rgba_linear=[0.,0.,0.,1.];
    let (mut engine,mut input)=contact_engine(brush.clone(),Run::default());
    let mut time=1_000_000_000;
    let first=pass(&mut engine,&mut input,&mut time,1.,4);
    for _ in 0..2 {pass(&mut engine,&mut input,&mut time,1.,4);}
    brush.color_rgba_linear=[1.,0.,0.,1.];engine.set_brush(brush.clone()).unwrap();
    let actual=pass(&mut engine,&mut input,&mut time,1.,4);
    let linear=engine.document().composition().blend==BlendSpace::Linear;
    let expected=support::Image {size:first.size,rgba:first.rgba.as_chunks::<4>().0.iter().flat_map(|pixel| {
        let a=f64::from(pixel[3])/255.;let alpha=1.-(1.-a).powi(4);
        let red=if alpha>0. {a/alpha} else {0.};
        let red=if linear {color::RgbSpace::Srgb.encode(red)} else {red};
        [(255.*red).round() as u8,0,0,(255.*alpha).round() as u8]
    }).collect()};
    actual.assert_near(&expected,2,"full-Strength, full-pressure red over three heavy black passes");
    brush.contact.as_mut().unwrap().paper=0.;
    for hardness in [0.,0.5,1.] {
        brush.contact.as_mut().unwrap().dry.as_mut().unwrap().loading=hardness;
        let image=line(brush.clone(),1.,0.,Run::default());
        assert!(image.rgba[(128*256+128)*4+3]>=253,"Strength 100%, hardness {hardness}: saturated core");
    }
}

#[test]
fn light_dry_caps_match_additive_reference() {
    for (preset,pressure,hardness,paper,pixel,expected) in [
        (DefaultBrushPreset::PointyPencil,0.15,0.,0.563,[33,128],71),
        (DefaultBrushPreset::PointyPencil,0.15,0.25,0.564,[33,128],52),
        (DefaultBrushPreset::Chalk,0.3,0.,0.627,[226,132],193),
    ] {
        let mut brush=default_brush(preset);
        let contact=brush.contact.as_mut().unwrap();
        contact.paper*=2.*paper-1.;contact.dry.as_mut().unwrap().loading=hardness;
        let image=line(brush,pressure,0.,Run::default());
        let actual=image.rgba[(pixel[1]*256+pixel[0])*4+3];
        assert!(actual.abs_diff(expected)<=1,"{preset:?} pressure={pressure} hardness={hardness} cap={pixel:?}: alpha {actual}, additive {expected}");
    }
}

fn analytic(corner: f32, paper: f32) -> BrushSnapshot {
    let mut brush = default_brush(DefaultBrushPreset::Pencil);
    brush.diameter = 80.;
    brush.flow = 0.3;
    brush.color_rgba_linear = [0., 0., 0., 1.];
    brush.mappings = Arc::from([]);
    let contact = brush.contact.as_mut().unwrap();
    contact.paper = paper;
    contact.pressure_gain = 1.;
    contact.dry = Some(BrushDryMedia { poses: [BrushDryPose { radii: [1.; 2],
        relief: [0.08, 0.75, 0.3] }; 3], softness: 0.07, loading: 0.3, corner, relief_offset: 0. });
    brush
}

fn contact_q(position: [f64; 2], brush: &BrushSnapshot) -> f64 {
    let dry = brush.contact.unwrap().dry.unwrap();
    let radii = dry.poses[0].radii.map(|v| f64::from(v*brush.diameter*0.5));
    let (sin, cos) = f64::from(brush.angle_radians).sin_cos();
    let [x, y] = position;
    let (u, v) = (x*cos+y*sin, y*cos-x*sin);
    if dry.corner == 0. { return (u/radii[0]).powi(2)+(v/radii[1]).powi(2); }
    let rim = f64::from(dry.corner)*radii[0].min(radii[1]);
    (((u.abs()-radii[0]+rim).max(0.)+(v.abs()-radii[1]+rim).max(0.))/rim).powi(2)
}

fn relief_q(position: [f64; 2], brush: &BrushSnapshot) -> f64 {
    let dry = brush.contact.unwrap().dry.unwrap();
    let radii = dry.poses[0].radii.map(|v| f64::from(v*brush.diameter*0.5));
    let (sin, cos) = f64::from(brush.angle_radians).sin_cos();
    let [x, y] = position;
    let u = (x*cos+y*sin)/radii[0] + f64::from(dry.relief_offset);
    let v = (y*cos-x*sin)/radii[1];
    u*u+v*v
}

fn material_pressure(pressure: f64, brush: &BrushSnapshot) -> f64 {
    (pressure*f64::from(brush.contact.unwrap().pressure_gain)).clamp(0., 1.)*f64::from(brush.flow)
}

fn material_response(pressure:f64, brush:&BrushSnapshot)->(f64,f64) {
    let hardness=f64::from(brush.contact.unwrap().dry.unwrap().loading);
    let softness=(1.-hardness).powi(2);
    (pressure,softness+(1.-softness)*pressure*pressure)
}

fn reference_exposure(y: usize, pressure: f64, brush: &BrushSnapshot) -> f64 {
    let contact = brush.contact.unwrap();
    let dry = contact.dry.unwrap();
    let profile = dry.poses[0].relief.map(f64::from);
    let height = if y % 16 < 8 { 1. } else { 0. };
    let pressure = material_pressure(pressure, brush);
    let (contact_pressure,loading)=material_response(pressure,brush);
    let d = 0.5 - (0.5 + f64::from(contact.paper) * (height - 0.5)).clamp(0., 1.);
    let mut chord = 0.;
    let mut deposition = 0.;
    for i in 0..16384 {
        let x = (i as f64 + 0.5 - 8192.) / 8192. * f64::from(brush.diameter);
        let q = contact_q([x, y as f64-128.], brush);
        if q > 1. { continue; }
        chord += 1.;
        let relief_q = if dry.relief_offset == 0. { q } else {
            relief_q([x, y as f64-128.], brush)
        };
        let recess = profile[0] + profile[1]*relief_q + profile[2]*relief_q*relief_q;
        let x = ((0.5*contact_pressure - recess - (1. + 0.25*recess)*d) / f64::from(dry.softness)).clamp(0., 1.);
        deposition += x*x*(3.-2.*x) * loading;
    }
    if chord == 0. { return 0.; }
    deposition/chord
}

fn reference_alpha(y: usize, pressure: f64, brush: &BrushSnapshot) -> u8 {
    (255.*(1.-(-6.*f64::from(brush.opacity*brush.color_rgba_linear[3])
        *reference_exposure(y,pressure,brush)).exp())).round() as u8
}

fn grip_dose(dose:f64,delta:f64,full:f64,pressure:f64)->f64 {
    let linear=(full-dose).max(0.);
    let phi=0.12+0.88*pressure.clamp(0.,1.).powi(3);
    let first=delta.min(linear);
    dose+first+(delta-first)*phi
}

#[test]
fn rotated_block_taps_keep_every_supported_corner_and_no_outside_pixels() {
    for angle in [0., 0.7] {
        let mut brush = analytic(0.35, 0.);
        brush.angle_radians = angle;
        for pose in &mut brush.contact.as_mut().unwrap().dry.as_mut().unwrap().poses {
            pose.radii = [1.2, 0.6];
            pose.relief = [0.; 3];
        }
        let image = paint(brush.clone(), Run { samples: 2, ..Default::default() },
            |_| ([128.5; 2], 1., [0.; 2]));
        for y in 64..193 {
            for x in 64..193 {
                let q = contact_q([x as f64-128., y as f64-128.], &brush);
                let alpha = image.rgba[(y*256+x)*4+3];
                if q < 0.99 { assert!(alpha > 0, "angle={angle} supported tap corner ({x},{y}) is clipped"); }
                if q > 1.01 { assert_eq!(alpha, 0, "angle={angle} tap outside support ({x},{y}) paints"); }
            }
        }
    }
}

#[test]
fn short_sweeps_preserve_sharp_relief_transfer() {
    let mut brush=analytic(0.,1.4);
    let dry=brush.contact.as_mut().unwrap().dry.as_mut().unwrap();
    dry.softness=0.005;
    dry.relief_offset=0.35;
    for pose in &mut dry.poses {pose.relief[2]=4.;}
    let image=line(brush.clone(),1.,0.5,Run::default());
    for y in 84..173 {
        let actual=image.rgba[(y*256+128)*4+3];
        let expected=reference_alpha(y,1.,&brush);
        assert!(actual.abs_diff(expected)<=2,"y={y}: GPU alpha {actual}, quadrature {expected}");
    }
}

#[test]
fn swept_contact_matches_independent_quadrature_for_ellipse_and_rounded_block() {
    for corner in [0., 0.35] {
        for offset in if corner == 0. { &[0., 0.35][..] } else { &[0.][..] } {
            for paper in [0., 1.4] {
                for (pressure, softness, c4) in [0.1, 0.5, 1.].into_iter().flat_map(|p| [(p, 0.07, 0.3), (p, 0.005, 4.)]) {
                    for angle in [0., 0.7] {
                        let mut brush = analytic(corner, paper);
                        let dry = brush.contact.as_mut().unwrap().dry.as_mut().unwrap();
                        dry.softness = softness;
                        dry.relief_offset = *offset;
                        for pose in &mut dry.poses { pose.relief[2] = c4; }
                        brush.angle_radians = angle;
                        if angle > 0. {
                            for pose in &mut brush.contact.as_mut().unwrap().dry.as_mut().unwrap().poses { pose.radii = [1.2, 0.6]; }
                        }
                        let image = line(brush.clone(), pressure, if *offset==0. {0.} else {0.5}, Run::default());
                        for y in 84..173 {
                            let actual = image.rgba[(y * 256 + 128)*4 + 3];
                            let expected = reference_alpha(y, pressure as f64, &brush);
                            assert!(actual.abs_diff(expected) <= 2,
                                "corner={corner} offset={offset} angle={angle} paper={paper} pressure={pressure} c4={c4} softness={softness} y={y}: GPU alpha {actual}, quadrature {expected}");
                            if expected == 0 { assert_eq!(actual, 0, "empty contacts must stay exact at y={y}"); }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn pointy_tip_contrast_tracks_stylus_pose_when_motion_reverses() {
    let mut brush = default_brush(DefaultBrushPreset::PointyPencil);
    brush.diameter = 24.;
    assert_eq!((brush.flow,brush.contact.unwrap().pressure_gain),(0.7,1.));
    brush.mappings = Arc::from([]);
    brush.grain = None;
    brush.contact.as_mut().unwrap().paper = 0.;
    for (tilt, vertical, distance) in [
        ([0., 0.], true, 3),
        ([40f32.to_radians(), 0.], true, 8),
        ([60f32.to_radians(), 0.], true, 43),
        ([0., 60f32.to_radians()], false, 43),
    ] {
        let mut tip_and_back = Vec::new();
        for reverse in [false, true] {
            let image = paint(brush.clone(), Run::default(), |t| {
                let travel = 48.5 + 160. * if reverse { 1.-t } else { t };
                let position = if vertical { [128.5, travel] } else { [travel, 128.5] };
                (position, 0.65, tilt)
            });
            let (tip, back) = if vertical {
                ((128-distance, 128), (128+distance, 128))
            } else {
                ((128, 128-distance), (128, 128+distance))
            };
            let alpha = |(x, y): (usize, usize)| image.rgba[(y*256+x)*4+3];
            let result = (alpha(tip), alpha(back));
            if tilt==[0.;2] {
                assert!(result.0.abs_diff(result.1)<=2,"upright contact has no directional offset: {result:?}");
            } else {
                assert!(result.0 > result.1.saturating_add(12),
                    "tilt={tilt:?} vertical={vertical} reverse={reverse}: tip/back alpha={result:?}");
            }
            if tilt!=[0.;2] && !reverse {
                let mut previous=brush.clone();previous.contact.as_mut().unwrap().dry.as_mut().unwrap().relief_offset=0.35;
                let old=paint(previous,Run::default(),|t| {
                    let travel=48.5+160.*t;let position=if vertical {[128.5,travel]}else{[travel,128.5]};
                    (position,0.65,tilt)
                });
                let old_alpha=|(x,y):(usize,usize)|old.rgba[(y*256+x)*4+3];
                let old_contrast=i16::from(old_alpha(tip))-i16::from(old_alpha(back));
                let contrast=i16::from(result.0)-i16::from(result.1);
                assert!(contrast>old_contrast,"stronger Pointy offset must improve tip/heel contrast: tilt={tilt:?} old={old_contrast} new={contrast}");
                println!("Pointy tilt={tilt:?}: old contrast={old_contrast}, strengthened contrast={contrast}");
            }
            tip_and_back.push(result);
        }
        assert!(tip_and_back[0].0.abs_diff(tip_and_back[1].0) <= 5,
            "reversing movement changed tip darkness: tilt={tilt:?} {tip_and_back:?}");
        assert!(tip_and_back[0].1.abs_diff(tip_and_back[1].1) <= 5,
            "reversing movement changed back darkness: tilt={tilt:?} {tip_and_back:?}");
    }
    brush.angle_radians = 0.7;
    let (sin, cos) = brush.angle_radians.sin_cos();
    let rotated = paint(brush, Run::default(), |t| {
        let travel = 160.*t-80.;
        ([128.5-travel*sin, 128.5+travel*cos], 0.65, [40f32.to_radians(),0.])
    });
    let alpha = |x: usize, y: usize| rotated.rgba[(y*256+x)*4+3];
    let (dx, dy) = ((8.*cos).round() as usize, (8.*sin).round() as usize);
    let tip = alpha(128-dx, 128-dy);
    let back = alpha(128+dx, 128+dy);
    assert!(tip > back.saturating_add(12),
        "brush rotation must rotate tip darkness: tip={tip} back={back}");
}

#[test]
fn native_masks_evaluate_the_same_stationary_paper_and_swept_relief() {
    for corner in [0., 0.35] {
        let mut brush = analytic(corner, 1.4);
        brush.angle_radians = 0.7;
        brush.opacity = 0.6;
        for pose in &mut brush.contact.as_mut().unwrap().dry.as_mut().unwrap().poses { pose.radii = [1.2, 0.6]; }
        for samples in [1, 64] {
            let image = line(brush.clone(), 0.5, 0., Run { samples, mask: true, ..Default::default() });
            for y in 84..173 {
                let actual = image.rgba[(y*256+128)*4+3];
                let expected = reference_alpha(y, 0.5, &brush);
                assert!(actual.abs_diff(expected) <= 2,
                    "native mask corner={corner} samples={samples} y={y}: alpha {actual}, quadrature {expected}");
                if expected == 0 { assert_eq!(actual, 0, "native mask empty paper contact y={y}"); }
            }
        }
    }
}

#[test]
fn dry_exposure_preserves_path_partition_cadence_and_masks() {
    for (corner,diameter,pressure,angle,paper,check_mask) in [(0.,80.,0.55,0.,1.4,false),
        (0.35,80.,0.55,0.,1.4,false),(0.,2.,0.5,0.,0.,true),
        (0.,4.,0.25,0.7,1.4,true),(0.,6.,0.65,0.4,1.4,true)] {
        let mut brush=analytic(corner,paper);brush.diameter=diameter;brush.angle_radians=angle;
        for &mask in if check_mask {&[false,true][..]} else {&[false][..]} {
            let reference=line(brush.clone(),pressure,0.,Run {samples:2,cadence:1,mask,..Default::default()});
            for (samples,cadence) in [(64,1),(64,17),(128,9)] {
                let actual=line(brush.clone(),pressure,0.,Run {samples,cadence,mask,..Default::default()});
                actual.assert_near(&reference,2,&format!("corner={corner} diameter={diameter} pressure={pressure} angle={angle} paper={paper} mask={mask} samples={samples} cadence={cadence}"));
            }
        }
    }
}

#[test]
fn every_dry_medium_preserves_committed_pressure_and_tilt_with_prediction_and_zoom() {
    for preset in PRESETS {
        let mut brush = default_brush(preset);
        brush.stabilization.strength = 0.;
        brush.diameter = 90.;
        let path = |t: f32| ([32.5 + 192.*t, 128.5 + 20.*(t*std::f32::consts::TAU).sin()],
            0.15 + 0.8*(t*std::f32::consts::PI).sin(),
            [1.3*t, 0.2*t]);
        let real = paint(brush.clone(), Run { cadence: 1, ..Default::default() }, path);
        assert!(real.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0), "{preset:?} paints");
        for scale in [1., 0.1] {
            let predicted = paint(brush.clone(), Run { cadence: 8, feedback: true, scale, ..Default::default() }, path);
            predicted.assert_near(&real, 5, &format!("{preset:?} prediction scale={scale}"));
        }
    }
}

#[test]
fn pressure_recruits_contacts_without_forcing_the_rim_or_zero_force_to_paint() {
    for preset in PRESETS {
        let mut brush = default_brush(preset);
        brush.diameter = 120.;
        brush.mappings = Arc::from([]);
        let mut previous = vec![0; (EXTENT[0]*EXTENT[1]*4) as usize];
        for pressure in [0., 0.15, 0.45, 1.] {
            let image = line(brush.clone(), pressure, 0.85, Run::default());
            let mut recruited = 0;
            for (before, after) in previous.as_chunks::<4>().0.iter().zip(image.rgba.as_chunks::<4>().0.iter()) {
                assert!(after[3] >= before[3], "{preset:?} pressure={pressure} loses contact: {} -> {}", before[3], after[3]);
                recruited += usize::from(before[3] == 0 && after[3] > 0);
            }
            if pressure == 0. { assert!(image.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0), "{preset:?}: zero pressure paints"); }
            if pressure == 1. { assert!(recruited > 0, "{preset:?}: pressure must recruit empty contacts"); }
            previous = image.rgba;
        }
    }
    for (corner,offset) in [(0.,0.),(0.35,0.),(0.,0.35)] {
        let mut brush=analytic(corner,1.4);brush.flow=0.4;
        brush.contact.as_mut().unwrap().pressure_gain=0.85;
        let dry=brush.contact.as_mut().unwrap().dry.as_mut().unwrap();dry.relief_offset=offset;
        for pose in &mut dry.poses {pose.radii=[1.,0.65];}
        let reduced=line(brush.clone(),0.8,0.75,Run::default());
        let mut reference=brush.clone();reference.flow=1.;reference.contact.as_mut().unwrap().pressure_gain=1.;
        let low=line(reference.clone(),0.272,0.75,Run::default());
        reduced.assert_near(&low,2,&format!("Strength scales clamped material pressure only: corner={corner} offset={offset}"));
        let full=line(reference.clone(),1.,0.75,Run::default());
        let lost=full.rgba.as_chunks::<4>().0.iter().zip(reduced.rgba.as_chunks::<4>().0.iter()).filter(|(a,b)|a[3]>0 && b[3]==0).count();
        assert!(lost>0,"corner={corner} offset={offset}: reduced Strength removes eligible contacts, not just their pigment dose");
        println!("Strength pressure control: corner={corner} offset={offset} lost_contacts={lost} material_pressure=0.272");
        for y in 88..169 {
            let expected=reference_alpha(y,0.8,&brush);let actual=reduced.rgba[(y*256+128)*4+3];
            assert!(actual.abs_diff(expected)<=2,"Strength pressure quadrature corner={corner} offset={offset} y={y}: GPU {actual}, expected {expected}");
        }
        brush.flow=0.;let empty=line(brush,0.8,0.75,Run::default());
        assert!(empty.rgba.as_chunks::<4>().0.iter().all(|p|p[3]==0),"corner={corner} offset={offset}: Strength zero");
    }
    for hardness in [0.,0.5,1.] {
        let mut brush=analytic(0.,1.4);brush.flow=0.3;
        brush.contact.as_mut().unwrap().pressure_gain=0.85;
        brush.contact.as_mut().unwrap().dry.as_mut().unwrap().loading=hardness;
        let low=line(brush.clone(),0.8,0.,Run::default());
        for y in 88..169 {
            let actual=low.rgba[(y*256+128)*4+3];let expected=reference_alpha(y,0.8,&brush);
            assert!(actual.abs_diff(expected)<=2,"hardness={hardness} y={y}: GPU{actual} independent{expected}");
        }
        brush.flow=0.8;let high=line(brush.clone(),0.8,0.,Run::default());
        let mut recruited=0;let mut empty=0;
        for (a,b) in low.rgba.as_chunks::<4>().0.iter().zip(high.rgba.as_chunks::<4>().0.iter()) {
            assert!(b[3]>=a[3],"hardness={hardness}: material pressure loses contact");
            recruited+=usize::from(a[3]==0 && b[3]>0);
            empty+=usize::from(b[3]==0);
        }
        assert!(recruited>0 && empty>0,"hardness={hardness}: pressure recruits contacts while recessed paper stays empty");
    }
    for (preset,prior_gain) in [(DefaultBrushPreset::Pencil,0.7),(DefaultBrushPreset::PointyPencil,0.7),
        (DefaultBrushPreset::ShadingPencil,0.63),(DefaultBrushPreset::Charcoal,0.85),
        (DefaultBrushPreset::Chalk,0.75),(DefaultBrushPreset::PastelBlock,0.85),(DefaultBrushPreset::Crayon,0.7)] {
        let current=default_brush(preset);
        let mut prior=current.clone();prior.flow=1.;prior.contact.as_mut().unwrap().pressure_gain=prior_gain;
        for pressure in [0.25,0.7,1.] {for tilt in [0.,0.5,0.85] {
            let actual=line(current.clone(),pressure,tilt,Run::default());
            let expected=line(prior.clone(),pressure,tilt,Run::default());
            actual.assert_near(&expected,2,&format!("{preset:?}: default Strength matches prior gain pressure={pressure} tilt={tilt}"));
        }}
    }
    let image = line(analytic(0., 0.), 1., 0., Run::default());
    assert!(image.rgba[(128*256+128)*4+3] > 0);
    for y in [91, 92, 164, 165] {
        assert_eq!(image.rgba[(y*256+128)*4+3], 0, "full pressure leaves recessed interior rim y={y} empty");
    }
}

#[test]
fn paper_contacts_stay_at_document_coordinates_when_a_stroke_starts_elsewhere() {
    for corner in [0., 0.35] {
        let brush = analytic(corner, 1.4);
        let first = line(brush.clone(), 0.55, 0., Run::default());
        let second = paint(brush, Run::default(), |t| ([8.5 + 240.*t, 128.5], 0.55, [0.; 2]));
        first.crop([110, 88], [35, 81]).assert_near(&second.crop([110, 88], [35, 81]), 2,
            &format!("stationary paper corner={corner}"));
    }
}

#[test]
fn paper_stays_on_the_drawing_when_the_layer_moves() {
    let brush = analytic(0., 1.4);
    let still = line(brush.clone(), 0.55, 0., Run::default());
    let moved = line(brush, 0.55, 0., Run { offset: [5, 3], ..Default::default() });
    still.crop([40, 88], [160, 81]).assert_near(&moved.crop([40, 88], [160, 81]), 2, "paper under a moved layer");
}

#[test]
fn dry_pose_keys_have_continuous_visible_contact() {
    for preset in PRESETS {
        let mut brush = default_brush(preset);
        brush.diameter = 90.;
        for key in [40. / 90., 60. / 90.] {
            let before = line(brush.clone(), 0.7, key-0.0001, Run::default());
            let after = line(brush.clone(), 0.7, key+0.0001, Run::default());
            after.assert_near(&before, 1, &format!("{preset:?} tilt key {key}"));
        }
    }
}

fn strip_pixel_area(pixel: [usize; 2], center: [f64; 2], normal: [f64; 2], half: f64) -> f64 {
    let [x, y] = pixel.map(|v| v as f64);
    let mut polygon = vec![[x, y], [x+1., y], [x+1., y+1.], [x, y+1.]];
    for side in [-1., 1.] {
        let plane = |p: [f64; 2]| side*((p[0]-center[0])*normal[0]+(p[1]-center[1])*normal[1])-half;
        let mut clipped = Vec::new();
        if polygon.is_empty() { return 0.; }
        for i in 0..polygon.len() {
            let a = polygon[i];
            let b = polygon[(i+1)%polygon.len()];
            let (fa, fb) = (plane(a), plane(b));
            if fa <= 0. { clipped.push(a); }
            if (fa <= 0.) != (fb <= 0.) {
                let t = fa/(fa-fb);
                clipped.push([a[0]+t*(b[0]-a[0]), a[1]+t*(b[1]-a[1])]);
            }
        }
        polygon = clipped;
    }
    (0..polygon.len()).map(|i| {
        let a = polygon[i]; let b = polygon[(i+1)%polygon.len()];
        a[0]*b[1]-a[1]*b[0]
    }).sum::<f64>().abs()*0.5
}

#[test]
fn fine_contact_alpha_matches_independent_pixel_area_for_rotated_sweeps() {
    for (diameter, angle, direction, phase, flow, offset) in [
        (2., 0., 0f64, 0., 0.3, 0.), (2., 0., 0., 0.5, 1., 0.35),
        (2., 0.7, 0.4, 0.25, 0.3, 0.35), (2., 0.7, 0.9, 0.5, 1., 0.),
        (4., 0.7, 0., 0.375, 1., 0.), (4., 0., 0.7, 0.625, 0.3, 0.35),
    ] {
        let mut brush = analytic(0., 0.);
        brush.diameter = diameter;
        brush.flow = flow;
        brush.angle_radians = angle;
        brush.contact.as_mut().unwrap().dry.as_mut().unwrap().relief_offset = offset;
        for pose in &mut brush.contact.as_mut().unwrap().dry.as_mut().unwrap().poses {
            pose.radii = [1., 0.5]; pose.relief = [0.; 3];
        }
        let (s, c) = direction.sin_cos();
        let normal = [-s, c];
        let center = [128.5+phase*normal[0], 128.5+phase*normal[1]];
        let image = paint(brush.clone(), Run::default(), |t| {
            let distance = 192.*f64::from(t)-96.;
            ([(center[0]+distance*c) as f32, (center[1]+distance*s) as f32], 0.5, [0.; 2])
        });
        let (s, c) = f64::from(angle).sin_cos();
        let radius = f64::from(diameter)*0.5;
        let half = radius*((normal[0]*c+normal[1]*s).powi(2)
            +0.25*(-normal[0]*s+normal[1]*c).powi(2)).sqrt();
        let (_,loading) = material_response(material_pressure(0.5, &brush),&brush);
        let alpha = 1.-(-6.*loading).exp();
        for y in 120..137 { for x in 120..137 {
            let expected = (255.*alpha*strip_pixel_area([x, y], center, normal, half)).round() as u8;
            let actual = image.rgba[(y*256+x)*4+3];
            assert!(actual.abs_diff(expected) <= 2,
                "diameter={diameter} angle={angle} direction={direction} phase={phase} flow={flow} offset={offset} ({x},{y}): GPU {actual}, independently clipped pixel area {expected}");
            if expected == 0 { assert_eq!(actual, 0, "outside finite contact at ({x},{y})"); }
        }}
    }
}

#[test]
fn fine_stationary_contact_matches_independent_two_dimensional_pixel_integration() {
    for (diameter, angle, center, relief, pressure, paper, offset) in [
        (1.,0.,[128.5,128.5],[0.;3],0.5,0.,0.),
        (2.,0.7,[128.75,128.625],[0.,0.75,0.3],0.5,0.,0.35),
        (4.,0.4,[128.25,128.875],[0.,0.75,0.3],0.15,1.4,0.35),
    ] {
        let mut brush=analytic(0.,paper);brush.diameter=diameter;brush.angle_radians=angle;
        brush.contact.as_mut().unwrap().dry.as_mut().unwrap().relief_offset=offset;
        for pose in &mut brush.contact.as_mut().unwrap().dry.as_mut().unwrap().poses {pose.relief=relief;}
        let image=paint(brush.clone(),Run {samples:2,..Default::default()},|_|(center,pressure,if offset==0. {[0.;2]}else{[std::f32::consts::FRAC_PI_4,0.]}));
        let material=brush.contact.unwrap();let dry=material.dry.unwrap();
        for y in 124..133 {for x in 124..133 {
            let height=if y%16<8 {1.} else {0.};
            let depth=0.5-(0.5+f64::from(paper)*(height-0.5)).clamp(0.,1.);
            let p=material_pressure(f64::from(pressure), &brush);
            let dose=6.*f64::from(brush.opacity*brush.color_rgba_linear[3]);
            let (contact_pressure,loading)=material_response(p,&brush);
            let mut alpha=0.;
            for sy in 0..256 {for sx in 0..256 {
                let q=contact_q([x as f64+(sx as f64+0.5)/256.-f64::from(center[0]),
                    y as f64+(sy as f64+0.5)/256.-f64::from(center[1])],&brush);
                if q>1. {continue;}
                let relief_q = if offset == 0. { q } else { relief_q([
                    x as f64+(sx as f64+0.5)/256.-f64::from(center[0]),
                    y as f64+(sy as f64+0.5)/256.-f64::from(center[1])], &brush) };
                let recess=f64::from(relief[0])+relief_q*(f64::from(relief[1])+f64::from(relief[2])*relief_q);
                let t=((0.5*contact_pressure-recess-(1.+0.25*recess)*depth)/f64::from(dry.softness)).clamp(0.,1.);
                alpha+=1.-(-dose*loading*t*t*(3.-2.*t)).exp();
            }}
            let expected=(255.*alpha/65536.).round() as u8;
            let actual=image.rgba[(y*256+x)*4+3];
            assert!(actual.abs_diff(expected)<=2,
                "stationary diameter={diameter} angle={angle} offset={offset} p={pressure} paper={paper} ({x},{y}): GPU {actual}, independent2D {expected}");
        }}
    }
}

fn contact_stages(brush: BrushSnapshot, feedback: bool,
    samples: &[([f32; 2], f32)]) -> Vec<support::Image> {
    let (mut engine, mut input) = contact_engine(brush, Run { feedback, ..Default::default() });
    samples.iter().enumerate().map(|(i, &(position, pressure))| {
        let time = 1_000_000_000 + i as u64 * 16_000_000;
        let mut event = support::pen_sample(i as u64, samples.len() as u64 - 1, time,
            Point { x: position[0], y: position[1] });
        event.sequence = time;
        event.pressure = pressure;
        event.tilt_radians = [std::f32::consts::FRAC_PI_4, 0.];
        input.push(event).unwrap();
        support::drain_input(&mut engine, time, true);
        support::image(&mut engine, time)
    }).collect()
}

#[test]
fn dry_taps_leave_one_peak_pressure_imprint_only_on_release() {
    for preset in PRESETS {
        let mut brush = default_brush(preset);
        brush.diameter = 80.;
        brush.mappings = Arc::from([]);
        brush.stabilization.strength = 0.;
        brush.color_rgba_linear = [0., 0., 0., 0.1];
        let center = [128.5; 2];
        for strength in [0.2, 1.] {
            brush.flow = strength;
            let reference = contact_stages(brush.clone(), false,
                &[(center, 0.6), (center, 0.6), (center, 0.6)]);
            assert!(reference.last().unwrap().rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
                "{preset:?} Strength={strength}: release leaves an imprint");
            for feedback in [false, true] {
                let jitter = contact_stages(brush.clone(), feedback,
                    &[(center, 0.6), (center, 0.2), ([128.55, 128.5], 0.2),
                        ([128.45, 128.5], 0.2), (center, 0.2)]);
                for image in &jitter[..2] {
                    assert!(image.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0),
                        "{preset:?} Strength={strength} feedback={feedback}: held pen paints before release");
                }
                jitter.last().unwrap().assert_near(reference.last().unwrap(), 1,
                    &format!("{preset:?} Strength={strength} feedback={feedback}: jitter tap retains one peak-pressure imprint"));
            }
        }
    }
}

#[test]
fn dry_drag_caps_complete_the_body_exposure() {
    let mut brush = analytic(0., 0.);
    brush.stabilization.strength = 0.;
    brush.color_rgba_linear[3] = 0.1;
    for pose in &mut brush.contact.as_mut().unwrap().dry.as_mut().unwrap().poses {
        pose.relief = [0.; 3];
    }
    let image = line(brush.clone(), 0.55, 0., Run::default());
    let alpha = |x: usize, y: usize| image.rgba[(y*256+x)*4+3];
    assert!(alpha(128, 128).abs_diff(alpha(32, 128)) <= 2,
        "completed caps match the body: body={} cap={}",alpha(128,128),alpha(32,128));
    for y in 100..157 {
        assert!(alpha(32, y).abs_diff(alpha(224, y)) <= 2,
            "start and end caps must match at y={y}: {} vs {}", alpha(32, y), alpha(224, y));
    }
    let predicted = line(brush, 0.55, 0., Run { feedback: true, cadence: 1, ..Default::default() });
    predicted.assert_near(&image, 2, "prediction preserves committed dry drag caps");
}

fn polyline(brush: BrushSnapshot, vertices: &[[f32;2]], subdivisions: u64, cadence: u64) -> support::Image {
    let count=vertices.len()-1;
    paint(brush,Run {samples:count as u64*subdivisions,cadence,..Default::default()},|t| {
        let position=t*count as f32;
        let i=(position as usize).min(count-1);
        let fraction=(position-i as f32).clamp(0.,1.);
        ([vertices[i][0]+fraction*(vertices[i+1][0]-vertices[i][0]),
            vertices[i][1]+fraction*(vertices[i+1][1]-vertices[i][1])],1.,[0.;2])
    })
}

#[test]
fn sampled_dry_hairpins_and_squiggles_preserve_nonflat_pressure_tilt_and_opacity() {
    for (preset,diameter) in [(DefaultBrushPreset::Pencil,4.),(DefaultBrushPreset::Pencil,36.),
        (DefaultBrushPreset::PastelBlock,36.)] {
        let mut brush=default_brush(preset);
        brush.diameter=diameter;brush.opacity=0.35;brush.color_rgba_linear=[0.,0.,0.,0.6];
        brush.stabilization.strength=0.;brush.mappings=Arc::from([]);
        for hairpin in [true,false] {
            let pose=|t:f32| {
                let angle=t*std::f32::consts::PI;
                let position=if hairpin {[128.5+56.*angle.sin(),128.5+56.*angle.cos()]}
                    else {[32.5+192.*t,128.5+40.*(angle*4.).sin()]};
                (position,0.3+0.6*angle.sin(),[0.6*angle.sin(),0.35*angle.cos()])
            };
            let reference=paint(brush.clone(),Run {samples:128,cadence:1,..Default::default()},pose);
            assert!(reference.rgba.as_chunks::<4>().0.iter().filter(|p|p[3]>0).count()>256,
                "{preset:?} diameter={diameter} hairpin={hairpin}: nonflat curve must leave visible paint");
            for feedback in [false,true] {
                let batched=paint(brush.clone(),Run {samples:128,cadence:7,feedback,..Default::default()},pose);
                batched.assert_near(&reference,2,&format!("{preset:?} diameter={diameter} hairpin={hairpin} feedback={feedback}: curved contact cadence"));
            }
        }
    }
}

#[test]
fn tiny_completed_caps_match_independent_pixel_area() {
    let mut failures=Vec::new();
    for (opacity,direction) in [(0.35,0_f32),(1.,0.),(0.35,0.7),(1.,0.7)] {
    let (sin,cos)=direction.sin_cos();
    for diameter in [1.,2.,4.] {
        let mut brush=analytic(0.,0.);
        brush.diameter=diameter;brush.flow=1.;brush.opacity=opacity;brush.color_rgba_linear[3]=0.6;
        brush.stabilization.strength=0.;
        for pose in &mut brush.contact.as_mut().unwrap().dry.as_mut().unwrap().poses {pose.relief=[0.;3];}
        let image=paint(brush.clone(),Run {samples:32,cadence:4,..Default::default()},
            |t|([32.75+192.*t*cos,128.625+192.*t*sin],1.,[0.;2]));
        let radius=f64::from(diameter)*0.5;
        let full=f64::from(brush.opacity)*(1.-(-6.*f64::from(brush.color_rgba_linear[3])).exp());
        for y in 125..132 {for x in 29..36 {
            let mut covered=0u32;
            for sy in 0..256 {for sx in 0..256 {
                let px=x as f64+(sx as f64+0.5)/256.;
                let py=y as f64+(sy as f64+0.5)/256.;
                let delta=[px-32.75,py-128.625];
                let along=(delta[0]*f64::from(cos)+delta[1]*f64::from(sin)).max(0.);
                let dx=delta[0]-along*f64::from(cos);let dy=delta[1]-along*f64::from(sin);
                covered+=u32::from(dx*dx+dy*dy<radius*radius);
            }}
            let expected=(255.*full*f64::from(covered)/65536.).round() as u8;
            let actual=image.rgba[(y*256+x)*4+3];
            if actual.abs_diff(expected)>3 {failures.push(format!(
                "diameter={diameter} opacity={opacity} direction={direction} cap pixel=({x},{y}): GPU{actual} independent area{expected}"));}
        }}
    }
    }
    assert!(failures.is_empty(),"{}",failures.join("\n"));
}

#[test]
fn nonflat_pencil_and_charcoal_caps_match_independent_chord_integration() {
    for preset in [DefaultBrushPreset::Pencil,DefaultBrushPreset::Charcoal] {
        let mut brush=default_brush(preset);
        brush.diameter=80.;brush.mappings=Arc::from([]);brush.stabilization.strength=0.;
        brush.color_rgba_linear=[0.,0.,0.,0.3];brush.contact.as_mut().unwrap().paper=0.;
        let dry=brush.contact.unwrap().dry.unwrap();
        for pressure in [0.4,0.8] {
            let image=line(brush.clone(),pressure,0.,Run::default());
            let p=material_pressure(f64::from(pressure),&brush);
            for y in 106..151 {
                let q=relief_q([0.,y as f64-128.],&brush);
                let profile=dry.poses[0].relief.map(f64::from);
                let recess=profile[0]+q*(profile[1]+q*profile[2]);
                let t=((0.5*p-recess)/f64::from(dry.softness)).clamp(0.,1.);
                let window=t*t*(3.-2.*t);
                let exposure=reference_exposure(y,f64::from(pressure),&brush)*0.5*(1.+window);
                let expected=(255.*f64::from(brush.opacity)*(1.-(-6.*f64::from(brush.color_rgba_linear[3])*exposure).exp())).round() as u8;
                for x in [32,224] {
                    let actual=image.rgba[(y*256+x)*4+3];
                    assert!(actual.abs_diff(expected)<=2,
                        "{preset:?} pressure={pressure} cap=({x},{y}): GPU{actual} independent chord{expected}");
                }
            }
        }
    }
}

#[test]
fn nonflat_turn_completion_matches_independent_incident_chords() {
    let mut brush=analytic(0.,0.);
    brush.flow=1.;brush.color_rgba_linear[3]=0.2;brush.stabilization.strength=0.;
    let dry=brush.contact.unwrap().dry.unwrap();
    let transfer=|q:f64| {
        let profile=dry.poses[0].relief.map(f64::from);
        let t=((0.5-profile[0]-q*(profile[1]+q*profile[2]))/f64::from(dry.softness)).clamp(0.,1.);
        t*t*(3.-2.*t)
    };
    let incident=|transverse:f64,limit:f64,positive:bool| {
        let half=(1600.-transverse*transverse).sqrt();
        let mut full=0.;let mut passed=0.;
        for i in 0..8192 {
            let along=half*((i as f64+0.5)/4096.-1.);
            let contact=transfer((transverse*transverse+along*along)/1600.);
            full+=contact;
            if (positive && along>=limit) || (!positive && along<=limit) {passed+=contact;}
        }
        [passed/8192.,full/8192.]
    };
    let image=polyline(brush,&[[32.5,128.5],[128.5,128.5],[128.5,224.5]],1,1);
    for dy in [-27,-18,-9,0,9,18,27] {for dx in [-27,-18,-9,0,9,18,27] {
        let a=incident(f64::from(dy),f64::from(dx),true);
        let b=incident(f64::from(dx),f64::from(dy),false);
        let window=transfer(f64::from(dx*dx+dy*dy)/1600.);
        let completion=window*(a[1].max(b[1])-a[0]-b[0]).max(0.);
        let mut dose=grip_dose(0.,1.2*a[0],1.2*a[1],1.);
        dose=grip_dose(dose,1.2*b[0],1.2*b[1],1.);
        dose=grip_dose(dose,1.2*completion,1.2*a[1].max(b[1]).max(a[0]+b[0]+completion),1.);
        let expected=(255.*(1.-(-dose).exp())).round() as u8;
        let x=(128+dx) as usize;let y=(128+dy) as usize;
        let actual=image.rgba[(y*256+x)*4+3];
        assert!(actual.abs_diff(expected)<=2,
            "nonflat turn pixel=({x},{y}): GPU{actual} independent incident chords{expected}");
    }}
}

#[test]
fn broad_completion_preserves_independent_chord_exposure_across_tiles() {
    let size=[1024;2];
    let mut document=Document::new(PortableId::random(),size[0],size[1],
        DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let paper=support::named_occurrence(&document,"Paper");
    document.artwork.occurrences.get_mut(paper).unwrap().visible=false;
    let (mut engine,mut input)=support::engine(document);
    engine.set_view(ViewState {width_px:size[0],height_px:size[1],
        document_to_surface:[1.,0.,0.,1.,0.,0.]},ViewTransform::IDENTITY);
    let mut brush=analytic(0.,0.);
    brush.diameter=640.;brush.flow=1.;brush.color_rgba_linear[3]=0.2;brush.stabilization.strength=0.;
    for pose in &mut brush.contact.as_mut().unwrap().dry.as_mut().unwrap().poses {pose.relief=[0.;3];}
    engine.set_brush(brush).unwrap();
    let vertices=[[64.5,512.5],[512.5,512.5],[512.5,960.5]];
    let mut time=1_000_000_000;
    for (i,position) in vertices.into_iter().enumerate() {
        time+=4_000_000;
        let mut event=support::pen_sample(i as u64,2,time,Point {x:position[0],y:position[1]});
        event.pressure=1.;event.tilt_radians=[0.;2];
        input.push(event).unwrap();support::drain_input(&mut engine,time,true);
    }
    let image=support::image(&mut engine,time);
    let radius=320_f64;
    let chord=|offset:f64,position:f64,lo:f64,hi:f64| {
        if offset.abs()>=radius {return 0.;}
        let half=(radius*radius-offset*offset).sqrt();
        ((position+half).min(hi)-(position-half).max(lo)).max(0.)/(2.*half)
    };
    let mut inner=0;let mut outer=0;
    for y in (128..897).step_by(32) {for x in (128..897).step_by(32) {
        let px=x as f64+0.5;let py=y as f64+0.5;
        let dx=px-512.5;let dy=py-512.5;
        let squared=dx*dx+dy*dy;
        if (squared-radius*radius).abs()<2.*radius {continue;}
        if (px-64.5).hypot(dy)<=radius+1. || dx.hypot(py-960.5)<=radius+1. {continue;}
        let a=chord(dy,px,64.5,512.5);let b=chord(dx,py,512.5,960.5);
        let mut dose=grip_dose(0.,1.2*a,1.2,1.);
        dose=grip_dose(dose,1.2*b,1.2,1.);
        if squared<radius*radius {
            if dx>dy {outer+=1;} else {inner+=1;}
            let completion=(1.-a-b).max(0.);
            dose=grip_dose(dose,1.2*completion,1.2*(a+b+completion).max(1.),1.);
        }
        let expected=(255.*(1.-(-dose).exp())).round() as u8;
        let actual=image.rgba[(y*1024+x)*4+3];
        assert!(actual.abs_diff(expected)<=2,
            "cross-tile cap pixel=({x},{y}): GPU{actual} independent chord{expected}");
    }}
    assert!(inner>10 && outer>10,"both retained outer and omitted inner tiles are sampled");
}

#[test]
fn dry_completion_fills_turns_and_preserves_subdivision_and_retracing() {
    for corner in [0.,0.35] {
        let mut brush=analytic(corner,0.);
        brush.stabilization.strength=0.;brush.flow=1.;brush.color_rgba_linear[3]=0.2;
        for pose in &mut brush.contact.as_mut().unwrap().dry.as_mut().unwrap().poses {pose.relief=[0.;3];}
        let straight=polyline(brush.clone(),&[[32.5,128.5],[224.5,128.5]],32,4);
        let body=straight.rgba[(128*256+128)*4+3];
        for (vertices,probes) in [
            (vec![[32.5,128.5],[160.5,128.5],[32.5,128.5]],vec![[176,128],[184,128]]),
            (vec![[32.5,128.5],[128.5,128.5],[128.5,224.5]],vec![[144,112],[148,108]]),
            (vec![[32.5,192.5],[96.5,64.5],[160.5,192.5],[224.5,64.5]],vec![[96,48],[160,208]]),
        ] {
            let reference=polyline(brush.clone(),&vertices,1,1);
            for [x,y] in probes {
                let actual=reference.rgba[(y*256+x)*4+3];
                assert!(actual.saturating_add(2)>=body,"corner={corner}, path={vertices:?}, point=({x},{y}), alpha={actual}, full passage={body}");
            }
            let refined=polyline(brush.clone(),&vertices,32,7);
            refined.assert_near(&reference,3,&format!("completion subdivision corner={corner}, path={vertices:?}"));
        }
        let reference=polyline(brush.clone(),&[[32.5,128.5],[160.5,128.5],[32.5,128.5]],1,1);
        let jitter=polyline(brush,&[[32.5,128.5],[160.6,128.5],[160.4,128.5],[160.55,128.5],[160.45,128.5],[160.5,128.5],[32.5,128.5]],1,1);
        jitter.crop([168,112],[24,32]).assert_near(&reference.crop([168,112],[24,32]),3,
            &format!("shrinking reversal completion corner={corner}"));
        assert!(reference.rgba[(128*256+96)*4+3]>body+10,"retracing keeps ordinary buildup");
    }
}
