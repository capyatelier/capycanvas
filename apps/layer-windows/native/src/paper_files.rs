use layer_host::NativeHost;
use layer_ui::{HostRequestKind, UiAction, paper_library::{PaperEntry, PaperLibraryError, PaperRequest}};
use std::{io::BufReader, path::PathBuf, sync::{Arc, atomic::AtomicBool, mpsc}};

enum Outcome {
    Entries(Vec<PaperEntry>),
    Loaded(String, layer_core::color::source::SourceImage),
    Imported(Option<u64>, String, layer_core::color::source::SourceImage, Vec<PaperEntry>),
}
struct Pending {
    epoch: u64,
    request: Option<u32>,
    receive: mpsc::Receiver<Result<Outcome, PaperLibraryError>>,
}
pub(crate) struct Service {
    signal: Arc<crate::workspace_async::Signal>,
    pending: Option<Pending>,
    initialized: bool,
    generation: u64,
    imports: std::collections::BTreeMap<u32, (u64, String)>,
    library: Option<String>,
}
impl Service {
    pub(crate) fn stop(&self) { self.signal.deactivate(); }
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { signal:crate::workspace_async::Signal::new(move || wake()), pending: None, initialized: false, generation: 0, imports:Default::default(), library: None }
    }
    pub(crate) fn status(&self) -> serde_json::Value {
        serde_json::json!({"generation":self.generation.to_string(), "busy":self.pending.is_some()})
    }
    fn start(&mut self, host: &NativeHost, request: Option<u32>, job: impl FnOnce() -> Result<Outcome, PaperLibraryError> + Send + 'static) -> Result<(), String> {
        let (send, receive) = mpsc::channel();
        let wake = std::task::Waker::from(self.signal.clone());
        std::thread::Builder::new().name("capy-paper-file".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                .unwrap_or_else(|_| Err(PaperLibraryError::Failed(std::io::Error::from(std::io::ErrorKind::Other).to_string())));
            let _ = send.send(result);
            wake.wake();
        }).map_err(|error| error.to_string())?;
        self.pending = Some(Pending { epoch:host.session.state().document_file.epoch, request, receive });
        Ok(())
    }
    pub(crate) fn import(&mut self, host: &mut NativeHost, id: Option<u32>, path: Option<String>) -> Result<(), String> {
        let Some(id) = id else {
            let path = path.ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound).to_string())?;
            crate::document_io::location(&path)?;
            self.library = Some(path);
            host.invalidate_snapshot();
            return self.poll(host);
        };
        if !host.session.state().requests.iter().any(|request| request.id == id && matches!(request.kind, HostRequestKind::Paper { request:PaperRequest::Import { .. } })) {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound).to_string());
        }
        if self.pending.as_ref().is_some_and(|pending| pending.request == Some(id)) { return Ok(()) }
        if let Some(path) = path {
            crate::document_io::location(&path)?;
            self.imports.insert(id, (host.session.state().document_file.epoch, path));
        } else { host.dispatch(UiAction::CompleteRequest { id, error:None })?; }
        host.invalidate_snapshot();
        self.poll(host)
    }
    fn import_file(&mut self, host: &mut NativeHost, request: Option<u32>, layer: Option<u64>, path: String) -> Result<(), String> {
        let directory = crate::storage::roots()?.papers();
        self.start(host, request, move || {
            let path = PathBuf::from(path);
            let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            let cancelled = Arc::new(AtomicBool::new(false));
            let file = std::fs::File::open(&path).map_err(|error| error.to_string())?;
            let mut batch = layer_ui::ImageImportBatch::new(Default::default(), layer_core::color::RgbSpace::ProPhoto, Default::default());
            batch.read_with_decoder(BufReader::new(file), &name, &cancelled, |mut input, limits, cancelled| crate::photo_reader::read(&mut input, limits, cancelled))?;
            let (_, source) = batch.take_sources(false)?.into_iter().next().ok_or(PaperLibraryError::Unsupported)?;
            let (id, image, entries) = layer_host::papers::import(&directory, &source, &name, &|| false)?;
            Ok(Outcome::Imported(layer, id, image, entries))
        })?;
        host.invalidate_snapshot();
        Ok(())
    }
    fn request(&mut self, host: &mut NativeHost, id: u32, request: PaperRequest) -> Result<(), String> {
        let directory = crate::storage::roots()?.papers();
        let image = match request { PaperRequest::Add { layer, .. } => host.session.paper_image(layer), _ => None };
        self.start(host, Some(id), move || Ok(match request {
            PaperRequest::Load { id } => Outcome::Loaded(id.clone(), layer_host::papers::load(&directory, &id)?),
            PaperRequest::Add { name, .. } => {
                let image = image.ok_or(PaperLibraryError::Unsupported)?;
                Outcome::Entries(layer_host::papers::store(&directory, &image, &name)?.1)
            }
            PaperRequest::Rename { id, name } => Outcome::Entries(layer_host::papers::rename(&directory, &id, &name)?),
            PaperRequest::Delete { id } => Outcome::Entries(layer_host::papers::delete(&directory, &id)?),
            PaperRequest::Import { .. } => return Err(PaperLibraryError::Unsupported),
        }))?;
        host.invalidate_snapshot();
        Ok(())
    }
    pub(crate) fn poll(&mut self, host: &mut NativeHost) -> Result<(), String> {
        if let Some(pending) = &self.pending {
            let result = match pending.receive.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => Err(PaperLibraryError::Failed(std::io::Error::from(std::io::ErrorKind::BrokenPipe).to_string())),
            };
            let pending = self.pending.take().unwrap();
            let current = pending.epoch == host.session.state().document_file.epoch;
            let accepted = pending.request.is_none_or(|id| current && host.session.state().requests.iter().any(|request| request.id == id));
            if let Some(id) = pending.request.filter(|_| accepted) {
                host.dispatch(UiAction::CompleteRequest { id, error:None })?;
            }
            let change = match result {
                Ok(Outcome::Entries(entries)) => host.session.set_paper_library(entries).map(|_| ()),
                Ok(Outcome::Loaded(id, image)) if accepted => host.session.paper_loaded(&id, image).map(|_| ()),
                Ok(Outcome::Loaded(..)) => Ok(()),
                Ok(Outcome::Imported(layer, id, image, entries)) => {
                    if accepted {
                        host.session.paper_imported(layer, entries, &id, image).map(|_| ())
                    } else { host.session.set_paper_library(entries).map(|_| ()) }
                }
                Err(error) if accepted => Err(error.message(host.session.localization())),
                Err(_) => Ok(()),
            };
            self.generation = self.generation.wrapping_add(1);
            if let Err(error) = change { host.error = Some(error); }
            host.invalidate_snapshot();
        }
        if self.pending.is_some() { return Ok(()) }
        if let Some(path) = self.library.take() { return self.import_file(host, None, None, path); }
        if !self.initialized {
            self.initialized = true;
            if let Ok(roots) = crate::storage::roots() {
                let directory = roots.papers();
                self.start(host, None, move || Ok(Outcome::Entries(layer_host::papers::list(&directory)?)))?;
                return Ok(());
            }
        }
        let state = host.session.state();
        self.imports.retain(|id, (epoch, _)| *epoch == state.document_file.epoch && state.requests.iter().any(|request| request.id == *id));
        let next = state.requests.iter().find_map(|request| match &request.kind {
            HostRequestKind::Paper { request:PaperRequest::Import { .. } } if !self.imports.contains_key(&request.id) => None,
            HostRequestKind::Paper { request:paper } => Some((request.id, paper.clone())),
            _ => None,
        });
        if let Some((id, request)) = next {
            if let PaperRequest::Import { layer } = request {
                let (_, path) = self.imports.remove(&id).unwrap();
                self.import_file(host, Some(id), layer, path)?;
            } else { self.request(host, id, request)?; }
        }
        Ok(())
    }
}

impl Drop for Service {
    fn drop(&mut self) { self.stop(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_ui::{EffectAction, Platform};
    use std::time::{Duration, Instant};

    fn new_host() -> NativeHost { NativeHost::new(Platform::Windows).unwrap() }
    fn service() -> Service {
        let mut service = Service::new(Arc::new(|| {}));
        service.initialized = true;
        service
    }
    fn import_request(host: &mut NativeHost) -> (u32, u64) {
        let layer = layer_ui::occurrence_token(layer_core::paper::paper_occurrence(host.session.engine().document().scene(), None).unwrap());
        host.dispatch(UiAction::Effect { action:EffectAction::ImportPaper { layer } }).unwrap();
        let id = host.session.state().requests.iter().find_map(|request| matches!(request.kind, HostRequestKind::Paper { request:PaperRequest::Import { .. } }).then_some(request.id)).unwrap();
        (id, layer)
    }
    fn pending(service: &mut Service, host: &NativeHost, request: Option<u32>) -> mpsc::Sender<Result<Outcome, PaperLibraryError>> {
        let (send, receive) = mpsc::channel();
        service.pending = Some(Pending { epoch:host.session.state().document_file.epoch, request, receive });
        send
    }
    fn gray() -> layer_core::color::source::SourceImage {
        use layer_core::color::{SampleDepth, source::{SourceBuilder, SourceChannels, SourceInterpretation}};
        let interpretation = SourceInterpretation { channels:SourceChannels::Gray, depth:SampleDepth::U8, profile:Default::default(), profile_assumed:false };
        let mut builder = SourceBuilder::new([8, 8], interpretation, 1 << 20).unwrap();
        for _ in 0..8 { builder.push_row(&[127; 8]).unwrap(); }
        builder.finish().unwrap()
    }
    #[test]
    fn queued_import_cancel_retires_only_its_live_request_without_reading_a_file() {
        let mut host = new_host();let mut service = service();let (id, _) = import_request(&mut host);
        let checkpoint = host.session.engine().checkpoint();let send = pending(&mut service, &host, None);
        assert!(service.import(&mut host, Some(id), Some("relative.png".into())).is_err());
        assert!(service.imports.is_empty());
        let directory = crate::test_support::TempDir::new();
        service.import(&mut host, Some(id), Some(directory.path.join("absent.png").to_string_lossy().into_owned())).unwrap();
        assert!(service.imports.contains_key(&id));
        service.import(&mut host, Some(id), None).unwrap();
        assert!(host.session.state().requests.iter().all(|request| request.id != id));
        send.send(Ok(Outcome::Entries(vec![]))).unwrap();service.poll(&mut host).unwrap();
        assert!(service.pending.is_none() && service.imports.is_empty());
        assert!(host.error.is_none());assert_eq!(host.session.engine().checkpoint(), checkpoint);
        assert!(service.import(&mut host, Some(id), None).is_err());
        assert_eq!(std::fs::read_dir(&directory.path).unwrap().count(), 0);
    }
    #[test]
    fn retired_import_updates_the_library_without_applying_paper_to_a_replacement_document() {
        let mut host = new_host();let mut service = service();let (request, layer) = import_request(&mut host);
        let send = pending(&mut service, &host, Some(request));let image = gray();
        let id = layer_ui::paper_library::paper_identity(&image).unwrap();
        let entry = PaperEntry { id:id.clone(), name:"Library scan".into(), bytes:64, preview:vec![127; 96*96].into() };
        let mut replacement = new_host();replacement.session.inherit_window_state(&host.session).unwrap();host = replacement;
        let paper = host.session.engine().document().paper(None).texture.clone();
        let checkpoint = host.session.engine().checkpoint();let generation = service.status()["generation"].clone();
        send.send(Ok(Outcome::Imported(Some(layer), id.clone(), image, vec![entry]))).unwrap();service.poll(&mut host).unwrap();
        assert_ne!(service.status()["generation"], generation);
        assert!(host.session.paper_preview(&id).is_some());
        assert_eq!(host.session.engine().checkpoint(), checkpoint);
        assert_eq!(host.session.engine().document().paper(None).texture, paper);
        assert!(host.session.state().settings.new_document.paper.is_none());
        let paper = layer_ui::occurrence_token(layer_core::paper::paper_occurrence(host.session.engine().document().scene(), None).unwrap());
        host.dispatch(UiAction::SelectLayer { id:paper }).unwrap();
        assert_eq!(host.session.state().layer_properties.controls.iter().find_map(|control| control.paper.as_ref()).unwrap().tiles.last().unwrap().label.as_ref(), "Library scan");
        assert!(host.error.is_none());
    }
    #[test]
    fn library_import_without_a_request_picks_the_paper_in_the_new_drawing_drawer() {
        let mut host = new_host();let mut service = service();
        assert!(service.import(&mut host, None, Some("relative.png".into())).is_err());
        assert!(service.import(&mut host, None, None).is_err());
        host.dispatch(UiAction::Invoke { command:layer_ui::CommandId::NewDocument }).unwrap();
        let checkpoint = host.session.engine().checkpoint();
        for action in [layer_ui::NewDrawingAction::Open { screen:None, untitled:2 }, layer_ui::NewDrawingAction::Paper] {
            host.dispatch(UiAction::NewDrawing { action }).unwrap();
        }
        let send = pending(&mut service, &host, None);let image = gray();
        let id = layer_ui::paper_library::paper_identity(&image).unwrap();
        let entry = PaperEntry { id:id.clone(), name:"Library scan".into(), bytes:64, preview:vec![127; 96*96].into() };
        send.send(Ok(Outcome::Imported(None, id.clone(), image, vec![entry]))).unwrap();service.poll(&mut host).unwrap();
        let Some(layer_ui::NewDrawingDrawerView::Paper { tiles, .. }) = host.session.new_drawing_view().unwrap().drawer.clone() else { panic!("Paper drawer closed") };
        let picked = tiles.iter().find(|tile| tile.selected).unwrap();
        assert_eq!((picked.id.as_deref(), picked.label.as_ref()), (Some(id.as_str()), "Library scan"));
        assert!(host.session.paper_preview(&id).is_some());
        assert_eq!(host.session.engine().checkpoint(), checkpoint);
        assert!(host.error.is_none());
    }
    #[test]
    fn retired_paper_failure_does_not_replace_the_current_documents_error() {
        let mut host = new_host();let mut service = service();let (id, _) = import_request(&mut host);
        let send = pending(&mut service, &host, Some(id));
        host.dispatch(UiAction::CompleteRequest { id, error:None }).unwrap();
        host.error = Some("Current document error".into());
        send.send(Err(PaperLibraryError::Unsupported)).unwrap();service.poll(&mut host).unwrap();
        assert_eq!(host.error.as_deref(), Some("Current document error"));assert!(service.pending.is_none());
    }
    #[test]
    fn panicked_paper_worker_wakes_and_finishes_with_an_error() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let mut host = new_host();let count = Arc::new(AtomicUsize::new(0));let wake = count.clone();
        let mut service = Service::new(Arc::new(move || { wake.fetch_add(1, Ordering::SeqCst); }));service.initialized = true;
        service.start(&host, None, || std::panic::resume_unwind(Box::new("paper worker fixture"))).unwrap();
        let deadline = Instant::now()+Duration::from_secs(2);
        while service.pending.is_some() || count.load(Ordering::SeqCst)==0 {
            service.poll(&mut host).unwrap();assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);assert!(host.error.is_some());
    }
    #[test]
    fn dropping_paper_jobs_releases_the_window_callback_before_io_completes() {
        let host = new_host();
        for stop in [false, true] {
        let callback: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let retained = Arc::downgrade(&callback);
        let mut service = Service::new(callback);
        let (entered, inside) = mpsc::channel();let (release, released) = mpsc::channel();
        service.start(&host, None, move || { entered.send(()).unwrap();released.recv().unwrap();Ok(Outcome::Entries(vec![])) }).unwrap();
        inside.recv_timeout(Duration::from_secs(2)).unwrap();
        let receive = service.pending.take().unwrap().receive;
        let mut service = Some(service);
        if stop { service.as_ref().unwrap().stop(); } else { drop(service.take()); }
        let disconnected = retained.upgrade().is_none();
        release.send(()).unwrap();
        assert!(receive.recv_timeout(Duration::from_secs(2)).unwrap().is_ok());
        assert!(disconnected, "unfinished Paper I/O must not retain a closed window callback: stop={stop}");
        }
    }

}
