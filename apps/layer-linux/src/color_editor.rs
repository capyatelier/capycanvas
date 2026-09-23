//! Native numeric color drafts. Shared Rust owns parsing, conversion and the
//! palette/effect definitions; this sheet publishes one accepted color change.
use crate::display_color::{ColorPatch, ViewColor};
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_core::color::{RgbColor, RgbSpace};
use layer_ui::{ColorAction, ColorEditor, ColorInputModel, ColorSlot, UiAction};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub(crate) struct Form {
    editor: RefCell<ColorEditor>,
    updating: Cell<bool>,
    dialog: adw::AlertDialog,
    model: adw::ComboRow,
    fields: [adw::EntryRow; 4],
    intensity: adw::EntryRow,
    description: gtk::Label,
    validation: gtk::Label,
    preview: ColorPatch,
    base_preview: ColorPatch,
    view: Cell<ViewColor>,
    headroom: Cell<f32>,
    space: RgbSpace,
    hdr: bool,
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
        let view = self.view.get();
        let color: Result<(RgbColor, RgbColor), String> = (|| {
            let editor = self.editor.borrow();
            if self.hdr {
                let stops = self.intensity.text().trim().parse::<f32>().map_err(|_| "Enter a finite EV value".to_string())?;
                if !stops.is_finite() || editor.intensity() != Some(stops) {
                    return Err("Enter an EV value within the document’s color range".into());
                }
            }
            Ok((editor.color()?, editor.base_color()?))
        })();
        match color {
            Ok((color, base)) => {
                let text = layer_ui::color_validation(color, self.space, view.space(), self.hdr).unwrap();
                self.validation.remove_css_class("error");
                self.validation.set_text(&text);
                self.dialog.set_response_enabled("apply", true);
                self.preview.set_display_color(color, view, self.headroom.get());
                self.base_preview.set_display_color(base, view, self.headroom.get());
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

/// Capability changes refresh live drafts without publishing a color edit.
pub(crate) fn refresh_display(workspace: &Workspace) {
    let view = workspace.view_color();
    let headroom = workspace.picker_headroom();
    workspace.color_editors.borrow_mut().retain(|weak| {
        let Some(form) = weak.upgrade() else { return false; };
        let previous_view = form.view.replace(view);
        let previous_headroom = form.headroom.replace(headroom);
        if previous_view != view || previous_headroom != headroom { form.refresh_preview(); }
        true
    });
}

pub fn show(workspace: &Rc<Workspace>, slot: ColorSlot) {
    let Some(colors) = workspace
        .gpu
        .borrow()
        .as_ref()
        .map(|g| g.session.state().display_colors().clone())
    else {
        return;
    };
    let definition = match slot {
        ColorSlot::Foreground => colors.foreground,
        ColorSlot::Background => colors.background,
        ColorSlot::Transparent => return,
    };
    let mut selected = colors.clone();
    selected.apply(ColorAction::Select { slot }).unwrap();
    choose_with_intensity(workspace, definition, Some(selected.hdr_intensity()), move |workspace, color, intensity| {
        workspace.dispatch(UiAction::Color {
            action: if let Some(stops) = intensity { ColorAction::SetSlotIntensity { slot, color, stops } }
                else { ColorAction::SetSlot { slot, color } },
        });
    });
}

pub fn choose(
    workspace: &Rc<Workspace>,
    definition: RgbColor,
    accepted: impl FnOnce(&Rc<Workspace>, RgbColor) + 'static,
) {
    choose_with_intensity(workspace, definition, None, move |w, color, _| accepted(w, color));
}
fn choose_with_intensity(
    workspace: &Rc<Workspace>,
    definition: RgbColor,
    intensity: Option<f32>,
    accepted: impl FnOnce(&Rc<Workspace>, RgbColor, Option<f32>) + 'static,
) {
    let Some((space, epoch, depth)) = workspace.gpu.borrow().as_ref().map(|g| {
        (
            g.session.state().display_colors().rgb_space(),
            g.session.state().document_file.epoch,
            g.session.engine().document().color.depth,
        )
    }) else {
        return;
    };
    let mut editor = match ColorEditor::new(definition, space) {
        Ok(editor) => editor,
        Err(error) => {
            workspace.changed(Err(error));
            return;
        }
    };
    let hdr = depth.is_float();
    editor.set_document_depth(depth);
    if hdr {
        editor.set_model(ColorInputModel::LinearRgb).unwrap();
        let stops = intensity.unwrap_or_else(|| definition.brightness_ev(space).ok().flatten().unwrap_or(0.).max(0.));
        if let Err(error) = editor.enable_hdr(stops) { workspace.changed(Err(error)); return; }
    }
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
    let intensity = adw::EntryRow::builder().title("Intensity (EV)").build();
    intensity.set_widget_name("edit-color-ev");
    intensity.set_visible(hdr);
    if hdr { intensity.set_text(&editor.intensity().unwrap().to_string()); }
    group.add(&intensity);
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
    preview.update_property(&[gtk::accessible::Property::Label("EV-adjusted color")]);
    let base_preview = ColorPatch::new(false);
    base_preview.set_height_request(48);
    base_preview.set_widget_name("edit-color-base-preview");
    base_preview.update_property(&[gtk::accessible::Property::Label("Color before EV adjustment")]);
    base_preview.set_visible(hdr);
    let colors = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    colors.set_homogeneous(true);
    colors.append(&base_preview);
    colors.append(&preview);
    let labels = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    labels.set_homogeneous(true);
    labels.add_css_class("caption");
    labels.add_css_class("dim-label");
    labels.append(&gtk::Label::new(Some("Base")));
    labels.append(&gtk::Label::new(Some("Adjusted")));
    labels.set_visible(hdr);
    let comparison = gtk::Box::new(gtk::Orientation::Vertical, 4);
    comparison.set_widget_name("edit-color-comparison");
    comparison.append(&labels);
    comparison.append(&colors);
    content.append(&description);
    if hdr { content.append(&comparison); }
    content.append(&group);
    if !hdr { content.append(&comparison); }
    content.append(&validation);
    let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(540)
        .child(&content)
        .build());
    dialog.set_extra_child(Some(&scroll));
    let form = Rc::new(Form {
        editor: RefCell::new(editor),
        updating: Cell::new(false),
        dialog,
        model,
        fields,
        intensity,
        description,
        validation,
        preview,
        base_preview,
        view: Cell::new(workspace.view_color()),
        headroom: Cell::new(workspace.picker_headroom()),
        space,
        hdr,
    });
    workspace.color_editors.borrow_mut().push(Rc::downgrade(&form));
    refresh_display(workspace);
    let weak = Rc::downgrade(&form);
    form.intensity.connect_changed(move |row| {
        let Some(form) = weak.upgrade() else { return; };
        if form.updating.get() { return; }
        let result = row.text().trim().parse::<f32>().map_err(|_| "Enter a finite EV value".to_string())
            .and_then(|stops| form.editor.borrow_mut().set_intensity(stops));
        match result {
            Ok(()) => form.populate(),
            Err(error) => {
                form.validation.add_css_class("error");
                form.validation.set_text(&error);
                form.dialog.set_response_enabled("apply", false);
            }
        }
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
            if crate::alert::choose(form.dialog.clone(), &workspace.window)
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
                Ok(color) => accepted(&workspace, color, form.editor.borrow().intensity()),
                Err(error) => workspace.changed(Err(error)),
            }
        }
    ));
}

/// Retained native button with an explicitly tagged artwork patch. Refreshing
/// coordinates never emits an edit; only accepting a live draft publishes one.
pub struct ColorButton {
    pub widget: gtk::Button,
    definition: Cell<RgbColor>,
    patch: ColorPatch,
}
impl ColorButton {
    pub fn new() -> Rc<Self> {
        let patch = ColorPatch::new(false);
        patch.set_size_request(32, 20);
        let widget = gtk::Button::builder()
            .child(&patch)
            .tooltip_text("Edit Color")
            .build();
        Rc::new(Self {
            widget,
            definition: Cell::new(RgbColor::BLACK),
            patch,
        })
    }
    pub fn color(&self) -> RgbColor {
        self.definition.get()
    }
    pub fn set_color(&self, color: RgbColor, view: ViewColor) {
        self.set_display_color(color, view, 1.);
    }
    pub fn set_display_color(&self, color: RgbColor, view: ViewColor, headroom: f32) {
        self.definition.set(color);
        self.patch.set_display_color(color, view, headroom);
    }
    pub fn bind(
        self: &Rc<Self>,
        workspace: &Rc<Workspace>,
        accepted: impl Fn(&Rc<Workspace>, RgbColor) + 'static,
    ) {
        let weak = Rc::downgrade(self);
        let workspace = Rc::downgrade(workspace);
        let accepted = Rc::new(accepted);
        self.widget.connect_clicked(move |_| {
            let (Some(button), Some(workspace)) = (weak.upgrade(), workspace.upgrade()) else {
                return;
            };
            let original = button.color();
            let weak = weak.clone();
            let accepted = accepted.clone();
            choose(&workspace, original, move |workspace, color| {
                let Some(button) = weak.upgrade() else {
                    return;
                };
                if button.widget.root().is_some() && button.color() == original {
                    accepted(workspace, color);
                }
            });
        });
    }
}
