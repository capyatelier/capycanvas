//! Nonmodal Proof panel. Docking/input belongs to the existing workspace;
//! validation, rendition history and print transforms stay in shared Rust.
use super::*;
use crate::number_control::NumberControl;
use layer_core::color::{
    DocumentColor, ProofRecipe, RenderingIntent, RgbSpace,
    hdr::{SdrMethod, SdrRendition},
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
    Sdr,
    Print,
}
#[derive(Clone)]
struct PrintDraft {
    profile: Option<ExportProfile>,
    intent: RenderingIntent,
    bpc: bool,
    simulation: u32,
}
impl Default for PrintDraft {
    fn default() -> Self {
        Self {
            profile: None,
            intent: RenderingIntent::RelativeColorimetric,
            bpc: true,
            simulation: 1,
        }
    }
}
impl PrintDraft {
    fn recipe(&self) -> Result<ProofRecipe, String> {
        let profile = self.profile.as_ref().ok_or("Choose a print profile")?;
        let mut recipe = ProofRecipe::new(profile.name.clone(), profile.profile.clone());
        recipe.conversion.intent = self.intent;
        recipe.conversion.black_point_compensation = self.bpc;
        recipe.simulate_paper = self.simulation == 2;
        recipe.simulate_black_ink = self.simulation != 0;
        recipe.validate()?;
        Ok(recipe)
    }
}
#[derive(Default)]
struct Model {
    identity: Cell<Option<(u64, DocumentColor)>>,
    page: Cell<Page>,
    saved_sdr: Cell<SdrRendition>,
    draft: Cell<Option<SdrRendition>>,
    compare: Cell<bool>,
    print: RefCell<PrintDraft>,
    saved_proof: RefCell<Option<ProofRecipe>>,
    print_dirty: Cell<bool>,
    busy: Cell<bool>,
    cancelled: RefCell<Option<Arc<AtomicBool>>>,
    error: RefCell<String>,
    views: RefCell<Vec<Weak<ProofPanel>>>,
    export_wait: Cell<bool>,
    completed: Cell<u64>,
}
pub(crate) struct ProofPanel {
    pub root: gtk::Box,
    model: Rc<Model>,
    form: RefCell<Option<Rc<Form>>>,
}
struct Form {
    stack: gtk::Stack,
    sdr_tab: gtk::ToggleButton,
    print_tab: gtk::ToggleButton,
    controls: [NumberControl; 3],
    method: gtk::DropDown,
    range_row: gtk::Box,
    sdr_controls: gtk::Box,
    sdr_unavailable: gtk::Label,
    preview: gtk::CheckButton,
    compare: gtk::CheckButton,
    apply: gtk::Button,
    revert: gtk::Button,
    back: gtk::Button,
    chooser: profile::ProfileChooser,
    intent: adw::ComboRow,
    bpc: adw::SwitchRow,
    simulation: adw::ComboRow,
    proof_preview: gtk::CheckButton,
    warning: gtk::CheckButton,
    proof_apply: gtk::Button,
    proof_revert: gtk::Button,
    proof_cancel: gtk::Button,
    print_controls: gtk::Box,
    error: gtk::Label,
    updating: Cell<bool>,
}
const INTENTS: [RenderingIntent; 4] = [
    RenderingIntent::RelativeColorimetric,
    RenderingIntent::Perceptual,
    RenderingIntent::Saturation,
    RenderingIntent::AbsoluteColorimetric,
];
fn button(label: &str, name: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.set_widget_name(name);
    b
}
fn choice(title: &str, name: &str, values: &[&str]) -> adw::ComboRow {
    let row = adw::ComboRow::builder()
        .title(title)
        .title_lines(2)
        .subtitle_lines(2)
        .use_subtitle(name == "proof-intent")
        .build();
    row.set_expression(Some(gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    )));
    row.set_model(Some(&gtk::StringList::new(values)));
    row.set_widget_name(name);
    row
}
fn compact_row(title: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.set_hexpand(true);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.);
    label.set_width_chars(9);
    label.set_halign(gtk::Align::Start);
    label.set_mnemonic_widget(Some(control));
    row.append(&label);
    row.append(control);
    row
}
impl ProofPanel {
    pub fn new() -> Rc<Self> {
        Self::with_model(Rc::default())
    }
    fn with_model(model: Rc<Model>) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.add_css_class("proof-panel");
        root.set_widget_name("proof-panel");
        root.set_vexpand(true);
        root.set_margin_top(6);
        root.set_margin_bottom(6);
        root.set_margin_start(8);
        root.set_margin_end(8);
        let panel = Rc::new(Self {
            root,
            model: model.clone(),
            form: RefCell::default(),
        });
        model.views.borrow_mut().push(Rc::downgrade(&panel));
        panel
    }
    pub fn duplicate(&self, w: &Rc<Workspace>) -> Rc<Self> {
        let panel = Self::with_model(self.model.clone());
        panel.ensure(w);
        panel
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
        for view in self.views() {
            view.update(w);
        }
    }
    fn issue(&self, w: &Rc<Workspace>, error: impl Into<String>) {
        *self.model.error.borrow_mut() = error.into();
        self.update_all(w);
    }
    pub fn refresh(self: &Rc<Self>, w: &Rc<Workspace>, state: &UiState) {
        if state.workspace.layout.panel_group(Panel::Proof).is_some()
            || self.form.borrow().is_some()
        {
            self.ensure(w);
            if state.workspace.layout.panel_group(Panel::Proof).is_none() {
                self.cancel_job();
                if self.model.draft.take().is_some() {
                    let weak = Rc::downgrade(w);
                    let identity = self.model.identity.get();
                    self.model.compare.set(false);
                    glib::idle_add_local_once(move || {
                        if let Some(w) = weak.upgrade() {
                            let change = w
                                .gpu
                                .borrow_mut()
                                .as_mut()
                                .filter(|g| {
                                    Some((
                                        g.session.state().document_file.epoch,
                                        g.session.engine().document().color,
                                    )) == identity
                                })
                                .map(|g| g.session.preview_sdr_appearance(None));
                            if let Some(change) = change {
                                w.changed(change);
                            }
                        }
                    });
                }
                self.finish_export();
            }
            self.update(w);
        }
    }
    fn ensure(self: &Rc<Self>, w: &Rc<Workspace>) {
        let Some((identity, sdr, proof)) = w.gpu.borrow().as_ref().map(|g| {
            let s = &g.session;
            let d = s.engine().document();
            (
                (s.state().document_file.epoch, d.color),
                d.sdr_rendition,
                d.proof.clone(),
            )
        }) else {
            return;
        };
        if self.model.identity.get() != Some(identity) {
            self.cancel_job();
            self.finish_export();
            self.model.identity.set(Some(identity));
            if !identity.1.depth.is_float() {
                self.model.page.set(Page::Print);
            }
            self.model.draft.set(None);
            self.model.compare.set(false);
            self.model.saved_sdr.set(sdr);
            self.model.print_dirty.set(false);
            self.model.error.borrow_mut().clear();
            for view in self.views() {
                view.form.borrow_mut().take();
                while let Some(child) = view.root.first_child() {
                    view.root.remove(&child);
                }
            }
            self.restore_print(w, proof);
        } else if !self.model.print_dirty.get() && *self.model.saved_proof.borrow() != proof {
            self.restore_print(w, proof);
        }
        if self.model.draft.get().is_none() {
            self.model.saved_sdr.set(sdr);
        }
        if self.form.borrow().is_none() {
            let form = Form::new(self, w, identity.1.space);
            *self.form.borrow_mut() = Some(form);
        }
    }
    fn restore_print(self: &Rc<Self>, w: &Rc<Workspace>, recipe: Option<ProofRecipe>) {
        *self.model.saved_proof.borrow_mut() = recipe.clone();
        *self.model.print.borrow_mut() = PrintDraft::default();
        self.model.print_dirty.set(false);
        let Some(recipe) = recipe else { return };
        {
            let mut draft = self.model.print.borrow_mut();
            draft.intent = recipe.conversion.intent;
            draft.bpc = recipe.conversion.black_point_compensation;
            draft.simulation = if recipe.simulate_paper {
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
            let (Some(panel), Some(w)) = (weak.upgrade(), workspace.upgrade()) else {
                return;
            };
            if panel.model.identity.get() != identity
                || panel.model.print_dirty.get()
                || panel.model.saved_proof.borrow().as_ref() != Some(&recipe)
            {
                return;
            }
            match result {
                Ok(mut p) => {
                    if p.name == profile::UNNAMED_PROFILE {
                        p.name = recipe.name;
                    }
                    panel.model.print.borrow_mut().profile = Some(p);
                }
                Err(e) => {
                    *panel.model.error.borrow_mut() = e;
                }
            }
            panel.update_all(&w);
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
    fn set_page(&self, w: &Rc<Workspace>, page: Page) {
        self.model.page.set(page);
        if page == Page::Sdr {
            let hdr = self
                .model
                .identity
                .get()
                .is_some_and(|(_, c)| c.depth.is_float());
            if hdr {
                self.preview_sdr(w, true, false);
            }
        } else {
            let hdr = self
                .model
                .identity
                .get()
                .is_some_and(|(_, c)| c.depth.is_float());
            if hdr {
                self.preview_sdr(w, false, false);
            }
            let turn_on = w.gpu.borrow().as_ref().is_some_and(|g| {
                g.session.engine().document().proof.is_some() && !g.session.state().soft_proof
            });
            if turn_on {
                w.dispatch(UiAction::Invoke {
                    command: CommandId::SoftProof,
                });
            }
        }
        self.update_all(w);
    }
    fn preview_sdr(&self, w: &Rc<Workspace>, enabled: bool, compare: bool) {
        self.model.compare.set(compare);
        let recipe = if compare {
            Some(self.model.saved_sdr.get())
        } else {
            self.model.draft.get()
        };
        let result = w
            .gpu
            .borrow_mut()
            .as_mut()
            .ok_or("Canvas unavailable".into())
            .and_then(|g| g.session.set_sdr_view(recipe, enabled));
        if let Err(e) = &result {
            *self.model.error.borrow_mut() = e.clone();
        }
        w.changed(result);
        self.update_all(w);
    }
    fn edit_sdr(&self, w: &Rc<Workspace>, form: &Form) {
        if form.updating.get() {
            return;
        }
        let r = SdrRendition {
            exposure: form.controls[0].value() as f32,
            contrast: form.controls[1].value() as f32,
            headroom: form.controls[2].value() as f32,
            method: match form.method.selected() {
                1 => SdrMethod::Scale,
                2 => SdrMethod::Clip,
                _ => SdrMethod::ToneMap,
            },
        };
        match r.validate().map(|()| r) {
            Ok(r) => {
                self.model.draft.set(Some(r));
                self.model.error.borrow_mut().clear();
                self.preview_sdr(w, true, false);
            }
            Err(e) => self.issue(w, e),
        }
    }
    fn apply_sdr(&self, w: &Rc<Workspace>) {
        if let Some(recipe) = self.model.draft.get() {
            let result = (|| {
                let mut gpu = w.gpu.borrow_mut();
                let s = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
                if s.engine().document().sdr_rendition != self.model.saved_sdr.get() {
                    return Err(
                        "Saved appearance changed. Revert this draft before editing again.".into(),
                    );
                }
                let change = s.set_sdr_rendition(recipe)?;
                s.preview_sdr_appearance(None)?;
                Ok(change)
            })();
            if let Err(e) = &result {
                self.issue(w, e);
                return;
            }
            self.model.draft.set(None);
            self.model.compare.set(false);
            self.model.saved_sdr.set(recipe);
            w.changed(result);
        }
        self.finish_export();
        self.update_all(w);
    }
    fn revert_sdr(&self, w: &Rc<Workspace>) {
        self.model.draft.set(None);
        self.model.error.borrow_mut().clear();
        self.preview_sdr(w, true, false);
        self.finish_export();
    }
    fn finish_export(&self) {
        if self.model.export_wait.replace(false) {
            self.model
                .completed
                .set(self.model.completed.get().wrapping_add(1));
        }
    }
    pub async fn for_export(self: &Rc<Self>, w: &Rc<Workspace>) -> Result<(), String> {
        self.open(w, Page::Sdr)?;
        let generation = self.model.completed.get();
        self.model.export_wait.set(true);
        self.update_all(w);
        while self.model.completed.get() == generation && w.window.is_visible() {
            glib::timeout_future(std::time::Duration::from_millis(40)).await;
        }
        Ok(())
    }
    fn edit_print(&self, w: &Rc<Workspace>, form: &Form) {
        if form.updating.get() || self.model.busy.get() {
            return;
        }
        let intent = INTENTS[form.intent.selected().min(3) as usize];
        *self.model.print.borrow_mut() = PrintDraft {
            profile: (form.chooser.selected)().ok(),
            intent,
            bpc: form.bpc.is_active() && intent != RenderingIntent::AbsoluteColorimetric,
            simulation: form.simulation.selected(),
        };
        self.model.print_dirty.set(true);
        self.model.error.borrow_mut().clear();
        self.update_all(w);
    }
    fn cancel_job(&self) {
        if let Some(c) = self.model.cancelled.borrow().as_ref() {
            c.store(true, Ordering::Release);
        }
    }
    fn apply_print(self: &Rc<Self>, w: &Rc<Workspace>) {
        if self.model.busy.replace(true) {
            return;
        }
        let recipe = match self.model.print.borrow().recipe() {
            Ok(r) => r,
            Err(e) => {
                self.model.busy.set(false);
                self.issue(w, e);
                return;
            }
        };
        let Some(identity) = self.model.identity.get() else {
            self.model.busy.set(false);
            return;
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        *self.model.cancelled.borrow_mut() = Some(cancelled.clone());
        self.model.error.borrow_mut().clear();
        self.update_all(w);
        let panel = self.clone();
        let w = w.clone();
        glib::spawn_future_local(async move {
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
            let result = async {
                if cancelled.load(Ordering::Acquire) || panel.model.identity.get() != Some(identity)
                {
                    return Ok(None);
                }
                let lut = Arc::new(result?);
                let previous = panel.model.saved_proof.borrow().clone();
                profile::preserve_replaced_proof(previous.as_ref(), &recipe).await?;
                if cancelled.load(Ordering::Acquire) || panel.model.identity.get() != Some(identity)
                {
                    return Ok(None);
                }
                let change = {
                    let mut gpu = w.gpu.borrow_mut();
                    let s = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
                    if (s.state().document_file.epoch, s.engine().document().color) != identity {
                        return Ok(None);
                    }
                    if s.engine().document().proof != previous {
                        return Err(
                            "Saved proof changed. Revert this draft before editing again.".into(),
                        );
                    }
                    s.set_proof_recipe(Some(recipe.clone()))?
                };
                w.proof.retain(identity.1.space, recipe.clone(), lut);
                Ok::<_, String>(Some(change))
            }
            .await;
            panel.model.busy.set(false);
            panel.model.cancelled.borrow_mut().take();
            match result {
                Ok(Some(change)) => {
                    *panel.model.saved_proof.borrow_mut() = Some(recipe);
                    panel.model.print_dirty.set(false);
                    w.changed(Ok(change));
                }
                Ok(None) => (),
                Err(e) => panel.issue(&w, e),
            }
            w.proof.resume(&w);
            panel.update_all(&w);
        });
    }
    fn update(&self, w: &Rc<Workspace>) {
        let form = self.form.borrow().clone();
        let Some(f) = form else { return };
        let Some((hdr, preview, proof, warning, saved, has_proof)) =
            w.gpu.borrow().as_ref().map(|g| {
                let s = &g.session;
                let d = s.engine().document();
                (
                    d.color.depth.is_float(),
                    s.state().preview_sdr || s.state().sdr_appearance_preview.is_some(),
                    s.state().soft_proof,
                    s.state().gamut_warning,
                    d.sdr_rendition,
                    d.proof.is_some(),
                )
            })
        else {
            return;
        };
        f.updating.set(true);
        f.stack
            .set_visible_child_name(if self.model.page.get() == Page::Sdr {
                "sdr"
            } else {
                "print"
            });
        f.sdr_tab.set_active(self.model.page.get() == Page::Sdr);
        f.print_tab.set_active(self.model.page.get() == Page::Print);
        f.sdr_tab.set_visible(hdr);
        f.print_tab.set_visible(hdr);
        f.sdr_controls.set_visible(hdr);
        f.sdr_unavailable.set_visible(!hdr);
        let recipe = self.model.draft.get().unwrap_or(saved);
        for (c, v) in f
            .controls
            .iter()
            .zip([recipe.exposure, recipe.contrast, recipe.headroom])
        {
            c.set_value(v.into());
        }
        f.method.set_selected(match recipe.method {
            SdrMethod::ToneMap => 0,
            SdrMethod::Scale => 1,
            SdrMethod::Clip => 2,
        });
        f.range_row.set_visible(recipe.method != SdrMethod::Clip);
        f.preview.set_active(preview && !proof);
        f.compare.set_sensitive(self.model.draft.get().is_some());
        f.compare.set_active(self.model.compare.get());
        f.apply.set_sensitive(self.model.draft.get().is_some());
        f.revert.set_sensitive(self.model.draft.get().is_some());
        f.back.set_visible(self.model.export_wait.get());
        let draft = self.model.print.borrow();
        if let Some(profile) = &draft.profile {
            if (f.chooser.selected)().ok().as_ref() != Some(profile) {
                f.chooser.restore_document(profile.clone());
            }
        }
        f.intent
            .set_selected(INTENTS.iter().position(|v| *v == draft.intent).unwrap_or(0) as u32);
        f.bpc.set_active(draft.bpc);
        f.bpc
            .set_sensitive(draft.intent != RenderingIntent::AbsoluteColorimetric);
        f.simulation.set_selected(draft.simulation);
        f.proof_apply
            .set_sensitive(draft.recipe().is_ok() && !self.model.busy.get());
        f.proof_revert
            .set_sensitive(self.model.print_dirty.get() && !self.model.busy.get());
        f.proof_cancel.set_visible(self.model.busy.get());
        f.print_controls.set_sensitive(!self.model.busy.get());
        f.proof_preview.set_sensitive(has_proof);
        f.proof_preview.set_active(proof);
        f.warning.set_sensitive(has_proof);
        f.warning.set_active(warning);
        let error = self.model.error.borrow();
        f.error.set_label(&error);
        f.error.set_visible(!error.is_empty());
        f.updating.set(false);
    }
}
impl Form {
    fn new(panel: &Rc<ProofPanel>, w: &Rc<Workspace>, space: RgbSpace) -> Rc<Self> {
        let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tabs.add_css_class("linked");
        tabs.set_homogeneous(true);
        let sdr_tab = gtk::ToggleButton::with_label("SDR");
        sdr_tab.set_widget_name("proof-sdr-tab");
        let print_tab = gtk::ToggleButton::with_label("Print");
        print_tab.set_widget_name("proof-print-tab");
        print_tab.set_group(Some(&sdr_tab));
        tabs.append(&sdr_tab);
        tabs.append(&print_tab);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        tabs.set_hexpand(true);
        header.append(&tabs);
        let more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .build();
        more.add_css_class("flat");
        more.set_tooltip_text(Some("SDR comparison and reset"));
        more.set_widget_name("proof-sdr-options");
        header.append(&more);
        let print_more = gtk::MenuButton::builder().icon_name("view-more-symbolic").build();
        print_more.add_css_class("flat");
        print_more.set_tooltip_text(Some("Print options"));
        print_more.set_widget_name("proof-advanced");
        header.append(&print_more);
        panel.root.append(&header);
        let stack = gtk::Stack::new();
        stack.set_vhomogeneous(false);
        stack.set_vexpand(true);
        stack.set_hhomogeneous(false);
        stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
        panel.root.append(&stack);
        let sdr = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let sdr_unavailable =
            gtk::Label::new(Some("SDR appearance is available for HDR drawings."));
        sdr_unavailable.set_wrap(true);
        sdr_unavailable.add_css_class("dim-label");
        sdr.append(&sdr_unavailable);
        let sdr_controls = gtk::Box::new(gtk::Orientation::Vertical, 2);
        sdr.append(&sdr_controls);
        let preview = gtk::CheckButton::with_label("Preview");
        preview.set_tooltip_text(Some("Show the SDR rendition on the canvas"));
        preview.set_widget_name("proof-preview-sdr");
        let method = gtk::DropDown::from_strings(&["Tone map", "Scale", "Clip"]);
        method.set_hexpand(true);
        method.set_widget_name("sdr-appearance-method");
        method.set_tooltip_text(Some("Tone map: browser reference-white curve. Scale: divide by the HDR range. Clip: keep brightness and cut off colors outside SDR."));
        let row = compact_row("Method", &method);
        sdr_controls.append(&row);
        let range_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let controls = [
            ("Exposure", "exposure", -12., 12., 0.1, 2, "EV", 1.),
            ("Contrast", "contrast", 0.25, 4., 0.01, 0, "%", 100.),
            ("HDR range", "headroom", 0., 16., 0.1, 2, "EV", 1.),
        ]
        .map(|(title, name, min, max, step, digits, unit, scale)| {
            let mut spec = NumericControl::number(min, max, step, digits);
            spec.scale = scale;
            spec.unit = unit.into();
            spec.kind = NumericKind::Slider;
            if name == "contrast" {
                spec.resolution = 0.01;
                spec.soft_min = 0.5;
                spec.soft_max = 2.;
            }
            if name == "exposure" {
                spec.soft_min = -4.;
                spec.soft_max = 4.;
            }
            if name == "headroom" {
                spec.soft_min = 0.;
                spec.soft_max = 6.;
            }
            let c = NumberControl::inline(spec, title);
            c.set_widget_name(&format!("sdr-appearance-{name}"));
            let row = compact_row(title, &c);
            if name == "headroom" {
                range_row.append(&row);
                sdr_controls.append(&range_row);
            } else {
                sdr_controls.append(&row);
            }
            c
        });
        controls[2].set_tooltip_text(Some(
            "Brightest input mapped to SDR white, in stops above reference white. Raise to retain stronger highlights; lower for brighter SDR. Default: 2.30 EV (1000 nits).",
        ));
        let compare = gtk::CheckButton::with_label("Compare saved");
        compare.set_widget_name("sdr-appearance-compare");

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let reset = button("Reset", "sdr-appearance-reset");
        let revert = button("Revert", "sdr-appearance-cancel");
        let apply = button("Apply", "sdr-appearance-apply");
        apply.add_css_class("suggested-action");
        let menu_contents = gtk::Box::new(gtk::Orientation::Vertical, 4);
        menu_contents.set_margin_top(6);
        menu_contents.set_margin_bottom(6);
        menu_contents.set_margin_start(6);
        menu_contents.set_margin_end(6);
        reset.set_label("Reset SDR settings");
        reset.add_css_class("flat");
        menu_contents.append(&compare);
        menu_contents.append(&reset);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&menu_contents));
        more.set_popover(Some(&popover));
        preview.set_hexpand(true);
        actions.append(&preview);
        actions.append(&revert);
        actions.append(&apply);
        let sdr_page = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let sdr_scroll = crate::workspace::scroll(&sdr);
        sdr_scroll.set_vexpand(true);
        sdr_page.append(&sdr_scroll);
        sdr_page.append(&actions);
        let back = button("Return to Export", "proof-return-export");
        sdr_page.append(&back);
        stack.add_named(&sdr_page, Some("sdr"));
        let print = gtk::Box::new(gtk::Orientation::Vertical, 4);
        print.set_widget_name("soft-proof-setup");
        let proof_preview = gtk::CheckButton::with_label("Preview");
        proof_preview.set_tooltip_text(Some("Show the saved print proof on the canvas"));
        proof_preview.set_widget_name("proof-preview-print");

        let print_controls = gtk::Box::new(gtk::Orientation::Vertical, 4);
        print.append(&print_controls);
        let chooser = profile::ProfileChooser::new(
            w,
            "Printer & paper",
            "proof-profile",
            space,
            profile::ProfilePurpose::Proof,
        );
        chooser.row.set_title_lines(1);
        chooser.row.set_subtitle_lines(1);
        let group = adw::PreferencesGroup::new();
        group.add(&chooser.row);
        let simulation = choice(
            "Simulate",
            "proof-simulation",
            &["Colors only", "Black ink", "Paper and ink"],
        );
        group.add(&simulation);
        print_controls.append(&group);
        print_controls.append(&chooser.error);
        let advanced = gtk::Popover::new();
        let advanced_group = adw::PreferencesGroup::new();
        let intent = choice(
            "Rendering intent",
            "proof-intent",
            &[
                "Relative colorimetric",
                "Perceptual",
                "Saturation",
                "Absolute colorimetric",
            ],
        );
        advanced_group.add(&intent);
        let bpc = adw::SwitchRow::builder()
            .title("Black point compensation")
            .title_lines(2)
            .active(true)
            .build();
        bpc.set_widget_name("proof-bpc");
        advanced_group.add(&bpc);
        advanced_group.set_margin_top(8);
        advanced_group.set_margin_bottom(8);
        advanced_group.set_margin_start(8);
        advanced_group.set_margin_end(8);
        advanced_group.set_width_request(280);
        advanced.set_child(Some(&advanced_group));
        print_more.set_popover(Some(&advanced));
        let warning = gtk::CheckButton::with_label("Show out-of-gamut colors");
        warning.set_widget_name("proof-gamut-warning");
        if let Some(label) = warning.child().and_downcast::<gtk::Label>() {
            label.set_wrap(true);
            label.set_xalign(0.);
        }
        advanced_group.add(&warning);
        let proof_actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        proof_preview.set_hexpand(true);
        proof_actions.append(&proof_preview);
        let proof_apply = button("Apply", "proof-apply");
        proof_apply.add_css_class("suggested-action");
        let proof_revert = button("Revert", "proof-revert");
        let proof_cancel = button("Cancel preparation", "proof-cancel");
        proof_actions.append(&proof_revert);
        proof_actions.append(&proof_apply);
        let print_page = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let print_scroll = crate::workspace::scroll(&print);
        print_scroll.set_vexpand(true);
        print_page.append(&print_scroll);
        print_page.append(&proof_actions);
        print_page.append(&proof_cancel);
        stack.add_named(&print_page, Some("print"));
        let error = gtk::Label::new(None);
        error.set_wrap(true);
        error.set_xalign(0.);
        error.add_css_class("error");
        error.set_widget_name("proof-setup-error");
        panel.root.append(&error);
        let f = Rc::new(Self {
            stack,
            sdr_tab,
            print_tab,
            controls,
            method,
            range_row,
            sdr_controls,
            sdr_unavailable,
            preview,
            compare,
            apply,
            revert,
            back,
            chooser,
            intent,
            bpc,
            simulation,
            proof_preview,
            warning,
            proof_apply,
            proof_revert,
            proof_cancel,
            print_controls,
            error,
            updating: Cell::new(false),
        });
        for (tab, page) in [(&f.sdr_tab, Page::Sdr), (&f.print_tab, Page::Print)] {
            tab.connect_toggled(glib::clone!(
                #[weak]
                panel,
                #[weak]
                w,
                #[weak]
                f,
                move |tab| {
                    if !f.updating.get() && tab.is_active() {
                        panel.set_page(&w, page);
                    }
                }
            ));
        }
        f.method.connect_selected_notify(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            f,
            move |_| panel.edit_sdr(&w, &f)
        ));
        // The menu contains only SDR actions, including when there is no draft.
        f.stack.connect_visible_child_name_notify(glib::clone!(
            #[weak] more,
            #[weak] print_more,
            move |stack| {
                let sdr = stack.visible_child_name().as_deref() == Some("sdr");
                more.set_visible(sdr);
                print_more.set_visible(!sdr);
            }
        ));
        more.set_visible(
            panel.model.page.get() == Page::Sdr
                && panel
                    .model
                    .identity
                    .get()
                    .is_some_and(|(_, c)| c.depth.is_float()),
        );
        print_more.set_visible(!more.is_visible());
        for control in &f.controls {
            control.connect_value_changed(glib::clone!(
                #[weak]
                panel,
                #[weak]
                w,
                #[weak]
                f,
                move |_| panel.edit_sdr(&w, &f)
            ));
        }
        f.preview.connect_toggled(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            f,
            move |b| {
                if !f.updating.get() {
                    panel.preview_sdr(&w, b.is_active(), f.compare.is_active());
                }
            }
        ));
        f.compare.connect_toggled(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            f,
            move |b| {
                if !f.updating.get() {
                    panel.preview_sdr(&w, true, b.is_active());
                }
            }
        ));
        f.apply.connect_clicked(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            move |_| panel.apply_sdr(&w)
        ));
        f.revert.connect_clicked(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            move |_| panel.revert_sdr(&w)
        ));
        reset.connect_clicked(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            #[weak]
            popover,
            move |_| {
                popover.popdown();
                panel.model.draft.set(Some(SdrRendition::default()));
                panel.preview_sdr(&w, true, false);
            }
        ));
        f.back.connect_clicked(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            move |_| {
                if panel.model.draft.get().is_some() {
                    panel.issue(
                        &w,
                        "Apply or revert the SDR changes before returning to Export.",
                    );
                } else {
                    panel.finish_export();
                    panel.update_all(&w);
                }
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
        f.bpc.connect_active_notify(glib::clone!(
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
        f.proof_apply.connect_clicked(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            move |_| panel.apply_print(&w)
        ));
        f.proof_cancel.connect_clicked(glib::clone!(
            #[weak]
            panel,
            move |_| panel.cancel_job()
        ));
        f.proof_revert.connect_clicked(glib::clone!(
            #[weak]
            panel,
            #[weak]
            w,
            move |_| {
                let previous = panel.model.saved_proof.borrow().clone();
                panel.restore_print(&w, previous);
                // Recreate selectors to clear any profile when the saved proof is empty.
                for view in panel.views() {
                    view.form.borrow_mut().take();
                    while let Some(c) = view.root.first_child() {
                        view.root.remove(&c);
                    }
                    view.ensure(&w);
                }
                panel.update_all(&w);
            }
        ));
        for (toggle, command) in [
            (&f.proof_preview, CommandId::SoftProof),
            (&f.warning, CommandId::GamutWarning),
        ] {
            toggle.connect_toggled(glib::clone!(
                #[weak]
                w,
                #[weak]
                f,
                move |_| {
                    if !f.updating.get() {
                        w.dispatch(UiAction::Invoke { command });
                    }
                }
            ));
        }
        f
    }
}

pub(crate) fn run(w: &Rc<Workspace>) -> Result<(), String> {
    w.proof_panel.open(w, Page::Print)
}
