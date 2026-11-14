use super::*;
use super::super::test_support::*;
use layer_engine::SampleFlags;
use std::sync::Arc;

fn held_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.frame(1, 1).unwrap();
    s.renderer_mut().painting_unready = true;
    s
}

fn settle(s: &mut UiSession<Recorder>) {
    for frame in 2..40 {
        s.frame(frame * 10_000_000, frame * 10_000_000).unwrap();
    }
}

fn predicted(s: &UiSession<Recorder>, sequence: u64) -> PenEvent {
    PenEvent { flags: SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::PREDICTED.0), ..event(s, sequence, PenPhase::Move, 0.5) }
}

#[test]
fn invalid_pen_samples_preserve_valid_siblings_and_finish_invalid_release() {
    for field in 0..7 {
        for (terminal,held) in [(false,false),(true,false),(false,true),(true,true)] {
            let mut s=if held {held_session()} else {session(Platform::Gtk)};
            s.frame(1,1).unwrap();
            let checkpoint=s.engine.checkpoint();
            s.pen(event(&s,1,PenPhase::Down,0.5));
            let mut bad=event(&s,2,if terminal {PenPhase::Up} else {PenPhase::Move},0.5);
            match field {
                0=>bad.surface_position.x=f32::NAN,1=>bad.surface_position.y=f32::INFINITY,
                2=>bad.pressure=f32::NAN,3=>bad.tilt_radians[0]=f32::NAN,
                4=>bad.tilt_radians[1]=f32::INFINITY,5=>bad.twist_radians=f32::NAN,
                _=>bad.distance=f32::NAN,
            }
            s.pen(bad);
            if !terminal {s.pen(event(&s,3,PenPhase::Up,0.5));}
            s.renderer_mut().painting_unready=false;
            settle(&mut s);
            assert!(!s.pen_contact && !s.engine.has_active_stroke() && !s.holding_input(),"field={field} terminal={terminal}");
            assert_ne!(s.engine.checkpoint(),checkpoint);
            assert_eq!(s.engine.metrics().committed_strokes,1);
            for (sequence,phase) in [(4,PenPhase::Down),(5,PenPhase::Move),(6,PenPhase::Up)] {s.pen(event(&s,sequence,phase,0.5));}
            settle(&mut s);
            assert_eq!(s.engine.metrics().committed_strokes,2);
            assert!(s.engine.can_undo());
            s.engine.undo().unwrap();
            assert!(s.engine.can_redo());
            s.engine.redo().unwrap();
            assert!(s.engine.capture_artwork(0).is_ok());
        }
    }
}

#[test]
fn a_completed_contact_waits_for_painting_before_the_edits_that_follow_it() {
    let mut s = held_session();
    let committed = s.engine.metrics().committed_strokes;
    let diameter = s.state.brush.diameter;
    for (sequence, phase) in [(1, PenPhase::Down), (2, PenPhase::Move), (3, PenPhase::Up)] {
        s.pen(event(&s, sequence, phase, 0.5));
    }
    s.pen(predicted(&s, 4));
    assert!(s.holding_input());
    assert_eq!(s.deferred_edits.len(), 3, "predictions of a held contact are dropped");
    s.dispatch(UiAction::SetBrushSize { value: diameter * 4. }).unwrap();
    assert_eq!(s.state.brush.diameter, diameter, "a later brush change waits behind the held contact");
    assert!(!s.command(CommandId::SaveDocument).enabled);
    assert!(!s.can_park_document());
    settle(&mut s);
    assert!(s.holding_input(), "the contact waits while painting is not ready");
    assert_eq!(s.engine.metrics().committed_strokes, committed);
    s.renderer_mut().painting_unready = false;
    settle(&mut s);
    assert!(!s.holding_input() && s.deferred_edits.is_empty());
    assert_eq!(s.engine.metrics().committed_strokes, committed + 1);
    assert_eq!(s.state.brush.diameter, diameter * 4.);
    let radius = s.renderer_mut().recorded_dabs.iter().map(|dab| dab.radii[0]).fold(0., f32::max);
    assert!(radius > 0. && radius <= diameter * 0.5 + 0.01, "the stroke used the brush of its contact: {radius}");
}

#[test]
fn a_contact_waiting_eight_seconds_after_undo_keeps_every_sample() {
    let mut s=session(Platform::Gtk);
    s.frame(1,1).unwrap();
    s.engine.set_instant_feedback(layer_engine::InstantFeedbackConfig {enabled:false,..Default::default()}).unwrap();
    for (sequence,phase) in [(1,PenPhase::Down),(2,PenPhase::Move),(3,PenPhase::Up)] {s.pen(event(&s,sequence,phase,0.5));}
    settle(&mut s);
    assert_eq!(s.engine.metrics().committed_strokes,1);
    s.dispatch(UiAction::Invoke {command:CommandId::Undo}).unwrap();
    settle(&mut s);
    let before=s.engine.metrics();
    let original=s.renderer_mut().recorded_dabs.len();
    s.renderer_mut().painting_unready=true;
    for (sequence,phase) in [(4,PenPhase::Down),(5,PenPhase::Move)] {s.pen(event(&s,sequence,phase,0.5));}
    let start=std::time::Instant::now();
    while start.elapsed()<std::time::Duration::from_secs(8) {
        let now=1_000_000_000+start.elapsed().as_nanos() as u64;
        s.frame(now,now).unwrap();
        assert!(s.holding_input(),"an unresolved selected destination must retain the whole contact");
        assert_eq!(s.engine.metrics().input_events,before.input_events);
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    s.pen(event(&s,6,PenPhase::Up,0.5));
    s.renderer_mut().painting_unready=false;
    settle(&mut s);
    assert!(!s.holding_input());
    assert_eq!(s.engine.metrics().input_events-before.input_events,3);
    assert_eq!(s.engine.metrics().committed_strokes-before.committed_strokes,1);
    let actual=s.renderer_mut().recorded_dabs[original..].to_vec();
    let mut replay=session(Platform::Gtk);
    replay.frame(1,1).unwrap();
    replay.engine.set_instant_feedback(layer_engine::InstantFeedbackConfig {enabled:false,..Default::default()}).unwrap();
    assert_eq!(replay.engine.allocate_stroke_id(),layer_core::StrokeId(1));
    for (sequence,phase) in [(4,PenPhase::Down),(5,PenPhase::Move),(6,PenPhase::Up)] {replay.pen(event(&replay,sequence,phase,0.5));}
    settle(&mut replay);
    assert!(!actual.is_empty());
    assert_eq!(actual,replay.renderer_mut().recorded_dabs,"every retained sample must produce the same stroke as immediate replay");
}

#[test]
fn a_held_contact_and_the_tool_chosen_after_it_keep_their_order() {
    let mut s = held_session();
    let committed = s.engine.metrics().committed_strokes;
    s.pen(event(&s, 1, PenPhase::Down, 0.5));
    s.pen(event(&s, 2, PenPhase::Move, 0.5));
    invoke(&mut s, CommandId::Eraser);
    assert_ne!(s.state.brush.tool, Tool::Eraser, "the tool change waits behind the held stroke");
    s.pen(event(&s, 3, PenPhase::Up, 0.5));
    s.renderer_mut().painting_unready = false;
    settle(&mut s);
    assert_eq!(s.engine.metrics().committed_strokes, committed + 1);
    assert_eq!(s.renderer_mut().last_style.as_ref().unwrap().mode, layer_render::DabMode::Paint);
    assert_eq!(s.state.brush.tool, Tool::Eraser);
}

#[test]
fn queued_color_selection_rejects_an_older_hdr_contact_and_its_rollback() {
    use layer_core::color::SampleDepth;
    let mut document = Document::new(layer_core::PortableId::random(), 1000, 1000, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = SampleDepth::F32;
    let mut s = UiSession::new(Recorder { color: document.composition().color, ..Default::default() }, document, [1000, 1000], Platform::Gtk).unwrap();
    for (slot, rgba, stops) in [(ColorSlot::Background, [0.6, 0.3, 0.1, 1.], -1.), (ColorSlot::Foreground, [0.2, 0.4, 0.6, 1.], 1.)] {
        s.dispatch(UiAction::Color { action: ColorAction::Select { slot } }).unwrap();
        s.dispatch(UiAction::SetColor { rgba }).unwrap();
        s.dispatch(UiAction::Color { action: ColorAction::HdrIntensity { stops } }).unwrap();
    }
    let paint = |colors: &ColorState, slot| {
        let mut colors = colors.clone();
        colors.apply(ColorAction::Select { slot }).unwrap();
        (colors.definition(), colors.hdr_intensity())
    };
    let before = [paint(&s.state.colors, ColorSlot::Foreground), paint(&s.state.colors, ColorSlot::Background)];
    let context = s.color_edit_context();
    s.frame(1, 1).unwrap();
    s.renderer_mut().painting_unready = true;
    for (sequence, phase) in [(1, PenPhase::Down), (2, PenPhase::Move), (3, PenPhase::Up)] {
        s.pen(event(&s, sequence, phase, 0.5));
    }
    s.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Background } }).unwrap();
    for stops in [3., before[0].1] {
        s.dispatch(UiAction::ColorEdit { context, action: ColorAction::HdrIntensity { stops } }).unwrap();
    }
    assert!(s.holding_input());
    assert_eq!(s.state.colors.slot, ColorSlot::Foreground);
    s.renderer_mut().painting_unready = false;
    settle(&mut s);
    assert!(!s.holding_input() && s.deferred_edits.is_empty());
    assert_eq!(s.state.colors.slot, ColorSlot::Background);
    assert_eq!([paint(&s.state.colors, ColorSlot::Foreground), paint(&s.state.colors, ColorSlot::Background)], before);
}

#[test]
fn queued_transparent_selection_rejects_an_older_hdr_contact_and_its_rollback() {
    use layer_core::color::SampleDepth;
    let mut document = Document::new(layer_core::PortableId::random(), 1000, 1000, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = SampleDepth::F32;
    let mut s = UiSession::new(Recorder { color: document.composition().color, ..Default::default() }, document, [1000, 1000], Platform::Gtk).unwrap();
    s.dispatch(UiAction::Color { action: ColorAction::HdrIntensity { stops: 1. } }).unwrap();
    let context = s.color_edit_context();
    let mut expected = s.state.colors.clone();
    expected.apply(ColorAction::Select { slot: ColorSlot::Transparent }).unwrap();
    s.frame(1, 1).unwrap();
    s.renderer_mut().painting_unready = true;
    for (sequence, phase) in [(1, PenPhase::Down), (2, PenPhase::Move), (3, PenPhase::Up)] {
        s.pen(event(&s, sequence, phase, 0.5));
    }
    s.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Transparent } }).unwrap();
    for stops in [3., 1.] {
        s.dispatch(UiAction::ColorEdit { context, action: ColorAction::HdrIntensity { stops } }).unwrap();
    }
    assert!(s.holding_input());
    assert_eq!(s.state.colors.slot, ColorSlot::Foreground);
    s.renderer_mut().painting_unready = false;
    settle(&mut s);
    assert!(!s.holding_input() && s.deferred_edits.is_empty());
    assert_eq!(s.state.colors.slot, ColorSlot::Transparent);
    assert_eq!(s.state.colors, expected);
}

#[test]
fn interrupted_held_ink_commits_while_oversized_contacts_are_discarded() {
    let mut s = held_session();
    let committed = s.engine.metrics().committed_strokes;
    s.pen(event(&s, 1, PenPhase::Down, 0.5));
    s.pen(event(&s, 2, PenPhase::Cancel, 0.5));
    assert!(s.holding_input(), "the interrupted prefix waits for painting");
    s.renderer_mut().painting_unready=false;
    settle(&mut s);
    assert_eq!(s.engine.metrics().committed_strokes,committed+1);
    let committed=s.engine.metrics().committed_strokes;
    s.renderer_mut().painting_unready=true;

    s.pen(event(&s, 6, PenPhase::Down, 0.5));
    for sequence in 7..8 + HELD_SAMPLES as u64 {
        s.pen(event(&s, sequence, PenPhase::Move, 0.5));
    }
    assert!(!s.holding_input(), "an oversized contact is dropped");
    s.pen(event(&s, 9 + HELD_SAMPLES as u64, PenPhase::Up, 0.5));
    s.renderer_mut().painting_unready = false;
    settle(&mut s);
    assert!(!s.holding_input());
    assert_eq!(s.engine.metrics().committed_strokes, committed);
    s.pen(event(&s, 10 + HELD_SAMPLES as u64, PenPhase::Down, 0.5));
    s.pen(event(&s, 11 + HELD_SAMPLES as u64, PenPhase::Up, 0.5));
    settle(&mut s);
    assert_eq!(s.engine.metrics().committed_strokes, committed + 1, "the device paints again after its discarded contact ends");
}

#[test]
fn completed_contacts_keep_the_held_queue_bounded() {
    let mut s = held_session();
    let committed = s.engine.metrics().committed_strokes;
    for contact in 0..HELD_SAMPLES / 2 + 1 {
        for (step, phase) in [(0, PenPhase::Down), (1, PenPhase::Up)] {
            s.pen(event(&s, (contact * 2 + step) as u64, phase, 0.5));
        }
    }
    assert_eq!(s.deferred_edits.len(), HELD_SAMPLES);
    assert!(s.discarded_contacts.is_empty());
    assert_eq!(s.engine.metrics().committed_strokes, committed);
}

#[test]
fn suspension_retires_held_input_with_its_queue() {
    let mut s = held_session();
    s.pen(event(&s, 1, PenPhase::Down, 0.5));
    s.pen(event(&s, 2, PenPhase::Up, 0.5));
    assert!(s.require_raster_snapshot().is_err());
    s.suspend_renderer().unwrap();
    assert!(!s.holding_input());
    assert!(s.require_raster_snapshot().is_ok());
}

fn paper_source() -> layer_core::color::source::SourceImage {
    use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
    let interpretation = SourceInterpretation { channels: SourceChannels::Gray, depth: layer_core::color::SampleDepth::U8,
        profile: Default::default(), profile_assumed: false };
    let mut builder = SourceBuilder::new([4, 4], interpretation, 1 << 20).unwrap();
    for row in 0..4u8 { builder.push_row(&[row * 40; 4]).unwrap(); }
    let mut source = builder.finish().unwrap();
    source.resolution = Some(layer_core::ImageResolution::ppi(300));
    source
}

#[test]
fn a_paper_that_loads_during_a_stroke_applies_after_it() {
    let mut s = session(Platform::Gtk);
    s.frame(1, 1).unwrap();
    let paper = crate::occurrence_token(s.engine.document().bottom_paper().unwrap());
    let id = "01".repeat(32);
    s.papers.pending = Some(crate::paper_library::PendingPaper { id: id.clone(), layer: paper, epoch: s.state.document_file.epoch, image: None });
    assert!(s.frame_demand() == layer_render::FrameDemand::Wait, "Waiting for the Paper image does not poll frames");
    s.pen(event(&s, 1, PenPhase::Down, 0.5));
    s.frame(2, 2).unwrap();
    s.paper_loaded(&id, paper_source()).unwrap();
    assert!(s.papers.pending.is_some(), "the chosen paper waits for the stroke");
    assert!(s.paper_image(paper).is_none());
    s.pen(event(&s, 2, PenPhase::Up, 0.5));
    let mut wake = true;
    let mut frame = 2;
    while s.papers.pending.is_some() && (wake || s.frame_demand() != layer_render::FrameDemand::Wait) {
        assert!(frame < 40, "Paper completion must settle within the existing frame budget");
        wake = s.frame(frame * 10_000_000, frame * 10_000_000).unwrap().canvas_wake;
        frame += 1;
    }
    assert!(s.papers.pending.is_none());
    assert!(s.paper_image(paper).is_some(), "the chosen paper applies once the stroke ends");
    let expired = frame * 10_000_000 + 1_300_000_000;
    s.frame(expired, expired).unwrap();
    assert!(s.frame_demand() == layer_render::FrameDemand::Wait, "Completed Paper work stops polling after its relief preview");
}

#[test]
fn a_paste_lands_before_the_undo_queued_behind_its_request() {
    let mut s = session(Platform::Gtk);
    s.renderer_mut().tiled_sources = true;
    s.frame(1, 1).unwrap();
    let checkpoint = s.engine.checkpoint();
    invoke(&mut s, CommandId::PasteImage);
    let id = s.state.requests.iter().find_map(|request| matches!(request.kind, HostRequestKind::Document { .. }).then_some(request.id)).unwrap();
    invoke(&mut s, CommandId::Undo);
    assert_eq!(s.deferred_edits.len(), 1, "Undo waits behind the Paste request");
    let source = Arc::new(Arc::unwrap_or_clone(layer_core::color::source::rgba8_source([8, 8], |_, _| [90, 120, 200, 255])));
    let clip = PixelClip { nonce: "clip".into(), name: "Ink".into(), source, policy: layer_core::PaintBasePolicy::WorkingPixels,
        origin: [0, 0], color: s.engine.document().composition().color, blend: layer_core::BlendSpace::Linear,
        png: Arc::from(&b"png"[..]), layers: None };
    s.paste_clip(&clip, PasteMode::Paste).unwrap();
    s.complete_document_request(id, Ok(true)).unwrap();
    assert_ne!(s.engine.checkpoint(), checkpoint);
    settle(&mut s);
    assert!(s.deferred_edits.is_empty());
    assert_eq!(s.engine.checkpoint(), checkpoint, "the queued Undo removes the paste that preceded it");
    s.suspend_renderer().unwrap();
    assert!(s.paste_clip(&clip, PasteMode::Paste).is_err(), "a completion cannot leave raster work without a renderer");
}

#[test]
fn held_contacts_keep_their_pressure_and_tilt_calibration_at_admission() {
    use layer_core::{TiltAxes,TiltCalibration,TiltResponse};
    use layer_engine::recording::{Record,Event,StopReason};
    let mut s=held_session();
    let pressure=layer_core::PressureResponse::try_from(vec![[0.,0.],[0.5,0.1],[1.,1.]]).unwrap();
    let tilt=layer_engine::TiltCurve::new(&TiltCalibration{response:TiltResponse::default(),axes:TiltAxes{reverse_horizontal:true,..Default::default()}},TiltAxes::default());
    s.state.settings.tilt_calibrations.insert("held".into(),TiltCalibration{response:TiltResponse::default(),axes:TiltAxes{reverse_horizontal:true,..Default::default()}});
    s.register_pen_device(&PenDeviceInfo{id:1,key:"held".into(),label:"Held pen".into(),native_to_surface:TiltAxes::default()});
    s.engine.set_pressure_curve(pressure.clone().into());s.engine.set_tilt_curve(tilt.clone());
    s.stroke_recording().start("held-calibration").unwrap();
    for (sequence,phase) in [(1,PenPhase::Down),(2,PenPhase::Move),(3,PenPhase::Up)] {
        s.pen(PenEvent{tilt_radians:[0.4,0.],..event(&s,sequence,phase,0.5)});
    }
    s.engine.set_pressure_curve(layer_engine::PressureCurve::default());s.engine.set_tilt_curve(layer_engine::TiltCurve::default());
    s.renderer_mut().painting_unready=false;settle(&mut s);
    let records={let mut recorder=s.stroke_recording();recorder.stop(StopReason::Manual);layer_engine::recording::read(recorder.bytes().unwrap().as_slice()).unwrap()};
    let samples:Vec<_>=records.iter().filter_map(|record|if let Record::Predictor(Event::Sample(point))=record{Some(layer_core::StrokePoint::from(*point))}else{None}).collect();
    assert!(!samples.is_empty());
    assert!(samples.iter().all(|point|(point.pressure-pressure.map(0.5)).abs()<1e-5 && point.tilt[0]<0.));
    assert_eq!(s.engine.configured_tilt_curve(),&layer_engine::TiltCurve::default());
}

#[derive(Clone, Copy, Debug)]
enum Landing { Pipelines, Submission }

fn edits(s: &UiSession<Recorder>) -> (u64, u64) {
    (s.engine.checkpoint(), s.engine.document().working.generation)
}

fn land(s: &mut UiSession<Recorder>) {
    s.renderer_mut().settling = false;
    s.renderer_mut().raster_dependencies_blocked = false;
    settle(s);
}

fn region_reply(s: &mut UiSession<Recorder>) {
    let request = s.renderer_mut().region_requests.last().unwrap().request_id;
    s.renderer_mut().region_reply = Some(layer_render::RegionResult { translucent_seed: false, colour_on_solid: false, gaps: None, underfill: None, request_id: request, tonal_sample: None,
        pixels: Arc::new(layer_core::SelectionPixels::bytes([8, 1], [0, 0, 8, 1], vec![0xffffffff; 2]).unwrap()),
        placement: layer_core::Affine::IDENTITY });
}

#[test]
fn tool_contacts_wait_for_raster_backing_and_commit_once_it_lands() {
    use layer_core::{FigurePaint, FigureShape, GradientShape, RulerKind};
    let tools = [
        LayerCanvasTool::Gradient { shape: GradientShape::Linear },
        LayerCanvasTool::Figure { shape: FigureShape::Rectangle, paint: FigurePaint::Fill },
        LayerCanvasTool::Figure { shape: FigureShape::Line, paint: FigurePaint::Outline },
        LayerCanvasTool::LassoFill,
        LayerCanvasTool::Select,
        LayerCanvasTool::Selection { kind: SelectionTool::Rectangle },
        LayerCanvasTool::Selection { kind: SelectionTool::Ellipse },
        LayerCanvasTool::Ruler { kind: RulerKind::Straight },
    ];
    for landing in [Landing::Pipelines, Landing::Submission] {
        for tool in tools {
            let mut s = session(Platform::Gtk);
            s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
            invoke(&mut s, CommandId::FitCanvas);
            select(&mut s, rectangle([100., 100., 900., 900.]));
            s.layer_action(LayerAction::Tool { tool }).unwrap();
            if let Landing::Pipelines = landing {
                s.dispatch(UiAction::Invoke { command: CommandId::FillSelection }).unwrap();
                s.renderer_mut().raster_dependencies_blocked = true;
                s.frame(2, 2).unwrap();
            }
            let before = edits(&s);
            pen_at(&mut s, 1, PenPhase::Down, [200., 200.]);
            pen_at(&mut s, 2, PenPhase::Move, [400., 250.]);
            pen_at(&mut s, 3, PenPhase::Move, [350., 450.]);
            if let Landing::Submission = landing {
                s.frame(3, 3).unwrap();
                s.renderer_mut().settling = true;
            }
            pen_at(&mut s, 4, PenPhase::Up, [350., 450.]);
            assert!(s.engine.raster_backing_pending());
            assert_eq!(notice_text(&s), None, "{landing:?} {tool:?}");
            assert_eq!(edits(&s), before, "{landing:?} {tool:?}: the release waits for the earlier work");
            land(&mut s);
            assert_eq!(notice_text(&s), None, "{landing:?} {tool:?}");
            assert!(s.deferred_edits.is_empty(), "{landing:?} {tool:?}");
            assert_ne!(edits(&s), before, "{landing:?} {tool:?}: the release commits once the work lands");
        }
    }
}

#[test]
fn vector_contacts_wait_for_raster_backing_and_commit_once_it_lands() {
    use layer_core::{FigurePaint, FigureShape};
    let mut s = session(Platform::Gtk);
    s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
    invoke(&mut s, CommandId::FitCanvas);
    layer(&mut s, LayerAction::NewVector);
    let layer = s.engine.document().working.occurrence.unwrap();
    let paths = |s: &UiSession<Recorder>| s.engine.document().scene().vector_layer(layer).unwrap().path_count();
    let waits = |s: &mut UiSession<Recorder>, first: u64, points: &[[f32; 2]], commit: PenPhase| {
        let before = edits(s);
        for (index, point) in points.iter().enumerate() {
            let phase = if index == 0 { PenPhase::Down } else if index + 1 == points.len() { PenPhase::Up } else { PenPhase::Move };
            if phase == commit { s.renderer_mut().settling = true; }
            pen_at(s, first + index as u64, phase, *point);
        }
        assert_eq!(notice_text(s), None);
        assert_eq!(edits(s), before, "the {commit:?} waits for the earlier work");
        land(s);
        assert_eq!(notice_text(s), None);
        assert_ne!(edits(s), before, "the {commit:?} commits once the work lands");
    };
    let line: Vec<[f32; 2]> = (0..16).map(|i| [200. + i as f32 * 20., 300. + (i as f32 * 0.6).sin() * 40.]).collect();
    waits(&mut s, 10, &line, PenPhase::Up);
    assert_eq!(paths(&s), 1);
    invoke(&mut s, CommandId::Move);
    settle(&mut s);
    assert!(s.engine.document().working.vector_items.is_none());
    waits(&mut s, 40, &[line[0], line[0]], PenPhase::Down);
    assert!(s.engine.document().working.vector_items.is_some(), "the press selects the path");
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Outline } }).unwrap();
    for (index, point) in [[400., 500.], [500., 500.], [450., 580.]].into_iter().enumerate() {
        pen_at(&mut s, 50 + index as u64 * 2, PenPhase::Down, point);
        pen_at(&mut s, 51 + index as u64 * 2, PenPhase::Up, point);
    }
    settle(&mut s);
    waits(&mut s, 60, &[[401., 501.], [401., 501.]], PenPhase::Down);
    assert_eq!(paths(&s), 2, "clicking the first anchor closes the path");
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Region { fill: true, source: RegionSource::Visible } }).unwrap();
    waits(&mut s, 70, &[[450., 520.], [450., 520.]], PenPhase::Up);
    invoke(&mut s, CommandId::Eraser);
    waits(&mut s, 80, &[[300., 200.], [300., 400.], [300., 400.]], PenPhase::Up);
}

#[test]
fn region_and_selection_brush_results_wait_for_raster_backing() {
    for tool in [LayerCanvasTool::Region { fill: true, source: RegionSource::Visible },
        LayerCanvasTool::Region { fill: false, source: RegionSource::Visible }] {
        let mut s = session(Platform::Gtk);
        s.layer_action(LayerAction::Tool { tool }).unwrap();
        let before = edits(&s);
        pen_at(&mut s, 1, PenPhase::Down, [200., 200.]);
        pen_at(&mut s, 2, PenPhase::Up, [200., 200.]);
        s.frame(2, 2).unwrap();
        s.renderer_mut().settling = true;
        region_reply(&mut s);
        s.frame(3, 3).unwrap();
        assert_eq!(edits(&s), before, "{tool:?}: the result waits for the earlier work");
        land(&mut s);
        assert_eq!(notice_text(&s), None);
        assert_ne!(edits(&s), before, "{tool:?}: the result commits once the work lands");
    }
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::SelectionBrush);
    pen_at(&mut s, 1, PenPhase::Down, [100., 100.]);
    pen_at(&mut s, 2, PenPhase::Up, [100., 100.]);
    s.frame(2, 2).unwrap();
    let id = s.engine.backend().selection_updates.last().unwrap().id;
    s.renderer_mut().settling = true;
    s.renderer_mut().selection_reply = Some(layer_render::SelectionPaintResult { changed: true, request_id: id,
        pixels: Arc::new(layer_core::SelectionPixels::bytes([4, 1], [0, 0, 4, 1], vec![0x80808080]).unwrap()) });
    s.frame(3, 3).unwrap();
    assert!(s.engine.document().working.selection.is_none(), "the capture waits for the earlier work");
    land(&mut s);
    assert!(s.engine.document().working.selection.is_some(), "the capture commits once the work lands");
}

#[test]
fn a_waiting_selection_press_that_is_cancelled_leaves_no_selection_mode_behind() {
    let mut s = session(Platform::Gtk);
    s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
    invoke(&mut s, CommandId::FitCanvas);
    select(&mut s, rectangle([100., 100., 300., 300.]));
    s.layer_action(LayerAction::Tool { tool: LayerCanvasTool::Selection { kind: SelectionTool::Rectangle } }).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::FillSelection }).unwrap();
    s.renderer_mut().raster_dependencies_blocked = true;
    s.frame(2, 2).unwrap();
    s.interaction.modifiers.shift = true;
    pen_at(&mut s, 1, PenPhase::Down, [500., 500.]);
    pen_at(&mut s, 2, PenPhase::Cancel, [500., 500.]);
    s.interaction.modifiers.shift = false;
    land(&mut s);
    pen_at(&mut s, 3, PenPhase::Down, [500., 500.]);
    pen_at(&mut s, 4, PenPhase::Move, [700., 700.]);
    pen_at(&mut s, 5, PenPhase::Up, [700., 700.]);
    settle(&mut s);
    let bounds = s.engine.document().working.selection.as_ref().unwrap().coverage_bounds();
    assert!(bounds.min.x >= 499., "the later drag replaces the selection instead of adding to it: {bounds:?}");
}
