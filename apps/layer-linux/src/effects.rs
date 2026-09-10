//! GTK views of the shared effect/property schemas. No filter-specific widgets.
use crate::{number_control::NumberControl, workspace::Workspace};
use gtk::{glib, prelude::*};
use layer_core::EffectValue;
use layer_render::CanvasRenderer;
use layer_ui::{
    EffectAction, FilterPickerAction, LayerPropertiesView, PropertyKind, UiAction, UiState,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct PreviewKey {
    revision: (u64, u64),
    size: [u32; 2],
}

pub struct EffectPanels {
    pub adjustments: gtk::Box,
    picker_body: gtk::Box,
    picker_scroller: gtk::ScrolledWindow,
    category: gtk::DropDown,
    search_button: gtk::Button,
    search_entry: gtk::SearchEntry,
    picker_bound: Cell<bool>,
    picker_visible: RefCell<Vec<std::sync::Arc<str>>>,
    picker_rows: RefCell<HashMap<std::sync::Arc<str>, (gtk::Button, gtk::Picture)>>,
    preview_key: Cell<Option<PreviewKey>>,
    preview_loaded: RefCell<HashSet<std::sync::Arc<str>>>,
    preview_request: Cell<u64>,
    preview_pending: Cell<Option<(u64, PreviewKey)>>,
    pub properties: gtk::Box,
    pub stats: gtk::Box,
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
    Color(gtk::ColorDialogButton),
    Curve(CurveEditor),
    Gradient(GradientEditor),
}
impl EffectPanels {
    pub fn new() -> Self {
        let adjustments = gtk::Box::new(gtk::Orientation::Vertical, 6);
        adjustments.add_css_class("filter-picker");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let category = gtk::DropDown::from_strings(&[]);
        category.set_hexpand(true);
        let search_entry = gtk::SearchEntry::builder()
            .hexpand(true)
            .visible(false)
            .build();
        let search_button = gtk::Button::from_icon_name("system-search-symbolic");
        search_button.add_css_class("flat");
        header.append(&category);
        header.append(&search_entry);
        header.append(&search_button);
        let picker_body = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&picker_body)
            .build();
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
            adjustments,
            picker_body,
            picker_scroller: scroller,
            category,
            search_button,
            search_entry,
            picker_bound: Cell::new(false),
            picker_visible: RefCell::new(Vec::new()),
            picker_rows: RefCell::new(HashMap::new()),
            preview_key: Cell::new(None),
            preview_loaded: RefCell::new(HashSet::new()),
            preview_request: Cell::new(0),
            preview_pending: Cell::new(None),
            properties,
            stats,
            title,
            body,
            schema: RefCell::new(None),
            fields: RefCell::new(Vec::new()),
            stats_labels: RefCell::new(Vec::new()),
            stats_plot,
            stats_samples,
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>, state: &UiState) {
        if self.picker_bound.replace(true) {
            return;
        }
        let weak = Rc::downgrade(w);
        glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            let Some(w) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if w.effects.stats.is_mapped() {
                w.effects.refresh_stats(&w);
            }
            w.effects.refresh_previews(&w);
            glib::ControlFlow::Continue
        });
        self.category.set_model(Some(&gtk::StringList::new(
            &state
                .filter_categories
                .iter()
                .map(|c| c.label.as_ref())
                .collect::<Vec<_>>(),
        )));
        let categories = state.filter_categories.clone();
        self.category.connect_selected_notify(glib::clone!(
            #[weak]
            w,
            move |drop| {
                if let Some(choice) = categories.get(drop.selected() as usize) {
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
            move |_| {
                w.dispatch(UiAction::FilterPicker {
                    action: FilterPickerAction::ToggleSearch,
                });
                if w.effects.search_entry.is_visible() {
                    w.effects.search_entry.grab_focus();
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
        let picker = &state.filter_picker;
        self.category.set_visible(picker.search.is_none());
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
            if category.as_ref() != Some(&choice.category) {
                let heading = gtk::Label::new(Some(&choice.category_label));
                heading.set_xalign(0.);
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
                    let icon = gtk::Image::from_icon_name("layer-animation-symbolic");
                    icon.set_pixel_size(12);
                    icon.add_css_class("dim-label");
                    caption.append(&icon);
                }
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
        }
        if ids.is_empty() {
            let empty = gtk::Label::new(Some(picker.empty_label));
            empty.add_css_class("dim-label");
            self.picker_body.append(&empty);
        }
        *self.picker_visible.borrow_mut() = ids;
    }
    fn refresh_previews(&self, w: &Workspace) {
        let mut gpu = w.gpu.borrow_mut();
        let Some(gpu) = gpu.as_mut() else {
            return;
        };
        let revision = gpu.session.filter_preview_revision();
        while let Some(result) = gpu.session.renderer_mut().take_filter_previews() {
            let pending = self.preview_pending.take();
            let Ok(result) = result else {
                continue;
            };
            if pending.is_none_or(|(id, key)| {
                id != result.image.request_id
                    || key.revision != revision
                    || Some(key) != self.preview_key.get()
            }) {
                continue;
            }
            let count = result.filters.len();
            let height = result.image.height / count as u32;
            let stride = result.image.stride as usize;
            let bytes = glib::Bytes::from_owned(result.image.bytes);
            let rows = self.picker_rows.borrow();
            for (i, id) in result.filters.into_iter().enumerate() {
                if let Some((_, picture)) = rows.get(&id) {
                    let start = i * height as usize * stride;
                    let row =
                        glib::Bytes::from_bytes(&bytes, start..start + height as usize * stride);
                    let texture = gtk::gdk::MemoryTexture::new(
                        result.image.width as i32,
                        height as i32,
                        gtk::gdk::MemoryFormat::R8g8b8a8,
                        &row,
                        stride,
                    );
                    picture.set_paintable(Some(&texture));
                    self.preview_loaded.borrow_mut().insert(id);
                }
            }
        }
        if !self.adjustments.is_mapped() || self.preview_pending.get().is_some() {
            return;
        }
        let rows = self.picker_rows.borrow();
        let on_screen: Vec<_> = self
            .picker_visible
            .borrow()
            .iter()
            .filter_map(|id| {
                let (_, picture) = rows.get(id)?;
                let rect = picture.compute_bounds(&self.picker_scroller)?;
                (rect.y() + rect.height() > 0. && rect.y() < self.picker_scroller.height() as f32)
                    .then_some((id.clone(), picture))
            })
            .collect();
        let Some((_, first)) = on_screen.first() else {
            return;
        };
        let scale = first.scale_factor() as u32;
        let size = [
            (first.width().max(1) as u32 * scale).clamp(80, 512),
            (40 * scale).min(128),
        ];
        let key = PreviewKey { revision, size };
        if self.preview_key.get() != Some(key) {
            self.preview_loaded.borrow_mut().clear();
            self.preview_key.set(Some(key));
        }
        let filters = on_screen
            .into_iter()
            .filter_map(|(id, _)| (!self.preview_loaded.borrow().contains(&id)).then_some(id))
            .take(8)
            .collect::<Vec<_>>();
        if filters.is_empty() {
            return;
        }
        let id = self.preview_request.get().wrapping_add(1);
        if gpu
            .session
            .request_filter_previews(id, filters, size)
            .unwrap_or(false)
        {
            self.preview_request.set(id);
            self.preview_pending.set(Some((id, key)));
        }
    }
    pub fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        self.bind(w, state);
        self.refresh_picker(w, state);
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
                for (index, control) in view.controls.iter().enumerate() {
                    if section != control.section.as_deref() {
                        if index > 0 {
                            let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
                            divider.add_css_class("property-divider");
                            self.body.append(&divider);
                        }
                        section = control.section.as_deref();
                        if let Some(text) = section {
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
                            input.connect_value_changed(move |i| {
                                dispatch(EffectValue::Number(i.value() as f32))
                            });
                            self.body.append(&input);
                            Field::Number(input)
                        }
                        PropertyKind::Toggle => {
                            let input = gtk::Switch::new();
                            input.set_valign(gtk::Align::Center);
                            input.connect_active_notify(move |i| {
                                dispatch(EffectValue::Toggle(i.is_active()))
                            });
                            self.body.append(&row(&control.label, &input));
                            Field::Toggle(input)
                        }
                        PropertyKind::Choice { options } => {
                            let input = gtk::DropDown::from_strings(
                                &options.iter().map(|s| s.as_ref()).collect::<Vec<_>>(),
                            );
                            input.connect_selected_notify(move |i| {
                                dispatch(EffectValue::Choice(i.selected()))
                            });
                            self.body.append(&row(&control.label, &input));
                            Field::Choice(input)
                        }
                        PropertyKind::Color => {
                            let input = gtk::ColorDialogButton::new(Some(
                                gtk::ColorDialog::builder().with_alpha(true).build(),
                            ));
                            input.connect_rgba_notify(move |i| {
                                let c = i.rgba();
                                dispatch(EffectValue::Color([
                                    c.red(),
                                    c.green(),
                                    c.blue(),
                                    c.alpha(),
                                ]));
                            });
                            self.body.append(&row(&control.label, &input));
                            Field::Color(input)
                        }
                        PropertyKind::Curve => {
                            let input = CurveEditor::new(w, layer, &control.key);
                            curve_stack.add_named(&input.root, Some(&control.key));
                            Field::Curve(input)
                        }
                        PropertyKind::Gradient => {
                            let input = GradientEditor::new(w, layer, &control.key);
                            self.body.append(&input.root);
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
                    i.set_rgba(&gtk::gdk::RGBA::new(c[0], c[1], c[2], c[3]))
                }
                (Field::Curve(i), EffectValue::Curve(p)) => {
                    *i.points.borrow_mut() = p.clone();
                    i.root.queue_draw();
                }
                (Field::Gradient(i), EffectValue::Gradient(stops)) => i.update(stops),
                _ => {}
            }
        }
        *self.schema.borrow_mut() = Some(view.clone());
    }
    fn refresh_stats(&self, w: &Workspace) {
        let Some(view) = w.gpu.borrow().as_ref().map(|g| g.session.renderer_stats()) else {
            return;
        };
        if self.stats_labels.borrow().is_empty() {
            for metric in &view.rows {
                let value = gtk::Label::new(None);
                value.set_xalign(1.);
                value.add_css_class("numeric");
                let row = row(metric.label, &value);
                row.set_tooltip_text(Some(metric.description));
                self.stats.append(&row);
                self.stats_labels.borrow_mut().push(value);
            }
            self.stats_plot.set_tooltip_text(Some(view.chart_label));
            self.stats.append(&self.stats_plot);
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
        let bar = gtk::DrawingArea::builder()
            .content_width(160)
            .content_height(44)
            .hexpand(true)
            .build();
        bar.set_tooltip_text(Some("Click to select a color stop or add one."));
        bar.set_widget_name("effect-gradient");
        let stops = Rc::new(RefCell::new(Vec::<layer_core::GradientStop>::new()));
        let selected = Rc::new(Cell::new(0usize));
        let updating = Rc::new(Cell::new(false));
        let color =
            gtk::ColorDialogButton::new(Some(gtk::ColorDialog::builder().with_alpha(true).build()));
        let position = NumberControl::new(layer_ui::NumericControl::percent(), "Position", "");
        let remove = gtk::Button::from_icon_name("layer-minus-symbolic");
        remove.set_tooltip_text(Some("Remove color stop"));
        let reset = gtk::Button::from_icon_name("layer-undo-symbolic");
        reset.set_tooltip_text(Some("Reset gradient"));
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let label = gtk::Label::new(Some("Color"));
        label.set_hexpand(true);
        label.set_xalign(0.);
        actions.append(&label);
        actions.append(&color);
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
        bar.set_draw_func(glib::clone!(
            #[strong]
            stops,
            #[strong]
            selected,
            move |area, cr, width, height| {
                let stops = stops.borrow();
                if stops.len() < 2 {
                    return;
                }
                let ramp = gtk::cairo::LinearGradient::new(6., 0., width as f64 - 6., 0.);
                for s in stops.iter() {
                    ramp.add_color_stop_rgba(
                        s.position as f64,
                        s.color[0] as f64,
                        s.color[1] as f64,
                        s.color[2] as f64,
                        s.color[3] as f64,
                    );
                }
                cr.set_source(&ramp).ok();
                cr.rectangle(6., 0., width as f64 - 12., height as f64 - 12.);
                cr.fill().ok();
                let c = area.color();
                cr.set_source_rgb(c.red() as f64, c.green() as f64, c.blue() as f64);
                for (i, s) in stops.iter().enumerate() {
                    let x = 6. + s.position as f64 * (width as f64 - 12.);
                    let y = height as f64 - 5.;
                    cr.arc(
                        x,
                        y,
                        if selected.get() == i { 4. } else { 2.5 },
                        0.,
                        std::f64::consts::TAU,
                    );
                    cr.fill().ok();
                }
            }
        ));
        let update: Rc<dyn Fn()> = Rc::new(glib::clone!(
            #[strong]
            updating,
            #[strong]
            stops,
            #[strong]
            selected,
            #[weak]
            color,
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
                    color.set_rgba(&gtk::gdk::RGBA::new(
                        s.color[0], s.color[1], s.color[2], s.color[3],
                    ));
                    position.set_value(s.position as f64);
                    position.set_sensitive(index > 0 && index + 1 < list.len());
                    remove.set_sensitive(index > 0 && index + 1 < list.len());
                }
                updating.set(false);
                bar.queue_draw();
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
        color.connect_rgba_notify(glib::clone!(
            #[strong]
            updating,
            #[strong]
            selected,
            #[strong]
            stops,
            #[strong]
            change,
            move |button| {
                if updating.get() {
                    return;
                }
                let i = selected.get();
                let position = stops.borrow().get(i).map(|s| s.position);
                if let Some(p) = position {
                    let c = button.rgba();
                    change(
                        Some(i),
                        p,
                        Some([c.red(), c.green(), c.blue(), c.alpha()]),
                        false,
                    );
                }
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
        root.set_draw_func(glib::clone!(
            #[strong]
            points,
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
        Self { root, points }
    }
}
