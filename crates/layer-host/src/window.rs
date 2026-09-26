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
    pub recovered: bool,
}

pub struct DocumentWindow<P> {
    pub documents: DocumentSessions<P>,
    pub gpu: Option<GpuContext>,
}
impl<P> Default for DocumentWindow<P> {
    fn default() -> Self {
        Self {
            documents: Default::default(),
            gpu: None,
        }
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
                .ok_or_else(|| "Drawing tab is no longer open".into())
        }
    }

    pub fn park_ready(&self, host: &NativeHost) -> Result<bool, String> {
        let session = &host.session;
        Ok(session.can_park_document()
            && (session.rendering_suspended()
                || session.state().document_file.close_ready
                || session.retained_document_tiles().try_blobs()?.is_some()))
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
            "tabs": documents.labels(&host.session.state().document_file, |p| &p.session().state().document_file),
            "selected": documents.selected(),
            "compact": layer_ui::DocumentTabs::compact(width, documents.order().len()),
            "can_undo": documents.can_undo(),
            "can_redo": documents.can_redo(),
            "resident_bytes": documents.resident_bytes(),
            "storage_error": documents.storage_error(),
            "parked_renderers": documents.parked().filter(|(_, p)| p.owner.session().engine().backend().0.is_some()).count(),
        })
    }

    pub fn request(&mut self, host: &mut NativeHost, request: TabRequest) -> Result<Value, String> {
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
                    document.busy = s.capture_project_recovery().is_err();
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
        if closing && !host.session.state().document_file.close_ready {
            return Err("Confirm closing the drawing first".into());
        }
        let target = if closing {
            self.documents.after_close().unwrap_or(0)
        } else {
            id
        };
        if !closing && target == self.documents.selected() {
            return Ok(None);
        }
        if !closing && !self.documents.contains_parked(target) {
            return Err("Drawing tab is no longer open".into());
        }
        if !host.session.can_park_document() {
            return Err("Finish the current operation before switching drawings".into());
        }
        if !self.park_ready(host)? {
            return Err("Wait for drawing capture before switching drawings".into());
        }
        if target != 0 {
            self.documents
                .parked_owner_mut(target)
                .ok_or("Drawing tab is no longer open")?
                .session_mut()
                .inherit_window_state(&host.session)?;
        }
        let tiles = host.session.park_document()?;
        let retired = self.retire_gpu(host);
        let mut closed = None;
        if closing {
            if let Some(mut next) = self.documents.close_selected() {
                std::mem::swap(&mut host.session, next.session_mut());
                exchange(&mut next);
                closed = Some(next);
            }
        } else {
            self.documents.exchange_with(target, tiles, |next| {
                std::mem::swap(&mut host.session, next.session_mut());
                exchange(next);
            })?;
        }
        self.changed(host);
        host.startup = Default::default();
        let activation = Activation {
            selected: self.documents.selected(),
            epoch: host.session.state().document_file.epoch,
            gpu: self.gpu.clone(),
            color: host.session.engine().document().color,
            options,
            retired,
            renderer: None,
        };
        Ok(Some((activation, closed)))
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
            .admit(&tiles, &next.capture_project_recovery()?)?;
        next.initialize_document_location(open.location)?;
        if open.recovered {
            next.mark_recovered();
        }
        next.set_document_replacement(false);
        next.inherit_window_state(&host.session)?;
        next.inherit_initial_drawing_tools(&host.session)?;
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
        self.documents.append(park(outgoing), tiles);
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
    use layer_ui::{LayerAction, UiAction};
    use std::time::{Duration, Instant};

    type Window = DocumentWindow<UiSession<Renderer>>;

    fn host() -> NativeHost {
        let document = layer_core::Document::new("Window", 64, 48);
        let gpu = WgpuRasterizer::new_native_headless(document.color).unwrap();
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
            recovered: false,
        }
    }

    fn candidate(window: &Window, host: &NativeHost) -> Option<Box<UiSession<Renderer>>> {
        let admission = window
            .documents
            .admission(&host.session.retained_document_tiles());
        let environment =
            OpenEnvironment::capture(&host.session, admission, Default::default()).unwrap();
        let project = layer_ui::NewDocumentOptions::default().project().unwrap();
        Some(environment.prepare(project, || false).unwrap())
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
        let mut digests: Vec<_> = tiles.blobs().unwrap().iter().map(|b| b.digest).collect();
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
            Some("Finish the current operation before switching drawings")
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
}
