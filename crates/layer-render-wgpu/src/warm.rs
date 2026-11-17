use super::*;
use layer_core::MemoryReport;

const MIB: u64 = 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Warm {
    owner: u64,
    artwork: layer_core::authored::PortableId,
    revision: u64,
}
impl Warm {
    pub(crate) fn of(document: &layer_core::Document) -> Self {
        Self { owner: document.owner, artwork: document.artwork.id, revision: document.revision }
    }
}

impl WgpuRasterizer {
    /// Keeps the display levels a background drawing presents, its Navigator
    /// image while the Navigator is shown, and the selected layer's halves
    /// nearest the last dab, as optional memory below every active demand.
    /// False when that set exceeds the device's cap.
    pub fn park_warm(&mut self, document: &layer_core::Document, view: layer_render::ViewState, navigator: bool) -> bool {
        if !self.has_presentable_frame() { return false; }
        let cap = self.warm_tab_bytes();
        let mut encoder = submission::CommandEncoder::new(&self.device, &Default::default());
        let mut scene = self.scene.take();
        let mut cache = self.scale_display.take();
        self.warm = Some(Warm::of(document));
        self.retire_scratch(scene.as_deref_mut(), cache.as_mut(), &mut encoder);
        if !navigator { self.navigator.release(); }
        let kept = cache.as_mut().map_or(Ok(Vec::new()), |cache| cache.keep_warm(self, &mut encoder, view, cap / 4));
        let parked = kept.is_ok();
        self.scene_tiers.display.heap().reclaim(self.scene_tiers.display.used());
        self.memory.invalidate_limits();
        self.apply_memory_grants(scene.as_deref_mut(), cache.as_mut(), &mut encoder);
        self.device.source_samples.set_limit(0);
        self.device.source_samples.trim_unused(usize::MAX);
        self.scene = scene;
        let mut cells = kept.unwrap_or_default();
        while self.warm_bytes() > cap && let (Some(cache), Some(cell)) = (&cache, cells.pop()) { cache.release_warm_cell(cell); }
        self.scale_display = cache;
        self.uploads.finish(&encoder);
        if encoder.has_work() { self.last_submission = encoder.submit(&self.queue); }
        parked && self.warm_bytes() <= cap && self.keeps_warm()
    }
    /// Publishes a parked drawing's live size and whether the budget still holds it.
    pub fn keeps_warm(&self) -> bool {
        self.publish_memory();
        self.publish_samples();
        self.factory.memory_budget().flush_reports();
        self.memory.retention_grant().saturating_add(self.scene_tiers.retiring.used()) >= self.scene_tiers.display.used()
    }
    pub fn warm_bytes(&self) -> u64 {
        self.resident_bytes().saturating_sub(self.scene_tiers.retiring.used()).saturating_sub(self.retired_display_bytes.used())
            .saturating_sub(self.device.transient_display.used())
    }
    /// Phones, Web and 4 GB tablets keep 64 MiB of background drawings,
    /// larger tablets 192 MiB and desktops 512 MiB.
    pub fn warm_tab_bytes(&self) -> u64 {
        let facts = self.factory.memory_budget().facts();
        match facts.total_bytes {
            Some(total) if facts.tablet => if total > 6 << 30 { 192 * MIB } else { 64 * MIB },
            None if facts.tablet || facts.process_limit_bytes.is_some() => 64 * MIB,
            _ => 512 * MIB,
        }
    }
    pub(crate) fn warm_report(&self, report: MemoryReport, optional: u64) -> MemoryReport {
        if self.warm.is_none() { return report; }
        MemoryReport { ram_bytes: report.ram_bytes, gpu_bytes: report.gpu_bytes, optional_gpu_bytes: optional, ..Default::default() }
    }
}

#[cfg(test)]
#[path = "warm_tests.rs"]
mod tests;
