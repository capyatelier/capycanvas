//! GTK views of the shared effect/property schemas. No filter-specific widgets.
#[path = "gradient_preview.rs"]
mod gradient_preview;
use crate::{number_control::NumberControl, workspace::Workspace};
use adw::prelude::*;
use gtk::glib;
use layer_core::EffectValue;
use layer_ui::{
    ContactPhase, EffectAction, FilterPickerAction, LayerPropertiesView, PropertyKind, UiAction,
    UiState,
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
    picker_localization: RefCell<Option<std::sync::Arc<layer_ui::Localizer>>>,
    picker_headings: RefCell<HashMap<std::sync::Arc<str>, gtk::Label>>,
    picker_empty: RefCell<Option<gtk::Label>>,
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
    page: gtk::DropDown,
    property_actions: adw::WrapBox,
    resource_name: gtk::Label,
    tonal_histogram:Rc<crate::histogram::Inspector>,
    properties_updating: Cell<bool>,
    title: gtk::Label,
    body: gtk::Box,
    schema: RefCell<Option<LayerPropertiesView>>,
    property_document: Cell<u64>,
    property_localization: RefCell<Option<std::sync::Arc<layer_ui::Localizer>>>,
    fields: RefCell<Vec<Field>>,
    property_labels: RefCell<Vec<(usize, gtk::Label)>>,
    section_labels: RefCell<Vec<(usize, gtk::Label)>>,
    property_updating: Rc<Cell<bool>>,
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
impl Field {
    fn widget(&self) -> &gtk::Widget {
        match self {
            Self::Number(input) => input.upcast_ref(), Self::Toggle(input) => input.upcast_ref(), Self::Choice(input) => input.upcast_ref(),
            Self::Color(input) => input.widget.upcast_ref(), Self::Curve(input) => input.area.upcast_ref(), Self::Gradient(input) => input.root.upcast_ref(),
        }
    }
    fn row(&self, body: &gtk::Box) -> gtk::Widget {
        let mut widget=self.widget().clone();
        while let Some(parent)=widget.parent() {if parent==*body.upcast_ref::<gtk::Widget>() {break;}widget=parent;}
        widget
    }
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
        let category = crate::panel_controls::dropdown(&[]);
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
        let page = crate::panel_controls::dropdown(&[]);
        page.set_widget_name("properties-page");
        properties.append(&title);
        properties.append(&page);
        let property_actions = adw::WrapBox::new();
        property_actions.set_child_spacing(6);property_actions.set_line_spacing(6);
        let resource_name = gtk::Label::builder().xalign(0.).hexpand(true).width_chars(1)
            .ellipsize(gtk::pango::EllipsizeMode::Middle).build();
        resource_name.add_css_class("dim-label");resource_name.set_widget_name("property-resource-name");
        properties.append(&resource_name);
        properties.append(&property_actions);
        let tonal_histogram=crate::histogram::Inspector::new();
        tonal_histogram.root.set_widget_name("levels-histogram");tonal_histogram.root.set_visible(false);
        properties.append(&tonal_histogram.root);
        properties.append(&body);
        let stats = gtk::Box::new(gtk::Orientation::Vertical, 6);
        stats.add_css_class("renderer-stats");
        let recording_button = gtk::Button::with_label("Start stroke recording");
        recording_button.set_widget_name("stroke-recording");
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
            picker_localization: RefCell::new(None),
            picker_headings: RefCell::default(),
            picker_empty: RefCell::default(),
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
            page,
            property_actions,
            resource_name,
            tonal_histogram,
            properties_updating: Cell::new(false),
            title,
            body,
            schema: RefCell::new(None),
            property_document: Cell::new(0),
            property_localization: RefCell::new(None),
            fields: RefCell::new(Vec::new()),
            property_labels: RefCell::default(),
            section_labels: RefCell::default(),
            property_updating: Rc::new(Cell::new(false)),
            stats_labels: RefCell::new(Vec::new()),
            stats_plot,
            stats_samples,
        }
    }
    pub fn bind(self: &Rc<Self>, w: &Rc<Workspace>, state: &UiState) {
        if self.picker_bound.replace(true) {
            return;
        }
        w.on_localization(glib::clone!(#[weak(rename_to = button)] self.recording_button, #[upgrade_or] false, move |localization| {
            button.set_tooltip_text(Some(&layer_ui::NativeCopy::new(localization).color.record_tablet));
            true
        }));
        self.page.connect_selected_notify(glib::clone!(#[weak] w, #[weak(rename_to = this)] self, move |drop| {
            if this.properties_updating.get() { return; }
            let selection = this.schema.borrow().as_ref().and_then(|view| {
                Some((view.layer?, view.pages.get(drop.selected() as usize)?.id.clone()))
            });
            if let Some((layer, page)) = selection { w.dispatch(UiAction::Effect { action: EffectAction::SelectPage { layer, page } }); }
        }));
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
            .set_placeholder_text(Some(&state.filter_picker.search_label));
        self.search_button
            .set_tooltip_text(Some(&state.filter_picker.search_label));
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
        self.search_entry.connect_changed(glib::clone!(
            #[weak]
            w,
            move |entry| {
                if entry.is_visible() && !w.effects.picker_updating.get() {
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
        let localization = w.localization();
        let language_changed = self.picker_localization.borrow().as_ref().is_none_or(|old| !std::sync::Arc::ptr_eq(old, &localization));
        *self.picker_localization.borrow_mut() = Some(localization.clone());
        self.cancel_filter.set_label(&layer_ui::CommonCopy::new(&localization).cancel);
        self.search_entry.set_placeholder_text(Some(&state.filter_picker.search_label));
        self.search_entry.update_property(&[gtk::accessible::Property::Label(&state.filter_picker.search_label)]);
        self.search_button.set_tooltip_text(Some(&state.filter_picker.search_label));
        self.search_button.update_property(&[gtk::accessible::Property::Label(&state.filter_picker.search_label)]);
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
        if language_changed {
            *self.picker_categories.borrow_mut() = state.filter_categories.clone();
            for (button, choice) in self.type_buttons.borrow().iter().zip(&state.filter_categories) {
                if let Some(label) = button.child().and_then(|row| row.last_child()).and_downcast::<gtk::Label>() { label.set_label(&choice.label); label.set_tooltip_text(Some(&choice.label)); }
                button.set_tooltip_text(Some(&choice.label));
                button.update_property(&[gtk::accessible::Property::Label(&choice.label)]);
            }
            if let Some(model) = self.category.model().and_downcast::<gtk::StringList>() {
                model.splice(0, model.n_items(), &state.filter_categories.iter().map(|choice| choice.label.as_ref()).collect::<Vec<_>>());
            }
            for choice in &state.adjustments {
                if let Some((button, _)) = self.picker_rows.borrow().get(&choice.id) {
                    if let Some(label) = button.child().and_then(|body| body.last_child()).and_then(|caption| caption.last_child()).and_downcast::<gtk::Label>() { label.set_label(&choice.label); label.set_tooltip_text(Some(&choice.label)); }
                    button.set_tooltip_text(Some(&choice.tooltip));
                    button.update_property(&[gtk::accessible::Property::Label(&choice.label)]);
                }
                if let Some(label) = self.picker_headings.borrow().get(&choice.category) { label.set_label(&choice.category_label); label.set_tooltip_text(Some(&choice.category_label)); }
            }
            if let Some(empty) = self.picker_empty.borrow().as_ref() { empty.set_label(&state.filter_picker.empty_label); }
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
        self.search_entry.set_visible(picker.search.is_some());
        let query = picker.search.as_deref().unwrap_or("");
        if self.search_entry.text() != query {
            self.search_entry.set_text(query);
        }
        self.picker_updating.set(false);
        let ids: Vec<_> = state.adjustments.iter().map(|c| c.id.clone()).collect();
        if *self.picker_visible.borrow() == ids {
            return;
        }
        while let Some(child) = self.picker_body.first_child() {
            self.picker_body.remove(&child);
        }
        self.picker_headings.borrow_mut().clear();
        self.picker_empty.borrow_mut().take();
        let mut category = None;
        let mut rows = self.picker_rows.borrow_mut();
        for choice in &state.adjustments {
            if !self.split_picker && category.as_ref() != Some(&choice.category) {
                let heading =
                    crate::tool_panels::icon_label(&choice.category_label, choice.category_icon);
                heading.add_css_class("filter-category");
                self.picker_headings.borrow_mut().insert(choice.category.clone(), heading.last_child().and_downcast::<gtk::Label>().unwrap());
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
            if let Some(label) = row.0.child().and_then(|body| body.last_child()).and_then(|caption| caption.last_child()).and_downcast::<gtk::Label>() { label.set_label(&choice.label); label.set_tooltip_text(Some(&choice.label)); }
            row.0.set_tooltip_text(Some(&choice.tooltip));
            row.0.update_property(&[gtk::accessible::Property::Label(&choice.label)]);
            self.picker_body.append(&row.0);
            if picker.selected.as_ref() == Some(&choice.id) { row.0.add_css_class("selected-tool"); }
            else { row.0.remove_css_class("selected-tool"); }
        }
        if ids.is_empty() {
            let empty = gtk::Label::new(Some(&picker.empty_label));
            empty.add_css_class("dim-label");
            self.picker_body.append(&empty);
            self.picker_empty.replace(Some(empty));
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
    pub fn refresh_histograms(&self,w:&Rc<Workspace>,state:&UiState) {
        if state.layer_properties.histogram {self.tonal_histogram.refresh_tonal(w,state);}
        for field in self.fields.borrow().iter() {if let Field::Curve(editor)=field {editor.refresh_histogram(state);}}
    }
    pub fn refresh(self: &Rc<Self>, w: &Rc<Workspace>, state: &UiState) {
        self.bind(w, state);
        if self.adjustments.parent().is_some() || self.filter_types.parent().is_some() {
            self.refresh_picker(w, state);
        }
        if self.properties.parent().is_none() {
            return;
        }
        let localization = w.localization();
        let language_changed = self.property_localization.borrow().as_ref().is_none_or(|old| !std::sync::Arc::ptr_eq(old, &localization));
        *self.property_localization.borrow_mut() = Some(localization);
        let view = &state.layer_properties;
        let document_changed=self.property_document.replace(state.document_file.epoch)!=state.document_file.epoch;
        if self.schema.borrow().as_ref().is_none_or(|old| old.actions.iter().map(|action| &action.action).ne(view.actions.iter().map(|action| &action.action))) {
            while let Some(child) = self.property_actions.first_child() { self.property_actions.remove(&child); }
            for action in &view.actions {
                let button = gtk::Button::with_label(&action.label);
                if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                    label.set_wrap(true);
                    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                }
                button.set_widget_name("property-picker");
                let action = action.action.clone();
                button.connect_clicked(glib::clone!(#[weak] w, move |_| {
                    w.dispatch(UiAction::Effect { action: action.clone() });
                }));
                self.property_actions.append(&button);
            }
        }
        self.resource_name.set_label(view.resource_name.as_deref().unwrap_or(""));
        self.resource_name.set_tooltip_text(view.resource_name.as_deref());
        self.resource_name.set_visible(view.resource_name.is_some());
        let mut child = self.property_actions.first_child();
        for action in &view.actions {
            if let Some(button) = child.take() {
                if let Some(button) = button.downcast_ref::<gtk::Button>() { button.set_label(&action.label); button.set_tooltip_text(Some(&action.label)); }
                child = button.next_sibling();
            }
        }
        self.property_actions.set_sensitive(view.enabled);
        self.tonal_histogram.root.set_visible(view.histogram);
        self.title.set_text(&view.title);
        self.title.set_tooltip_text(Some(&view.description));
        self.body.set_sensitive(view.enabled);
        self.properties_updating.set(true);
        if self.schema.borrow().as_ref().is_none_or(|old| old.pages != view.pages) {
            let model = self.page.model().unwrap().downcast::<gtk::StringList>().unwrap();
            model.splice(0, model.n_items(), &view.pages.iter().map(|page| page.label.as_str()).collect::<Vec<_>>());
        }
        self.page.set_selected(view.pages.iter().position(|page| Some(&page.id) == view.page.as_ref()).map_or(gtk::INVALID_LIST_POSITION, |index| index as u32));
        self.page.set_visible(view.pages.len() > 1);
        self.properties_updating.set(false);
        let rebuild = self.schema.borrow().as_ref().is_none_or(|old| {
            document_changed || old.layer != view.layer
                || old.controls.len() != view.controls.len()
                || old.controls.iter().zip(&view.controls).any(|(a, b)| {
                    a.key != b.key
                        || !same_property_kind(&a.kind, &b.kind)
                        || a.section_id != b.section_id
                        || a.color_action != b.color_action
                        || a.curve.as_ref().map(|curve| curve.domain) != b.curve.as_ref().map(|curve| curve.domain)
                })
        });
        if rebuild {
            let old=self.schema.borrow();
            let labels=self.property_labels.borrow();
            let mut retained:HashMap<_,_>=std::mem::take(&mut *self.fields.borrow_mut()).into_iter().enumerate().filter_map(|(index,field)| {
                let old=old.as_ref().filter(|old|!document_changed && old.layer==view.layer)?.controls.get(index)?;
                let current=view.controls.iter().find(|control|control.key==old.key)?;
                if !same_property_kind(&old.kind,&current.kind) || old.color_action!=current.color_action
                    || old.curve.as_ref().map(|curve|curve.domain)!=current.curve.as_ref().map(|curve|curve.domain) {return None;}
                let row=field.row(&self.body);let label=labels.iter().find(|(i,_)|*i==index).map(|(_,label)|label.clone());
                Some((old.key.clone(),(field,row,label)))
            }).collect();
            drop(labels);drop(old);
            let mut child=self.body.first_child();
            while let Some(current)=child {
                child=current.next_sibling();
                if !retained.values().any(|(_,row,_)|*row==current) {self.body.remove(&current);}
            }
            self.property_labels.borrow_mut().clear();
            self.section_labels.borrow_mut().clear();
            if let Some(layer) = view.layer {
                let mut section = None;let mut previous:Option<gtk::Widget>=None;
                for (index, control) in view.controls.iter().enumerate() {
                    if section != control.section_id.as_ref() {
                        if index > 0 {
                            let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
                            divider.add_css_class("property-divider");
                            self.body.insert_child_after(&divider,previous.as_ref());previous=Some(divider.upcast());
                        }
                        section = control.section_id.as_ref();
                        if let Some(text) = control.section.as_deref() {
                            let heading = gtk::Label::new(Some(text));
                            heading.set_xalign(0.);
                            heading.add_css_class("heading");
                            heading.add_css_class("property-section");
                            self.body.insert_child_after(&heading,previous.as_ref());previous=Some(heading.clone().upcast());
                            self.section_labels.borrow_mut().push((index, heading));
                        }
                    }
                    if let Some((field,row,label))=retained.remove(&control.key) {
                        self.body.reorder_child_after(&row,previous.as_ref());previous=Some(row);
                        if let Some(label)=label {self.property_labels.borrow_mut().push((index,label));}
                        self.fields.borrow_mut().push(field);continue;
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
                            let input = NumberControl::new(numeric.clone(), &control.label, "", w.localization().clone());
                            input.set_widget_name(&format!("property-{}", control.key));
                            let key = control.key.clone();
                            bind_number(&input, w, move |value| EffectAction::Set { layer, key: key.clone(), value: EffectValue::Number(value as f32) });
                            self.body.append(&input);
                            Field::Number(input)
                        }
                        PropertyKind::Toggle => {
                            let input = gtk::Switch::new();
                            input.set_valign(gtk::Align::Center);
                            input.connect_active_notify(move |i| {
                                dispatch(EffectValue::Toggle(i.is_active()))
                            });
                            self.append_property_row(index, &control.label, &input);
                            Field::Toggle(input)
                        }
                        PropertyKind::Choice { options } => {
                            let input = crate::panel_controls::dropdown(
                                &options.iter().map(|s| s.as_ref()).collect::<Vec<_>>(),
                            );
                            let updating = self.property_updating.clone();
                            input.set_widget_name(&format!("property-{}", control.key));
                            input.connect_selected_notify(move |i| {
                                if !updating.get() { dispatch(EffectValue::Choice(i.selected())); }
                            });
                            self.append_property_row(index, &control.label, &input);
                            Field::Choice(input)
                        }
                        PropertyKind::Color => {
                            let input = crate::color_editor::ColorButton::new();
                            input.widget.set_widget_name(&format!("effect-color-{}", control.key));
                            input.bind(w, move |_, color| dispatch(EffectValue::Color(color)));
                            if let Some(action) = &control.color_action {
                                let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                                line.append(&input.widget);
                                let bucket = w.action_button("", action.clone());
                                bucket.set_widget_name(&format!("{}-bucket", control.key.replace('_', "-")));
                                crate::icons::set_button(&bucket, "layer-fill-symbolic");
                                let caption = bucket.downgrade();
                                w.on_localization(move |localization| {
                                    let Some(bucket) = caption.upgrade() else { return false; };
                                    let copy = layer_ui::NativeCopy::new(localization).color;
                                    bucket.set_tooltip_text(Some(&copy.use_selected));
                                    bucket.update_property(&[gtk::accessible::Property::Label(&copy.use_selected)]);
                                    true
                                });
                                line.append(&bucket);
                                self.append_property_row(index, &control.label, &line);
                            } else { self.append_property_row(index, &control.label, &input.widget); }
                            Field::Color(input)
                        }
                        PropertyKind::Curve => {
                            let input = CurveEditor::new(w, layer, control);
                            self.body.append(&input.root);
                            Field::Curve(input)
                        }
                        PropertyKind::Gradient => {
                            let input = GradientEditor::new(w, layer, &control.key);
                            self.body.append(&input.root);
                            Field::Gradient(input)
                        }
                    };
                    let row=field.row(&self.body);self.body.reorder_child_after(&row,previous.as_ref());previous=Some(row);
                    self.fields.borrow_mut().push(field);
                }
            }
        }
        self.property_updating.set(true);
        for (index, label) in self.property_labels.borrow().iter() {
            label.set_label(&view.controls[*index].label); label.set_tooltip_text(Some(&view.controls[*index].label));
        }
        for (index, heading) in self.section_labels.borrow().iter() { heading.set_label(view.controls[*index].section.as_deref().unwrap_or("")); }
        let old_schema = self.schema.borrow();
        for (index, (field, c)) in self.fields.borrow().iter().zip(&view.controls).enumerate() {
            if language_changed {
                field.widget().update_property(&[gtk::accessible::Property::Label(&c.label)]);
            }
            if language_changed || old_schema.as_ref().is_none_or(|old| old.controls.get(index).is_none_or(|old| old.label != c.label || old.kind != c.kind)) {
                match (field, &c.kind) {
                    (Field::Number(input), _) => input.set_caption(&c.label, "", w.localization()),
                    (Field::Choice(input), PropertyKind::Choice { options }) => {
                        if let Some(model) = input.model().and_downcast::<gtk::StringList>() {
                            let selected = input.selected(); model.splice(0, model.n_items(), &options.iter().map(|label| label.as_ref()).collect::<Vec<_>>()); input.set_selected(selected);
                        }
                    }
                    (Field::Gradient(input), _) => input.position.update_localization(w.localization()),
                    _ => {}
                }
            }
            match (field, &c.value) {
                (Field::Number(i), EffectValue::Number(v)) => i.set_value(*v as f64),
                (Field::Toggle(i), EffectValue::Toggle(v)) => i.set_active(*v),
                (Field::Choice(i), EffectValue::Choice(v)) => i.set_selected(*v),
                (Field::Color(i), EffectValue::Color(c)) => {
                    i.set_color(*c, w.view_color())
                }
                (Field::Curve(i), EffectValue::Curve(_)) => i.update(c, &w.localization()),
                (Field::Gradient(i), EffectValue::Gradient(stops)) => i.update(stops),
                _ => {}
            }
        }
        drop(old_schema);
        self.property_updating.set(false);
        *self.schema.borrow_mut() = Some(view.clone());
    }
    fn append_property_row(&self, index: usize, title: &str, input: &impl IsA<gtk::Widget>) {
        let row = row(title, input);
        self.property_labels.borrow_mut().push((index, row.first_child().and_downcast::<gtk::Label>().unwrap()));
        self.body.append(&row);
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
fn same_property_kind(a: &PropertyKind, b: &PropertyKind) -> bool {
    match (a, b) {
        (PropertyKind::Number { numeric: a }, PropertyKind::Number { numeric: b }) => a == b,
        (PropertyKind::Choice { options: a }, PropertyKind::Choice { options: b }) => a.len() == b.len(),
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
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
    position: NumberControl,
    sync: Rc<dyn Fn()>,
}
impl GradientEditor {
    fn new(w: &Rc<Workspace>, layer: u64, key: &str) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let bar = gradient_preview::GradientPreview::new();
        bar.set_widget_name("effect-gradient");
        let stops = Rc::new(RefCell::new(Vec::<layer_core::GradientStop>::new()));
        let selected = Rc::new(Cell::new(0usize));
        let updating = Rc::new(Cell::new(false));
        let color = crate::color_editor::ColorButton::new();
        color.bind_copy(w);
        color.widget.set_widget_name("effect-gradient-color");
        let position = NumberControl::new(layer_ui::NumericControl::percent(), "", "", w.localization().clone());
        position.set_widget_name("effect-gradient-position");
        let remove = crate::icons::button("layer-minus-symbolic");
        remove.set_widget_name("effect-gradient-remove");
        let reset = crate::icons::button("layer-reset-symbolic");
        reset.set_widget_name("effect-gradient-reset");
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let label = gtk::Label::new(None);
        label.set_widget_name("effect-gradient-color-label");
        label.set_hexpand(true);
        label.set_xalign(0.);
        actions.append(&label);
        actions.append(&color.widget);
        actions.append(&remove);
        actions.append(&reset);
        root.append(&bar);
        root.append(&position);
        root.append(&actions);
        w.on_localization(glib::clone!(#[weak] bar, #[weak] position, #[weak] remove, #[weak] reset, #[weak] label, #[upgrade_or] false, move |localization| {
            let copy = layer_ui::NativeCopy::new(localization).color;
            bar.set_tooltip_text(Some(&copy.add_stop));
            position.set_caption(&copy.position, "", localization.clone());
            remove.set_tooltip_text(Some(&copy.remove_stop));
            reset.set_tooltip_text(Some(&copy.reset_gradient));
            label.set_text(&copy.color);
            true
        }));
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
            position,
            sync: update,
        }
    }
    fn update(&self, stops: &[layer_core::GradientStop]) {
        *self.stops.borrow_mut() = stops.to_vec();
        (self.sync)();
    }
}
fn bind_number(input: &NumberControl, w: &Rc<Workspace>, action: impl Fn(f64) -> EffectAction + 'static) {
    let action = Rc::new(action);
    let captured = Rc::new(Cell::new(None));
    input.connect_edit_phase(glib::clone!(#[weak] w, #[weak] input, #[strong] captured, #[strong] action, move |phase| {
        captured.set(match phase { ContactPhase::Down | ContactPhase::Move => Some(ContactPhase::Move), ContactPhase::Cancel => Some(ContactPhase::Cancel), ContactPhase::Up => None });
        w.dispatch(UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(action(input.value())) } });
    }));
    input.connect_value_changed(glib::clone!(#[weak] w, move |input| {
        if captured.get() == Some(ContactPhase::Cancel) { return; }
        let action = action(input.value());
        w.dispatch(UiAction::Effect { action: if captured.get().is_some() {
            EffectAction::Gesture { phase: ContactPhase::Move, action: Box::new(action) }
        } else { action } });
    }));
}
struct CurveEditor {
    root: gtk::Box,
    area: gtk::DrawingArea,
    reset: gtk::Button,
    view: Rc<RefCell<layer_ui::PropertyControl>>,
    coordinates: [NumberControl; 2],
    ev: [gtk::Label; 2],
    axes: [[gtk::Label; 3]; 2],
    histogram: Rc<RefCell<layer_ui::HistogramView>>,
    histogram_colors: Rc<Cell<[[u8;3];4]>>,
    histogram_status: gtk::Label,
    clipping: [gtk::CheckButton;2],
    clipping_updating: Rc<Cell<bool>>,
}
impl CurveEditor {
    fn new(w: &Rc<Workspace>, layer: u64, control: &layer_ui::PropertyControl) -> Self {
        let curve = control.curve.as_ref().expect("shared curve controls");
        let histogram=Rc::new(RefCell::new(layer_ui::HistogramView::default()));
        let histogram_colors=Rc::new(Cell::new([[0;3];4]));
        let area = gtk::DrawingArea::builder().content_width(128).content_height(200)
            .hexpand(true).focusable(true).build();
        area.set_widget_name(&format!("property-{}-graph", control.key));
        area.add_css_class("customizable-target");
        area.add_css_class("curve-key-scope");
        area.set_tooltip_text(Some(&curve.help));
        area.update_property(&[gtk::accessible::Property::Label(&control.label)]);
        let view = Rc::new(RefCell::new(control.clone()));
        area.set_draw_func(glib::clone!(#[strong] view, #[strong] histogram, #[strong] histogram_colors, move |area, cr, width, height| {
            let view = view.borrow();
            let Some(curve) = &view.curve else { return; };
            let EffectValue::Curve(points) = &view.value else { return; };
            let (width, height) = (f64::from(width), f64::from(height));
            let c = area.color();
            let color = |alpha| cr.set_source_rgba(f64::from(c.red()), f64::from(c.green()), f64::from(c.blue()), alpha);
            color(0.12); cr.paint().ok();
            crate::histogram::draw(cr,&histogram.borrow(),histogram_colors.get(),width,height);
            color(0.2); cr.set_line_width(1.);
            for i in 1..4 {
                let t = f64::from(i) / 4.;
                cr.move_to(t * width, 0.); cr.line_to(t * width, height);
                cr.move_to(0., t * height); cr.line_to(width, t * height);
            }
            cr.stroke().ok();
            cr.set_dash(&[3., 3.], 0.);
            if let Some(white) = curve.axes[0].white {
                cr.move_to(f64::from(white) * width, 0.); cr.line_to(f64::from(white) * width, height);
            }
            if let Some(white) = curve.axes[1].white {
                cr.move_to(0., (1. - f64::from(white)) * height); cr.line_to(width, (1. - f64::from(white)) * height);
            }
            cr.stroke().ok(); cr.set_dash(&[], 0.);
            color(1.); cr.set_line_width(1.5);
            for (index, [x, y]) in view.plot.iter().enumerate() {
                let (x, y) = (f64::from(*x) * width, (1. - f64::from(*y)) * height);
                if index == 0 { cr.move_to(x, y); } else { cr.line_to(x, y); }
            }
            cr.stroke().ok();
            for (index, point) in points.iter().enumerate() {
                let (x, y) = (f64::from(point[0]) * width, (1. - f64::from(point[1])) * height);
                cr.arc(x, y, 3.5, 0., std::f64::consts::TAU); cr.fill().ok();
                if curve.selected == Some(index) {
                    cr.arc(x, y, 6., 0., std::f64::consts::TAU); cr.stroke().ok();
                }
            }
        }));
        let key: Rc<str> = control.key.as_str().into();
        let capture = Rc::new(Cell::new(None::<(u64, [f64; 2], [f64; 2], [f32; 2])>));
        let removing = Rc::new(Cell::new(false));
        let drag = gtk::GestureDrag::new(); drag.set_button(1);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        drag.connect_drag_begin(glib::clone!(#[weak] w, #[weak] area, #[strong] view, #[strong] key, #[strong] capture, #[strong] removing, move |gesture, x, y| {
            if removing.get() { return; }
            let Some((sx, sy)) = gesture.current_event().and_then(|event| event.position()) else { return; };
            let epoch = view.borrow().curve.as_ref().unwrap().epoch;
            let extent = [area.width() as f32, area.height() as f32];
            area.grab_focus(); capture.set(Some((epoch, [x, y], [sx, sy], extent)));
            gesture.set_state(gtk::EventSequenceState::Claimed);
            w.dispatch(UiAction::Effect { action: EffectAction::CurveContact { layer, key: key.to_string(), epoch,
                phase: ContactPhase::Down, point: [x as f32, y as f32], extent } });
        }));
        for (phase, ending) in [(ContactPhase::Move, false), (ContactPhase::Up, true)] {
            let callback = glib::clone!(#[weak] w, #[strong] key, #[strong] capture, move |gesture: &gtk::GestureDrag, _: f64, _: f64| {
                let Some((epoch, [x, y], [sx, sy], extent)) = (if ending { capture.take() } else { capture.get() }) else { return; };
                let Some((px, py)) = gesture.current_event().and_then(|event| event.position()) else {
                    if ending { w.dispatch(UiAction::Effect { action: EffectAction::CurveContact { layer, key: key.to_string(), epoch,
                        phase: ContactPhase::Cancel, point: [0.; 2], extent: [0.; 2] } }); }
                    return;
                };
                w.dispatch(UiAction::Effect { action: EffectAction::CurveContact { layer, key: key.to_string(), epoch,
                    phase, point: [(x + (px - sx)) as f32, (y + (py - sy)) as f32], extent } });
            });
            if ending { drag.connect_drag_end(callback); } else { drag.connect_drag_update(callback); }
        }
        drag.connect_cancel(glib::clone!(#[weak] w, #[strong] key, #[strong] capture, move |_, _| {
            if let Some((epoch, ..)) = capture.take() {
                w.dispatch(UiAction::Effect { action: EffectAction::CurveContact { layer, key: key.to_string(), epoch,
                    phase: ContactPhase::Cancel, point: [0.; 2], extent: [0.; 2] } });
            }
        }));
        area.add_controller(drag.clone());
        let click = gtk::GestureClick::new(); click.set_button(0);
        let point_count = Rc::new(Cell::new(0));
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(#[weak] w, #[weak] area, #[strong] view, #[strong] key, #[strong] removing, #[strong] point_count, move |gesture, count, x, y| {
            if count == 1 && let EffectValue::Curve(points) = &view.borrow().value {point_count.set(points.len());}
            removing.set(gesture.current_button() == 3 || count == 2);
            if !removing.get() { return; }
            area.grab_focus();
            let epoch = view.borrow().curve.as_ref().unwrap().epoch;
            w.dispatch(UiAction::Effect { action: EffectAction::CurveRemoveAt { layer, key: key.to_string(), epoch,
                point: [x as f32, y as f32], extent: [area.width() as f32, area.height() as f32],
                point_count: (gesture.current_button() != 3).then(|| point_count.get()) } });
        }));
        area.add_controller(click.clone()); click.group_with(&drag);
        let keys = gtk::EventControllerKey::new();
        let dispatch_key = Rc::new(glib::clone!(#[weak] w, #[strong] view, #[strong] key, #[upgrade_or] false, move |native, pressed, modifiers| {
            let layer_ui::UiInput::Key { key: key_event, repeat, modifiers, .. } = crate::input::key_input(native, pressed, modifiers, false, None) else { return false; };
            if !matches!(key_event.as_str(), "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Delete" | "Backspace" | "Escape")
                || pressed && key_event != "Escape" && (modifiers.command || modifiers.alt) { return false; }
            let epoch = view.borrow().curve.as_ref().unwrap().epoch;
            w.dispatch(UiAction::Effect { action: EffectAction::CurveKey { layer, key: key.to_string(), epoch, key_event, pressed, repeat, modifiers } });
            true
        }));
        keys.connect_key_pressed(glib::clone!(#[strong] dispatch_key, move |_, key, _, modifiers| {
            if dispatch_key(key, true, modifiers) { glib::Propagation::Stop } else { glib::Propagation::Proceed }
        }));
        keys.connect_key_released(move |_, key, _, modifiers| { dispatch_key(key, false, modifiers); });
        area.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(glib::clone!(#[weak] w, #[strong] view, #[strong] key, #[strong] capture, move |_| {
            let epoch = capture.take().map_or_else(|| view.borrow().curve.as_ref().unwrap().epoch, |(epoch, ..)| epoch);
            w.dispatch(UiAction::Effect { action: EffectAction::CurveContact { layer, key: key.to_string(), epoch,
                phase: ContactPhase::Cancel, point: [0.; 2], extent: [0.; 2] } });
        }));
        area.add_controller(focus);
        let reset = crate::icons::button("layer-reset-symbolic");
        reset.add_css_class("flat"); reset.add_css_class("circular");
        reset.set_halign(gtk::Align::End); reset.set_valign(gtk::Align::End);
        reset.set_widget_name("curve-reset");
        reset.set_tooltip_text(Some(&curve.reset_label));
        reset.connect_clicked(glib::clone!(#[weak] w, #[strong] key, move |_| {
            w.dispatch(UiAction::Effect { action: EffectAction::Reset { layer, key: key.to_string() } });
        }));
        let overlay = gtk::Overlay::builder().child(&area).build(); overlay.add_overlay(&reset);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.set_vexpand(false); root.set_valign(gtk::Align::Start);
        let graph = gtk::Grid::builder().column_spacing(6).row_spacing(4).build();
        let vertical = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let maximum = gtk::Label::new(Some(&curve.axes[1].maximum));
        let title = gtk::Label::new(Some(&curve.axes[1].label)); title.set_vexpand(true);
        let minimum = gtk::Label::new(Some(&curve.axes[1].minimum));
        vertical.add_css_class("dim-label"); vertical.append(&maximum); vertical.append(&title); vertical.append(&minimum);
        let y_axis = [minimum.clone(), title.clone(), maximum.clone()];
        graph.attach(&vertical, 0, 0, 1, 1); graph.attach(&overlay, 1, 0, 1, 1);
        let horizontal = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let minimum = gtk::Label::new(Some(&curve.axes[0].minimum));
        let title = gtk::Label::new(Some(&curve.axes[0].label)); title.set_hexpand(true);
        let maximum = gtk::Label::new(Some(&curve.axes[0].maximum));
        horizontal.add_css_class("dim-label"); horizontal.append(&minimum); horizontal.append(&title); horizontal.append(&maximum);
        let axes = [[minimum.clone(), title.clone(), maximum.clone()], y_axis];
        graph.attach(&horizontal, 1, 1, 1, 1); root.append(&graph);
        let ev = std::array::from_fn(|_| { let label = gtk::Label::new(None); label.set_xalign(1.); label.add_css_class("dim-label"); label });
        let coordinates = std::array::from_fn(|index| {
            let input = NumberControl::new(curve.domain.numeric(), &curve.axes[index].label, "", w.localization().clone());
            input.set_widget_name(&format!("property-{}-{}", control.key, if index == 0 { "input" } else { "output" }));
            bind_number(&input, w, glib::clone!(#[strong] view, #[strong] key, move |value| {
                EffectAction::CurveNumber { layer, key: key.to_string(), epoch: view.borrow().curve.as_ref().unwrap().epoch,
                    axis: if index == 0 { layer_ui::CurveAxis::Input } else { layer_ui::CurveAxis::Output },
                    operation: layer_ui::NumericOperation::Value { value } }
            }));
            root.append(&input); root.append(&ev[index]); input
        });
        let histogram_status=gtk::Label::builder().xalign(0.).wrap(true).build();histogram_status.set_widget_name("curve-statistics");
        root.append(&histogram_status);
        let clipping_updating=Rc::new(Cell::new(false));
        let clipping=std::array::from_fn(|index| {
            let button=gtk::CheckButton::new();button.set_widget_name(if index==0 {"curve-shadows"} else {"curve-highlights"});
            button.connect_toggled(glib::clone!(#[weak] w, #[strong] clipping_updating, move |button| {
                if !clipping_updating.get() {w.dispatch(UiAction::Histogram {action:if index==0 {layer_ui::HistogramAction::Shadows {enabled:button.is_active()}}
                    else {layer_ui::HistogramAction::Highlights {enabled:button.is_active()}}});}
            }));root.append(&button);button
        });
        let editor = Self { root, area, reset, view, coordinates, ev, axes, histogram, histogram_colors, histogram_status, clipping, clipping_updating }; editor.update(control, &w.localization()); editor
    }
    fn refresh_histogram(&self,state:&UiState) {
        *self.histogram.borrow_mut()=state.tonal_histogram.clone();
        self.histogram_colors.set(state.palette.histogram_colors().map(|color|color.0));
        self.histogram_status.set_label(&state.tonal_histogram.status);
        self.clipping_updating.set(true);
        for (index,button) in self.clipping.iter().enumerate() {
            button.set_label(state.histogram.labels.get(index+1).map(|s|s.as_ref()));
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_wrap(true);
                label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            }
            button.set_tooltip_text(state.histogram.labels.get(index+1).map(|s|s.as_ref()));
            button.set_active(if index==0 {state.histogram.shadows} else {state.histogram.highlights});
        }
        self.clipping_updating.set(false);self.area.queue_draw();
    }
    fn update(&self, control: &layer_ui::PropertyControl, localization: &std::sync::Arc<layer_ui::Localizer>) {
        *self.view.borrow_mut() = control.clone();
        let curve = control.curve.as_ref().unwrap();
        self.area.set_tooltip_text(Some(&curve.help));
        self.reset.set_tooltip_text(Some(&curve.reset_label));
        for (labels, axis) in self.axes.iter().zip(&curve.axes) {
            for (label, text) in labels.iter().zip([&axis.minimum, &axis.label, &axis.maximum]) { label.set_label(text); }
        }
        for (index, coordinate) in [&curve.input, &curve.output].into_iter().enumerate() {
            let input = &self.coordinates[index];
            input.set_caption(&curve.axes[index].label, "", localization.clone());
            input.set_sensitive(coordinate.as_ref().is_some_and(|value| !value.read_only));
            let (value, text) = coordinate.as_ref().map_or((0., ""), |value| (value.value, value.text.as_str()));
            input.set_presented_value(value, text);
            self.ev[index].set_label(coordinate.as_ref().and_then(|value| value.ev.as_deref()).unwrap_or(""));
            self.ev[index].set_visible(matches!(curve.domain, layer_ui::CurveDomain::LogHdr { .. }));
        }
        self.reset.set_visible(control.modified); self.area.queue_draw();
    }
}

pub fn owns_native_key(focus: &gtk::Widget, key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType) -> bool {
    focus.has_css_class("curve-key-scope") && (key == gtk::gdk::Key::Escape
        || !modifiers.intersects(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::ALT_MASK | gtk::gdk::ModifierType::META_MASK)
            && matches!(key, gtk::gdk::Key::Left | gtk::gdk::Key::Right | gtk::gdk::Key::Up | gtk::gdk::Key::Down | gtk::gdk::Key::Delete | gtk::gdk::Key::BackSpace))
}
