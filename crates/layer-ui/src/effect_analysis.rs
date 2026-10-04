use super::*;
use layer_core::{ArtworkQuery, ArtworkSource, authored::OccurrenceHandle};
use std::sync::Arc;

struct Entry { query: ArtworkQuery, ready: bool, failed: bool, animated: bool }
impl Entry {
    fn new(document: &Document, layer: OccurrenceHandle, time: f32) -> Self {
        let mut query = ArtworkQuery::new(document, ArtworkSource::EffectInput(layer));
        Arc::make_mut(&mut query.snapshot).context.elapsed = time;
        let animated = layer_core::EffectInputKey::new(query.snapshot.clone(), layer).is_some_and(|key|
            key.contributors().any(|handle| key.scene().effect(handle).is_some_and(|effect| effect.animated())));
        Self { query, ready: false, failed: false, animated }
    }
    fn layer(&self) -> OccurrenceHandle { let ArtworkSource::EffectInput(id) = self.query.source else { unreachable!() }; id }
}
#[derive(Default)]
pub(super) struct Analyses {
    entries: Vec<Entry>,
    active: Option<ArtworkQuery>,
    epoch: u64,
    started: u64,
}
impl Analyses {
    pub fn busy(&self) -> bool { self.active.is_some() || self.entries.iter().any(|entry| !entry.ready && !entry.failed) }
    pub fn status(&self, layer: OccurrenceHandle) -> Option<MessageId> {
        self.entries.iter().find(|entry| entry.layer() == layer).and_then(|entry|
            if entry.failed {Some(MessageId::RESOURCES_ANALYSIS_ERROR)} else if !entry.ready {Some(MessageId::RESOURCES_ANALYSIS_UPDATING)} else {None})
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn cancel_effect_analyses(&mut self) {
        self.engine.backend_mut().cancel_effect_analysis();
        self.effect_analyses = Analyses {epoch: self.state.document_file.epoch, ..Default::default()};
    }
    pub(super) fn clear_effect_analyses(&mut self) -> Result<(), String> {
        self.cancel_effect_analyses();
        self.engine.backend_mut().retain_effect_analyses(&[]).map_err(error)
    }
    pub(super) fn poll_effect_analyses(&mut self, now: u64) -> Result<u32, String> {
        if self.rendering_suspended { return Ok(0); }
        if self.effect_analyses.epoch != self.state.document_file.epoch { self.clear_effect_analyses()?; }
        let mut tasks = std::mem::take(&mut self.effect_analyses);
        let document = self.engine.document();
        let time = self.engine.animation_time();
        let scene = document.scene();
        let ids: Vec<_> = scene.order().iter().copied().filter(|handle| scene.visible(*handle)
            && scene.effect(*handle).is_some_and(|effect| effect.program.analysis().is_some())).collect();
        let before = tasks.entries.len();
        tasks.entries.retain(|entry| ids.contains(&entry.layer()));
        let mut changed = before != tasks.entries.len();
        let mut incompatible = changed;
        for id in &ids {
            if let Some(entry) = tasks.entries.iter_mut().find(|entry| entry.layer() == *id) {
                let source_changed = !entry.query.matches_source(document);
                let refresh = entry.animated && entry.ready && (time - entry.query.snapshot.context.elapsed).abs() >= 0.5;
                if source_changed || refresh {
                    incompatible |= !entry.query.matches_source_identity(document);
                    *entry = Entry::new(document, *id, time);
                    changed = true;
                }
            } else { tasks.entries.push(Entry::new(document, *id, time)); changed = true; }
        }
        if tasks.active.as_ref().is_some_and(|query| !query.matches_source(document)
            || !ids.iter().any(|id| query.source == ArtworkSource::EffectInput(*id))) {
            self.engine.backend_mut().cancel_effect_analysis(); tasks.active = None;
        }
        if incompatible {
            let retained: Vec<_> = tasks.entries.iter().filter(|entry| entry.ready).map(Entry::layer).collect();
            if let Err(failure) = self.engine.backend_mut().retain_effect_analyses(&retained) {
                self.effect_analyses = tasks;
                return Err(error(failure));
            }
        }
        if tasks.active.is_some() && self.require_document_snapshot_idle().is_ok() && !self.engine.has_pending_document_edits()
            && let Some(result) = self.engine.backend_mut().take_effect_analysis() {
            let query = tasks.active.take().unwrap();
            if let Some(entry) = tasks.entries.iter_mut().find(|entry| entry.query.source == query.source) {
                match result.and_then(|()| self.engine.backend_mut().accept_effect_analysis()) {
                    Ok(()) => { entry.ready = true; Arc::make_mut(&mut entry.query.snapshot).context = query.snapshot.context.clone(); },
                    Err(error) => { eprintln!("Effect analysis: {error}"); entry.failed = true; self.engine.backend_mut().cancel_effect_analysis(); },
                }
                changed = true;
            } else { self.engine.backend_mut().cancel_effect_analysis(); }
        }
        if tasks.active.is_none() && now.saturating_sub(tasks.started) >= 100_000_000
            && self.require_document_snapshot_idle().is_ok() && !self.engine.has_pending_document_edits()
            && let Some(entry) = tasks.entries.iter_mut().find(|entry| !entry.ready && !entry.failed) {
            let mut query = entry.query.clone(); Arc::make_mut(&mut query.snapshot).context.elapsed = time;
            if query.validate().is_ok() {
                match self.engine.backend_mut().request_effect_analysis(query.clone()) {
                    Ok(true) => { tasks.active = Some(query); tasks.started = now; },
                    Ok(false) => {},
                    Err(error) => { eprintln!("Effect analysis: {error}"); entry.failed = true; changed = true; },
                }
            }
        }
        self.effect_analyses = tasks;
        if changed { self.refresh_document(); }
        Ok(if changed {regions::DOCUMENT} else {0})
    }
}
