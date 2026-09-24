//! Native projections of shared tool controls and color-wheel geometry.
use crate::display_color::{ColorPatch, ViewColor};
use crate::{
    number_control::NumberControl,
    workspace::{Workspace, selected},
};
use gtk::{cairo, glib, prelude::*, subclass::prelude::*};
use layer_ui::{
    ColorAction, ColorPanelLayout, ColorReadout, ColorShape, ColorSlot, ColorState,
    ColorWheelGeometry, Theme, ToolSetItem, ToolSetView, ToolSetting, ToolSettingAction, UiAction,
    UiState,
};

/// Placement completion remains visible when the artist's workspace hides the
/// Tool Settings panel. Commands and transaction ownership stay in shared Rust.
pub struct PlacementActions {
    pub root: gtk::Box,
    buttons: [(layer_ui::CommandId, gtk::Button); 3],
}
impl PlacementActions {
    pub fn new() -> Self {
        use layer_ui::CommandId::*;
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        root.set_widget_name("photo-placement-actions");
        root.add_css_class("toolbar");
        root.add_css_class("card");
        root.set_halign(gtk::Align::Center);
        root.set_valign(gtk::Align::End);
        root.set_margin_bottom(40);
        root.set_visible(false);
        let buttons = [
            (PlacementOriginalSize, "Original Size", "placement-original-size"),
            (CancelTransform, "Cancel", "placement-cancel"),
            (ApplyTransform, "Apply", "placement-apply"),
        ].map(|(command, label, name)| {
            let button = gtk::Button::with_label(label);
            button.set_widget_name(name);
            if command == ApplyTransform { button.add_css_class("suggested-action"); }
            root.append(&button);
            (command, button)
        });
        Self { root, buttons }
    }
    pub fn bind(&self, workspace: &Rc<Workspace>) {
        for (command, button) in &self.buttons {
            let action = UiAction::Invoke { command: *command };
            workspace.bind_action_tooltip(button, action.clone());
            button.connect_clicked(glib::clone!(#[weak] workspace, move |_| workspace.dispatch(action.clone())));
        }
    }
    pub fn refresh(&self, state: &UiState) {
        self.root.set_visible(state.tool_actions.iter()
            .any(|a| a.command == layer_ui::CommandId::PlacementOriginalSize));
        for (id, button) in &self.buttons {
            button.set_sensitive(state.commands.iter().any(|c| c.id == *id && c.enabled));
        }
    }
}

const TOOL_ROW_HEIGHT: i32 = 44;

/// A body can be projected in a dock or a tool drawer without reparenting the
/// other view. Preview textures are shared by the existing immutable cache.
pub struct ToolSet {
    pub root: gtk::Box,
    panel: layer_ui::Panel,
    groups: gtk::FlowBox,
    list: gtk::Box,
    pub group_buttons: RefCell<Vec<gtk::Button>>,
    pub buttons: RefCell<Vec<(ToolSetItem, gtk::Button, Option<gtk::Picture>)>>,
    view: RefCell<ToolSetView>,
    theme: Cell<Option<Theme>>,
}
impl ToolSet {
    pub fn new() -> Self {
        Self::for_panel(layer_ui::Panel::Brushes)
    }
    pub fn for_panel(panel: layer_ui::Panel) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let groups = gtk::FlowBox::builder()
            .homogeneous(true)
            .min_children_per_line(1)
            .max_children_per_line(if matches!(panel, layer_ui::Panel::BrushSets | layer_ui::Panel::SculptSets) { 1 } else { 16 })
            .selection_mode(gtk::SelectionMode::None)
            .column_spacing(2)
            .row_spacing(2)
            .build();
        groups.add_css_class("tool-groups");
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        root.append(&groups);
        root.append(&list);
        groups.set_visible(panel != layer_ui::Panel::Tools);
        list.set_visible(!matches!(panel, layer_ui::Panel::BrushSets | layer_ui::Panel::SculptSets));
        Self {
            root,
            panel,
            groups,
            list,
            group_buttons: RefCell::default(),
            buttons: RefCell::default(),
            view: RefCell::default(),
            theme: Cell::new(None),
        }
    }
    pub fn refresh_state(&self, workspace: &Rc<Workspace>, state: &UiState) {
        self.refresh(workspace, state.tool_panel(self.panel), state.theme);
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
                let media = matches!(self.panel, layer_ui::Panel::BrushSets | layer_ui::Panel::SculptSets);
                if media { button.set_widget_name(&format!("{}-{}", if self.panel == layer_ui::Panel::SculptSets { "sculpt-set" } else { "brush-set" }, item.icon)); }
                button.set_halign(if media { gtk::Align::Fill } else { gtk::Align::Start });
                button.set_hexpand(media);
                button.set_size_request(
                    ((if media { layer_ui::BRUSH_SETS_MIN_WIDTH } else { layer_ui::TOOL_PANEL_MIN_WIDTH }) - 2.0 * layer_ui::PANEL_CONTENT_INSET) as i32,
                    if media { TOOL_ROW_HEIGHT } else { layer_ui::TILE_SIZE as i32 },
                );
                button.set_child(Some(&aligned_icon_label(item.label, item.icon, if media { 0.0 } else { 1.0 })));
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
                    content.append(&preview);
                    content.append(&tool_label(item));
                    button.set_child(Some(&content));
                    preview
                });
                if preview.is_none() {
                    button.set_size_request(-1, TOOL_ROW_HEIGHT);
                    if let UiAction::Invoke { command } = item.action {
                        button.set_widget_name(&format!("tool-choice-{command:?}"));
                    }
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
    aligned_icon_label(item.label, item.icon, 1.0)
}

pub fn icon_label(text: &str, icon: &str) -> gtk::Box {
    aligned_icon_label(text, icon, 0.0)
}

pub(crate) fn aligned_icon_label(text: &str, icon: &str, xalign: f32) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.set_valign(gtk::Align::Center);
    row.append(&crate::icons::image(&format!("layer-{}-symbolic", icon)));
    let label = gtk::Label::new(Some(text));
    label.set_hexpand(true);
    label.set_xalign(xalign);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(1);
    label.set_tooltip_text(Some(text));
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
            let mode_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            mode_row.set_homogeneous(true);
            mode_row.set_widget_name("selection-mode-row");
            mode_row.add_css_class("linked");
            mode_row.add_css_class("selection-modes");
            mode_row.update_property(&[gtk::accessible::Property::Label("Selection mode")]);
            if state.layer_tools.tool.selection_tool().is_some() {
                self.root.append(&mode_row);
            }
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
            let mut source_group: Option<gtk::CheckButton> = None;
            let mut mode_group: Option<gtk::ToggleButton> = None;
            for action in &state.tool_actions {
                let command = state
                    .commands
                    .iter()
                    .find(|c| c.id == action.command)
                    .expect("core command exists");
                let mode = matches!(action.command, layer_ui::CommandId::SelectionNew | layer_ui::CommandId::SelectionAdd | layer_ui::CommandId::SelectionSubtract | layer_ui::CommandId::SelectionIntersect);
                let widget: gtk::Widget = if mode {
                    let button = gtk::ToggleButton::new();
                    button.set_size_request(28, layer_ui::TILE_SIZE as i32);
                    let image = crate::icons::image(&format!("layer-{}-symbolic", command.icon.unwrap()));
                    image.set_pixel_size(20);
                    if state.layer_tools.tool.selection_tool() == Some(layer_ui::SelectionTool::Brush) {
                        let label = if action.command == layer_ui::CommandId::SelectionAdd { "Add" } else { "Subtract" };
                        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                        row.set_halign(gtk::Align::Center);
                        row.append(&image);
                        row.append(&gtk::Label::new(Some(label)));
                        button.set_child(Some(&row));
                        button.set_size_request(44,44);
                    } else { button.set_child(Some(&image)); }
                    button.update_property(&[gtk::accessible::Property::Label(command.label)]);
                    if let Some(first) = &mode_group { button.set_group(Some(first)); }
                    else { mode_group = Some(button.clone()); }
                    let updating = self.updating.clone();
                    let id = action.command;
                    button.connect_toggled(glib::clone!(
                        #[weak]
                        workspace,
                        move |button| {
                            if !updating.get() && button.is_active() {
                                workspace.dispatch(UiAction::Invoke { command: id });
                            }
                        }
                    ));
                    button.upcast()
                } else if action.checkable {
                    let check = gtk::CheckButton::new();
                    if action.command == layer_ui::CommandId::SelectionBrushPressure { check.set_size_request(-1,44); }
                    let source_choice = matches!(action.command, layer_ui::CommandId::SelectionVisible | layer_ui::CommandId::SelectionEditing | layer_ui::CommandId::SelectionReference);
                    if source_choice {
                        if let Some(first) = &source_group { check.set_group(Some(first)); }
                        else { source_group = Some(check.clone()); }
                    }
                    let label = gtk::Label::new(Some(command.label));
                    check.set_tooltip_text(Some(command.label));
                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    label.set_xalign(0.);
                    check.set_child(Some(&label));
                    let updating = self.updating.clone();
                    let id = action.command;
                    check.connect_toggled(glib::clone!(
                        #[weak]
                        workspace,
                        move |check| {
                            if !updating.get() && (!source_choice || check.is_active()) {
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
                    button.set_child(Some(&icon_label(command.label, command.icon.unwrap())));
                    button.upcast()
                };
                widget.set_widget_name(&format!("tool-action-{:?}", action.command));
                if mode { mode_row.append(&widget); }
                else { self.root.append(&widget); }
                actions.push((*action, widget));
            }
            if state.layer_tools.tool.selection_tool().is_some() {
                let menu = crate::selection_masks::menu_button(workspace, "Selection Actions…", crate::selection_masks::Menu::Selection);
                menu.set_widget_name("selection-actions-menu");
                self.root.append(&menu);
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
                if let Some(button) = widget.downcast_ref::<gtk::ToggleButton>() {
                    button.set_active(command.selected);
                    selected(button, command.selected);
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

// Shared allocations reserve the curved readout band and the optional HDR footer.
// Four 36px tiles (144px panel, 128px content) is the smallest supported width.
mod wheel_button {
    use super::*;
    #[derive(Default)]
    pub struct WheelButton {
        pub readout: Cell<bool>,
        pub rotation: Cell<f32>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for WheelButton {
        const NAME: &'static str = "CapyColorWheelButton";
        type Type = super::WheelButton;
        type ParentType = gtk::Button;
    }
    impl ObjectImpl for WheelButton {}
    impl WidgetImpl for WheelButton {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let o = self.obj();
            let center = gtk::graphene::Point::new(o.width() as f32 * 0.5, o.height() as f32 * 0.5);
            snapshot.save();
            snapshot.translate(&center);
            snapshot.rotate(self.rotation.get());
            snapshot.translate(&gtk::graphene::Point::new(-center.x(), -center.y()));
            self.parent_snapshot(snapshot);
            snapshot.restore();
        }
        fn contains(&self, x: f64, y: f64) -> bool {
            let o = self.obj();
            let (w, h) = (o.width() as f64, o.height() as f64);
            if x < 0. || y < 0. || x > w || y > h {
                return false;
            }
            if self.readout.get() {
                let r = ColorPanelLayout::new((w * 2.) as f32)
                    .map(|layout| layout.wheel[2] as f64 * 0.49 + 2.)
                    .unwrap_or(1.);
                (x - w).hypot(y - h) >= r
            } else {
                (x - w * 0.5).hypot(y - h * 0.5) <= w.min(h) * 0.5
            }
        }
    }
    impl ButtonImpl for WheelButton {}
}
glib::wrapper! {
    pub struct WheelButton(ObjectSubclass<wheel_button::WheelButton>) @extends gtk::Button, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}
mod wheel {
    use super::*;
    #[derive(Default)]
    pub struct Wheel {
        pub color: RefCell<ColorState>,
        pub intensity: RefCell<Option<crate::hdr_color_scale::HdrColorScale>>,
        pub hdr: Cell<bool>,
        // Background precedes foreground so their deliberate overlap also picks correctly.
        pub corners: RefCell<Vec<WheelButton>>,
        pub menu: RefCell<Option<gtk::Popover>>,
        pub menu_slot: Cell<ColorSlot>,
        pub view: Cell<ViewColor>,
        pub headroom: Cell<f32>,
        pub linear_field: RefCell<Option<(u32, f32, ColorShape, layer_core::color::RgbSpace, Vec<[f32; 4]>)>>,
        pub disc: RefCell<Option<(u32, f32, ColorShape, layer_core::color::RgbSpace, ViewColor, f32, f32, gtk::gdk::Texture)>>,
        pub ring: RefCell<Option<(u32, ColorShape, layer_core::color::RgbSpace, ViewColor, gtk::gdk::Texture)>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Wheel {
        const NAME: &'static str = "CapyColorWheel";
        type Type = super::ColorWheel;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Wheel {
        fn dispose(&self) {
            if let Some(intensity) = self.intensity.take() { intensity.unparent(); }
            if let Some(menu) = self.menu.take() {
                menu.unparent();
            }
            for button in self.corners.borrow_mut().drain(..) {
                button.unparent();
            }
        }
    }
    impl WidgetImpl for Wheel {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Vertical {
                let height = |size: i32| if self.hdr.get() {
                    ColorPanelLayout::with_hdr(size as f32).unwrap().height().ceil() as i32
                } else { ColorPanelLayout::new(size as f32).unwrap().height().ceil() as i32 };
                (height(128), height(if for_size < 0 { 226 } else { for_size.max(128) }), -1, -1)
            } else {
                (128, 226, -1, -1)
            }
        }
        fn size_allocate(&self, _width: i32, _height: i32, baseline: i32) {
            let (size, [x, y]) = self.obj().stage_bounds();
            let Some(layout) = (if self.hdr.get() { ColorPanelLayout::with_hdr(size) } else { ColorPanelLayout::new(size) }) else {
                return;
            };
            let boxes = [
                layout.white,
                layout.black,
                layout.background,
                layout.foreground,
                layout.transparent,
                layout.shapes[0],
                layout.shapes[1],
                layout.swap,
                layout.readout,
                layout.edit,
            ];
            for (button, rotation) in self
                .corners
                .borrow()
                .iter()
                .skip(5)
                .zip(layout.shape_rotations)
            {
                button.imp().rotation.set(rotation);
            }
            for (button, [bx, by, w, h]) in self.corners.borrow().iter().zip(boxes) {
                button.allocate(
                    w.round() as i32,
                    h.round() as i32,
                    baseline,
                    Some(
                        gtk::gsk::Transform::new()
                            .translate(&gtk::graphene::Point::new(x + bx.round(), y + by.round())),
                    ),
                );
            }
            if let Some(intensity) = self.intensity.borrow().as_ref().filter(|i| i.is_visible()) {
                intensity.allocate(size.round() as i32, layout.height().ceil() as i32, baseline,
                    Some(gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(x, y))));
            }
            if let Some(menu) = self.menu.borrow().as_ref() {
                menu.present();
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let (size, [x, y]) = self.obj().drawing_bounds();
            if let Some(geometry) = ColorWheelGeometry::new(size) {
                snapshot.save();
                snapshot.translate(&gtk::graphene::Point::new(x, y));
                let bounds = gtk::graphene::Rect::new(0., 0., size, size);
                let center = gtk::graphene::Point::new(geometry.center[0], geometry.center[1]);
                let ring = gtk::gsk::PathBuilder::new();
                ring.add_circle(&center, (geometry.outer + geometry.inner) * 0.5);
                snapshot.push_stroke(
                    &ring.to_path(),
                    &gtk::gsk::Stroke::new(geometry.outer - geometry.inner),
                );
                let state = self.color.borrow();
                let view = self.view.get();
                // Retain the managed guide at display DPI; ordinary color
                // changes and panel motion only sample the cached texture.
                let side = (size * self.obj().scale_factor() as f32).ceil() as u32;
                let shape = state.wheel_shape();
                let space = state.rgb_space();
                {
                    let mut cache = self.ring.borrow_mut();
                    if cache.as_ref().is_none_or(|(s, p, c, v, _)| *s != side || *p != shape || *c != space || *v != view) {
                        // Quantizing P3 to eight bits before GTK converts back
                        // to sRGB amplifies dark-channel error at gamut edges.
                        // Keep this small display derivative in half precision;
                        // the cached texture and document precision are separate.
                        let mut pixels = Vec::with_capacity(side as usize * side as usize * 8);
                        let logical_pixel = size / side as f32;
                        for i in 0..side * side {
                            let point = [((i % side) as f32 + 0.5) * logical_pixel,
                                         ((i / side) as f32 + 0.5) * logical_pixel];
                            let hue = state.wheel_hue_at(&geometry, point);
                            let rgb = state.wheel_hue_color_in(hue, view.space());
                            for value in rgb.into_iter().chain([1.]) {
                                pixels.extend_from_slice(&layer_core::color::f16::from_f32(value).to_bits().to_ne_bytes());
                            }
                        }
                        let texture = view.texture([side, side], gtk::gdk::MemoryFormat::R16g16b16a16Float,
                            side as usize * 8, pixels);
                        *cache = Some((side, shape, space, view, texture));
                    }
                    snapshot.append_texture(&cache.as_ref().unwrap().4, &bounds);
                }
                snapshot.pop();
                let hue = state.wheel_components()[0];
                {
                    let mut cache = self.disc.borrow_mut();
                    let headroom = self.headroom.get();
                    let intensity = state.hdr_intensity();
                    if cache.as_ref().is_none_or(|(s, h, p, c, v, e, d, _)| *s != side || *h != hue || *p != shape || *c != space || *v != view || *e != intensity || *d != headroom) {
                        let texture = if matches!(view, ViewColor::Mapped { .. }) {
                            let mut linear = self.linear_field.borrow_mut();
                            if linear.as_ref().is_none_or(|(s, h, p, c, _)| *s != side || *h != hue || *p != shape || *c != space) {
                                let mut pixels = vec![[0.;4]; side as usize * side as usize];
                                state.render_field_base_linear(side, &mut pixels);
                                *linear = Some((side, hue, shape, space, pixels));
                            }
                            crate::display_color::picker_texture_with_gain(view, headroom, space, [side,side], &linear.as_ref().unwrap().4, f64::from(intensity).exp2())
                        } else {
                            let mut pixels = vec![0; side as usize * side as usize * 4];
                            state.render_field_in(side, view.space(), &mut pixels);
                            view.rgba8([side,side], pixels)
                        };
                        *cache = Some((side, hue, shape, space, view, intensity, headroom, texture));
                    }
                    snapshot.push_fill(&color_field_path(shape, &geometry), gtk::gsk::FillRule::Winding);
                    snapshot.append_texture(&cache.as_ref().unwrap().7, &bounds);
                    snapshot.pop();
                }
                draw_wheel(snapshot, &state, &geometry, view, self.headroom.get());
                snapshot.restore();
            }
            if let Some(intensity) = self.intensity.borrow().as_ref().filter(|i| i.is_visible()) {
                self.obj().snapshot_child(intensity, snapshot);
            }
            for button in self.corners.borrow().iter() {
                self.obj().snapshot_child(button, snapshot);
            }
            if let Some(menu) = self.menu.borrow().as_ref() {
                self.obj().snapshot_child(menu, snapshot);
            }
        }
    }
}
glib::wrapper! {
    pub struct ColorWheel(ObjectSubclass<wheel::Wheel>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl ColorWheel {
    fn stage_bounds(&self) -> (f32, [f32; 2]) {
        let mut size = self.width().min(self.height()).max(0);
        let mut footer = 0.;
        {
            let layout_for = |side| if self.imp().hdr.get() { ColorPanelLayout::with_hdr(side) } else { ColorPanelLayout::new(side) };
            // Fit the shared, width-dependent footer when a short panel constrains height.
            let (mut low, mut high) = (128, size);
            size = 0;
            while low <= high {
                let candidate = low + (high - low) / 2;
                let height = layout_for(candidate as f32).unwrap().height().ceil() as i32;
                if height <= self.height() { size = candidate; low = candidate + 1; }
                else { high = candidate - 1; }
            }
            if let Some(layout) = layout_for(size as f32) {
                footer = layout.height().ceil() - size as f32;
            }
        }
        let size = size as f32;
        (
            size,
            [
                (self.width() as f32 - size) * 0.5,
                (self.height() as f32 - size - footer) * 0.5,
            ],
        )
    }
    pub(crate) fn drawing_bounds(&self) -> (f32, [f32; 2]) {
        let (size, [x, y]) = self.stage_bounds();
        let Some(layout) = ColorPanelLayout::new(size) else {
            return (0., [x, y]);
        };
        (layout.wheel[2], [x + layout.wheel[0], y + layout.wheel[1]])
    }
}
pub struct ColorPanel {
    pub root: gtk::Box,
    initialized: Cell<bool>,
    wheel: ColorWheel,
    swatches: Vec<(ColorSlot, WheelButton, ColorPatch)>,
    quick_colors: Vec<(bool, WheelButton, ColorPatch)>,
    shape_buttons: [WheelButton; 2],
    readout: WheelButton,
    readout_drawing: gtk::DrawingArea,
    swap: WheelButton,
    menu_swap: gtk::Button,
    menu_edit: gtk::Button,
    menu_library: gtk::Button,
    intensity: crate::hdr_color_scale::HdrColorScale,
    edit_color: WheelButton,
}
impl ColorPanel {
    pub fn new() -> Self {
        let root = body();
        root.add_css_class("color-panel");
        let wheel: ColorWheel = glib::Object::new();
        wheel.set_hexpand(true);
        wheel.set_valign(gtk::Align::Fill);
        wheel.set_widget_name("color-wheel");
        root.append(&wheel);
        let intensity = crate::hdr_color_scale::HdrColorScale::new();
        intensity.set_parent(&wheel);
        *wheel.imp().intensity.borrow_mut() = Some(intensity.clone());
        let edit_color: WheelButton = glib::Object::new();
        edit_color.set_icon_name("document-edit-symbolic");
        edit_color.set_tooltip_text(Some("Edit Color…"));
        edit_color.update_property(&[gtk::accessible::Property::Label("Edit Color")]);
        edit_color.add_css_class("flat");
        edit_color.add_css_class("color-utility");
        edit_color.add_css_class("color-swap");
        edit_color.set_widget_name("color-edit-button");
        let mut quick_colors = Vec::new();
        for preset in ColorState::default().quick_colors().into_iter().rev() {
            let button: WheelButton = glib::Object::new();
            button.add_css_class("flat");
            button.add_css_class("color-swatch");
            button.set_tooltip_text(Some(preset.label));
            button.update_property(&[gtk::accessible::Property::Label(preset.label)]);
            button.set_widget_name(if preset.white { "color-White" } else { "color-Black" });
            let sample = ColorPatch::new(true);
            sample.set_size_request(12, 12);
            button.set_child(Some(&sample));
            button.set_parent(&wheel);
            wheel.imp().corners.borrow_mut().push(button.clone());
            quick_colors.push((preset.white, button, sample));
        }
        let mut swatches = Vec::new();
        for (slot, label) in [
            (ColorSlot::Background, "Background color"),
            (ColorSlot::Foreground, "Foreground color"),
            (ColorSlot::Transparent, "Transparent paint"),
        ] {
            let button: WheelButton = glib::Object::new();
            button.add_css_class("flat");
            button.add_css_class("color-swatch");
            button.set_tooltip_text(Some(if slot == ColorSlot::Transparent {
                label
            } else if slot == ColorSlot::Foreground {
                "Foreground color · Double-click to edit"
            } else {
                "Background color · Double-click to edit"
            }));
            button.update_property(&[gtk::accessible::Property::Label(label)]);
            button.set_widget_name(&format!("color-{slot:?}"));
            let sample = ColorPatch::new(true);
            sample.set_size_request(16, 16);
            button.set_child(Some(&sample));
            button.set_parent(&wheel);
            wheel.imp().corners.borrow_mut().push(button.clone());
            swatches.push((slot, button, sample));
        }
        let shape_buttons = std::array::from_fn(|i| {
            let button: WheelButton = glib::Object::new();
            button.add_css_class("flat");
            button.add_css_class("color-shape");
            button.set_widget_name(&format!("color-shape-{i}"));
            button.set_parent(&wheel);
            wheel.imp().corners.borrow_mut().push(button.clone());
            button
        });
        let swap: WheelButton = glib::Object::new();
        crate::icons::set_button(&swap, "layer-color-swap-symbolic");
        swap.add_css_class("flat");
        swap.add_css_class("color-utility");
        swap.add_css_class("color-swap");
        swap.set_widget_name("color-swap");
        swap.set_tooltip_text(Some("Swap foreground and background"));
        swap.update_property(&[gtk::accessible::Property::Label(
            "Swap foreground and background",
        )]);
        swap.set_parent(&wheel);
        wheel.imp().corners.borrow_mut().push(swap.clone());
        let readout: WheelButton = glib::Object::new();
        readout.imp().readout.set(true);
        readout.add_css_class("flat");
        readout.add_css_class("color-readout");
        readout.set_widget_name("color-readout");
        let readout_drawing = gtk::DrawingArea::new();
        readout_drawing.set_can_target(false);
        readout_drawing.set_draw_func(glib::clone!(
            #[weak]
            wheel,
            move |area, cr, width, _| {
                draw_readout(area, cr, width as f64, &wheel.imp().color.borrow(), wheel.imp().view.get());
            }
        ));
        readout.connect_state_flags_changed(glib::clone!(
            #[weak]
            readout_drawing,
            move |_, _| readout_drawing.queue_draw()
        ));
        readout.set_child(Some(&readout_drawing));
        readout.set_parent(&wheel);
        wheel.imp().corners.borrow_mut().push(readout.clone());
        edit_color.set_parent(&wheel);
        wheel.imp().corners.borrow_mut().push(edit_color.clone());
        let menu = gtk::Popover::new();
        menu.set_parent(&wheel);

        let menu_swap = gtk::Button::new();
        menu_swap.add_css_class("flat");
        menu_swap.set_widget_name("color-swap-menu");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&crate::icons::image("layer-color-swap-symbolic"));
        row.append(&gtk::Label::new(Some("Swap foreground and background")));
        menu_swap.set_child(Some(&row));
        menu_swap.update_property(&[gtk::accessible::Property::Label(
            "Swap foreground and background",
        )]);
        let menu_edit = gtk::Button::with_label("Edit Color…");
        menu_edit.add_css_class("flat");
        menu_edit.set_widget_name("color-edit-menu");
        let actions = gtk::Box::new(gtk::Orientation::Vertical, 4);
        actions.append(&menu_edit);
        let menu_library = gtk::Button::with_label("Color Swatches…");
        menu_library.add_css_class("flat");
        menu_library.set_widget_name("color-library-menu");
        actions.append(&menu_library);
        actions.append(&menu_swap);
        menu.set_child(Some(&actions));
        *wheel.imp().menu.borrow_mut() = Some(menu);
        Self {
            root,
            wheel,
            initialized: Cell::new(false),
            swatches,
            quick_colors,
            shape_buttons,
            readout,
            readout_drawing,
            swap,
            menu_swap,
            menu_edit,
            menu_library,
            intensity,
            edit_color,
        }
    }
    pub fn bind(&self, workspace: &Rc<Workspace>) {
        self.intensity.connect_value_changed(glib::clone!(#[weak] workspace, move |i| {
            if i.updating() { return; }
            workspace.dispatch(UiAction::Color { action: ColorAction::HdrIntensity { stops: i.value() as f32 } });
            // Rejected out-of-storage-range edits leave the native control at
            // the accepted value, including in retained color-panel drawers.
            let colors = workspace.gpu.borrow().as_ref().map(|g| g.session.state().display_colors().clone());
            if let Some(colors) = colors { i.refresh(&colors, workspace.view_color(), workspace.picker_headroom()); }
        }));
        self.edit_color.connect_clicked(glib::clone!(#[weak] workspace, move |_| {
            let slot = workspace.gpu.borrow().as_ref().map(|g| g.session.state().display_colors().slot);
            if let Some(slot) = slot { crate::color_editor::show(&workspace, slot); }
        }));
        workspace.watch_popover(self.wheel.imp().menu.borrow().as_ref().unwrap());
        for (white, button, _) in &self.quick_colors {
            let white = *white;
            button.connect_clicked(glib::clone!(#[weak] workspace, move |_| {
                workspace.dispatch(UiAction::Color { action: ColorAction::QuickColor { white } });
            }));
        }
        for (slot, button, _) in &self.swatches {
            let slot = *slot;
            button.connect_clicked(glib::clone!(
                #[weak]
                workspace,
                move |_| workspace.dispatch(UiAction::Color {
                    action: ColorAction::Select { slot }
                })
            ));
            if slot == ColorSlot::Transparent {
                continue;
            }
            let edit = gtk::GestureClick::new();
            edit.set_button(1);
            edit.set_propagation_phase(gtk::PropagationPhase::Capture);
            edit.connect_pressed(glib::clone!(#[weak] workspace, move |g, count, _, _| {
                if count == 2 {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    crate::color_editor::show(&workspace, slot);
                }
            }));
            button.add_controller(edit);
            // This target owns its menu; the enclosing panel's customization
            // capture controller must leave its secondary clicks and holds alone.
            button.add_css_class("customizable-target");
            let popup = glib::clone!(
                #[weak(rename_to = wheel)]
                self.wheel,
                #[weak]
                button,
                move || {
                    wheel.imp().menu_slot.set(slot);
                    let b = button.compute_bounds(&wheel).unwrap();
                    let menu = wheel.imp().menu.borrow();
                    let menu = menu.as_ref().unwrap();
                    menu.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
                        b.x() as i32,
                        b.y() as i32,
                        b.width() as i32,
                        b.height() as i32,
                    )));
                    menu.popup();
                }
            );
            let right = gtk::GestureClick::new();
            right.set_button(3);
            right.set_propagation_phase(gtk::PropagationPhase::Capture);
            right.connect_pressed({
                let popup = popup.clone();
                move |g, _, _, _| {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    popup();
                }
            });
            button.add_controller(right);
            let hold = gtk::GestureLongPress::new();
            hold.set_touch_only(false);
            hold.set_propagation_phase(gtk::PropagationPhase::Capture);
            hold.connect_pressed({
                let popup = popup.clone();
                move |g, _, _| {
                    if !crate::input::touch_or_pen(g) {
                        return;
                    }
                    g.set_state(gtk::EventSequenceState::Claimed);
                    popup();
                }
            });
            button.add_controller(hold);
            let key = gtk::EventControllerKey::new();
            key.connect_key_pressed(move |_, key, _, modifiers| {
                if key == gtk::gdk::Key::Menu
                    || (key == gtk::gdk::Key::F10
                        && modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK))
                {
                    popup();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            button.add_controller(key);
        }
        for (i, button) in self.shape_buttons.iter().enumerate() {
            button.connect_clicked(glib::clone!(
                #[weak]
                workspace,
                #[weak(rename_to = wheel)]
                self.wheel,
                move |_| {
                    let shape = wheel.imp().color.borrow().other_shapes()[i];
                    workspace.dispatch(UiAction::Color {
                        action: ColorAction::Shape { shape },
                    });
                }
            ));
        }
        self.readout.connect_clicked(glib::clone!(
            #[weak]
            workspace,
            move |_| workspace.dispatch(UiAction::Color {
                action: ColorAction::ToggleReadout
            })
        ));
        let swap = glib::clone!(
            #[weak]
            workspace,
            #[weak(rename_to = wheel)]
            self.wheel,
            move || {
                wheel.imp().menu.borrow().as_ref().unwrap().popdown();
                workspace.dispatch(UiAction::Color {
                    action: ColorAction::Swap,
                });
            }
        );
        self.swap.connect_clicked({
            let swap = swap.clone();
            move |_| swap()
        });
        self.menu_swap.connect_clicked(move |_| swap());
        self.menu_edit.connect_clicked(glib::clone!(
            #[weak] workspace,
            #[weak(rename_to = wheel)] self.wheel,
            move |_| {
                let slot = wheel.imp().menu_slot.get();
                wheel.imp().menu.borrow().as_ref().unwrap().popdown();
                crate::color_editor::show(&workspace, slot);
            }
        ));
        self.menu_library.connect_clicked(glib::clone!(
            #[weak] workspace,
            #[weak(rename_to = wheel)] self.wheel,
            move |_| {
                let slot = wheel.imp().menu_slot.get();
                wheel.imp().menu.borrow().as_ref().unwrap().popdown();
                crate::color_library::show(&workspace, slot);
            }
        ));
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
                part.set(None);
                // Popovers are separate native surfaces but still descendants
                // in GTK's widget tree. Never pick through their buttons.
                let native_surface = wheel.native().and_then(|n| n.surface());
                if gesture
                    .current_event()
                    .and_then(|e| e.surface())
                    .is_some_and(|s| Some(s) != native_surface)
                    || wheel
                        .pick(x, y, gtk::PickFlags::DEFAULT)
                        .is_some_and(|w| w != wheel.clone().upcast::<gtk::Widget>())
                {
                    return;
                }
                let (size, [ox, oy]) = wheel.drawing_bounds();
                let point = [x as f32 - ox, y as f32 - oy];
                let hit = ColorWheelGeometry::new(size)
                    .and_then(|g| g.hit_shape(point, wheel.imp().color.borrow().wheel_shape()));
                part.set(hit);
                if let Some(part) = hit {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    workspace.dispatch(UiAction::Color {
                        action: ColorAction::PickWheel { part, point, size },
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
                        action: ColorAction::PickWheel {
                            part,
                            point: [(x + dx) as f32 - ox, (y + dy) as f32 - oy],
                            size,
                        },
                    });
                }
            }
        ));
        drag.connect_drag_end({
            let part = part.clone();
            move |_, _, _| part.set(None)
        });
        drag.connect_cancel(move |_, _| part.set(None));
        self.wheel.add_controller(drag);
    }
    pub fn headroom(&self) -> f32 { self.wheel.imp().headroom.get() }
    pub fn refresh(&self, state: &ColorState, view: ViewColor, headroom: f32) {
        let hdr = matches!(view, ViewColor::Mapped { .. });
        self.intensity.set_visible(hdr);
        if self.wheel.imp().hdr.replace(hdr) != hdr { self.wheel.queue_resize(); }
        if !hdr { self.wheel.imp().linear_field.borrow_mut().take(); }
        self.edit_color.set_sensitive(state.slot != ColorSlot::Transparent);
        let ev = state.definition().brightness_ev(state.rgb_space()).ok().flatten();
        self.intensity.set_sensitive(state.slot != ColorSlot::Transparent);
        if hdr { self.intensity.refresh(state, view, headroom); }
        let previous_headroom = self.wheel.imp().headroom.replace(headroom);
        let previous_view = self.wheel.imp().view.replace(view);
        if self.initialized.replace(true) && previous_view == view && previous_headroom == headroom && *self.wheel.imp().color.borrow() == *state {
            return;
        }
        *self.wheel.imp().color.borrow_mut() = state.clone();
        self.wheel.queue_draw();
        for (button, shape) in self.shape_buttons.iter().zip(state.other_shapes()) {
            let (icon, description) = match shape {
                ColorShape::Circle => ("layer-color-circle-symbolic", "Use Okhsv circle"),
                ColorShape::Square => ("layer-color-square-symbolic", "Use HSV square"),
                ColorShape::Triangle => ("layer-color-triangle-symbolic", "Use HLS triangle"),
            };
            crate::icons::set_button(button, icon);
            button.set_tooltip_text(Some(description));
            button.update_property(&[gtk::accessible::Property::Label(description)]);
        }
        let gamut = if hdr {
            let mut text = state.rgb_space().name().to_string();
            if !state.definition().in_hdr_gamut(state.rgb_space()).unwrap() { text.push_str(" · Outside document gamut"); }
            if !state.definition().in_hdr_gamut(view.space()).unwrap() { text.push_str(" · Outside display gamut"); }
            if ev.is_some_and(|v| v > 0.00001) { text.push_str(" · Above SDR white"); }
            text
        } else { state.gamut_description_in(view.space()) };
        let description = format!("{}. {}", gamut, state.readout_description());
        self.readout.set_tooltip_text(Some(&description));
        self.readout
            .update_property(&[gtk::accessible::Property::Label(&description)]);
        self.readout_drawing.queue_draw();
        for (white, button, sample) in &self.quick_colors {
            let preset = &state.quick_colors()[usize::from(*white)];
            selected(button, preset.selected);
            sample.set_display_color(if *white { layer_core::color::RgbColor::WHITE } else { layer_core::color::RgbColor::BLACK }, ViewColor::Srgb, 1.);
        }
        for (slot, button, sample) in &self.swatches {
            if *slot == state.slot {
                button.add_css_class("selected-tool");
            } else {
                button.remove_css_class("selected-tool");
            }
            let color = match slot { ColorSlot::Foreground => state.foreground, ColorSlot::Background => state.background, ColorSlot::Temporary => state.temporary, ColorSlot::Transparent => layer_core::color::RgbColor { linear_rgb: None, space: state.rgb_space(), rgba: [0.; 4] } };
            sample.set_display_color(color, view, headroom);
        }
    }
}

fn draw_readout(area: &gtk::DrawingArea, cr: &cairo::Context, half: f64, state: &ColorState, view: ViewColor) {
    let mut font = (half * 2. * 0.044).clamp(9., 12.);
    let radius = ColorPanelLayout::new((half * 2.) as f32)
        .unwrap()
        .readout_radius as f64;
    let color = area.color();
    let ink = |alpha| {
        cr.set_source_rgba(
            color.red() as f64,
            color.green() as f64,
            color.blue() as f64,
            alpha,
        )
    };
    cr.select_font_face(
        "Adwaita Sans",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    cr.set_font_size(font);
    ink(0.9);
    cr.move_to(2., font + 1.);
    let _ = cr.show_text(state.readout_label());
    let label_width = cr.text_extents(state.readout_label()).unwrap().x_advance();
    let gamut = |space| if matches!(view, ViewColor::Mapped { .. }) { state.definition().in_hdr_gamut(space) } else { state.definition().in_gamut(space) };
    if !gamut(view.space()).unwrap() || !gamut(state.rgb_space()).unwrap()
    {
        // The definition remains intact. The compact marker's tooltip and
        // accessible label identify the gamut the preview cannot represent.
        cr.move_to(label_width + 6., font + 1.);
        let _ = cr.show_text("!");
    }
    if area
        .parent()
        .is_some_and(|button| button.has_visible_focus())
    {
        // Keep the focus outline in the corner, clear of the picking field.
        rounded_rect(cr, 1., 1., label_width + 5., font + 4., 5.);
        cr.set_line_width(1.5);
        let _ = cr.stroke();
    }
    cr.select_font_face(
        "Adwaita Sans",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    let rgb = state.readout == ColorReadout::Rgb;
    let texts = state.readout_layout_text();
    let mut options = cr.font_options().unwrap();
    options.set_hint_metrics(cairo::HintMetrics::Off);
    cr.set_font_options(&options);
    let available = radius * std::f64::consts::FRAC_PI_2 - 4.;
    let (widths, total, digit_advance) = loop {
        cr.set_font_size(font);
        let digit_advance = ('0'..='9')
            .map(|c| cr.text_extents(&c.to_string()).unwrap().x_advance())
            .fold(0., f64::max);
        // Measure the individual glyph advances used for drawing on the arc.
        let widths = texts.each_ref().map(|text| {
            text.chars()
                .map(|c| if c.is_ascii_digit() || c == ' ' { digit_advance }
                    else { cr.text_extents(&c.to_string()).unwrap().x_advance() })
                .sum::<f64>()
                + if rgb { font * 0.8 + 2. } else { 0. }
        });
        let total: f64 = widths.iter().sum();
        if total + 6. <= available || font <= 8. {
            break (widths, total, digit_advance);
        }
        font -= 0.25;
    };
    let chip = font * 0.8;
    let gap = ((available - total) * 0.5).clamp(3., radius * 0.24);
    let mut cursor = -(total + gap * 2.) * 0.5;
    for (index, text) in texts.iter().enumerate() {
        let width = widths[index];
        let mid = (-135_f64).to_radians() + (cursor + width * 0.5) / radius;
        cursor += width + gap;
        let mut along = -width * 0.5;
        if rgb {
            let angle = mid + (along + chip * 0.5) / radius;
            let _ = cr.save();
            cr.translate(half + radius * angle.cos(), half + radius * angle.sin());
            cr.rotate(angle + std::f64::consts::FRAC_PI_2);
            let [r, g, b] = [[0.93, 0.31, 0.36], [0.25, 0.73, 0.43], [0.29, 0.56, 0.98]][index];
            cr.set_source_rgb(r, g, b);
            rounded_rect(cr, -chip * 0.5, -font * 0.76, chip, chip, 2.);
            let _ = cr.fill();
            let _ = cr.restore();
            along += chip + 2.;
        }
        for c in text.chars() {
            let glyph = c.to_string();
            let glyph_advance = cr.text_extents(&glyph).unwrap().x_advance();
            let advance = if c.is_ascii_digit() || c == ' ' { digit_advance } else { glyph_advance };
            let angle = mid + (along + advance * 0.5) / radius;
            let _ = cr.save();
            cr.translate(half + radius * angle.cos(), half + radius * angle.sin());
            cr.rotate(angle + std::f64::consts::FRAC_PI_2);
            ink(0.8);
            cr.move_to(-glyph_advance * 0.5, 0.);
            let _ = cr.show_text(&glyph);
            let _ = cr.restore();
            along += advance;
        }
    }
}
fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    for (cx, cy, a) in [
        (x + w - r, y + r, -90.),
        (x + w - r, y + h - r, 0.),
        (x + r, y + h - r, 90.),
        (x + r, y + r, 180.),
    ] {
        cr.arc(
            cx,
            cy,
            r,
            a * std::f64::consts::PI / 180.,
            (a + 90.) * std::f64::consts::PI / 180.,
        );
    }
    cr.close_path();
}

fn color_field_path(shape: ColorShape, g: &ColorWheelGeometry) -> gtk::gsk::Path {
    let path = gtk::gsk::PathBuilder::new();
    match shape {
        ColorShape::Circle => path.add_circle(&gtk::graphene::Point::new(g.center[0], g.center[1]), g.disc_radius()),
        ColorShape::Triangle => {
            path.move_to(g.triangle[0][0], g.triangle[0][1]);
            for p in &g.triangle[1..] { path.line_to(p[0], p[1]); }
            path.close();
        }
        ColorShape::Square => {
            let [x,y,w] = g.square;
            path.add_rounded_rect(&gtk::gsk::RoundedRect::from_rect(gtk::graphene::Rect::new(x,y,w,w), (g.center[0] * 0.04).min(6.)));
        }
    }
    path.to_path()
}

fn draw_wheel(snapshot: &gtk::Snapshot, state: &ColorState, g: &ColorWheelGeometry, view: ViewColor, headroom: f32) {
    let hue = state.wheel_hue_color_in(state.wheel_components()[0], view.space());
    let color = state.preview_in(state.definition(), view.space());
    let radius = g.marker_radius();
    for (index, (point, rgb)) in [
        (state.wheel_hue_marker(g, state.wheel_components()[0]), hue),
        (state.wheel_marker(g), [color[0], color[1], color[2]]),
    ].into_iter().enumerate() {
        let path = gtk::gsk::PathBuilder::new();
        path.add_circle(&gtk::graphene::Point::new(point[0], point[1]), radius);
        snapshot.push_fill(&path.to_path(), gtk::gsk::FillRule::Winding);
        let bounds = gtk::graphene::Rect::new(point[0]-radius, point[1]-radius, radius*2.,radius*2.);
        let texture = if index == 1 && matches!(view, ViewColor::Mapped { .. }) {
            let mut p = state.definition().linear_in(state.rgb_space()).unwrap(); p[3] = 1.;
            crate::display_color::picker_texture(view, headroom, state.rgb_space(), [1,1], &[p])
        } else { view.solid([rgb[0],rgb[1],rgb[2],1.]) };
        snapshot.append_texture(&texture, &bounds);
        snapshot.pop();
        let cr = snapshot.append_cairo(&gtk::graphene::Rect::new(0.,0.,g.center[0]*2.,g.center[1]*2.));
        cr.arc(point[0] as f64,point[1] as f64,radius as f64,0.,std::f64::consts::TAU);
        cr.set_source_rgba(0.,0.,0.,0.5);
        cr.set_line_width(4.);
        let _ = cr.stroke_preserve();
        cr.set_source_rgb(1.,1.,1.);
        cr.set_line_width(2.);
        let _ = cr.stroke();
    }
}
