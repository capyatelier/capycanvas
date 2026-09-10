//! Native Navigator projection; camera policy and preview scheduling live in Rust UI.
use crate::workspace::{Workspace, selected};
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};
use layer_ui::{Camera, CommandId, ContactPhase, NavigatorGeometry, UiAction, UiState};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
pub struct Images {
    texture: RefCell<Option<gdk::Texture>>,
    views: RefCell<Vec<glib::WeakRef<Overview>>>,
    update_clock: RefCell<Option<gdk::FrameClock>>,
}
impl Drop for Images {
    fn drop(&mut self) {
        if let Some(clock) = self.update_clock.get_mut().take() {
            clock.end_updating();
        }
    }
}
impl Images {
    #[cfg(test)]
    pub fn updating(&self) -> bool {
        self.update_clock.borrow().is_some()
    }
    #[cfg(test)]
    pub fn texture(&self) -> Option<gdk::Texture> {
        self.texture.borrow().clone()
    }
    pub fn bind(self: &Rc<Self>, w: &Rc<Workspace>) {
        let weak = Rc::downgrade(w);
        let images = self.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(67), move || {
            let Some(w) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            images.views.borrow_mut().retain(|v| v.upgrade().is_some());
            let visible = images
                .views
                .borrow()
                .iter()
                .filter_map(|v| v.upgrade())
                .any(|v| {
                    if !v.is_mapped() {
                        return false;
                    }
                    let mut parent = v.parent();
                    while let Some(widget) = parent {
                        if widget.has_css_class("zen-hidden") || widget.opacity() == 0.0 {
                            return false;
                        }
                        parent = widget.parent();
                    }
                    true
                });
            let live = visible
                && w.gpu
                    .borrow()
                    .as_ref()
                    .is_some_and(|g| g.session.navigator_updates_continuously());
            // Intermittent (15Hz) thumbnails must not restart GTK's idle clock
            // for each image: that missed compositor deadlines while painting.
            // Keep update timing alive, not redraws; release it as soon as idle.
            if live && images.update_clock.borrow().is_none() {
                if let Some(clock) = w.area.frame_clock() {
                    clock.begin_updating();
                    *images.update_clock.borrow_mut() = Some(clock);
                }
            } else if !live && let Some(clock) = images.update_clock.borrow_mut().take() {
                clock.end_updating();
            }
            let image = w.gpu.borrow_mut().as_mut().and_then(|g| {
                g.session
                    .poll_navigator_preview(glib::monotonic_time().max(0) as u64 * 1000, visible)
                    .ok()
                    .flatten()
            });
            if let Some(image) = image {
                let bytes = glib::Bytes::from_owned(image.bytes);
                *images.texture.borrow_mut() = Some(
                    gdk::MemoryTexture::new(
                        image.width as i32,
                        image.height as i32,
                        gdk::MemoryFormat::R8g8b8a8,
                        &bytes,
                        image.stride as usize,
                    )
                    .upcast(),
                );
                for view in images.views.borrow().iter().filter_map(|v| v.upgrade()) {
                    view.queue_draw();
                }
            }
            glib::ControlFlow::Continue
        });
    }
}

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Overview {
        pub view: RefCell<Option<(Camera, [u32; 2])>>,
        pub images: RefCell<Option<Rc<Images>>>,
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
                _ => (100, 164, -1, -1),
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let Some((camera, document)) = self.view.borrow().clone() else {
                return;
            };
            let size = [widget.width() as f32, widget.height() as f32];
            let Some(g) = NavigatorGeometry::new(&camera, document, size) else {
                return;
            };
            let b = g.image;
            let bounds = gtk::graphene::Rect::new(b.x, b.y, b.width, b.height);
            if let Some(images) = self.images.borrow().as_ref()
                && let Some(texture) = images.texture.borrow().as_ref()
            {
                snapshot.append_texture(texture, &bounds);
            }
            // Eight tiny GPU scene rectangles, not a new Cairo bitmap per pan.
            snapshot.push_clip(&bounds);
            for (color, thickness) in [(gdk::RGBA::WHITE, 3.0), (widget.color(), 1.5)] {
                for i in 0..4 {
                    let [x, y] = g.work_area[i];
                    let [nx, ny] = g.work_area[(i + 1) % 4];
                    let (dx, dy) = (nx - x, ny - y);
                    snapshot.save();
                    snapshot.translate(&gtk::graphene::Point::new(x, y));
                    snapshot.rotate(dy.atan2(dx).to_degrees());
                    snapshot.append_color(
                        &color,
                        &gtk::graphene::Rect::new(
                            -thickness * 0.5,
                            -thickness * 0.5,
                            dx.hypot(dy) + thickness,
                            thickness,
                        ),
                    );
                    snapshot.restore();
                }
            }
            snapshot.pop();
        }
    }
}
glib::wrapper! {
    pub struct Overview(ObjectSubclass<imp::Overview>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
pub struct Navigator {
    pub root: gtk::Box,
    overview: Overview,
    buttons: Vec<(CommandId, gtk::Button)>,
}
impl Navigator {
    pub fn new(images: &Rc<Images>) -> Self {
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
        *overview.imp().images.borrow_mut() = Some(images.clone());
        images.views.borrow_mut().push(overview.downgrade());
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
            *self.overview.imp().view.borrow_mut() =
                Some((state.camera.clone(), [tab.width, tab.height]));
            self.overview.queue_draw();
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
