//! Native Navigator geometry; camera policy and input behavior live in Rust UI.
use crate::workspace::{Workspace, selected};
use gtk::{glib, prelude::*, subclass::prelude::*};
use layer_ui::{Camera, CommandId, ContactPhase, NavigatorGeometry, UiAction, UiState};
use std::{cell::RefCell, rc::Rc};

/// Native geometry only. The document image and outline are drawn by the
/// existing canvas worker; GTK never imports an overview texture.
#[derive(Default)]
pub struct Overviews {
    views: RefCell<Vec<glib::WeakRef<Overview>>>,
    owner: RefCell<std::rc::Weak<Workspace>>,
    wake_pending: std::cell::Cell<bool>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Projection {
    image: gtk::graphene::Rect,
    clip: gtk::graphene::Rect,
    origin: [f32; 2],
    opacity: f32,
    order: usize,
}
impl Overviews {
    pub fn bind(self: &Rc<Self>, w: &Rc<Workspace>) {
        *self.owner.borrow_mut() = Rc::downgrade(w);
    }
    fn wake(self: &Rc<Self>) {
        if self.wake_pending.replace(true) {
            return;
        }
        let this = self.clone();
        // Snapshot can run during a native allocation or explicit capture.
        // Defer session access rather than reentering its RefCell borrow.
        glib::idle_add_local_once(move || {
            this.wake_pending.set(false);
            if let Some(w) = this.owner.borrow().upgrade() {
                w.wake();
            }
        });
    }
    pub fn placements(
        &self,
        state: &UiState,
        scale: f32,
    ) -> Vec<layer_render_wgpu::OverviewPlacement> {
        let Some(tab) = state.tabs.first() else {
            return Vec::new();
        };
        let bg = state.palette.panel.linear();
        let fg = state.palette.text.linear();
        let mut views: Vec<_> = self
            .views
            .borrow()
            .iter()
            .filter_map(|v| v.upgrade())
            .filter_map(|v| {
                if !v.is_mapped() {
                    return None;
                }
                let p = v.imp().projection.get()?;
                let g = NavigatorGeometry::new(
                    &state.camera,
                    [tab.width, tab.height],
                    [v.width() as f32, v.height() as f32],
                )?;
                let rect = |r: gtk::graphene::Rect| {
                    [
                        r.x() * scale,
                        r.y() * scale,
                        r.width() * scale,
                        r.height() * scale,
                    ]
                };
                Some((
                    p.order,
                    layer_render_wgpu::OverviewPlacement {
                        bounds: rect(p.image),
                        clip: Some(rect(p.clip)),
                        work_area: g
                            .work_area
                            .map(|[x, y]| [(p.origin[0] + x) * scale, (p.origin[1] + y) * scale]),
                        outline_linear: [fg[0], fg[1], fg[2]],
                        background_linear: [bg[0], bg[1], bg[2]],
                        opacity: p.opacity,
                        scale,
                    },
                ))
            })
            .collect();
        views.sort_by_key(|(order, _)| *order);
        views.into_iter().map(|(_, view)| view).collect()
    }
    pub fn project_child(
        self: &Rc<Self>,
        parent: &gtk::Widget,
        child: &gtk::Widget,
        order: usize,
        node: Option<&gtk::gsk::RenderNode>,
    ) -> Vec<gtk::graphene::Rect> {
        self.views.borrow_mut().retain(|v| v.upgrade().is_some());
        let views: Vec<_> = self
            .views
            .borrow()
            .iter()
            .filter_map(|v| v.upgrade())
            .filter(|v| v.is_ancestor(child))
            .collect();
        let mut holes = Vec::new();
        for view in views {
            let projection = node.and_then(|node| view.projection(parent, child, node, order));
            if view.imp().projection.replace(projection) != projection {
                self.wake();
            }
            if let Some(p) = projection {
                holes.push(p.clip);
            }
        }
        holes
    }
    pub fn append_clipped(
        target: &gtk::Snapshot,
        node: &gtk::gsk::RenderNode,
        holes: impl Iterator<Item = gtk::graphene::Rect>,
    ) {
        let holes: Vec<_> = holes
            .filter(|h| node.bounds().intersection(h).is_some())
            .collect();
        if holes.is_empty() {
            target.append_node(node);
            return;
        }
        // Rectangular subtraction preserves the native group's shadow/background
        // without a mask texture or offscreen render target. Holes include
        // overviews in later panels, so their images can cover this panel too.
        let mut pieces = vec![node.bounds()];
        for hole in holes {
            pieces = pieces
                .into_iter()
                .flat_map(|rect| subtract(rect, hole))
                .collect();
        }
        for piece in pieces {
            target.push_clip(&piece);
            target.append_node(node);
            target.pop();
        }
    }
}
fn subtract(rect: gtk::graphene::Rect, hole: gtk::graphene::Rect) -> Vec<gtk::graphene::Rect> {
    let Some(c) = rect.intersection(&hole) else {
        return vec![rect];
    };
    [
        gtk::graphene::Rect::new(rect.x(), rect.y(), rect.width(), c.y() - rect.y()),
        gtk::graphene::Rect::new(
            rect.x(),
            c.y() + c.height(),
            rect.width(),
            rect.y() + rect.height() - c.y() - c.height(),
        ),
        gtk::graphene::Rect::new(rect.x(), c.y(), c.x() - rect.x(), c.height()),
        gtk::graphene::Rect::new(
            c.x() + c.width(),
            c.y(),
            rect.x() + rect.width() - c.x() - c.width(),
            c.height(),
        ),
    ]
    .into_iter()
    .filter(|r| r.width() > 0. && r.height() > 0.)
    .collect()
}
fn node_opacity(node: &gtk::gsk::RenderNode) -> f32 {
    if let Some(n) = node.downcast_ref::<gtk::gsk::OpacityNode>() {
        n.opacity() * node_opacity(&n.child())
    } else if let Some(n) = node.downcast_ref::<gtk::gsk::TransformNode>() {
        node_opacity(&n.child())
    } else if let Some(n) = node.downcast_ref::<gtk::gsk::DebugNode>() {
        node_opacity(&n.child())
    } else if let Some(n) = node.downcast_ref::<gtk::gsk::ShadowNode>() {
        node_opacity(&n.child())
    } else {
        1.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_holes_partition_the_native_panel_without_overdraw() {
        use gtk::graphene::{Point, Rect};
        let panel = Rect::new(10., 20., 40., 30.);
        for hole in [
            Rect::new(20., 25., 10., 10.),
            Rect::new(0., 0., 100., 100.),
            Rect::new(0., 0., 15., 28.),
            Rect::new(70., 80., 10., 10.),
            Rect::new(45., 22., 40., 15.),
        ] {
            let pieces = subtract(panel, hole);
            for y in 0..100 {
                for x in 0..100 {
                    let point = Point::new(x as f32 + 0.5, y as f32 + 0.5);
                    let count = pieces.iter().filter(|r| r.contains_point(&point)).count();
                    assert_eq!(
                        count,
                        usize::from(panel.contains_point(&point) && !hole.contains_point(&point))
                    );
                }
            }
        }
    }
}

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Overview {
        pub view: RefCell<Option<(Camera, [u32; 2])>>,
        pub(super) projection: std::cell::Cell<Option<Projection>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Overview {
        const NAME: &'static str = "CapyNavigatorOverview";
        type Type = super::Overview;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Overview {}
    impl WidgetImpl for Overview {
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => (0, 220, -1, -1),
                _ => (0, 164, -1, -1),
            }
        }
        // Native hit testing/layout only; pixels live on the canvas surface.
    }
}
glib::wrapper! {
    pub struct Overview(ObjectSubclass<imp::Overview>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl Overview {
    fn projection(
        &self,
        root: &gtk::Widget,
        panel: &gtk::Widget,
        node: &gtk::gsk::RenderNode,
        order: usize,
    ) -> Option<Projection> {
        if !self.is_mapped() {
            return None;
        }
        let (camera, extent) = self.imp().view.borrow().clone()?;
        let g =
            NavigatorGeometry::new(&camera, extent, [self.width() as f32, self.height() as f32])?;
        let origin = self.compute_point(root, &gtk::graphene::Point::new(0., 0.))?;
        let image = gtk::graphene::Rect::new(
            origin.x() + g.image.x,
            origin.y() + g.image.y,
            g.image.width,
            g.image.height,
        );
        let mut clip = image.intersection(&gtk::graphene::Rect::new(
            0.,
            0.,
            root.width() as f32,
            root.height() as f32,
        ))?;
        let mut opacity = node_opacity(node);
        let mut ancestor: Option<gtk::Widget> = Some(self.clone().upcast());
        while let Some(widget) = ancestor {
            if !widget.is_mapped() {
                return None;
            }
            // Root panel opacity is already represented by its native snapshot.
            if widget != *panel {
                opacity *= widget.opacity() as f32;
            }
            if widget.overflow() == gtk::Overflow::Hidden {
                let p = widget.compute_point(root, &gtk::graphene::Point::new(0., 0.))?;
                clip = clip.intersection(&gtk::graphene::Rect::new(
                    p.x(),
                    p.y(),
                    widget.width() as f32,
                    widget.height() as f32,
                ))?;
            }
            if widget == *root {
                break;
            }
            ancestor = widget.parent();
        }
        (opacity > 0. && clip.width() > 0. && clip.height() > 0.).then_some(Projection {
            image,
            clip,
            origin: [origin.x(), origin.y()],
            opacity,
            order,
        })
    }
}
pub struct Navigator {
    pub root: gtk::Box,
    overview: Overview,
    buttons: Vec<(CommandId, gtk::Button)>,
}
impl Navigator {
    pub fn new(images: &Rc<Overviews>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 2);
        root.set_widget_name("navigator-panel");
        root.set_margin_start(8);
        root.set_margin_end(8);
        root.set_margin_top(8);
        root.set_margin_bottom(8);
        let overview: Overview = glib::Object::new();
        overview.set_widget_name("navigator-overview");
        overview.add_css_class("navigator-overview");
        overview.set_hexpand(true);
        overview.set_vexpand(true);
        overview.set_cursor_from_name(Some("grab"));
        images.views.borrow_mut().push(overview.downgrade());
        overview.connect_unmap(glib::clone!(
            #[weak]
            images,
            move |view| {
                // A detached/hidden view is no longer visited by project_child.
                // Remove its GPU image now, and invalidate the cached projection
                // so reopening at the same size also schedules a fresh frame.
                if view.imp().projection.take().is_some() {
                    images.wake();
                }
            }
        ));
        root.append(&overview);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        row.set_homogeneous(true);
        row.add_css_class("navigator-controls");
        let buttons = [
            CommandId::ZoomOut,
            CommandId::ZoomIn,
            CommandId::RotateLeft,
            CommandId::RotateRight,
            CommandId::FlipHorizontal,
            CommandId::FlipVertical,
        ]
        .into_iter()
        .map(|id| {
            let button =
                gtk::Button::from_icon_name(&format!("layer-{}-symbolic", id.icon().unwrap()));
            button.add_css_class("flat");
            button.set_widget_name(&format!("navigator-{id:?}"));
            button.set_tooltip_text(Some(id.label()));
            row.append(&button);
            (id, button)
        })
        .collect();
        root.append(&row);
        Self {
            root,
            overview,
            buttons,
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        for (id, button) in &self.buttons {
            let id = *id;
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| w.dispatch(UiAction::Invoke { command: id })
            ));
        }
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        for phase in [ContactPhase::Down, ContactPhase::Move, ContactPhase::Up] {
            let weak = Rc::downgrade(w);
            let view = self.overview.downgrade();
            let callback = move |drag: &gtk::GestureDrag, x: f64, y: f64| {
                let (Some(w), Some(view)) = (weak.upgrade(), view.upgrade()) else {
                    return;
                };
                let (sx, sy) = if phase == ContactPhase::Down {
                    (0., 0.)
                } else {
                    drag.start_point().unwrap_or_default()
                };
                w.dispatch(UiAction::Navigator {
                    phase,
                    position: [(sx + x) as f32, (sy + y) as f32],
                    viewport: [view.width() as f32, view.height() as f32],
                });
            };
            match phase {
                ContactPhase::Down => {
                    drag.connect_drag_begin(callback);
                }
                ContactPhase::Move => {
                    drag.connect_drag_update(callback);
                }
                _ => {
                    drag.connect_drag_end(callback);
                }
            }
        }
        drag.connect_cancel(glib::clone!(
            #[weak]
            w,
            move |_, _| {
                w.dispatch(UiAction::Navigator {
                    phase: ContactPhase::Cancel,
                    position: [0.0; 2],
                    viewport: [0.0; 2],
                });
            }
        ));
        self.overview.add_controller(drag);
    }
    pub fn refresh(&self, state: &UiState) {
        if let Some(tab) = state.tabs.first() {
            let changed = self
                .overview
                .imp()
                .view
                .borrow()
                .as_ref()
                .is_none_or(|(_, size)| *size != [tab.width, tab.height]);
            *self.overview.imp().view.borrow_mut() =
                Some((state.camera.clone(), [tab.width, tab.height]));
            if changed {
                self.overview.queue_draw();
            }
        }
        for (id, button) in &self.buttons {
            if let Some(command) = state.commands.iter().find(|c| c.id == *id) {
                button.set_sensitive(command.enabled);
                button.set_tooltip_text(Some(&command.tooltip));
                selected(button, command.selected);
            }
        }
    }
}
