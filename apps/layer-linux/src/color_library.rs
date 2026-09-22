//! Native palette management, backed by shared workspace color definitions.
use crate::{display_color::ColorPatch, workspace::Workspace};
use adw::prelude::*;
use gtk::glib;
use layer_ui::{ColorAction, ColorLibraryAction as Action, ColorSlot, UiAction};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

struct Library {
    workspace: Weak<Workspace>,
    dialog: adw::AlertDialog,
    combo: adw::ComboRow,
    rows: adw::PreferencesGroup,
    items: RefCell<Vec<adw::ActionRow>>,
    selected: Cell<u64>,
    updating: Cell<bool>,
    refresh_pending: Cell<bool>,
    model_items: RefCell<Vec<(u64, String)>>,
    validation: gtk::Label,
    slot: ColorSlot,
}
impl Library {
    fn state(&self) -> Option<layer_ui::ColorState> {
        let workspace = self.workspace.upgrade()?;
        let colors = workspace
            .gpu
            .borrow()
            .as_ref()?
            .session
            .state()
            .colors
            .clone();
        Some(colors)
    }
    fn apply(self: &Rc<Self>, action: Action) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let created_palette = matches!(&action, Action::CreatePalette { .. });
        let selected_color = if let Action::Use { id } = &action {
            self.state()
                .and_then(|s| s.library.swatch(*id).map(|s| s.color))
        } else {
            None
        };
        let action = if let Some(color) = selected_color {
            ColorAction::SetSlot {
                slot: self.slot,
                color,
            }
        } else {
            ColorAction::Library { action }
        };
        let result = workspace
            .gpu
            .borrow_mut()
            .as_mut()
            .map(|g| g.session.dispatch(UiAction::Color { action }));
        if let Some(result) = result {
            match result {
                Ok(change) => {
                    workspace.changed(Ok(change));
                    self.validation.set_text("");
                    if created_palette {
                        if let Some(id) = self
                            .state()
                            .and_then(|c| c.library.palettes.last().map(|p| p.id))
                        {
                            self.selected.set(id);
                        }
                    }
                    if selected_color.is_some() {
                        self.dialog.close();
                    } else {
                        self.queue_refresh();
                    }
                }
                Err(error) => self.validation.set_text(&error),
            }
        }
    }
    fn queue_refresh(self: &Rc<Self>) {
        if self.refresh_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(library) = weak.upgrade() {
                library.refresh_pending.set(false);
                library.refresh();
            }
        });
    }
    fn refresh(self: &Rc<Self>) {
        let Some(colors) = self.state() else {
            return;
        };
        self.updating.set(true);
        let view = self.workspace.upgrade().map_or(Default::default(), |w| w.view_color());
        let library = &colors.library;
        let items: Vec<_> = library
            .palettes
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect();
        // Replacing a ComboRow model inside its selection notification can
        // reenter GTK's selector. Only library structure/name changes replace
        // this model, and signal-driven refreshes run after the signal returns.
        if *self.model_items.borrow() != items {
            self.combo.set_model(Some(&gtk::StringList::new(
                &items
                    .iter()
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>(),
            )));
            *self.model_items.borrow_mut() = items;
        }
        let index = library
            .palettes
            .iter()
            .position(|p| p.id == self.selected.get())
            .unwrap_or(0);
        self.selected.set(library.palettes[index].id);
        self.combo.set_selected(index as u32);
        for row in self.items.take() {
            self.rows.remove(&row);
        }
        for swatch in &library.palettes[index].swatches {
            let row = adw::ActionRow::builder()
                .title(&swatch.name)
                .activatable(true)
                .build();
            row.set_use_markup(false);
            row.set_widget_name(&format!("saved-color-{}", swatch.id));
            let mut detail = swatch.color.space.name().to_string();
            if !swatch.color.in_gamut(view.space()).unwrap() {
                detail.push_str(&format!(" · Outside {} preview gamut", view.space().name()));
            }
            row.set_tooltip_text(Some(&detail));
            let subtitle = format!(
                "{}{}",
                swatch.color.space.name(),
                if swatch.color.in_gamut(view.space()).unwrap() {
                    ""
                } else {
                    " · !"
                }
            );
            row.set_subtitle(&subtitle);
            row.upcast_ref::<gtk::Widget>()
                .update_property(&[gtk::accessible::Property::Description(&detail)]);
            row.set_subtitle_lines(1);
            let color = swatch.color;
            let preview = ColorPatch::new(false);
            preview.set_size_request(36, 28);
            preview.set_valign(gtk::Align::Center);
            preview.set_color(color, view);
            row.add_prefix(&preview);
            let weak = Rc::downgrade(self);
            let id = swatch.id;
            row.connect_activated(move |_| {
                if let Some(library) = weak.upgrade() {
                    library.apply(Action::Use { id });
                }
            });
            for (label, rename) in [("Rename", true), ("Remove", false)] {
                let button = gtk::Button::from_icon_name(if rename {
                    "document-edit-symbolic"
                } else {
                    "edit-delete-symbolic"
                });
                let description = format!("{label} {}", swatch.name);
                button.set_tooltip_text(Some(&description));
                button.update_property(&[gtk::accessible::Property::Label(&description)]);
                button.set_valign(gtk::Align::Center);
                button.set_widget_name(&format!("saved-color-{}-{label}", id));
                let weak = Rc::downgrade(self);
                let name = swatch.name.clone();
                button.connect_clicked(move |_| {
                    let Some(library) = weak.upgrade() else {
                        return;
                    };
                    if rename {
                        let id = id;
                        library.ask_name("Rename Swatch", &name, move |name| Action::Rename {
                            id,
                            name,
                        });
                    } else {
                        library.apply(Action::Remove { id });
                    }
                });
                row.add_suffix(&button);
            }
            self.rows.add(&row);
            self.items.borrow_mut().push(row);
        }
        self.rows
            .set_description(Some(if library.palettes[index].swatches.is_empty() {
                "This palette is empty. Save the current color to reuse it in any document."
            } else {
                "Choose a swatch to use its color. Its original color space is retained."
            }));
        self.updating.set(false);
    }
    fn ask_name(
        self: &Rc<Self>,
        heading: &str,
        initial: &str,
        action: impl Fn(String) -> Action + 'static,
    ) {
        let Some(colors) = self.state() else {
            return;
        };
        let dialog = adw::AlertDialog::builder().heading(heading).build();
        dialog.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
        dialog.set_close_response("cancel");
        dialog.set_default_response(Some("save"));
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        let group = adw::PreferencesGroup::new();
        let name = adw::EntryRow::builder().title("Name").text(initial).build();
        name.set_widget_name("color-library-name");
        group.add(&name);
        dialog.set_extra_child(Some(&group));
        let action: Rc<dyn Fn(String) -> Action> = Rc::new(action);
        let validate = {
            let library = colors.library;
            let action = action.clone();
            glib::clone!(
                #[weak]
                dialog,
                #[weak]
                group,
                move |name: &adw::EntryRow| {
                    let result = library.clone().apply(action(name.text().into()));
                    dialog.set_response_enabled("save", result.is_ok());
                    group.set_description(result.err().as_deref());
                }
            )
        };
        validate(&name);
        name.connect_changed(validate);
        let library = self.clone();
        glib::MainContext::default().spawn_local(async move {
            if crate::alert::choose(dialog, &library.dialog).await == "save" {
                library.apply(action(name.text().into()));
            }
        });
    }
}

pub fn show(workspace: &Rc<Workspace>, slot: ColorSlot) {
    if slot == ColorSlot::Transparent {
        return;
    }
    let dialog = adw::AlertDialog::builder()
        .heading("Color Swatches")
        .build();
    dialog.set_widget_name("color-library-dialog");
    dialog.add_response("close", "Close");
    dialog.set_close_response("close");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let group = adw::PreferencesGroup::new();
    let combo = adw::ComboRow::builder().title("Palette").build();
    combo.set_use_markup(false);
    combo.set_widget_name("color-library-palette");
    group.add(&combo);
    content.append(&group);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let add = gtk::Button::with_label("Save Current Color…");
    add.set_widget_name("color-library-store");
    buttons.append(&add);
    let new = gtk::Button::with_label("New Palette…");
    new.set_widget_name("color-library-create");
    buttons.append(&new);
    content.append(&buttons);
    let manage = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let rename = gtk::Button::with_label("Rename Palette…");
    let remove = gtk::Button::with_label("Remove Palette…");
    manage.append(&rename);
    manage.append(&remove);
    content.append(&manage);
    let validation = gtk::Label::builder().wrap(true).xalign(0.).build();
    validation.add_css_class("error");
    content.append(&validation);
    let rows = adw::PreferencesGroup::new();
    content.append(&rows);
    let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(540)
        .child(&content)
        .build());
    dialog.set_extra_child(Some(&scroll));
    let library = Rc::new(Library {
        workspace: Rc::downgrade(workspace),
        dialog,
        combo,
        rows,
        items: Default::default(),
        selected: Cell::new(1),
        updating: Cell::new(false),
        refresh_pending: Cell::new(false),
        model_items: Default::default(),
        validation,
        slot,
    });
    let weak = Rc::downgrade(&library);
    library.combo.connect_selected_notify(move |row| {
        let Some(library) = weak.upgrade() else {
            return;
        };
        if library.updating.get() {
            return;
        }
        if let Some(palette) = library
            .state()
            .and_then(|c| c.library.palettes.get(row.selected() as usize).cloned())
        {
            if library.selected.replace(palette.id) != palette.id {
                library.queue_refresh();
            }
        }
    });
    let weak = Rc::downgrade(&library);
    add.connect_clicked(move |_| {
        let Some(library) = weak.upgrade() else {
            return;
        };
        let Some(colors) = library.state() else {
            return;
        };
        let palette = library.selected.get();
        let color = if library.slot == ColorSlot::Background {
            colors.background
        } else {
            colors.foreground
        };
        library.ask_name("Save Swatch", "New color", move |name| Action::Store {
            palette,
            name,
            color,
        });
    });
    let weak = Rc::downgrade(&library);
    new.connect_clicked(move |_| {
        if let Some(library) = weak.upgrade() {
            library.ask_name("New Palette", "", |name| Action::CreatePalette { name });
        }
    });
    let weak = Rc::downgrade(&library);
    rename.connect_clicked(move |_| {
        let Some(library) = weak.upgrade() else {
            return;
        };
        let Some(colors) = library.state() else {
            return;
        };
        let id = library.selected.get();
        let Some(palette) = colors.library.palettes.iter().find(|p| p.id == id) else {
            return;
        };
        library.ask_name("Rename Palette", &palette.name, move |name| {
            Action::RenamePalette { id, name }
        });
    });
    let weak = Rc::downgrade(&library);
    remove.connect_clicked(move |_| {
        let Some(library) = weak.upgrade() else {
            return;
        };
        let Some(colors) = library.state() else {
            return;
        };
        let id = library.selected.get();
        let Some(palette) = colors.library.palettes.iter().find(|p| p.id == id) else {
            return;
        };
        if colors.library.palettes.len() == 1 {
            library.validation.set_text("Keep at least one palette");
            return;
        }
        let confirm = adw::AlertDialog::builder()
            .heading("Remove Palette?")
            .body(format!(
                "Remove “{}” and its {} swatches?",
                palette.name,
                palette.swatches.len()
            ))
            .build();
        confirm.add_responses(&[("cancel", "Cancel"), ("remove", "Remove")]);
        confirm.set_close_response("cancel");
        confirm.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        glib::MainContext::default().spawn_local(async move {
            if crate::alert::choose(confirm, &library.dialog).await == "remove" {
                library.apply(Action::RemovePalette { id });
            }
        });
    });
    library.refresh();
    glib::MainContext::default().spawn_local(glib::clone!(
        #[weak]
        workspace,
        async move {
            crate::alert::choose(library.dialog.clone(), &workspace.window)
                .await;
        }
    ));
}
