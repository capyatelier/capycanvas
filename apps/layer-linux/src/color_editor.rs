//! Native numeric color drafts. Shared Rust owns parsing, conversion and the
//! palette definitions; this sheet publishes one accepted paint change.
use crate::display_color::{ColorPatch, ViewColor};
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_core::color::RgbSpace;
use layer_ui::{ColorAction, ColorEditor, ColorInputModel, ColorSlot, UiAction};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Form {
    editor: RefCell<ColorEditor>,
    updating: Cell<bool>,
    dialog: adw::AlertDialog,
    model: adw::ComboRow,
    fields: [adw::EntryRow; 4],
    description: gtk::Label,
    validation: gtk::Label,
    preview: ColorPatch,
    view: ViewColor,
    space: RgbSpace,
}
impl Form {
    fn populate(&self) {
        self.updating.set(true);
        let editor = self.editor.borrow();
        let labels = editor.model().labels();
        self.model.set_selected(
            ColorInputModel::ALL
                .iter()
                .position(|m| *m == editor.model())
                .unwrap() as u32,
        );
        for (i, row) in self.fields.iter().enumerate() {
            row.set_title(labels[i]);
            row.set_visible(!labels[i].is_empty());
            row.set_text(&editor.fields()[i]);
        }
        self.description.set_text(&editor.description());
        drop(editor);
        self.updating.set(false);
        self.refresh_preview();
    }
    fn refresh_preview(&self) {
        match self.editor.borrow().color() {
            Ok(color) => {
                let mut text = format!("Defined in {}", color.space.name());
                if !color.in_gamut(self.space).unwrap() {
                    text.push_str(" · Outside document gamut");
                }
                if !color.in_gamut(self.view.space()).unwrap() {
                    text.push_str(&format!(" · Outside {} preview gamut", self.view.space().name()));
                }
                self.validation.remove_css_class("error");
                self.validation.set_text(&text);
                self.dialog.set_response_enabled("apply", true);
                self.preview.set_color(color, self.view);
            }
            Err(error) => {
                self.validation.add_css_class("error");
                self.validation.set_text(&error);
                self.dialog.set_response_enabled("apply", false);
            }
        }
        self.preview.queue_draw();
    }
}

pub fn show(workspace: &Rc<Workspace>, slot: ColorSlot) {
    let Some((colors, epoch)) = workspace.gpu.borrow().as_ref().map(|g| {
        (
            g.session.state().colors.clone(),
            g.session.state().document_file.epoch,
        )
    }) else {
        return;
    };
    let definition = match slot {
        ColorSlot::Foreground => colors.foreground,
        ColorSlot::Background => colors.background,
        ColorSlot::Transparent => return,
    };
    let space = colors.rgb_space();
    let editor = match ColorEditor::new(definition, space) {
        Ok(editor) => editor,
        Err(error) => {
            workspace.changed(Err(error));
            return;
        }
    };
    let dialog = adw::AlertDialog::builder()
        .heading("Edit Color")
        .content_width(400)
        .build();
    dialog.set_widget_name("edit-color-dialog");
    dialog.add_responses(&[("cancel", "Cancel"), ("apply", "Use Color")]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("apply"));
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let group = adw::PreferencesGroup::new();
    let model = adw::ComboRow::builder().title("Model").build();
    model.set_widget_name("edit-color-model");
    model.set_model(Some(&gtk::StringList::new(
        &ColorInputModel::ALL.map(ColorInputModel::name),
    )));
    group.add(&model);
    let fields = std::array::from_fn(|i| {
        let row = adw::EntryRow::new();
        row.set_widget_name(&format!("edit-color-value-{i}"));
        group.add(&row);
        row
    });
    let description = gtk::Label::builder().wrap(true).xalign(0.).build();
    description.add_css_class("dim-label");
    let validation = gtk::Label::builder().wrap(true).xalign(0.).build();
    validation.set_widget_name("edit-color-validation");
    let preview = ColorPatch::new(false);
    preview.set_height_request(48);
    preview.set_widget_name("edit-color-preview");
    content.append(&description);
    content.append(&group);
    content.append(&preview);
    content.append(&validation);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(540)
        .child(&content)
        .build();
    dialog.set_extra_child(Some(&scroll));
    let form = Rc::new(Form {
        editor: RefCell::new(editor),
        updating: Cell::new(false),
        dialog,
        model,
        fields,
        description,
        validation,
        preview,
        view: workspace.view_color(),
        space,
    });
    for (i, row) in form.fields.iter().enumerate() {
        let weak = Rc::downgrade(&form);
        row.connect_changed(move |row| {
            let Some(form) = weak.upgrade() else {
                return;
            };
            if form.updating.get() {
                return;
            }
            let result = form.editor.borrow_mut().set_field(i, row.text().into());
            if let Err(error) = result {
                form.validation.set_text(&error);
                form.dialog.set_response_enabled("apply", false);
            } else {
                form.refresh_preview();
            }
        });
    }
    let weak = Rc::downgrade(&form);
    form.model.connect_selected_notify(move |row| {
        let Some(form) = weak.upgrade() else {
            return;
        };
        if form.updating.get() {
            return;
        }
        let Some(model) = ColorInputModel::ALL.get(row.selected() as usize) else {
            return;
        };
        let result = form.editor.borrow_mut().set_model(*model);
        if result.is_ok() {
            form.populate();
        } else {
            form.updating.set(true);
            row.set_selected(
                ColorInputModel::ALL
                    .iter()
                    .position(|m| *m == form.editor.borrow().model())
                    .unwrap() as u32,
            );
            form.updating.set(false);
            form.refresh_preview();
        }
    });
    form.populate();
    glib::MainContext::default().spawn_local(glib::clone!(
        #[weak]
        workspace,
        async move {
            if form
                .dialog
                .clone()
                .choose_future(Some(&workspace.window))
                .await
                != "apply"
            {
                return;
            }
            let current_epoch = workspace
                .gpu
                .borrow()
                .as_ref()
                .map(|g| g.session.state().document_file.epoch);
            if current_epoch != Some(epoch) {
                workspace.changed(Err("The document changed; reopen Edit Color".into()));
                return;
            }
            match form.editor.borrow().color() {
                Ok(color) => workspace.dispatch(UiAction::Color {
                    action: ColorAction::SetSlot { slot, color },
                }),
                Err(error) => workspace.changed(Err(error)),
            }
        }
    ));
}
