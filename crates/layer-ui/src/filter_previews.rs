//! One optional-work lifecycle per editor. Hosts supply visible IDs, physical
//! geometry and a monotonic clock; they retain only the delivered native images.
use crate::UiSession;
use layer_render::{CanvasRenderer, FilterPreviewImage};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

const IDLE_NS: u64 = 200_000_000;
const RETRY_NS: u64 = 1_000_000_000;
const CACHE_ROWS: usize = 64;

/// Presentation facts, not a host-side invalidation policy. Reporting retained
/// images also recovers from a lost view or a failed native bitmap conversion.
#[derive(Default, Deserialize)]
pub struct FilterPreviewCache {
    pub key: Option<String>,
    pub rows: Vec<Arc<str>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FilterPreviewStatus {
    /// Opaque presentation generation, including document and GPU replacement.
    pub key: String,
    pub epoch: u64,
    pub pending: bool,
    /// Zero means service on the next host frame; never spin in the caller.
    pub wait_ms: u32,
    pub retained: Vec<Arc<str>>,
    pub requests: u64,
    pub error: Option<String>,
}
pub struct FilterPreviewUpdate {
    pub status: FilterPreviewStatus,
    pub image: Option<FilterPreviewImage>,
}
#[derive(Clone, Copy, PartialEq)]
struct Source {
    epoch: u64,
    revision: (u64, u64, u64),
}
struct Pending {
    id: u64,
    filters: Vec<Arc<str>>,
}
#[derive(Default)]
pub(crate) struct Previews {
    source: Option<Source>,
    size: [u32; 2],
    generation: u64,
    serial: u64,
    requests: u64,
    pending: Option<Pending>,
    loaded: BTreeMap<Arc<str>, u64>,
    retry_at: u64,
    paused: bool,
}
impl Previews {
    pub fn renderer_replaced(&mut self) {
        self.source = None;
        self.pending = None;
        self.loaded.clear();
        self.generation = self.generation.wrapping_add(1);
        self.retry_at = 0;
    }
    fn cancel<R: CanvasRenderer>(&mut self, renderer: &mut R) {
        if self.pending.take().is_some() {
            renderer.cancel_filter_previews();
        }
    }
    fn invalidate<R: CanvasRenderer>(&mut self, renderer: &mut R, now_ns: u64) {
        self.cancel(renderer);
        self.loaded.clear();
        self.generation = self.generation.wrapping_add(1);
        self.retry_at = now_ns.saturating_add(IDLE_NS);
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    /// A host output-color change can replace the preview surface without
    /// changing document pixels. Retire its requests through the same lifecycle.
    pub fn reset_filter_previews(&mut self) {
        self.renderer_mut().cancel_filter_previews();
        self.filter_previews.renderer_replaced();
    }
    /// Poll once per host wake-up. An empty visible list cancels optional work.
    /// This call may submit one bounded GPU chunk, and must run on its owner.
    pub fn poll_filter_previews(
        &mut self,
        now_ns: u64,
        filters: Vec<Arc<str>>,
        size: [u32; 2],
        cache: FilterPreviewCache,
    ) -> Result<FilterPreviewUpdate, String> {
        if filters.len() > 256
            || cache.rows.len() > CACHE_ROWS
            || filters.iter().chain(&cache.rows).any(|id| id.len() > 256)
        {
            return Err("Invalid visible filter list".into());
        }
        let mut visible = Vec::new();
        for id in filters {
            if self.effect_catalog.get(&id).is_some() && !visible.contains(&id) {
                visible.push(id);
            }
            if visible.len() == CACHE_ROWS {
                break;
            }
        }
        let source = Source {
            epoch: self.state.document_file.epoch,
            revision: self.filter_preview_revision(),
        };
        let idle = self.filter_previews_idle();
        let mut driver = std::mem::take(&mut self.filter_previews);
        let requested_size = [size[0].clamp(80, 512), size[1].clamp(1, 128)];
        let source_changed = driver.source != Some(source);
        // Retain a sufficient backing size across drawer/panel projections.
        let size = if source_changed {
            requested_size
        } else {
            std::array::from_fn(|i| driver.size[i].max(requested_size[i]))
        };
        if source_changed || (!visible.is_empty() && driver.size != size) {
            driver.invalidate(self.renderer_mut(), now_ns);
            driver.source = Some(source);
            driver.size = size;
        }
        if !idle {
            if !driver.paused {
                driver.invalidate(self.renderer_mut(), now_ns);
            }
            driver.retry_at = now_ns.saturating_add(IDLE_NS);
        }
        driver.paused = !idle;
        let key = format!("{}:{}", source.epoch, driver.generation);
        if cache.key.as_deref() != Some(&key) {
            driver.loaded.clear();
        } else {
            let previous = driver.loaded.len();
            driver.loaded.retain(|id, _| cache.rows.contains(id));
            if previous != driver.loaded.len() {
                driver.retry_at = now_ns.saturating_add(RETRY_NS);
            }
        }
        if visible.is_empty()
            || driver
                .pending
                .as_ref()
                .is_some_and(|p| p.filters.iter().all(|id| !visible.contains(id)))
        {
            driver.cancel(self.renderer_mut());
            driver.retry_at = driver.retry_at.max(now_ns.saturating_add(IDLE_NS));
        }
        for id in &visible {
            if let Some(used) = driver.loaded.get_mut(id) {
                *used = now_ns;
            }
        }
        let mut image = None;
        let mut error = None;
        if idle && !visible.is_empty() {
            if driver.pending.is_some()
                && let Some(result) = self.renderer_mut().take_filter_previews()
            {
                let pending = driver.pending.take().unwrap();
                match result {
                    Ok(atlas)
                        if atlas.image.request_id == pending.id
                            && atlas.filters == pending.filters
                            && atlas.image.width == driver.size[0]
                            && atlas.image.height
                                == driver.size[1] * atlas.filters.len() as u32
                            && atlas.image.stride == atlas.image.width * 4
                            && atlas.image.bytes.len()
                                == (atlas.image.stride * atlas.image.height) as usize =>
                    {
                        for id in &atlas.filters {
                            driver.loaded.insert(id.clone(), now_ns);
                        }
                        while driver.loaded.len() > CACHE_ROWS {
                            let oldest = driver
                                .loaded
                                .iter()
                                .filter(|(id, _)| !visible.contains(id))
                                .min_by_key(|(_, used)| *used)
                                .map(|(id, _)| id.clone())
                                .unwrap();
                            driver.loaded.remove(&oldest);
                        }
                        image = Some(atlas);
                    }
                    Ok(_) => error = Some("Invalid or stale filter preview atlas".into()),
                    Err(e) => error = Some(e.to_string()),
                }
            }
            if error.is_none() && driver.pending.is_none() && now_ns >= driver.retry_at {
                let missing: Vec<_> = visible
                    .iter()
                    .filter(|id| !driver.loaded.contains_key(*id))
                    .take(8)
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    driver.serial = driver.serial.wrapping_add(1);
                    match self.request_filter_previews(driver.serial, missing.clone(), driver.size)
                    {
                        Ok(true) => {
                            driver.requests += 1;
                            driver.pending = Some(Pending {
                                id: driver.serial,
                                filters: missing,
                            });
                        }
                        Ok(false) => driver.retry_at = now_ns.saturating_add(IDLE_NS),
                        Err(e) => error = Some(e),
                    }
                }
            }
        }
        if error.is_some() {
            driver.cancel(self.renderer_mut());
            driver.retry_at = now_ns.saturating_add(RETRY_NS);
        }
        let wait_ms = if !idle {
            50
        } else if driver.pending.is_some() {
            0
        } else {
            ((driver.retry_at.saturating_sub(now_ns).max(IDLE_NS) + 999_999) / 1_000_000) as u32
        };
        let status = FilterPreviewStatus {
            key,
            epoch: source.epoch,
            pending: driver.pending.is_some(),
            wait_ms,
            retained: driver.loaded.keys().cloned().collect(),
            requests: driver.requests,
            error,
        };
        self.filter_previews = driver;
        Ok(FilterPreviewUpdate { status, image })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{AssetId, Document};
    use layer_render::{BackendError, FilterPreviewRequest, FramePacket, HostImage, ReadbackImage};

    #[derive(Default)]
    struct Backend {
        request: Option<FilterPreviewRequest>,
        ready: Option<Result<FilterPreviewImage, BackendError>>,
        requests: usize,
        takes: usize,
        cancels: usize,
        reject: bool,
    }
    impl CanvasRenderer for Backend {
        type Error = BackendError;
        fn resize_surface(&mut self, _: u32, _: u32) -> Result<(), Self::Error> {
            Ok(())
        }
        fn prepare_asset(&mut self, _: &AssetId, _: HostImage<'_>) -> Result<(), Self::Error> {
            Ok(())
        }
        fn release_asset(&mut self, _: &AssetId) {}
        fn submit(&mut self, _: FramePacket<'_>) -> Result<(), Self::Error> {
            Ok(())
        }
        fn request_filter_previews(
            &mut self,
            request: FilterPreviewRequest,
        ) -> Result<bool, Self::Error> {
            if self.reject {
                return Err(BackendError("preview unavailable"));
            }
            assert!(self.request.is_none(), "only one request may be in flight");
            self.requests += 1;
            self.request = Some(request);
            Ok(true)
        }
        fn take_filter_previews(&mut self) -> Option<Result<FilterPreviewImage, Self::Error>> {
            self.takes += 1;
            let ready = self.ready.take()?;
            self.request = None;
            Some(ready)
        }
        fn cancel_filter_previews(&mut self) {
            self.cancels += 1;
            self.request = None;
            self.ready = None;
        }
    }
    struct View {
        key: Option<String>,
        rows: Vec<Arc<str>>,
        size: [u32; 2],
    }
    impl Default for View {
        fn default() -> Self {
            Self {
                key: None,
                rows: Vec::new(),
                size: [96, 40],
            }
        }
    }
    impl View {
        fn poll(
            &mut self,
            s: &mut UiSession<Backend>,
            ms: u64,
            ids: &[&str],
        ) -> FilterPreviewUpdate {
            let update = s
                .poll_filter_previews(
                    ms * 1_000_000,
                    ids.iter().map(|id| Arc::from(*id)).collect(),
                    self.size,
                    FilterPreviewCache {
                        key: self.key.clone(),
                        rows: self.rows.clone(),
                    },
                )
                .unwrap();
            if self.key.as_ref() != Some(&update.status.key) {
                self.rows.clear();
            }
            self.key = Some(update.status.key.clone());
            self.rows.retain(|id| update.status.retained.contains(id));
            if let Some(image) = &update.image {
                self.rows.extend(image.filters.iter().cloned());
            }
            update
        }
    }
    fn session() -> UiSession<Backend> {
        let mut session = UiSession::new(
            Backend::default(),
            Document::new("preview", 512, 512),
            [512, 512],
        )
        .unwrap();
        session.frame(0, 0).unwrap();
        assert!(session.filter_previews_idle());
        session
    }
    fn finish(s: &mut UiSession<Backend>) {
        let r = s.renderer_mut();
        let request = r.request.as_ref().unwrap();
        let [width, height] = request.size;
        let height = height * request.filters.len() as u32;
        r.ready = Some(Ok(FilterPreviewImage {
            image: ReadbackImage {
                request_id: request.request_id,
                width,
                height,
                stride: width * 4,
                bytes: vec![255; (width * height * 4) as usize],
            },
            filters: request
                .filters
                .iter()
                .map(|f| f.program.id.clone())
                .collect(),
        }));
    }
    #[test]
    fn pending_work_is_prompt_idle_work_is_debounced_and_rows_are_reused() {
        let mut s = session();
        let mut v = View::default();
        assert!(
            !v.poll(&mut s, 0, &["curves", "curves", "levels"])
                .status
                .pending
        );
        assert_eq!(s.renderer_mut().requests, 0);
        assert!(v.poll(&mut s, 200, &["curves", "levels"]).status.pending);
        let pending = v.poll(&mut s, 208, &["curves", "levels"]);
        assert_eq!(pending.status.wait_ms, 0);
        assert_eq!(s.renderer_mut().takes, 1);
        finish(&mut s);
        assert!(v.poll(&mut s, 216, &["curves", "levels"]).image.is_some());
        v.poll(&mut s, 500, &[]);
        let reopened = v.poll(&mut s, 700, &["levels"]);
        assert_eq!(reopened.status.requests, 1);
        assert!(!reopened.status.pending);
        assert!(reopened.status.wait_ms >= 200);
        v.size = [80, 20];
        assert_eq!(
            v.poll(&mut s, 900, &["curves"]).status.requests,
            1,
            "smaller projections reuse rows"
        );
        v.size = [200, 40];
        assert!(v.poll(&mut s, 1000, &["curves"]).status.retained.is_empty());
        v.poll(&mut s, 1200, &["curves"]);
        assert_eq!(s.renderer_mut().request.as_ref().unwrap().size, [200, 40]);
    }
    #[test]
    fn drawing_cancels_without_taking_or_advancing_the_old_request() {
        let mut s = session();
        let mut v = View::default();
        v.poll(&mut s, 0, &["curves"]);
        v.poll(&mut s, 200, &["curves"]);
        finish(&mut s);
        s.input_pending = true;
        let paused = v.poll(&mut s, 208, &["curves"]);
        assert!(!paused.status.pending);
        assert_eq!(paused.status.wait_ms, 50);
        assert!(paused.image.is_none());
        assert_eq!(s.renderer_mut().takes, 0);
        assert_eq!(s.renderer_mut().cancels, 1);
        v.poll(&mut s, 500, &["curves"]);
        assert_eq!(s.renderer_mut().requests, 1);
        s.input_pending = false;
        v.poll(&mut s, 600, &["curves"]);
        assert_eq!(s.renderer_mut().requests, 1);
        v.poll(&mut s, 700, &["curves"]);
        assert_eq!(s.renderer_mut().requests, 2);
    }
    #[test]
    fn hiding_and_replacement_retire_pending_work_even_at_equal_revisions() {
        let mut s = session();
        let mut v = View::default();
        v.poll(&mut s, 0, &["curves"]);
        v.poll(&mut s, 200, &["curves"]);
        let original = v.key.clone();
        v.poll(&mut s, 208, &[]);
        assert_eq!(s.renderer_mut().cancels, 1);
        v.poll(&mut s, 408, &["curves"]);
        finish(&mut s);
        s.state.document_file.epoch += 1;
        let replaced = v.poll(&mut s, 416, &["curves"]);
        assert!(replaced.image.is_none());
        assert_ne!(v.key, original);
        assert_eq!(s.renderer_mut().cancels, 2);
        v.poll(&mut s, 616, &["curves"]);
        let before_gpu = v.key.clone();
        s.replace_renderer(Backend::default()).unwrap();
        s.frame(700_000_000, 700_000_000).unwrap();
        assert!(v.poll(&mut s, 700, &["curves"]).image.is_none());
        assert_ne!(v.key, before_gpu);
        assert!(v.poll(&mut s, 900, &["curves"]).status.pending);
    }
    #[test]
    fn changing_visible_categories_retires_work_and_stale_atlases_back_off() {
        let mut s = session();
        let mut v = View::default();
        v.poll(&mut s, 0, &["curves"]);
        v.poll(&mut s, 200, &["curves"]);
        finish(&mut s);
        let changed = v.poll(&mut s, 208, &["levels"]);
        assert!(changed.image.is_none());
        assert!(!changed.status.pending);
        assert_eq!(s.renderer_mut().cancels, 1);
        assert_eq!(s.renderer_mut().takes, 0);
        v.poll(&mut s, 408, &["levels"]);
        finish(&mut s);
        s.renderer_mut()
            .ready
            .as_mut()
            .unwrap()
            .as_mut()
            .unwrap()
            .image
            .request_id = 1;
        let stale = v.poll(&mut s, 416, &["levels"]);
        assert!(stale.image.is_none());
        assert!(stale.status.error.is_some());
        assert_eq!(stale.status.wait_ms, 1000);
        assert!(stale.status.retained.is_empty());
        assert!(!v.poll(&mut s, 1000, &["levels"]).status.pending);
        assert!(v.poll(&mut s, 1416, &["levels"]).status.pending);
    }
    #[test]
    fn optional_failures_back_off_and_missing_host_images_can_be_retried() {
        let mut s = session();
        let mut v = View::default();
        s.renderer_mut().reject = true;
        v.poll(&mut s, 0, &["curves"]);
        let failure = v.poll(&mut s, 200, &["curves"]);
        assert!(failure.status.error.is_some());
        assert_eq!(failure.status.wait_ms, 1000);
        s.renderer_mut().reject = false;
        assert!(!v.poll(&mut s, 1199, &["curves"]).status.pending);
        assert!(v.poll(&mut s, 1200, &["curves"]).status.pending);
        finish(&mut s);
        v.poll(&mut s, 1208, &["curves"]);
        v.rows.clear(); // Native conversion failed or its retained image was lost.
        assert!(!v.poll(&mut s, 1400, &["curves"]).status.pending);
        assert!(v.poll(&mut s, 2400, &["curves"]).status.pending);
        s.renderer_mut().ready = Some(Err(BackendError("readback failed")));
        assert!(v.poll(&mut s, 2408, &["curves"]).status.error.is_some());
        assert!(!v.poll(&mut s, 2500, &["curves"]).status.pending);
        assert!(v.poll(&mut s, 3408, &["curves"]).status.pending);
    }
    #[test]
    fn visible_rows_are_batched_and_host_cache_is_bounded() {
        let mut s = session();
        let mut v = View::default();
        let ids: Vec<String> = s
            .effect_catalog
            .filters()
            .iter()
            .take(10)
            .map(|f| f.program.id.to_string())
            .collect();
        let ids: Vec<_> = ids.iter().map(String::as_str).collect();
        assert_eq!(ids.len(), 10);
        v.poll(&mut s, 0, &ids);
        v.poll(&mut s, 200, &ids);
        assert_eq!(s.renderer_mut().request.as_ref().unwrap().filters.len(), 8);
        finish(&mut s);
        let first = v.poll(&mut s, 208, &ids);
        assert_eq!(first.image.unwrap().filters.len(), 8);
        assert_eq!(s.renderer_mut().request.as_ref().unwrap().filters.len(), 2);
        // Fill the cache with previously visited offscreen rows before delivery.
        for i in 0..56 {
            let id: Arc<str> = format!("offscreen-{i}").into();
            s.filter_previews.loaded.insert(id.clone(), 0);
            v.rows.push(id);
        }
        finish(&mut s);
        let second = v.poll(&mut s, 216, &ids);
        assert_eq!(second.status.retained.len(), CACHE_ROWS);
        assert!(ids.iter().all(|id| {
            second
                .status
                .retained
                .iter()
                .any(|kept| kept.as_ref() == *id)
        }));
    }
}
