//! Live Proof panel. Viewing is transient; rendition/profile edits are saved
//! document edits. Native controls and shared Rust history own their behavior.
use super::*;
use crate::panel_controls;
use layer_core::color::{DocumentColor, ProofRecipe, RgbSpace};
use layer_ui::{
    ProofMode,
    proof_panel::{PROOF_INTENTS, PrintProofControl, PrintProofSettings, ProofSimulation},
};
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
#[derive(Default)]
struct Model {
    identity: Cell<Option<(u64, DocumentColor)>>,
    page: Cell<Page>,
    print: RefCell<PrintProofSettings>,
    saved_proof: RefCell<Option<ProofRecipe>>,
    print_dirty: Cell<bool>,
    serial: Cell<u64>,
    busy: Cell<bool>,
    cancelled: RefCell<Option<Arc<AtomicBool>>>,
    error: RefCell<String>,
    views: RefCell<Vec<Weak<ProofPanel>>>,
    export_wait: Cell<bool>,
    completed: Cell<u64>,
    preview: RefCell<Option<Arc<crate::proof_dial::PreviewSource>>>,
}
pub(crate) struct ProofPanel {
    pub root: gtk::Box,
    model: Rc<Model>,
    form: RefCell<Option<Rc<Form>>>,
}
struct Form {
    mode: adw::ToggleGroup,
    stack: gtk::Stack,
    dial: Rc<crate::proof_dial::ProofDial>,
    back: gtk::Button,
    chooser: profile::ProfilePicker,
    intent: gtk::DropDown,
    bpc: gtk::CheckButton,
    simulation: gtk::DropDown,
    warning: gtk::CheckButton,
    progress: gtk::Spinner,
    error: gtk::Label,
    updating: Cell<bool>,
}
impl ProofPanel {
    pub fn new() -> Rc<Self> {
        Self::with_model(Rc::default())
    }
    fn with_model(model: Rc<Model>) -> Rc<Self> {
        let root = panel_controls::column();
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
    pub fn set_preview(&self, source: Option<Arc<crate::proof_dial::PreviewSource>>) {
        *self.model.preview.borrow_mut() = source.clone();
        for view in self.views() {
            if let Some(form) = view.form.borrow().as_ref() {
                form.dial.set_preview(source.clone());
            }
        }
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
                s.proof_panel_mode(),
            )
        }) else {
            return;
        };
        if self.model.identity.get() != Some(identity) {
            self.cancel_job();
            self.finish_export();
            self.model.identity.set(Some(identity));
            self.set_preview(None);
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
        *self.model.print.borrow_mut() = PrintProofSettings::default();
        self.model.print_dirty.set(false);
        let Some(recipe) = recipe else { return };
        {
            let mut p = self.model.print.borrow_mut();
            p.intent = recipe.conversion.intent;
            p.bpc = recipe.conversion.black_point_compensation;
            p.simulation = ProofSimulation::from_recipe(&recipe);
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
        if self.model.page.replace(page) != page {
            self.model
                .serial
                .set(self.model.serial.get().wrapping_add(1));
        }
        self.model.error.borrow_mut().clear();
        if page != Page::Print {
            self.cancel_job();
        }
        let result = w
            .gpu
            .borrow_mut()
            .as_mut()
            .ok_or("Canvas unavailable".into())
            .and_then(|g| g.session.select_proof_mode(page.mode()));
        w.changed(result);
        if page == Page::Print && self.model.print_dirty.get() {
            self.prepare_print(w);
        }
        self.update_all(w);
    }
    fn edit_sdr(&self, w: &Rc<Workspace>, form: &Form, phase: Option<ContactPhase>) {
        if form.updating.get() {
            return;
        }
        let recipe = form.dial.recipe();
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
    pub async fn finish_pending(&self, w: &Rc<Workspace>) -> Result<(), String> {
        while (self.model.busy.get()
            || self
                .form
                .borrow()
                .as_ref()
                .is_some_and(|f| f.chooser.is_pending()))
            && w.window.is_visible()
        {
            glib::timeout_future(std::time::Duration::from_millis(20)).await;
        }
        if self.model.page.get() == Page::Print && self.model.print_dirty.get() {
            if let Some(form) = self.form.borrow().as_ref() {
                (form.chooser.selected)()?;
            }
        }
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
        let intent = PROOF_INTENTS[form.intent.selected().min(3) as usize].value;
        *self.model.print.borrow_mut() = PrintProofSettings {
            profile: (form.chooser.selected)().ok(),
            intent,
            bpc: form.bpc.is_active(),
            simulation: ProofSimulation::CHOICES[form.simulation.selected() as usize].value,
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
            let close = w.window.connect_destroy(glib::clone!(
                #[strong]
                cancelled,
                move |_| cancelled.store(true, Ordering::Release)
            ));
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
                    if s.proof_panel_mode()!=ProofMode::Print{return Ok(None)}
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
                s.proof_panel_mode(),
            )
        }) else {
            return;
        };
        f.updating.set(true);
        // The shared selection includes first-profile setup. Menu Off must also
        // cancel it, so a late worker cannot unexpectedly enable print proofing.
        let page = match mode {
            ProofMode::Off => Page::Off,
            ProofMode::Sdr => Page::Sdr,
            ProofMode::Print => Page::Print,
        };
        if self.model.page.replace(page) != page {
            self.model
                .serial
                .set(self.model.serial.get().wrapping_add(1));
        }
        if page != Page::Print {
            self.cancel_job();
        }
        f.mode.set_active_name(Some(page.name()));
        f.stack.set_visible_child_name(page.name());
        f.dial.set_recipe(recipe);
        f.back.set_visible(self.model.export_wait.get());
        let p = self.model.print.borrow();
        if let Some(profile) = &p.profile {
            if (f.chooser.selected)().ok().as_ref() != Some(profile) {
                f.chooser.restore_document(profile.clone());
            }
        }
        f.intent.set_selected(
            PROOF_INTENTS
                .iter()
                .position(|v| v.value == p.intent)
                .unwrap_or(0) as u32,
        );
        f.bpc.set_active(p.bpc && p.bpc_available());
        f.bpc.set_sensitive(p.bpc_available());
        f.simulation.set_selected(
            ProofSimulation::CHOICES
                .iter()
                .position(|v| v.value == p.simulation)
                .unwrap() as u32,
        );
        f.warning.set_sensitive(has_proof);
        f.warning.set_active(warning);
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
        let dial = crate::proof_dial::ProofDial::new();
        dial.set_preview(panel.model.preview.borrow().clone());
        stack.add_named(&dial.root, Some("sdr"));
        let print = panel_controls::column();
        print.set_widget_name("soft-proof-setup");
        let chooser = profile::ProfilePicker::compact(
            w,
            "proof-profile-choose",
            space,
            profile::ProfilePurpose::Proof,
        );
        let simulation = panel_controls::dropdown(&ProofSimulation::CHOICES.map(|c| c.label));
        simulation.set_widget_name("proof-simulation");
        let intent = panel_controls::dropdown(&PROOF_INTENTS.map(|c| c.label));
        intent.set_widget_name("proof-intent");
        let bpc = panel_controls::check(PrintProofControl::BlackPointCompensation.label());
        bpc.set_widget_name("proof-bpc");
        let warning = panel_controls::check(PrintProofControl::GamutWarning.label());
        warning.set_widget_name("proof-gamut-warning");
        // Shared order and labels; only native widget construction lives here.
        for field in PrintProofControl::ALL {
            match field {
                PrintProofControl::Profile => {
                    let row = panel_controls::row(field.label(), &chooser.button);
                    row.set_widget_name("proof-profile");
                    print.append(&row);
                    print.append(&chooser.error);
                }
                PrintProofControl::Simulation => {
                    print.append(&panel_controls::row(field.label(), &simulation))
                }
                PrintProofControl::Intent => {
                    print.append(&panel_controls::row(field.label(), &intent))
                }
                PrintProofControl::BlackPointCompensation => print.append(&bpc),
                PrintProofControl::GamutWarning => print.append(&warning),
            }
        }
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
            dial,
            back,
            chooser,
            intent,
            bpc,
            simulation,
            warning,
            progress,
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
        f.dial.connect_changed(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            f,
            move |phase, _| panel.edit_sdr(&w, &f, Some(phase))
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
        f.chooser.connect_changed(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            f,
            move || panel.edit_print(&w, &f)
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
    // Reveal requests and Document Properties open the shared selection.
    let mode = w
        .gpu
        .borrow()
        .as_ref()
        .map(|g| g.session.proof_panel_mode())
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
