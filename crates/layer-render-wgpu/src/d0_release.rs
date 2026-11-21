use std::cell::Cell;
use std::collections::BTreeMap;
use std::time::Duration;
use web_time::Instant;

pub(crate) const KINDS: [&str; 4] = ["exact", "halves", "levels", "other"];
const RETRIES: u8 = 4;
const RETRY: Duration = Duration::from_millis(50);

pub(crate) type Tally = BTreeMap<u32, [[u64; 2]; 4]>;

#[derive(Default)]
pub(crate) struct Released {
    pub released: Tally,
    pub kept: Tally,
    pub pinned: Tally,
}
impl Released {
    pub(crate) fn bytes(tally: &Tally) -> [u64; 4] {
        let mut bytes = [0; 4];
        for kinds in tally.values() { for (total, [_, kind]) in bytes.iter_mut().zip(kinds) { *total += kind; } }
        bytes
    }
}

pub(crate) struct Switch {
    pub kinds: [bool; 4],
    pub keep_view: bool,
    after: Duration,
    armed: Cell<bool>,
    since: Cell<Option<Instant>>,
    attempt: Cell<u8>,
    retry: Cell<Option<Instant>>,
}
impl Switch {
    pub(crate) fn from_env() -> Option<Self> {
        let chosen = std::env::var("CAPY_D0_RELEASE").ok()?;
        let kinds = KINDS.map(|kind| chosen.split(',').any(|name| name.trim() == kind));
        let keep_view = std::env::var("CAPY_D0_KEEP_VIEW").map_or(true, |value| value.trim() != "0");
        let after = std::env::var("CAPY_D0_AFTER_MS").ok().and_then(|value| value.trim().parse().ok()).unwrap_or(0);
        kinds.contains(&true).then(|| Self::new(kinds, keep_view, Duration::from_millis(after)))
    }
    pub(crate) fn new(kinds: [bool; 4], keep_view: bool, after: Duration) -> Self {
        Self { kinds, keep_view, after, armed: Cell::new(true), since: Cell::new(None), attempt: Cell::new(0), retry: Cell::new(None) }
    }
    pub(crate) fn input(&self) {
        self.armed.set(true);
        self.since.set(None);
        self.attempt.set(0);
        self.retry.set(None);
    }
    pub(crate) fn due(&self, settled: bool) -> bool {
        if !settled { self.since.set(None); }
        if !settled || !self.armed.get() { return false; }
        let now = Instant::now();
        self.since.set(Some(self.since.get().unwrap_or(now)));
        self.deadline().is_some_and(|deadline| deadline <= now)
    }
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.since.get().filter(|_| self.armed.get()).map(|since| self.retry.get().unwrap_or(since + self.after))
    }
    pub(crate) fn finish(&self, report: &Released, store_bytes: u64) -> Option<String> {
        let attempt = self.attempt.get();
        let retry = !report.pinned.is_empty() && attempt < RETRIES;
        self.armed.set(retry);
        self.attempt.set(attempt + 1);
        self.retry.set(retry.then(|| Instant::now() + RETRY));
        if attempt != 0 && report.released.is_empty() { return None; }
        let unix_ms = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default().as_millis();
        let chosen: Vec<_> = KINDS.iter().zip(self.kinds).filter(|(_, chosen)| *chosen).map(|(kind, _)| format!("\"{kind}\"")).collect();
        let rows = |tally: &Tally| tally.iter().flat_map(|(level, kinds)| KINDS.iter().zip(kinds).filter(|(_, [pages, _])| *pages != 0)
            .map(move |(kind, [pages, bytes])| format!("{{\"level\":{level},\"kind\":\"{kind}\",\"pages\":{pages},\"bytes\":{bytes}}}")))
            .collect::<Vec<_>>().join(",");
        let total = |tally: &Tally| Released::bytes(tally).into_iter().sum::<u64>();
        Some(format!("{{\"unix_ms\":{unix_ms},\"kinds\":[{}],\"keep_view\":{},\"after_ms\":{},\"attempt\":{attempt},\"retrying\":{retry},\
            \"released_bytes\":{},\"kept_bytes\":{},\"pinned_bytes\":{},\"store_bytes\":{store_bytes},\
            \"released\":[{}],\"kept\":[{}],\"pinned\":[{}]}}",
            chosen.join(","), self.keep_view, self.after.as_millis(), total(&report.released), total(&report.kept), total(&report.pinned),
            rows(&report.released), rows(&report.kept), rows(&report.pinned)))
    }
}
