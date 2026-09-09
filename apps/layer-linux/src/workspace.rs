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

#[path = "workspace_customization.rs"]
mod customization;

mod allocation {
    use super::*;

    #[derive(Default)]
    pub struct PanelColumns {
        pub preview: RefCell<Option<gtk::Widget>>,
        pub configuration: RefCell<Option<gtk::Widget>>,
        pub expansion: Cell<Option<PanelExpansion>>,
        pub join: RefCell<Option<gtk::DrawingArea>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for PanelColumns {
        const NAME: &'static str = "CapyPanelColumns";
        type Type = super::PanelColumns;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for PanelColumns {
        fn dispose(&self) {
            if let Some(join) = self.join.take() {
                join.unparent();
            }
            for child in [self.preview.take(), self.configuration.take()]
                .into_iter()
                .flatten()
            {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for PanelColumns {
        fn contains(&self, x: f64, y: f64) -> bool {
            self.expansion.get().map_or_else(
                || self.parent_contains(x, y),
                |e| e.contains([e.bounds.x + x as f32, e.bounds.y + y as f32]),
            )
        }
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }
        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let normal = Bounds {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
            };
            for (child, bounds) in [
                (
                    self.preview.borrow().clone(),
                    self.expansion.get().map_or(normal, |e| e.preview),
                ),
                (
                    self.configuration.borrow().clone(),
                    self.expansion.get().map_or(normal, |e| e.configuration),
                ),
            ] {
                if let Some(child) = child {
                    allocate_at(&child, bounds);
                }
            }
            if let Some(join) = self.join.borrow().as_ref() {
                let expanded = self.expansion.get();
                join.set_visible(expanded.is_some_and(|e| e.concave_join));
                if let Some(e) = expanded {
                    let left = e.configuration.x < e.preview.x;
                    let class = if left {
                        "configuration-left"
                    } else {
                        "configuration-right"
                    };
                    let other = if left {
                        "configuration-right"
                    } else {
                        "configuration-left"
                    };
                    self.obj().remove_css_class(other);
                    self.obj().add_css_class(class);
                    if e.configuration.y == 0.0 {
                        self.obj().add_css_class("configuration-flush");
                    } else {
                        self.obj().remove_css_class("configuration-flush");
                    }
                    let x = if left {
                        e.preview.x - 8.0
                    } else {
                        e.preview.x + e.preview.width
                    };
                    if join.is_visible() {
                        allocate_at(
                            join.upcast_ref(),
                            Bounds {
                                x,
                                y: e.configuration.y - 8.0,
                                width: 8.0,
                                height: 8.0,
                            },
                        );
                        join.queue_draw();
                    }
                }
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            for child in [
                self.preview.borrow().clone(),
                self.configuration.borrow().clone(),
            ]
            .into_iter()
            .flatten()
            {
                self.obj().snapshot_child(&child, snapshot);
            }
            if let Some(join) = self.join.borrow().as_ref()
                && join.is_visible()
            {
                self.obj().snapshot_child(join, snapshot);
            }
        }
    }

    #[derive(Default)]
    pub struct DockSurface {
        pub(super) layout: RefCell<DockLayout>,
        pub(super) children: RefCell<Vec<(Slot, gtk::Widget)>>,
        pub(super) owner: RefCell<std::rc::Weak<Workspace>>,
        pub(super) transition: Cell<Option<(u32, Bounds, f32)>>,
        pub(super) animation: RefCell<Option<adw::TimedAnimation>>,
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
            // size. Content panels scroll; tool ribbons clip their overflow.
            if orientation == gtk::Orientation::Horizontal {
                (640, 1200, -1, -1)
            } else {
                (480, 900, -1, -1)
            }
        }
        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let mut resolved = self.layout.borrow().workspace(
                width as f32,
                height as f32,
                HEADER_HEIGHT,
                STATUS_HEIGHT,
            );
            if let Some((id, from, progress)) = self.transition.get()
                && let Some(group) = resolved.groups.iter_mut().find(|g| g.id == id)
            {
                group.interpolate_from(from, progress);
            }
            let expansion = self
                .owner
                .borrow()
                .upgrade()
                .and_then(|w| w.customization.geometry(&w));
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
                    Slot::ZenButton => Some(Bounds {
                        x: WORKSPACE_SPACING,
                        y: WORKSPACE_SPACING,
                        width: TILE_SIZE,
                        height: TILE_SIZE,
                    }),
                    Slot::Status => Some(resolved.status),
                    Slot::Group(id) => {
                        let expanded = expansion.filter(|e| e.group == *id);
                        child
                            .downcast_ref::<super::PanelColumns>()
                            .unwrap()
                            .imp()
                            .expansion
                            .set(expanded);
                        expanded.map(|e| e.bounds).or_else(|| {
                            resolved
                                .groups
                                .iter()
                                .find(|g| g.id == *id)
                                .map(|g| g.bounds)
                        })
                    }
                    Slot::Divider(id) => resolved
                        .dividers
                        .iter()
                        .find(|d| d.id == *id)
                        .map(|d| d.bounds),
                    Slot::FloatingResize(group, edge) => resolved
                        .groups
                        .iter()
                        .find(|g| g.id == *group && expansion.is_none_or(|e| e.group != *group))
                        .and_then(|g| g.resize_handles.iter().find(|h| h.edge == *edge))
                        .map(|h| h.bounds),
                };
                child.set_child_visible(bounds.is_some());
                if let Some(b) = bounds {
                    allocate_at(child, b);
                }
            }
            if let Some(owner) = self.owner.borrow().upgrade() {
                owner.queue_panel_measurements();
                owner.customization.present_popovers();
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
                let expanded = child.has_css_class("expanded-panel");
                if expanded {
                    // One shadow around the complete stepped silhouette:
                    // preview, tabs and drawer, with no shadow at their seam.
                    snapshot.push_shadow(&[gtk::gsk::Shadow::new(
                        gdk::RGBA::new(0.0, 0.0, 0.0, 0.4),
                        0.0,
                        8.0,
                        24.0,
                    )]);
                }
                self.obj().snapshot_child(child, snapshot);
                if expanded {
                    snapshot.pop();
                }
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

fn allocate_at(child: &gtk::Widget, b: Bounds) {
    child.allocate(
        b.width.max(1.0).round() as i32,
        b.height.max(1.0).round() as i32,
        -1,
        Some(gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(b.x, b.y))),
    );
}

glib::wrapper! {
    pub struct PanelColumns(ObjectSubclass<allocation::PanelColumns>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl PanelColumns {
    fn new(preview: &gtk::Box) -> Self {
        let this: Self = glib::Object::new();
        this.add_css_class("dock-panel");
        this.set_overflow(gtk::Overflow::Hidden);
        preview.set_parent(&this);
        preview.add_css_class("panel-preview");
        preview.set_overflow(gtk::Overflow::Hidden);
        *this.imp().preview.borrow_mut() = Some(preview.clone().upcast());
        // The same concave foot as the tabs, joining the two content surfaces
        // without rounding their shared seam into two separate bubbles.
        let join = gtk::DrawingArea::new();
        join.add_css_class("panel-column-join");
        join.set_can_target(false);
        join.set_visible(false);
        join.set_draw_func(|area, cr, width, height| {
            let color = area.color();
            cr.set_source_rgba(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                color.alpha().into(),
            );
            let left = area
                .parent()
                .is_some_and(|p| p.has_css_class("configuration-left"));
            let x = if left { width as f64 } else { 0.0 };
            let direction = if left { -1.0 } else { 1.0 };
            concave_foot(cr, x, height as f64, width as f64, direction);
            let _ = cr.fill();
        });
        join.set_parent(&this);
        *this.imp().join.borrow_mut() = Some(join);
        this
    }
    fn set_configuration(&self, widget: Option<&gtk::Widget>) {
        if let Some(old) = self.imp().configuration.take() {
            old.unparent();
        }
        if let Some(widget) = widget {
            widget.set_parent(self);
        }
        *self.imp().configuration.borrow_mut() = widget.cloned();
        self.queue_allocate();
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Canvas,
    Header,
    ZenButton,
    Status,
    Group(u32),
    Divider(u32),
    FloatingResize(u32, ResizeEdge),
}
glib::wrapper! {
    pub struct DockSurface(ObjectSubclass<allocation::DockSurface>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl DockSurface {
    fn raise_group(&self, id: u32) {
        let mut children = self.imp().children.borrow_mut();
        let slots: Vec<_> = children
            .iter()
            .filter_map(|(slot, _)| {
                matches!(*slot, Slot::Group(group) | Slot::FloatingResize(group, _) if group == id)
                    .then_some(*slot)
            })
            .collect();
        for slot in slots {
            let index = children.iter().position(|(s, _)| *s == slot).unwrap();
            let item = children.remove(index);
            item.1.insert_after(self, children.last().map(|(_, w)| w));
            children.push(item);
        }
    }
    fn add(&self, slot: Slot, child: &impl IsA<gtk::Widget>) {
        child.set_parent(self);
        self.imp()
            .children
            .borrow_mut()
            .push((slot, child.clone().upcast()));
    }
    fn clear_docks(&self) {
        self.remove_slots(|slot| {
            !matches!(
                slot,
                Slot::Canvas | Slot::Header | Slot::ZenButton | Slot::Status
            )
        });
    }
    fn remove_slots(&self, remove: impl Fn(Slot) -> bool) {
        if !self
            .imp()
            .children
            .borrow()
            .iter()
            .any(|(slot, _)| remove(*slot))
        {
            return;
        }
        let (retained, removed): (Vec<_>, Vec<_>) = self
            .imp()
            .children
            .take()
            .into_iter()
            .partition(|(slot, _)| !remove(*slot));
        *self.imp().children.borrow_mut() = retained;
        // Unparenting can synchronously deliver gesture/popover cancellation.
        // Never retain a mutable collection borrow across GTK callbacks.
        for (_, widget) in removed {
            widget.unparent();
        }
    }
    fn animate_size(&self, group: u32, from: Bounds) {
        if let Some(animation) = self.imp().animation.take() {
            animation.pause();
        }
        self.imp().transition.set(Some((group, from, 0.0)));
        let target = adw::CallbackAnimationTarget::new(glib::clone!(
            #[weak(rename_to = surface)]
            self,
            move |progress| {
                surface
                    .imp()
                    .transition
                    .set(Some((group, from, progress as f32)));
                surface.queue_allocate();
            }
        ));
        let animation = adw::TimedAnimation::new(self, 0.0, 1.0, PANEL_EXPANSION_MS, target);
        animation.set_easing(adw::Easing::EaseOutCubic);
        animation.connect_done(glib::clone!(
            #[weak(rename_to = surface)]
            self,
            move |_| {
                surface.imp().transition.set(None);
                surface.queue_allocate();
            }
        ));
        *self.imp().animation.borrow_mut() = Some(animation.clone());
        animation.play();
    }
}

#[derive(Clone, glib::Boxed)]
#[boxed_type(name = "LayerDockItem")]
struct NativeDockItem(DockItem);

#[derive(Clone, Copy)]
enum DragTarget {
    Dock(DockItem),
    Divider(u32),
    Resize(u32, ResizeEdge),
}
impl DragTarget {
    fn action(
        self,
        phase: ContactPhase,
        position: [f32; 2],
        viewport: [f32; 2],
        tabs: Vec<TabHit>,
    ) -> UiAction {
        match self {
            Self::Dock(item) => UiAction::DragWorkspace {
                item,
                phase,
                position,
                viewport,
                tabs,
            },
            Self::Divider(id) => UiAction::DragDivider {
                id,
                phase,
                position,
                viewport,
            },
            Self::Resize(group, edge) => UiAction::ResizeFloating {
                group,
                edge,
                phase,
                position,
                viewport,
            },
        }
    }
}

#[derive(Clone)]
struct NativeWorkspaceDrag {
    target: DragTarget,
    origin: [f32; 2],
    point: [f32; 2],
    started: bool,
    sequence: Option<gdk::EventSequence>,
}

struct GroupView {
    id: u32,
    root: PanelColumns,
    panels: Vec<Panel>,
    floating: bool,
    tabs_visible: bool,
    stack: gtk::Stack,
    tabs: Vec<(Panel, gtk::Button)>,
    tab_joins: gtk::DrawingArea,
}
struct NativeMenu {
    model: gtk::gio::Menu,
    commands: Vec<CommandId>,
    accelerators: Vec<String>,
}
fn native_accelerator(chord: &KeyChord) -> String {
    let name = match chord.key.as_str() {
        " " | "space" => "space",
        "arrowleft" => "Left",
        "arrowright" => "Right",
        "arrowup" => "Up",
        "arrowdown" => "Down",
        "enter" => "Return",
        "backspace" => "BackSpace",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "escape" => "Escape",
        "tab" => "Tab",
        "pageup" => "Page_Up",
        "pagedown" => "Page_Down",
        other => other,
    };
    let key = if name.chars().count() == 1 {
        // GDK returns a valid keyval for a Unicode scalar.
        unsafe {
            glib::translate::from_glib(gdk::unicode_to_keyval(name.chars().next().unwrap().into()))
        }
    } else {
        gdk::Key::from_name(name)
            .or_else(|| gdk::Key::from_name(name.to_uppercase()))
            .unwrap_or(gdk::Key::VoidSymbol)
    };
    let mut modifiers = gdk::ModifierType::empty();
    if chord.command {
        modifiers |= gdk::ModifierType::CONTROL_MASK;
    }
    if chord.alt {
        modifiers |= gdk::ModifierType::ALT_MASK;
    }
    if chord.shift {
        modifiers |= gdk::ModifierType::SHIFT_MASK;
    }
    gtk::accelerator_name(key, modifiers).into()
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
                    concave_foot(cr, x, y, 6.0, direction);
                }
                let _ = cr.fill();
            }
        }
    ));
    joins
}

fn concave_foot(cr: &gtk::cairo::Context, x: f64, y: f64, radius: f64, direction: f64) {
    let k = radius * 0.447_715;
    cr.move_to(x, y - radius);
    cr.line_to(x, y);
    cr.line_to(x + direction * radius, y);
    cr.curve_to(x + direction * k, y, x, y - k, x, y - radius);
    cr.close_path();
}

pub struct Workspace {
    pub window: adw::ApplicationWindow,
    pub area: gtk::Picture,
    pub gpu: RefCell<Option<GpuCanvas>>,
    pub input: Rc<crate::input::Input>,
    surface: DockSurface,
    palette_css: gtk::CssProvider,
    palette: Cell<Option<ThemePalette>>,
    header: adw::HeaderBar,
    popovers: RefCell<Vec<glib::WeakRef<gtk::Popover>>>,
    chrome_held: Cell<bool>,
    dragging: Cell<bool>,
    drag_targets: RefCell<Vec<(glib::WeakRef<gtk::Widget>, DragTarget)>>,
    workspace_drag: RefCell<Option<NativeWorkspaceDrag>>,
    measuring_panels: Cell<bool>,
    drop_hint: RefCell<Option<DropHint>>,
    toolbar: TileStrip,
    panels: [(Panel, gtk::Widget); Panel::ALL.len()],
    groups: RefCell<Vec<GroupView>>,
    commands: RefCell<Vec<(CommandId, gtk::Button)>>,
    menus: RefCell<Vec<NativeMenu>>,
    menu_actions: gtk::gio::SimpleActionGroup,
    brush_buttons: RefCell<Vec<(u32, gtk::Button)>>,
    brush_previews: RefCell<Vec<(u32, gtk::Picture)>>,
    size_buttons: RefCell<Vec<(f32, gtk::Button)>>,
    size_number: crate::number_control::NumberControl,
    opacity: crate::number_control::NumberControl,
    color: gtk::ColorDialogButton,
    layers: gtk::Box,
    layer_opacity: crate::number_control::NumberControl,
    tab: gtk::Label,
    view_info: gtk::Label,
    status: gtk::Label,
    pub(crate) preferences: crate::preferences::Preferences,
    customization: customization::Customization,
    refreshing: Cell<bool>,
    ticking: Cell<bool>,
    frame_deadline: Cell<u64>,
}
impl Drop for Workspace {
    fn drop(&mut self) {
        // Weak unrealize callbacks cannot upgrade once the final Rc is gone.
        // Join the GPU worker before any native window/surface fields drop.
        self.gpu.get_mut().take();
        self.customization.dispose();
        gtk::style_context_remove_provider_for_display(&self.area.display(), &self.palette_css);
    }
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
        window.set_icon_name(Some("art.capycanvas.CapyCanvas"));
        static NEXT_WINDOW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        window.set_widget_name(&format!(
            "capy-{}",
            NEXT_WINDOW.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let palette_css = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &gtk::prelude::WidgetExt::display(&window),
            &palette_css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
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
        let size_number = crate::number_control::NumberControl::new(
            NumericControl::brush_size(),
            "Brush size",
            "",
        );
        size_number.set_widget_name("brush-size");
        let opacity =
            crate::number_control::NumberControl::new(NumericControl::percent(), "Opacity", "");
        opacity.set_width_request(100);
        let layer_opacity = crate::number_control::NumberControl::new(
            NumericControl::percent(),
            "Layer opacity",
            "",
        );
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
            palette_css,
            palette: Cell::new(None),
            header,
            popovers: RefCell::new(Vec::new()),
            chrome_held: Cell::new(false),
            dragging: Cell::new(false),
            drag_targets: RefCell::new(Vec::new()),
            workspace_drag: RefCell::new(None),
            drop_hint: RefCell::new(None),
            measuring_panels: Cell::new(false),
            toolbar: toolbar.clone(),
            groups: RefCell::new(Vec::new()),
            panels: [
                (Panel::Toolbar, toolbar.clone().upcast()),
                (Panel::Brushes, scroll(&brushes)),
                (Panel::Sizes, scroll(&sizes)),
                (Panel::Layers, scroll(&layers_panel)),
            ],
            commands: RefCell::new(Vec::new()),
            menus: RefCell::new(Vec::new()),
            menu_actions: gtk::gio::SimpleActionGroup::new(),
            brush_buttons: RefCell::new(Vec::new()),
            brush_previews: RefCell::new(Vec::new()),
            size_buttons: RefCell::new(Vec::new()),
            size_number,
            opacity,
            color,
            layers,
            layer_opacity,
            tab,
            view_info,
            status,
            preferences: crate::preferences::Preferences::new(),
            customization: customization::Customization::new(),
            refreshing: Cell::new(false),
            ticking: Cell::new(false),
            frame_deadline: Cell::new(0),
            input: Rc::default(),
        });
        this.apply_palette(Settings::default().palette(
            if adw::StyleManager::default().is_dark() {
                Theme::Dark
            } else {
                Theme::Light
            },
            Platform::Gtk,
        ));
        *this.surface.imp().owner.borrow_mut() = Rc::downgrade(&this);
        this.build_controls(&brushes, &sizes, &layers_panel);
        this.customization.bind(&this);
        this.preferences.bind(&this);
        this.install_chrome();
        crate::input::install(&this);
        this.install_gpu();
        this.reconcile_layout(&DockLayout::default());
        this.install_drop_target();
        this
    }

    fn build_controls(self: &Rc<Self>, brushes: &gtk::Box, sizes: &gtk::Box, layers: &gtk::Box) {
        margins(brushes, 8);
        let brush_list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        brushes.append(&brush_list);
        for category in brush_categories() {
            let heading = gtk::Label::new(Some(category.label));
            heading.add_css_class("heading");
            heading.add_css_class("dim-label");
            heading.set_halign(gtk::Align::Start);
            margins(&heading, 8);
            brush_list.append(&heading);
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
                brush_list.append(&button);
                self.brush_buttons.borrow_mut().push((choice.id, button));
            }
        }
        self.customization
            .track(Panel::Brushes, PanelControl::Brushes, &brush_list);
        self.append_panel_fields(Panel::Brushes, brushes);
        margins(sizes, 8);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        self.size_number.set_hexpand(true);
        row.append(&self.size_number);
        sizes.append(&row);
        self.customization
            .track(Panel::Sizes, PanelControl::BrushSize, &row);
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
        self.customization
            .track(Panel::Sizes, PanelControl::SizePresets, &grid);
        self.append_panel_fields(Panel::Sizes, sizes);
        margins(layers, 12);
        let commands = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        commands.add_css_class("layer-tools");
        for command in CommandId::LAYERS {
            let button = self.command_button(command);
            button.set_width_request(40);
            commands.append(&button);
        }
        layers.append(&commands);
        self.customization
            .track(Panel::Layers, PanelControl::LayerActions, &commands);
        layers.append(&self.layers);
        self.customization
            .track(Panel::Layers, PanelControl::Layers, &self.layers);
        let layer_alpha = gtk::Box::new(gtk::Orientation::Vertical, 12);
        layer_alpha.append(&self.layer_opacity);
        layers.append(&layer_alpha);
        self.customization
            .track(Panel::Layers, PanelControl::LayerOpacity, &layer_alpha);
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
                if this.preferences.recording() {
                    return glib::Propagation::Proceed;
                }
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
        self.window
            .insert_action_group("editor", Some(&self.menu_actions));
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
        if let Some(image) = zen.child().and_downcast::<gtk::Image>() {
            image.set_pixel_size(ZEN_ICON_SIZE as i32);
        }
        zen.add_css_class("chrome-control");
        zen.add_css_class("workspace-zen");
        // Keep one button outside the fading header, at the same 6px inset.
        // Its spacer preserves the native menu and title allocation in both modes.
        let zen_space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        zen_space.set_size_request(TILE_SIZE as i32, TILE_SIZE as i32);
        self.header.pack_start(&zen_space);
        self.surface.add(Slot::ZenButton, &zen);
        self.install_context(&zen, ContextTarget::ZenMode);
        for menu in MENUS {
            self.header
                .pack_start(&self.chrome_menu(menu.label, menu.sections));
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
                if matches!(
                    event.event_type(),
                    gdk::EventType::ButtonPress | gdk::EventType::TouchBegin
                ) && this.customization.placement().is_some()
                    && let Some((x, y)) = event.position()
                {
                    let offset = this.window.surface_transform();
                    if this
                        .chrome_event(ChromeEvent::Contact {
                            position: [(x + offset.0) as f32, (y + offset.1) as f32],
                            canvas: false,
                        })
                        .handled
                    {
                        return glib::Propagation::Stop;
                    }
                }
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

    fn chrome_menu(self: &Rc<Self>, label: &str, sections: &[&[CommandId]]) -> gtk::MenuButton {
        let menu = gtk::MenuButton::builder()
            .label(label)
            .tooltip_text(label)
            .build();
        menu.add_css_class("flat");
        menu.add_css_class("chrome-control");
        menu.set_direction(gtk::ArrowType::None);
        let root = gtk::gio::Menu::new();
        for &commands in sections {
            let model = gtk::gio::Menu::new();
            for &id in commands {
                let name = id.shortcut_id();
                let action = if id.is_toggle() {
                    gtk::gio::SimpleAction::new_stateful(&name, None, &false.to_variant())
                } else {
                    gtk::gio::SimpleAction::new(&name, None)
                };
                action.connect_activate(glib::clone!(
                    #[weak(rename_to = this)]
                    self,
                    move |_, _| this.dispatch(UiAction::Invoke { command: id })
                ));
                self.menu_actions.add_action(&action);
                model.append(Some(id.label()), Some(&format!("editor.{name}")));
            }
            root.append_section(None, &model);
            // Keep each section's model so shortcut updates target its own items.
            self.menus.borrow_mut().push(NativeMenu {
                model,
                commands: commands.to_vec(),
                accelerators: vec![String::new(); commands.len()],
            });
        }
        let popover = gtk::PopoverMenu::from_model(Some(&root));
        if sections.is_empty() {
            popover.set_widget_name("workspace-menu");
            popover.connect_show(glib::clone!(
                #[weak(rename_to = w)]
                self,
                move |popup| {
                    let model = w.gpu.borrow().as_ref().map(|g| g.session.workspace_menu());
                    if let Some(model) = model {
                        w.populate_workspace_menu(popup, model);
                    }
                }
            ));
        }
        self.watch_popover(popover.upcast_ref());
        menu.set_popover(Some(&popover));
        menu
    }

    fn watch_popover(self: &Rc<Self>, popover: &gtk::Popover) {
        self.popovers.borrow_mut().retain(|p| p.upgrade().is_some());
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
        let contact_tab = if let ChromeEvent::Contact { position, .. } = event {
            let picked = self.surface.pick(
                position[0] as f64,
                position[1] as f64,
                gtk::PickFlags::DEFAULT,
            );
            self.groups
                .borrow()
                .iter()
                .flat_map(|g| &g.tabs)
                .find_map(|(panel, tab)| {
                    let picked = picked.as_ref()?;
                    (picked == tab.upcast_ref::<gtk::Widget>() || picked.is_ancestor(tab))
                        .then_some(*panel)
                })
        } else {
            None
        };
        let facts = ChromeFacts {
            contact_tab,
            expanded_panel: self.customization.placement(),
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
        if reply.change.regions != 0
            && let Some(owner) = self.surface.imp().owner.borrow().upgrade()
        {
            owner.changed(Ok(reply.change));
        }
        reply
    }

    pub fn interact(self: &Rc<Self>, input: UiInput) -> InputReply {
        if matches!(input, UiInput::Blur) {
            self.workspace_drag.borrow_mut().take();
            self.clear_drop();
        }
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
        self.set_chrome_hidden(
            reply.chrome_hidden,
            reply.hide_floating_panels,
            reply.keep_zen_button,
        );
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

    fn set_chrome_hidden(&self, hidden: bool, hide_floating_panels: bool, keep_zen_button: bool) {
        for (slot, widget) in self.surface.imp().children.borrow().iter() {
            if !matches!(slot, Slot::Canvas) {
                let hidden = if *slot == Slot::ZenButton {
                    if hidden && keep_zen_button {
                        widget.add_css_class("zen-button-neutral");
                    } else {
                        widget.remove_css_class("zen-button-neutral");
                    }
                    hidden && !keep_zen_button
                } else {
                    hidden && (hide_floating_panels || !widget.has_css_class("floating-panel"))
                };
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
        let animate = if let UiAction::DoubleClickPanelHandle { group, .. } = action {
            self.groups
                .borrow()
                .iter()
                .find(|g| g.id == group)
                .and_then(|g| {
                    g.root.compute_bounds(&self.surface).map(|b| {
                        (
                            group,
                            Bounds {
                                x: b.x(),
                                y: b.y(),
                                width: b.width(),
                                height: b.height(),
                            },
                        )
                    })
                })
        } else {
            None
        };
        let result = self
            .gpu
            .borrow_mut()
            .as_mut()
            .map(|g| g.session.dispatch(action));
        if let Some(result) = result {
            let succeeded = result.is_ok();
            self.changed(result);
            if let Some((group, from)) = animate.filter(|_| succeeded) {
                self.surface.animate_size(group, from);
            }
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
            for menu in self.menus.borrow_mut().iter_mut() {
                for (index, id) in menu.commands.iter().enumerate() {
                    if let Some(command) = state.commands.iter().find(|c| c.id == *id) {
                        let action = self
                            .menu_actions
                            .lookup_action(&id.shortcut_id())
                            .unwrap()
                            .downcast::<gtk::gio::SimpleAction>()
                            .unwrap();
                        action.set_enabled(command.enabled);
                        if action.state().is_some() {
                            action.set_state(&command.selected.to_variant());
                        }
                        let accel = command
                            .bindings
                            .first()
                            .map(native_accelerator)
                            .unwrap_or_default();
                        if menu.accelerators[index] != accel {
                            let item = gtk::gio::MenuItem::new(
                                Some(command.label),
                                Some(&format!("editor.{}", id.shortcut_id())),
                            );
                            item.set_attribute_value("accel", Some(&accel.to_variant()));
                            menu.model.remove(index as i32);
                            menu.model.insert_item(index as i32, &item);
                            menu.accelerators[index] = accel;
                        }
                    }
                }
            }
        }
        if regions & regions::SETTINGS != 0 {
            self.apply_palette(state.palette);
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
        if regions
            & (regions::CUSTOMIZATION
                | regions::LAYOUT
                | regions::BRUSH
                | regions::COMMANDS
                | regions::DOCUMENT)
            != 0
        {
            self.customization.refresh(self);
        }
        self.refreshing.set(false);
        if regions & (regions::LAYOUT | regions::SETTINGS) != 0 {
            self.update_zen();
        }
    }

    fn apply_palette(&self, palette: ThemePalette) {
        if self.palette.replace(Some(palette)) == Some(palette) {
            return;
        }
        let roles = [
            ("bg", palette.bg),
            ("panel", palette.panel),
            ("tabbar", palette.tabbar),
            ("input", palette.input),
            ("view", palette.view),
            ("settings", palette.settings),
            ("sidebar", palette.sidebar),
            ("sidebar-backdrop", palette.sidebar_backdrop),
            ("dialog", palette.dialog),
            ("thumb", palette.thumb),
            ("text", palette.text),
        ];
        let mut css = format!("window#{} {{", self.window.widget_name());
        for (name, color) in roles {
            use std::fmt::Write;
            write!(css, "--capy-{name}: {color};").unwrap();
        }
        css.push('}');
        self.palette_css.load_from_string(&css);
    }

    fn resolved(&self) -> ResolvedLayout {
        self.surface.imp().layout.borrow().workspace(
            self.surface.width().max(1) as f32,
            self.surface.height().max(1) as f32,
            HEADER_HEIGHT,
            STATUS_HEIGHT,
        )
    }
    fn queue_panel_measurements(self: &Rc<Self>) {
        if self.measuring_panels.replace(true) {
            return;
        }
        glib::idle_add_local_once(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move || {
                w.measuring_panels.set(false);
                let Some(layout) = w
                    .gpu
                    .borrow()
                    .as_ref()
                    .map(|g| g.session.state().workspace.layout.clone())
                else {
                    return;
                };
                let groups = w.groups.borrow();
                let resolved = w.resolved();
                let measurements = layout
                    .panels
                    .iter()
                    .map(|config| {
                        let tab_width = groups
                            .iter()
                            .flat_map(|g| &g.tabs)
                            .find(|(id, _)| *id == config.id)
                            .map_or(0.0, |(_, tab)| {
                                tab.measure(gtk::Orientation::Horizontal, -1).1 as f32
                            });
                        let width = resolved
                            .groups
                            .iter()
                            .find(|g| g.panels.contains(&config.id))
                            .map_or(232.0, |g| g.bounds.width);
                        let widget = w.panel_widget(config.id);
                        let content = widget
                            .downcast_ref::<gtk::ScrolledWindow>()
                            .and_then(|s| s.child())
                            .unwrap_or(widget);
                        // Manual shrink can clip a panel below its natural
                        // minimum. GTK still requires a valid measure request.
                        let width =
                            (width as i32).max(content.measure(gtk::Orientation::Horizontal, -1).0);
                        PanelMeasurement {
                            panel: config.id,
                            tab_width,
                            content_height: content.measure(gtk::Orientation::Vertical, width).1
                                as f32,
                        }
                    })
                    .collect::<Vec<_>>();
                drop(groups);
                if measurements != layout.measurements {
                    w.dispatch(UiAction::MeasurePanels { measurements });
                }
            }
        ));
    }
    fn reconcile_layout(self: &Rc<Self>, layout: &DockLayout) {
        self.customization.reconcile_toolbars(self, layout);
        *self.surface.imp().layout.borrow_mut() = layout.clone();
        let resolved = self.resolved();
        let same_groups = self.groups.borrow().len() == resolved.groups.len()
            && self.groups.borrow().iter().all(|view| {
                resolved.groups.iter().any(|g| {
                    view.id == g.id
                        && view.panels == g.panels
                        && view.tabs_visible == g.tabs_visible
                })
            });
        let same_dividers = self
            .surface
            .imp()
            .children
            .borrow()
            .iter()
            .filter_map(|(slot, _)| {
                if let Slot::Divider(id) = slot {
                    Some(*id)
                } else {
                    None
                }
            })
            .eq(resolved.dividers.iter().map(|d| d.id));
        if !same_groups {
            self.customization.collapse_panel();
            for panel in layout.panels.iter().map(|p| self.panel_widget(p.id)) {
                if let Some(stack) = panel.parent().and_downcast::<gtk::Stack>() {
                    stack.remove(&panel);
                }
            }
            self.surface.clear_docks();
            self.groups.borrow_mut().clear();
            for group in &resolved.groups {
                let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
                let root = PanelColumns::new(&column);
                if group.floating {
                    root.add_css_class("floating-panel");
                }
                if !group.tabs_visible && group.active.kind() == PanelKind::Tiles {
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
                        self.install_context(&tab, ContextTarget::Panel { panel });
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
                    self.install_panel_drag(&header, DockItem::Group { group: group.id });
                    header.append(&grip);
                    self.install_context(&header, ContextTarget::Group { group: group.id });
                    column.append(&header);
                }
                let stack = gtk::Stack::new();
                stack.set_hexpand(true);
                stack.set_vexpand(true);
                stack.set_hhomogeneous(false);
                stack.set_vhomogeneous(false);
                for &panel in &group.panels {
                    let widget = self.panel_widget(panel);
                    stack.add_named(&widget, Some(&format!("{panel:?}")));
                }
                column.append(&stack);
                if let Some(bounds) = group.footer_grip {
                    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                    footer.add_css_class("panel-footer");
                    footer.set_widget_name(&format!("panel-footer-grip-{}", group.id));
                    footer.set_height_request(bounds.height as i32);
                    let grip = tiles::grip();
                    grip.set_hexpand(true);
                    grip.set_halign(gtk::Align::Center);
                    grip.set_valign(gtk::Align::Center);
                    footer.append(&grip);
                    self.install_panel_drag(&footer, DockItem::Group { group: group.id });
                    self.install_context(&footer, ContextTarget::Group { group: group.id });
                    column.append(&footer);
                }
                self.surface.add(Slot::Group(group.id), &root);
                self.groups.borrow_mut().push(GroupView {
                    id: group.id,
                    root,
                    panels: group.panels.clone(),
                    floating: group.floating,
                    tabs_visible: group.tabs_visible,
                    stack,
                    tabs,
                    tab_joins,
                });
            }
        }
        if !same_groups || !same_dividers {
            self.surface
                .remove_slots(|slot| matches!(slot, Slot::Divider(_)));
            for divider in &resolved.dividers {
                self.add_divider(divider.clone());
            }
        }
        self.surface.remove_slots(|slot| {
            matches!(slot, Slot::FloatingResize(id, _)
            if !resolved.groups.iter().any(|g| g.id == id && g.floating))
        });
        for group in resolved.groups.iter().filter(|g| g.floating) {
            for resize in &group.resize_handles {
                let slot = Slot::FloatingResize(group.id, resize.edge);
                if self
                    .surface
                    .imp()
                    .children
                    .borrow()
                    .iter()
                    .any(|(s, _)| *s == slot)
                {
                    continue;
                }
                let handle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                handle.add_css_class("floating-panel");
                handle.set_cursor_from_name(Some(resize.edge.cursor()));
                handle.set_widget_name(&format!("floating-resize-{}-{:?}", group.id, resize.edge));
                self.register_drag(&handle, DragTarget::Resize(group.id, resize.edge));
                self.surface.add(slot, &handle);
            }
        }
        // Z-order changes retain widgets and the native pointer grab.
        self.groups
            .borrow_mut()
            .sort_by_key(|g| resolved.groups.iter().position(|r| r.id == g.id));
        for group in resolved.groups.iter().filter(|g| g.floating) {
            self.surface.raise_group(group.id);
        }
        if let Some(expansion) = self.customization.placement() {
            self.surface.raise_group(expansion.group);
        }
        for (view, group) in self.groups.borrow_mut().iter_mut().zip(&resolved.groups) {
            view.floating = group.floating;
            if group.floating {
                view.root.add_css_class("floating-panel");
            } else {
                view.root.remove_css_class("floating-panel");
            }
            let name = format!("{:?}", group.active);
            if view.stack.child_by_name(&name).is_some() {
                view.stack.set_visible_child_name(&name);
            }
            for toolbar in self
                .customization
                .toolbars
                .borrow()
                .iter()
                .filter(|t| group.panels.contains(&t.id))
            {
                toolbar.strip.configure(group.axis, !group.tabs_visible);
            }
            for (panel, button) in &view.tabs {
                let config = layout.panel(*panel).expect("validated panel");
                if config.tab_style == TabStyle::Name {
                    button.set_label(config.title());
                } else {
                    button.set_icon_name(&format!("layer-{}-symbolic", config.icon()));
                }
                button.set_tooltip_text(Some(config.title()));
                button.update_property(&[gtk::accessible::Property::Label(config.title())]);
                selected(button, *panel == group.active);
            }
            view.tab_joins.queue_draw();
        }
        self.surface.queue_allocate();
    }
    fn install_panel_drag(self: &Rc<Self>, widget: &impl IsA<gtk::Widget>, item: DockItem) {
        self.register_drag(widget, DragTarget::Dock(item));
        if !matches!(item, DockItem::Tile { .. }) {
            return;
        }
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
    fn tab_hits(&self) -> Vec<TabHit> {
        self.groups
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
            .collect()
    }
    fn drop_at(&self, x: f32, y: f32, item: DockItem) -> Option<DropHint> {
        self.gpu.borrow().as_ref()?.session.drop_hint(
            [self.surface.width() as f32, self.surface.height() as f32],
            [x, y],
            &self.tab_hits(),
            item,
            self.customization.placement(),
        )
    }
    fn install_drop_target(self: &Rc<Self>) {
        self.install_workspace_drag();
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
        self.register_drag(&handle, DragTarget::Divider(divider.id));
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

    fn register_drag(&self, widget: &impl IsA<gtk::Widget>, target: DragTarget) {
        let mut targets = self.drag_targets.borrow_mut();
        targets.retain(|(widget, _)| widget.upgrade().is_some());
        targets.push((widget.as_ref().downgrade(), target));
    }

    fn drag_target_at(&self, point: [f32; 2]) -> Option<DragTarget> {
        let mut picked =
            self.surface
                .pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT);
        while let Some(widget) = picked {
            if let Some(target) = self
                .drag_targets
                .borrow()
                .iter()
                .rev()
                .find_map(|(w, target)| (w.upgrade().as_ref() == Some(&widget)).then_some(*target))
            {
                return (!matches!(target, DragTarget::Dock(DockItem::Tile { .. })))
                    .then_some(target);
            }
            picked = widget.parent();
        }
        None
    }

    fn dispatch_drag(self: &Rc<Self>, target: DragTarget, phase: ContactPhase, position: [f32; 2]) {
        let tabs = if matches!(target, DragTarget::Dock(_)) && phase == ContactPhase::Up {
            self.tab_hits()
        } else {
            Vec::new()
        };
        self.dispatch(target.action(
            phase,
            position,
            [self.surface.width() as f32, self.surface.height() as f32],
            tabs,
        ));
    }

    fn workspace_drag_input(
        self: &Rc<Self>,
        phase: ContactPhase,
        point: [f32; 2],
        sequence: Option<gdk::EventSequence>,
    ) -> bool {
        if phase == ContactPhase::Down {
            if self.workspace_drag.borrow().is_none()
                && let Some(target) = self.drag_target_at(point)
            {
                *self.workspace_drag.borrow_mut() = Some(NativeWorkspaceDrag {
                    target,
                    origin: point,
                    point,
                    started: false,
                    sequence,
                });
            }
            return false;
        }
        let Some(mut drag) = self
            .workspace_drag
            .borrow()
            .clone()
            .filter(|d| d.sequence == sequence)
        else {
            return false;
        };
        if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
            self.workspace_drag.borrow_mut().take();
            if drag.started {
                self.dispatch_drag(drag.target, phase, point);
            }
            self.clear_drop();
            self.update_zen();
            return drag.started;
        }
        if !drag.started {
            let recognized = if matches!(drag.target, DragTarget::Dock(_)) {
                self.surface.drag_check_threshold(
                    drag.origin[0] as i32,
                    drag.origin[1] as i32,
                    point[0] as i32,
                    point[1] as i32,
                )
            } else {
                point != drag.origin
            };
            if !recognized {
                return false;
            }
            // Reset click/hold recognizers once this is a drag. Merely denying
            // them leaves stale sequence state: this stable controller consumes
            // the release, so their next click would only clear that old drag.
            let mut picked = self.surface.pick(
                drag.origin[0] as f64,
                drag.origin[1] as f64,
                gtk::PickFlags::DEFAULT,
            );
            while let Some(widget) = picked {
                let controllers = widget.observe_controllers();
                for i in 0..controllers.n_items() {
                    if let Some(gesture) = controllers.item(i).and_downcast::<gtk::Gesture>() {
                        gesture.reset();
                    }
                }
                if &widget == self.surface.upcast_ref::<gtk::Widget>() {
                    break;
                }
                picked = widget.parent();
            }
            drag.started = true;
            *self.workspace_drag.borrow_mut() = Some(drag.clone());
            self.dispatch_drag(drag.target, ContactPhase::Down, drag.origin);
        }
        drag.point = point;
        *self.workspace_drag.borrow_mut() = Some(drag.clone());
        self.dispatch_drag(drag.target, ContactPhase::Move, point);
        if let DragTarget::Dock(item) = drag.target {
            *self.drop_hint.borrow_mut() = self.drop_at(point[0], point[1], item);
            self.surface.queue_draw();
        }
        true
    }

    fn install_workspace_drag(self: &Rc<Self>) {
        let click = gtk::GestureClick::new();
        click.set_name(Some("panel-handle-double-click"));
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |gesture, count, x, y| {
                if count == 2
                    && let Some(DragTarget::Dock(item)) = w.drag_target_at([x as f32, y as f32])
                    && let Some(group) = {
                        let layout = w.surface.imp().layout.borrow();
                        layout.panel_handle_target(item)
                    }
                {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    w.dispatch(UiAction::DoubleClickPanelHandle {
                        group,
                        viewport: [w.surface.width() as f32, w.surface.height() as f32],
                    });
                }
            }
        ));
        self.surface.add_controller(click);
        // A window-surface event stream survives unparenting the pressed tab.
        // GtkGestureDrag cancels that sequence when tear-off rebuilds its group.
        // Native widgets still receive clicks until GTK's drag threshold passes.
        let pointer = gtk::EventControllerLegacy::new();
        pointer.set_name(Some("workspace-drag"));
        pointer.set_propagation_phase(gtk::PropagationPhase::Capture);
        pointer.connect_event(glib::clone!(
            #[weak(rename_to = w)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |controller, event| {
                let (phase, touch) = match event.event_type() {
                    gdk::EventType::ButtonPress | gdk::EventType::ButtonRelease => {
                        if event
                            .downcast_ref::<gdk::ButtonEvent>()
                            .is_none_or(|e| e.button() != 1)
                        {
                            return glib::Propagation::Proceed;
                        }
                        (
                            if event.event_type() == gdk::EventType::ButtonPress {
                                ContactPhase::Down
                            } else {
                                ContactPhase::Up
                            },
                            false,
                        )
                    }
                    gdk::EventType::MotionNotify => (ContactPhase::Move, false),
                    gdk::EventType::TouchBegin => (ContactPhase::Down, true),
                    gdk::EventType::TouchUpdate => (ContactPhase::Move, true),
                    gdk::EventType::TouchEnd => (ContactPhase::Up, true),
                    gdk::EventType::TouchCancel => (ContactPhase::Cancel, true),
                    _ => return glib::Propagation::Proceed,
                };
                let sequence = touch.then(|| event.event_sequence());
                let point = w
                    .event_point(controller)
                    .or_else(|| w.workspace_drag.borrow().as_ref().map(|d| d.point));
                if point.is_some_and(|point| w.workspace_drag_input(phase, point, sequence)) {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        self.surface.add_controller(pointer);
    }
}

fn margins(widget: &impl IsA<gtk::Widget>, value: i32) {
    widget.set_margin_start(value);
    widget.set_margin_end(value);
    widget.set_margin_top(value);
    widget.set_margin_bottom(value);
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
