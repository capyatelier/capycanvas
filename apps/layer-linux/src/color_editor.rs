//! Native numeric color drafts. Shared Rust owns parsing, conversion and the
//! palette/effect definitions; this sheet publishes one accepted color change.
use crate::display_color::{ColorPatch, ViewColor};
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::glib;
use layer_core::color::{RgbColor, RgbSpace};
use layer_ui::{ColorAction, ColorEditor, ColorEditorError, ColorFormCopy, ColorInputModel, ColorSlot, ColorValidationCopy, UiAction};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub(crate) struct Form {
    localization: RefCell<std::sync::Arc<layer_ui::Localizer>>,
    editor: RefCell<ColorEditor>,
    updating: Cell<bool>,
    composing: [Cell<bool>; 5],
    intensity_error: RefCell<Option<ColorEditorError>>,
    copy: RefCell<ColorFormCopy>,
    dialog: adw::AlertDialog,
    model: adw::ComboRow,
    models: gtk::StringList,
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
        let labels = editor.model().localized_labels(&self.localization.borrow());
        self.model.set_selected(
            ColorInputModel::ALL
                .iter()
                .position(|m| *m == editor.model())
                .unwrap() as u32,
        );
        for (i, row) in self.fields.iter().enumerate() {
            row.set_title(&labels[i]);
            row.set_visible(!labels[i].is_empty());
            row.set_text(&editor.fields()[i]);
        }
        drop(editor);
        self.updating.set(false);
        self.refresh_preview();
    }
    fn refresh_preview(&self) {
        if self.composing.iter().any(Cell::get) { self.refresh_copy(); return; }
        let view = self.view.get();
        let editor = self.editor.borrow();
        let color = self.intensity_error.borrow().clone().map_or_else(|| editor.colors(), Err);
        let mut copy = ColorFormCopy { model:editor.model(), document_space:self.space, validation:None, error:None };
        match color {
            Ok((color, base)) => {
                copy.validation = Some(ColorValidationCopy::new(color, self.space, view.space(), self.hdr).unwrap());
                self.preview.set_display_color(color, view, self.headroom.get());
                self.base_preview.set_display_color(base, view, self.headroom.get());
            }
            Err(error) => copy.error = Some(error),
        }
        *self.copy.borrow_mut() = copy;
        drop(editor);
        self.refresh_copy();
        self.preview.queue_draw();
    }
    fn refresh_copy(&self) {
        let localization = self.localization.borrow();
        let model = self.copy.borrow().model.localized_name(&localization);
        self.model.set_tooltip_text(Some(&model));
        self.model.upcast_ref::<gtk::Widget>().update_property(&[gtk::accessible::Property::Description(&model)]);
        let copy = self.copy.borrow().localized(&localization).unwrap();
        for (row, caption) in self.fields.iter().zip(copy.labels) { row.set_title(&caption); }
        self.description.set_text(&copy.description);
        if let Some(error) = copy.error {
            self.validation.add_css_class("error");
            self.validation.set_text(&error);
            self.dialog.set_response_enabled("apply", false);
        } else {
            self.validation.remove_css_class("error");
            self.validation.set_text(copy.validation.as_deref().unwrap_or_default());
            self.dialog.set_response_enabled("apply", copy.validation.is_some() && !self.composing.iter().any(Cell::get));
        }
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
        ColorSlot::Temporary => colors.temporary,
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
        if let Err(error) = editor.enable_hdr(stops) { workspace.changed(Err(error.message(editor.model(), &workspace.localization()))); return; }
    }
    let copy = layer_ui::NativeCopy::new(&workspace.localization()).color;
    let common = layer_ui::CommonCopy::new(&workspace.localization());
    let dialog = adw::AlertDialog::builder()
        .heading(copy.edit.as_ref())
        .content_width(400)
        .build();
    dialog.set_widget_name("edit-color-dialog");
    dialog.add_responses(&[("cancel", common.cancel.as_ref()), ("apply", copy.use_color.as_ref())]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("apply"));
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let group = adw::PreferencesGroup::new();
    let model = adw::ComboRow::builder().title(copy.model.as_ref()).use_subtitle(true).build();
    model.set_widget_name("edit-color-model");
    let models = gtk::StringList::new(
        &ColorInputModel::ALL.map(|model| model.localized_name(&workspace.localization())).iter().map(|name|name.as_ref()).collect::<Vec<_>>(),
    );
    model.set_model(Some(&models));
    group.add(&model);
    let intensity = adw::EntryRow::builder().title(copy.intensity_ev.as_ref()).build();
    intensity.set_widget_name("edit-color-ev");
    crate::input::guard_editable_activation(&intensity);
    intensity.set_visible(hdr);
    if hdr { intensity.set_text(&editor.intensity().unwrap().to_string()); }
    group.add(&intensity);
    let fields = std::array::from_fn(|i| {
        let row = adw::EntryRow::new();
        row.set_widget_name(&format!("edit-color-value-{i}"));
        crate::input::guard_editable_activation(&row);
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
    preview.update_property(&[gtk::accessible::Property::Label(copy.adjusted.as_ref())]);
    let base_preview = ColorPatch::new(false);
    base_preview.set_height_request(48);
    base_preview.set_widget_name("edit-color-base-preview");
    base_preview.update_property(&[gtk::accessible::Property::Label(copy.base.as_ref())]);
    base_preview.set_visible(hdr);
    let colors = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    colors.set_homogeneous(true);
    colors.append(&base_preview);
    colors.append(&preview);
    let labels = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    labels.set_homogeneous(true);
    labels.add_css_class("caption");
    labels.add_css_class("dim-label");
    labels.append(&gtk::Label::new(Some(copy.base.as_ref())));
    labels.append(&gtk::Label::new(Some(copy.adjusted.as_ref())));
    labels.set_visible(hdr);
    let comparison = gtk::Box::new(gtk::Orientation::Vertical, 4);
    comparison.set_widget_name("edit-color-comparison");
    comparison.append(&labels);
    comparison.append(&colors);
    content.append(&description);
    if hdr { content.append(&comparison); }
    content.append(&group);
    if !hdr { content.append(&comparison); }
    let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(540)
        .child(&content)
        .build());
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.append(&scroll);
    body.append(&validation);
    dialog.set_extra_child(Some(&body));
    let form = Rc::new(Form {
        localization: RefCell::new(workspace.localization()),
        copy: RefCell::new(ColorFormCopy { model:editor.model(), document_space:space, validation:None, error:None }),
        editor: RefCell::new(editor),
        updating: Cell::new(false),
        composing: std::array::from_fn(|_| Cell::new(false)),
        intensity_error: RefCell::new(None),
        dialog,
        model,
        models,
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
    let weak_form = Rc::downgrade(&form);
    workspace.on_localization(glib::clone!(#[weak] labels, #[upgrade_or] false, move |localization| {
        let Some(form) = weak_form.upgrade() else { return false; };
        *form.localization.borrow_mut() = localization.clone();
        let copy = layer_ui::NativeCopy::new(localization).color;
        let common = layer_ui::CommonCopy::new(localization);
        form.updating.set(true);
        form.dialog.set_heading(Some(&copy.edit));
        form.dialog.set_response_label("cancel", &common.cancel);
        form.dialog.set_response_label("apply", &copy.use_color);
        form.model.set_title(&copy.model);
        let selected = form.model.selected();
        form.models.splice(0, form.models.n_items(), &ColorInputModel::ALL.map(|model| model.localized_name(localization)).iter().map(|name| name.as_ref()).collect::<Vec<_>>());
        form.model.set_selected(selected);
        form.intensity.set_title(&copy.intensity_ev);
        form.preview.update_property(&[gtk::accessible::Property::Label(&copy.adjusted)]);
        form.base_preview.update_property(&[gtk::accessible::Property::Label(&copy.base)]);
        if let Some(label) = labels.first_child().and_downcast::<gtk::Label>() { label.set_text(&copy.base); }
        if let Some(label) = labels.last_child().and_downcast::<gtk::Label>() { label.set_text(&copy.adjusted); }
        form.updating.set(false);
        form.refresh_copy();
        true
    }));
    workspace.color_editors.borrow_mut().push(Rc::downgrade(&form));
    refresh_display(workspace);
    let weak = Rc::downgrade(&form);
    form.intensity.connect_changed(move |row| {
        let Some(form) = weak.upgrade() else { return; };
        if form.updating.get() || form.composing[4].get() { return; }
        let result = layer_ui::color_intensity_input_typed(&row.text())
            .and_then(|stops| form.editor.borrow_mut().set_intensity(stops));
        match result {
            Ok(()) => { form.intensity_error.borrow_mut().take(); form.populate(); },
            Err(error) => {
                *form.intensity_error.borrow_mut() = Some(error);
                form.refresh_preview();
            }
        }
    });
    for (i, row) in form.fields.iter().enumerate() {
        let weak = Rc::downgrade(&form);
        row.connect_changed(move |row| {
            let Some(form) = weak.upgrade() else {
                return;
            };
            if form.updating.get() || form.composing[i].get() {
                return;
            }
            let result = form.editor.borrow_mut().set_field(i, row.text().into());
            if let Err(error) = result {
                form.copy.borrow_mut().error = Some(ColorEditorError::Detail(error));
                form.refresh_copy();
            } else {
                form.refresh_preview();
            }
        });
    }
    for (i, row) in form.fields.iter().chain([&form.intensity]).enumerate() {
        if let Some(text) = row.delegate().and_downcast::<gtk::Text>() {
            let weak = Rc::downgrade(&form);
            let row = row.downgrade();
            text.connect_preedit_changed(move |_, preedit| {
                let Some(form) = weak.upgrade() else { return; };
                form.composing[i].set(!preedit.is_empty());
                if preedit.is_empty() { if let Some(row) = row.upgrade() { row.emit_by_name::<()>("changed", &[]); } }
                form.refresh_copy();
            });
        }
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
            if form.copy.borrow().error.is_some() || form.composing.iter().any(Cell::get) { return; }
            let current_epoch = workspace
                .gpu
                .borrow()
                .as_ref()
                .map(|g| g.session.state().document_file.epoch);
            if current_epoch != Some(epoch) {
                if let Some(gpu) = workspace.gpu.borrow_mut().as_mut() {
                    gpu.session.raise_message_notice(layer_ui::MessageId::COMMON_ACTION_FAILED);
                }
                workspace.changed(Ok(layer_ui::UiChange { regions:layer_ui::regions::HOST, ..Default::default() }));
                return;
            }
            match form.editor.borrow().color_localized(&workspace.localization()) {
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
        self.bind_copy(workspace);
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
    pub(crate) fn bind_copy(&self, workspace: &Workspace) {
        let widget = self.widget.downgrade();
        workspace.on_localization(move |localization| {
            let Some(widget) = widget.upgrade() else { return false; };
            let copy = layer_ui::NativeCopy::new(localization).color;
            widget.set_tooltip_text(Some(&copy.edit));
            widget.update_property(&[gtk::accessible::Property::Label(&copy.edit)]);
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::{NativeTestApp, find_named, new_drawing_at, new_photo::ready, pump, save_snapshot, until};
    use gtk::subclass::prelude::ObjectSubclassIsExt;
    use layer_core::color::SampleDepth;
    use layer_ui::{EffectAction, PreferenceAction, PreferenceId, PreferenceValue, Theme, UiLanguage};

    #[test]
    #[ignore = "private display and hardware GPU retained color drafts"]
    fn native_color_editor_live_language() {
        let output = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());
        let (application, active) = crate::application("art.capycanvas.ColorDraftLanguages");
        let app = NativeTestApp(application);
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        for depth in [SampleDepth::U8, SampleDepth::F16] {
            let index = active.borrow().len();
            crate::open_workspace(&app, &active, Some((new_drawing_at(64, 64, depth), None)), None);
            until(|| active.borrow().len() > index, "prepared color draft window");
            let w = active.borrow().last().unwrap().clone();
            w.window.maximize(); w.window.present(); ready(&w);
            w.dispatch(UiAction::Effect { action:EffectAction::Insert { effect:"black_white".into() } });
            w.dispatch(UiAction::Color { action:ColorAction::Definition { color:RgbColor::WHITE } });
            ready(&w);
            let color_button = find_named(w.window.upcast_ref(), "effect-color-tint_color").unwrap().downcast::<gtk::Button>().unwrap();
            let bucket = find_named(w.window.upcast_ref(), "tint-color-bucket").unwrap().downcast::<gtk::Button>().unwrap();
            let switch = |language| {
                let choice = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as u32;
                w.dispatch(UiAction::Preferences { action:PreferenceAction::Edit { id:PreferenceId::Language, value:PreferenceValue::Choice(choice) } });
                until(|| w.localization().language() == language, "retained color draft language");
            };
            let checkpoint = w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint();
            let original = w.gpu.borrow().as_ref().unwrap().session.state().colors.clone();
            for theme in [Theme::Light, Theme::Dark] {
                w.dispatch(UiAction::SetTheme { theme:Some(theme) }); ready(&w);
                show(&w, ColorSlot::Foreground);
                until(|| w.color_editors.borrow().iter().any(|form| form.upgrade().is_some()), "native color draft form");
                let form = w.color_editors.borrow().iter().rev().find_map(std::rc::Weak::upgrade).unwrap();
                until(|| form.dialog.is_mapped(), "native color draft mapped");
                let model = form.model.model().unwrap();
                let selected = form.model.selected();
                let fields = form.fields.clone();
                let preview = form.preview.clone();
                if depth.is_float() { form.intensity.set_text("2"); }
                let validation = form.copy.borrow().clone();
                assert!(validation.error.is_none());
                assert!(validation.validation.as_ref().is_some_and(|copy| copy.above_white == depth.is_float()));
                for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                    switch(language);
                    assert_eq!(*form.copy.borrow(), validation);
                    assert_eq!(form.model.model().unwrap(), model);
                    assert_eq!(form.model.selected(), selected);
                    assert_eq!(form.model.subtitle().as_deref(), Some(form.copy.borrow().model.localized_name(&w.localization()).as_ref()));
                    assert_eq!(form.model.tooltip_text().as_deref(),Some(form.copy.borrow().model.localized_name(&w.localization()).as_ref()));
                    assert_eq!(find_named(w.window.upcast_ref(), "effect-color-tint_color").unwrap(), color_button.clone().upcast::<gtk::Widget>());
                    assert_eq!(color_button.tooltip_text().as_deref(), Some(layer_ui::NativeCopy::new(&w.localization()).color.edit.as_ref()));
                    assert_eq!(find_named(w.window.upcast_ref(), "tint-color-bucket").unwrap(), bucket.clone().upcast::<gtk::Widget>());
                    assert_eq!(bucket.tooltip_text().as_deref(), Some(layer_ui::NativeCopy::new(&w.localization()).color.use_selected.as_ref()));
                    assert_eq!(form.preview, preview);
                    assert_eq!(form.dialog.heading().as_deref(), Some(layer_ui::NativeCopy::new(&w.localization()).color.edit.as_ref()));
                    assert_eq!(form.validation.text(), validation.localized(&w.localization()).unwrap().validation.unwrap());
                    assert!(form.dialog.is_response_enabled("apply"));
                    for (index, option) in ColorInputModel::ALL.iter().enumerate() {
                        assert_eq!(form.models.string(index as u32).unwrap(), option.localized_name(&w.localization()).as_ref());
                    }
                    save_snapshot(&w, 60, || output.join(format!("color-valid-{depth:?}-{}-{theme:?}.png", language.tag())));
                    if matches!(language, UiLanguage::French | UiLanguage::German) {
                        let target_width = w.window.width().min(744);
                        let settings = gtk::Settings::default().unwrap();
                        let previous = settings.property::<String>("gtk-font-name");
                        settings.set_property("gtk-font-name", "Sans 16");
                        w.window.unmaximize(); w.window.set_default_size(target_width, 780); pump(250);
                        assert_eq!([w.window.width(), w.window.height()], [target_width, 780]);
                        let selected_caption = form.copy.borrow().model.localized_name(&w.localization());
                        let mut selected_label = None;
                        crate::text_language::visit(form.model.upcast_ref(), &mut |widget| {
                            if let Some(label) = widget.downcast_ref::<gtk::Label>()
                                && label.is_mapped() && label.text() == selected_caption.as_ref() {
                                selected_label = Some(label.clone());
                            }
                        });
                        let selected_label = selected_label.expect("visible selected color model caption");
                        assert!(!selected_label.layout().is_ellipsized(), "readable selected color model {}", language.tag());
                        assert!(form.validation.layout().pixel_size().1 <= form.validation.height());
                        let mut child = form.validation.clone().upcast::<gtk::Widget>();
                        while let Some(parent) = child.parent() {
                            let bounds = child.compute_bounds(&parent).unwrap();
                            assert!(bounds.y() >= -1. && bounds.y() + bounds.height() <= parent.height() as f32 + 1., "visible color status in {}: {bounds:?} / {}", language.tag(), parent.height());
                            if parent == form.dialog.clone().upcast::<gtk::Widget>() { break; }
                            child = parent;
                        }
                        save_snapshot(&w, 60, || output.join(format!("color-large-narrow-{depth:?}-{}-{theme:?}.png", language.tag())));
                        settings.set_property("gtk-font-name", previous);
                        w.window.maximize(); pump(150);
                    }
                }
                if depth.is_float() {
                    form.intensity.set_text("17");
                    form.intensity.grab_focus(); pump(100); form.intensity.select_region(0, 2);
                    let selection = form.intensity.selection_bounds();
                    let refused = form.copy.borrow().clone();
                    assert!(matches!(refused.error, Some(ColorEditorError::Intensity(layer_ui::NumericError::Range { .. }))));
                    assert_eq!(form.editor.borrow().intensity(), Some(2.));
                    for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                        switch(language);
                        assert_eq!(*form.copy.borrow(), refused);
                        assert_eq!(form.intensity.text(), "17");
                        assert_eq!(form.intensity.selection_bounds(), selection);
                        assert_eq!(form.editor.borrow().intensity(), Some(2.));
                        assert!(!form.dialog.is_response_enabled("apply"), "parse-valid refused EV remains refused in {}", language.tag());
                        assert_eq!(form.validation.text(), refused.localized(&w.localization()).unwrap().error.unwrap());
                        save_snapshot(&w, 60, || output.join(format!("color-ev-refused-{}-{theme:?}.png", language.tag())));
                    }
                    form.intensity.set_text("2");
                }
                let literal = "Tiếng Việt ไทย İı {draft} 🎨";
                form.fields[0].set_text(literal);
                form.fields[0].grab_focus(); pump(100); form.fields[0].select_region(1, 6);
                let selection = form.fields[0].selection_bounds();
                let refused = form.copy.borrow().clone();
                assert!(matches!(refused.error, Some(ColorEditorError::Numeric { field:0, .. })));
                for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                    switch(language);
                    assert_eq!(*form.copy.borrow(), refused);
                    assert_eq!(form.model.model().unwrap(), model);
                    assert_eq!(form.fields, fields);
                    assert_eq!(form.fields[0].text(), literal);
                    assert_eq!(form.fields[0].selection_bounds(), selection);
                    assert!(!form.dialog.is_response_enabled("apply"));
                    assert_eq!(form.validation.text(), refused.localized(&w.localization()).unwrap().error.unwrap());
                    assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint(), checkpoint);
                    save_snapshot(&w, 60, || output.join(format!("color-draft-{depth:?}-{}-{theme:?}.png", language.tag())));
                }
                switch(UiLanguage::English);
                let text = form.fields[0].delegate().and_downcast::<gtk::Text>().unwrap();
                text.emit_by_name::<()>("preedit-changed", &[&"tieengs"]);
                let choice = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == UiLanguage::Vietnamese).unwrap() as u32;
                w.dispatch(UiAction::Preferences { action:PreferenceAction::Edit { id:PreferenceId::Language, value:PreferenceValue::Choice(choice) } });
                pump(100);
                assert_eq!(w.localization().language(), UiLanguage::English);
                assert!(!form.dialog.is_response_enabled("apply"));
                text.emit_by_name::<()>("preedit-changed", &[&""]);
                until(|| w.localization().language() == UiLanguage::Vietnamese, "color draft synthetic preedit boundary");
                assert_eq!(form.fields[0].text(), literal);
                assert_eq!(form.fields[0].selection_bounds(), selection);
                assert!(!form.dialog.is_response_enabled("apply"));
                form.dialog.emit_by_name::<()>("response", &[&"apply"]);
                form.dialog.force_close();
                until(|| !form.dialog.is_mapped(), "invalid color draft forced response closes without publication");
                pump(50);
                assert_eq!(w.gpu.borrow().as_ref().unwrap().session.state().colors, original);
                assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint(), checkpoint);
                eprintln!("GTK color draft {depth:?}/{theme:?}: retained typed refusal/warnings/model/fields/preview and synthetic composition boundary all15 passed");
            }
            assert!(w.window.visible_dialog().is_none());assert!(!w.servicing.get());
            assert!(w.gpu.borrow().as_ref().unwrap().session.can_park_document());
            let before_epoch = w.gpu.borrow().as_ref().unwrap().session.state().document_file.epoch;
            let before_notice = w.gpu.borrow().as_ref().unwrap().session.state().notice.as_ref().map(|notice|notice.id);
            let mut opening = std::pin::pin!(w.documents.open(&w, (new_drawing_at(80, 80, depth), None, None)));
            let mut shown = false;
            glib::MainContext::default().block_on(std::future::poll_fn(|context| {
                use std::future::Future;
                match opening.as_mut().poll(context) {
                    std::task::Poll::Pending => {
                        if !shown && w.documents.changing.get() && w.gpu.borrow().is_some() {
                            show(&w, ColorSlot::Foreground);shown = true;
                        }
                        std::task::Poll::Pending
                    }
                    result => result,
                }
            })).unwrap();
            assert!(shown,"color dialog opens after document transport admission and before async replacement");
            until(|| w.window.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "edit-color-dialog" && dialog.is_mapped()), "stale color dialog");
            ready(&w);
            {
                let gpu=w.gpu.borrow();let session=&gpu.as_ref().unwrap().session;
                assert_ne!(session.state().document_file.epoch,before_epoch);
                assert_eq!([session.engine().document().width,session.engine().document().height],[80,80]);
            }
            let replacement = w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint();
            crate::workspace::tests::new_photo::response(&w, "apply");
            until(|| w.gpu.borrow().as_ref().unwrap().session.state().notice.is_some(), "stale color action refusal");
            let notice = w.gpu.borrow().as_ref().unwrap().session.state().notice.clone().unwrap();
            assert_ne!(Some(notice.id),before_notice);
            for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                switch(language);
                let current = w.gpu.borrow().as_ref().unwrap().session.state().notice.clone().unwrap();
                assert_eq!(current.id, notice.id);
                assert_eq!(current.text, w.localization().text(layer_ui::MessageId::COMMON_ACTION_FAILED).as_ref());
                assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint(), replacement);
            }
            w.dispatch(UiAction::Effect { action:EffectAction::Insert { effect:"gradient_fill".into() } }); ready(&w);
            let bar = find_named(w.window.upcast_ref(), "effect-gradient").unwrap();
            let position = find_named(w.window.upcast_ref(), "effect-gradient-position").unwrap().downcast::<crate::number_control::NumberControl>().unwrap();
            let label = find_named(w.window.upcast_ref(), "effect-gradient-color-label").unwrap().downcast::<gtk::Label>().unwrap();
            let color = find_named(w.window.upcast_ref(), "effect-gradient-color").unwrap().downcast::<gtk::Button>().unwrap();
            let remove = find_named(w.window.upcast_ref(), "effect-gradient-remove").unwrap().downcast::<gtk::Button>().unwrap();
            let reset = find_named(w.window.upcast_ref(), "effect-gradient-reset").unwrap().downcast::<gtk::Button>().unwrap();
            let checkpoint = w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint();
            for theme in [Theme::Light, Theme::Dark] {
                w.dispatch(UiAction::SetTheme { theme:Some(theme) }); ready(&w);
                for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                    switch(language);
                    let copy = layer_ui::NativeCopy::new(&w.localization()).color;
                    assert_eq!(find_named(w.window.upcast_ref(), "effect-gradient").unwrap(), bar);
                    assert_eq!(find_named(w.window.upcast_ref(), "effect-gradient-position").unwrap(), position.clone().upcast::<gtk::Widget>());
                    assert_eq!(*position.imp().editor_title.borrow(), copy.position.as_ref());
                    assert_eq!(bar.tooltip_text().as_deref(), Some(copy.add_stop.as_ref()));
                    assert_eq!(label.text(), copy.color.as_ref());
                    assert_eq!(color.tooltip_text().as_deref(), Some(copy.edit.as_ref()));
                    assert_eq!(remove.tooltip_text().as_deref(), Some(copy.remove_stop.as_ref()));
                    assert_eq!(reset.tooltip_text().as_deref(), Some(copy.reset_gradient.as_ref()));
                    assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().checkpoint(), checkpoint);
                    save_snapshot(&w, 60, || output.join(format!("gradient-{depth:?}-{}-{theme:?}.png", language.tag())));
                }
            }
            w.window.destroy(); pump(100);
        }
    }
}
