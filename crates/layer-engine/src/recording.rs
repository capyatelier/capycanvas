//! Bounded, opt-in capture of raw input and the production predictor's causal log.
//! Input callbacks only append binary records; compression happens on export.
use crate::{PenEvent, PressureCurve, ViewTransform};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use web_time::Instant;

mod schema;
pub use schema::{Event, Policy, Sample};

pub const MAX_DURATION_SECS: u64 = 600;
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
pub const MAGIC: &[u8; 8] = b"CAPYPEN3";

/// Parked drawings share their window's recorder. Disabled capture needs only
/// an atomic load, with no locking, allocation, clock reads or serialization.
#[derive(Clone)]
pub struct Recording {
    inner: Arc<Mutex<Recorder>>,
    active: Arc<AtomicBool>,
}
impl Default for Recording {
    fn default() -> Self {
        let recorder = Recorder::default();
        Self {
            active: recorder.active.clone(),
            inner: Arc::new(Mutex::new(recorder)),
        }
    }
}
impl Recording {
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
    pub fn lock(&self) -> std::sync::LockResult<std::sync::MutexGuard<'_, Recorder>> {
        self.inner.lock()
    }
    fn record(&self, f: impl FnOnce(&mut Recorder)) {
        if self.is_active() {
            f(&mut self.inner.lock().unwrap());
        }
    }
    pub fn raw(&self, event: PenEvent, transform: ViewTransform, pressure: PressureCurve) {
        self.record(|r| r.raw(event, transform, pressure));
    }
    pub fn begin(&self, timestamp_ns: u64, policy: Policy) {
        self.record(|r| r.begin(timestamp_ns, policy));
    }
    pub fn event(&self, event: Event) {
        self.record(|r| r.event(event));
    }
    pub fn observe(&self, event: PenEvent) {
        self.record(|r| r.observe(event));
    }
    pub fn query(&self, policy: Policy, now: u32, requested: u32) {
        self.record(|r| r.query(policy, now, requested));
    }
    pub fn end(&self, cancelled: bool) {
        self.record(|r| r.end(cancelled));
    }
}

/// Append-only schema. Variant order and field order are part of version 3.
/// All clocks are monotonic, never wall time. Raw timestamps retain host units
/// converted to ns; delivery_ns is relative to the start of the recording.
#[derive(Debug, Serialize, Deserialize)]
pub enum Record {
    Metadata(String),
    Raw {
        delivery_ns: u64,
        event: PenEvent,
        transform: ViewTransform,
        pressure: PressureCurve,
    },
    Begin {
        id: u64,
        timestamp_ns: u64,
        policy: Policy,
    },
    Predictor(Event),
    End {
        cancelled: bool,
        interrupted: bool,
    },
    Footer {
        records: u64,
        duration_ns: u64,
        reason: StopReason,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Manual,
    Duration,
    Size,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub recording: bool,
    pub ready: bool,
    pub elapsed_seconds: u64,
    pub bytes: usize,
    pub raw_events: u64,
    pub reason: Option<StopReason>,
    pub label: &'static str,
}

#[derive(Default)]
pub struct Recorder {
    active: Arc<AtomicBool>,
    data: Vec<u8>,
    started: Option<Instant>,
    reason: Option<StopReason>,
    duration_ns: u64,
    records: u64,
    raw_events: u64,
    contacts: u64,
    contact_start: Option<u64>,
    query: u64,
}

fn invalid(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

/// Length framing bounds each decode and permits sequential dataset tooling.
pub fn write_record(out: &mut Vec<u8>, record: &Record) {
    let offset = out.len();
    out.extend_from_slice(&[0; 4]);
    bincode::serde::encode_into_std_write(record, out, bincode::config::standard())
        .expect("Vec writes cannot fail");
    let length = (out.len() - offset - 4) as u32;
    out[offset..offset + 4].copy_from_slice(&length.to_le_bytes());
}

/// Read and validate framing, size, footer, and gzip CRC, including empty captures.
pub fn read(mut input: impl Read) -> io::Result<Vec<Record>> {
    let mut magic = [0; 8];
    input.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(invalid("unsupported stroke recording version"));
    }
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(input)
        .take((MAX_BYTES + 4097) as u64)
        .read_to_end(&mut decoded)?;
    if decoded.len() > MAX_BYTES + 4096 {
        return Err(invalid("stroke recording exceeds size limit"));
    }
    let mut remaining = decoded.as_slice();
    let mut records = Vec::new();
    while !remaining.is_empty() {
        if remaining.len() < 4 {
            return Err(invalid("truncated record length"));
        }
        let len = u32::from_le_bytes(remaining[..4].try_into().unwrap()) as usize;
        remaining = &remaining[4..];
        if len > remaining.len() || len > 64 * 1024 {
            return Err(invalid("invalid record length"));
        }
        let (record, used): (Record, usize) = bincode::serde::decode_from_slice(
            &remaining[..len],
            bincode::config::standard().with_limit::<65536>(),
        )
        .map_err(invalid)?;
        if used != len {
            return Err(invalid("trailing record bytes"));
        }
        records.push(record);
        remaining = &remaining[len..];
    }
    match (records.first(), records.last()) {
        (Some(Record::Metadata(_)), Some(Record::Footer { records: count, .. }))
            if *count == records.len() as u64 - 1 =>
        {
            ()
        }
        _ => return Err(invalid("missing metadata/footer or incomplete recording")),
    }
    if records[1..records.len() - 1]
        .iter()
        .any(|r| matches!(r, Record::Metadata(_) | Record::Footer { .. }))
    {
        return Err(invalid("duplicate metadata/footer"));
    }
    Ok(records)
}

pub fn compress(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = MAGIC.to_vec();
    let mut gzip = flate2::write::GzEncoder::new(&mut out, flate2::Compression::fast());
    gzip.write_all(data)?;
    gzip.finish()?;
    Ok(out)
}

impl Recorder {
    pub fn start(&mut self, platform: &str) -> Result<(), &'static str> {
        if self.started.is_some() || self.reason.is_some() {
            return Err("Save the existing stroke recording first");
        }
        let active = self.active.clone();
        *self = Self::default();
        self.active = active;
        self.data = Vec::with_capacity(MAX_BYTES);
        self.started = Some(Instant::now());
        self.active.store(true, Ordering::Relaxed);
        self.append(Record::Metadata(serde_json::json!({"format":"capy-pen-recording", "version":3, "platform":platform, "raw_input":true, "max_duration_seconds":MAX_DURATION_SECS}).to_string()));
        Ok(())
    }
    fn append(&mut self, record: Record) {
        write_record(&mut self.data, &record);
        self.records += 1;
    }
    fn live(&mut self) -> bool {
        if let Some(start) = self.started {
            if start.elapsed().as_secs() >= MAX_DURATION_SECS {
                self.stop(StopReason::Duration);
            } else if self.data.len() >= MAX_BYTES - 4096 {
                self.stop(StopReason::Size);
            }
        }
        self.started.is_some()
    }
    pub fn raw(&mut self, event: PenEvent, transform: ViewTransform, pressure: PressureCurve) {
        if !self.live() {
            return;
        }
        self.append(Record::Raw {
            delivery_ns: self.started.unwrap().elapsed().as_nanos() as u64,
            event,
            transform,
            pressure,
        });
        self.raw_events += 1;
    }
    pub fn begin(&mut self, timestamp_ns: u64, policy: Policy) {
        if !self.live() {
            return;
        }
        self.end(true);
        self.contacts += 1;
        self.contact_start = Some(timestamp_ns);
        self.query = 0;
        self.append(Record::Begin {
            id: self.contacts,
            timestamp_ns,
            policy,
        });
    }
    pub fn event(&mut self, event: Event) {
        if self.live() && self.contact_start.is_some() {
            self.append(Record::Predictor(event));
        }
    }
    pub fn observe(&mut self, event: PenEvent) {
        if let Some(start) = self.contact_start {
            self.event(Event::Observe(
                event.timestamp_ns.saturating_sub(start),
                event.pressure,
                event.tool,
                event.flags,
            ));
        }
    }
    pub fn query(&mut self, policy: Policy, now: u32, requested: u32) {
        self.event(Event::Policy(policy));
        self.event(Event::Query(self.query, now, requested));
        self.query += 1;
    }
    pub fn end(&mut self, cancelled: bool) {
        if self.contact_start.take().is_some() {
            self.append(Record::End {
                cancelled,
                interrupted: false,
            });
        }
    }
    pub fn stop(&mut self, reason: StopReason) {
        let Some(start) = self.started.take() else {
            return;
        };
        self.active.store(false, Ordering::Relaxed);
        self.duration_ns = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        let reason = if self.duration_ns >= MAX_DURATION_SECS * 1_000_000_000 {
            self.duration_ns = MAX_DURATION_SECS * 1_000_000_000;
            StopReason::Duration
        } else {
            reason
        };
        if self.contact_start.take().is_some() {
            self.append(Record::End {
                cancelled: false,
                interrupted: true,
            });
        }
        write_record(
            &mut self.data,
            &Record::Footer {
                records: self.records,
                duration_ns: self.duration_ns,
                reason,
            },
        );
        self.reason = Some(reason);
    }
    pub fn status(&mut self) -> Status {
        self.live();
        Status {
            recording: self.started.is_some(),
            ready: self.reason.is_some(),
            elapsed_seconds: self
                .started
                .map_or(self.duration_ns / 1_000_000_000, |s| s.elapsed().as_secs()),
            bytes: self.data.len(),
            raw_events: self.raw_events,
            reason: self.reason,
            label: if self.started.is_some() {
                "Stop stroke recording"
            } else if self.reason.is_some() {
                "Save stroke recording"
            } else {
                "Start stroke recording"
            },
        }
    }
    /// Retained until successful delivery; cancelling a chooser never loses data.
    pub fn snapshot(&self) -> io::Result<Vec<u8>> {
        if self.reason.is_none() {
            return Err(invalid("stop recording before export"));
        }
        Ok(self.data.clone())
    }
    pub fn bytes(&self) -> io::Result<Vec<u8>> {
        if self.reason.is_none() {
            return Err(invalid("stop recording before export"));
        }
        compress(&self.data)
    }
    pub fn saved(&mut self) {
        if self.reason.is_some() {
            let active = self.active.clone();
            *self = Self::default();
            self.active = active;
        }
    }
}

#[cfg(test)]
mod tests;
