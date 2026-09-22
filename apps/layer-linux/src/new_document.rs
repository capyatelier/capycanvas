//! Native creation form. Shared options own validation and preset semantics.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
use layer_ui::*;
use std::{cell::Cell, rc::Rc};

struct Form {
    dialog: adw::AlertDialog,
    preset: adw::ComboRow,
    width: adw::SpinRow,
    height: adw::SpinRow,
    background: adw::ComboRow,
    space: adw::ComboRow,
    depth: adw::ComboRow,
    color: adw::ExpanderRow,
    note: gtk::Label,
    remove: gtk::Button,
    updating: Cell<bool>,
}
impl Form {
    fn options(&self) -> NewDocumentOptions {
        NewDocumentOptions {
            extent: [self.width.value() as u32, self.height.value() as u32],
            color: DocumentColor {
                space: RgbSpace::ALL[self.space.selected().min(3) as usize],
                depth: [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32][self.depth.selected().min(3) as usize],
            },
            background: if self.background.selected() == 0 {
                DocumentBackground::White
            } else {
                DocumentBackground::Transparent
            },
        }
    }
    fn populate(&self, options: NewDocumentOptions) {
        self.updating.set(true);
        self.width.set_value(f64::from(options.extent[0]));
        self.height.set_value(f64::from(options.extent[1]));
        self.space.set_selected(
            RgbSpace::ALL
                .iter()
                .position(|s| *s == options.color.space)
                .unwrap() as u32,
        );
        self.depth
            .set_selected(match options.color.depth { SampleDepth::U8 => 0, SampleDepth::U16 => 1, SampleDepth::F16 => 2, SampleDepth::F32 => 3 });
        self.background.set_selected(u32::from(
            options.background == DocumentBackground::Transparent,
        ));
        self.updating.set(false);
        self.describe();
    }
    fn describe(&self) {
        let options = self.options();
        self.color.set_subtitle(&format!(
            "{} · {}",
            options.color.space.name(),
            options.color.depth.label()
        ));
        self.note
            .set_text("16-bit SDR is recommended for ProPhoto gradients and photo adjustments.");
        self.note.set_visible(
            options.color.space == RgbSpace::ProPhoto && options.color.depth == SampleDepth::U8,
        );
        self.dialog
            .set_response_enabled("create", options.validate().is_ok());
        self.remove.set_sensitive(
            self.preset.selected() >= 5 && self.preset.selected() != gtk::INVALID_LIST_POSITION,
        );
    }
    fn edited(&self) {
        if !self.updating.get() {
            self.preset.set_selected(0);
            self.describe();
        }
    }
    fn presets(&self, settings: &NewDocumentSettings, selected: u32) {
        self.updating.set(true);
        let mut names = vec!["Custom".to_string()];
        names.extend(NewDocumentPreset::builtins_for(layer_ui::Platform::Gtk).into_iter().map(|p| p.name));
        names.extend(settings.presets.iter().map(|p| p.name.clone()));
        self.preset.set_model(Some(&gtk::StringList::new(
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        )));
        self.preset.set_selected(selected);
        self.updating.set(false);
        self.describe();
    }
}

pub(crate) async fn run(w: &Rc<Workspace>) -> Result<Option<layer_core::Project>, String> {
    configure(w, false).await
}

pub(crate) async fn configure(w: &Rc<Workspace>, defaults_only: bool) -> Result<Option<layer_core::Project>, String> {
    let settings = w
        .gpu
        .borrow()
        .as_ref()
        .ok_or("Canvas unavailable")?
        .session
        .state()
        .settings
        .new_document
        .clone();
    let dialog = adw::AlertDialog::builder()
        .heading(if defaults_only { "Drawing defaults and presets" } else { "New drawing" })
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
    let preset = combo("Preset", "new-document-preset", &["Custom"]);
    let dimension = |title: &str, name: &str| {
        let row = adw::SpinRow::with_range(1., f64::from(MAX_NEW_DOCUMENT_DIMENSION), 1.);
        row.set_title(title);
        row.set_widget_name(name);
        row.set_snap_to_ticks(true);
        row.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
        group.add(&row);
        row
    };
    let width = dimension(DOCUMENT_WIDTH_LABEL, "new-document-width");
    let height = dimension(DOCUMENT_HEIGHT_LABEL, "new-document-height");
    let background = combo(
        "Background",
        "new-document-background",
        &["White", "Transparent"],
    );
    let space = combo(
        "Color space",
        "new-document-space",
        &RgbSpace::ALL.map(|s| s.name()),
    );
    let depth = combo(
        "Bit depth",
        "new-document-depth",
        &["8-bit SDR", "16-bit SDR", "16-bit float HDR", "32-bit float HDR"],
    );
    group.remove(&space);
    group.remove(&depth);
    let color = adw::ExpanderRow::builder().title("Color").build();
    color.set_widget_name("new-document-color");
    color.add_row(&space);
    color.add_row(&depth);
    group.add(&color);
    let note = gtk::Label::builder()
        .wrap(true)
        .xalign(0.)
        .margin_top(8)
        .build();
    note.add_css_class("dim-label");
    let save = gtk::Button::with_label("Save Preset…");
    save.set_widget_name("new-document-save-preset");
    let remove = gtk::Button::from_icon_name("user-trash-symbolic");
    remove.set_tooltip_text(Some("Remove saved preset"));
    remove.update_property(&[gtk::accessible::Property::Label("Remove saved preset")]);
    remove.set_widget_name("new-document-remove-preset");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_margin_top(8);
    buttons.append(&save);
    buttons.append(&remove);
    let remember = gtk::CheckButton::with_label("Use these settings for new drawings");
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
    dialog.add_responses(&[("cancel", CANCEL_DOCUMENT_LABEL), ("create", if defaults_only { "Use Defaults" } else { "Create" })]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("create"));
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    let form = Rc::new(Form {
        dialog: dialog.clone(),
        preset,
        width,
        height,
        background,
        space,
        depth,
        color,
        note,
        remove,
        updating: Cell::new(false),
    });
    let selected = NewDocumentPreset::builtins_for(layer_ui::Platform::Gtk)
        .iter()
        .chain(settings.presets.iter())
        .position(|p| p.options == settings.defaults)
        .map_or(0, |i| i as u32 + 1);
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
    form.preset.connect_selected_notify(glib::clone!(
        #[weak]
        form,
        #[weak]
        w,
        move |row| {
            if form.updating.get() {
                return;
            }
            let selected = row.selected();
            let settings = w
                .gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .state()
                .settings
                .new_document
                .clone();
            if selected > 0 {
                if let Some(preset) = NewDocumentPreset::builtins_for(layer_ui::Platform::Gtk)
                    .iter()
                    .chain(settings.presets.iter())
                    .nth(selected as usize - 1)
                {
                    form.populate(preset.options);
                }
            }
            form.describe();
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
                    let name = adw::EntryRow::builder().title("Preset name").build();
                    name.set_widget_name("new-document-preset-name");
                    let group = adw::PreferencesGroup::new();
                    group.add(&name);
                    let note = gtk::Label::builder().wrap(true).xalign(0.).build();
                    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
                    content.append(&group);
                    content.append(&note);
                    let dialog = adw::AlertDialog::builder()
                        .heading("Save Drawing Preset")
                        .extra_child(&content)
                        .build();
                    dialog.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
                    dialog.set_close_response("cancel");
                    dialog.set_default_response(Some("save"));
                    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
                    dialog.set_response_enabled("save", false);
                    let options = form.options();
                    name.connect_changed(glib::clone!(
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
                            note.set_text(result.as_ref().err().map_or("", String::as_str));
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
                            let selected = settings.presets.len() as u32 + 4;
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
                    if let Some(index) = form
                        .preset
                        .selected()
                        .checked_sub(5)
                        .map(|v| v as usize)
                        .filter(|i| *i < settings.presets.len())
                    {
                        settings.apply(NewDocumentAction::Remove { index }).unwrap();
                        w.dispatch(UiAction::NewDocumentPreferences {
                            action: NewDocumentAction::Remove { index },
                        });
                        form.presets(&settings, 0);
                    }
                }
            ));
        }
    ));
    if crate::alert::choose(dialog, &w.window).await != "create" {
        return Ok(None);
    }
    let options = form.options();
    let project = options.project()?;
    if remember.is_active() {
        w.dispatch(UiAction::NewDocumentPreferences {
            action: NewDocumentAction::Remember { options, name: String::new(), defaults: true },
        });
    }
    Ok(Some(project))
}
