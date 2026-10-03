//! One cancellable CPU preparation and one immutable viewing cache per window.
//! GTK owns scheduling; the GPU owner receives only completed samples.
use crate::workspace::Workspace;
use gtk::{gio, glib, prelude::*};
use layer_core::color::{ProofRecipe, RgbSpace};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, PartialEq, Eq)]
struct Key {
    epoch: u64,
    owner: u64,
    space: RgbSpace,
    recipe: Option<ProofRecipe>,
}
#[derive(Clone, PartialEq, Eq)]
struct Desired {
    key: Key,
    enabled: bool,
    gamut: bool,
}
type Cache = (RgbSpace, ProofRecipe, Arc<layer_color::ProofLut>);

pub(crate) struct ProofView {
    pub label: gtk::Label,
    desired: RefCell<Option<Desired>>,
    published: RefCell<Option<Desired>>,
    cache: RefCell<Option<Cache>>,
    cancelled: RefCell<Option<Arc<AtomicBool>>>,
    running: Cell<bool>,
    paused: Cell<bool>,
    error: RefCell<Option<layer_ui::ColorFeatureError>>,
    copy_language:Cell<Option<layer_ui::UiLanguage>>,
}
impl ProofView {
    #[cfg(test)]
    pub fn work_info(&self) -> (bool, Option<usize>) {
        (self.running.get(), self.cancelled.borrow().as_ref().map(|job| Arc::as_ptr(job) as usize))
    }
    #[cfg(test)]
    pub fn cache_info(&self) -> Option<(usize, u32, usize)> {
        self.cache
            .borrow()
            .as_ref()
            .map(|(_, _, lut)| (Arc::as_ptr(lut) as usize, lut.edge(), lut.byte_len()))
    }

    pub fn new() -> Rc<Self> {
        let label = gtk::Label::builder()
            .visible(false)
            .max_width_chars(40)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .valign(gtk::Align::End)
            .build();
        label.set_widget_name("proof-target-indicator");
        label.add_css_class("status-bubble");
        Rc::new(Self {
            label,
            desired: Default::default(),
            published: Default::default(),
            cache: Default::default(),
            cancelled: Default::default(),
            running: Cell::new(false),
            paused: Cell::new(false),
            error:Default::default(),
            copy_language:Cell::new(None),
        })
    }
    pub async fn pause(&self) {
        self.paused.set(true);
        if let Some(cancelled) = self.cancelled.borrow().as_ref() {
            cancelled.store(true, Ordering::Release);
        }
        while self.running.get() {
            glib::timeout_future(Duration::from_millis(5)).await;
        }
    }
    pub fn resume(self: &Rc<Self>, w: &Rc<Workspace>) {
        self.paused.set(false);
        self.desired.borrow_mut().take();
        self.published.borrow_mut().take();
        self.sync(w);
    }
    pub fn retain(&self, space: RgbSpace, recipe: ProofRecipe, lut: Arc<layer_color::ProofLut>) {
        *self.cache.borrow_mut() = Some((space, recipe, lut));
    }
    fn current(w: &Workspace) -> Option<Desired> {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref()?.session;
        if session.rendering_suspended() {
            return None;
        }
        let document = session.engine().document();
        Some(Desired {
            key: Key {
                epoch: session.state().document_file.epoch,
                owner: session.engine().backend().proof_owner,
                space: document.color.space,
                recipe: document.proof.clone(),
            },
            enabled: session.state().soft_proof,
            gamut: session.state().gamut_warning,
        })
    }
    fn localize(&self,w:&Workspace) {
        let localizer=w.localization();
        self.copy_language.set(Some(localizer.language()));
        let desired=self.desired.borrow();
        let Some(desired)=desired.as_ref() else { self.label.set_visible(false);return; };
        self.label.set_visible(desired.key.recipe.is_some()&&(desired.enabled||desired.gamut));
        let copy=layer_ui::color_feature_copy::ProofCopy::new(&localizer);
        let error=self.error.borrow();
        let text=if error.is_some() {copy.unavailable.to_string()}
            else if self.published.borrow().as_ref()!=Some(desired) {copy.preparing.to_string()}
            else if desired.enabled||desired.gamut {
                let name=desired.key.recipe.as_ref().map_or("",|recipe|recipe.name.as_str());
                let name=layer_ui::ExportProfileCaption::for_name(name.to_owned()).message(&localizer);
                layer_ui::color_feature_copy::proof_status(&localizer,&name,desired.enabled,desired.gamut)
            } else {copy.normal.to_string()};
        self.label.set_text(&text);
        self.label.set_tooltip_text(Some(&error.as_ref().map_or(text,|reason|reason.proof_message(&localizer))));
    }
    pub fn sync(self: &Rc<Self>, w: &Rc<Workspace>) {
        let desired = Self::current(w);
        if *self.desired.borrow() == desired {
            if self.copy_language.get()!=Some(w.localization().language()) {self.localize(w);}
            return;
        }
        let target_changed =
            self.desired.borrow().as_ref().map(|d| &d.key) != desired.as_ref().map(|d| &d.key);
        if target_changed && let Some(cancelled) = self.cancelled.borrow().as_ref() {
            cancelled.store(true, Ordering::Release);
        }
        self.error.borrow_mut().take();
        *self.desired.borrow_mut() = desired;
        self.localize(w);
        if self.paused.get() || self.running.replace(true) {
            return;
        }
        glib::MainContext::default().spawn_local(glib::clone!(#[strong(rename_to = state)] self, #[weak] w, async move {
            loop {
                if state.paused.get() { break; }
                let Some(desired) = state.desired.borrow().clone() else { break; };
                if state.published.borrow().as_ref() == Some(&desired) { break; }
                let result = state.prepare_and_publish(&w, &desired).await;
                if Self::current(&w).as_ref() != Some(&desired) { continue; }
                *state.published.borrow_mut() = Some(desired.clone());
                *state.error.borrow_mut()=result.err();
                state.localize(&w);
                w.wake();
            }
            state.running.set(false);
        }));
    }
    async fn publish(
        w: &Workspace,
        lut: Option<Arc<layer_color::ProofLut>>,
        enabled: bool,
        gamut: bool,
    ) -> Result<(), String> {
        let receiver = w
            .gpu
            .borrow()
            .as_ref()
            .ok_or("Canvas unavailable")?
            .session
            .engine()
            .backend()
            .set_proof(lut, enabled, gamut)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            match receiver.try_recv() {
                Ok(result) => return result,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err("Proof GPU publication stopped".into());
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => (),
            }
            if std::time::Instant::now() >= deadline {
                return Err("Proof GPU publication timed out".into());
            }
            glib::timeout_future(Duration::from_millis(5)).await;
        }
    }
    async fn prepare_and_publish(&self, w: &Workspace, desired: &Desired) -> Result<(), layer_ui::ColorFeatureError> {
        let cached = self
            .cache
            .borrow()
            .as_ref()
            .filter(|(space, recipe, _)| {
                *space == desired.key.space && Some(recipe) == desired.key.recipe.as_ref()
            })
            .map(|(_, _, lut)| lut.clone());
        let Some(recipe) = desired.key.recipe.clone() else {
            self.cache.borrow_mut().take();
            return Self::publish(w, None, false, false).await.map_err(Into::into);
        };
        if !desired.enabled && !desired.gamut {
            return Self::publish(w, cached, false, false).await.map_err(Into::into);
        }
        let lut = if let Some(lut) = cached {
            lut
        } else {
            // Retire the previous target before compilation; normal viewing is
            // explicitly labelled as preparation until the complete upload lands.
            Self::publish(w, None, false, false).await?;
            if self.paused.get() || Self::current(w).as_ref().map(|d| &d.key) != Some(&desired.key)
            {
                return Ok(());
            }
            self.cache.borrow_mut().take();
            let cancelled = Arc::new(AtomicBool::new(false));
            *self.cancelled.borrow_mut() = Some(cancelled.clone());
            let worker_cancelled = cancelled.clone();
            let space = desired.key.space;
            let target = recipe.clone();
            let result = gio::spawn_blocking(move || {
                layer_color::ProofLut::build(space, &target, || {
                    worker_cancelled.load(Ordering::Acquire)
                })
            })
            .await
            .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Proof preparation worker failed".into()))
            .and_then(|r| r.map_err(Into::into));
            self.cancelled.borrow_mut().take();
            if cancelled.load(Ordering::Acquire) {
                return Err(layer_ui::ColorFeatureError::ProofCancelled);
            }
            let lut = Arc::new(result?);
            self.retain(space, recipe, lut.clone());
            lut
        };
        if Self::current(w).as_ref() != Some(desired) {
            return Ok(());
        }
        Self::publish(w, Some(lut), desired.enabled, desired.gamut).await.map_err(Into::into)
    }
}
impl Drop for ProofView {
    fn drop(&mut self) {
        if let Some(cancelled) = self.cancelled.get_mut().take() {
            cancelled.store(true, Ordering::Release);
        }
    }
}
