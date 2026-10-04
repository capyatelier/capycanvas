//! One window's drawing collection shared by the native hosts: membership,
//! parking, switch/resume validity and open adoption. Hosts own ABI, threading,
//! renderer installation and their platform resets.
use crate::{GpuContext, NativeHost, Renderer, RendererOptions};
use layer_core::color::DocumentColor;
use layer_render_wgpu::WgpuRasterizer;
use layer_ui::{
    CommandId, DocumentLocation, DocumentRequest, DocumentSessions, DocumentTabHit,
    HostRequestKind, UiSession,
};
use serde::Deserialize;
use serde_json::{Value, json};

pub trait Parked {
    fn session(&self) -> &UiSession<Renderer>;
    fn session_mut(&mut self) -> &mut UiSession<Renderer>;
}
impl Parked for UiSession<Renderer> {
    fn session(&self) -> &UiSession<Renderer> {
        self
    }
    fn session_mut(&mut self) -> &mut UiSession<Renderer> {
        self
    }
}
impl Parked for Box<UiSession<Renderer>> {
    fn session(&self) -> &UiSession<Renderer> {
        self
    }
    fn session_mut(&mut self) -> &mut UiSession<Renderer> {
        self
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TabRequest {
    View {
        #[serde(default)]
        width: f32,
    },
    Ready,
    Recovery {
        id: u64,
    },
    Adjacent {
        forward: bool,
    },
    Reorder {
        id: u64,
        before: Option<u64>,
    },
    Step {
        id: u64,
        forward: bool,
    },
    History {
        redo: bool,
    },
    Drop {
        hits: Vec<DocumentTabHit>,
        point: [f32; 2],
        vertical: bool,
    },
    Slide {
        id: u64,
        hits: Vec<DocumentTabHit>,
        clip: layer_ui::Bounds,
        press: [f32; 2],
        point: [f32; 2],
    },
    Storage {
        error: Option<String>,
    },
    ResetClose,
}

/// Worker half of a drawing switch: drops the retired renderer and builds the
/// selected drawing's renderer on the window device.
pub struct Activation {
    selected: u64,
    epoch: u64,
    gpu: Option<GpuContext>,
    color: DocumentColor,
    options: RendererOptions,
    retired: Option<Box<WgpuRasterizer>>,
    renderer: Option<Box<WgpuRasterizer>>,
}
impl Activation {
    pub fn selected(&self) -> u64 {
        self.selected
    }
    pub fn work(&mut self) -> Result<(), String> {
        drop(self.retired.take());
        if self.selected == 0 {
            return Ok(());
        }
        let gpu = self
            .gpu
            .as_ref()
            .ok_or("The window GPU is unavailable; restart the canvas")?;
        self.renderer = Some(gpu.rasterizer(self.color, &self.options, true)?.into());
        Ok(())
    }
    pub fn take_renderer(&mut self) -> Option<Box<WgpuRasterizer>> {
        self.renderer.take()
    }
}

pub struct OpenAdoption {
    pub epoch: u64,
    pub revision: u64,
    pub location: Option<DocumentLocation>,
}

pub type SessionIdentityMap = Vec<(u64, u64)>;

pub struct PreparedClose {
    suspended: bool,
    options: RendererOptions,
    selected: u64,
    order: Vec<u64>,
    source: CloseDocumentFence,
    target: Option<(u64, CloseDocumentFence)>,
}

#[derive(PartialEq)]
struct CloseDocumentFence {
    owner: u64,
    artwork: layer_core::authored::PortableId,
    epoch: u64,
    revision: u64,
    working: u64,
    checkpoint: u64,
    saved_checkpoint: u64,
    unpublished: bool,
    location: Option<DocumentLocation>,
    unsaved_name: Option<String>,
    recovered: bool,
    destination: Option<layer_ui::DestinationFingerprint>,
    last_export: Option<layer_ui::session_recovery::SessionExport>,
}
impl CloseDocumentFence {
    fn capture(session: &UiSession<Renderer>) -> Self {
        let document = session.engine().document();
        let layer_ui::SessionDocumentState {camera:_,location,unsaved_name,saved_checkpoint,unpublished,recovered,destination,last_export} = session.session_stamp().state;
        Self {owner:document.owner,artwork:document.artwork.id,epoch:session.state().document_file.epoch,
            revision:document.revision,working:document.working.generation,checkpoint:session.engine().checkpoint(),
            saved_checkpoint,unpublished,location,unsaved_name,recovered,destination,last_export}
    }
}

pub struct DocumentWindow<P> {
    pub documents: DocumentSessions<P>,
    pub gpu: Option<GpuContext>,
}
impl<P> Default for DocumentWindow<P> {
    fn default() -> Self {
        Self::localized(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English))
    }
}
impl<P> DocumentWindow<P> {
    pub fn localized(localization: &layer_ui::Localizer) -> Self {
        Self { documents: DocumentSessions::localized(localization), gpu: None }
    }
    pub fn set_localization(&mut self, localization: std::sync::Arc<layer_ui::Localizer>) -> bool {
        self.documents.set_localization(&localization)
    }
}

impl<P: Parked> DocumentWindow<P> {
    pub fn session<'a>(
        &'a self,
        host: &'a NativeHost,
        id: u64,
    ) -> Result<&'a UiSession<Renderer>, String> {
        if id == self.documents.selected() {
            Ok(&host.session)
        } else {
            self.documents
                .parked()
                .find(|(key, _)| **key == id)
                .map(|(_, p)| p.owner.session())
                .ok_or_else(|| layer_ui::DocumentSessionError::TabClosed.message(host.session.localization()))
        }
    }

    pub fn park_ready(&self, host: &NativeHost) -> Result<bool, String> {
        let session = &host.session;
        Ok(session.can_park_document()
            && (session.rendering_suspended()
                || session.state().document_file.close_ready
                || session.retained_document_tiles().try_blobs()?.is_some()))
    }

    pub fn adoption_ready(&self, host: &NativeHost) -> Result<bool, String> {
        Ok(!host.document_close_prepared && host.session.retained_document_tiles().try_blobs()?.is_some())
    }

    /// Takes the active renderer and retains its device for later activations.
    pub fn retire_gpu(&mut self, host: &mut NativeHost) -> Option<Box<WgpuRasterizer>> {
        let renderer = host.session.renderer_mut().0.take();
        if let Some(gpu) = &renderer {
            self.gpu = Some(GpuContext::of(gpu));
        }
        renderer
    }

    pub fn changed(&self, host: &mut NativeHost) {
        host.document_count = self.documents.order().len();
        host.document_adopted();
        host.invalidate_snapshot();
    }

    pub fn view(&self, host: &NativeHost, width: f32) -> Value {
        let documents = &self.documents;
        json!({
            "tabs": documents.labels(&host.session.state().document_file, |p| &p.session().state().document_file, host.session.localization()),
            "selected": documents.selected(),
            "compact": layer_ui::DocumentTabs::compact(width, documents.order().len()),
            "can_undo": documents.can_undo(),
            "can_redo": documents.can_redo(),
            "resident_bytes": documents.resident_bytes(),
            "storage_error": documents.storage_error(),
            "parked_renderers": documents.parked().filter(|(_, p)| p.owner.session().engine().backend().0.is_some()).count(),
            "session_stamps": documents.order().iter().filter_map(|id| self.session(host, *id).ok().map(|session| json!({"id": id, "stamp": session.session_stamp()}))).collect::<Vec<_>>(),
        })
    }

    pub fn request(&mut self, host: &mut NativeHost, request: TabRequest) -> Result<Value, String> {
        if !matches!(&request, TabRequest::View { .. } | TabRequest::Ready | TabRequest::Recovery { .. } | TabRequest::Storage { .. }) {
            host.require_document_owner()?;
        }
        Ok(match request {
            TabRequest::View { width } => self.view(host, width),
            TabRequest::Ready => json!({
                "available": host.session.can_park_document(),
                "park": self.park_ready(host)?,
                "close": host.session.command(CommandId::CloseDocument).enabled,
                "approved": host.session.state().document_file.close_ready,
            }),
            TabRequest::Recovery { id } => {
                let s = self.session(host, id)?;
                let mut document = s.recovery_document();
                if !s.state().requests.is_empty()
                    && s.state().requests.iter().all(|r| opening(&r.kind))
                {
                    document.busy = s.capture_artwork().is_err();
                }
                json!(document)
            }
            TabRequest::Adjacent { forward } => json!(self.documents.adjacent(forward)),
            TabRequest::Drop {
                hits,
                point,
                vertical,
            } => json!(
                self.documents
                    .drop_target(&hits, point, vertical)
                    .map(|before| json!({"before": before}))
            ),
            TabRequest::Slide {
                id,
                hits,
                clip,
                press,
                point,
            } => json!(
                self.documents
                    .drag(id, press, &hits, clip)
                    .and_then(|drag| drag.preview(point))
            ),
            TabRequest::Storage { error } => {
                self.documents.storage_completed(error.map_or(Ok(()), Err));
                Value::Null
            }
            TabRequest::ResetClose => {
                host.session.reset_document_close();
                for (_, parked) in self.documents.parked_mut() {
                    parked.owner.session_mut().reset_document_close();
                }
                self.changed(host);
                Value::Null
            }
            TabRequest::Reorder { id, before } => self.arrange(host, |d| {
                d.reorder(id, before);
            })?,
            TabRequest::Step { id, forward } => self.arrange(host, |d| {
                if let Some(before) = d.step(id, forward) {
                    d.reorder(id, before);
                }
            })?,
            TabRequest::History { redo } => {
                self.arrange(host, |d| if redo { d.redo() } else { d.undo() })?
            }
        })
    }

    fn arrange(
        &mut self,
        host: &mut NativeHost,
        change: impl FnOnce(&mut DocumentSessions<P>),
    ) -> Result<Value, String> {
        if !host.session.can_park_document() {
            return Err("Finish the current operation before reordering drawings".into());
        }
        change(&mut self.documents);
        self.changed(host);
        Ok(Value::Null)
    }

    /// Parks the active drawing and selects `id` (or the drawing after the
    /// approved close). `exchange` receives the incoming parked owner after the
    /// session swap. Returns `None` when `id` is already selected, otherwise the
    /// worker activation and, when closing, the closed owner.
    pub fn switch(
        &mut self,
        host: &mut NativeHost,
        id: u64,
        closing: bool,
        options: RendererOptions,
        exchange: impl FnOnce(&mut P),
    ) -> Result<Option<(Activation, Option<P>)>, String> {
        host.require_document_owner()?;
        if closing {
            let prepared = self.prepare_close(host, options)?;
            return Ok(Some(self.commit_close(host, prepared, exchange)));
        }
        let target = id;
        if target == self.documents.selected() {
            return Ok(None);
        }
        if !self.documents.contains_parked(target) {
            return Err(layer_ui::DocumentSessionError::TabClosed.message(host.session.localization()));
        }
        if !host.session.can_park_document() {
            return Err(layer_ui::DocumentTransportRefusal::SwitchOperation.message(host.session.localization()).to_string());
        }
        if !self.park_ready(host)? {
            return Err("Wait for drawing capture before switching drawings".into());
        }
        if target != 0 {
            self.documents
                .parked_owner_mut(target)
                .ok_or_else(|| layer_ui::DocumentSessionError::TabClosed.message(host.session.localization()))?
                .session_mut()
                .inherit_window_state(&host.session)?;
        }
        let tiles = host.session.park_document()?;
        let retired = self.retire_gpu(host);
        self.documents.exchange_with(target, tiles, |next| {
            std::mem::swap(&mut host.session, next.session_mut());
            exchange(next);
        }).map_err(|reason| reason.message(host.session.localization()))?;
        self.changed(host);
        host.startup = Default::default();
        let activation = Activation {
            selected: self.documents.selected(),
            epoch: host.session.state().document_file.epoch,
            gpu: self.gpu.clone(),
            color: host.session.engine().document().composition().color,
            options,
            retired,
            renderer: None,
        };
        Ok(Some((activation, None)))
    }

    pub fn prepare_close(&mut self, host: &mut NativeHost, options: RendererOptions) -> Result<PreparedClose, String> {
        host.require_document_owner()?;
        if !host.session.state().document_file.close_ready {
            return Err("Confirm closing the drawing first".into());
        }
        if !host.session.can_park_document() || !self.park_ready(host)? {
            return Err(layer_ui::DocumentTransportRefusal::CloseOperation.message(host.session.localization()).to_string());
        }
        if let Some(target) = self.documents.after_close() {
            self.documents.parked_owner_mut(target)
                .ok_or_else(|| layer_ui::DocumentSessionError::TabClosed.message(host.session.localization()))?
                .session_mut().inherit_window_state(&host.session)?;
        }
        let suspended = host.session.rendering_suspended();
        host.session.park_document()?;
        host.document_close_prepared = true;
        host.invalidate_snapshot();
        let target = self.documents.after_close().map(|id| (id, CloseDocumentFence::capture(self.session(host, id).unwrap())));
        Ok(PreparedClose {suspended,options,selected:self.documents.selected(),order:self.documents.order().to_vec(),
            source:CloseDocumentFence::capture(&host.session),target})
    }

    fn close_is_current(&self, host: &NativeHost, prepared: &PreparedClose) -> bool {
        host.document_close_prepared && host.session.rendering_suspended() && host.session.state().document_file.close_ready
            && self.documents.selected() == prepared.selected && self.documents.order() == prepared.order
            && CloseDocumentFence::capture(&host.session) == prepared.source
            && prepared.target.as_ref().is_none_or(|(id, fence)| self.session(host, *id).is_ok_and(|session| CloseDocumentFence::capture(session) == *fence))
    }

    pub fn validate_close(&self, host: &NativeHost, prepared: &PreparedClose) -> Result<(), String> {
        if !self.close_is_current(host, prepared) {
            return Err(layer_ui::DocumentTransportRefusal::SnapshotChanged.message(host.session.localization()).to_string());
        }
        Ok(())
    }

    pub fn cancel_close(&mut self, host: &mut NativeHost, prepared: PreparedClose) -> Result<(), String> {
        if !host.document_close_prepared || self.documents.selected() != prepared.selected
            || host.session.engine().document().owner != prepared.source.owner
            || host.session.engine().document().artwork.id != prepared.source.artwork
        {
            return Err(layer_ui::DocumentTransportRefusal::SnapshotChanged.message(host.session.localization()).to_string());
        }
        host.document_close_prepared = false;
        if !prepared.suspended {
            let previous = host.session.state().revision;
            let change = host.session.cancel_document_park()?;
            host.apply_change(previous, change);
        }
        host.session.reset_document_close();
        self.changed(host);
        Ok(())
    }

    pub fn commit_close(&mut self, host: &mut NativeHost, prepared: PreparedClose, exchange: impl FnOnce(&mut P)) -> (Activation, Option<P>) {
        assert!(self.close_is_current(host, &prepared), "Prepared close no longer targets this window");
        let retired = self.retire_gpu(host);
        let closed = self.documents.close_selected().map(|mut next| {
            next.session_mut().inherit_parked_viewport(&host.session);
            std::mem::swap(&mut host.session, next.session_mut());
            exchange(&mut next);
            next
        });
        host.document_close_prepared = false;
        self.changed(host);
        host.startup = Default::default();
        (Activation {selected:self.documents.selected(),epoch:host.session.state().document_file.epoch,
            gpu:self.gpu.clone(),color:host.session.engine().document().composition().color,
            options:prepared.options,retired,renderer:None}, closed)
    }

    /// Checks that the activation still targets the selected drawing, its
    /// activation epoch and the window device, and takes the renderer to
    /// install. `None` means the window has no drawing left to activate.
    pub fn resume(
        &self,
        host: &NativeHost,
        job: &mut Activation,
    ) -> Result<Option<Box<WgpuRasterizer>>, String> {
        if job.selected != self.documents.selected()
            || job.epoch != host.session.state().document_file.epoch
            || job.gpu.as_ref().map(|g| &g.device) != self.gpu.as_ref().map(|g| &g.device)
            || host.session.engine().backend().0.is_some()
        {
            return Err("Drawing activation is no longer current".into());
        }
        if job.selected == 0 {
            return Ok(None);
        }
        job.renderer
            .take()
            .map(Some)
            .ok_or_else(|| "Drawing renderer is not prepared".into())
    }

    /// Publishes a prepared drawing as a new tab after its Open/New request
    /// completes. The candidate is taken only on success; the retired window
    /// renderer is returned for destruction off the owner.
    pub fn adopt(
        &mut self,
        host: &mut NativeHost,
        candidate: &mut Option<Box<UiSession<Renderer>>>,
        open: OpenAdoption,
        begin_commit: impl FnOnce() -> bool,
        park: impl FnOnce(UiSession<Renderer>) -> P,
    ) -> Result<Option<Box<WgpuRasterizer>>, String> {
        self.adopt_prepared(host, candidate, open, false, begin_commit, park)
    }

    pub fn adopt_session(
        &mut self,
        host: &mut NativeHost,
        candidate: &mut Option<Box<UiSession<Renderer>>>,
        open: OpenAdoption,
        begin_commit: impl FnOnce() -> bool,
        park: impl FnOnce(UiSession<Renderer>) -> P,
    ) -> Result<Option<Box<WgpuRasterizer>>, String> {
        self.adopt_prepared(host, candidate, open, true, begin_commit, park)
    }

    pub fn restore_sessions(
        &mut self,
        host: &mut NativeHost,
        candidates: &mut Vec<(u64, Box<UiSession<Renderer>>)>,
        selected: u64,
        stamp: layer_ui::SessionStamp,
        mut park: impl FnMut(UiSession<Renderer>) -> P,
    ) -> Result<Vec<Box<WgpuRasterizer>>, String> {
        host.require_document_owner()?;
        if self.documents.order().len() != 1 || !host.session.can_replace_startup_session(&stamp) {
            return Err(layer_ui::DocumentTransportRefusal::SnapshotChanged.message(host.session.localization()).to_string());
        }
        let order: Vec<_> = candidates.iter().map(|(id, _)| *id).collect();
        let unique: std::collections::BTreeSet<_> = order.iter().copied().collect();
        if order.is_empty() || unique.len() != order.len() || unique.contains(&0)
            || unique.contains(&u64::MAX) || !unique.contains(&selected)
        {
            return Err("The restored drawing membership is invalid".into());
        }
        let mut identities = DocumentSessions::<()>::localized(host.session.localization());
        for id in &order { identities.restore_identity(*id)?; }
        let mut metadata = 0usize;
        for (id, candidate) in candidates.iter() {
            if candidate.engine().backend().0.is_some() {
                if device(candidate) != device(&host.session) {
                    return Err(layer_ui::DocumentTransportRefusal::RecoveryGpuChanged.message(host.session.localization()).to_string());
                }
            } else if *id == selected || !candidate.rendering_suspended() {
                return Err(layer_ui::DocumentTransportRefusal::RecoveryGpuChanged.message(host.session.localization()).to_string());
            }
            let tiles = candidate.retained_document_tiles();
            if !candidate.can_park_document() || tiles.try_blobs()?.is_none() {
                return Err(layer_ui::DocumentTransportRefusal::RestoreOperation.message(host.session.localization()).to_string());
            }
            metadata = metadata.saturating_add(tiles.metadata_bytes);
        }
        if metadata > self.documents.budget.metadata {
            return Err(layer_ui::DocumentSessionError::MetadataBudgetExceeded.message(host.session.localization()));
        }
        for (id, candidate) in candidates.iter_mut() {
            candidate.set_document_replacement(false);
            if *id != selected { candidate.park_document()?; }
            candidate.inherit_window_state(&host.session)?;
        }
        let active = candidates.iter().position(|(id, _)| *id == selected).unwrap();
        let selected_candidate = candidates.remove(active);
        candidates.push(selected_candidate);
        let mut restored = DocumentSessions::localized(host.session.localization());
        restored.budget = self.documents.budget;
        let mut retired = Vec::new();
        let mut iter = candidates.drain(..);
        let (id, mut current) = iter.next().unwrap();
        restored.restore_identity(id).expect("validated restored identity");
        for (id, incoming) in iter {
            let tiles = current.retained_document_tiles();
            if let Some(renderer) = current.renderer_mut().0.take() { retired.push(renderer); }
            restored.append(park(*current), tiles, host.session.localization());
            restored.restore_identity(id).expect("validated restored identity");
            current = incoming;
        }
        restored.restore_order(&order, selected).expect("validated restored membership");
        if let Some(renderer) = self.retire_gpu(host) { retired.push(renderer); }
        let outgoing = std::mem::replace(&mut host.session, *current);
        self.documents = restored;
        self.changed(host);
        drop(outgoing);
        Ok(retired)
    }

    pub fn append_restored_sessions(
        &mut self,
        host: &mut NativeHost,
        candidates: &mut Vec<(u64, Box<UiSession<Renderer>>)>,
        mut park: impl FnMut(UiSession<Renderer>) -> P,
    ) -> Result<(SessionIdentityMap, Vec<Box<WgpuRasterizer>>), String> {
        host.require_document_owner()?;
        if !host.session.can_park_document() || self.documents.selected() == 0 {
            return Err(layer_ui::DocumentTransportRefusal::RestoreOperation.message(host.session.localization()).to_string());
        }
        let mut metadata = host.session.retained_document_tiles().metadata_bytes;
        for (_, existing) in self.documents.parked() { metadata = metadata.saturating_add(existing.tiles.metadata_bytes); }
        let mut identities = DocumentSessions::<()>::localized(host.session.localization());
        let mut unique = std::collections::BTreeSet::new();
        for (id, candidate) in candidates.iter() {
            identities.restore_identity(*id)?;
            if !unique.insert(*id) { return Err("The restored drawing membership is invalid".into()); }
            if device(candidate) != device(&host.session) {
                return Err(layer_ui::DocumentTransportRefusal::RecoveryGpuChanged.message(host.session.localization()).to_string());
            }
            let tiles = candidate.retained_document_tiles();
            if !candidate.can_park_document() || tiles.try_blobs()?.is_none() {
                return Err(layer_ui::DocumentTransportRefusal::RestoreOperation.message(host.session.localization()).to_string());
            }
            self.documents.admit(&host.session.retained_document_tiles(), candidate.engine().document())
                .map_err(|reason| reason.message(host.session.localization()))?;
            metadata = metadata.saturating_add(tiles.metadata_bytes);
        }
        if metadata > self.documents.budget.metadata {
            return Err(layer_ui::DocumentSessionError::MetadataBudgetExceeded.message(host.session.localization()));
        }
        for (_, candidate) in candidates.iter_mut() {
            candidate.set_document_replacement(false);
            candidate.park_document()?;
            candidate.inherit_window_state(&host.session)?;
        }
        let mut mapping = Vec::with_capacity(candidates.len());
        let mut retired = Vec::with_capacity(candidates.len());
        for (old, mut candidate) in candidates.drain(..) {
            let tiles = candidate.retained_document_tiles();
            if let Some(renderer) = candidate.renderer_mut().0.take() { retired.push(renderer); }
            let id = self.documents.append_parked(park(*candidate), tiles, host.session.localization());
            mapping.push((old, id));
        }
        self.changed(host);
        Ok((mapping, retired))
    }

    pub fn hydrate_restored(
        &mut self,
        host: &mut NativeHost,
        candidate: &mut Option<Box<UiSession<Renderer>>>,
        id: u64,
        park: impl FnOnce(UiSession<Renderer>) -> P,
    ) -> Result<(u64, Option<Box<WgpuRasterizer>>), String> {
        host.require_document_owner()?;
        let next = candidate.as_mut().ok_or("Project preparation is incomplete")?;
        let mut identities = DocumentSessions::<()>::localized(host.session.localization());
        identities.restore_identity(id)?;
        if self.documents.selected() == 0 {
            return Err(layer_ui::DocumentTransportRefusal::RestoreOperation.message(host.session.localization()).to_string());
        }
        if next.engine().backend().0.is_some() {
            if device(next) != device(&host.session) {
                return Err(layer_ui::DocumentTransportRefusal::RecoveryGpuChanged.message(host.session.localization()).to_string());
            }
        } else if !next.rendering_suspended() {
            return Err(layer_ui::DocumentTransportRefusal::RestoreOperation.message(host.session.localization()).to_string());
        }
        let tiles = next.retained_document_tiles();
        if !next.can_park_document() || tiles.try_blobs()?.is_none() {
            return Err(layer_ui::DocumentTransportRefusal::RestoreOperation.message(host.session.localization()).to_string());
        }
        self.documents.admit(&host.session.retained_document_tiles(), next.engine().document())
            .map_err(|reason| reason.message(host.session.localization()))?;
        let metadata = self.documents.parked().fold(host.session.retained_document_tiles().metadata_bytes, |n, (_, parked)| n.saturating_add(parked.tiles.metadata_bytes));
        if metadata.saturating_add(tiles.metadata_bytes) > self.documents.budget.metadata {
            return Err(layer_ui::DocumentSessionError::MetadataBudgetExceeded.message(host.session.localization()));
        }
        next.set_document_replacement(false);
        next.park_document()?;
        next.inherit_window_state(&host.session)?;
        let mut next = candidate.take().unwrap();
        let retired = next.renderer_mut().0.take();
        let id = if self.documents.order().contains(&id) {
            self.documents.append_parked(park(*next), tiles, host.session.localization())
        } else {
            self.documents.append_parked_with_id(id, park(*next), tiles, host.session.localization())
                .unwrap_or_else(|_| unreachable!("validated restored identity"));
            id
        };
        host.document_count = self.documents.order().len();
        host.invalidate_snapshot();
        host.dirty = true;
        Ok((id, retired))
    }

    fn adopt_prepared(
        &mut self,
        host: &mut NativeHost,
        candidate: &mut Option<Box<UiSession<Renderer>>>,
        open: OpenAdoption,
        restored: bool,
        begin_commit: impl FnOnce() -> bool,
        park: impl FnOnce(UiSession<Renderer>) -> P,
    ) -> Result<Option<Box<WgpuRasterizer>>, String> {
        host.require_document_owner()?;
        let next = candidate
            .as_mut()
            .ok_or("Project preparation is incomplete")?;
        if device(next) != device(&host.session) {
            return Err("The canvas changed while preparing this drawing; open it again".into());
        }
        if host.session.state().document_file.epoch != open.epoch
            || host.session.engine().document().revision != open.revision
        {
            return Err("The drawing changed while opening; try again".into());
        }
        let tiles = host.session.retained_document_tiles();
        if tiles.try_blobs()?.is_none() {
            return Err("Wait for drawing capture before opening".into());
        }
        self.documents
            .admit(&tiles, next.engine().document()).map_err(|reason| reason.message(host.session.localization()))?;
        if !restored {
            next.initialize_document_location(open.location)?;
        }
        next.set_document_replacement(false);
        next.inherit_window_state(&host.session)?;
        if !restored {
            next.inherit_initial_drawing_tools(&host.session)?;
        }
        if !begin_commit() {
            return Err("Document operation cancelled".into());
        }
        let requests: Vec<_> = host
            .session
            .state()
            .requests
            .iter()
            .filter_map(|r| opening(&r.kind).then_some(r.id))
            .collect();
        for id in requests {
            host.session.complete_document_request(id, Ok(true))?;
        }
        let tiles = host.session.park_document()?;
        let retired = self.retire_gpu(host);
        let outgoing = std::mem::replace(&mut host.session, *candidate.take().unwrap());
        self.documents.append(park(outgoing), tiles, host.session.localization());
        self.changed(host);
        Ok(retired)
    }
}

fn device(session: &UiSession<Renderer>) -> Option<&wgpu::Device> {
    session.engine().backend().0.as_ref().map(|g| g.device())
}

fn opening(kind: &HostRequestKind) -> bool {
    matches!(
        kind,
        HostRequestKind::Document {
            request: DocumentRequest::New | DocumentRequest::Open
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open::OpenEnvironment;
    use layer_ui::{LayerAction, UiAction, UiInput};
    use std::time::{Duration, Instant};

    type Window = DocumentWindow<UiSession<Renderer>>;

    fn host() -> NativeHost {
        let document = layer_core::Document::new(layer_core::authored::PortableId::random(), 64, 48, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let gpu = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        let mut host = NativeHost::new(layer_ui::Platform::Mac).unwrap();
        host.session = UiSession::new(Renderer(Some(gpu.into())), document, [64, 48], layer_ui::Platform::Mac).unwrap();
        host.session.set_document_replacement(false);
        settle(&mut host);
        host
    }

    fn settle(host: &mut NativeHost) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            host.prepare_canvas_frame(0, 0, true).unwrap();
            let gpu = host.session.engine().backend().0.as_ref().unwrap();
            gpu.device().poll(wgpu::PollType::Poll).unwrap();
            if host.session.can_park_document()
                && host
                    .session
                    .retained_document_tiles()
                    .try_blobs()
                    .unwrap()
                    .is_some()
            {
                return;
            }
            assert!(Instant::now() < deadline, "drawing did not settle");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn invoke(host: &mut NativeHost, command: CommandId) {
        host.dispatch(UiAction::Invoke { command }).unwrap();
    }

    fn fill(host: &mut NativeHost) {
        invoke(host, CommandId::SelectAll);
        host.dispatch(UiAction::Layer {
            action: LayerAction::FillSelection,
        })
        .unwrap();
        invoke(host, CommandId::Deselect);
        settle(host);
    }

    fn opened(host: &NativeHost) -> OpenAdoption {
        OpenAdoption {
            epoch: host.session.state().document_file.epoch,
            revision: host.session.engine().document().revision,
            location: None,
        }
    }

    fn candidate(window: &Window, host: &NativeHost) -> Option<Box<UiSession<Renderer>>> {
        let admission = window
            .documents
            .admission(&host.session.retained_document_tiles());
        let environment =
            OpenEnvironment::capture(&host.session, admission, Default::default()).unwrap();
        let project = layer_ui::NewDocumentOptions::default().project(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        Some(environment.prepare(project, || false).unwrap())
    }

    fn settle_candidate(candidate: &mut UiSession<Renderer>) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            candidate.frame(0, 0).unwrap();
            candidate.engine().backend().0.as_ref().unwrap().device().poll(wgpu::PollType::Poll).unwrap();
            if candidate.can_park_document() && candidate.retained_document_tiles().try_blobs().unwrap().is_some() { return; }
            assert!(Instant::now() < deadline, "restored drawing did not settle");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn open(window: &mut Window, host: &mut NativeHost) {
        let mut next = candidate(window, host);
        let open = opened(host);
        drop(window.adopt(host, &mut next, open, || true, |s| s).unwrap());
        settle(host);
    }

    fn switch(window: &mut Window, host: &mut NativeHost, id: u64) -> Activation {
        let (activation, closed) = window
            .switch(host, id, false, Default::default(), |_| {})
            .unwrap()
            .unwrap();
        assert!(closed.is_none());
        activation
    }

    fn resume(window: &mut Window, host: &mut NativeHost, activation: &mut Activation) {
        let gpu = window.resume(host, activation).unwrap().unwrap();
        host.session.replace_renderer(Renderer(Some(gpu))).unwrap();
        window.changed(host);
        settle(host);
    }

    fn finish(host: NativeHost, window: Window) {
        drop((host, window));
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }

    fn digests(host: &NativeHost) -> Vec<[u8; 32]> {
        let tiles = host.session.retained_document_tiles();
        let mut digests: Vec<_> = tiles.blobs().unwrap().iter().map(|b| b.content_digest().unwrap()).collect();
        digests.sort();
        digests
    }

    #[test]
    fn parked_spill_keeps_exact_redo_pixels() {
        let mut host = host();
        let mut window = Window::default();
        fill(&mut host);
        let exact = digests(&host);
        invoke(&mut host, CommandId::Undo);
        settle(&mut host);
        open(&mut window, &mut host);
        assert_eq!(window.documents.order(), [1, 2]);
        window.documents.budget.inactive_ram = 0;
        let directory = std::env::temp_dir().join(format!("capy-window-{}", std::process::id()));
        while let Some(tiles) = window.documents.spill_candidate() {
            layer_core::raster_storage::spill_to_directory(&tiles, &directory).unwrap();
        }
        assert_eq!(window.documents.resident_bytes(), 0);
        let mut activation = switch(&mut window, &mut host, 1);
        activation.work().unwrap();
        resume(&mut window, &mut host, &mut activation);
        invoke(&mut host, CommandId::Redo);
        settle(&mut host);
        assert_eq!(digests(&host), exact);
        let _ = std::fs::remove_dir_all(directory);
        finish(host, window);
    }

    #[test]
    fn switch_guards_parking_and_resume_fences_stale_activations() {
        let mut host = host();
        let mut window = Window::default();
        open(&mut window, &mut host);
        let epoch = |w: &mut Window| {
            w.documents
                .parked_owner_mut(1)
                .unwrap()
                .state()
                .document_file
                .epoch
        };
        let before = epoch(&mut window);
        invoke(&mut host, CommandId::DocumentProperties);
        assert_eq!(
            window
                .switch(&mut host, 1, false, Default::default(), |_| {})
                .err()
                .as_deref(),
            Some(layer_ui::DocumentTransportRefusal::SwitchOperation.message(host.session.localization()).as_ref())
        );
        assert_eq!(epoch(&mut window), before);
        let id = host.session.state().requests[0].id;
        host.session
            .complete_document_request(id, Ok(true))
            .unwrap();
        settle(&mut host);
        let mut stale = switch(&mut window, &mut host, 1);
        stale.work().unwrap();
        let mut current = switch(&mut window, &mut host, 2);
        assert!(window.resume(&host, &mut stale).is_err());
        current.work().unwrap();
        let context = window.gpu.take();
        assert!(window.resume(&host, &mut current).is_err());
        window.gpu = context;
        resume(&mut window, &mut host, &mut current);
        assert_eq!(window.documents.selected(), 2);
        drop((stale, current));
        finish(host, window);
    }

    #[test]
    fn adopt_inherits_window_state_and_tools() {
        let mut host = host();
        let mut window = Window::default();
        let red: UiAction =
            serde_json::from_value(json!({"type": "color", "action": {"op": "set_slot",
            "slot": "foreground", "color": {"space": "Srgb", "rgba": [0.9, 0.1, 0.1, 1.0]}}}))
            .unwrap();
        host.dispatch(red).unwrap();
        settle(&mut host);
        let brush = host.session.engine().configured_brush().color_rgba_linear;
        invoke(&mut host, CommandId::NewDocument);
        let mut next = candidate(&window, &host);
        let open = opened(&host);
        assert!(
            window
                .adopt(&mut host, &mut next, open, || false, |s| s)
                .is_err()
        );
        assert!(next.is_some());
        let open = opened(&host);
        window
            .adopt(&mut host, &mut next, open, || true, |s| s)
            .unwrap();
        assert!(next.is_none());
        assert!(host.session.state().requests.is_empty());
        assert_eq!(window.documents.order(), [1, 2]);
        assert_eq!(host.document_count, 2);
        assert_eq!(
            host.session.engine().configured_brush().color_rgba_linear,
            brush
        );
        finish(host, window);
    }

    #[test]
    fn session_adoption_preserves_destination_camera_and_edit_history() {
        let mut host = host();
        let mut window = Window::default();
        let mut next = candidate(&window, &host);
        let session = next.as_mut().unwrap();
        let location = DocumentLocation { uri: "private:test-drawing".into(), name: "Drawing.capy".into() };
        session.initialize_document_location(Some(location.clone())).unwrap();
        let count = session.engine().document().scene().order().len();
        session.dispatch(UiAction::Invoke { command: CommandId::AddLayer }).unwrap();
        session.dispatch(UiAction::SetZoom { zoom: 1.25 }).unwrap();
        session.dispatch(UiAction::SetRotation { rotation: 0.4 }).unwrap();
        let rotation = session.state().camera.rotation;
        let brush = session.engine().configured_brush().clone();
        let checkpoint = session.engine().checkpoint();
        let open = opened(&host);
        drop(window.adopt_session(&mut host, &mut next, open, || true, |s| s).unwrap());
        assert!(next.is_none());
        assert_eq!(host.session.state().document_file.location, Some(location));
        assert!(host.session.state().document_file.modified);
        assert_eq!(host.session.state().camera.zoom, 1.25);
        assert_eq!(host.session.state().camera.rotation, rotation);
        assert_eq!(host.session.engine().configured_brush(), &brush);
        assert_eq!(host.session.engine().checkpoint(), checkpoint);
        settle(&mut host);
        invoke(&mut host, CommandId::Undo);
        assert_eq!(host.session.engine().document().scene().order().len(), count);
        assert!(!host.session.state().document_file.modified);
        finish(host, window);
    }

    #[test]
    fn batch_restore_preserves_identity_order_and_rejects_changed_startup() {
        let mut host = host();
        let mut window = Window::default();
        host.session.set_document_replacement(true);
        let stamp = host.session.session_stamp();
        let mut first = candidate(&window, &host).unwrap();
        let mut second = candidate(&window, &host).unwrap();
        first.dispatch(UiAction::SetZoom { zoom: 1.25 }).unwrap();
        second.dispatch(UiAction::Invoke { command: CommandId::AddLayer }).unwrap();
        for candidate in [&mut first, &mut second] {
            settle_candidate(candidate);
        }
        let mut candidates = vec![(9, first), (3, second)];
        host.dispatch(UiAction::SetZoom { zoom: 1.5 }).unwrap();
        assert!(window.restore_sessions(&mut host, &mut candidates, 9, stamp, |s| s).is_err());
        assert_eq!(candidates.len(), 2);
        assert_eq!(window.documents.order(), [1]);
        assert_eq!(host.session.state().camera.zoom, 1.5);
        settle(&mut host);
        let stamp = host.session.session_stamp();
        let retired = window.restore_sessions(&mut host, &mut candidates, 9, stamp, |s| s).unwrap();
        assert!(candidates.is_empty());
        assert_eq!(window.documents.order(), [9, 3]);
        assert_eq!(window.documents.selected(), 9);
        assert_eq!(host.session.state().camera.zoom, 1.25);
        let parked = window.documents.parked_owner_mut(3).unwrap();
        assert!(parked.rendering_suspended());
        assert!(parked.engine().backend().0.is_none());
        assert!(parked.state().document_file.modified);
        assert!(parked.engine().can_undo());
        assert_eq!(retired.len(), 2);
        drop(retired);
        finish(host, window);
    }

    #[test]
    fn restored_append_preserves_live_edits_selection_and_renderer() {
        let mut host = host();
        let mut window = Window::default();
        host.dispatch(UiAction::Invoke { command: CommandId::AddLayer }).unwrap();
        host.dispatch(UiAction::SetZoom { zoom: 1.5 }).unwrap();
        settle(&mut host);
        let stamp = host.session.session_stamp();
        let document = host.session.engine().document().clone();
        let renderer = std::ptr::from_ref(host.session.engine().backend().0.as_ref().unwrap().as_ref());
        let mut next = candidate(&window, &host).unwrap();
        settle_candidate(&mut next);
        let mut candidates = vec![(1, next)];
        window.documents.budget.metadata = 1;
        assert!(window.append_restored_sessions(&mut host, &mut candidates, |s| s).is_err());
        assert_eq!(candidates.len(), 1);
        assert_eq!(window.documents.order(), [1]);
        window.documents.budget = Default::default();
        let (mapping, retired) = window.append_restored_sessions(&mut host, &mut candidates, |s| s).unwrap();
        assert_eq!(mapping, [(1, 2)]);
        assert_eq!(window.documents.order(), [1, 2]);
        assert_eq!(window.documents.selected(), 1);
        assert_eq!(host.session.session_stamp(), stamp);
        assert_eq!(host.session.engine().document(), &document);
        assert_eq!(std::ptr::from_ref(host.session.engine().backend().0.as_ref().unwrap().as_ref()), renderer);
        assert!(window.documents.parked_owner_mut(2).unwrap().rendering_suspended());
        assert_eq!(retired.len(), 1);
        drop(retired);
        finish(host, window);
    }

    #[test]
    fn parked_hydration_preserves_queued_input_and_reserved_identities() {
        let mut host = host();
        let mut window = Window::default();
        window.documents.reserve_identities(&[1, 3]).unwrap();
        let mut next = candidate(&window, &host).unwrap();
        settle_candidate(&mut next);
        next.park_document().unwrap();
        drop(next.renderer_mut().0.take());
        let mut next = Some(next);
        host.session.pen(layer_engine::PenEvent {
            device_id:1,sequence:1,timestamp_ns:1,view_revision:host.session.state().camera.revision,
            surface_position:layer_core::Point {x:16.,y:16.},pressure:1.,tilt_radians:[0.;2],twist_radians:0.,distance:0.,
            phase:layer_engine::PenPhase::Down,tool:layer_engine::ToolKind::Pen,flags:layer_engine::SampleFlags::NONE,
        }).unwrap();
        assert!(host.session.engine().has_pending_input());
        let (id, retired) = window.hydrate_restored(&mut host, &mut next, 1, |s| s).unwrap();
        assert_eq!(id, 2);
        assert!(retired.is_none());
        assert!(next.is_none());
        assert_eq!(window.documents.selected(), 1);
        assert_eq!(window.documents.order(), [1, 2]);
        assert!(host.session.engine().has_pending_input());
        finish(host, window);
    }

    #[test]
    fn prepared_close_cancel_restores_input_and_commit_changes_membership() {
        let mut host = host();
        let mut window = Window::default();
        open(&mut window, &mut host);
        let before = host.session.engine().document().clone();
        assert!(window.prepare_close(&mut host, Default::default()).is_err());
        assert_eq!(window.documents.order(), [1, 2]);
        assert!(!host.document_close_prepared());
        invoke(&mut host, CommandId::CloseDocument);
        assert!(host.session.state().document_file.close_ready);
        let prepared = window.prepare_close(&mut host, Default::default()).unwrap();
        assert_eq!(window.documents.order(), [1, 2]);
        assert_eq!(window.documents.selected(), 2);
        assert!(host.document_close_prepared());
        assert!(host.dispatch(UiAction::Invoke {command:CommandId::AddLayer}).is_err());
        assert!(window.switch(&mut host, 1, false, Default::default(), |_| {}).is_err());
        window.cancel_close(&mut host, prepared).unwrap();
        assert!(!host.document_close_prepared());
        assert!(!host.session.rendering_suspended());
        assert!(!host.session.state().document_file.close_ready);
        assert_eq!(host.session.engine().document(), &before);
        invoke(&mut host, CommandId::CloseDocument);
        let prepared = window.prepare_close(&mut host, Default::default()).unwrap();
        let (mut activation, closed) = window.commit_close(&mut host, prepared, |_| {});
        assert_eq!(window.documents.order(), [1]);
        assert_eq!(window.documents.selected(), 1);
        assert!(!host.document_close_prepared());
        assert_eq!(closed.as_ref().unwrap().engine().document(), &before);
        drop(closed);
        activation.work().unwrap();
        resume(&mut window, &mut host, &mut activation);
        invoke(&mut host, CommandId::AddLayer);
        finish(host, window);
    }

    #[test]
    fn close_validation_rejects_another_owner_and_changed_file_state() {
        let mut source = host();
        let mut other = host();
        let mut window = Window::default();
        invoke(&mut source, CommandId::CloseDocument);
        let prepared = window.prepare_close(&mut source, Default::default()).unwrap();
        assert!(window.validate_close(&source, &prepared).is_ok());
        assert!(window.validate_close(&other, &prepared).is_err());
        source.session.initialize_document_location(Some(DocumentLocation {uri:"private:changed.capy".into(),name:"Changed.capy".into()})).unwrap();
        assert!(window.validate_close(&source, &prepared).is_err());
        window.cancel_close(&mut source, prepared).unwrap();
        assert!(!source.document_close_prepared());
        assert!(!source.session.rendering_suspended());
        assert_eq!(source.session.state().document_file.location.as_ref().unwrap().name,"Changed.capy");
        assert_eq!(window.documents.order(), [1]);
        drop(other.session.renderer_mut().0.take());
        drop(other);
        finish(source, window);
    }

    #[test]
    fn prepared_close_adopts_resize_received_during_storage_publication() {
        let mut host = host();
        let mut window = Window::default();
        host.resize(800, 600, 1.).unwrap();
        host.dispatch(UiAction::SetZoom {zoom:1.5}).unwrap();
        settle(&mut host);
        open(&mut window, &mut host);
        invoke(&mut host, CommandId::CloseDocument);
        let prepared = window.prepare_close(&mut host, Default::default()).unwrap();
        let target = window.documents.parked_owner_mut(1).unwrap().session_stamp();
        host.resize(1200, 800, 2.).unwrap();
        let source_view = host.session.state().camera.revision;
        window.validate_close(&host, &prepared).unwrap();
        let (mut activation, closed) = window.commit_close(&mut host, prepared, |_| {});
        assert!(host.session.session_stamp().same_editor(&target));
        assert_eq!(host.session.state().camera.viewport, [1200, 800]);
        assert_eq!(host.session.state().camera.zoom, 1.5);
        assert_eq!(host.logical, [600.,400.]);
        assert!(host.session.state().camera.revision > source_view);
        assert!(!host.accepts_pointer_input(source_view));
        let work_area = host.session.layout(host.logical).work_area;
        assert_eq!(host.session.state().camera.work_area,
            [work_area.x*2.,work_area.y*2.,work_area.width*2.,work_area.height*2.]);
        let camera = host.session.state().camera.clone();
        host.resize(1200, 800, 2.).unwrap();
        assert_eq!(host.session.state().camera, camera);
        let view = host.session.engine().view();
        assert_eq!([view.width_px, view.height_px], [1200, 800]);
        drop(closed);
        activation.work().unwrap();
        resume(&mut window, &mut host, &mut activation);
        assert_eq!(host.session.state().camera.viewport, [1200, 800]);
        finish(host, window);
    }

    #[test]
    fn prepared_close_ignores_late_native_input_without_invalidating_commit_or_cancel() {
        let mut host = host();
        let mut window = Window::default();
        open(&mut window, &mut host);
        invoke(&mut host, CommandId::CloseDocument);
        let prepared = window.prepare_close(&mut host, Default::default()).unwrap();
        let stamp = host.session.session_stamp();
        let revision = host.session.state().revision;
        let document = host.session.engine().document().clone();
        let chrome = (host.chrome_hidden, host.keep_zen_button, host.pan_cursor);
        let dirty = host.dirty;
        host.take_service_changes();
        for input in [
            UiInput::Chrome {event:layer_ui::ChromeEvent::Refresh,facts:Default::default(),viewport:[64.,48.]},
            UiInput::Blur,
            UiInput::Key {key:"Space".into(),pressed:false,repeat:false,modifiers:Default::default(),editing:false,divider:None},
            UiInput::Axes {pan:[8.,4.],zoom:2.},
        ] {
            let reply = host.input(input).unwrap();
            assert!(reply.handled);
            assert!(!reply.paint && !reply.cancel_paint && !reply.change.canvas_wake);
            assert_eq!(reply.change.regions, 0);
            assert_eq!(reply.change.revision, revision);
            assert_eq!((reply.chrome_hidden, reply.keep_zen_button, reply.pan_cursor), chrome);
        }
        host.scroll([32.,24.], [8.,4.], 1., true, false).unwrap();
        host.gesture([32.,24.], 2., 0.5).unwrap();
        assert_eq!(host.session.session_stamp(), stamp);
        assert_eq!(host.session.engine().document(), &document);
        assert_eq!(host.dirty, dirty);
        assert_eq!(host.take_service_changes(), 0);
        assert!(host.dispatch(UiAction::Invoke {command:CommandId::AddLayer}).is_err());
        window.validate_close(&host, &prepared).unwrap();
        window.cancel_close(&mut host, prepared).unwrap();
        invoke(&mut host, CommandId::AddLayer);
        settle(&mut host);
        assert_ne!(host.session.engine().document(), &document);
        invoke(&mut host, CommandId::Undo);
        settle(&mut host);
        invoke(&mut host, CommandId::CloseDocument);
        let prepared = window.prepare_close(&mut host, Default::default()).unwrap();
        host.input(UiInput::Chrome {event:layer_ui::ChromeEvent::Refresh,facts:Default::default(),viewport:[64.,48.]}).unwrap();
        let (mut activation, closed) = window.commit_close(&mut host, prepared, |_| {});
        assert!(closed.is_some());
        drop(closed);
        activation.work().unwrap();
        resume(&mut window, &mut host, &mut activation);
        invoke(&mut host, CommandId::AddLayer);
        finish(host, window);
    }

    #[test]
    fn language_adoption_keeps_open_candidates_save_completions_and_parked_documents() {
        use layer_ui::{Localizer, UiLanguage};
        let mut host = host();
        let mut window = Window::default();
        let mut next = candidate(&window, &host);
        let candidate_document = next.as_ref().unwrap().engine().document().clone();
        let open = opened(&host);
        let epoch = host.session.state().document_file.epoch;
        let japanese = Localizer::shared(UiLanguage::Japanese);
        assert!(host.set_localization(japanese.clone()));
        assert!(window.set_localization(japanese.clone()));
        assert_eq!(host.session.state().document_file.epoch, epoch);
        drop(window.adopt(&mut host, &mut next, open, || true, |s| s).unwrap());
        assert_eq!(host.session.engine().document().artwork, candidate_document.artwork);
        assert_eq!(host.session.localization().language(), UiLanguage::Japanese);
        assert_eq!(host.localization_generation(), 1);
        settle(&mut host);
        invoke(&mut host, CommandId::SaveDocument);
        let request = host.session.state().requests[0].id;
        let location = DocumentLocation { uri: "save-result".into(), name: "Untitled.capy".into() };
        host.session.capture_project_save(request, location.clone()).unwrap();
        let epoch = host.session.state().document_file.epoch;
        let korean = Localizer::shared(UiLanguage::Korean);
        assert!(host.set_localization(korean.clone()));
        assert!(window.set_localization(korean.clone()));
        assert_eq!(host.session.state().document_file.epoch, epoch);
        assert_eq!(window.documents.parked_owner_mut(1).unwrap().localization().language(), UiLanguage::Japanese);
        host.session.complete_document_request(request, Ok(true)).unwrap();
        assert_eq!(host.session.state().document_file.location, Some(location));
        assert!(!host.session.state().document_file.modified);
        let titles = window.view(&host, 800.);
        assert_eq!(titles["tabs"][1]["title"], "Untitled.capy");
        let mut activation = switch(&mut window, &mut host, 1);
        assert_eq!(host.session.localization().language(), UiLanguage::Korean);
        assert_eq!(host.localization_generation(), 2);
        activation.work().unwrap();
        let epoch = host.session.state().document_file.epoch;
        let chinese = Localizer::shared(UiLanguage::SimplifiedChinese);
        assert!(host.set_localization(chinese.clone()));
        assert!(window.set_localization(chinese));
        assert_eq!(host.session.state().document_file.epoch, epoch);
        resume(&mut window, &mut host, &mut activation);
        assert_eq!(host.localization_generation(), 3);
        finish(host, window);
    }
}
