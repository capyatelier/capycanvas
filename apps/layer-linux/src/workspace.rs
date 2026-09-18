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

#[path = "workspace_columns.rs"]
mod columns;
#[path = "workspace_customization.rs"]
mod customization;
#[path = "workspace_drawer.rs"]
mod drawers;
#[path = "workspace_header.rs"]
mod header;
#[path = "workspace_manager.rs"]
mod manager;
#[path = "workspace_tab_drag.rs"]
mod tab_drag;
#[path = "tool_catalog.rs"]
mod tool_catalog;
#[path = "workspace_update.rs"]
mod workspace_update;
use tab_drag::NativeTabSlide;

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
            let drawers = self
                .owner
                .borrow()
                .upgrade()
                .map(|w| {
                    w.drawers()
                        .into_iter()
                        .filter(|d| d.id != 0)
                        .filter_map(|d| d.geometry(&w).map(|p| (d.id, p)))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for (slot, child) in self.children.borrow().iter() {
                if matches!(
                    slot,
                    Slot::Drawer(0) | Slot::DrawerConnection(0) | Slot::DrawerShadow(0)
                ) {
                    continue; // Allocate parents before measuring child origins.
                }
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
                        height: self
                            .owner
                            .borrow()
                            .upgrade()
                            .map_or(HEADER_HEIGHT, |w| w.header.height()),
                    }),
                    Slot::Status => {
                        // HDR/proof status remains operable when the optional
                        // zoom/rotation HUD is hidden. Measure native buttons.
                        let mut bounds = resolved.status;
                        let height = child.measure(gtk::Orientation::Vertical, bounds.width as i32).1 as f32;
                        let extra = (height - bounds.height).max(0.);
                        bounds.y -= extra;
                        bounds.height += extra;
                        Some(bounds)
                    },
                    Slot::Drawer(id) | Slot::DrawerShadow(id) => {
                        drawers.iter().find(|(i, _)| i == id).map(|(_, d)| d.bounds)
                    }
                    Slot::DrawerConnection(id) => drawers
                        .iter()
                        .find(|(i, _)| i == id)
                        .and_then(|(_, d)| d.connection().map(|c| c.bounds)),
                    Slot::ColumnConnection(id, panel) => resolved.collapsed.iter()
                        .find(|c| c.id == *id).and_then(|c| c.open.as_ref())
                        .and_then(|o| o.connections.iter().find(|(p, _)| p == panel))
                        .map(|(_, connection)| connection.bounds),
                    Slot::Column(id) => resolved
                        .collapsed
                        .iter()
                        .find(|c| c.id == *id)
                        .map(|c| c.bounds),
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
                owner.allocate_workspace_motion();
                owner.measure_drawer_tiles();
                let placement = owner.drawer.geometry(&owner);
                for (slot, child) in self.children.borrow().iter() {
                    let bounds = match slot {
                        Slot::Drawer(0) | Slot::DrawerShadow(0) => {
                            placement.as_ref().map(|p| p.bounds)
                        }
                        Slot::DrawerConnection(0) => placement
                            .as_ref()
                            .and_then(|p| p.connection().map(|c| c.bounds)),
                        _ => continue,
                    };
                    child.set_child_visible(bounds.is_some());
                    if let Some(b) = bounds {
                        allocate_at(child, b);
                    }
                }
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
            let owner = self.owner.borrow().upgrade();
            let mut nodes = Vec::new();
            let mut holes = Vec::new();
            for (order, (slot, child)) in self.children.borrow().iter().enumerate() {
                let native = gtk::Snapshot::new();
                if child.has_css_class("expanded-panel") {
                    native.push_shadow(&[gtk::gsk::Shadow::new(
                        gdk::RGBA::new(0., 0., 0., 0.4),
                        0.,
                        8.,
                        24.,
                    )]);
                }
                self.obj().snapshot_child(child, &native);
                if child.has_css_class("expanded-panel") {
                    native.pop();
                }
                let node = native.to_node();
                if let Some(owner) = &owner {
                    holes.extend(
                        owner
                            .navigator_overviews
                            .project_child(self.obj().upcast_ref(), child, order, node.as_ref())
                            .into_iter()
                            .map(|hole| (order, hole)),
                    );
                }
                nodes.push((*slot, node));
            }
            for (order, (slot, node)) in nodes.iter().enumerate() {
                let Some(node) = node else {
                    continue;
                };
                // The canvas already contains the GPU overviews. In explicit
                // screenshots it is represented by a native texture: don't cut it.
                if matches!(slot, Slot::Canvas) {
                    snapshot.append_node(node);
                } else {
                    crate::navigator::Overviews::append_clipped(
                        snapshot,
                        node,
                        holes
                            .iter()
                            .filter(|(above, _)| *above >= order)
                            .map(|(_, r)| *r),
                    );
                }
            }
            if let Some(owner) = self.owner.borrow().upgrade()
                && let Some(hint) = owner.drop_hint.borrow().as_ref()
            {
                let b = hint.bounds;
                let body = matches!(hint.target, DockTarget::Tab { .. })
                    && b.width > 3. && b.height > 3.;
                let rect = gtk::graphene::Rect::new(b.x, b.y, b.width, b.height);
                snapshot.append_color(
                    &gdk::RGBA::new(0.38, 0.68, 1.0, if body { 0.25 } else { 0.95 }),
                    &rect,
                );
                if body {
                    snapshot.append_border(
                        &gtk::gsk::RoundedRect::from_rect(rect, 0.),
                        &[2.; 4],
                        &[gdk::RGBA::new(0.38, 0.68, 1.0, 0.95); 4],
                    );
                }
            }
            if let Some(owner) = &owner
                && let Some(drag) = owner.workspace_drag.borrow().as_ref()
                && let Some(tab) = &drag.tab
            {
                tab.snapshot(
                    snapshot,
                    self.obj()
                        .frame_clock()
                        .map_or(0, |clock| clock.frame_time()),
                    self.obj().scale_factor() as f32,
                );
            }
            if let Some(owner) = &owner {
                owner.header.snapshot_drag(
                    snapshot,
                    self.obj()
                        .frame_clock()
                        .map_or(0, |clock| clock.frame_time()),
                    self.obj().scale_factor() as f32,
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
    Status,
    Group(u32),
    Divider(u32),
    FloatingResize(u32, ResizeEdge),
    Drawer(u32),
    DrawerShadow(u32),
    DrawerConnection(u32),
    Column(u32),
    ColumnConnection(u32, Panel),
}
glib::wrapper! {
    pub struct DockSurface(ObjectSubclass<allocation::DockSurface>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl DockSurface {
    fn raise_drawer(&self, id: u32) {
        let source = self.imp().owner.borrow().upgrade().and_then(|w| {
            w.drawers()
                .into_iter()
                .find(|d| d.id == id)
                .and_then(|d| d.source_container(&w))
        });
        let mut children = self.imp().children.borrow_mut();
        // Paint each shadow beneath its source container. A translucent active
        // tile then composites once on clean chrome, including nested drawers.
        if let Some(source) = source
            && let Some(index) = children
                .iter()
                .position(|(s, _)| *s == Slot::DrawerShadow(id))
            && children.get(index + 1).is_none_or(|(_, w)| *w != source)
        {
            let shadow = children.remove(index);
            if let Some(index) = children.iter().position(|(_, w)| *w == source) {
                shadow.1.insert_before(self, Some(&source));
                children.insert(index, shadow);
            } else {
                children.insert(index, shadow);
            }
        }
        if children
            .iter()
            .rev()
            .take(2)
            .map(|(slot, _)| *slot)
            .eq([Slot::DrawerConnection(id), Slot::Drawer(id)])
        {
            return;
        }
        for slot in [Slot::Drawer(id), Slot::DrawerConnection(id)] {
            if let Some(index) = children.iter().position(|(s, _)| *s == slot)
                && index + 1 != children.len()
            {
                let item = children.remove(index);
                item.1.insert_after(self, children.last().map(|(_, w)| w));
                children.push(item);
            }
        }
    }
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
                Slot::Canvas
                    | Slot::Header
                    | Slot::Status
                    | Slot::Drawer(_)
                    | Slot::DrawerShadow(_)
                    | Slot::DrawerConnection(_)
                    | Slot::Divider(_)
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

#[derive(Clone, Copy, PartialEq)]
enum DragTarget {
    Header(HeaderDragSource),
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
    ) -> Option<UiAction> {
        Some(match self {
            Self::Header(_) => return None,
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
        })
    }
}

#[derive(Clone)]
struct NativeWorkspaceDrag {
    target: DragTarget,
    origin: [f32; 2],
    point: [f32; 2],
    started: bool,
    held: bool,
    context: bool,
    source: gtk::Widget,
    parent: Option<gtk::Widget>,
    wait_for_hold: bool,
    sequence: Option<gdk::EventSequence>,
    device: Option<gdk::Device>,
    cursor: Option<(gtk::Widget, Option<gdk::Cursor>)>,
    tab: Option<NativeTabSlide>,
    tab_grab: Option<NativeTabSlide>,
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
pub(crate) fn native_accelerator(chord: &KeyChord) -> String {
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
                if !tab.has_css_class("selected-tool") || tab.opacity() == 0. {
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
    pub(crate) proof: Rc<crate::proof_view::ProofView>,
    pub(crate) proof_panel: Rc<crate::files::proof::ProofPanel>,
    pub(crate) hdr_status: gtk::Button,
    pub(crate) recovery: Rc<crate::recovery::Recovery>,
    pub input: Rc<crate::input::Input>,
    pub(crate) tooltips: Rc<crate::tooltips::PenTooltips>,
    surface: DockSurface,
    palette_css: gtk::CssProvider,
    palette: Cell<Option<ThemePalette>>,
    header: header::Header,
    system_status: Rc<crate::system_status::SystemStatus>,
    popovers: RefCell<Vec<glib::WeakRef<gtk::Popover>>>,
    chrome_held: Cell<bool>,
    dragging: Cell<bool>,
    drag_targets: RefCell<Vec<(glib::WeakRef<gtk::Widget>, DragTarget)>>,
    workspace_drag: RefCell<Option<NativeWorkspaceDrag>>,
    publication: workspace_update::Publication,
    measuring_panels: Cell<bool>,
    drop_hint: RefCell<Option<DropHint>>,
    toolbar: TileStrip,
    panels: Vec<(Panel, gtk::Widget)>,
    groups: RefCell<Vec<GroupView>>,
    commands: RefCell<Vec<(CommandId, gtk::Button)>>,
    tool_set: crate::tool_panels::ToolSet,
    size_buttons: RefCell<Vec<(f32, gtk::Button)>>,
    size_number: crate::number_control::NumberControl,
    opacity: crate::number_control::NumberControl,
    color: Rc<crate::color_editor::ColorButton>,
    pub(crate) color_editors: RefCell<Vec<std::rc::Weak<crate::color_editor::Form>>>,
    tool_settings: crate::tool_panels::ToolSettings,
    placement_actions: crate::tool_panels::PlacementActions,
    color_panel: crate::tool_panels::ColorPanel,
    navigator: crate::navigator::Navigator,
    navigator_overviews: Rc<crate::navigator::Overviews>,
    pub(crate) layer_panel: crate::layers::LayerPanel,
    pub(crate) effects: Rc<crate::effects::EffectPanels>,
    tab: gtk::Label,
    view_info: gtk::Label,
    status: gtk::Label,
    restart_canvas: gtk::Button,
    pub(crate) preferences: crate::preferences::Preferences,
    pub(crate) workspaces: manager::NativeWorkspaces,
    pub(crate) servicing: Cell<bool>,
    pub(crate) histogram: RefCell<Option<Rc<crate::histogram::Inspector>>>,
    pub(crate) open_document: RefCell<Option<crate::files::OpenDocument>>,
    pub(crate) image_drop: RefCell<Option<crate::files::drop::Incoming>>,
    pub(crate) image_drop_label: gtk::Label,
    initial_project: RefCell<Option<(layer_core::Project, Option<DocumentLocation>)>>,
    customization: customization::Customization,
    pub(crate) drawer: Rc<drawers::Drawer>,
    columns: columns::Columns,
    refreshing: Cell<bool>,
    frame_timer: RefCell<Option<crate::canvas::FrameTimer>>,
    frame_deadline: Cell<u64>,
    navigation_input: Cell<u64>,
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
        Self::with_project(app, None)
    }
    pub(crate) fn with_project(
        app: &adw::Application,
        project: Option<(layer_core::Project, Option<DocumentLocation>)>,
    ) -> Rc<Self> {
        static ICONS: std::sync::Once = std::sync::Once::new();
        ICONS.call_once(|| {
            crate::icons::register();
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
        window.add_css_class("capy-workspace");
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
        // Dragged panels retain their size and may extend beyond any edge.
        // Clip at the application surface even in a decorated, windowed app.
        surface.set_overflow(gtk::Overflow::Hidden);
        let tab = gtk::Label::new(Some(APP_NAME));
        tab.add_css_class("document-title");
        tab.set_ellipsize(gtk::pango::EllipsizeMode::End);
        tab.set_width_chars(1);
        let header = header::Header::new();
        let system_status = crate::system_status::SystemStatus::new();
        let view_info = gtk::Label::new(Some("100% · 0°"));
        let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let proof = crate::proof_view::ProofView::new();
        let proof_panel = crate::files::proof::ProofPanel::new();
        status_bar.append(&proof.label);
        let hdr_status=gtk::Button::builder().visible(false).build();
        hdr_status.add_css_class("flat");
        hdr_status.set_widget_name("hdr-view-status");
        hdr_status.add_css_class("status-bubble");
        status_bar.append(&hdr_status);
        status_bar.add_css_class("workspace-status");
        view_info.add_css_class("status-bubble");
        view_info.set_hexpand(true);
        view_info.set_halign(gtk::Align::End);
        view_info.set_valign(gtk::Align::End);
        status_bar.append(&view_info);
        surface.add(Slot::Canvas, &area);
        surface.add(Slot::Header, &header.root);
        surface.add(Slot::Status, &status_bar);
        let toolbar = TileStrip::new();
        toolbar.add_css_class("toolbar-controls");
        let brushes = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let tool_set = crate::tool_panels::ToolSet::new();
        let tool_settings = crate::tool_panels::ToolSettings::new();
        let color_panel = crate::tool_panels::ColorPanel::new();
        let navigator_overviews = Rc::new(crate::navigator::Overviews::default());
        let navigator = crate::navigator::Navigator::new(&navigator_overviews);
        let sizes = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let layer_panel = crate::layers::LayerPanel::new();
        let effects = Rc::new(crate::effects::EffectPanels::new());
        let size_number = crate::number_control::NumberControl::new(
            NumericControl::brush_size(),
            "Brush size",
            "",
        );
        size_number.set_widget_name("brush-size");
        let opacity =
            crate::number_control::NumberControl::new(NumericControl::percent(), "Opacity", "");
        opacity.set_width_request(100);
        let color = crate::color_editor::ColorButton::new();
        let status = gtk::Label::new(None);
        status.set_visible(false);
        status.add_css_class("error");
        status.add_css_class("workspace-notice");
        status.set_wrap(true);
        let restart_canvas = gtk::Button::with_label("Restart canvas");
        restart_canvas.set_widget_name("restart-canvas");
        restart_canvas.set_halign(gtk::Align::Center);
        restart_canvas.set_visible(false);
        let content = gtk::Overlay::new();
        let workspaces = manager::NativeWorkspaces::new();
        workspaces.root.add_css_class("workspace-notice");
        content.set_child(Some(&surface));
        // Notices must not resize the full-window canvas, change its viewport,
        // or recreate the GPU swapchain while opening/saving a workspace.
        let notices = gtk::Box::new(gtk::Orientation::Vertical, 0);
        notices.set_valign(gtk::Align::End);
        notices.append(&status);
        notices.append(&restart_canvas);
        notices.append(&workspaces.root);
        content.add_overlay(&notices);
        let image_drop_label = gtk::Label::new(Some("Add image as layer"));
        image_drop_label.set_halign(gtk::Align::Center);
        image_drop_label.set_valign(gtk::Align::Center);
        image_drop_label.add_css_class("card");
        image_drop_label.set_can_target(false);
        image_drop_label.set_visible(false);
        content.add_overlay(&image_drop_label);
        let placement_actions = crate::tool_panels::PlacementActions::new();
        content.add_overlay(&placement_actions.root);
        window.set_content(Some(&content));
        let this = Rc::new(Self {
            window,
            area,
            gpu: RefCell::new(None),
            image_drop: RefCell::new(None),
            image_drop_label,
            proof,
            hdr_status,
            recovery: Rc::new(crate::recovery::Recovery::default()),
            surface,
            palette_css,
            palette: Cell::new(None),
            header,
            system_status,
            popovers: RefCell::new(Vec::new()),
            chrome_held: Cell::new(false),
            dragging: Cell::new(false),
            drag_targets: RefCell::new(Vec::new()),
            workspace_drag: RefCell::new(None),
            publication: workspace_update::Publication::default(),
            drop_hint: RefCell::new(None),
            measuring_panels: Cell::new(false),
            toolbar: toolbar.clone(),
            groups: RefCell::new(Vec::new()),
            panels: vec![
                (Panel::Toolbar, toolbar.clone().upcast()),
                (Panel::Brushes, scroll(&brushes)),
                (Panel::ToolSettings, scroll(&tool_settings.root)),
                (Panel::Color, scroll(&color_panel.root)),
                (Panel::Sizes, scroll(&sizes)),
                (Panel::Layers, layer_panel.root.clone().upcast()),
                (Panel::Adjustments, effects.adjustments.clone().upcast()),
                (Panel::Properties, scroll(&effects.properties)),
                (Panel::Stats, scroll(&effects.stats)),
                (Panel::Navigator, navigator.root.clone().upcast()),
                (Panel::Proof, proof_panel.root.clone().upcast()),
            ],
            commands: RefCell::new(Vec::new()),
            tool_set,
            size_buttons: RefCell::new(Vec::new()),
            size_number,
            opacity,
            color,
            color_editors: RefCell::default(),
            tool_settings,
            placement_actions,
            color_panel,
            proof_panel,
            navigator,
            navigator_overviews,
            layer_panel,
            effects,
            tab,
            view_info,
            status,
            restart_canvas,
            preferences: crate::preferences::Preferences::new(),
            workspaces,
            servicing: Cell::new(false),
            histogram: RefCell::new(None),
            open_document: RefCell::new(None),
            initial_project: RefCell::new(project),
            customization: customization::Customization::new(),
            drawer: drawers::Drawer::new(0),
            columns: columns::Columns::default(),
            refreshing: Cell::new(false),
            frame_timer: RefCell::new(None),
            frame_deadline: Cell::new(0),
            navigation_input: Cell::new(0),
            input: Rc::default(),
            tooltips: Rc::default(),
        });
        this.hdr_status.connect_clicked(glib::clone!(#[weak] this, move |_| crate::hdr::display_details(&this)));
        this.apply_palette(Settings::default().palette(
            if adw::StyleManager::default().is_dark() {
                Theme::Dark
            } else {
                Theme::Light
            },
            Platform::Gtk,
        ));
        *this.surface.imp().owner.borrow_mut() = Rc::downgrade(&this);
        this.build_controls(&brushes, &sizes);
        this.placement_actions.bind(&this);
        this.color_panel.bind(&this);
        this.navigator.bind(&this);
        this.navigator_overviews.bind(&this);
        this.customization.bind(&this);
        this.preferences.bind(&this);
        this.workspaces.bind(&this);
        this.install_chrome();
        this.tooltips.install(&this.window);
        crate::input::install(&this);
        crate::display_color::bind_monitor_updates(&this);
        this.install_gpu();
        this.restart_canvas.connect_clicked(glib::clone!(
            #[weak]
            this,
            move |_| this.restart_gpu()
        ));
        this.install_document_close();
        crate::recovery::install(&this);
        this.reconcile_layout(&DockLayout::default());
        this.install_drop_target();
        this
    }

    fn build_controls(self: &Rc<Self>, brushes: &gtk::Box, sizes: &gtk::Box) {
        self.customization.track(
            Panel::Navigator,
            PanelControl::Navigator,
            &self.navigator.root,
        );
        margins(brushes, layer_ui::PANEL_CONTENT_INSET as i32);
        let brush_list = &self.tool_set.root;
        brushes.append(brush_list);
        self.customization
            .track(Panel::Brushes, PanelControl::Brushes, brush_list);
        self.customization.track(
            Panel::ToolSettings,
            PanelControl::ToolSettings,
            &self.tool_settings.root,
        );
        self.customization.track(
            Panel::Color,
            PanelControl::ColorWheel,
            &self.color_panel.root,
        );
        self.append_panel_fields(Panel::Brushes, brushes);
        margins(sizes, 8);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        self.size_number.set_hexpand(true);
        row.append(&self.size_number);
        sizes.append(&row);
        self.customization
            .track(Panel::Sizes, PanelControl::BrushSize, &row);
        let (grid, buttons) = crate::tool_panels::size_grid(self);
        *self.size_buttons.borrow_mut() = buttons;
        sizes.append(&grid);
        self.customization
            .track(Panel::Sizes, PanelControl::SizePresets, &grid);
        self.append_panel_fields(Panel::Sizes, sizes);
        self.layer_panel.bind(self);
        self.customization.track(
            Panel::Adjustments,
            PanelControl::Adjustments,
            &self.effects.adjustments,
        );
        self.customization.track(
            Panel::Properties,
            PanelControl::Properties,
            &self.effects.properties,
        );
        self.customization
            .track(Panel::Stats, PanelControl::Stats, &self.effects.stats);
        self.customization.track(
            Panel::Layers,
            PanelControl::LayerActions,
            &self.layer_panel.footer,
        );
        self.customization
            .track(Panel::Layers, PanelControl::Layers, &self.layer_panel.list);
        self.customization.track(
            Panel::Layers,
            PanelControl::LayerOpacity,
            &self.layer_panel.header,
        );
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
        self.color.bind(self, |workspace, color| workspace.dispatch(UiAction::Color {
            action: layer_ui::ColorAction::Definition { color },
        }));
        let keys = gtk::EventControllerKey::new();
        keys.set_name(Some("workspace-shortcuts"));
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                this.update_zen();
                // Native sheets own their keys. The main Preferences dialog
                // retains the shared type-to-search behavior; its focused text
                // inputs and nested sheets still own ordinary typing.
                if this.window.visible_dialog().is_some_and(|d| d != this.preferences.dialog)
                    || this.preferences.recording()
                    || this.header.is_editing()
                {
                    return glib::Propagation::Proceed;
                }
                // Space also pans the canvas, but focused color buttons own
                // native Space / Enter activation, including in retained drawers.
                if matches!(key, gdk::Key::space | gdk::Key::Return | gdk::Key::KP_Enter)
                    && gtk::prelude::GtkWindowExt::focus(&this.window).is_some_and(|w| {
                        w.is::<gtk::Button>()
                            && w.ancestor(crate::tool_panels::ColorWheel::static_type())
                                .is_some()
                    })
                {
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
                if this.window.visible_dialog().is_some_and(|d| d != this.preferences.dialog) {
                    return;
                }
                // Release the shortcut that opened the editor, even though its
                // key presses now belong to native controls. Otherwise reopening
                // with the same shortcut is mistaken for an already-held key.
                this.interact(crate::input::key_input(key, false, modifiers, false, None));
            }
        ));
        self.window.add_controller(keys);
    }

    pub fn action_button(self: &Rc<Self>, label: &str, action: UiAction) -> gtk::Button {
        let button = gtk::Button::with_label(label);
        self.bind_action_tooltip(&button, action.clone());
        button.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.dispatch(action.clone())
        ));
        button
    }
    pub fn bind_action_tooltip(self: &Rc<Self>, button: &gtk::Button, action: UiAction) {
        self.bind_dynamic_action_tooltip(button, move |_| Some(action.clone()));
    }
    pub fn bind_dynamic_action_tooltip(
        self: &Rc<Self>,
        button: &impl IsA<gtk::Button>,
        action: impl Fn(&layer_ui::UiState) -> Option<UiAction> + 'static,
    ) {
        let button = button.as_ref();
        self.tooltips.bind(
            button,
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                #[upgrade_or]
                None,
                move |widget| {
                    let button = widget.downcast_ref::<gtk::Button>()?;
                    let label = button
                        .tooltip_text()
                        .or_else(|| button.label())
                        .unwrap_or_default();
                    let gpu = this.gpu.borrow();
                    let g = gpu.as_ref()?;
                    let state = g.session.state();
                    let action = action(state)?;
                    Some(
                        state
                            .settings
                            .action_tooltip(&label, &action, state.platform),
                    )
                }
            ),
        );
    }
    fn install_chrome(self: &Rc<Self>) {
        self.window.connect_fullscreened_notify(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |window| {
                this.fullscreen_changed(window.is_fullscreen());
            }
        ));
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
        self.header.bind(self);
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
            move |controller, event| {
                let point = this.event_point(controller);
                if matches!(
                    event.event_type(),
                    gdk::EventType::ButtonPress | gdk::EventType::TouchBegin
                ) && this.window.visible_dialog().is_none()
                    && let Some(position) = point
                    // Canvas input has its own reveal/dismiss boundary before
                    // any pen samples are queued. Do not process it twice.
                    && !this.surface.pick(position[0] as f64, position[1] as f64, gtk::PickFlags::DEFAULT)
                        .is_some_and(|picked| picked == this.area)
                    && this
                        .chrome_event(ChromeEvent::Contact {
                            position,
                            canvas: false,
                        })
                        .handled
                {
                    return glib::Propagation::Stop;
                }
                let was_held = this.chrome_held.get();
                if event.event_type() == gdk::EventType::ButtonPress
                    && point.is_some_and(|[_, y]| y < this.header.height())
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

    fn chrome_menu(self: &Rc<Self>, id: ApplicationMenu) -> gtk::MenuButton {
        let menu = gtk::MenuButton::builder()
            .label(id.label())
            .tooltip_text(id.label())
            .build();
        menu.add_css_class("flat");
        menu.add_css_class("chrome-control");
        menu.set_direction(gtk::ArrowType::None);
        let root = gtk::gio::Menu::new();
        let popover = gtk::PopoverMenu::from_model(Some(&root));
        if id == ApplicationMenu::Window {
            popover.set_widget_name("workspace-menu");
        }
        popover.connect_show(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |popup| {
                let model = w
                    .gpu
                    .borrow()
                    .as_ref()
                    .map(|g| g.session.application_menu(id));
                if let Some(model) = model {
                    w.populate_workspace_menu(popup, model);
                }
            }
        ));
        self.watch_popover(popover.upcast_ref());
        menu.set_popover(Some(&popover));
        menu
    }

    pub(crate) fn watch_popover(self: &Rc<Self>, popover: &gtk::Popover) {
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
            content_drawer: self.drawer.placement().map(|p| p.bounds),
            drawer_connection: self
                .drawer
                .placement()
                .and_then(|p| p.connection().map(|c| c.bounds)),
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
        let finishing = matches!(
            &input,
            UiInput::Blur
                | UiInput::Pointer {
                    phase: ContactPhase::Up | ContactPhase::Cancel,
                    ..
                }
        ) || matches!(&input,UiInput::Key {key,..} if key == "Escape" || key == "Enter");
        if !finishing && !self.workspaces.accepts_input(self) {
            return InputReply {
                handled: true,
                ..Default::default()
            };
        }
        if matches!(&input, UiInput::Key { key, pressed: true, .. } if key == "Escape") {
            let drag = self.workspace_drag.borrow().clone();
            if let Some(drag) = drag {
                self.workspace_drag_input(ContactPhase::Cancel, drag.point, drag.sequence);
            }
        }
        if matches!(input, UiInput::Blur) {
            if let Some(mut drag) = self.workspace_drag.borrow_mut().take() {
                self.reset_drag_recognizers(&drag);
                if drag.context {
                    self.dismiss_context();
                }
                if matches!(
                    drag.target,
                    DragTarget::Dock(DockItem::Tile { .. }) | DragTarget::Header(_)
                ) {
                    self.dragging.set(false);
                }
                self.header.clear_drop();
                self.clear_tab_slide(&mut drag);
                self.header.cancel_drag(self);
                self.restore_drag_cursor(&drag);
            }
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
                let hidden = hidden && !widget.has_css_class("floating-panel");
                let can_target = !hidden && !widget.has_css_class("fixed-stack-divider")
                    && !matches!(slot, Slot::DrawerShadow(_) | Slot::ColumnConnection(_, _));
                if widget.has_css_class("zen-hidden") == hidden && widget.can_target() == can_target
                {
                    continue;
                }
                if hidden {
                    widget.add_css_class("zen-hidden");
                } else {
                    widget.remove_css_class("zen-hidden");
                }
                widget.set_can_target(can_target);
            }
        }
    }
    fn fullscreen_changed(self: &Rc<Self>, fullscreen: bool) {
        let show_clock = self
            .gpu
            .borrow()
            .as_ref()
            .map(|g| g.session.state().settings.show_clock)
            .unwrap_or_default();
        self.system_status.set_visibility(fullscreen, show_clock);
        self.dispatch(UiAction::WindowFullscreen { fullscreen });
    }
    pub(crate) fn view_color(&self) -> crate::display_color::ViewColor {
        self.gpu.borrow().as_ref().map_or(Default::default(), |g| {
            let document = g.session.engine().document();
            g.session.engine().backend().view_color.with_rendition(document.color, g.session.effective_sdr_rendition())
        })
    }
    pub(crate) fn snapshot_gpu(&self) -> Result<layer_render_wgpu::snapshot::SnapshotGpu, String> {
        self.gpu.borrow().as_ref().ok_or("Canvas unavailable")?.session.engine().backend().snapshot_gpu()
    }
    pub(crate) fn picker_headroom(&self) -> f32 {
        if !self.window.renderer().is_some_and(|r| matches!(r.type_().name(), "GskVulkanRenderer" | "GskGLRenderer")) { return 1.; }
        self.gpu.borrow().as_ref().map_or(1., |g| g.session.engine().backend().display_headroom)
    }
    pub(crate) fn display_description(&self) -> String {
        let encoding = self.gpu.borrow().as_ref().and_then(|g| g.session.engine().backend().display_encoding);
        let mut description = match encoding {
            Some(layer_render_wgpu::SdrSurfaceColor::Bt2100Pq) => "Managed BT.2020 PQ canvas. The compositor maps its color and brightness to each monitor; the HDR picker and paint previews use the same display headroom. The hue guide stays an SDR reference.".to_string(),
            Some(_) => "Managed linear scRGB canvas. The compositor maps its color and brightness to each monitor; the HDR picker and paint previews use the same display headroom. The hue guide stays an SDR reference.".to_string(),
            None => self.view_color().description().to_string(),
        };
        if let Some(monitor) = self.window.surface().and_then(|s| s.display().monitor_at_surface(&s)) {
            if let Some(name) = monitor.description().or_else(|| monitor.model()).or_else(|| monitor.connector()) {
                description.push_str(&format!(" Monitor: {name}."));
            }
        }
        description
    }
    pub fn dispatch(self: &Rc<Self>, action: UiAction) {
        if self.refreshing.get() {
            return;
        }
        // Display-wide settings arrive from the host, including while another
        // window owns this workspace or its layout is still loading.
        if !matches!(
            &action,
            UiAction::WorkspaceManager { .. }
                | UiAction::MeasureColumnDrawers { .. }
                | UiAction::MeasureDrawerTiles { .. }
                | UiAction::Invoke {
                    command: CommandId::ApplyTransform | CommandId::CancelTransform
                }
                | UiAction::DragWorkspace {
                    phase: ContactPhase::Up | ContactPhase::Cancel,
                    ..
                }
                | UiAction::DragDivider {
                    phase: ContactPhase::Up | ContactPhase::Cancel,
                    ..
                }
                | UiAction::ResizeFloating {
                    phase: ContactPhase::Up | ContactPhase::Cancel,
                    ..
                }
                | UiAction::MeasureColumnScroll { .. }
                | UiAction::MeasurePanels { .. }
                | UiAction::MeasureTitlebar { .. }
                | UiAction::MeasureHeader { .. }
                | UiAction::SystemThemeChanged { .. }
                | UiAction::RestoreSettings { .. }
                | UiAction::WindowFullscreen { .. }
        ) && !self.workspaces.accepts_input(self)
        {
            return;
        }
        if !self.workspaces.ready.get()
            && self.workspaces.busy.get()
            && !matches!(
                &action,
                UiAction::MeasureColumnDrawers { .. }
                    | UiAction::MeasureDrawerTiles { .. }
                    | UiAction::MeasureColumnScroll { .. }
                    | UiAction::MeasurePanels { .. }
                    | UiAction::MeasureTitlebar { .. }
                    | UiAction::SystemThemeChanged { .. }
                    | UiAction::RestoreSettings { .. }
                    | UiAction::WindowFullscreen { .. }
            )
        {
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
            Ok(mut change) => {
                if let Some(g) = self.gpu.borrow_mut().as_mut() {
                    let hdr = g.session.engine().document().color.depth.is_float();
                    let rendition = hdr.then(|| g.session.effective_sdr_rendition());
                    let proof = g.session.state().soft_proof || g.session.state().gamut_warning;
                    self.hdr_status.set_visible(hdr && !proof);
                    let preview = g.session.state().preview_sdr || g.session.state().sdr_appearance_preview.is_some();
                    let headroom = g.session.renderer_mut().display_headroom;
                    if g.session.set_hdr_display_available(headroom > 1.) { change.regions |= regions::COMMANDS | regions::BRUSH; }
                    self.hdr_status.set_label(if preview && headroom > 1. { "SDR preview" } else if headroom > 1. { "HDR" } else { "Showing SDR" });
                    self.hdr_status.set_tooltip_text(Some("Display details"));
                    if let Err(e) = g.session.renderer_mut().set_hdr_view(rendition, preview) { eprintln!("HDR viewing: {e}"); }

                }
                if self.color_panel.headroom() != self.picker_headroom() { change.regions |= regions::BRUSH; }
                self.proof.sync(self);
                let publication = self
                    .gpu
                    .borrow()
                    .as_ref()
                    .map(|g| g.session.workspace_update());
                if let Some(update) = &publication
                    && self.publication.model_revision.get() == Some(update.model_revision)
                    && change.regions == (regions::LAYOUT | regions::CUSTOMIZATION)
                {
                    self.publish_workspace(update.clone());
                    if change.canvas_wake {
                        self.wake();
                    }
                    return;
                }
                if let Some(update) = &publication
                    && self.publication.content_revision.get() == Some(update.content_revision)
                    && change.regions == (regions::LAYOUT | regions::CUSTOMIZATION)
                {
                    self.publish_workspace_layout(update.clone());
                    if change.canvas_wake {
                        self.wake();
                    }
                    return;
                }
                self.refresh_cursor();
                if self.status.is_visible()
                    && self.gpu.borrow().as_ref().is_none_or(|g| {
                        !g.session.rendering_suspended() && g.session.state().host_error.is_none()
                    })
                {
                    self.status.set_visible(false);
                }
                if change.regions != 0 {
                    let pending_layout = self.reset_workspace_publication();
                    let moving = publication.as_ref().is_some_and(|u| u.drag.is_some());
                    self.refresh(
                        change.regions
                            | if moving || pending_layout {
                                regions::LAYOUT
                            } else {
                                0
                            },
                    );
                    self.workspaces.observe(self, change.regions);
                }
                if let Some(update) = publication.filter(|_| change.regions != 0) {
                    self.publish_workspace(update);
                }
                if change.canvas_wake {
                    let navigation = change.regions == regions::CAMERA;
                    if navigation {
                        self.navigation_input.set(glib::monotonic_time().max(0) as u64 * 1000);
                    }
                    self.wake_frame(false, navigation);
                }
                if change.regions & regions::HOST != 0 {
                    if !self
                        .gpu
                        .borrow()
                        .as_ref()
                        .is_some_and(|g| g.session.rendering_suspended())
                    {
                        self.status.set_visible(false);
                    }
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
                    self.service_requests();
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
        self.wake_frame(false, false);
    }
    pub(crate) fn wake_stroke_end(self: &Rc<Self>) {
        self.wake_frame(true, false);
    }
    fn wake_frame(self: &Rc<Self>, immediate: bool, navigation: bool) {
        if self
            .gpu
            .borrow()
            .as_ref()
            .is_some_and(|g| g.session.rendering_suspended())
        {
            return;
        }
        let now = glib::monotonic_time().max(0) as u64 * 1000;
        let (navigation, period, deadline) = self.gpu.borrow().as_ref().map(|g| {
            let engine = g.session.engine();
            let clock = &engine.backend().clock;
            let navigation = navigation && engine.backend().startup.complete
                && !engine.has_active_stroke() && !engine.has_pending_document_edits()
                && engine.transform_preview().is_none();
            (navigation, clock.period(), if navigation {
                clock.navigation_start(self.navigation_input.get(), now)
            } else {
                clock.deadline(now).unwrap_or(self.frame_deadline.get())
            })
        }).unwrap_or((false, crate::canvas::FRAME_NS, self.frame_deadline.get()));
        // Keep one timer through the input burst. Rearming for corrected display
        // feedback must retain the real input anchor, not invent another input.
        if !navigation {
            self.navigation_input.set(0);
        }
        if let Some(timer) = self.frame_timer.borrow().as_ref() {
            if immediate {
                self.frame_deadline.set(timer.expedite());
            }
            return;
        }
        let (first, timer) = crate::canvas::schedule(
            deadline,
            period,
            immediate,
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
                        this.frame_timer.borrow_mut().take();
                        return glib::ControlFlow::Break;
                    }
                    // The first wake can precede initial allocation.
                    // Fit the document only once the real canvas extent exists.
                    if area.width() <= 1 || area.height() <= 1 {
                        return glib::ControlFlow::Continue;
                    }
                    let now = glib::monotonic_time().max(0) as u64 * 1000;
                    let expedited = this.frame_timer.borrow().as_ref()
                        .is_some_and(|timer| timer.take_expedited());
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
                    let next = if previous == 0 {
                        now + period
                    } else {
                        previous + ((now.saturating_sub(previous) / period) + 1) * period
                    };
                    this.frame_deadline.set(next);
                    this.input.flush(&this);
                    let result = this.gpu.borrow_mut().as_mut().map(|g| {
                        let overviews = this
                            .navigator_overviews
                            .placements(g.session.state(), area.scale_factor() as f32);
                        g.session.renderer_mut().overviews = overviews;
                        g.render(area, now)
                    });
                    match result {
                        Some(Ok(change)) => this.changed(Ok(change)),
                        Some(Err(error)) => {
                            this.gpu_error(&error);
                            this.frame_timer.borrow_mut().take();
                            return glib::ControlFlow::Break;
                        }
                        None => {}
                    }
                    let last_navigation = this.navigation_input.get();
                    let navigating = last_navigation != 0
                        && now.saturating_sub(last_navigation) < period * 2;
                    let active = navigating || this.input.has_pending()
                        || this.gpu.borrow().as_ref().is_some_and(|g| {
                            g.session.wants_continuous_frames() || g.needs_present
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
                        // An expedited wake deliberately changes timer phase.
                        // Restore it even when the change is inside the normal
                        // feedback-jitter tolerance; otherwise each pen-up can
                        // leave subsequent movement slightly early or late.
                        if expedited || this.gpu.borrow().as_ref().is_some_and(|g| {
                            let clock = &g.session.engine().backend().clock;
                            if navigating {
                                !clock.navigation_aligned(next, period)
                            } else {
                                !clock.aligned(next, period)
                            }
                        }) {
                            this.frame_timer.borrow_mut().take();
                            this.wake_frame(false, navigating && !expedited);
                            return glib::ControlFlow::Break;
                        }
                        glib::ControlFlow::Continue
                    } else {
                        this.frame_timer.borrow_mut().take();
                        glib::ControlFlow::Break
                    }
                }
            ),
        );
        // schedule may advance an expired deadline after a genuinely idle gap.
        // Keep our next deadline aligned with the actual kernel timer phase.
        self.frame_deadline.set(first);
        *self.frame_timer.borrow_mut() = Some(timer);
    }
    fn install_gpu(self: &Rc<Self>) {
        let actions = gtk::gio::SimpleActionGroup::new();
        let stopped = gtk::gio::SimpleAction::new("worker-stopped", None);
        stopped.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| this.wake()
        ));
        actions.add_action(&stopped);
        let display = gtk::gio::SimpleAction::new("display-changed", None);
        display.connect_activate(glib::clone!(#[weak(rename_to = this)] self, move |_, _| this.wake()));
        actions.add_action(&display);
        self.area.insert_action_group("canvas", Some(&actions));
        self.area.connect_realize(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |area| {
                let reattached = this.gpu.borrow_mut().as_mut().map(|gpu| gpu.reattach(area));
                if let Some(result) = reattached {
                    if let Err(error) = result {
                        this.gpu_error(&error);
                    }
                    this.wake();
                    return;
                }
                match GpuCanvas::with_project(area, this.initial_project.borrow_mut().take()) {
                    Ok(mut gpu) => {
                        if this.recovery.recovered.get() {
                            gpu.session.mark_recovered();
                        }
                        *this.gpu.borrow_mut() = Some(gpu);
                        this.fullscreen_changed(this.window.is_fullscreen());
                        this.refresh(regions::ALL);
                        this.workspaces.start(&this);
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
                if let Some(gpu) = this.gpu.borrow_mut().as_mut() {
                    gpu.session.renderer_mut().stop();
                }
            }
        ));
    }
    fn gpu_error(self: &Rc<Self>, error: &str) {
        if self
            .gpu
            .borrow()
            .as_ref()
            .is_some_and(|g| g.session.rendering_suspended())
        {
            return;
        }
        eprintln!("Canvas failed: {error}");
        self.input.discard();
        let change = self.gpu.borrow_mut().as_mut().map(|g| {
            g.needs_present = false;
            g.session.suspend_renderer()
        });
        if let Some(change) = change {
            match change {
                Ok(change) => {
                    self.changed(Ok(change));
                    self.restart_canvas.set_visible(true);
                }
                Err(error) => {
                    eprintln!("Canvas recovery failed: {error}");
                    self.refresh(regions::ALL);
                    self.status.set_text("Canvas stopped. Automatic recovery failed. Reopen a saved drawing or recovery copy.");
                    self.status.set_visible(true);
                    return;
                }
            }
        }
        self.status.set_text("Canvas stopped. The interrupted work was canceled. Save the drawing or restart the canvas to continue.");
        self.status.set_visible(true);
    }
    fn restart_gpu(self: &Rc<Self>) {
        let result = self
            .gpu
            .borrow_mut()
            .as_mut()
            .map(|g| g.reattach(&self.area));
        match result {
            Some(Ok(())) => {
                self.restart_canvas.set_visible(false);
                self.status.set_visible(false);
                self.refresh(regions::ALL);
                self.wake();
            }
            Some(Err(error)) => {
                eprintln!("Canvas restart failed: {error}");
                self.status
                    .set_text("Canvas could not restart. Save your drawing, then reopen it.");
            }
            None => (),
        }
    }
    fn refresh(self: &Rc<Self>, regions: u32) {
        self.publication.content_revision.set(
            self.gpu
                .borrow()
                .as_ref()
                .map(|g| g.session.workspace_content_revision()),
        );
        self.publication.model_revision.set(
            self.gpu
                .borrow()
                .as_ref()
                .map(|g| g.session.workspace_model_revision()),
        );
        #[cfg(test)]
        self.publication
            .refreshes
            .set(self.publication.refreshes.get() + 1);
        let Some(state) = self
            .gpu
            .borrow()
            .as_ref()
            .map(|g| g.session.state().clone())
        else {
            return;
        };
        self.refreshing.set(true);
        if regions & (regions::DOCUMENT | regions::COMMANDS | regions::LAYOUT) != 0 {
            self.proof_panel.refresh(self, &state);
        }
        self.header.refresh(self, &state);
        self.view_info
            .set_visible(state.workspace.layout.canvas_info.visible);
        self.view_info.set_halign(gtk::Align::End);
        if regions & (regions::CAMERA | regions::LAYOUT | regions::DOCUMENT | regions::COMMANDS)
            != 0
        {
            self.navigator.refresh(&state);
        }
        if regions & (regions::BRUSH | regions::SETTINGS | regions::DOCUMENT) != 0 {
            self.tool_set.refresh(self, &state.tool_set, state.theme);
        }
        if regions & (regions::BRUSH | regions::DOCUMENT | regions::COMMANDS) != 0 {
            self.tool_settings.refresh(self, &state);
            self.placement_actions.refresh(&state);
        }
        if regions & (regions::BRUSH | regions::DOCUMENT | regions::SETTINGS | regions::COMMANDS) != 0 {
            self.color_panel.refresh(&state.colors, self.view_color(), self.picker_headroom());
            crate::color_editor::refresh_display(self);
        }
        if regions & (regions::BRUSH | regions::DOCUMENT) != 0 {
            self.size_number.set_value(state.brush.diameter as f64);
            self.opacity.set_value(state.brush.opacity as f64);
            self.color.set_display_color(state.colors.definition(), self.view_color(), self.picker_headroom());
            self.toolbar.queue_draw();
            for (value, button) in self.size_buttons.borrow().iter() {
                selected(button, *value == state.brush.diameter);
            }
        }
        if regions & regions::DOCUMENT != 0 {
            self.layer_panel.refresh(&state);
            self.effects.refresh(self, &state);
            if let Some(tab) = state.tabs.first() {
                let modified = if state.document_file.modified {
                    "• "
                } else {
                    ""
                };
                self.tab.set_text(&format!(
                    "{modified}{} · {} × {}",
                    tab.title, tab.width, tab.height
                ));
                self.window
                    .set_title(Some(&format!("{modified}{} — {APP_NAME}", tab.title)));
            }
        }
        if regions & regions::COMMANDS != 0 {
            for (id, button) in self.commands.borrow().iter() {
                if let Some(command) = state.commands.iter().find(|c| c.id == *id) {
                    button.set_sensitive(command.enabled);
                    selected(button, command.selected);
                    if let Some(icon) = command.icon
                        && let Some(image) = button.child().and_downcast::<gtk::Image>()
                    {
                        let name = format!("layer-{icon}-symbolic");
                        if crate::icons::name(&image).as_deref() != Some(&name) {
                            crate::icons::set(&image, Some(&name));
                        }
                    }
                }
            }
        }
        if regions & regions::SETTINGS != 0 {
            self.system_status
                .set_visibility(self.window.is_fullscreen(), state.settings.show_clock);
            self.apply_palette(state.palette);
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
            self.columns.refresh_drawers(self, &state, regions);
            self.drawer
                .refresh(self, &state, regions, state.customization.drawer.as_ref());
            if state.customization.drawer.is_some() {
                self.surface.raise_drawer(0);
            }
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
                w.measure_panels();
            }
        ));
    }
    // Also run synchronously at release: an idle measurement from before a
    // tear-off may describe a different width or an older layer count.
    fn measure_panels(self: &Rc<Self>) {
        let Some(layout) = self
            .gpu
            .borrow()
            .as_ref()
            .map(|g| g.session.state().workspace.layout.clone())
        else {
            return;
        };
        let groups = self.groups.borrow();
        let resolved = self.resolved();
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
                let widget = self.panel_widget(config.id);
                let content = widget
                    .downcast_ref::<gtk::ScrolledWindow>()
                    .and_then(|s| s.child())
                    .unwrap_or(widget);
                // Manual shrink can clip a panel below its natural
                // minimum. GTK still requires a valid measure request.
                let width = (width as i32).max(content.measure(gtk::Orientation::Horizontal, -1).0);
                let (content_height, scroll) = match config.id {
                    Panel::Layers => {
                        let (height, scroll) = self.layer_panel.content_measurement(width);
                        (height, Some(scroll))
                    }
                    Panel::Adjustments => {
                        let (height, scroll) = self.effects.picker_content_measurement(width);
                        (height, Some(scroll))
                    }
                    _ => (
                        content.measure(gtk::Orientation::Vertical, width).1 as f32,
                        (config.id != Panel::Color
                            && self.panel_widget(config.id).is::<gtk::ScrolledWindow>())
                        .then_some(layer_ui::PanelScrollMeasurement {
                            fixed_height: 0.0,
                            unit_height: 0.0,
                        }),
                    ),
                };
                PanelMeasurement {
                    panel: config.id,
                    tab_width,
                    content_height,
                    scroll,
                }
            })
            .collect::<Vec<_>>();
        drop(groups);
        if measurements != layout.measurements {
            self.dispatch(UiAction::MeasurePanels { measurements });
        }
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
                        let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                        content.set_halign(gtk::Align::Center);
                        content.set_valign(gtk::Align::Center);
                        content.append(&gtk::Image::new());
                        let title = gtk::Label::new(None);
                        if group.panels.len() == 1 {
                            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
                        }
                        content.append(&title);
                        tab.set_child(Some(&content));
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
            // The pressed divider owns GTK's implicit pointer grab and resize
            // cursor. Keep it parented when collapse/expansion rebuilds panels.
            self.surface.remove_slots(|slot| {
                matches!(slot, Slot::Divider(id) if !resolved.dividers.iter().any(|d| d.id == id))
            });
            for divider in &resolved.dividers {
                let existing = self
                    .surface
                    .imp()
                    .children
                    .borrow()
                    .iter()
                    .find(|(slot, _)| *slot == Slot::Divider(divider.id))
                    .map(|(_, widget)| widget.clone());
                if let Some(handle) = existing {
                    handle.set_cursor_from_name(Some(if divider.axis == Axis::Horizontal {
                        "col-resize"
                    } else {
                        "row-resize"
                    }));
                    // Raise retained handles above newly built groups without
                    // unparenting them or breaking the ongoing pointer grab.
                    let mut children = self.surface.imp().children.borrow_mut();
                    let index = children
                        .iter()
                        .position(|(slot, _)| *slot == Slot::Divider(divider.id))
                        .unwrap();
                    let item = children.remove(index);
                    handle.insert_after(&self.surface, children.last().map(|(_, widget)| widget));
                    children.push(item);
                } else {
                    self.add_divider(divider.clone());
                }
            }
        }
        for (slot, handle) in self.surface.imp().children.borrow().iter() {
            let Slot::Divider(id) = slot else { continue };
            let Some(divider) = resolved.dividers.iter().find(|d| d.id == *id) else { continue };
            let fixed = layout.fixed_stack_divider(divider);
            if fixed {
                handle.add_css_class("fixed-stack-divider");
            } else {
                handle.remove_css_class("fixed-stack-divider");
            }
            handle.set_can_target(!fixed && !handle.has_css_class("zen-hidden"));
            handle.set_focusable(!fixed);
            handle.set_tooltip_text((!fixed).then_some("Resize dock"));
            handle.set_cursor_from_name(Some(if fixed {
                "default"
            } else if divider.axis == Axis::Horizontal {
                "col-resize"
            } else {
                "row-resize"
            }));
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
                let tab = layout.tab_presentation(*panel);
                let content = button.child().unwrap();
                let icon = content.first_child().and_downcast::<gtk::Image>().unwrap();
                let label = content.last_child().and_downcast::<gtk::Label>().unwrap();
                crate::icons::set(&icon, Some(&format!("layer-{}-symbolic", config.icon())));
                icon.set_visible(tab.show_icon);
                label.set_label(config.title());
                label.set_visible(tab.show_name);
                if tab.show_name {
                    button.remove_css_class("icon-only-tab");
                } else {
                    button.add_css_class("icon-only-tab");
                }
                button.set_tooltip_text(Some(config.title()));
                button.update_property(&[gtk::accessible::Property::Label(config.title())]);
                selected(button, *panel == group.active);
            }
            view.tab_joins.queue_draw();
        }
        self.columns.reconcile(self, layout, &resolved);
        self.surface.queue_allocate();
    }
    pub(crate) fn drawers(&self) -> Vec<Rc<drawers::Drawer>> {
        std::iter::once(self.drawer.clone())
            .chain(self.columns.drawers.borrow().iter().cloned())
            .collect()
    }
    fn measure_drawer_tiles(&self) {
        self.publication.hits.borrow_mut().take();
        let mut measurements = Vec::new();
        let mut columns = Vec::new();
        for drawer in self.columns.drawers.borrow().iter() {
            drawer.tile_measurements(self, &mut measurements);
            columns.extend(drawer.measurement());
        }
        if let Some(g) = self.gpu.borrow_mut().as_mut() {
            // Measurement-only dispatch: no widget refresh during allocation.
            if let Err(error) = g
                .session
                .dispatch(UiAction::MeasureDrawerTiles { measurements })
            {
                eprintln!("Drawer measurement: {error}");
            }
            if let Err(error) = g.session.dispatch(UiAction::MeasureColumnDrawers {
                measurements: columns,
            }) {
                eprintln!("Column drawer measurement: {error}");
            }
        }
    }
    fn install_panel_drag(self: &Rc<Self>, widget: &impl IsA<gtk::Widget>, item: DockItem) {
        if matches!(item, DockItem::Tile { .. }) {
            widget.add_css_class("drag-hold");
        }
        self.register_drag(widget, DragTarget::Dock(item));
    }
    fn clear_drop(&self) {
        self.drop_hint.borrow_mut().take();
        self.surface.queue_draw();
    }
    fn tab_hits(&self) -> Vec<TabHit> {
        let mut hits: Vec<_> = self
            .groups
            .borrow()
            .iter()
            .flat_map(|g| {
                g.tabs.iter().enumerate().filter_map(|(index, (_, tab))| {
                    let b = tab.compute_bounds(&self.surface)?;
                    let clip = tab_drag::tab_clip(tab.upcast_ref(), &self.surface)?;
                    Some(TabHit {
                        group: g.id,
                        index,
                        bounds: Bounds {
                            x: b.x(),
                            y: b.y(),
                            width: b.width(),
                            height: b.height(),
                        }
                        .intersection(clip)?,
                    })
                })
            })
            .collect();
        for drawer in self.columns.drawers.borrow().iter() {
            hits.extend(drawer.tab_hits(self));
        }
        hits
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
        crate::files::drop::install(self);
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
        let p = crate::input::widget_point(&self.surface, x, y)?;
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
        if matches!(target, DragTarget::Dock(_) | DragTarget::Header(_)) {
            widget.set_cursor_from_name(Some(if widget.has_css_class("drag-hold") {
                "default"
            } else {
                "grab"
            }));
        }
        let mut targets = self.drag_targets.borrow_mut();
        targets.retain(|(widget, _)| widget.upgrade().is_some());
        targets.push((widget.as_ref().downgrade(), target));
    }

    fn drag_target_at(&self, point: [f32; 2]) -> Option<DragTarget> {
        self.drag_source_at(point).map(|(_, target)| target)
    }

    fn drag_source_at(&self, point: [f32; 2]) -> Option<(gtk::Widget, DragTarget)> {
        let mut picked =
            self.surface
                .pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT);
        while let Some(widget) = picked {
            if widget.has_css_class("catalog-add") {
                return None;
            }
            if let Some(target) = self
                .drag_targets
                .borrow()
                .iter()
                .rev()
                .find_map(|(w, target)| (w.upgrade().as_ref() == Some(&widget)).then_some(*target))
            {
                if matches!(target, DragTarget::Header(_)) && !self.header.editing.get() {
                    return None;
                }
                if self.header.editing.get() && !matches!(target, DragTarget::Header(_)) {
                    return None;
                }
                return Some((widget, target));
            }
            picked = widget.parent();
        }
        None
    }

    // Every input reaches Rust. workspace_update retains models by revision and
    // replaces only pending presentation on GTK's frame clock (see module).
    fn dispatch_drag(self: &Rc<Self>, target: DragTarget, phase: ContactPhase, position: [f32; 2]) {
        #[cfg(test)]
        let start = std::time::Instant::now();
        let tabs = if matches!(target, DragTarget::Dock(_)) {
            if phase != ContactPhase::Move || self.publication.hits.borrow().is_none() {
                *self.publication.hits.borrow_mut() = Some(self.tab_hits());
            }
            self.publication.hits.borrow().clone().unwrap_or_default()
        } else {
            Vec::new()
        };
        if phase == ContactPhase::Up && matches!(target, DragTarget::Dock(_)) {
            self.measure_panels();
        }
        if let Some(action) = target.action(
            phase,
            position,
            [self.surface.width() as f32, self.surface.height() as f32],
            tabs,
        ) {
            self.dispatch(action);
        }
        #[cfg(test)]
        if phase == ContactPhase::Move {
            self.publication
                .inputs
                .borrow_mut()
                .push(start.elapsed().as_secs_f64() * 1000.);
        }
    }

    fn set_drag_cursor(&self, drag: &mut NativeWorkspaceDrag, name: &str) {
        let Some(device) = &drag.device else {
            return;
        };
        // Remember the original cursor once, across armed and dragging states.
        if drag.cursor.is_none() {
            let widget = self
                .surface
                .pick(
                    drag.origin[0] as f64,
                    drag.origin[1] as f64,
                    gtk::PickFlags::DEFAULT,
                )
                .unwrap_or_else(|| drag.source.clone());
            drag.cursor = Some((widget.clone(), widget.cursor()));
        }
        if let Some((widget, _)) = &drag.cursor
            && widget.cursor().and_then(|cursor| cursor.name()).as_deref() != Some(name)
        {
            widget.set_cursor_from_name(Some(name));
        }
        if let Some(surface) = self.window.surface()
            && surface
                .device_cursor(device)
                .and_then(|cursor| cursor.name())
                .as_deref()
                != Some(name)
            && let Some(cursor) = gdk::Cursor::from_name(name, None)
        {
            // The pressed widget may be unparented by a tab tear-off. Keep
            // feedback on the originating mouse/pen throughout that gesture.
            surface.set_device_cursor(device, &cursor);
        }
    }

    fn restore_drag_cursor(&self, drag: &NativeWorkspaceDrag) {
        if drag.cursor.is_none() {
            return;
        }
        if let Some((widget, cursor)) = &drag.cursor {
            widget.set_cursor(cursor.as_ref());
        }
        let mut picked = self.surface.pick(
            drag.point[0] as f64,
            drag.point[1] as f64,
            gtk::PickFlags::DEFAULT,
        );
        let mut cursor = None;
        while let Some(widget) = picked {
            cursor = widget.cursor();
            if cursor.is_some() {
                break;
            }
            picked = widget.parent();
        }
        if let Some(surface) = self.window.surface()
            && let Some(device) = &drag.device
            && let Some(cursor) = cursor.or_else(|| gdk::Cursor::from_name("default", None))
        {
            surface.set_device_cursor(device, &cursor);
        }
    }

    fn reset_drag_recognizers(&self, drag: &NativeWorkspaceDrag) {
        // The stable controller consumes release. Reset native click/hold
        // recognizers so cancellation cannot leave a timer or a stale click.
        let mut picked = self
            .surface
            .pick(
                drag.origin[0] as f64,
                drag.origin[1] as f64,
                gtk::PickFlags::DEFAULT,
            )
            .or_else(|| Some(drag.source.clone()));
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
    }

    fn workspace_drag_input(
        self: &Rc<Self>,
        phase: ContactPhase,
        point: [f32; 2],
        sequence: Option<gdk::EventSequence>,
    ) -> bool {
        if phase == ContactPhase::Down {
            if self.workspace_drag.borrow().is_none()
                && let Some((source, target)) = self.drag_source_at(point)
            {
                let mut drag = NativeWorkspaceDrag {
                    target,
                    origin: point,
                    point,
                    started: false,
                    held: false,
                    context: false,
                    wait_for_hold: source.has_css_class("drag-hold"),
                    parent: source.parent(),
                    source,
                    sequence,
                    device: self
                        .surface
                        .display()
                        .default_seat()
                        .and_then(|seat| seat.pointer()),
                    cursor: None,
                    tab: None,
                    tab_grab: None,
                };
                self.grab_tab_slide(&mut drag);
                *self.workspace_drag.borrow_mut() = Some(drag);
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
        let phase = if matches!(drag.target, DragTarget::Header(_))
            && (!drag.source.is_ancestor(&self.surface) || drag.source.parent() != drag.parent)
        {
            ContactPhase::Cancel
        } else {
            phase
        };
        if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
            self.workspace_drag.borrow_mut().take();
            if phase == ContactPhase::Cancel || drag.held {
                self.reset_drag_recognizers(&drag);
            }
            self.clear_tab_slide(&mut drag);
            if drag.started {
                if matches!(drag.target, DragTarget::Header(_)) {
                    self.dragging.set(false);
                    self.header
                        .finish_drag(self, point, phase == ContactPhase::Cancel);
                    self.header.clear_drop();
                } else if let DragTarget::Dock(item @ DockItem::Tile { .. }) = drag.target {
                    self.dragging.set(false);
                    if phase == ContactPhase::Up
                        && let Some(hint) = self.drop_at(point[0], point[1], item)
                    {
                        self.dispatch(item.move_action(
                            hint.target,
                            [self.surface.width() as f32, self.surface.height() as f32],
                        ));
                    }
                } else {
                    self.dispatch_drag(drag.target, phase, point);
                }
            } else if drag.context {
                if phase == ContactPhase::Cancel {
                    self.dismiss_context();
                } else {
                    self.finish_context_hold();
                }
            }
            drag.point = point;
            self.restore_drag_cursor(&drag);
            self.clear_drop();
            self.update_zen();
            return drag.started || drag.held || drag.context;
        }
        if !drag.started {
            if !drag.source.is_ancestor(&self.surface) || drag.source.parent() != drag.parent {
                self.workspace_drag.borrow_mut().take();
                drag.point = point;
                self.restore_drag_cursor(&drag);
                self.dismiss_context();
                return false;
            }
            let recognized = if matches!(drag.target, DragTarget::Dock(_) | DragTarget::Header(_)) {
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
            if drag.wait_for_hold && !drag.held {
                // Moving before the native hold wins belongs to scrolling.
                self.workspace_drag.borrow_mut().take();
                return false;
            }
            self.dismiss_context();
            self.reset_drag_recognizers(&drag);
            drag.started = true;
            if matches!(
                drag.target,
                DragTarget::Dock(DockItem::Tile { .. }) | DragTarget::Header(_)
            ) {
                self.dragging.set(true);
            }
            if let DragTarget::Header(source) = drag.target
                && !self
                    .header
                    .start_drag(self, source, drag.origin, &drag.source)
            {
                self.workspace_drag.borrow_mut().take();
                self.dragging.set(false);
                self.restore_drag_cursor(&drag);
                return false;
            }
            self.start_tab_slide(&mut drag);
            if matches!(drag.target, DragTarget::Dock(_) | DragTarget::Header(_)) {
                self.set_drag_cursor(&mut drag, "grabbing");
            }
            *self.workspace_drag.borrow_mut() = Some(drag.clone());
            if !matches!(
                drag.target,
                DragTarget::Dock(DockItem::Tile { .. }) | DragTarget::Header(_)
            ) {
                self.dispatch_drag(drag.target, ContactPhase::Down, drag.origin);
            }
            if let Some(tab) = &drag.tab
                && let Some(gpu) = self.gpu.borrow_mut().as_mut()
            {
                gpu.session.begin_tab_drag(&tab.hits, tab.clip);
            }
        }
        drag.point = point;
        *self.workspace_drag.borrow_mut() = Some(drag.clone());
        if !matches!(
            drag.target,
            DragTarget::Dock(DockItem::Tile { .. }) | DragTarget::Header(_)
        ) {
            self.dispatch_drag(drag.target, ContactPhase::Move, point);
        }
        if let DragTarget::Dock(item) = drag.target {
            if matches!(item, DockItem::Tile { .. }) {
                *self.drop_hint.borrow_mut() = self.drop_at(point[0], point[1], item);
                self.surface.queue_draw();
            }
            self.set_drag_cursor(&mut drag, "grabbing");
        }
        if matches!(drag.target, DragTarget::Header(_)) {
            self.header.drag_motion(self, point);
            self.set_drag_cursor(&mut drag, "grabbing");
        }
        true
    }

    fn install_workspace_drag(self: &Rc<Self>) {
        let column_click = Rc::new(Cell::new(None::<(DragTarget, u32, [f32; 2], Option<gdk::InputSource>)>));
        let click = gtk::GestureClick::new();
        click.set_name(Some("panel-handle-double-click"));
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            #[strong]
            column_click,
            move |gesture, _, x, y| {
                let point = [x as f32, y as f32];
                let target = w.drag_target_at(point);
                let collapsed_column = w.columns.background_at(&w, point);
                let Some(event) = gesture.current_event() else { return };
                let source = event.device().map(|device| device.source());
                let handle = collapsed_column
                    .map(|column| DragTarget::Dock(DockItem::Column { column }))
                    .or(target);
                let previous = column_click.replace(handle.map(|item| (item, event.time(), point, source)));
                let settings = gtk::Settings::for_display(&w.surface.display());
                // A workspace GestureClick sees every control. Require the same
                // handle and device class for touch/pen as well as mouse; a tap
                // on a tile must not turn the next divider press into a reset.
                let double = handle.is_some_and(|item| previous.is_some_and(|(id, time, position, device)| {
                    id == item && device == source
                        && event.time().wrapping_sub(time) <= settings.gtk_double_click_time().max(0) as u32
                        && (point[0] - position[0]).abs().max((point[1] - position[1]).abs())
                            <= settings.gtk_double_click_distance().max(0) as f32
                }));
                if !double { return; }
                column_click.set(None);
                let viewport = [w.surface.width() as f32, w.surface.height() as f32];
                let action = if let Some(column) = collapsed_column {
                    w.resolved()
                        .collapsed
                        .iter()
                        .find(|c| c.id == column)
                        .map(|c| c.expand_action())
                } else {
                    match target {
                        Some(DragTarget::Divider(id))
                            if w.resolved().dividers.iter().any(|d| {
                                d.id == id && d.band && d.axis == layer_ui::Axis::Horizontal
                            }) =>
                        {
                            Some(UiAction::ResetColumnWidth { id, viewport })
                        }
                        Some(DragTarget::Dock(item)) => w
                            .surface
                            .imp()
                            .layout
                            .borrow()
                            .panel_handle_target(item)
                            .map(|group| UiAction::DoubleClickPanelHandle { group, viewport }),
                        _ => None,
                    }
                };
                let Some(action) = action else { return };
                gesture.set_state(gtk::EventSequenceState::Claimed);
                // The captured double-click consumes release; retire any
                // pending press so later motion cannot start a stale drag.
                w.workspace_drag_input(ContactPhase::Cancel, point, None);
                w.dispatch(action);
            }
        ));
        self.surface.add_controller(click);
        // A window-surface event stream survives unparenting the pressed tab.
        // GtkGestureDrag cancels that sequence when tear-off rebuilds its group.
        // Native widgets keep clicks until pickup is eligible and crosses slop.
        let pointer = gtk::EventControllerLegacy::new();
        pointer.set_name(Some("workspace-drag"));
        pointer.set_propagation_phase(gtk::PropagationPhase::Capture);
        pointer.connect_event(glib::clone!(
            #[weak(rename_to = w)]
            self,
            #[strong]
            column_click,
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
                let starting = phase == ContactPhase::Down && w.workspace_drag.borrow().is_none();
                let handled =
                    point.is_some_and(|point| w.workspace_drag_input(phase, point, sequence));
                if starting && let Some(drag) = w.workspace_drag.borrow_mut().as_mut() {
                    // A tablet has its own GDK device; touch has no cursor.
                    drag.device = if touch { None } else { event.device() };
                    if drag.source.has_css_class("drag-row") {
                        drag.wait_for_hold = touch
                            || event.device_tool().is_some()
                            || event.device().is_some_and(|d| {
                                matches!(
                                    d.source(),
                                    gdk::InputSource::Touchscreen | gdk::InputSource::Pen
                                )
                            });
                    }
                }
                if handled {
                    column_click.set(None);
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
pub(crate) fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::Widget {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(child)
        .build()
        .upcast()
}
pub(crate) fn selected(widget: &impl IsA<gtk::Widget>, selected: bool) {
    if selected {
        widget.add_css_class("selected-tool");
    } else {
        widget.remove_css_class("selected-tool");
    }
}
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
