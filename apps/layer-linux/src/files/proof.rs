//! Live Proof panel. Viewing is transient; rendition/profile edits are saved
//! document edits. Native controls and shared Rust history own their behavior.
use super::*;
use crate::{number_control::NumberControl, panel_controls};
use layer_core::color::{
    DocumentColor, ProofRecipe, RenderingIntent, RgbSpace,
    hdr::{SdrMethod, SdrRendition},
};
use layer_ui::ProofMode;
use std::{
    cell::{Cell, RefCell},
    rc::Weak,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Page {
    #[default]
    Off,
    Sdr,
    Print,
}
impl Page {
    fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Sdr => "sdr",
            Self::Print => "print",
        }
    }
    fn mode(self) -> ProofMode {
        match self {
            Self::Off => ProofMode::Off,
            Self::Sdr => ProofMode::Sdr,
            Self::Print => ProofMode::Print,
        }
    }
}
#[derive(Clone)]
struct PrintSettings {
    profile: Option<ExportProfile>,
    intent: RenderingIntent,
    bpc: bool,
    simulation: u32,
}
impl Default for PrintSettings {
    fn default() -> Self {
        Self {
            profile: None,
            intent: RenderingIntent::RelativeColorimetric,
            bpc: true,
            simulation: 1,
        }
    }
}
impl PrintSettings {
    fn recipe(&self) -> Result<ProofRecipe, String> {
        let p = self.profile.as_ref().ok_or("Choose a print profile")?;
        let mut r = ProofRecipe::new(p.name.clone(), p.profile.clone());
        r.conversion.intent = self.intent;
        r.conversion.black_point_compensation = self.bpc;
        r.simulate_paper = self.simulation == 2;
        r.simulate_black_ink = self.simulation != 0;
        r.validate()?;
        Ok(r)
    }
}
#[derive(Default)]
struct Model {
    identity: Cell<Option<(u64, DocumentColor)>>,
    page: Cell<Page>,
    print: RefCell<PrintSettings>,
    saved_proof: RefCell<Option<ProofRecipe>>,
    print_dirty: Cell<bool>,
    serial: Cell<u64>,
    busy: Cell<bool>,
    cancelled: RefCell<Option<Arc<AtomicBool>>>,
    error: RefCell<String>,
    views: RefCell<Vec<Weak<ProofPanel>>>,
    export_wait: Cell<bool>,
    completed: Cell<u64>,
    analysis: RefCell<Option<layer_render_wgpu::snapshot::CaptureControl>>,
}
pub(crate) struct ProofPanel {
    pub root: gtk::Box,
    model: Rc<Model>,
    form: RefCell<Option<Rc<Form>>>,
}
struct Form {
    mode: adw::ToggleGroup,
    stack: gtk::Stack,
    controls: [NumberControl; 3],
    method: gtk::DropDown,
    legacy_method: Cell<SdrMethod>,
    range_row: gtk::Box,
    back: gtk::Button,
    auto: gtk::Button,
    chooser: profile::ProfileChooser,
    intent: gtk::DropDown,
    bpc: gtk::CheckButton,
    simulation: gtk::DropDown,
    warning: gtk::CheckButton,
    progress: gtk::Spinner,
    error: gtk::Label,
    updating: Cell<bool>,
}
const INTENTS: [RenderingIntent; 4] = [
    RenderingIntent::RelativeColorimetric,
    RenderingIntent::Perceptual,
    RenderingIntent::Saturation,
    RenderingIntent::AbsoluteColorimetric,
];
impl ProofPanel {
    pub fn new() -> Rc<Self> {
        Self::with_model(Rc::default())
    }
    fn with_model(model: Rc<Model>) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.set_widget_name("proof-panel");
        root.set_vexpand(true);
        root.set_margin_top(6);
        root.set_margin_bottom(6);
        root.set_margin_start(8);
        root.set_margin_end(8);
        let p = Rc::new(Self {
            root,
            model: model.clone(),
            form: RefCell::default(),
        });
        model.views.borrow_mut().push(Rc::downgrade(&p));
        p
    }
    pub fn duplicate(&self, w: &Rc<Workspace>) -> Rc<Self> {
        let p = Self::with_model(self.model.clone());
        p.ensure(w);
        p
    }
    fn views(&self) -> Vec<Rc<Self>> {
        self.model
            .views
            .borrow_mut()
            .retain(|v| v.strong_count() > 0);
        self.model
            .views
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    }
    fn update_all(&self, w: &Rc<Workspace>) {
        for p in self.views() {
            p.update(w)
        }
    }
    fn issue(&self, w: &Rc<Workspace>, error: impl Into<String>) {
        *self.model.error.borrow_mut() = error.into();
        self.update_all(w)
    }
    pub fn refresh(self: &Rc<Self>, w: &Rc<Workspace>, state: &UiState) {
        if state.workspace.layout.panel_group(Panel::Proof).is_some()
            || self.form.borrow().is_some()
        {
            self.ensure(w);
            if state.workspace.layout.panel_group(Panel::Proof).is_none() {
                self.finish_export();
            }
            self.update(w);
        }
    }
    fn ensure(self: &Rc<Self>, w: &Rc<Workspace>) {
        let Some((identity, proof, mode)) = w.gpu.borrow().as_ref().map(|g| {
            let s = &g.session;
            (
                (s.state().document_file.epoch, s.engine().document().color),
                s.engine().document().proof.clone(),
                s.proof_mode(),
            )
        }) else {
            return;
        };
        if self.model.identity.get() != Some(identity) {
            self.cancel_job();
            if let Some(c)=self.model.analysis.borrow().as_ref(){c.cancel();}
            self.finish_export();
            self.model.identity.set(Some(identity));
            self.model.page.set(match mode {
                ProofMode::Off => Page::Off,
                ProofMode::Sdr => Page::Sdr,
                ProofMode::Print => Page::Print,
            });
            self.model.print_dirty.set(false);
            self.model.error.borrow_mut().clear();
            for p in self.views() {
                p.form.borrow_mut().take();
                while let Some(c) = p.root.first_child() {
                    p.root.remove(&c)
                }
            }
            self.restore_print(w, proof);
        } else if !self.model.print_dirty.get() && *self.model.saved_proof.borrow() != proof {
            self.restore_print(w, proof);
        }
        if self.form.borrow().is_none() {
            *self.form.borrow_mut() = Some(Form::new(
                self,
                w,
                identity.1.space,
                identity.1.depth.is_float(),
            ));
        }
    }
    fn restore_print(self: &Rc<Self>, w: &Rc<Workspace>, recipe: Option<ProofRecipe>) {
        *self.model.saved_proof.borrow_mut() = recipe.clone();
        *self.model.print.borrow_mut() = PrintSettings::default();
        self.model.print_dirty.set(false);
        let Some(recipe) = recipe else { return };
        {
            let mut p = self.model.print.borrow_mut();
            p.intent = recipe.conversion.intent;
            p.bpc = recipe.conversion.black_point_compensation;
            p.simulation = if recipe.simulate_paper {
                2
            } else {
                u32::from(recipe.simulate_black_ink)
            };
        }
        let identity = self.model.identity.get();
        let weak = Rc::downgrade(self);
        let workspace = Rc::downgrade(w);
        glib::spawn_future_local(async move {
            let embedded = recipe.profile.clone();
            let result = gio::spawn_blocking(move || profile::describe(embedded))
                .await
                .map_err(|_| "Profile reader failed".to_string())
                .and_then(|r| r);
            let (Some(p), Some(w)) = (weak.upgrade(), workspace.upgrade()) else {
                return;
            };
            if p.model.identity.get() != identity
                || p.model.print_dirty.get()
                || p.model.saved_proof.borrow().as_ref() != Some(&recipe)
            {
                return;
            }
            match result {
                Ok(mut value) => {
                    if value.name == profile::UNNAMED_PROFILE {
                        value.name = recipe.name;
                    }
                    p.model.print.borrow_mut().profile = Some(value);
                }
                Err(e) => *p.model.error.borrow_mut() = e,
            }
            p.update_all(&w);
        });
    }
    pub fn open(self: &Rc<Self>, w: &Rc<Workspace>, page: Page) -> Result<(), String> {
        self.ensure(w);
        self.model.page.set(page);
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetPanelVisible {
                panel: Panel::Proof,
                visible: true,
            },
        });
        let group = w
            .gpu
            .borrow()
            .as_ref()
            .and_then(|g| g.session.state().workspace.layout.panel_group(Panel::Proof));
        if let Some(group) = group {
            let (active, collapsed, drawer_open) = {
                let gpu = w.gpu.borrow();
                let state = gpu.as_ref().unwrap().session.state();
                let layout = &state.workspace.layout;
                let collapsed = layout.collapsed_column_for_group(group);
                let open=collapsed.is_some_and(|column| {
                    let settings=layout.column_stack(column);
                    if settings.drawers {state.customization.column_drawers.iter().any(|d| matches!(d.anchor, layer_ui::DrawerAnchor::Column {group:g,origin,..} if g==group && origin==Panel::Proof))}
                    else {settings.open_column==Some(column) && layout.active_panel(Panel::Proof)==Some(Panel::Proof)}
                });
                (layout.active_panel(Panel::Proof), collapsed, open)
            };
            if collapsed.is_some() {
                if !drawer_open {
                    w.dispatch(UiAction::Customize {
                        action: CustomizationAction::ToggleColumnDrawer {
                            group,
                            panel: Panel::Proof,
                        },
                    });
                }
            } else if active != Some(Panel::Proof) {
                w.dispatch(UiAction::SelectPanelTab {
                    group,
                    panel: Panel::Proof,
                });
            }
        }
        self.set_page(w, page);
        self.update_all(w);
        Ok(())
    }
    fn set_page(self: &Rc<Self>, w: &Rc<Workspace>, page: Page) {
        if page != Page::Sdr {
            if let Some(c) = self.model.analysis.borrow().as_ref() {
                c.cancel();
            }
        }
        self.model.page.set(page);
        self.model.error.borrow_mut().clear();
        if page != Page::Print {
            self.cancel_job();
        }
        let mode = if page == Page::Print
            && w.gpu
                .borrow()
                .as_ref()
                .is_some_and(|g| g.session.engine().document().proof.is_none())
        {
            ProofMode::Off
        } else {
            page.mode()
        };
        let result = w
            .gpu
            .borrow_mut()
            .as_mut()
            .ok_or("Canvas unavailable".into())
            .and_then(|g| g.session.set_proof_mode(mode));
        w.changed(result);
        if page == Page::Print && self.model.print_dirty.get() {
            self.prepare_print(w);
        }
        self.update_all(w);
    }
    fn sdr_recipe(form: &Form) -> SdrRendition {
        SdrRendition {
            exposure: form.controls[0].value() as f32,
            contrast: form.controls[1].value() as f32,
            headroom: form.controls[2].value() as f32,
            method: match form.method.selected() {
                1 => SdrMethod::ToneMap,
                2 => form.legacy_method.get(),
                _ => SdrMethod::Bt2390,
            },
        }
    }
    fn edit_sdr(&self, w: &Rc<Workspace>, form: &Form, phase: Option<ContactPhase>) {
        if form.updating.get() {
            return;
        }
        let recipe = Self::sdr_recipe(form);
        let phase = phase.or_else(|| {
            form.controls
                .iter()
                .any(NumberControl::is_interacting)
                .then_some(ContactPhase::Move)
        });
        let result = w
            .gpu
            .borrow_mut()
            .as_mut()
            .ok_or("Canvas unavailable".into())
            .and_then(|g| {
                if let Some(phase) = phase {
                    g.session.edit_sdr_rendition(phase, recipe)
                } else {
                    g.session.set_sdr_rendition(recipe)
                }
            });
        if let Err(e) = &result {
            *self.model.error.borrow_mut() = e.clone();
        } else {
            self.model.error.borrow_mut().clear();
        }
        w.changed(result);
        self.update_all(w);
    }
    fn finish_export(&self) {
        if self.model.export_wait.replace(false) {
            self.model
                .completed
                .set(self.model.completed.get().wrapping_add(1));
        }
    }
    /// Export must freeze the recipe the user just selected, including a profile
    /// whose validation is still finishing. One bounded worker owns that work.
    fn fit_sdr(self: &Rc<Self>, w: &Rc<Workspace>) {
        if let Some(c) = self.model.analysis.borrow().as_ref() {
            c.cancel();
            return;
        }
        let snapshot = (|| {
            let gpu = w.gpu.borrow();
            let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
            Ok::<_, String>((
                session.capture_project_recovery()?,
                session.state().camera.view().background_rgba_linear,
                session.engine().animation_time(),
                session.state().document_file.epoch,
                w.snapshot_gpu()?,
            ))
        })();
        let (project, background, time, epoch, gpu) = match snapshot {
            Ok(v) => v,
            Err(e) => {
                self.issue(w, e);
                return;
            }
        };
        let revision = project.document.revision;
        let control = layer_render_wgpu::snapshot::CaptureControl::default();
        *self.model.analysis.borrow_mut() = Some(control.clone());
        self.update_all(w);
        let panel = self.clone();
        let w = w.clone();
        glib::spawn_future_local(async move {
            let close=w.window.connect_destroy(glib::clone!(#[strong] control,move |_|control.cancel()));
            let worker_control = control.clone();
            let result = gio::spawn_blocking(move || {
                gpu.capture(
                    project,
                    background,
                    time,
                    Default::default(),
                    worker_control,
                )
                .map_err(|e| e.to_string())?
                .hdr_headroom()
            })
            .await
            .map_err(|_| "HDR analysis failed".to_string())
            .and_then(|r| r);
            w.window.disconnect(close);
            panel.model.analysis.borrow_mut().take();
            if !control.cancellation_flag().load(Ordering::Acquire) {
                let current = w.gpu.borrow().as_ref().is_some_and(|g| {
                    g.session.state().document_file.epoch == epoch
                        && g.session.engine().document().revision == revision
                });
                if current {
                    match result {
                        Ok(headroom) => {
                            let change = w
                                .gpu
                                .borrow_mut()
                                .as_mut()
                                .unwrap()
                                .session
                                .set_sdr_rendition(SdrRendition {
                                    headroom,
                                    ..Default::default()
                                });
                            w.changed(change);
                        }
                        Err(e) => panel.issue(&w, e),
                    }
                }
            }
            panel.update_all(&w);
        });
    }
    pub async fn finish_pending(&self, w: &Rc<Workspace>) -> Result<(), String> {
        while (self.model.analysis.borrow().is_some()
            || self.model.busy.get()
            || self
                .form
                .borrow()
                .as_ref()
                .is_some_and(|f| f.chooser.is_pending()))
            && w.window.is_visible()
        {
            glib::timeout_future(std::time::Duration::from_millis(20)).await;
        }
        if self.model.page.get() == Page::Print && self.model.print_dirty.get(){if let Some(form)=self.form.borrow().as_ref(){(form.chooser.selected)()?;}}
        if self.model.page.get() == Page::Print && !self.model.error.borrow().is_empty() {
            return Err(self.model.error.borrow().clone());
        }
        Ok(())
    }
    pub async fn for_export(self: &Rc<Self>, w: &Rc<Workspace>) -> Result<(), String> {
        self.open(w, Page::Sdr)?;
        let generation = self.model.completed.get();
        self.model.export_wait.set(true);
        self.update_all(w);
        while self.model.completed.get() == generation && w.window.is_visible() {
            glib::timeout_future(std::time::Duration::from_millis(40)).await;
        }
        self.finish_pending(w).await
    }
    fn edit_print(self: &Rc<Self>, w: &Rc<Workspace>, form: &Form) {
        if form.updating.get() {
            return;
        }
        let intent = INTENTS[form.intent.selected().min(3) as usize];
        *self.model.print.borrow_mut() = PrintSettings {
            profile: (form.chooser.selected)().ok(),
            intent,
            bpc: form.bpc.is_active() && intent != RenderingIntent::AbsoluteColorimetric,
            simulation: form.simulation.selected(),
        };
        self.model
            .serial
            .set(self.model.serial.get().wrapping_add(1));
        self.model.print_dirty.set(true);
        self.model.error.borrow_mut().clear();
        self.cancel_job();
        self.prepare_print(w);
    }
    fn cancel_job(&self) {
        if let Some(c) = self.model.cancelled.borrow().as_ref() {
            c.store(true, Ordering::Release);
        }
    }
    fn prepare_print(self: &Rc<Self>, w: &Rc<Workspace>) {
        if self.model.busy.get()
            || !self.model.print_dirty.get()
            || self.model.page.get() != Page::Print
        {
            return;
        }
        let recipe = match self.model.print.borrow().recipe() {
            Ok(r) => r,
            Err(_) => {
                self.update_all(w);
                return;
            }
        };
        let Some(identity) = self.model.identity.get() else {
            return;
        };
        let previous = self.model.saved_proof.borrow().clone();
        if previous.as_ref() == Some(&recipe) {
            self.model.print_dirty.set(false);
            self.update_all(w);
            return;
        }
        self.model.busy.set(true);
        let serial = self.model.serial.get();
        let cancelled = Arc::new(AtomicBool::new(false));
        *self.model.cancelled.borrow_mut() = Some(cancelled.clone());
        self.update_all(w);
        let panel = self.clone();
        let w = w.clone();
        glib::spawn_future_local(async move {
            let close=w.window.connect_destroy(glib::clone!(#[strong] cancelled,move |_|cancelled.store(true,Ordering::Release)));
            w.proof.pause().await;
            let worker_cancelled = cancelled.clone();
            let worker_recipe = recipe.clone();
            let result = gio::spawn_blocking(move || {
                layer_color::ProofLut::build(identity.1.space, &worker_recipe, || {
                    worker_cancelled.load(Ordering::Acquire)
                })
            })
            .await
            .map_err(|_| "Proof preparation worker failed".to_string())
            .and_then(|r| r);
            let result=async{
                if cancelled.load(Ordering::Acquire)||panel.model.identity.get()!=Some(identity){return Ok(None)}
                let lut=Arc::new(result?);
                profile::preserve_replaced_proof(previous.as_ref(),&recipe).await?;
                if cancelled.load(Ordering::Acquire)||panel.model.identity.get()!=Some(identity){return Ok(None)}
                let change={let mut gpu=w.gpu.borrow_mut();let s=&mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
                    if (s.state().document_file.epoch,s.engine().document().color)!=identity{return Ok(None)}
                    if s.engine().document().proof!=previous{return Err("Print settings changed while preparing the proof. Choose the profile again.".into())}
                    s.set_proof_recipe(Some(recipe.clone()))?};
                w.proof.retain(identity.1.space,recipe.clone(),lut);Ok::<_,String>(Some(change))
            }.await;
            w.window.disconnect(close);
            panel.model.busy.set(false);
            panel.model.cancelled.borrow_mut().take();
            if panel.model.identity.get() == Some(identity) && panel.model.serial.get() == serial {
                match result {
                    Ok(Some(change)) => {
                        *panel.model.saved_proof.borrow_mut() = Some(recipe);
                        panel.model.print_dirty.set(false);
                        w.changed(Ok(change));
                    }
                    Ok(None) => (),
                    Err(e) => {
                        panel.model.print_dirty.set(false);
                        panel.issue(&w, e);
                        let saved = w
                            .gpu
                            .borrow()
                            .as_ref()
                            .and_then(|g| g.session.engine().document().proof.clone());
                        panel.restore_print(&w, saved);
                    }
                }
            }
            w.proof.resume(&w);
            panel.update_all(&w);
            // At most one LUT worker exists. New choices cancel it and replace
            // the pending recipe; only the latest is started after it drains.
            if panel.model.serial.get() != serial || panel.model.identity.get() != Some(identity) {
                panel.prepare_print(&w);
            }
        });
    }
    fn update(&self, w: &Rc<Workspace>) {
        let Some(f) = self.form.borrow().clone() else {
            return;
        };
        let Some((recipe, warning, has_proof, mode)) = w.gpu.borrow().as_ref().map(|g| {
            let s = &g.session;
            (
                s.engine().document().sdr_rendition,
                s.state().gamut_warning,
                s.engine().document().proof.is_some(),
                s.proof_mode(),
            )
        }) else {
            return;
        };
        f.updating.set(true);
        // Menu shortcuts change the same transient viewing state. Keep the
        // print page visible while choosing/preparing its first profile.
        let pending_print = self.model.page.get() == Page::Print
            && (!has_proof || self.model.busy.get() || self.model.print_dirty.get());
        let page = if pending_print {
            Page::Print
        } else {
            match mode {
                ProofMode::Off => Page::Off,
                ProofMode::Sdr => Page::Sdr,
                ProofMode::Print => Page::Print,
            }
        };
        self.model.page.set(page);
        f.mode.set_active_name(Some(page.name()));
        f.stack.set_visible_child_name(page.name());
        for (c, v) in f
            .controls
            .iter()
            .zip([recipe.exposure, recipe.contrast, recipe.headroom])
        {
            c.set_value(v.into());
        }
        let mut methods = vec!["Perceptual", "Browser"];
        match recipe.method {
            SdrMethod::Scale => methods.push("Saved: Scale"),
            SdrMethod::Clip => methods.push("Saved: Clip"),
            _ => (),
        }
        let list = f
            .method
            .model()
            .unwrap()
            .downcast::<gtk::StringList>()
            .unwrap();
        if list.n_items() != methods.len() as u32
            || (methods.len() == 3 && list.string(2).as_deref() != Some(methods[2]))
        {
            f.method.set_model(Some(&gtk::StringList::new(&methods)));
        }
        f.legacy_method.set(recipe.method);
        f.method.set_selected(match recipe.method {
            SdrMethod::Bt2390 => 0,
            SdrMethod::ToneMap => 1,
            SdrMethod::Scale | SdrMethod::Clip => 2,
        });
        f.range_row.set_visible(recipe.method != SdrMethod::Clip);
        f.back.set_visible(self.model.export_wait.get());
        let p = self.model.print.borrow();
        if let Some(profile) = &p.profile {
            if (f.chooser.selected)().ok().as_ref() != Some(profile) {
                f.chooser.restore_document(profile.clone());
            }
        }
        f.intent
            .set_selected(INTENTS.iter().position(|v| *v == p.intent).unwrap_or(0) as u32);
        f.bpc.set_active(p.bpc);
        f.bpc
            .set_sensitive(p.intent != RenderingIntent::AbsoluteColorimetric);
        f.simulation.set_selected(p.simulation);
        f.warning.set_sensitive(has_proof);
        f.warning.set_active(warning);
        f.auto.set_label(if self.model.analysis.borrow().is_some() {
            "Cancel"
        } else {
            "Auto"
        });
        f.progress.set_visible(self.model.busy.get());
        f.progress.set_spinning(self.model.busy.get());
        let error = self.model.error.borrow();
        f.error.set_label(&error);
        f.error.set_visible(!error.is_empty());
        f.updating.set(false);
    }
}
impl Form {
    fn new(panel: &Rc<ProofPanel>, w: &Rc<Workspace>, space: RgbSpace, hdr: bool) -> Rc<Self> {
        let mode = panel_controls::segmented(
            "proof-mode",
            if hdr {
                &[("off", "Off"), ("sdr", "SDR"), ("print", "Print")]
            } else {
                &[("off", "Off"), ("print", "Print")]
            },
        );
        panel.root.append(&mode);
        let stack = gtk::Stack::builder()
            .vhomogeneous(false)
            .hhomogeneous(false)
            .vexpand(true)
            .transition_type(gtk::StackTransitionType::SlideLeftRight)
            .build();
        stack.add_named(&gtk::Box::new(gtk::Orientation::Vertical, 0), Some("off"));
        panel.root.append(&stack);
        let sdr = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let method = gtk::DropDown::from_strings(&["Perceptual", "Browser"]);
        method.set_widget_name("sdr-appearance-method");
        sdr.append(&panel_controls::row("Method", &method));
        let range_row = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let controls = [
            ("Exposure", "exposure", -12., 12., 0.1, 2, "EV", 1.),
            ("Contrast", "contrast", 0.25, 4., 0.01, 0, "%", 100.),
            ("HDR range", "headroom", 0., 16., 0.1, 2, "EV", 1.),
        ]
        .map(|(title, name, min, max, step, digits, unit, scale)| {
            let mut spec = NumericControl::number(min, max, step, digits);
            spec.kind = NumericKind::Slider;
            spec.unit = unit.into();
            spec.scale = scale;
            match name {
                "contrast" => {
                    spec.resolution = 0.01;
                    spec.soft_min = 0.5;
                    spec.soft_max = 2.;
                }
                "exposure" => {
                    spec.soft_min = -4.;
                    spec.soft_max = 4.;
                }
                _ => {
                    spec.soft_max = 6.;
                }
            }
            let c = NumberControl::inline(spec, title);
            c.set_widget_name(&format!("sdr-appearance-{name}"));
            let row = panel_controls::row(title, &c);
            if name == "headroom" {
                range_row.append(&row);
                sdr.append(&range_row);
            } else {
                sdr.append(&row);
            }
            c
        });
        let reset = gtk::Button::with_label("Reset");
        reset.set_widget_name("sdr-appearance-reset");
        reset.set_halign(gtk::Align::End);
        let auto = gtk::Button::with_label("Auto");
        auto.set_widget_name("sdr-appearance-auto");
        auto.set_tooltip_text(Some(
            "Fit the highlight range to the edited image and reset exposure and contrast",
        ));
        auto.connect_clicked(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            move |_| panel.fit_sdr(&w)
        ));
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        actions.set_halign(gtk::Align::End);
        actions.append(&auto);
        actions.append(&reset);
        sdr.append(&actions);
        stack.add_named(&crate::workspace::scroll(&sdr), Some("sdr"));
        let print = gtk::Box::new(gtk::Orientation::Vertical, 4);
        print.set_widget_name("soft-proof-setup");
        let chooser = profile::ProfileChooser::new(
            w,
            "Printer & paper",
            "proof-profile",
            space,
            profile::ProfilePurpose::Proof,
        );
        chooser.row.set_title_lines(1);
        chooser.row.set_subtitle_lines(1);
        let profile_group = adw::PreferencesGroup::new();
        profile_group.add(&chooser.row);
        print.append(&profile_group);
        print.append(&chooser.error);
        let simulation = gtk::DropDown::from_strings(&["Colors", "Black ink", "Paper & ink"]);
        simulation.set_widget_name("proof-simulation");
        print.append(&panel_controls::row("Simulate", &simulation));
        let options = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let intent =
            gtk::DropDown::from_strings(&["Relative", "Perceptual", "Saturation", "Absolute"]);
        intent.set_widget_name("proof-intent");
        options.append(&panel_controls::row("Intent", &intent));
        let bpc = gtk::CheckButton::with_label("Black point compensation");
        bpc.set_widget_name("proof-bpc");
        options.append(&bpc);
        let warning = gtk::CheckButton::with_label("Show out-of-gamut colors");
        warning.set_widget_name("proof-gamut-warning");
        options.append(&warning);
        let advanced = gtk::Expander::builder()
            .label("Options")
            .child(&options)
            .build();
        advanced.set_widget_name("proof-options");
        print.append(&advanced);
        stack.add_named(&crate::workspace::scroll(&print), Some("print"));
        let progress = gtk::Spinner::new();
        progress.set_tooltip_text(Some("Preparing print proof"));
        progress.set_widget_name("proof-preparing");
        panel.root.append(&progress);
        let back = gtk::Button::with_label("Back to Export");
        back.set_widget_name("proof-return-export");
        panel.root.append(&back);
        let error = gtk::Label::new(None);
        error.set_wrap(true);
        error.set_xalign(0.);
        error.add_css_class("error");
        error.set_widget_name("proof-setup-error");
        panel.root.append(&error);
        let f = Rc::new(Self {
            mode,
            stack,
            controls,
            method,
            legacy_method: Cell::new(SdrMethod::Bt2390),
            range_row,
            back,
            chooser,
            intent,
            bpc,
            simulation,
            warning,
            progress,
            auto,
            error,
            updating: Cell::new(false),
        });
        f.mode.connect_active_name_notify(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            f,
            move |mode| {
                if !f.updating.get() {
                    panel.set_page(
                        &w,
                        match mode.active_name().as_deref() {
                            Some("sdr") => Page::Sdr,
                            Some("print") => Page::Print,
                            _ => Page::Off,
                        },
                    );
                }
            }
        ));
        f.method.connect_selected_notify(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            f,
            move |_| panel.edit_sdr(&w, &f, None)
        ));
        for c in &f.controls {
            c.connect_value_changed(glib::clone!(
                #[weak]
                panel,
                #[weak]
                w,
                #[weak]
                f,
                move |_| panel.edit_sdr(&w, &f, None)
            ));
            c.connect_interaction(glib::clone!(
                #[weak]
                panel,
                #[weak]
                w,
                #[weak]
                f,
                move |_, phase| panel.edit_sdr(&w, &f, Some(phase))
            ));
        }
        reset.connect_clicked(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            move |_| {
                let result = w
                    .gpu
                    .borrow_mut()
                    .as_mut()
                    .ok_or("Canvas unavailable".into())
                    .and_then(|g| g.session.set_sdr_rendition(SdrRendition::default()));
                w.changed(result);
                panel.update_all(&w);
            }
        ));
        f.back.connect_clicked(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            move |_| {
                panel.finish_export();
                panel.update_all(&w);
            }
        ));
        f.chooser.row.connect_subtitle_notify(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            f,
            move |_| panel.edit_print(&w, &f)
        ));
        f.intent.connect_selected_notify(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            f,
            move |_| panel.edit_print(&w, &f)
        ));
        f.simulation.connect_selected_notify(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            f,
            move |_| panel.edit_print(&w, &f)
        ));
        f.bpc.connect_toggled(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            f,
            move |_| panel.edit_print(&w, &f)
        ));
        f.warning.connect_toggled(glib::clone!(
            #[weak]
            w,
            #[weak]
            f,
            move |_| {
                if !f.updating.get() {
                    w.dispatch(UiAction::Invoke {
                        command: CommandId::GamutWarning,
                    });
                }
            }
        ));
        f
    }
}
pub(crate) fn run(w: &Rc<Workspace>) -> Result<(), String> {
    // One menu entry opens the current Proof page without changing viewing.
    let mode = w
        .gpu
        .borrow()
        .as_ref()
        .map(|g| g.session.proof_mode())
        .unwrap_or_default();
    w.proof_panel.open(
        w,
        match mode {
            ProofMode::Off => Page::Off,
            ProofMode::Sdr => Page::Sdr,
            ProofMode::Print => Page::Print,
        },
    )
}
