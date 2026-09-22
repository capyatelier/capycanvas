//! GTK views of the shared effect/property schemas. No filter-specific widgets.
#[path = "gradient_preview.rs"]
mod gradient_preview;
use crate::{number_control::NumberControl, workspace::Workspace};
use adw::prelude::*;
use gtk::glib;
use layer_core::EffectValue;
use layer_ui::{
    EffectAction, FilterPickerAction, LayerPropertiesView, PropertyKind, UiAction, UiState,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

pub struct EffectPanels {
    pub filter_types: gtk::Box,
    type_list: gtk::Box,
    type_buttons: RefCell<Vec<gtk::Button>>,
    cancel_filter: gtk::Button,
    split_picker: bool,
    pub adjustments: gtk::Box,
    picker_body: gtk::Box,
    picker_scroller: gtk::ScrolledWindow,
    category: gtk::DropDown,
    category_icon: gtk::Image,
    search_button: gtk::Button,
    search_entry: gtk::SearchEntry,
    picker_bound: Cell<bool>,
    picker_revision: Cell<Option<u64>>,
    picker_categories: RefCell<Vec<layer_ui::FilterCategoryChoice>>,
    picker_updating: Cell<bool>,
    picker_visible: RefCell<Vec<std::sync::Arc<str>>>,
    picker_rows: RefCell<HashMap<std::sync::Arc<str>, (gtk::Button, gtk::Picture)>>,
    preview_key: RefCell<Option<String>>,
    preview_color: Cell<Option<crate::display_color::ViewColor>>,
    preview_loaded: RefCell<HashMap<std::sync::Arc<str>, gtk::gdk::Texture>>,
    preview_request: Cell<u64>,
    pub properties: gtk::Box,
    pub stats: gtk::Box,
    recording_button: gtk::Button,
    recording_save_open: Cell<bool>,
    recording_was_active: Cell<bool>,
    title: gtk::Label,
    body: gtk::Box,
    schema: RefCell<Option<LayerPropertiesView>>,
    fields: RefCell<Vec<Field>>,
    stats_labels: RefCell<Vec<gtk::Label>>,
    stats_plot: gtk::DrawingArea,
    stats_samples: Rc<RefCell<Vec<f32>>>,
}
enum Field {
    Number(NumberControl),
    Toggle(gtk::Switch),
    Choice(gtk::DropDown),
    Color(Rc<crate::color_editor::ColorButton>),
    Curve(CurveEditor),
    Gradient(GradientEditor),
}
impl EffectPanels {
    pub fn picker_content_measurement(
        &self,
        width: i32,
    ) -> (f32, layer_ui::PanelScrollMeasurement) {
        let fixed_height = (self
            .adjustments
            .measure(gtk::Orientation::Vertical, width)
            .1
            - self
                .picker_scroller
                .measure(gtk::Orientation::Vertical, width)
                .1)
            .max(0) as f32;
        let unit_height = self.picker_body.first_child().map_or(0, |row| {
            row.measure(gtk::Orientation::Vertical, width).1 + self.picker_body.spacing()
        }) as f32;
        (
            fixed_height
                + self
                    .picker_body
                    .measure(gtk::Orientation::Vertical, width)
                    .1 as f32,
            layer_ui::PanelScrollMeasurement {
                fixed_height,
                unit_height,
            },
        )
    }

    #[cfg(test)]
    pub fn preview_requests(&self) -> u64 {
        self.preview_request.get()
    }
    pub fn new() -> Self {
        Self::with_filter_types(false)
    }
    pub fn with_filter_types(split_picker: bool) -> Self {
        let filter_types = gtk::Box::new(gtk::Orientation::Vertical, 6);
        filter_types.add_css_class("filter-types");
        let type_list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let types_scroll = crate::workspace::scroll(&type_list);
        types_scroll.set_vexpand(true);
        filter_types.append(&types_scroll);
        let cancel_filter = gtk::Button::with_label("Cancel");
        cancel_filter.set_widget_name("cancel-filter");
        cancel_filter.set_halign(gtk::Align::Start);
        filter_types.append(&cancel_filter);
        let adjustments = gtk::Box::new(gtk::Orientation::Vertical, 6);
        adjustments.add_css_class("filter-picker");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.add_css_class("filter-picker-header");
        header.set_visible(!split_picker);
        let category = gtk::DropDown::from_strings(&[]);
        category.set_hexpand(true);
        let category_icon = crate::icons::image("layer-adjustments-symbolic");
        category_icon.add_css_class("dim-label");
        let search_entry = gtk::SearchEntry::builder()
            .hexpand(true)
            .visible(false)
            .build();
        let search_button = crate::icons::button("system-search-symbolic");
        search_button.add_css_class("flat");
        header.append(&category_icon);
        header.append(&category);
        header.append(&search_entry);
        header.append(&search_button);
        let picker_body = gtk::Box::new(gtk::Orientation::Vertical, 2);
        picker_body.add_css_class("filter-picker-body");
        let scroller = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&picker_body)
            .build());
        scroller.add_css_class("filter-picker-scroll");
        adjustments.append(&header);
        adjustments.append(&scroller);
        let properties = gtk::Box::new(gtk::Orientation::Vertical, 6);
        properties.add_css_class("effect-properties");
        let title = gtk::Label::new(None);
        title.set_xalign(0.);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.add_css_class("heading");
        let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
        properties.append(&title);
        properties.append(&body);
        let stats = gtk::Box::new(gtk::Orientation::Vertical, 6);
        stats.add_css_class("renderer-stats");
        let recording_button = gtk::Button::with_label("Start stroke recording");
        recording_button.set_widget_name("stroke-recording");
        recording_button.set_tooltip_text(Some("Record tablet input for up to 10 minutes"));
        stats.append(&recording_button);
        let stats_plot = gtk::DrawingArea::builder()
            .content_width(180)
            .content_height(46)
            .hexpand(true)
            .build();
        let stats_samples = Rc::new(RefCell::new(Vec::<f32>::new()));
        stats_plot.set_draw_func(glib::clone!(
            #[strong]
            stats_samples,
            move |area, cr, width, height| {
                let samples = stats_samples.borrow();
                let max = samples.iter().copied().fold(1000. / 120., f32::max) * 1.1;
                let y = |ms: f32| height as f64 * (1. - (ms / max) as f64);
                let c = area.color();
                cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, 0.3);
                cr.set_line_width(1.);
                cr.set_dash(&[3., 3.], 0.);
                cr.move_to(0., y(1000. / 120.));
                cr.line_to(width as f64, y(1000. / 120.));
                cr.stroke().ok();
                cr.set_dash(&[], 0.);
                cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, 0.9);
                cr.set_line_width(1.5);
                for (i, &ms) in samples.iter().enumerate() {
                    let x = i as f64 * width as f64 / 119.;
                    if i == 0 {
                        cr.move_to(x, y(ms));
                    } else {
                        cr.line_to(x, y(ms));
                    }
                }
                cr.stroke().ok();
            }
        ));
        Self {
            filter_types,
            type_list,
            type_buttons: Default::default(),
            cancel_filter,
            split_picker,
            adjustments,
            picker_body,
            picker_scroller: scroller,
            category,
            category_icon,
            search_button,
            search_entry,
            picker_bound: Cell::new(false),
            picker_revision: Cell::new(None),
            picker_categories: RefCell::new(Vec::new()),
            picker_updating: Cell::new(false),
            picker_visible: RefCell::new(Vec::new()),
            picker_rows: RefCell::new(HashMap::new()),
            preview_key: RefCell::new(None),
            preview_color: Cell::new(None),
            preview_loaded: RefCell::new(HashMap::new()),
            preview_request: Cell::new(0),
            properties,
            stats,
            recording_button,
            recording_save_open: Cell::new(false),
            recording_was_active: Cell::new(false),
            title,
            body,
            schema: RefCell::new(None),
            fields: RefCell::new(Vec::new()),
            stats_labels: RefCell::new(Vec::new()),
            stats_plot,
            stats_samples,
        }
    }
    pub fn bind(self: &Rc<Self>, w: &Rc<Workspace>, state: &UiState) {
        if self.picker_bound.replace(true) {
            return;
        }
        self.cancel_filter.connect_clicked(glib::clone!(#[weak] w, move |_| {
            w.dispatch(UiAction::Effect { action: EffectAction::CancelFilter });
        }));
        self.recording_button.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                w.effects.recording_clicked(&w);
            }
        ));
        // One producer services every projection, including open drawers.
        if Rc::ptr_eq(self, &w.effects) {
            let weak = Rc::downgrade(w);
            glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                let Some(w) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                w.effects.recording_tick(&w);
                let extra: Vec<_> = w.drawers().iter().filter_map(|d| d.effects()).collect();
                for view in std::iter::once(&w.effects).chain(extra.iter()) {
                    if view.stats.is_mapped() {
                        view.refresh_stats(&w);
                    }
                }
                w.effects.refresh_previews(&w, &extra);
                glib::ControlFlow::Continue
            });
        }
        self.category.set_model(Some(&gtk::StringList::new(
            &state
                .filter_categories
                .iter()
                .map(|c| c.label.as_ref())
                .collect::<Vec<_>>(),
        )));
        self.category.connect_selected_notify(glib::clone!(
            #[weak]
            w,
            #[weak(rename_to = this)]
            self,
            move |drop| {
                if this.picker_updating.get() {
                    return;
                }
                let choice = this
                    .picker_categories
                    .borrow()
                    .get(drop.selected() as usize)
                    .cloned();
                if let Some(choice) = choice {
                    w.dispatch(UiAction::FilterPicker {
                        action: FilterPickerAction::Category {
                            category: choice.id.clone(),
                        },
                    });
                }
            }
        ));
        self.search_entry
            .set_placeholder_text(Some(state.filter_picker.search_label));
        self.search_button
            .set_tooltip_text(Some(state.filter_picker.search_label));
        self.search_button.connect_clicked(glib::clone!(
            #[weak]
            w,
            #[weak(rename_to = this)]
            self,
            move |_| {
                w.dispatch(UiAction::FilterPicker {
                    action: FilterPickerAction::ToggleSearch,
                });
                if this.search_entry.is_visible() {
                    this.search_entry.grab_focus();
                }
            }
        ));
        self.search_entry.connect_search_changed(glib::clone!(
            #[weak]
            w,
            move |entry| {
                if entry.is_visible() {
                    w.dispatch(UiAction::FilterPicker {
                        action: FilterPickerAction::Search {
                            query: entry.text().to_string(),
                        },
                    });
                }
            }
        ));
        self.search_entry.connect_stop_search(glib::clone!(
            #[weak]
            w,
            move |_| w.dispatch(UiAction::FilterPicker {
                action: FilterPickerAction::ToggleSearch
            })
        ));
    }
    fn refresh_picker(&self, w: &Rc<Workspace>, state: &UiState) {
        self.picker_updating.set(true);
        if self
            .picker_revision
            .replace(Some(state.filter_catalog_revision))
            != Some(state.filter_catalog_revision)
        {
            self.picker_visible.borrow_mut().clear();
            self.picker_rows.borrow_mut().clear();
            self.preview_loaded.borrow_mut().clear();
            *self.picker_categories.borrow_mut() = state.filter_categories.clone();
            while let Some(child) = self.type_list.first_child() { self.type_list.remove(&child); }
            let mut buttons = self.type_buttons.borrow_mut();
            buttons.clear();
            for choice in &state.filter_categories {
                let category = choice.id.clone();
                let button = w.action_button(&choice.label, UiAction::FilterPicker {
                    action: FilterPickerAction::Category { category },
                });
                button.add_css_class("flat");
                button.add_css_class("tool-group");
                button.set_widget_name(&format!("filter-type-{}", choice.id.as_deref().unwrap_or("all")));
                button.set_height_request(44);
                button.set_child(Some(&crate::tool_panels::aligned_icon_label(&choice.label, choice.icon, 0.)));
                self.type_list.append(&button);
                buttons.push(button);
            }
            self.category.set_model(Some(&gtk::StringList::new(
                &state
                    .filter_categories
                    .iter()
                    .map(|c| c.label.as_ref())
                    .collect::<Vec<_>>(),
            )));
        }
        let picker = &state.filter_picker;
        for (button, choice) in self.type_buttons.borrow().iter().zip(&state.filter_categories) {
            if choice.id == picker.category { button.add_css_class("selected-tool"); }
            else { button.remove_css_class("selected-tool"); }
        }
        for (id, (button, _)) in self.picker_rows.borrow().iter() {
            if picker.selected.as_ref() == Some(id) { button.add_css_class("selected-tool"); }
            else { button.remove_css_class("selected-tool"); }
        }
        self.category.set_visible(picker.search.is_none());
        self.category_icon.set_visible(picker.search.is_none());
        if let Some(choice) = state
            .filter_categories
            .iter()
            .find(|c| c.id == picker.category)
        {
            crate::icons::set(
                &self.category_icon,
                Some(&format!("layer-{}-symbolic", choice.icon)),
            );
        }
        self.category.set_selected(
            state
                .filter_categories
                .iter()
                .position(|c| c.id == picker.category)
                .unwrap_or(0) as u32,
        );
        self.picker_updating.set(false);
        self.search_entry.set_visible(picker.search.is_some());
        let query = picker.search.as_deref().unwrap_or("");
        if self.search_entry.text() != query {
            self.search_entry.set_text(query);
        }
        let ids: Vec<_> = state.adjustments.iter().map(|c| c.id.clone()).collect();
        if *self.picker_visible.borrow() == ids {
            return;
        }
        while let Some(child) = self.picker_body.first_child() {
            self.picker_body.remove(&child);
        }
        let mut category = None;
        let mut rows = self.picker_rows.borrow_mut();
        for choice in &state.adjustments {
            if !self.split_picker && category.as_ref() != Some(&choice.category) {
                let heading =
                    crate::tool_panels::icon_label(&choice.category_label, choice.category_icon);
                heading.add_css_class("filter-category");
                self.picker_body.append(&heading);
                category = Some(choice.category.clone());
            }
            let row = rows.entry(choice.id.clone()).or_insert_with(|| {
                let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
                let picture = gtk::Picture::builder()
                    .can_shrink(true)
                    .height_request(40)
                    .content_fit(gtk::ContentFit::Fill)
                    .build();
                let label = gtk::Label::new(Some(&choice.label));
                label.set_xalign(1.);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                let caption = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                caption.set_halign(gtk::Align::End);
                if choice.animated {
                    let icon = crate::icons::image("layer-animation-symbolic");
                    icon.set_pixel_size(12);
                    icon.add_css_class("dim-label");
                    caption.append(&icon);
                }
                caption.append(&crate::icons::image(&format!("layer-{}-symbolic", choice.icon)));
                caption.append(&label);
                body.append(&picture);
                body.append(&caption);
                let button = gtk::Button::builder()
                    .child(&body)
                    .tooltip_text(&choice.tooltip)
                    .build();
                button.add_css_class("flat");
                button.add_css_class("filter-row");
                button.set_widget_name(&format!("adjustment-{}", choice.id));
                let action = choice.action.clone();
                button.connect_clicked(glib::clone!(
                    #[weak]
                    w,
                    move |_| w.dispatch(action.clone())
                ));
                (button, picture)
            });
            self.picker_body.append(&row.0);
            if picker.selected.as_ref() == Some(&choice.id) { row.0.add_css_class("selected-tool"); }
            else { row.0.remove_css_class("selected-tool"); }
        }
        if ids.is_empty() {
            let empty = gtk::Label::new(Some(picker.empty_label));
            empty.add_css_class("dim-label");
            self.picker_body.append(&empty);
        }
        *self.picker_visible.borrow_mut() = ids;
    }
    fn refresh_previews(&self, w: &Workspace, extra: &[Rc<Self>]) {
        let mut gpu = w.gpu.borrow_mut();
        let Some(gpu) = gpu.as_mut() else {
            return;
        };
        let view_color = gpu.session.engine().backend().view_color;
        if self.preview_color.replace(Some(view_color)) != Some(view_color) {
            gpu.session.reset_filter_previews();
        }
        let on_screen: Vec<_> = std::iter::once(self)
            .chain(extra.iter().map(|v| v.as_ref()))
            .filter(|v| v.adjustments.is_mapped())
            .flat_map(|view| {
                let rows = view.picker_rows.borrow();
                view.picker_visible
                    .borrow()
                    .iter()
                    .filter_map(|id| {
                        let (_, picture) = rows.get(id)?;
                        let rect = picture.compute_bounds(&view.picker_scroller)?;
                        (rect.y() + rect.height() > 0.
                            && rect.y() < view.picker_scroller.height() as f32)
                            .then_some((id.clone(), picture.clone()))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let size = on_screen.iter().fold([80, 40], |size, (_, picture)| {
            let scale = picture.scale_factor() as u32;
            [size[0].max((picture.width().max(1) as u32 * scale).clamp(80, 512)),
                size[1].max((40 * scale).min(128))]
        });
        let filters = on_screen.iter().map(|(id, _)| id.clone()).collect();
        let cache = layer_ui::FilterPreviewCache {
            key: self.preview_key.borrow().clone(),
            rows: self.preview_loaded.borrow().keys().cloned().collect(),
        };
        let Ok(update) = gpu.session.poll_filter_previews(
            glib::monotonic_time().max(0) as u64 * 1000, filters, size, cache,
        ) else { return; };
        self.preview_request.set(update.status.requests);
        if self.preview_key.borrow().as_ref() != Some(&update.status.key) {
            self.preview_loaded.borrow_mut().clear();
            *self.preview_key.borrow_mut() = Some(update.status.key);
        }
        self.preview_loaded.borrow_mut().retain(|id, _| update.status.retained.contains(id));
        if let Some(result) = update.image {
            let count = result.filters.len();
            let height = result.image.height / count as u32;
            let stride = result.image.stride as usize;
            let bytes = result.image.bytes;
            for (i, id) in result.filters.into_iter().enumerate() {
                let start = i * height as usize * stride;
                let row = glib::Bytes::from_owned(bytes[start..start + height as usize * stride].to_vec());
                let texture = view_color.texture_bytes([result.image.width, height],
                    gtk::gdk::MemoryFormat::R8g8b8a8, stride, row);
                self.preview_loaded.borrow_mut().insert(id, texture);
            }
        }
        let loaded = self.preview_loaded.borrow();
        // Hidden retained pictures must release evicted rows too; otherwise
        // the native widgets would defeat the common cache bound.
        for view in std::iter::once(self).chain(extra.iter().map(|v| v.as_ref())) {
            for (id, (_, picture)) in view.picker_rows.borrow().iter() {
                let texture = loaded.get(id);
                if picture.paintable().as_ref() != texture.map(|t| t.upcast_ref()) {
                    picture.set_paintable(texture);
                }
            }
        }
    }
    pub fn refresh(self: &Rc<Self>, w: &Rc<Workspace>, state: &UiState) {
        self.bind(w, state);
        if self.adjustments.parent().is_some() || self.filter_types.parent().is_some() {
            self.refresh_picker(w, state);
        }
        if self.properties.parent().is_none() {
            return;
        }
        let view = &state.layer_properties;
        self.title.set_text(&view.title);
        self.title.set_tooltip_text(Some(&view.description));
        self.body.set_sensitive(view.enabled);
        let rebuild = self.schema.borrow().as_ref().is_none_or(|old| {
            old.layer != view.layer
                || old.controls.len() != view.controls.len()
                || old.controls.iter().zip(&view.controls).any(|(a, b)| {
                    a.key != b.key
                        || a.kind != b.kind
                        || a.label != b.label
                        || a.section != b.section
                })
        });
        if rebuild {
            while let Some(child) = self.body.first_child() {
                self.body.remove(&child);
            }
            self.fields.borrow_mut().clear();
            let curves: Vec<_> = view
                .controls
                .iter()
                .filter(|c| matches!(c.kind, PropertyKind::Curve))
                .collect();
            let curve_stack = gtk::Stack::new();
            curve_stack.set_vhomogeneous(false);
            if !curves.is_empty() {
                let chooser = gtk::DropDown::from_strings(
                    &curves.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
                );
                let keys: Vec<_> = curves.iter().map(|c| c.key.clone()).collect();
                chooser.connect_selected_notify(glib::clone!(
                    #[weak]
                    curve_stack,
                    move |i| {
                        if let Some(key) = keys.get(i.selected() as usize) {
                            curve_stack.set_visible_child_name(key);
                        }
                    }
                ));
                self.body.append(&chooser);
                self.body.append(&curve_stack);
            }
            if let Some(layer) = view.layer {
                let mut section = None;
                let mut target = self.body.clone();
                for (index, control) in view.controls.iter().enumerate() {
                    if section != control.section.as_deref() {
                        if index > 0 {
                            let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
                            divider.add_css_class("property-divider");
                            self.body.append(&divider);
                        }
                        section = control.section.as_deref();
                        target = self.body.clone();
                        if section == Some("Advanced") {
                            target = gtk::Box::new(gtk::Orientation::Vertical, 6);
                            let expander = gtk::Expander::builder().label("Advanced").child(&target).build();
                            self.body.append(&expander);
                        } else if let Some(text) = section {
                            let heading = gtk::Label::new(Some(text));
                            heading.set_xalign(0.);
                            heading.add_css_class("heading");
                            heading.add_css_class("property-section");
                            self.body.append(&heading);
                        }
                    }
                    let key = control.key.clone();
                    let dispatch: Rc<dyn Fn(EffectValue)> = Rc::new(glib::clone!(
                        #[weak]
                        w,
                        move |value| w.dispatch(UiAction::Effect {
                            action: EffectAction::Set {
                                layer,
                                key: key.clone(),
                                value
                            }
                        })
                    ));
                    let field = match &control.kind {
                        PropertyKind::Number { numeric } => {
                            let input = NumberControl::new(numeric.clone(), &control.label, "");
                            input.set_widget_name(&format!("property-{}", control.key));
                            input.connect_value_changed(move |i| {
                                dispatch(EffectValue::Number(i.value() as f32))
                            });
                            target.append(&input);
                            Field::Number(input)
                        }
                        PropertyKind::Toggle => {
                            let input = gtk::Switch::new();
                            input.set_valign(gtk::Align::Center);
                            input.connect_active_notify(move |i| {
                                dispatch(EffectValue::Toggle(i.is_active()))
                            });
                            target.append(&row(&control.label, &input));
                            Field::Toggle(input)
                        }
                        PropertyKind::Choice { options } => {
                            let input = gtk::DropDown::from_strings(
                                &options.iter().map(|s| s.as_ref()).collect::<Vec<_>>(),
                            );
                            input.connect_selected_notify(move |i| {
                                dispatch(EffectValue::Choice(i.selected()))
                            });
                            target.append(&row(&control.label, &input));
                            Field::Choice(input)
                        }
                        PropertyKind::Color => {
                            let input = crate::color_editor::ColorButton::new();
                            input.widget.set_widget_name(&format!("effect-color-{}", control.key));
                            input.bind(w, move |_, color| dispatch(EffectValue::Color(color)));
                            if let Some(action) = &control.color_action {
                                let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                                input.widget.set_hexpand(true);
                                input.widget.set_height_request(36);
                                line.append(&input.widget);
                                let bucket = w.action_button("Use selected color", action.clone());
                                bucket.set_widget_name("paper-color-bucket");
                                crate::icons::set_button(&bucket, "layer-fill-symbolic");
                                line.append(&bucket);
                                target.append(&line);
                            } else { target.append(&row(&control.label, &input.widget)); }
                            Field::Color(input)
                        }
                        PropertyKind::Curve => {
                            let input = CurveEditor::new(w, layer, &control.key);
                            curve_stack.add_named(&input.root, Some(&control.key));
                            Field::Curve(input)
                        }
                        PropertyKind::Gradient => {
                            let input = GradientEditor::new(w, layer, &control.key);
                            target.append(&input.root);
                            Field::Gradient(input)
                        }
                    };
                    self.fields.borrow_mut().push(field);
                }
            }
        }
        for (field, c) in self.fields.borrow().iter().zip(&view.controls) {
            match (field, &c.value) {
                (Field::Number(i), EffectValue::Number(v)) => i.set_value(*v as f64),
                (Field::Toggle(i), EffectValue::Toggle(v)) => i.set_active(*v),
                (Field::Choice(i), EffectValue::Choice(v)) => i.set_selected(*v),
                (Field::Color(i), EffectValue::Color(c)) => {
                    i.set_color(*c, w.view_color())
                }
                (Field::Curve(i), EffectValue::Curve(p)) => {
                    *i.points.borrow_mut() = p.clone();
                    i.range.set(view.curve_max);
                    i.root.queue_draw();
                }
                (Field::Gradient(i), EffectValue::Gradient(stops)) => i.update(stops),
                _ => {}
            }
        }
        *self.schema.borrow_mut() = Some(view.clone());
    }
    fn recording_error(w: &Rc<Workspace>, message: &str) {
        let dialog = adw::AlertDialog::builder()
            .heading("Stroke recording")
            .body(message)
            .build();
        dialog.add_response("ok", "OK");
        let w = w.clone();
        glib::spawn_future_local(async move {
            crate::alert::choose(dialog, &w.window).await;
        });
    }
    fn recording_tick(self: &Rc<Self>, w: &Rc<Workspace>) {
        let Some(status) = w
            .gpu
            .borrow_mut()
            .as_mut()
            .map(|g| g.session.stroke_recording().status())
        else {
            return;
        };
        let was_active = self.recording_was_active.replace(status.recording);
        let extra: Vec<_> = w.drawers().iter().filter_map(|d| d.effects()).collect();
        for view in std::iter::once(self).chain(extra.iter()) {
            view.recording_button.set_label(status.label);
            view.recording_button
                .set_sensitive(!self.recording_save_open.get());
        }
        if was_active && status.ready {
            self.save_recording(w);
        }
    }
    fn recording_clicked(self: &Rc<Self>, w: &Rc<Workspace>) {
        if self.recording_save_open.get() {
            return;
        }
        let result = {
            let mut gpu = w.gpu.borrow_mut();
            let Some(g) = gpu.as_mut() else {
                return;
            };
            let mut recorder = g.session.stroke_recording();
            let status = recorder.status();
            if status.recording {
                recorder.stop(layer_engine::recording::StopReason::Manual);
                Ok(true)
            } else if status.ready {
                Ok(true)
            } else {
                recorder.start("gtk").map(|_| false)
            }
        };
        match result {
            Ok(true) => {
                self.recording_was_active.set(false);
                self.save_recording(w);
            }
            Ok(false) => (),
            Err(e) => Self::recording_error(w, e),
        }
        self.recording_tick(w);
    }
    fn save_recording(self: &Rc<Self>, w: &Rc<Workspace>) {
        if self.recording_save_open.replace(true) {
            return;
        }
        let this = self.clone();
        let w = w.clone();
        glib::spawn_future_local(async move {
            let result: Result<(), String> = async {
                let dialog = gtk::FileDialog::builder()
                    .title("Save stroke recording")
                    .initial_name("stroke-recording.capystrokes")
                    .modal(true)
                    .build();
                let file = match dialog.save_future(Some(&w.window)).await {
                    Ok(file) => file,
                    Err(e)
                        if e.matches(gtk::DialogError::Dismissed)
                            || e.matches(gtk::DialogError::Cancelled) =>
                    {
                        return Ok(());
                    }
                    Err(e) => return Err(e.to_string()),
                };
                let data = w
                    .gpu
                    .borrow_mut()
                    .as_mut()
                    .ok_or("Canvas unavailable")?
                    .session
                    .stroke_recording()
                    .snapshot()
                    .map_err(|e| e.to_string())?;
                let bytes =
                    gtk::gio::spawn_blocking(move || layer_engine::recording::compress(&data))
                        .await
                        .map_err(|_| "Recording compression failed")?
                        .map_err(|e| e.to_string())?;
                file.replace_contents_future(
                    bytes,
                    None,
                    false,
                    gtk::gio::FileCreateFlags::REPLACE_DESTINATION,
                )
                .await
                .map_err(|(_, e)| e.to_string())?;
                if let Some(g) = w.gpu.borrow_mut().as_mut() {
                    g.session.stroke_recording().saved();
                }
                Ok(())
            }
            .await;
            this.recording_save_open.set(false);
            if let Err(e) = result {
                Self::recording_error(&w, &e);
            }
            this.recording_tick(&w);
        });
    }
    fn refresh_stats(&self, w: &Workspace) {
        let Some(view) = w.gpu.borrow().as_ref().map(|g| g.session.renderer_stats()) else {
            return;
        };
        if self.stats_labels.borrow().is_empty() {
            self.stats_plot.set_tooltip_text(Some(view.chart_label));
            for (index, metric) in view.rows.iter().enumerate() {
                let value = gtk::Label::new(None);
                value.set_xalign(1.);
                value.add_css_class("numeric");
                let row = row(metric.label, &value);
                row.set_tooltip_text(Some(metric.description));
                self.stats
                    .insert_child_after(&row, self.recording_button.prev_sibling().as_ref());
                self.stats_labels.borrow_mut().push(value);
                if index + 1 == view.chart_after_rows {
                    self.stats.insert_child_after(
                        &self.stats_plot,
                        self.recording_button.prev_sibling().as_ref(),
                    );
                }
            }
        }
        for (label, metric) in self.stats_labels.borrow().iter().zip(&view.rows) {
            label.set_text(&metric.value);
        }
        *self.stats_samples.borrow_mut() = view.samples;
        self.stats_plot.queue_draw();
    }
}
fn row(title: &str, input: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_tooltip_text(Some(title));
    label.set_hexpand(true);
    row.append(&label);
    row.append(input);
    row
}
/// Reusable gradient editor: click to insert/select, edit stop position/color,
/// remove interior stops. Rust constrains order, endpoints and interpolation.
struct GradientEditor {
    root: gtk::Box,
    stops: Rc<RefCell<Vec<layer_core::GradientStop>>>,
    sync: Rc<dyn Fn()>,
}
impl GradientEditor {
    fn new(w: &Rc<Workspace>, layer: u64, key: &str) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let bar = gradient_preview::GradientPreview::new();
        bar.set_tooltip_text(Some("Click to select a color stop or add one."));
        bar.set_widget_name("effect-gradient");
        let stops = Rc::new(RefCell::new(Vec::<layer_core::GradientStop>::new()));
        let selected = Rc::new(Cell::new(0usize));
        let updating = Rc::new(Cell::new(false));
        let color = crate::color_editor::ColorButton::new();
        color.widget.set_widget_name("effect-gradient-color");
        let position = NumberControl::new(layer_ui::NumericControl::percent(), "Position", "");
        let remove = crate::icons::button("layer-minus-symbolic");
        remove.set_tooltip_text(Some("Remove color stop"));
        let reset = crate::icons::button("layer-reset-symbolic");
        reset.set_tooltip_text(Some("Reset gradient"));
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let label = gtk::Label::new(Some("Color"));
        label.set_hexpand(true);
        label.set_xalign(0.);
        actions.append(&label);
        actions.append(&color.widget);
        actions.append(&remove);
        actions.append(&reset);
        root.append(&bar);
        root.append(&position);
        root.append(&actions);
        let change = Rc::new(glib::clone!(
            #[weak]
            w,
            #[to_owned]
            key,
            move |index, position, color, remove| w.dispatch(UiAction::Effect {
                action: EffectAction::GradientStop {
                    layer,
                    key: key.clone(),
                    index,
                    position,
                    color,
                    remove
                }
            })
        ));
        let update: Rc<dyn Fn()> = Rc::new(glib::clone!(
            #[strong]
            updating,
            #[strong]
            stops,
            #[strong]
            selected,
            #[strong]
            color,
            #[weak]
            w,
            #[weak]
            position,
            #[weak]
            remove,
            #[weak]
            bar,
            move || {
                updating.set(true);
                let list = stops.borrow();
                let index = selected.get().min(list.len().saturating_sub(1));
                selected.set(index);
                if let Some(s) = list.get(index) {
                    color.set_color(s.color, w.view_color());
                    position.set_value(s.position as f64);
                    position.set_sensitive(index > 0 && index + 1 < list.len());
                    remove.set_sensitive(index > 0 && index + 1 < list.len());
                }
                updating.set(false);
                if let Some(g) = w.gpu.borrow().as_ref() {
                    bar.set_gradient(&list, index, g.session.state().colors.rgb_space(), w.view_color());
                }
            }
        ));
        let click = gtk::GestureClick::new();
        click.connect_pressed(glib::clone!(
            #[strong]
            stops,
            #[strong]
            selected,
            #[strong]
            change,
            #[strong]
            update,
            move |gesture, _, x, _| {
                let width = gesture.widget().unwrap().width() as f32 - 12.;
                let position = ((x as f32 - 6.) / width).clamp(0., 1.);
                let existing = stops
                    .borrow()
                    .iter()
                    .position(|s| (s.position - position).abs() * width < 8.);
                let index = existing
                    .unwrap_or_else(|| stops.borrow().partition_point(|s| s.position < position));
                selected.set(index);
                if existing.is_none() {
                    change(None, position, None, false);
                }
                update();
            }
        ));
        bar.add_controller(click);
        color.widget.connect_clicked(glib::clone!(
            #[weak]
            w,
            #[weak]
            color,
            #[strong]
            selected,
            #[strong]
            stops,
            #[strong]
            change,
            move |_| {
                let index = selected.get();
                let original = stops.borrow().clone();
                let Some(stop) = original.get(index) else { return; };
                let selected = selected.clone();
                let stops = stops.clone();
                let change = change.clone();
                let weak = Rc::downgrade(&color);
                crate::color_editor::choose(&w, stop.color, move |_, color| {
                    if weak.upgrade().is_some_and(|button| button.widget.root().is_some())
                        && selected.get() == index && *stops.borrow() == original {
                        change(Some(index), original[index].position, Some(color), false);
                    }
                });
            }
        ));
        position.connect_value_changed(glib::clone!(
            #[strong]
            updating,
            #[strong]
            selected,
            #[strong]
            change,
            move |n| {
                if !updating.get() {
                    change(Some(selected.get()), n.value() as f32, None, false);
                }
            }
        ));
        remove.connect_clicked(glib::clone!(
            #[strong]
            selected,
            #[strong]
            change,
            move |_| {
                let index = selected.get();
                selected.set(index.saturating_sub(1));
                change(Some(index), 0., None, true);
            }
        ));
        reset.connect_clicked(glib::clone!(
            #[weak]
            w,
            #[to_owned]
            key,
            move |_| w.dispatch(UiAction::Effect {
                action: EffectAction::Reset {
                    layer,
                    key: key.clone()
                }
            })
        ));
        Self {
            root,
            stops,
            sync: update,
        }
    }
    fn update(&self, stops: &[layer_core::GradientStop]) {
        *self.stops.borrow_mut() = stops.to_vec();
        (self.sync)();
    }
}
struct CurveEditor {
    root: gtk::DrawingArea,
    points: Rc<RefCell<Vec<[f32; 2]>>>,
    range: Rc<Cell<Option<f32>>>,
}
impl CurveEditor {
    fn new(w: &Rc<Workspace>, layer: u64, key: &str) -> Self {
        let root = gtk::DrawingArea::builder()
            .content_width(160)
            .content_height(200)
            .hexpand(true)
            .build();
        root.set_tooltip_text(Some(
            "Drag points to shape the curve. Click to add; right-click to remove.",
        ));
        let points = Rc::new(RefCell::new(vec![[0., 0.], [1., 1.]]));
        let range = Rc::new(Cell::new(None::<f32>));
        root.set_draw_func(glib::clone!(
            #[strong]
            points,
            #[strong]
            range,
            move |area, cr, width, height| {
                let (width, height) = (width as f64, height as f64);
                let c = area.color();
                cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, 0.12);
                cr.paint().ok();
                cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, 0.2);
                cr.set_line_width(1.);
                for i in 1..4 {
                    let t = i as f64 / 4.;
                    cr.move_to(t * width, 0.);
                    cr.line_to(t * width, height);
                    cr.move_to(0., t * height);
                    cr.line_to(width, t * height);
                }
                cr.stroke().ok();
                cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, 0.7);
                cr.set_font_size(11.);
                if let Some(peak) = range.get() {
                    let white = 1. / f64::from(peak);
                    cr.set_dash(&[3., 3.], 0.);
                    cr.move_to(white * width, 0.); cr.line_to(white * width, height);
                    cr.move_to(0., (1. - white) * height); cr.line_to(width, (1. - white) * height);
                    cr.stroke().ok(); cr.set_dash(&[], 0.);
                    cr.move_to(5., 13.); let _ = cr.show_text("SDR white · 0 EV");
                    let label = format!("{peak:.0} · +{:.0} EV", peak.log2());
                    let label_width = cr.text_extents(&label).map_or(70., |e| e.x_advance());
                    cr.move_to((width - label_width - 5.).max(5.), height - 5.);
                    let _ = cr.show_text(&label);
                } else {
                    cr.move_to(5., 13.); let _ = cr.show_text("Output");
                    cr.move_to(width - 40., height - 5.); let _ = cr.show_text("Input");
                }
                let p = points.borrow();
                cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, 1.);
                cr.set_line_width(1.5);
                for i in 0..=128 {
                    let x = i as f32 / 128.;
                    let y = layer_core::curve_value(&p, x);
                    if i == 0 {
                        cr.move_to(x as f64 * width, (1. - y) as f64 * height);
                    } else {
                        cr.line_to(x as f64 * width, (1. - y) as f64 * height);
                    }
                }
                cr.stroke().ok();
                for p in p.iter() {
                    cr.arc(
                        p[0] as f64 * width,
                        (1. - p[1]) as f64 * height,
                        3.5,
                        0.,
                        std::f64::consts::TAU,
                    );
                    cr.fill().ok();
                }
            }
        ));
        let drag = gtk::GestureDrag::new();
        let selected = Rc::new(Cell::new(None));
        let start = Rc::new(Cell::new([0.; 2]));
        let key = key.to_string();
        drag.connect_drag_begin(glib::clone!(
            #[weak]
            w,
            #[weak]
            root,
            #[strong]
            points,
            #[strong]
            selected,
            #[strong]
            start,
            #[strong]
            key,
            move |_, x, y| {
                start.set([x, y]);
                let p = [
                    (x / root.width() as f64) as f32,
                    1. - (y / root.height() as f64) as f32,
                ];
                let nearest = points
                    .borrow()
                    .iter()
                    .enumerate()
                    .find(|(_, q)| {
                        ((q[0] - p[0]) * root.width() as f32)
                            .hypot((q[1] - p[1]) * root.height() as f32)
                            < 12.
                    })
                    .map(|(i, _)| i);
                if let Some(i) = nearest {
                    selected.set(Some(i));
                } else {
                    w.dispatch(UiAction::Effect {
                        action: EffectAction::CurvePoint {
                            layer,
                            key: key.clone(),
                            index: None,
                            point: p,
                            remove: false,
                        },
                    });
                    selected.set(
                        points
                            .borrow()
                            .iter()
                            .position(|q| (q[0] - p[0]).abs() < 0.002),
                    );
                }
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak]
            w,
            #[weak]
            root,
            #[strong]
            selected,
            #[strong]
            start,
            #[strong]
            key,
            move |_, dx, dy| {
                if let Some(index) = selected.get() {
                    let [x, y] = start.get();
                    w.dispatch(UiAction::Effect {
                        action: EffectAction::CurvePoint {
                            layer,
                            key: key.clone(),
                            index: Some(index),
                            point: [
                                ((x + dx) / root.width() as f64) as f32,
                                1. - ((y + dy) / root.height() as f64) as f32,
                            ],
                            remove: false,
                        },
                    });
                }
            }
        ));
        root.add_controller(drag);
        let remove = gtk::GestureClick::new();
        remove.set_button(3);
        remove.connect_pressed(glib::clone!(
            #[weak]
            w,
            #[weak]
            root,
            #[strong]
            points,
            move |_, _, x, y| {
                let index = points.borrow().iter().position(|p| {
                    (p[0] * root.width() as f32 - x as f32)
                        .hypot((1. - p[1]) * root.height() as f32 - y as f32)
                        < 12.
                });
                if index.is_some() {
                    w.dispatch(UiAction::Effect {
                        action: EffectAction::CurvePoint {
                            layer,
                            key: key.clone(),
                            index,
                            point: [0.; 2],
                            remove: true,
                        },
                    });
                }
            }
        ));
        root.add_controller(remove);
        Self { root, points, range }
    }
}
