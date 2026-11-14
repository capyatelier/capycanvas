use crate::authored::PaintSource;
use std::{any::Any, fmt::Debug, sync::Arc};

pub const CHUNK_DABS: usize = 8;
pub const MAX_HARDNESS: f32 = 0.75;

pub fn substeps(mode: crate::LiquifyMode, radii: [f32; 2], hardness: f32, strength: f32, motion: [f32; 2]) -> u32 {
    let radius = radii[0].min(radii[1]).max(1.);
    let edge = (1. - hardness.min(MAX_HARDNESS)).max(1. / radius);
    let bound = match mode {
        crate::LiquifyMode::Push => strength * motion[0].hypot(motion[1]) / radius,
        crate::LiquifyMode::Pinch | crate::LiquifyMode::Edge => 0.35 * strength,
        _ => 0.,
    };
    (2. * bound / edge).ceil().max(1.) as u32
}

#[cfg(not(target_arch = "wasm32"))]
pub trait Field: Any + Debug + Send + Sync {
    fn content_version(&self) -> crate::raster::ContentVersion;
    fn as_any(&self) -> &dyn Any;
    fn resources(&self, visit: &mut dyn FnMut(u64, usize));
    fn tiles(&self) -> Vec<[u32; 2]>;
}

#[cfg(target_arch = "wasm32")]
pub trait Field: Any + Debug {
    fn content_version(&self) -> crate::raster::ContentVersion;
    fn as_any(&self) -> &dyn Any;
    fn resources(&self, visit: &mut dyn FnMut(u64, usize));
    fn tiles(&self) -> Vec<[u32; 2]>;
}

#[derive(Debug)]
pub struct State {
    pub original: Arc<PaintSource>,
    pub field: Arc<dyn Field>,
    record: PaintSource,
    alpha_locked: bool,
    budgeted: std::sync::atomic::AtomicBool,
}

impl State {
    pub fn new(original: Arc<PaintSource>, field: Arc<dyn Field>, record: &PaintSource, alpha_locked: bool) -> Self {
        let mut record = record.clone();
        record.raster = Default::default();
        Self { original, field, record, alpha_locked, budgeted: false.into() }
    }

    pub(crate) fn needs_budget(&self) -> bool {
        !self.budgeted.swap(true, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn compatible(&self, source: &PaintSource, alpha_locked: bool) -> bool {
        self.alpha_locked == alpha_locked
            && self.record.color_mode == source.color_mode
            && self.record.domain == source.domain
            && self.record.base == source.base
            && self.record.operations == source.operations
    }
}

#[derive(Clone, Debug)]
pub struct Context {
    pub amount: Option<f32>,
    pub current: Arc<PaintSource>,
    pub original: Arc<PaintSource>,
    pub field: Option<Arc<dyn Field>>,
}

impl Context {
    pub fn for_source(source: &PaintSource, alpha_locked: bool) -> Self {
        let mut current = source.clone();
        current.raster = current.raster.without_liquify();
        #[cfg_attr(target_arch="wasm32",expect(clippy::arc_with_non_send_sync,reason="Shared paint ownership includes single-threaded Wasm Liquify fields"))]
        let current = Arc::new(current);
        if let Some(state) = source.raster.liquify().filter(|s| s.compatible(source, alpha_locked)) {
            return Self { amount: None, current, original: state.original.clone(), field: Some(state.field.clone()) };
        }
        Self { amount: None, original: current.clone(), current, field: None }
    }
}

impl PartialEq for Context {
    fn eq(&self, other: &Self) -> bool {
        self.amount == other.amount && Arc::ptr_eq(&self.original, &other.original)
            && match (&self.field, &other.field) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}

#[cfg(test)]
mod tests;
