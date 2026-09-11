//! Native projections of shared tool controls and color-wheel geometry.
use crate::{
    number_control::NumberControl,
    workspace::{Workspace, selected},
};
use gtk::{cairo, glib, prelude::*, subclass::prelude::*};
use layer_ui::{
    ColorAction, ColorSlot, ColorSpace, ColorState, ColorWheelGeometry, Theme, ToolSetItem,
    ToolSetView, ToolSetting, ToolSettingAction, UiAction, UiState,
};

/// A body can be projected in a dock or a tool drawer without reparenting the
/// other view. Preview textures are shared by the existing immutable cache.
pub struct ToolSet {
    pub root: gtk::Box,
    groups: gtk::FlowBox,
    list: gtk::Box,
    pub group_buttons: RefCell<Vec<gtk::Button>>,
    pub buttons: RefCell<Vec<(ToolSetItem, gtk::Button, Option<gtk::Picture>)>>,
    view: RefCell<ToolSetView>,
    theme: Cell<Option<Theme>>,
}
impl ToolSet {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let groups = gtk::FlowBox::builder()
            .homogeneous(true)
            .min_children_per_line(1)
            .max_children_per_line(16)
            .selection_mode(gtk::SelectionMode::None)
            .column_spacing(2)
            .row_spacing(2)
            .build();
        groups.add_css_class("tool-groups");
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        root.append(&groups);
        root.append(&list);
        Self {
            root,
            groups,
            list,
            group_buttons: RefCell::default(),
            buttons: RefCell::default(),
            view: RefCell::default(),
            theme: Cell::new(None),
        }
    }
    pub fn refresh(&self, workspace: &Rc<Workspace>, view: &ToolSetView, theme: Theme) {
        let same = |a: &[ToolSetItem], b: &[ToolSetItem]| {
            a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| {
                    a.label == b.label
                        && a.action == b.action
                        && a.icon == b.icon
                        && a.preview == b.preview
                })
        };
        let previous = self.view.borrow();
        if !same(&previous.groups, &view.groups) {
            while let Some(child) = self.groups.first_child() {
                self.groups.remove(&child);
            }
            let mut buttons = self.group_buttons.borrow_mut();
            buttons.clear();
            for item in &view.groups {
                let button = workspace.action_button(item.label, item.action.clone());
                button.add_css_class("flat");
                button.add_css_class("tool-group");
                button.set_halign(gtk::Align::Start);
                button.set_size_request(
                    (layer_ui::TOOL_PANEL_MIN_WIDTH - 2.0 * layer_ui::PANEL_CONTENT_INSET) as i32,
                    layer_ui::TILE_SIZE as i32,
                );
                button.set_child(Some(&tool_label(item)));
                self.groups.insert(&button, -1);
                buttons.push(button);
            }
        }
        let rebuild = !same(&previous.subtools, &view.subtools);
        if rebuild {
            while let Some(child) = self.list.first_child() {
                self.list.remove(&child);
            }
            let mut buttons = self.buttons.borrow_mut();
            buttons.clear();
            for item in &view.subtools {
                let button = workspace.action_button(item.label, item.action.clone());
                button.add_css_class("flat");
                button.add_css_class("brush-choice");
                let preview = item.preview.map(|id| {
                    button.set_widget_name(&format!("brush-{id}"));
                    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    let preview = gtk::Picture::builder()
                        .can_shrink(true)
                        .content_fit(gtk::ContentFit::Fill)
                        .height_request(40)
                        .build();
                    let label = gtk::Label::new(Some(item.label));
                    label.set_halign(gtk::Align::End);
                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    label.set_tooltip_text(Some(item.label));
                    content.append(&preview);
                    content.append(&label);
                    button.set_child(Some(&content));
                    preview
                });
                if preview.is_none() {
                    button.set_child(Some(&tool_label(item)));
                }
                self.list.append(&button);
                buttons.push((item.clone(), button, preview));
            }
        }
        for (button, item) in self.group_buttons.borrow().iter().zip(&view.groups) {
            selected(button, item.selected);
        }
        let theme_changed = self.theme.replace(Some(theme)) != Some(theme);
        for ((_, button, preview), item) in self.buttons.borrow().iter().zip(&view.subtools) {
            selected(button, item.selected);
            if (rebuild || theme_changed)
                && let (Some(id), Some(preview)) = (item.preview, preview)
            {
                preview.set_paintable(Some(&crate::previews::texture(id, theme)));
            }
        }
        drop(previous);
        *self.view.borrow_mut() = view.clone();
    }
}
fn tool_label(item: &ToolSetItem) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.set_valign(gtk::Align::Center);
    row.append(&gtk::Image::from_icon_name(&format!(
        "layer-{}-symbolic",
        item.icon
    )));
    let label = gtk::Label::new(Some(item.label));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(1);
    label.set_tooltip_text(Some(item.label));
    row.append(&label);
    row
}
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct ToolSettings {
    pub root: gtk::Box,
    fields: RefCell<Vec<(ToolSetting, NumberControl)>>,
    actions: RefCell<Vec<(ToolSettingAction, gtk::Widget)>>,
    updating: Rc<Cell<bool>>,
}
impl ToolSettings {
    pub fn new() -> Self {
        Self {
            root: body(),
            fields: RefCell::default(),
            actions: RefCell::default(),
            updating: Rc::new(Cell::new(false)),
        }
    }
    pub fn refresh(&self, workspace: &Rc<Workspace>, state: &UiState) {
        let controls = &state.tool_settings;
        self.updating.set(true);
        let mut fields = self.fields.borrow_mut();
        let mut actions = self.actions.borrow_mut();
        let same_schema = fields.len() == controls.len()
            && fields.iter().zip(controls).all(|((old, _), next)| {
                old.id == next.id
                    && old.numeric == next.numeric
                    && old.group == next.group
                    && old.label == next.label
            })
            && actions.len() == state.tool_actions.len()
            && actions
                .iter()
                .zip(&state.tool_actions)
                .all(|((old, _), next)| old == next);
        if !same_schema {
            while let Some(child) = self.root.first_child() {
                self.root.remove(&child);
            }
            fields.clear();
            actions.clear();
            let mut group = "";
            for control in controls {
                if group != control.group {
                    group = control.group;
                    if !group.is_empty() {
                        let title = gtk::Label::new(Some(group));
                        title.add_css_class("heading");
                        title.add_css_class("dim-label");
                        title.set_xalign(0.0);
                        title.set_margin_top(6);
                        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
                        self.root.append(&title);
                    }
                }
                let input = NumberControl::new(control.numeric.clone(), control.label, "");
                input.set_widget_name(&format!("tool-setting-{}", control.id));
                let id = control.id;
                input.connect_value_changed(glib::clone!(
                    #[weak]
                    workspace,
                    move |input| {
                        workspace.dispatch(UiAction::SetToolSetting {
                            id: id.into(),
                            value: input.value() as f32,
                        });
                    }
                ));
                self.root.append(&input);
                fields.push((control.clone(), input));
            }
            for action in &state.tool_actions {
                let command = state
                    .commands
                    .iter()
                    .find(|c| c.id == action.command)
                    .expect("core command exists");
                let widget: gtk::Widget = if action.checkable {
                    let check = gtk::CheckButton::new();
                    let label = gtk::Label::new(Some(command.label));
                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    label.set_xalign(0.);
                    check.set_child(Some(&label));
                    let updating = self.updating.clone();
                    let id = action.command;
                    check.connect_toggled(glib::clone!(
                        #[weak]
                        workspace,
                        move |_| {
                            if !updating.get() {
                                workspace.dispatch(UiAction::Invoke { command: id });
                            }
                        }
                    ));
                    check.upcast()
                } else {
                    let button = workspace.action_button(
                        command.label,
                        UiAction::Invoke {
                            command: action.command,
                        },
                    );
                    if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    }
                    button.upcast()
                };
                widget.set_widget_name(&format!("tool-action-{:?}", action.command));
                self.root.append(&widget);
                actions.push((*action, widget));
            }
        }
        for ((_, input), control) in fields.iter().zip(controls) {
            input.set_value(control.value as f64);
        }
        for (action, widget) in actions.iter() {
            if let Some(command) = state.commands.iter().find(|c| c.id == action.command) {
                widget.set_sensitive(command.enabled);
                widget.set_tooltip_text(Some(&command.tooltip));
                if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
                    check.set_active(command.selected);
                }
            }
        }
        self.updating.set(false);
    }
}

pub fn size_grid(workspace: &Rc<Workspace>) -> (gtk::FlowBox, Vec<(f32, gtk::Button)>) {
    let mut buttons = Vec::new();
    let grid = gtk::FlowBox::builder()
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(4)
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(2)
        .row_spacing(4)
        .build();
    for &value in layer_ui::BRUSH_SIZES {
        let button = workspace.action_button("", UiAction::SetBrushSize { value });
        button.add_css_class("flat");
        button.add_css_class("size-preset");
        button.set_tooltip_text(Some(&format!("{value} px")));
        let labels = gtk::Box::new(gtk::Orientation::Vertical, 4);
        // A fixed-height native UI glyph, not a canvas/brush raster path.
        // Font-size-dependent glyph ascent otherwise inflates every row.
        let dot = gtk::DrawingArea::builder().height_request(28).build();
        dot.set_draw_func(move |area, cr, width, height| {
            let color = area.color();
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                color.alpha() as f64,
            );
            cr.arc(
                width as f64 * 0.5,
                height as f64 * 0.5,
                (2.0 + value.sqrt() * 1.2).min(27.0) as f64 * 0.5,
                0.0,
                std::f64::consts::TAU,
            );
            let _ = cr.fill();
        });
        labels.append(&dot);
        let label = gtk::Label::new(Some(&value.to_string()));
        label.add_css_class("caption");
        labels.append(&label);
        button.set_child(Some(&labels));
        grid.insert(&button, -1);
        buttons.push((value, button));
    }
    (grid, buttons)
}

pub struct SizePanel {
    pub root: gtk::Box,
    number: NumberControl,
    buttons: Vec<(f32, gtk::Button)>,
}
impl SizePanel {
    pub fn new(workspace: &Rc<Workspace>) -> Self {
        let root = body();
        root.set_spacing(12);
        let number = NumberControl::new(layer_ui::NumericControl::brush_size(), "Brush size", "");
        number.connect_value_changed(glib::clone!(
            #[weak]
            workspace,
            move |i| {
                workspace.dispatch(UiAction::SetBrushSize {
                    value: i.value() as f32,
                });
            }
        ));
        let (grid, buttons) = size_grid(workspace);
        root.append(&number);
        root.append(&grid);
        Self {
            root,
            number,
            buttons,
        }
    }
    pub fn refresh(&self, brush: &layer_ui::BrushState) {
        self.number.set_value(brush.diameter as f64);
        for (value, button) in &self.buttons {
            selected(button, *value == brush.diameter);
        }
    }
}

fn body() -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let inset = layer_ui::PANEL_CONTENT_INSET as i32;
    root.set_margin_start(inset);
    root.set_margin_end(inset);
    root.set_margin_top(inset);
    root.set_margin_bottom(inset);
    root
}

mod wheel {
    use super::*;
    #[derive(Default)]
    pub struct Wheel {
        pub color: RefCell<ColorState>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Wheel {
        const NAME: &'static str = "CapyColorWheel";
        type Type = super::ColorWheel;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Wheel {}
    impl WidgetImpl for Wheel {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Vertical {
                let size = if for_size < 0 { 196 } else { for_size };
                // Compress the wheel before scrolling the swatches/component
                // editors out of a short dock. Drawers retain its natural size.
                (64, size.max(64), -1, -1)
            } else {
                (64, 196, -1, -1)
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let (size, [x, y]) = self.obj().drawing_bounds();
            if let Some(geometry) = ColorWheelGeometry::new(size) {
                snapshot.save();
                snapshot.translate(&gtk::graphene::Point::new(x, y));
                let bounds = gtk::graphene::Rect::new(0.0, 0.0, size, size);
                let center = gtk::graphene::Point::new(geometry.center[0], geometry.center[1]);
                let ring = gtk::gsk::PathBuilder::new();
                ring.add_circle(&center, (geometry.outer + geometry.inner) * 0.5);
                snapshot.push_stroke(
                    &ring.to_path(),
                    &gtk::gsk::Stroke::new(geometry.outer - geometry.inner),
                );
                let stops: Vec<_> = (0..=6)
                    .map(|i| {
                        let [r, g, b] = layer_ui::hue_color(i as f32 * 60.0);
                        gtk::gsk::ColorStop::new(i as f32 / 6.0, gtk::gdk::RGBA::new(r, g, b, 1.0))
                    })
                    .collect();
                snapshot.append_conic_gradient(&bounds, &center, -60.0, &stops);
                snapshot.pop();
                let cr = snapshot.append_cairo(&gtk::graphene::Rect::new(0.0, 0.0, size, size));
                draw_wheel(&cr, &self.color.borrow(), &geometry);
                snapshot.restore();
            }
        }
    }
}
glib::wrapper! {
    pub struct ColorWheel(ObjectSubclass<wheel::Wheel>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl ColorWheel {
    fn drawing_bounds(&self) -> (f32, [f32; 2]) {
        let size = self.width().min(self.height()) as f32;
        (
            size,
            [
                (self.width() as f32 - size) * 0.5,
                (self.height() as f32 - size) * 0.5,
            ],
        )
    }
}

pub struct ColorPanel {
    pub root: gtk::Box,
    initialized: Cell<bool>,
    wheel: ColorWheel,
    swatches: Vec<(ColorSlot, gtk::Button, gtk::DrawingArea)>,
    components: [NumberControl; 3],
    labels: [gtk::Label; 3],
    mode: gtk::DrawingArea,
    mode_button: gtk::Button,
    swap: gtk::Button,
}
impl ColorPanel {
    pub fn new() -> Self {
        let root = body();
        let wheel: ColorWheel = glib::Object::new();
        wheel.set_hexpand(true);
        wheel.set_vexpand(true);
        wheel.set_widget_name("color-wheel");
        root.append(&wheel);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        actions.set_homogeneous(true);
        let mut swatches = Vec::new();
        for (slot, label) in [
            (ColorSlot::Foreground, "Foreground color"),
            (ColorSlot::Background, "Background color"),
            (ColorSlot::Transparent, "Transparent paint"),
        ] {
            let button = gtk::Button::new();
            button.add_css_class("flat");
            button.add_css_class("color-swatch");
            button.set_tooltip_text(Some(label));
            button.set_widget_name(&format!("color-{slot:?}"));
            let sample = gtk::DrawingArea::builder()
                .width_request(14)
                .height_request(20)
                .build();
            sample.set_draw_func(glib::clone!(
                #[weak]
                wheel,
                move |_, cr, width, height| {
                    let state = wheel.imp().color.borrow();
                    let color = match slot {
                        ColorSlot::Foreground => state.foreground,
                        ColorSlot::Background => state.background,
                        ColorSlot::Transparent => [0.0; 4],
                    };
                    for row in 0..(height + 4) / 5 {
                        for col in 0..(width + 4) / 5 {
                            let c = if (row + col) % 2 == 0 { 0.8 } else { 0.55 };
                            cr.set_source_rgb(c, c, c);
                            cr.rectangle((col * 5) as f64, (row * 5) as f64, 5.0, 5.0);
                            let _ = cr.fill();
                        }
                    }
                    cr.set_source_rgba(
                        color[0] as f64,
                        color[1] as f64,
                        color[2] as f64,
                        color[3] as f64,
                    );
                    let _ = cr.paint();
                }
            ));
            button.set_child(Some(&sample));
            actions.append(&button);
            swatches.push((slot, button, sample));
        }
        let swap = gtk::Button::from_icon_name("layer-swap-symbolic");
        swap.add_css_class("flat");
        swap.add_css_class("color-swatch");
        swap.set_tooltip_text(Some("Swap foreground and background"));
        swap.set_widget_name("color-swap");
        actions.append(&swap);
        let mode_button = gtk::Button::new();
        mode_button.add_css_class("flat");
        mode_button.add_css_class("color-swatch");
        mode_button.set_tooltip_text(Some("Switch HSV square / HLS triangle"));
        mode_button.set_widget_name("color-space");
        let mode = gtk::DrawingArea::builder()
            .width_request(14)
            .height_request(20)
            .build();
        mode.set_draw_func(glib::clone!(
            #[weak]
            wheel,
            move |area, cr, w, h| {
                let c = area.color();
                cr.set_source_rgba(
                    c.red() as f64,
                    c.green() as f64,
                    c.blue() as f64,
                    c.alpha() as f64,
                );
                cr.set_line_width(1.2);
                if wheel.imp().color.borrow().space == ColorSpace::Hsv {
                    cr.move_to(2., h as f64 - 4.);
                    cr.line_to(w as f64 * 0.5, 4.);
                    cr.line_to(w as f64 - 2., h as f64 - 4.);
                    cr.close_path();
                } else {
                    cr.rectangle(2., 4., (w - 4) as f64, (h - 8) as f64);
                }
                let _ = cr.stroke();
            }
        ));
        mode_button.set_child(Some(&mode));
        actions.append(&mode_button);
        root.append(&actions);
        let values = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        values.set_homogeneous(true);
        let labels = std::array::from_fn(|_| gtk::Label::new(None));
        let components = std::array::from_fn(|index| {
            let input = NumberControl::value_only(
                ColorState::component_control(index).unwrap(),
                "Color component",
            );
            input.set_widget_name(&format!("color-component-{index}"));
            let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
            labels[index].add_css_class("dim-label");
            column.append(&labels[index]);
            column.append(&input);
            values.append(&column);
            input
        });
        root.append(&values);
        Self {
            root,
            wheel,
            initialized: Cell::new(false),
            swatches,
            components,
            labels,
            mode,
            mode_button,
            swap,
        }
    }
    pub fn bind(&self, workspace: &Rc<Workspace>) {
        for (slot, button, _) in &self.swatches {
            let slot = *slot;
            button.connect_clicked(glib::clone!(
                #[weak]
                workspace,
                move |_| workspace.dispatch(UiAction::Color {
                    action: ColorAction::Select { slot }
                })
            ));
        }
        // These are direct core actions; swatch selection and color-space state
        // never live in this native view.
        self.mode_button.connect_clicked(glib::clone!(
            #[weak]
            workspace,
            move |_| workspace.dispatch(UiAction::Color {
                action: ColorAction::ToggleSpace
            })
        ));
        self.swap.connect_clicked(glib::clone!(
            #[weak]
            workspace,
            move |_| workspace.dispatch(UiAction::Color {
                action: ColorAction::Swap
            })
        ));
        for (index, input) in self.components.iter().enumerate() {
            input.connect_value_changed(glib::clone!(
                #[weak]
                workspace,
                move |input| workspace.dispatch(UiAction::Color {
                    action: ColorAction::Component {
                        index,
                        value: input.value() as f32
                    }
                })
            ));
        }
        let part = Rc::new(Cell::new(None));
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin(glib::clone!(
            #[weak]
            workspace,
            #[weak(rename_to = wheel)]
            self.wheel,
            #[strong]
            part,
            move |gesture, x, y| {
                let (size, [ox, oy]) = wheel.drawing_bounds();
                let point = [x as f32 - ox, y as f32 - oy];
                let hit = ColorWheelGeometry::new(size)
                    .and_then(|g| g.hit(point, wheel.imp().color.borrow().space));
                part.set(hit);
                if let Some(part) = hit {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    workspace.dispatch(UiAction::Color {
                        action: ColorAction::Pick { part, point, size },
                    });
                }
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak]
            workspace,
            #[weak(rename_to = wheel)]
            self.wheel,
            #[strong]
            part,
            move |gesture, dx, dy| {
                if let Some(part) = part.get()
                    && let Some((x, y)) = gesture.start_point()
                {
                    let (size, [ox, oy]) = wheel.drawing_bounds();
                    workspace.dispatch(UiAction::Color {
                        action: ColorAction::Pick {
                            part,
                            point: [(x + dx) as f32 - ox, (y + dy) as f32 - oy],
                            size,
                        },
                    });
                }
            }
        ));
        self.wheel.add_controller(drag);
    }
    pub fn refresh(&self, state: &ColorState) {
        if self.initialized.replace(true) && *self.wheel.imp().color.borrow() == *state {
            return;
        }
        *self.wheel.imp().color.borrow_mut() = state.clone();
        self.wheel.queue_draw();
        self.mode.queue_draw();
        for (slot, button, sample) in &self.swatches {
            if *slot == state.slot {
                button.add_css_class("selected-tool");
            } else {
                button.remove_css_class("selected-tool");
            }
            sample.queue_draw();
        }
        for (index, input) in self.components.iter().enumerate() {
            self.labels[index].set_text(state.labels()[index]);
            input.set_value(state.components()[index] as f64);
        }
    }
}

fn draw_wheel(cr: &cairo::Context, state: &ColorState, g: &ColorWheelGeometry) {
    // This is a tiny UI vector drawing, never a canvas or brush raster path.
    // GTK caches the resulting node until the color or allocation changes.
    let [r, green, b] = layer_ui::hue_color(state.components()[0]).map(f64::from);
    if state.space == ColorSpace::Hsv {
        let [x, y, w] = g.square.map(f64::from);
        let h = w;
        let horizontal = cairo::LinearGradient::new(x, y, x + w, y);
        horizontal.add_color_stop_rgb(0., 1., 1., 1.);
        horizontal.add_color_stop_rgb(1., r, green, b);
        cr.rectangle(x, y, w, h);
        let _ = cr.set_source(&horizontal);
        let _ = cr.fill();
        let vertical = cairo::LinearGradient::new(x, y, x, y + h);
        vertical.add_color_stop_rgba(0., 0., 0., 0., 0.);
        vertical.add_color_stop_rgba(1., 0., 0., 0., 1.);
        cr.rectangle(x, y, w, h);
        let _ = cr.set_source(&vertical);
        let _ = cr.fill();
    } else {
        let mesh = cairo::Mesh::new();
        mesh.begin_patch();
        for (index, p) in g.triangle.iter().chain([&g.triangle[2]]).enumerate() {
            if index == 0 {
                mesh.move_to(p[0] as f64, p[1] as f64);
            } else {
                mesh.line_to(p[0] as f64, p[1] as f64);
            }
        }
        mesh.set_corner_color_rgb(cairo::MeshCorner::MeshCorner0, 1., 1., 1.);
        mesh.set_corner_color_rgb(cairo::MeshCorner::MeshCorner1, 0., 0., 0.);
        mesh.set_corner_color_rgb(cairo::MeshCorner::MeshCorner2, r, green, b);
        mesh.set_corner_color_rgb(cairo::MeshCorner::MeshCorner3, r, green, b);
        mesh.end_patch();
        let _ = cr.set_source(&mesh);
        let _ = cr.paint();
    }
    for point in [g.hue_marker(state.components()[0]), state.marker(g)] {
        cr.new_path();
        cr.arc(
            point[0] as f64,
            point[1] as f64,
            3.5,
            0.,
            std::f64::consts::TAU,
        );
        cr.set_source_rgb(0., 0., 0.);
        cr.set_line_width(3.);
        let _ = cr.stroke_preserve();
        cr.set_source_rgb(1., 1., 1.);
        cr.set_line_width(1.5);
        let _ = cr.stroke();
    }
}
