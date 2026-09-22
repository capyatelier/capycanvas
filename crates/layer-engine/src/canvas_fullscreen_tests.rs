// Included in canvas::tests to exercise the real preview/brush path with the
// existing recording renderer. Coordinates and assertions are physical pixels.
#[test]
fn fullscreen_prediction_with_wide_brushes_and_independent_clocks() {
    for rate in [200., 240.] {
        for zoom in [1., 2.] {
            for width in [4., 64., 128.] {
                let (mut input, consumer) = input_queue(32);
                let transform = ViewTransform {
                    revision: 1,
                    surface_to_document: [1. / zoom, 0., 0., 1. / zoom, 0., 0.],
                };
                let mut surface = view(3840, 2160);
                surface.document_to_surface = [zoom, 0., 0., zoom, 0., 0.];
                let mut engine = CanvasEngine::new(
                    RecordingRenderer::default(),
                    Document::new("full-screen prediction", 3840, 2160),
                    consumer,
                    surface,
                    transform,
                )
                .unwrap();
                let cfg = InstantFeedbackConfig {

                    use_platform_prediction: false,
                    prediction_horizon_micros: 32_000,
                    timestamp_resolution_micros: 1000,
                    ..Default::default()
                };
                engine.set_instant_feedback(cfg).unwrap();
                engine
                    .set_brush(BrushSnapshot {
                        diameter: width / zoom,
                        spacing: 0.1,
                        ..Default::default()
                    })
                    .unwrap();
                let position = |t: f64| Point {
                    x: (1920. + 850. * (8. * t).cos()) as f32,
                    y: (1080. + 850. * (8. * t).sin()) as f32,
                };
                let mut sample = 0;
                let mut last = event(1, PenPhase::Down, 0.);
                let mut previous: Option<f32> = None;
                let mut worst_jump = 0.0_f32;
                for frame in 1..96 {
                    let now = frame as f64 / 119.88;
                    loop {
                        let t = (sample as f64 + if sample % 2 == 0 { 0. } else { 0.1 }) / rate
                            + 0.00037;
                        if t + 0.002 > now {
                            break;
                        }
                        let p = position(t);
                        last = PenEvent {
                            timestamp_ns: 1_000_000_000 + (t * 1000.).floor() as u64 * 1_000_000,
                            surface_position: Point {
                                x: p.x + 0.35 * (sample as f32 * 2.399).sin(),
                                y: p.y + 0.35 * (sample as f32 * 1.731).cos(),
                            },
                            pressure: (0.18 + 0.08 * (60. * t).sin()) as f32,
                            phase: if sample == 0 {
                                PenPhase::Down
                            } else {
                                PenPhase::Move
                            },
                            sequence: sample + 1,
                            ..last
                        };
                        input.push(last).unwrap();
                        sample += 1;
                    }
                    let frame_ns = 1_000_000_000 + (now * 1e9) as u64;
                    engine
                        .render_frame_for(frame_ns, frame_ns + 8_340_000)
                        .unwrap();
                    let elapsed = engine.builder.elapsed_micros_at(frame_ns).unwrap();
                    let estimate = engine
                        .active_stroke
                        .as_mut()
                        .unwrap()
                        .prediction
                        .estimate_for(
                            engine.builder.real_points(),
                            &[],
                            elapsed + 32_000,
                            elapsed,
                            surface.document_to_surface,
                            cfg,
                        )
                        .unwrap();
                    assert!(dabs_cover_point(
                        &engine.backend.preview,
                        estimate.point.position
                    ));
                    if estimate.source == TipSource::Engine {
                        let rendered = engine.backend.preview.last().unwrap().center;
                        assert!(
                            surface_distance(
                                rendered,
                                estimate.point.position,
                                surface.document_to_surface
                            ) < 0.01,
                            "frame={frame} brush={width} rendered={rendered:?} estimate={estimate:?}"
                        );
                    }
                    if now > 0.2 {
                        assert_eq!(estimate.source, TipSource::Engine);
                        let truth = position(now);
                        let dx = estimate.point.position.x * zoom - truth.x;
                        let dy = estimate.point.position.y * zoom - truth.y;
                        let lead = dx * -(8. * now).sin() as f32 + dy * (8. * now).cos() as f32;
                        if let Some(old) = previous {
                            worst_jump = worst_jump.max((lead - old).abs());
                        }
                        previous = Some(lead);
                    }
                }
                assert!(
                    worst_jump < 8.,
                    "brush={width} rate={rate} zoom={zoom}: {worst_jump}px"
                );
                input
                    .push(PenEvent {
                        phase: PenPhase::Up,
                        sequence: sample + 1,
                        timestamp_ns: last.timestamp_ns + 1_000_000,
                        ..last
                    })
                    .unwrap();
                engine
                    .render_frame_at(last.timestamp_ns + 2_000_000)
                    .unwrap();
                assert!(engine.backend.preview.is_empty());
            }
        }
    }
}
