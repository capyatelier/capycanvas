use super::{Accuracy, Contact, DatasetHeader, Event, Policy, Sample};
use crate::{
    PenEvent, PenPhase, PredictionAlgorithm,
    feedback::{PredictionState, TipSource},
};
use layer_core::{Point, StrokePoint};
use std::{
    collections::HashSet,
    io::{self, BufRead, Read, Write},
};

#[derive(Debug, Default, serde::Serialize)]
pub struct ReplaySummary {
    pub contacts: usize,
    pub samples: usize,
    pub queries: usize,
    pub prediction_coverage: f64,
    pub mean_sample_horizon_ms: f64,
    pub mean_display_lead_ms: f64,
    pub accuracy: Accuracy,
}
struct Row {
    id: u64,
    now: u32,
    requested: u32,
    latest: u32,
    point: StrokePoint,
    source: TipSource,
    transform: [f32; 6],
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn surface(p: Point, m: [f32; 6]) -> [f64; 2] {
    let [x, y] = [f64::from(p.x), f64::from(p.y)];
    [
        f64::from(m[0]) * x + f64::from(m[2]) * y + f64::from(m[4]),
        f64::from(m[1]) * x + f64::from(m[3]) * y + f64::from(m[5]),
    ]
}
fn policy_valid(policy: Policy) -> io::Result<()> {
    policy
        .config
        .validate()
        .map_err(|_| invalid("invalid predictor policy"))?;
    if !policy.transform.iter().all(|x| x.is_finite()) {
        return Err(invalid("nonfinite transform"));
    }
    Ok(())
}
fn point(sample: Sample) -> io::Result<StrokePoint> {
    let p = StrokePoint::from(sample);
    if ![
        p.position.x,
        p.position.y,
        p.pressure,
        p.tilt[0],
        p.tilt[1],
        p.twist,
    ]
    .iter()
    .all(|v| v.is_finite())
    {
        return Err(invalid("nonfinite sample"));
    }
    Ok(p)
}
/// Offline truth only: exact samples or interpolation over at most 32 ms.
/// Stationary intervals and the final exact sample remain valid truth.
fn reference(points: &[StrokePoint], time: u32, transform: [f32; 6]) -> Option<[f64; 2]> {
    let next = points.partition_point(|p| p.elapsed_micros <= time);
    let a = points.get(next.checked_sub(1)?)?;
    let pa = surface(a.position, transform);
    if time == a.elapsed_micros {
        return Some(pa);
    }
    let b = points.get(next)?;
    let dt = b.elapsed_micros.checked_sub(a.elapsed_micros)?;
    if dt == 0 || dt > 32_000 {
        return None;
    }
    let t = f64::from(time - a.elapsed_micros) / f64::from(dt);
    let pb = surface(b.position, transform);
    Some([pa[0] + t * (pb[0] - pa[0]), pa[1] + t * (pb[1] - pa[1])])
}

/// Replay one recording causally through Smooth Motion, preserving input,
/// policy, native precedence and the recorded query schedule.
/// CSV positions and truth are physical surface pixels. Unavailable truth is
/// blank, never treated as zero error. Query IDs allow exact pair matching.
pub fn replay(reader: impl BufRead, csv: impl Write) -> io::Result<ReplaySummary> {
    replay_with_options(reader, csv, None, None)
}

/// Export the full causal preview path as well as endpoint accuracy. Actual
/// samples are incremental document-space replacements starting at real_start;
/// prediction includes the measured anchor and the production intermediate
/// points. This is pre-brush geometry, not a claim to reproduce an unrecorded
/// brush/material or actual GPU presentation timestamps.
pub fn replay_with_frames(
    reader: impl BufRead,
    csv: impl Write,
    mut frames: impl Write,
) -> io::Result<ReplaySummary> {
    replay_with_options(reader, csv, Some(&mut frames), None)
}

/// Optionally override the recorded algorithm for an otherwise identical replay.
/// `None` preserves the selection recorded in each policy event.
pub fn replay_with_options(
    mut reader: impl BufRead,
    csv: impl Write,
    mut frames: Option<&mut dyn Write>,
    algorithm: Option<PredictionAlgorithm>,
) -> io::Result<ReplaySummary> {
    if let Some(writer) = &mut frames {
        writeln!(
            writer,
            "{}",
            serde_json::json!({"format":"capy-prediction-frames","version":1,"coordinates":"document","geometry":"pre_brush","clock":"recorded_query"})
        )?;
    }
    let mut prefix = [0; 8];
    reader.read_exact(&mut prefix)?;
    let binary = &prefix == crate::recording::MAGIC;
    let reader = io::BufReader::new(io::Cursor::new(prefix).chain(reader));
    if binary {
        return replay_binary(reader, csv, frames, algorithm);
    }
    let mut lines = reader.lines();
    let header: DatasetHeader = serde_json::from_str(
        &lines
            .next()
            .ok_or_else(|| invalid("missing dataset header"))??,
    )?;
    if header.format != "capy-pen-dataset" || header.version != 1 {
        return Err(invalid("unsupported dataset format/version"));
    }
    if header.contacts == 0 || header.events == 0 {
        return Err(invalid("empty dataset"));
    }
    replay_contacts(
        header,
        lines.map(|line| Ok(serde_json::from_str::<Contact>(&line?)?)),
        csv,
        frames,
        algorithm,
    )
}

fn replay_contacts(
    header: DatasetHeader,
    contacts: impl IntoIterator<Item = io::Result<Contact>>,
    mut csv: impl Write,
    mut frames: Option<&mut dyn Write>,
    algorithm: Option<PredictionAlgorithm>,
) -> io::Result<ReplaySummary> {
    let mut summary = ReplaySummary::default();
    let mut ids = HashSet::new();
    let mut event_count = 0;
    let mut predictions = 0;
    writeln!(
        csv,
        "contact,query,frame_us,latest_us,requested_us,target_us,source,x,y,truth_x,truth_y"
    )?;
    for contact in contacts {
        let contact = contact?;
        if !ids.insert(contact.id) {
            return Err(invalid("duplicate contact"));
        }
        summary.contacts += 1;
        let mut policy = contact.policy;
        policy_valid(policy)?;
        let mut state = PredictionState::default();
        let mut real = Vec::new();
        let mut platform = Vec::new();
        let mut rows = Vec::new();
        let mut queries = HashSet::new();
        let mut traced_real = 0;
        for event in contact.events {
            event_count += 1;
            match event {
                Event::Sample(sample) => {
                    real.push(point(sample)?);
                    platform.clear();
                    summary.samples += 1;
                }
                Event::Predicted(sample) => {
                    if platform.len() == 32 {
                        platform.remove(0);
                    }
                    platform.push(point(sample)?);
                }
                Event::Stationary(sample) => real.push(point(sample)?),
                Event::Replace(index, sample) => {
                    traced_real = traced_real.min(index);
                    *real
                        .get_mut(index)
                        .ok_or_else(|| invalid("invalid correction index"))? = point(sample)?;
                    platform.clear();
                }
                Event::Observe(timestamp_ns, pressure, tool, flags) => {
                    if !pressure.is_finite() {
                        return Err(invalid("nonfinite raw pressure"));
                    }
                    state.observe(PenEvent {
                        timestamp_ns,
                        pressure,
                        tool,
                        flags,
                        device_id: contact.id,
                        sequence: 0,
                        view_revision: 0,
                        surface_position: Point { x: 0., y: 0. },
                        tilt_radians: [0.; 2],
                        twist_radians: 0.,
                        distance: 0.,
                        phase: PenPhase::Move,
                    });
                }
                Event::Reset => state = PredictionState::default(),
                Event::Policy(value) => {
                    policy_valid(value)?;
                    policy = value;
                }
                Event::Query(id, now, requested) => {
                    if !queries.insert(id) {
                        return Err(invalid("duplicate query"));
                    }
                    let latest = real
                        .last()
                        .ok_or_else(|| invalid("query without input"))?
                        .elapsed_micros;
                    let mut config = policy.config;
                    if let Some(algorithm) = algorithm {
                        config.prediction_algorithm = algorithm;
                    }
                    let forecast = state
                        .estimate_for(&real, &platform, requested, now, policy.transform, config)
                        .ok_or_else(|| invalid("missing forecast for nonempty input"))?;
                    if let Some(writer) = frames.as_mut() {
                        let anchor = *real.last().unwrap();
                        let mut preview = vec![Sample::from(anchor)];
                        if forecast.source == TipSource::Platform {
                            let raw = crate::feedback::estimate_tip(
                                &real,
                                &platform,
                                forecast.point.elapsed_micros,
                                policy.transform,
                                config,
                            )
                            .unwrap()
                            .point;
                            preview.extend(
                                platform
                                    .iter()
                                    .copied()
                                    .filter(|p| {
                                        p.elapsed_micros > latest
                                            && p.elapsed_micros < forecast.point.elapsed_micros
                                    })
                                    .map(|p| {
                                        Sample::from(PredictionState::platform_point(
                                            anchor,
                                            p,
                                            raw,
                                            forecast.point,
                                            policy.transform,
                                            config.max_prediction_distance_px,
                                        ))
                                    }),
                            );
                        }
                        preview.extend(state.engine_intermediates().map(Sample::from));
                        if forecast.point != anchor {
                            preview.push(forecast.point.into());
                        }
                        let actual: Vec<_> = real[traced_real..]
                            .iter()
                            .copied()
                            .map(Sample::from)
                            .collect();
                        serde_json::to_writer(
                            &mut **writer,
                            &serde_json::json!({
                                "contact":contact.id,"query":id,"frame_us":now,"latest_us":latest,
                                "requested_us":requested,"target_us":forecast.point.elapsed_micros,
                                "source":format!("{:?}",forecast.source),"transform":policy.transform,
                                "real_start":traced_real,"real":actual,"preview":preview,
                            }),
                        )?;
                        writeln!(writer)?;
                        traced_real = real.len();
                    }
                    summary.queries += 1;
                    predictions += usize::from(forecast.source != TipSource::Real);
                    summary.mean_sample_horizon_ms +=
                        f64::from(forecast.point.elapsed_micros.saturating_sub(latest)) / 1000.;
                    summary.mean_display_lead_ms +=
                        (f64::from(forecast.point.elapsed_micros) - f64::from(now)) / 1000.;
                    rows.push(Row {
                        id,
                        now,
                        requested,
                        latest,
                        point: forecast.point,
                        source: forecast.source,
                        transform: policy.transform,
                    });
                }
            }
        }
        // Sorting is restricted to offline truth, after prediction has finished.
        real.sort_by_key(|p| p.elapsed_micros);
        real.reverse();
        real.dedup_by_key(|p| p.elapsed_micros);
        real.reverse();
        let mut previous: Option<(u32, [f64; 2])> = None;
        for row in rows {
            let predicted = surface(row.point.position, row.transform);
            let truth = reference(&real, row.point.elapsed_micros, row.transform);
            if let Some(truth) = truth {
                let error = [predicted[0] - truth[0], predicted[1] - truth[1]];
                let prior = previous
                    .filter(|(time, _)| row.now > *time && row.now - *time <= 50_000)
                    .map(|(_, error)| error);
                summary.accuracy.add(error, prior);
                previous = Some((row.now, error));
            }
            let optional = |axis: usize| truth.map(|p| p[axis].to_string()).unwrap_or_default();
            writeln!(
                csv,
                "{},{},{},{},{},{},{:?},{},{},{},{}",
                contact.id,
                row.id,
                row.now,
                row.latest,
                row.requested,
                row.point.elapsed_micros,
                row.source,
                predicted[0],
                predicted[1],
                optional(0),
                optional(1)
            )?;
        }
    }
    if summary.contacts != header.contacts || event_count != header.events {
        return Err(invalid("incomplete dataset: contact/event counts differ"));
    }
    if summary.queries > 0 {
        summary.prediction_coverage = predictions as f64 / summary.queries as f64;
        summary.mean_sample_horizon_ms /= summary.queries as f64;
        summary.mean_display_lead_ms /= summary.queries as f64;
    }
    summary.accuracy.finish();
    csv.flush()?;
    if let Some(writer) = &mut frames {
        writer.flush()?;
    }
    Ok(summary)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

fn replay_binary(
    reader: impl io::Read,
    csv: impl Write,
    frames: Option<&mut dyn Write>,
    algorithm: Option<PredictionAlgorithm>,
) -> io::Result<ReplaySummary> {
    use crate::recording::Record;
    let records = crate::recording::read(reader)?;
    let mut contacts = Vec::new();
    let mut active: Option<Contact> = None;
    for record in records {
        match record {
            Record::Begin { id, policy, .. } => {
                if active.is_some() {
                    return Err(invalid("nested contact"));
                }
                active = Some(Contact {
                    id,
                    policy,
                    events: Vec::new(),
                    cancelled: false,
                });
            }
            Record::Predictor(event) => active
                .as_mut()
                .ok_or_else(|| invalid("event outside contact"))?
                .events
                .push(event),
            Record::End {
                cancelled,
                interrupted,
            } => {
                let mut contact = active
                    .take()
                    .ok_or_else(|| invalid("end outside contact"))?;
                contact.cancelled = cancelled || interrupted;
                contacts.push(contact);
            }
            _ => (),
        }
    }
    if active.is_some() {
        return Err(invalid("unterminated contact"));
    }
    let header = DatasetHeader {
        format: "capy-pen-dataset".into(),
        version: 2,
        contacts: contacts.len(),
        events: contacts.iter().map(|c| c.events.len()).sum(),
        metadata: Default::default(),
    };
    replay_contacts(header, contacts.into_iter().map(Ok), csv, frames, algorithm)
}
