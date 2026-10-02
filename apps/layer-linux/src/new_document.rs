//! Native creation form. Shared options own validation and preset semantics.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_core::{BlendSpace, color::DocumentColor};
use layer_ui::*;
use std::{cell::{Cell, RefCell}, rc::Rc, sync::Arc};

struct Form {
    localization: Arc<Localizer>,
    view: RefCell<NewDocumentForm>,
    dialog: adw::AlertDialog,
    preset: adw::ComboRow,
    width: adw::SpinRow,
    height: adw::SpinRow,
    background: adw::ComboRow,
    space: adw::ComboRow,
    depth: adw::ComboRow,
    blending: adw::ComboRow,
    blend_space: Cell<BlendSpace>,
    color: adw::ExpanderRow,
    note: gtk::Label,
    remove: gtk::Button,
    updating: Cell<bool>,
}
impl Form {
    fn options(&self) -> NewDocumentOptions {
        let view = self.view.borrow();
        NewDocumentOptions {
            extent: [self.width.value() as u32, self.height.value() as u32],
            color: DocumentColor {
                space: view.spaces.get(self.space.selected() as usize).map_or(view.options.color.space, |choice| choice.0),
                depth: view.depths.get(self.depth.selected() as usize).map_or(view.options.color.depth, |choice| choice.0),
            },
            background: view.backgrounds.get(self.background.selected() as usize).map_or(view.options.background, |choice| choice.0),
            blend_space: self.blend_space.get(),
        }
    }
    fn populate(&self, options: NewDocumentOptions) {
        self.updating.set(true);
        self.width.set_value(f64::from(options.extent[0]));
        self.height.set_value(f64::from(options.extent[1]));
        let view = self.view.borrow();
        self.space.set_selected(view.spaces.iter().position(|choice| choice.0 == options.color.space).unwrap() as u32);
        self.depth.set_selected(view.depths.iter().position(|choice| choice.0 == options.color.depth).unwrap() as u32);
        self.background.set_selected(view.backgrounds.iter().position(|choice| choice.0 == options.background).unwrap() as u32);
        drop(view);
        self.blend_space.set(options.blend_space);
        self.updating.set(false);
        self.describe();
    }
    fn selected_preset(&self) -> Option<NewDocumentPresetView> {
        let index = self.preset.selected().checked_sub(1)? as usize;
        self.view.borrow().presets.get(index).cloned()
    }
    fn describe(&self) {
        let options = self.options();
        let appearance = options.appearance(&self.localization);
        let updating = self.updating.replace(true);
        self.blending.set_selected(self.view.borrow().blending.choices.iter().position(|choice| choice.id == appearance.blending).unwrap() as u32);
        self.updating.set(updating);
        self.blending.set_sensitive(appearance.blending_editable);
        self.blending.set_subtitle(&appearance.blending_help);
        self.color.set_subtitle(&appearance.summary);
        self.note.set_text(appearance.note.as_deref().unwrap_or(""));
        self.note.set_visible(appearance.note.is_some());
        self.dialog.set_response_enabled("create", options.validate().is_ok());
        self.remove.set_sensitive(self.selected_preset().is_some_and(|preset| preset.remove.is_some()));
    }
    fn edited(&self) {
        if !self.updating.get() {
            self.preset.set_selected(0);
            self.describe();
        }
    }
    fn blending_edited(&self) {
        if !self.updating.get() && self.blending.is_sensitive() {
            if let Some(choice) = self.view.borrow().blending.choices.get(self.blending.selected() as usize) {
                self.blend_space.set(choice.id);
            }
            self.edited();
        }
    }
    fn presets(&self, settings: &NewDocumentSettings, selected: Option<NewDocumentPresetId>) {
        self.updating.set(true);
        self.view.replace(settings.form(&self.localization));
        let view = self.view.borrow();
        let names: Vec<_> = std::iter::once(view.text.custom.as_ref()).chain(view.presets.iter().map(|preset| preset.name.as_str())).collect();
        self.preset.set_model(Some(&gtk::StringList::new(&names)));
        self.preset.set_selected(selected.and_then(|id| view.presets.iter().position(|preset| preset.id == id)).map_or(0, |index| index as u32 + 1));
        drop(view);
        self.updating.set(false);
        self.describe();
    }
}

pub(crate) async fn run(w: &Rc<Workspace>) -> Result<Option<layer_core::Project>, String> {
    configure(w, false).await
}

pub(crate) async fn configure(w: &Rc<Workspace>, defaults_only: bool) -> Result<Option<layer_core::Project>, String> {
    let (settings, localization) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or_else(|| NewDocumentError::CanvasUnavailable.message(&w.localization))?.session;
        (session.state().settings.new_document.clone(), session.localization().clone())
    };
    let projection = settings.form(&localization);
    let dialog = adw::AlertDialog::builder()
        .heading(if defaults_only { projection.text.defaults_title.as_ref() } else { projection.text.new_title.as_ref() })
        .content_width(400)
        .build();
    dialog.set_widget_name(if defaults_only { "drawing-defaults-dialog" } else { "new-document-dialog" });
    let group = adw::PreferencesGroup::new();
    let combo = |title: &str, name: &str, choices: &[&str]| {
        let row = adw::ComboRow::builder()
            .title(title)
            .model(&gtk::StringList::new(choices))
            .build();
        row.set_widget_name(name);
        group.add(&row);
        row
    };
    let preset = combo(&projection.text.preset, "new-document-preset", &[&projection.text.custom]);
    let dimension = |title: &str, name: &str| {
        let row = adw::SpinRow::with_range(1., f64::from(MAX_NEW_DOCUMENT_DIMENSION), 1.);
        row.set_title(title);
        row.set_widget_name(name);
        row.set_snap_to_ticks(true);
        row.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
        group.add(&row);
        row
    };
    let width = dimension(&projection.text.width, "new-document-width");
    let height = dimension(&projection.text.height, "new-document-height");
    let background = combo(
        &projection.text.background,
        "new-document-background",
        &projection.backgrounds.iter().map(|choice| choice.1.as_ref()).collect::<Vec<_>>(),
    );
    let space = combo(
        &projection.text.space,
        "new-document-space",
        &projection.spaces.iter().map(|choice| choice.1).collect::<Vec<_>>(),
    );
    let depth = combo(
        &projection.text.depth,
        "new-document-depth",
        &projection.depths.iter().map(|choice| choice.1.as_ref()).collect::<Vec<_>>(),
    );
    let blending = combo(
        &projection.blending.label,
        "new-document-blending",
        &projection.blending.choices.iter().map(|choice| choice.label.as_ref()).collect::<Vec<_>>(),
    );
    blending.set_subtitle_lines(2);
    group.remove(&space);
    group.remove(&depth);
    group.remove(&blending);
    let color = adw::ExpanderRow::builder().title(projection.text.color.as_ref()).build();
    color.set_widget_name("new-document-color");
    color.add_row(&space);
    color.add_row(&depth);
    color.add_row(&blending);
    group.add(&color);
    let note = gtk::Label::builder()
        .wrap(true)
        .xalign(0.)
        .margin_top(8)
        .build();
    note.add_css_class("dim-label");
    let save = gtk::Button::with_label(&projection.text.save_preset);
    save.set_widget_name("new-document-save-preset");
    let remove = gtk::Button::from_icon_name("user-trash-symbolic");
    remove.set_tooltip_text(Some(&projection.text.remove_preset));
    remove.update_property(&[gtk::accessible::Property::Label(&projection.text.remove_preset)]);
    remove.set_widget_name("new-document-remove-preset");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_margin_top(8);
    buttons.append(&save);
    buttons.append(&remove);
    let remember = gtk::CheckButton::with_label(&projection.text.remember);
    remember.set_widget_name("new-document-remember");
    if defaults_only { remember.set_active(true); remember.set_visible(false); }
    remember.set_margin_top(8);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    for child in [group.upcast_ref::<gtk::Widget>(), note.upcast_ref()] {
        content.append(child);
    }
    let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(450)
        .child(&content)
        .build());
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&scroll);
    body.append(&buttons);
    body.append(&remember);
    dialog.set_extra_child(Some(&body));
    dialog.add_responses(&[("cancel", &projection.text.cancel), ("create", if defaults_only { &projection.text.use_defaults } else { &projection.text.create })]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("create"));
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    let selected = projection.selected;
    let form = Rc::new(Form {
        localization,
        view: RefCell::new(projection),
        dialog: dialog.clone(),
        preset,
        width,
        height,
        background,
        space,
        depth,
        blending,
        blend_space: Cell::new(settings.defaults.blend_space),
        color,
        note,
        remove,
        updating: Cell::new(false),
    });
    form.presets(&settings, selected);
    form.populate(settings.defaults);
    for row in [&form.width, &form.height] {
        row.connect_value_notify(glib::clone!(
            #[weak]
            form,
            move |_| form.edited()
        ));
    }
    for row in [&form.background, &form.space, &form.depth] {
        row.connect_selected_notify(glib::clone!(
            #[weak]
            form,
            move |_| form.edited()
        ));
    }
    form.blending.connect_selected_notify(glib::clone!(
        #[weak]
        form,
        move |_| form.blending_edited()
    ));
    form.preset.connect_selected_notify(glib::clone!(
        #[weak]
        form,
        move |_| {
            if form.updating.get() { return; }
            if let Some(preset) = form.selected_preset() { form.populate(preset.options); }
            else { form.describe(); }
        }
    ));
    save.connect_clicked(glib::clone!(
        #[weak]
        form,
        #[weak]
        w,
        move |_| {
            glib::MainContext::default().spawn_local(glib::clone!(
                #[strong]
                form,
                #[strong]
                w,
                async move {
                    let name = adw::EntryRow::builder().title(form.view.borrow().text.preset_name.as_ref()).build();
                    name.set_widget_name("new-document-preset-name");
                    let group = adw::PreferencesGroup::new();
                    group.add(&name);
                    let note = gtk::Label::builder().wrap(true).xalign(0.).build();
                    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
                    content.append(&group);
                    content.append(&note);
                    let dialog = adw::AlertDialog::builder()
                        .heading(form.view.borrow().text.save_preset_title.as_ref())
                        .extra_child(&content)
                        .build();
                    dialog.add_responses(&[("cancel", &form.view.borrow().text.cancel), ("save", &form.view.borrow().text.save)]);
                    dialog.set_close_response("cancel");
                    dialog.set_default_response(Some("save"));
                    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
                    dialog.set_response_enabled("save", false);
                    let options = form.options();
                    name.connect_changed(glib::clone!(
                        #[weak]
                        form,
                        #[weak]
                        w,
                        #[weak]
                        dialog,
                        #[weak]
                        note,
                        move |name| {
                            let mut settings = w
                                .gpu
                                .borrow()
                                .as_ref()
                                .unwrap()
                                .session
                                .state()
                                .settings
                                .new_document
                                .clone();
                            let result = settings.save(&name.text(), options);
                            dialog.set_response_enabled("save", result.is_ok());
                            note.set_text(&result.err().map(|error| error.message(&form.localization)).unwrap_or_default());
                        }
                    ));
                    if crate::alert::choose(dialog, &w.window).await == "save" {
                        let mut settings = w
                            .gpu
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .session
                            .state()
                            .settings
                            .new_document
                            .clone();
                        if settings.save(&name.text(), options).is_ok() {
                            let selected = settings.form(&form.localization).presets.last().map(|preset| preset.id);
                            w.dispatch(UiAction::NewDocumentPreferences {
                                action: NewDocumentAction::Remember { options, name: name.text().into(), defaults: false },
                            });
                            form.presets(&settings, selected);
                        }
                    }
                }
            ));
        }
    ));
    form.remove.connect_clicked(glib::clone!(
        #[weak]
        form,
        #[weak]
        w,
        move |_| {
            // Defer model replacement until the button's native signal has returned.
            glib::idle_add_local_once(glib::clone!(
                #[strong]
                form,
                #[strong]
                w,
                move || {
                    let mut settings = w
                        .gpu
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .session
                        .state()
                        .settings
                        .new_document
                        .clone();
                    if let Some(action) = form.selected_preset().and_then(|preset| preset.remove) {
                        settings.apply(action.clone()).unwrap();
                        w.dispatch(UiAction::NewDocumentPreferences { action });
                        form.presets(&settings, None);
                    }
                }
            ));
        }
    ));
    if crate::alert::choose(dialog, &w.window).await != "create" {
        return Ok(None);
    }
    let options = form.options();
    let project = options.project(&form.localization)?;
    if remember.is_active() {
        w.dispatch(UiAction::NewDocumentPreferences {
            action: NewDocumentAction::Remember { options, name: String::new(), defaults: true },
        });
    }
    Ok(Some(project))
}
