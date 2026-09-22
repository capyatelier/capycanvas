use super::*;
use crate::{PenPhase, SampleFlags, ToolKind};
use layer_core::Point;

fn pen() -> PenEvent {
    PenEvent {
        device_id: 0xfedcba9876543210,
        sequence: 37,
        timestamp_ns: 9_876_543_210,
        view_revision: 99,
        surface_position: Point {
            x: -12.345,
            y: 4096.125,
        },
        pressure: 0.12345679,
        tilt_radians: [-0.2, 0.7],
        twist_radians: 1.25,
        distance: 0.3,
        phase: PenPhase::Move,
        tool: ToolKind::Eraser,
        flags: SampleFlags(0x3e),
    }
}

#[test]
fn shared_capture_can_stop_save_and_restart_without_losing_its_enabled_flag() {
    let recording = Recording::default();
    let parked = recording.clone();
    for _ in 0..2 {
        assert!(!recording.is_active());
        recording.lock().unwrap().start("test").unwrap();
        assert!(parked.is_active());
        parked.raw(pen(), ViewTransform::IDENTITY, PressureCurve::default());
        assert_eq!(recording.lock().unwrap().status().raw_events, 1);
        parked.lock().unwrap().stop(StopReason::Manual);
        assert!(!recording.is_active());
        recording.lock().unwrap().saved();
    }
}

#[test]
fn raw_roundtrip_is_lossless_and_keeps_duplicates_and_delivery_order() {
    let mut recorder = Recorder::default();
    assert!(recorder.data.is_empty());
    recorder.raw(pen(), ViewTransform::IDENTITY, PressureCurve::default());
    assert!(recorder.data.is_empty());
    recorder.start("test").unwrap();
    let mut events = vec![
        pen(),
        pen(),
        PenEvent {
            timestamp_ns: pen().timestamp_ns - 1000,
            ..pen()
        },
    ];
    events.push(PenEvent {
        flags: SampleFlags::PREDICTED,
        ..pen()
    });
    for &event in &events {
        recorder.raw(event, ViewTransform::IDENTITY, PressureCurve::default());
    }
    recorder.stop(StopReason::Manual);
    assert_eq!(recorder.status().raw_events, 4);
    let bytes = recorder.bytes().unwrap();
    assert_eq!(
        bytes,
        recorder.bytes().unwrap(),
        "cancel/retry retains exactly the same recording"
    );
    assert!(recorder.start("test").is_err());
    let decoded: Vec<_> = read(bytes.as_slice())
        .unwrap()
        .into_iter()
        .filter_map(|r| {
            if let Record::Raw { event, .. } = r {
                Some(event)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(decoded, events);
    recorder.saved();
    assert_eq!(recorder.data.capacity(), 0);
    recorder.start("test").unwrap();
}

#[test]
fn idle_timeout_and_size_limit_finish_valid_interrupted_contacts() {
    for reason in [StopReason::Duration, StopReason::Size] {
        let mut recorder = Recorder::default();
        recorder.start("test").unwrap();
        recorder.begin(
            100,
            Policy {
                config: Default::default(),
                transform: [1., 0., 0., 1., 0., 0.],
            },
        );
        recorder.event(Event::Sample(Sample(0, 1., 2., 0.7, 0., 0., 0.)));
        if reason == StopReason::Duration {
            recorder.started =
                Some(Instant::now() - std::time::Duration::from_secs(MAX_DURATION_SECS));
        } else {
            // Fill with real framed records, so the test also decodes the cap.
            while recorder.data.len() < MAX_BYTES - 4096 {
                recorder.append(Record::Raw {
                    delivery_ns: 0,
                    event: pen(),
                    transform: ViewTransform::IDENTITY,
                    pressure: PressureCurve::default(),
                });
            }
        }
        let status = recorder.status();
        assert!(!status.recording && status.ready);
        assert_eq!(status.reason, Some(reason));
        if reason == StopReason::Duration {
            assert_eq!(status.elapsed_seconds, MAX_DURATION_SECS);
        }
        assert!(status.bytes <= MAX_BYTES);
        let before = recorder.data.len();
        recorder.raw(pen(), ViewTransform::IDENTITY, PressureCurve::default());
        assert_eq!(before, recorder.data.len());
        let records = read(recorder.bytes().unwrap().as_slice()).unwrap();
        assert!(matches!(
            records[records.len() - 2],
            Record::End {
                interrupted: true,
                ..
            }
        ));
        assert!(matches!(records.last(), Some(Record::Footer { reason: r, .. }) if *r == reason));
    }
}

#[test]
fn rejects_truncation_corruption_unsupported_versions_and_incomplete_streams() {
    let mut recorder = Recorder::default();
    recorder.start("test").unwrap();
    recorder.stop(StopReason::Manual);
    let bytes = recorder.bytes().unwrap();
    for end in 0..bytes.len() {
        assert!(read(&bytes[..end]).is_err(), "accepted truncation at {end}");
    }
    let mut corrupt = bytes.clone();
    let n = corrupt.len();
    corrupt[n - 7] ^= 1;
    assert!(read(corrupt.as_slice()).is_err());
    let mut unknown = bytes.clone();
    unknown[7] = b'9';
    assert!(read(unknown.as_slice()).is_err());
    assert!(read(compress(&[]).unwrap().as_slice()).is_err());
}

#[cfg(feature = "prediction-bench")]
#[test]
fn empty_and_queryless_recordings_replay_without_nan() {
    for contact in [false, true] {
        let mut r = Recorder::default();
        r.start("test").unwrap();
        if contact {
            r.begin(
                1,
                Policy {
                    config: Default::default(),
                    transform: [1., 0., 0., 1., 0., 0.],
                },
            );
            r.event(Event::Sample(Sample(0, 0., 0., 0.5, 0., 0., 0.)));
        }
        r.stop(StopReason::Manual);
        let summary = crate::prediction_bench::replay(
            std::io::BufReader::with_capacity(1, r.bytes().unwrap().as_slice()),
            std::io::sink(),
        )
        .unwrap();
        assert_eq!(summary.queries, 0);
        assert!(summary.prediction_coverage.is_finite());
    }
}
