use super::{Accuracy, Event, Policy, Sample};
use crate::{
    PenEvent, PenPhase,
    feedback::{PredictionState, TipSource},
    recording::Record,
};
use layer_core::{Point, StrokePoint};
use std::{
    collections::HashSet,
    io::{self, Read, Write},
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
struct Contact {
    id: u64,
    policy: Policy,
    events: Vec<Event>,
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
pub fn replay(reader: impl Read, csv: impl Write) -> io::Result<ReplaySummary> {
    replay_inner(reader, csv, None)
}

/// Export the full causal preview path as well as endpoint accuracy. Actual
/// samples are incremental document-space replacements starting at real_start;
/// prediction includes the measured anchor and the production intermediate
/// points. This is pre-brush geometry, not a claim to reproduce an unrecorded
/// brush/material or actual GPU presentation timestamps.
pub fn replay_with_frames(
    reader: impl Read,
    csv: impl Write,
    mut frames: impl Write,
) -> io::Result<ReplaySummary> {
    replay_inner(reader, csv, Some(&mut frames))
}

fn replay_inner(
    reader: impl Read,
    csv: impl Write,
    mut frames: Option<&mut dyn Write>,
) -> io::Result<ReplaySummary> {
    if let Some(writer) = &mut frames {
        writeln!(
            writer,
            "{}",
            serde_json::json!({"format":"capy-prediction-frames","version":1,"coordinates":"document","geometry":"pre_brush","clock":"recorded_query"})
        )?;
    }
    let mut contacts = Vec::new();
    let mut active: Option<Contact> = None;
    for record in crate::recording::read(reader)? {
        match record {
            Record::Begin { id, policy, .. } => {
                if active.is_some() {
                    return Err(invalid("nested contact"));
                }
                active = Some(Contact {
                    id,
                    policy,
                    events: Vec::new(),
                });
            }
            Record::Predictor(event) => active
                .as_mut()
                .ok_or_else(|| invalid("event outside contact"))?
                .events
                .push(event),
            Record::End { .. } => {
                contacts.push(active.take().ok_or_else(|| invalid("end outside contact"))?)
            }
            _ => (),
        }
    }
    if active.is_some() {
        return Err(invalid("unterminated contact"));
    }
    replay_contacts(contacts, csv, frames)
}

fn replay_contacts(
    contacts: Vec<Contact>,
    mut csv: impl Write,
    mut frames: Option<&mut dyn Write>,
) -> io::Result<ReplaySummary> {
    let mut summary = ReplaySummary::default();
    let mut ids = HashSet::new();
    let mut predictions = 0;
    writeln!(
        csv,
        "contact,query,frame_us,latest_us,requested_us,target_us,source,x,y,truth_x,truth_y"
    )?;
    for contact in contacts {
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
                    let config = policy.config;
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
        for row in rows {
            let predicted = surface(row.point.position, row.transform);
            let truth = reference(&real, row.point.elapsed_micros, row.transform);
            if let Some(truth) = truth {
                summary.accuracy.add([predicted[0] - truth[0], predicted[1] - truth[1]]);
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
