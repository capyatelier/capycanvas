//! Native controls around one shared session. This file translates state into
//! GTK widgets; document, command and docking decisions stay in layer-ui.

use crate::{
    canvas::GpuCanvas,
    tiles::{self, TileStrip},
};
use adw::prelude::*;
use gtk::{gdk, glib, subclass::prelude::*};
use layer_ui::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

mod allocation {
    use super::*;

    #[derive(Default)]
    pub struct DockSurface {
        pub(super) layout: RefCell<DockLayout>,
        pub(super) children: RefCell<Vec<(Slot, gtk::Widget)>>,
        pub(super) owner: RefCell<std::rc::Weak<Workspace>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for DockSurface {
        const NAME: &'static str = "LayerDockSurface";
        type Type = super::DockSurface;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for DockSurface {
        fn dispose(&self) {
            for (_, child) in self.children.take() {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for DockSurface {
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            // Dock allocation can shrink below an individual panel's natural
            // size. Native scrollers handle overflow, not the document canvas.
            if orientation == gtk::Orientation::Horizontal {
                (640, 1200, -1, -1)
            } else {
                (480, 900, -1, -1)
            }
        }
        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let resolved = self.layout.borrow().workspace(
                width as f32,
                height as f32,
                HEADER_HEIGHT,
                STATUS_HEIGHT,
            );
            for (slot, child) in self.children.borrow().iter() {
                let bounds = match slot {
                    // Native surface, input and cursor share full-window coordinates.
                    Slot::Canvas => Some(Bounds {
                        x: 0.0,
                        y: 0.0,
                        width: width as f32,
                        height: height as f32,
                    }),
                    Slot::Header => Some(Bounds {
                        x: 0.0,
                        y: 0.0,
                        width: width as f32,
                        height: HEADER_HEIGHT,
                    }),
                    Slot::Status => Some(resolved.status),
                    Slot::Group(id) => resolved
                        .groups
                        .iter()
                        .find(|g| g.id == *id)
                        .map(|g| g.bounds),
                    Slot::Divider(id) => resolved
                        .dividers
                        .iter()
                        .find(|d| d.id == *id)
                        .map(|d| d.bounds),
                };
                if let Some(b) = bounds {
                    let transform =
                        gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(b.x, b.y));
                    child.allocate(
                        b.width.max(1.0).round() as i32,
                        b.height.max(1.0).round() as i32,
                        -1,
                        Some(transform),
                    );
                }
            }
            if let Some(owner) = self.owner.borrow().upgrade() {
                let scale = owner.area.scale_factor() as u32;
                let extent = [
                    owner.area.width().max(1) as u32 * scale,
                    owner.area.height().max(1) as u32 * scale,
                ];
                if owner
                    .gpu
                    .borrow()
                    .as_ref()
                    .is_some_and(|g| g.session.state().camera.viewport != extent)
                {
                    owner.wake();
                }
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            for (_, child) in self.children.borrow().iter() {
                self.obj().snapshot_child(child, snapshot);
            }
            if let Some(owner) = self.owner.borrow().upgrade()
                && let Some(hint) = owner.drop_hint.borrow().as_ref()
            {
                let b = hint.bounds;
                snapshot.append_color(
                    &gdk::RGBA::new(0.38, 0.68, 1.0, 0.95),
                    &gtk::graphene::Rect::new(b.x, b.y, b.width, b.height),
                );
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Canvas,
    Header,
    Status,
    Group(u32),
    Divider(u32),
}
glib::wrapper! {
    pub struct DockSurface(ObjectSubclass<allocation::DockSurface>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl DockSurface {
    fn add(&self, slot: Slot, child: &impl IsA<gtk::Widget>) {
        child.set_parent(self);
        self.imp()
            .children
            .borrow_mut()
            .push((slot, child.clone().upcast()));
    }
    fn clear_docks(&self) {
        self.imp().children.borrow_mut().retain(|(slot, widget)| {
            if matches!(slot, Slot::Canvas | Slot::Header | Slot::Status) {
                true
            } else {
                widget.unparent();
                false
            }
        });
    }
}

#[derive(Clone, glib::Boxed)]
#[boxed_type(name = "LayerDockItem")]
struct NativeDockItem(DockItem);

struct GroupView {
    id: u32,
    panels: Vec<Panel>,
    stack: gtk::Stack,
    tabs: Vec<(Panel, gtk::Button)>,
    tab_joins: gtk::DrawingArea,
}

// GTK CSS has no pseudo-elements. This non-interactive native overlay paints
// only the selected tab's two concave feet; native buttons still own all input.
fn tab_joins(header: &gtk::Box) -> gtk::DrawingArea {
    let joins = gtk::DrawingArea::new();
    joins.add_css_class("tab-joins");
    joins.set_can_target(false);
    joins.set_draw_func(glib::clone!(
        #[weak]
        header,
        move |area, cr, _, height| {
            let color = area.color();
            cr.set_source_rgba(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                color.alpha().into(),
            );
            let mut child = header.first_child();
            while let Some(tab) = child {
                child = tab.next_sibling();
                if !tab.has_css_class("selected-tool") {
                    continue;
                }
                let Some(bounds) = tab.compute_bounds(area) else {
                    continue;
                };
                let y = height as f64;
                for (x, direction) in [
                    (bounds.x() as f64, -1.0),
                    ((bounds.x() + bounds.width()) as f64, 1.0),
                ] {
                    cr.move_to(x, y - 6.0);
                    cr.line_to(x, y);
                    cr.line_to(x + direction * 6.0, y);
                    cr.curve_to(x + direction * 2.686, y, x, y - 2.686, x, y - 6.0);
                    cr.close_path();
                }
                let _ = cr.fill();
            }
        }
    ));
    joins
}

pub struct Workspace {
    pub window: adw::ApplicationWindow,
    pub area: gtk::Picture,
    pub gpu: RefCell<Option<GpuCanvas>>,
    pub input: Rc<crate::input::Input>,
    surface: DockSurface,
    header: adw::HeaderBar,
    popovers: RefCell<Vec<glib::WeakRef<gtk::Popover>>>,
    chrome_held: Cell<bool>,
    dragging: Cell<bool>,
    drop_hint: RefCell<Option<DropHint>>,
    toolbar: TileStrip,
    panels: [(Panel, gtk::Widget); Panel::ALL.len()],
    groups: RefCell<Vec<GroupView>>,
    commands: RefCell<Vec<(CommandId, gtk::Button)>>,
    shortcut_hints: RefCell<Vec<(CommandId, gtk::Label)>>,
    brush_buttons: RefCell<Vec<(u32, gtk::Button)>>,
    brush_previews: RefCell<Vec<(u32, gtk::Picture)>>,
    size_buttons: RefCell<Vec<(f32, gtk::Button)>>,
    size: gtk::Scale,
    size_number: gtk::SpinButton,
    opacity: gtk::Scale,
    color: gtk::ColorDialogButton,
    layers: gtk::Box,
    layer_opacity: gtk::Scale,
    tab: gtk::Label,
    view_info: gtk::Label,
    status: gtk::Label,
    pub(crate) preferences: crate::preferences::Preferences,
    refreshing: Cell<bool>,
    ticking: Cell<bool>,
    frame_deadline: Cell<u64>,
}

impl Workspace {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        static ICONS: std::sync::Once = std::sync::Once::new();
        ICONS.call_once(|| {
            gtk::gio::resources_register_include!("layer-icons.gresource").expect("bundled icons");
            gtk::IconTheme::for_display(&gdk::Display::default().unwrap())
                .add_resource_path("/dev/layer/icons");
        });
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(APP_NAME)
            .default_width(1200)
            .default_height(900)
            .build();
        if !adw::StyleManager::default().is_dark() {
            window.add_css_class("light-theme");
        }
        let area = gtk::Picture::builder()
            .can_shrink(true)
            .content_fit(gtk::ContentFit::Fill)
            .hexpand(true)
            .vexpand(true)
            .focusable(true)
            .build();
        area.set_widget_name("drawing-canvas");
        area.set_cursor_from_name(Some("none"));
        let surface: DockSurface = glib::Object::new();
        surface.set_hexpand(true);
        surface.set_vexpand(true);
        let tab = gtk::Label::new(Some(APP_NAME));
        tab.add_css_class("document-title");
        let header = adw::HeaderBar::new();
        header.add_css_class("workspace-header");
        header.set_title_widget(Some(&tab));
        let view_info = gtk::Label::new(Some("100% · 0°"));
        let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        status_bar.add_css_class("workspace-status");
        view_info.add_css_class("status-bubble");
        view_info.set_hexpand(true);
        view_info.set_halign(gtk::Align::End);
        view_info.set_valign(gtk::Align::End);
        status_bar.append(&view_info);
        surface.add(Slot::Canvas, &area);
        surface.add(Slot::Header, &header);
        surface.add(Slot::Status, &status_bar);
        let toolbar = TileStrip::new();
        toolbar.add_css_class("toolbar-controls");
        let brushes = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let sizes = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let layers_panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let layers = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let size = scale(BRUSH_SIZE_SLIDER);
        let size_number = gtk::SpinButton::with_range(
            BRUSH_SIZE_CONTROL.min,
            BRUSH_SIZE_CONTROL.max,
            BRUSH_SIZE_CONTROL.step,
        );
        size_number.set_digits(BRUSH_SIZE_CONTROL.digits);
        size_number.set_widget_name("brush-size");
        let opacity = scale(OPACITY_CONTROL);
        opacity.set_width_request(100);
        let layer_opacity = scale(OPACITY_CONTROL);
        let color = gtk::ColorDialogButton::new(Some(
            gtk::ColorDialog::builder().with_alpha(false).build(),
        ));
        let status = gtk::Label::new(None);
        status.set_visible(false);
        status.add_css_class("error");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&surface);
        content.append(&status);
        window.set_content(Some(&content));
        let this = Rc::new(Self {
            window,
            area,
            gpu: RefCell::new(None),
            surface,
            header,
            popovers: RefCell::new(Vec::new()),
            chrome_held: Cell::new(false),
            dragging: Cell::new(false),
            drop_hint: RefCell::new(None),
            toolbar: toolbar.clone(),
            groups: RefCell::new(Vec::new()),
            panels: Panel::ALL.map(|panel| {
                (
                    panel,
                    match panel {
                        Panel::Toolbar => toolbar.clone().upcast(),
                        Panel::Brushes => scroll(&brushes),
                        Panel::Sizes => scroll(&sizes),
                        Panel::Layers => scroll(&layers_panel),
                    },
                )
            }),
            commands: RefCell::new(Vec::new()),
            shortcut_hints: RefCell::new(Vec::new()),
            brush_buttons: RefCell::new(Vec::new()),
            brush_previews: RefCell::new(Vec::new()),
            size_buttons: RefCell::new(Vec::new()),
            size,
            size_number,
            opacity,
            color,
            layers,
            layer_opacity,
            tab,
            view_info,
            status,
            preferences: crate::preferences::Preferences::new(),
            refreshing: Cell::new(false),
            ticking: Cell::new(false),
            frame_deadline: Cell::new(0),
            input: Rc::default(),
        });
        *this.surface.imp().owner.borrow_mut() = Rc::downgrade(&this);
        this.build_controls(&toolbar, &brushes, &sizes, &layers_panel);
        this.preferences.bind(&this);
        this.install_chrome();
        crate::input::install(&this);
        this.install_gpu();
        this.reconcile_layout(&DockLayout::default());
        this.install_drop_target();
        this
    }

    fn build_controls(
        self: &Rc<Self>,
        toolbar: &TileStrip,
        brushes: &gtk::Box,
        sizes: &gtk::Box,
        layers: &gtk::Box,
    ) {
        shared_spin_icons(self.size_number.upcast_ref());
        for &item in TOOLBAR_CONTROLS {
            let (is_color, label, control) = match item {
                ToolbarControl::Command { command } => {
                    toolbar.append(&self.command_button(command));
                    continue;
                }
                ToolbarControl::Color => (
                    true,
                    "Brush color",
                    self.color.clone().upcast::<gtk::Widget>(),
                ),
                ToolbarControl::Opacity => (
                    false,
                    "Brush opacity",
                    self.opacity.clone().upcast::<gtk::Widget>(),
                ),
            };
            let button = gtk::MenuButton::builder().tooltip_text(label).build();
            let icon = gtk::Image::from_icon_name(if is_color {
                "layer-color-symbolic"
            } else {
                "layer-opacity-symbolic"
            });
            if is_color {
                icon.add_css_class("brush-color");
                let palette = gtk::CssProvider::new();
                // Scoped to this image, including in multi-window sessions.
                #[allow(deprecated)]
                icon.style_context()
                    .add_provider(&palette, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
                let update = move |color: &gtk::ColorDialogButton| {
                    palette.load_from_string(&format!(
                        ".brush-color {{ -gtk-icon-palette: success {}; }}",
                        color.rgba()
                    ));
                };
                update(&self.color);
                self.color.connect_rgba_notify(update);
            }
            button.set_child(Some(&icon));
            button.add_css_class("flat");
            let popover = gtk::Popover::new();
            let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
            margins(&body, 12);
            body.append(&gtk::Label::new(Some(label)));
            body.append(&control);
            popover.set_child(Some(&body));
            self.watch_popover(&popover);
            button.set_popover(Some(&popover));
            toolbar.append(&button);
        }
        let grip = tiles::grip();
        self.install_panel_drag(
            &grip,
            DockItem::Panel {
                panel: Panel::Toolbar,
            },
        );
        toolbar.set_grip(&grip);
        margins(brushes, 8);
        for category in brush_categories() {
            let heading = gtk::Label::new(Some(category.label));
            heading.add_css_class("heading");
            heading.add_css_class("dim-label");
            heading.set_halign(gtk::Align::Start);
            margins(&heading, 8);
            brushes.append(&heading);
            for choice in category.brushes {
                let button =
                    self.action_button(choice.label, UiAction::SelectBrush { id: choice.id });
                button.add_css_class("flat");
                button.add_css_class("brush-choice");
                button.set_widget_name(&format!("brush-{}", choice.id));
                button.set_tooltip_text(Some(choice.label));
                let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
                let preview = gtk::Picture::builder()
                    .can_shrink(true)
                    .content_fit(gtk::ContentFit::Fill)
                    .height_request(40)
                    .build();
                preview.set_paintable(Some(&crate::previews::texture(choice.id, Theme::Dark)));
                let label = gtk::Label::new(Some(choice.label));
                label.set_halign(gtk::Align::End);
                content.append(&preview);
                content.append(&label);
                button.set_child(Some(&content));
                self.brush_previews.borrow_mut().push((choice.id, preview));
                brushes.append(&button);
                self.brush_buttons.borrow_mut().push((choice.id, button));
            }
        }
        margins(sizes, 8);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        self.size.set_hexpand(true);
        row.append(&self.size);
        row.append(&self.size_number);
        sizes.append(&row);
        let grid = gtk::FlowBox::builder()
            .homogeneous(true)
            .min_children_per_line(2)
            .max_children_per_line(4)
            .selection_mode(gtk::SelectionMode::None)
            .column_spacing(2)
            .row_spacing(4)
            .build();
        for &value in BRUSH_SIZES {
            let button = self.action_button("", UiAction::SetBrushSize { value });
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
            self.size_buttons.borrow_mut().push((value, button));
        }
        sizes.append(&grid);
        margins(layers, 12);
        let commands = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        commands.add_css_class("layer-tools");
        for command in CommandId::LAYERS {
            let button = self.command_button(command);
            button.set_width_request(40);
            commands.append(&button);
        }
        layers.append(&commands);
        layers.append(&self.layers);
        layers.append(&gtk::Label::new(Some("Layer opacity")));
        layers.append(&self.layer_opacity);
        self.size.connect_value_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |v| this.dispatch(UiAction::SetBrushSize {
                value: v.value() as f32
            })
        ));
        self.size_number.connect_value_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |v| this.dispatch(UiAction::SetBrushSize {
                value: v.value() as f32
            })
        ));
        self.opacity.connect_value_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |v| this.dispatch(UiAction::SetBrushOpacity {
                value: v.value() as f32
            })
        ));
        self.color.connect_rgba_notify(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |v| {
                let c = v.rgba();
                this.dispatch(UiAction::SetColor {
                    rgba: [c.red(), c.green(), c.blue(), c.alpha()],
                });
            }
        ));
        self.layer_opacity.connect_value_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |v| this.dispatch(UiAction::SetLayerOpacity {
                id: None,
                opacity: v.value() as f32
            })
        ));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                this.update_zen();
                let editing = gtk::prelude::GtkWindowExt::focus(&this.window).is_some_and(|w| {
                    w.is::<gtk::Text>()
                        || w.is::<gtk::Entry>()
                        || w.is::<gtk::Range>()
                        || w.is::<gtk::DropDown>()
                        || w.is::<gtk::CheckButton>()
                });
                if this
                    .interact(crate::input::key_input(key, true, modifiers, editing, None))
                    .handled
                {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        keys.connect_key_released(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, key, _, modifiers| {
                this.interact(crate::input::key_input(key, false, modifiers, false, None));
            }
        ));
        self.window.add_controller(keys);
    }

    pub fn action_button(self: &Rc<Self>, label: &str, action: UiAction) -> gtk::Button {
        let button = gtk::Button::with_label(label);
        button.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.dispatch(action.clone())
        ));
        button
    }
    fn command_button(self: &Rc<Self>, command: CommandId) -> gtk::Button {
        let button = self.action_button(command.label(), UiAction::Invoke { command });
        if let Some(icon) = command.icon() {
            button.set_icon_name(&format!("layer-{icon}-symbolic"));
        }
        button.add_css_class("flat");
        button.set_tooltip_text(Some(command.label()));
        button.set_widget_name(&format!("command-{command:?}"));
        self.commands.borrow_mut().push((command, button.clone()));
        button
    }

    fn install_chrome(self: &Rc<Self>) {
        // Leave the default manager following the system; apply explicit
        // overrides only to the display manager, so system changes stay observable.
        adw::StyleManager::default().connect_dark_notify(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |style| this.dispatch(UiAction::SystemThemeChanged {
                theme: if style.is_dark() {
                    Theme::Dark
                } else {
                    Theme::Light
                },
            })
        ));
        let zen = self.command_button(CommandId::ZenMode);
        zen.add_css_class("chrome-control");
        self.header.pack_start(&zen);
        for menu in MENUS {
            self.header
                .pack_start(&self.chrome_menu(menu.label, menu.commands));
        }
        let primary = self.chrome_menu("Main Menu", PRIMARY_MENU);
        primary.set_icon_name("layer-menu-symbolic");
        self.header.pack_end(&primary);
        // Observe native title-bar grabs without claiming events from Adw's
        // window handle. WM grabs can consume release; the next unpressed
        // motion also clears the latch, never a leave/cancel during the drag.
        let hold = gtk::EventControllerLegacy::new();
        hold.set_propagation_phase(gtk::PropagationPhase::Capture);
        hold.connect_event(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, event| {
                let was_held = this.chrome_held.get();
                if event.event_type() == gdk::EventType::ButtonPress
                    && event.position().is_some_and(|(_, y)| {
                        y + this.window.surface_transform().1 < HEADER_HEIGHT as f64
                    })
                {
                    this.chrome_held.set(true);
                } else if event.event_type() == gdk::EventType::ButtonRelease
                    || (matches!(
                        event.event_type(),
                        gdk::EventType::MotionNotify | gdk::EventType::EnterNotify
                    ) && !event
                        .modifier_state()
                        .contains(gdk::ModifierType::BUTTON1_MASK))
                {
                    this.chrome_held.set(false);
                }
                if was_held != this.chrome_held.get() {
                    this.update_zen();
                }
                glib::Propagation::Proceed
            }
        ));
        self.window.add_controller(hold);
        let motion = gtk::EventControllerMotion::new();
        motion.set_propagation_phase(gtk::PropagationPhase::Capture);
        motion.connect_motion(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, x, y| {
                this.chrome_event(ChromeEvent::Motion {
                    position: [x as f32, y as f32],
                });
            }
        ));
        motion.connect_leave(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                this.chrome_event(ChromeEvent::Leave { touch: false });
            }
        ));
        self.window.add_controller(motion);
    }

    fn chrome_menu(self: &Rc<Self>, label: &str, commands: &[CommandId]) -> gtk::MenuButton {
        let menu = gtk::MenuButton::builder()
            .label(label)
            .tooltip_text(label)
            .build();
        menu.add_css_class("flat");
        menu.add_css_class("chrome-control");
        menu.set_direction(gtk::ArrowType::None);
        let popover = gtk::Popover::new();
        let contents = gtk::Box::new(gtk::Orientation::Vertical, 2);
        margins(&contents, 6);
        for &id in commands {
            let button = self.command_button(id);
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 24);
            let label = gtk::Label::builder()
                .label(id.label())
                .xalign(0.0)
                .hexpand(true)
                .build();
            let hint = gtk::Label::new(None);
            hint.add_css_class("dim-label");
            hint.add_css_class("shortcut-hint");
            row.append(&label);
            row.append(&hint);
            button.set_child(Some(&row));
            self.shortcut_hints.borrow_mut().push((id, hint));
            button.connect_clicked(glib::clone!(
                #[weak]
                popover,
                move |_| popover.popdown()
            ));
            contents.append(&button);
        }
        popover.set_child(Some(&contents));
        self.watch_popover(&popover);
        menu.set_popover(Some(&popover));
        menu
    }

    fn watch_popover(self: &Rc<Self>, popover: &gtk::Popover) {
        self.popovers.borrow_mut().push(popover.downgrade());
        popover.connect_visible_notify(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.update_zen()
        ));
    }

    fn update_zen(&self) {
        self.chrome_event(ChromeEvent::Refresh);
    }

    fn chrome_event(&self, event: ChromeEvent) -> InputReply {
        let facts = ChromeFacts {
            held: self.chrome_held.get(),
            dragging: self.dragging.get(),
            popup_open: self
                .popovers
                .borrow()
                .iter()
                .filter_map(|p| p.upgrade())
                .any(|p| p.is_visible()),
        };
        let reply = self
            .gpu
            .borrow_mut()
            .as_mut()
            .and_then(|g| {
                g.session
                    .input(UiInput::Chrome {
                        event,
                        facts,
                        viewport: [
                            self.surface.width().max(1) as f32,
                            self.surface.height().max(1) as f32,
                        ],
                    })
                    .ok()
            })
            .unwrap_or_default();
        self.present_interaction(reply);
        reply
    }

    pub fn interact(self: &Rc<Self>, input: UiInput) -> InputReply {
        #[cfg(test)]
        let input_start = std::time::Instant::now();
        let result = self
            .gpu
            .borrow_mut()
            .as_mut()
            .map(|g| g.session.input(input));
        let reply = match result {
            Some(Ok(reply)) => {
                self.present_interaction(reply);
                self.changed(Ok(reply.change));
                reply
            }
            Some(Err(error)) => {
                self.changed(Err(error));
                InputReply::default()
            }
            None => InputReply::default(),
        };
        #[cfg(test)]
        if let Some(gpu) = self.gpu.borrow().as_ref() {
            gpu.session
                .engine()
                .backend()
                .stats
                .lock()
                .unwrap()
                .input_handler_cpu
                .push(input_start.elapsed().as_secs_f64() * 1000.0);
        }
        reply
    }

    fn present_interaction(&self, reply: InputReply) {
        self.set_chrome_hidden(reply.chrome_hidden);
        let cursor = Some(if reply.pan_cursor { "grab" } else { "none" });
        if self.area.cursor().and_then(|c| c.name()).as_deref() != cursor {
            self.area.set_cursor_from_name(cursor);
        }
        if reply.dismiss_popups {
            let popovers: Vec<_> = self
                .popovers
                .borrow()
                .iter()
                .filter_map(|p| p.upgrade())
                .collect();
            for popover in popovers {
                popover.popdown();
            }
        }
        self.refresh_cursor();
    }

    pub fn cursor_input(&self, event: Option<layer_engine::PenEvent>) {
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            gpu.session.cursor_input(event);
        }
        self.refresh_cursor();
    }

    pub fn refresh_cursor(&self) {
        let changed = self
            .gpu
            .borrow_mut()
            .as_mut()
            .is_some_and(|g| g.update_cursor());
        if changed && let Some(owner) = self.surface.imp().owner.borrow().upgrade() {
            owner.wake();
        }
    }

    /// A pen without hover (or a first touch) reveals hidden nearby controls
    /// without leaving an accidental mark. Coordinates are canvas-local units.
    pub fn reveal_chrome_at(&self, x: f32, y: f32) -> bool {
        self.chrome_event(ChromeEvent::Contact {
            position: [x, y],
            canvas: true,
        })
        .handled
    }

    fn set_chrome_hidden(&self, hidden: bool) {
        for (slot, widget) in self.surface.imp().children.borrow().iter() {
            if !matches!(slot, Slot::Canvas) {
                if widget.has_css_class("zen-hidden") == hidden && widget.can_target() != hidden {
                    continue;
                }
                if hidden {
                    widget.add_css_class("zen-hidden");
                } else {
                    widget.remove_css_class("zen-hidden");
                }
                widget.set_can_target(!hidden);
            }
        }
    }
    pub fn dispatch(self: &Rc<Self>, action: UiAction) {
        if self.refreshing.get() {
            return;
        }
        let result = self
            .gpu
            .borrow_mut()
            .as_mut()
            .map(|g| g.session.dispatch(action));
        if let Some(result) = result {
            self.changed(result);
        }
    }
    pub fn changed(self: &Rc<Self>, result: Result<UiChange, String>) {
        match result {
            Ok(change) => {
                self.refresh_cursor();
                self.status.set_visible(false);
                if change.regions != 0 {
                    self.refresh(change.regions);
                }
                if change.canvas_wake {
                    self.wake();
                }
                if change.regions & regions::HOST != 0 {
                    if let Some(error) = self
                        .gpu
                        .borrow()
                        .as_ref()
                        .and_then(|g| g.session.state().host_error.as_ref())
                        .cloned()
                    {
                        self.status.set_text(&error);
                        self.status.set_visible(true);
                    }
                    self.preferences.service(self);
                }
            }
            Err(error) => {
                self.status.set_text(&error);
                self.status.set_visible(true);
                eprintln!("{error}");
            }
        }
    }
    pub fn wake(self: &Rc<Self>) {
        if self.ticking.replace(true) {
            return;
        }
        let first = crate::canvas::schedule(
            self.frame_deadline.get(),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    #[cfg(test)]
                    let frame_start = std::time::Instant::now();
                    let area = &this.area;
                    if !area.is_mapped() {
                        this.ticking.set(false);
                        return glib::ControlFlow::Break;
                    }
                    // The first wake can precede initial allocation.
                    // Fit the document only once the real canvas extent exists.
                    if area.width() <= 1 || area.height() <= 1 {
                        return glib::ControlFlow::Continue;
                    }
                    let now = glib::monotonic_time().max(0) as u64 * 1000;
                    // Retain pacing across short pan/hover bursts as well as ink.
                    let previous = this.frame_deadline.get();
                    #[cfg(test)]
                    if previous != 0
                        && let Some(gpu) = this.gpu.borrow().as_ref()
                    {
                        gpu.session
                            .engine()
                            .backend()
                            .stats
                            .lock()
                            .unwrap()
                            .wake_lateness
                            .push(now.saturating_sub(previous) as f64 / 1_000_000.0);
                    }
                    let period = crate::canvas::FRAME_NS;
                    let next = if previous == 0 {
                        now + period
                    } else {
                        previous + ((now.saturating_sub(previous) / period) + 1) * period
                    };
                    this.frame_deadline.set(next);
                    this.input.flush(&this);
                    let result = this.gpu.borrow_mut().as_mut().map(|g| g.render(area, now));
                    match result {
                        Some(Ok(change)) => this.changed(Ok(change)),
                        Some(Err(error)) => {
                            this.gpu_error(&error);
                            this.ticking.set(false);
                            return glib::ControlFlow::Break;
                        }
                        None => {}
                    }
                    let active = this.input.has_pending()
                        || this.gpu.borrow().as_ref().is_some_and(|g| {
                            g.session.engine().has_active_stroke() || g.needs_present
                        });
                    #[cfg(test)]
                    if let Some(gpu) = this.gpu.borrow().as_ref() {
                        gpu.session
                            .engine()
                            .backend()
                            .stats
                            .lock()
                            .unwrap()
                            .frame_handler_cpu
                            .push(frame_start.elapsed().as_secs_f64() * 1000.0);
                    }
                    if active {
                        glib::ControlFlow::Continue
                    } else {
                        this.ticking.set(false);
                        glib::ControlFlow::Break
                    }
                }
            ),
        );
        // schedule may advance an expired deadline after a genuinely idle gap.
        // Keep our next deadline aligned with the actual kernel timer phase.
        self.frame_deadline.set(first);
    }
    fn install_gpu(self: &Rc<Self>) {
        self.area.connect_realize(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |area| {
                match GpuCanvas::new(area) {
                    Ok(gpu) => {
                        *this.gpu.borrow_mut() = Some(gpu);
                        this.refresh(regions::ALL);
                        this.wake();
                    }
                    Err(error) => this.gpu_error(&error),
                }
            }
        ));
        self.area.connect_map(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.wake()
        ));
        self.area.connect_scale_factor_notify(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.wake()
        ));
        self.area.connect_unrealize(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |area| {
                area.set_paintable(None::<&gdk::Texture>);
                this.gpu.borrow_mut().take();
            }
        ));
    }
    fn gpu_error(&self, error: &str) {
        eprintln!("Canvas failed: {error}");
        self.status.set_text(&format!("Canvas failed: {error}"));
        self.status.set_visible(true);
    }
    fn refresh(self: &Rc<Self>, regions: u32) {
        let Some(state) = self
            .gpu
            .borrow()
            .as_ref()
            .map(|g| g.session.state().clone())
        else {
            return;
        };
        self.refreshing.set(true);
        if regions & regions::BRUSH != 0 {
            self.size.set_value(state.brush.diameter as f64);
            self.size_number.set_value(state.brush.diameter as f64);
            self.opacity.set_value(state.brush.opacity as f64);
            let [r, g, b, a] = state.brush.color;
            self.color.set_rgba(&gdk::RGBA::new(r, g, b, a));
            self.toolbar.queue_draw();
            for (id, button) in self.brush_buttons.borrow().iter() {
                selected(button, *id == state.brush.preset);
            }
            for (value, button) in self.size_buttons.borrow().iter() {
                selected(button, *value == state.brush.diameter);
            }
        }
        if regions & regions::DOCUMENT != 0 {
            while let Some(child) = self.layers.first_child() {
                self.layers.remove(&child);
            }
            for layer in &state.layers {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                let visible = gtk::CheckButton::new();
                visible.set_active(layer.visible);
                visible.set_tooltip_text(Some(&format!("Show {}", layer.label)));
                let id = layer.id;
                visible.connect_toggled(glib::clone!(
                    #[weak(rename_to = this)]
                    self,
                    move |v| this.dispatch(UiAction::SetLayerVisibility {
                        id,
                        visible: v.is_active()
                    })
                ));
                row.append(&visible);
                let button = self.action_button(&layer.label, UiAction::SelectLayer { id });
                button.set_hexpand(true);
                button.set_sensitive(layer.editable);
                selected(&button, layer.selected);
                row.append(&button);
                self.layers.append(&row);
            }
            if let Some(layer) = state.layers.iter().find(|l| l.selected) {
                self.layer_opacity.set_value(layer.opacity as f64);
            }
            if let Some(tab) = state.tabs.first() {
                self.tab
                    .set_text(&format!("{} · {} × {}", tab.title, tab.width, tab.height));
            }
        }
        if regions & regions::COMMANDS != 0 {
            for (id, button) in self.commands.borrow().iter() {
                if let Some(command) = state.commands.iter().find(|c| c.id == *id) {
                    button.set_sensitive(command.enabled);
                    selected(button, command.selected);
                }
            }
            for (id, hint) in self.shortcut_hints.borrow().iter() {
                if let Some(command) = state.commands.iter().find(|c| c.id == *id) {
                    hint.set_text(&command.shortcut);
                }
            }
        }
        if regions & regions::SETTINGS != 0 {
            for (id, preview) in self.brush_previews.borrow().iter() {
                preview.set_paintable(Some(&crate::previews::texture(*id, state.theme)));
            }
            if state.theme == Theme::Light {
                self.window.add_css_class("light-theme");
            } else {
                self.window.remove_css_class("light-theme");
            }
            adw::StyleManager::for_display(&self.area.display()).set_color_scheme(
                match state.settings.theme {
                    None => adw::ColorScheme::Default,
                    Some(Theme::Light) => adw::ColorScheme::ForceLight,
                    Some(Theme::Dark) => adw::ColorScheme::ForceDark,
                },
            );
            let view = self
                .gpu
                .borrow()
                .as_ref()
                .and_then(|g| g.session.preferences());
            self.preferences.refresh(self, view);
        }
        if regions & regions::CAMERA != 0 {
            self.view_info.set_text(&format!(
                "{:.0}% · {:.0}°",
                state.camera.zoom * 100.0,
                state.camera.rotation.to_degrees()
            ));
        }
        if regions & regions::LAYOUT != 0 {
            self.reconcile_layout(&state.workspace.layout);
        }
        self.refreshing.set(false);
        if regions & (regions::LAYOUT | regions::SETTINGS) != 0 {
            self.update_zen();
        }
    }

    fn resolved(&self) -> ResolvedLayout {
        self.surface.imp().layout.borrow().workspace(
            self.surface.width().max(1) as f32,
            self.surface.height().max(1) as f32,
            HEADER_HEIGHT,
            STATUS_HEIGHT,
        )
    }
    fn reconcile_layout(self: &Rc<Self>, layout: &DockLayout) {
        *self.surface.imp().layout.borrow_mut() = layout.clone();
        let resolved = self.resolved();
        let same = self
            .groups
            .borrow()
            .iter()
            .map(|g| (g.id, &g.panels))
            .eq(resolved.groups.iter().map(|g| (g.id, &g.panels)));
        if !same {
            for (_, panel) in &self.panels {
                if let Some(stack) = panel.parent().and_downcast::<gtk::Stack>() {
                    stack.remove(panel);
                }
            }
            self.surface.clear_docks();
            self.groups.borrow_mut().clear();
            for group in &resolved.groups {
                let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
                root.add_css_class("dock-panel");
                if !group.tabs_visible {
                    root.add_css_class("tool-strip");
                }
                root.set_overflow(gtk::Overflow::Hidden);
                let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                header.add_css_class("dock-tabs");
                header.set_height_request(layer_ui::TAB_BAR_HEIGHT as i32);
                let labels = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                let tab_joins = tab_joins(&labels);
                let mut tabs = Vec::new();
                if group.tabs_visible {
                    for &panel in &group.panels {
                        let tab = self.action_button(
                            panel.label(),
                            UiAction::SelectPanelTab {
                                group: group.id,
                                panel,
                            },
                        );
                        tab.add_css_class("flat");
                        tab.set_valign(gtk::Align::Center);
                        self.install_panel_drag(&tab, DockItem::Panel { panel });
                        labels.append(&tab);
                        tabs.push((panel, tab));
                    }
                    let tab_bar = gtk::Overlay::new();
                    tab_bar.set_child(Some(&labels));
                    tab_bar.add_overlay(&tab_joins);
                    let scroll = gtk::ScrolledWindow::builder()
                        .hscrollbar_policy(gtk::PolicyType::External)
                        .vscrollbar_policy(gtk::PolicyType::Never)
                        .hexpand(true)
                        .child(&tab_bar)
                        .build();
                    header.append(&scroll);
                    let grip = tiles::grip();
                    grip.set_size_request(20, 24);
                    grip.set_halign(gtk::Align::End);
                    grip.set_valign(gtk::Align::Center);
                    self.install_panel_drag(&grip, DockItem::Group { group: group.id });
                    header.append(&grip);
                    root.append(&header);
                }
                let stack = gtk::Stack::new();
                stack.set_hexpand(true);
                stack.set_vexpand(true);
                stack.set_hhomogeneous(false);
                stack.set_vhomogeneous(false);
                for &panel in &group.panels {
                    let widget = &self.panels.iter().find(|(p, _)| *p == panel).unwrap().1;
                    stack.add_named(widget, Some(&format!("{panel:?}")));
                }
                root.append(&stack);
                self.surface.add(Slot::Group(group.id), &root);
                self.groups.borrow_mut().push(GroupView {
                    id: group.id,
                    panels: group.panels.clone(),
                    stack,
                    tabs,
                    tab_joins,
                });
            }
            for divider in &resolved.dividers {
                self.add_divider(divider.clone());
            }
        }
        for (view, group) in self.groups.borrow().iter().zip(&resolved.groups) {
            view.stack
                .set_visible_child_name(&format!("{:?}", group.active));
            if group.panels.contains(&Panel::Toolbar) {
                self.toolbar.configure(group.axis, !group.tabs_visible);
            }
            for (panel, button) in &view.tabs {
                selected(button, *panel == group.active);
            }
            view.tab_joins.queue_draw();
        }
        self.surface.queue_allocate();
    }
    fn install_panel_drag(self: &Rc<Self>, widget: &impl IsA<gtk::Widget>, item: DockItem) {
        let source = gtk::DragSource::builder()
            .actions(gdk::DragAction::MOVE)
            .build();
        source.set_content(Some(&gdk::ContentProvider::for_value(
            &NativeDockItem(item).to_value(),
        )));
        source.connect_drag_begin(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| {
                this.dragging.set(true);
                this.update_zen();
            }
        ));
        source.connect_drag_end(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _, _| {
                this.dragging.set(false);
                this.clear_drop();
                this.update_zen();
            }
        ));
        widget.add_controller(source);
    }
    fn clear_drop(&self) {
        self.drop_hint.borrow_mut().take();
        self.surface.queue_draw();
    }
    fn drop_at(&self, x: f32, y: f32, item: DockItem) -> Option<DropHint> {
        let tabs = self
            .groups
            .borrow()
            .iter()
            .flat_map(|g| {
                g.tabs.iter().enumerate().filter_map(|(index, (_, tab))| {
                    let b = tab.compute_bounds(&self.surface)?;
                    Some(TabHit {
                        group: g.id,
                        index,
                        bounds: Bounds {
                            x: b.x(),
                            y: b.y(),
                            width: b.width(),
                            height: b.height(),
                        },
                    })
                })
            })
            .collect::<Vec<_>>();
        self.gpu.borrow().as_ref()?.session.drop_hint(
            [self.surface.width() as f32, self.surface.height() as f32],
            [x, y],
            &tabs,
            item,
        )
    }
    fn install_drop_target(self: &Rc<Self>) {
        let drop = gtk::DropTarget::new(NativeDockItem::static_type(), gdk::DragAction::MOVE);
        drop.set_preload(true);
        drop.connect_motion(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            gdk::DragAction::empty(),
            move |drop, x, y| {
                let hint = drop
                    .value()
                    .and_then(|v| v.get::<NativeDockItem>().ok())
                    .and_then(|p| this.drop_at(x as f32, y as f32, p.0));
                let valid = hint.is_some();
                *this.drop_hint.borrow_mut() = hint;
                this.surface.queue_draw();
                if valid {
                    gdk::DragAction::MOVE
                } else {
                    gdk::DragAction::empty()
                }
            }
        ));
        drop.connect_leave(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.clear_drop()
        ));
        drop.connect_drop(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            false,
            move |_, value, x, y| {
                this.clear_drop();
                let Ok(NativeDockItem(item)) = value.get::<NativeDockItem>() else {
                    return false;
                };
                let Some(hint) = this.drop_at(x as f32, y as f32, item) else {
                    return false;
                };
                this.dispatch(item.move_action(
                    hint.target,
                    [this.surface.width() as f32, this.surface.height() as f32],
                ));
                true
            }
        ));
        self.surface.add_controller(drop);
    }
    fn event_point(&self, controller: &impl IsA<gtk::EventController>) -> Option<[f32; 2]> {
        let (x, y) = controller.current_event()?.position()?;
        let (dx, dy) = self.window.surface_transform();
        let p = self.window.compute_point(
            &self.surface,
            &gtk::graphene::Point::new((x + dx) as f32, (y + dy) as f32),
        )?;
        Some([p.x(), p.y()])
    }
    fn add_divider(self: &Rc<Self>, divider: Divider) {
        let handle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        handle.set_cursor_from_name(Some(if divider.axis == Axis::Horizontal {
            "col-resize"
        } else {
            "row-resize"
        }));
        handle.set_focusable(true);
        handle.set_tooltip_text(Some("Resize dock"));
        let drag = gtk::GestureDrag::new();
        drag.connect_drag_begin(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |gesture, _, _| {
                if let Some(position) = this.event_point(gesture) {
                    this.dispatch(UiAction::DragDivider {
                        id: divider.id,
                        phase: ContactPhase::Down,
                        position,
                        viewport: [this.surface.width() as f32, this.surface.height() as f32],
                    });
                }
                this.dragging.set(true);
                this.update_zen();
                gesture.set_state(gtk::EventSequenceState::Claimed);
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |gesture, _, _| {
                if let Some(point) = this.event_point(gesture) {
                    this.dispatch(UiAction::DragDivider {
                        id: divider.id,
                        phase: ContactPhase::Move,
                        position: point,
                        viewport: [this.surface.width() as f32, this.surface.height() as f32],
                    });
                }
            }
        ));
        drag.connect_drag_end(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _, _| {
                this.dispatch(UiAction::DragDivider {
                    id: divider.id,
                    phase: ContactPhase::Cancel,
                    position: [0.0; 2],
                    viewport: [this.surface.width() as f32, this.surface.height() as f32],
                });
                this.dragging.set(false);
                this.update_zen();
            }
        ));
        handle.add_controller(drag);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                if this
                    .interact(crate::input::key_input(
                        key,
                        true,
                        modifiers,
                        false,
                        Some(divider.id),
                    ))
                    .handled
                {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        handle.add_controller(keys);
        self.surface.add(Slot::Divider(divider.id), &handle);
    }
}

/// Keep the native spin behavior, replacing only its theme-dependent glyphs.
pub(crate) fn shared_spin_icons(widget: &gtk::Widget) {
    if let Some(button) = widget.downcast_ref::<gtk::Button>() {
        if button.has_css_class("up") {
            button.set_icon_name("layer-plus-symbolic");
        } else if button.has_css_class("down") {
            button.set_icon_name("layer-minus-symbolic");
        }
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        shared_spin_icons(&current);
        child = current.next_sibling();
    }
}

fn margins(widget: &impl IsA<gtk::Widget>, value: i32) {
    widget.set_margin_start(value);
    widget.set_margin_end(value);
    widget.set_margin_top(value);
    widget.set_margin_bottom(value);
}
fn scale(spec: NumericControl) -> gtk::Scale {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, spec.min, spec.max, spec.step);
    scale.set_draw_value(false);
    scale
}
fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::Widget {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(child)
        .build()
        .upcast()
}
fn selected(widget: &impl IsA<gtk::Widget>, selected: bool) {
    if selected {
        widget.add_css_class("selected-tool");
    } else {
        widget.remove_css_class("selected-tool");
    }
}
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
