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
        let summary = replay(recording(changed).as_slice(), None, &mut csv).unwrap();
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
            replay(malformed.as_bytes(), None, io::sink()).is_err(),
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
        .filter(|p| p.to_string_lossy().ends_with(".jsonl.gz"))
        .collect();
    recordings.sort();
    assert!(!recordings.is_empty());
    for path in recordings {
        let reader = io::BufReader::new(flate2::read::GzDecoder::new(
            std::fs::File::open(&path).unwrap(),
        ));
        let actual = replay(reader, Some(PredictionAlgorithm::Trajectory), io::sink()).unwrap();
        let baseline_path = path.with_file_name(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .replace(".jsonl.gz", ".expected.json"),
        );
        let baseline: serde_json::Value =
            serde_json::from_reader(std::fs::File::open(baseline_path).unwrap()).unwrap();
        let measured = serde_json::to_value(&actual).unwrap();
        for key in ["contacts", "samples", "queries"] {
            assert_eq!(measured[key], baseline[key], "{} {key}", path.display());
        }
        for key in ["graded_queries", "transitions"] {
            assert_eq!(
                measured["accuracy"][key], baseline["accuracy"][key],
                "{key}"
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
                path.display(),
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
