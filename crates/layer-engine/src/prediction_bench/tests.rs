use super::*;
use crate::{InstantFeedbackConfig, SampleFlags, ToolKind};

fn recording(change_future: bool) -> Vec<u8> {
    let policy = Policy {
        config: InstantFeedbackConfig {
            prediction_horizon_micros: 16_000,
            use_platform_prediction: true,
            ..Default::default()
        },
        transform: [1., 0., 0., 1., 0., 0.],
    };
    let mut events = Vec::new();
    for i in 0..40 {
        let x = i as f32 * 4. + if change_future && i >= 30 { 100. } else { 0. };
        let sample = Sample(i * 4000, x, 0., 0.6, 0.1, 0.2, 0.3);
        events.push(Event::Sample(sample));
        events.push(Event::Observe(
            u64::from(i) * 4_000_000,
            0.6,
            ToolKind::Pen,
            SampleFlags::PRIMARY,
        ));
        if i == 15 {
            events.push(Event::Replace(2, Sample(8000, 8.1, 0., 0.6, 0.1, 0.2, 0.3)));
            events.push(Event::Reset);
        }
        if i == 20 {
            events.push(Event::Predicted(Sample(
                i * 4000 + 16_000,
                x + 16.,
                8.,
                0.6,
                0.1,
                0.2,
                0.3,
            )));
        }
        if i >= 10 {
            events.push(Event::Query(u64::from(i), i * 4000, i * 4000 + 16_000));
        }
    }
    let header = DatasetHeader {
        format: "capy-pen-dataset".into(),
        version: 1,
        contacts: 1,
        events: events.len(),
        metadata: serde_json::Value::Null,
    };
    let contact = Contact {
        id: 1,
        policy,
        events,
        cancelled: false,
    };
    format!(
        "{}\n{}\n",
        serde_json::to_string(&header).unwrap(),
        serde_json::to_string(&contact).unwrap()
    )
    .into_bytes()
}

#[test]
fn replay_is_causal_and_keeps_native_precedence_and_corrections() {
    let mut outputs = Vec::new();
    for changed in [false, true] {
        let mut csv = Vec::new();
        let summary = replay(recording(changed).as_slice(), &mut csv).unwrap();
        assert_eq!(summary.contacts, 1);
        assert_eq!(summary.samples, 40);
        assert_eq!(summary.queries, 30);
        let csv = String::from_utf8(csv).unwrap();
        let rows: Vec<Vec<String>> = csv
            .lines()
            .skip(1)
            .map(|l| l.split(',').map(str::to_owned).collect())
            .collect();
        assert_eq!(rows[10][6], "Platform");
        assert_eq!(rows[10][8], "8");
        outputs.push(rows);
    }
    // Future truth changes scores, never earlier predictions or target times.
    for (a, b) in outputs[0][..20].iter().zip(&outputs[1][..20]) {
        assert_eq!(a[..9], b[..9]);
    }
}

#[test]
fn frame_export_is_causal_includes_corrections_and_does_not_change_predictions() {
    let input = recording(false);
    let mut plain = Vec::new();
    replay(input.as_slice(), &mut plain).unwrap();
    let mut csv = Vec::new();
    let mut frames = Vec::new();
    replay_with_frames(input.as_slice(), &mut csv, &mut frames).unwrap();
    assert_eq!(csv, plain);
    let frames: Vec<serde_json::Value> = std::str::from_utf8(&frames)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(frames[0]["geometry"], "pre_brush");
    assert_eq!(frames.len(), 31);
    let mut real: Vec<serde_json::Value> = Vec::new();
    for f in &frames[1..] {
        let start = f["real_start"].as_u64().unwrap() as usize;
        real.truncate(start);
        real.extend(f["real"].as_array().unwrap().iter().cloned());
        let curve = f["preview"].as_array().unwrap();
        assert_eq!(curve[0], *real.last().unwrap());
        assert_eq!(curve.last().unwrap()[0], f["target_us"]);
        assert!(
            real.iter()
                .all(|s| s[0].as_u64() <= f["latest_us"].as_u64())
        );
    }
    assert!((real[2][1].as_f64().unwrap() - 8.1).abs() < 1e-5);
    assert_eq!(frames[11]["source"], "Platform");
    let mut future = Vec::new();
    replay_with_frames(recording(true).as_slice(), io::sink(), &mut future).unwrap();
    for (a, b) in frames.iter().take(21).zip(
        std::str::from_utf8(&future)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()),
    ) {
        assert_eq!(*a, b, "future truth must never leak into exported previews");
    }
}

#[test]
fn refuses_incomplete_unknown_and_invalid_datasets() {
    let valid = String::from_utf8(recording(false)).unwrap();
    for malformed in [
        valid.lines().next().unwrap().to_owned(),
        valid.replace("\"version\":1", "\"version\":99"),
        valid.replace("\"contacts\":1", "\"contacts\":2"),
        valid.replace("\"id\":1", "\"id\":1,\"typo\":0"),
        valid.replace("\"replace\":[2,", "\"replace\":[9999,"),
        valid.replace("\"query\":[11,", "\"query\":[10,"),
    ] {
        assert!(
            replay(malformed.as_bytes(), io::sink()).is_err(),
            "{malformed}"
        );
    }
}

#[test]
fn reference_includes_stops_and_exact_endpoints_but_not_missing_history() {
    let points = [
        Sample(1000, 10., 0., 1., 0., 0., 0.),
        Sample(2000, 10., 0., 1., 0., 0., 0.),
        Sample(100_000, 40., 0., 1., 0., 0., 0.),
    ]
    .map(StrokePoint::from);
    let m = [2., 0., 0., 2., 5., 0.];
    assert_eq!(reference(&points, 1500, m), Some([25., 0.]));
    assert_eq!(reference(&points, 100_000, m), Some([85., 0.]));
    for t in [0, 50_000, 100_001] {
        assert!(reference(&points, t, m).is_none());
    }
}

#[test]
fn collected_recording_bank_preserves_accuracy_and_useful_prediction() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let mut recordings: Vec<_> = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "capystrokes"))
        .collect();
    recordings.sort();
    assert!(!recordings.is_empty());
    for path in recordings {
        let baseline_path = path.with_extension("expected.json");
        let fixture: serde_json::Value = serde_json::from_reader(
            std::fs::File::open(&baseline_path)
                .unwrap_or_else(|e| panic!("{}: {e}", baseline_path.display())),
        )
        .unwrap();
        assert_eq!(
            fixture["version"],
            1,
            "{}: unsupported baseline",
            path.display()
        );

        let mut references = vec![(PredictionAlgorithm::Optimized, &fixture)];
        if let Some(previous) = fixture.get("alternatives").and_then(|v| v.get("previous")) {
            references.push((PredictionAlgorithm::Previous, previous));
        }
        for (algorithm, reference) in references {
            let label = format!("{} {algorithm:?}", path.display());
            let baseline = &reference["summary"];
            assert!(baseline.is_object(), "{}: missing baseline", label);
            let reader = io::BufReader::new(std::fs::File::open(&path).unwrap());
            let actual = replay_with_options(reader, io::sink(), None, Some(algorithm)).unwrap();
            let measured = serde_json::to_value(&actual).unwrap();
            for key in ["contacts", "samples", "queries"] {
                assert_eq!(measured[key], baseline[key], "{} {key}", label);
            }
            for key in ["graded_queries", "transitions"] {
                assert_eq!(
                    measured["accuracy"][key],
                    baseline["accuracy"][key],
                    "{} {key}",
                    label
                );
            }
            for key in [
                "tiny_4_to_8",
                "small_8_to_16",
                "medium_16_to_32",
                "severe_ge32",
                "position_rms_px",
                "error_step_rms_px",
                "worst_step_px",
            ] {
                assert!(
                    measured["accuracy"][key].as_f64().unwrap()
                        <= baseline["accuracy"][key].as_f64().unwrap() + 1e-6,
                    "{} {key}: {} > {}",
                    label,
                    measured["accuracy"][key],
                    baseline["accuracy"][key]
                );
            }
            assert!(
                actual.mean_sample_horizon_ms
                    >= baseline["mean_sample_horizon_ms"].as_f64().unwrap() * 0.99
            );
            assert!(
                actual.mean_display_lead_ms
                    >= baseline["mean_display_lead_ms"].as_f64().unwrap() * 0.99
            );
            assert!(
                actual.prediction_coverage >= baseline["prediction_coverage"].as_f64().unwrap() - 0.001
            );
        }
    }
}
