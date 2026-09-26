// Included inside canvas::tests so this checks production capture against the
// actual live predictor state and backend, without a second input model.
#[test]
#[cfg(feature = "prediction-bench")]
fn recorded_production_queries_replay_exactly() {
    let (mut input, consumer) = input_queue(64);
    let transform = ViewTransform {
        revision: 1,
        ..ViewTransform::IDENTITY
    };
    let mut engine = CanvasEngine::new(
        RecordingRenderer::default(),
        Document::new("recording", 512, 512),
        consumer,
        view(512, 512),
        transform,
    )
    .unwrap();
    engine
        .set_instant_feedback(InstantFeedbackConfig {
            ..Default::default()
        })
        .unwrap();
    engine.set_pressure_curve(PressureCurve { gamma: 2. });
    engine
        .recording
        .lock()
        .unwrap()
        .start("engine-test")
        .unwrap();
    let mut expected = Vec::new();
    for i in 0..40 {
        let event = event(
            i * 4 + 1,
            if i == 0 {
                PenPhase::Down
            } else {
                PenPhase::Move
            },
            20. + i as f32 * 4.,
        );
        engine.record_raw_input(event, transform);
        input.push(event).unwrap();
        engine.process_input().unwrap();
        let now = engine
            .builder
            .elapsed_micros_at(event.timestamp_ns)
            .unwrap();
        let active = engine.active_stroke.as_ref().unwrap();
        let mut predictor = active.prediction.clone();
        let forecast = predictor
            .estimate_for(
                engine.builder.real_points(),
                engine.builder.predicted_points(),
                now + 8000,
                now,
                engine.view.document_to_surface,
                active.feedback,
            )
            .unwrap();
        expected.push(forecast.point.position);
        engine
            .render_frame_for(event.timestamp_ns, event.timestamp_ns + 8_000_000)
            .unwrap();
    }
    let end = event(162, PenPhase::Up, 176.);
    engine.record_raw_input(end, transform);
    input.push(end).unwrap();
    engine.render_frame_at(end.timestamp_ns).unwrap();
    engine
        .recording
        .lock()
        .unwrap()
        .stop(crate::recording::StopReason::Manual);
    let bytes = engine.recording.lock().unwrap().bytes().unwrap();
    let records = crate::recording::read(bytes.as_slice()).unwrap();
    let raw: Vec<_> = records
        .iter()
        .filter_map(|r| {
            if let crate::recording::Record::Raw { event, .. } = r {
                Some(event)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(raw.len(), 41);
    assert_eq!(raw[0].pressure, 0.8);
    assert!(records.iter().any(|r| matches!(r, crate::recording::Record::Predictor(crate::recording::Event::Sample(s)) if (s.3-0.64).abs()<1e-6)));
    let mut csv = Vec::new();
    let summary =
        crate::prediction_bench::replay(std::io::BufReader::new(bytes.as_slice()), &mut csv)
            .unwrap();
    assert_eq!(summary.queries, expected.len());
    assert_eq!(summary.samples, 41);
    for (row, point) in String::from_utf8(csv)
        .unwrap()
        .lines()
        .skip(1)
        .zip(expected)
    {
        let columns: Vec<_> = row.split(',').collect();
        assert_eq!(columns[7].parse::<f32>().unwrap(), point.x);
        assert_eq!(columns[8].parse::<f32>().unwrap(), point.y);
    }
}
