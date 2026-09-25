//! Retained swatch allocation. History reserves its last visible cell for the
//! expand/collapse button, including when the panel is resized.
use gtk::{glib, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};

const TILE: i32 = 40;
const GAP: i32 = 4;
const MIN_COLUMNS: i32 = 6;

struct Slide {
    source: gtk::Widget,
    from: Vec<gtk::graphene::Point>,
    to: Vec<gtk::graphene::Point>,
}
impl Slide {
    fn offset(&self, index: usize, progress: f32) -> gtk::graphene::Point {
        let (from, to) = (self.from[index], self.to[index]);
        gtk::graphene::Point::new(
            from.x() + (to.x() - from.x()) * progress,
            from.y() + (to.y() - from.y()) * progress,
        )
    }
}

fn cell_bounds(width: i32, index: usize) -> gtk::graphene::Rect {
    let columns = ((width + GAP) / (TILE + GAP)).max(1);
    let cell = (width + GAP) as f32 / columns as f32;
    let index = index as i32;
    let x = ((index % columns) as f32 * cell).round();
    let next = (((index % columns) + 1) as f32 * cell).round();
    gtk::graphene::Rect::new(
        x,
        (index / columns * (TILE + GAP)) as f32,
        next - x - GAP as f32,
        TILE as f32,
    )
}
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Grid {
        pub children: RefCell<Vec<gtk::Widget>>,
        pub history_rows: Cell<i32>,
        pub(super) slide: RefCell<Option<Slide>>,
        pub progress: Cell<f32>,
        pub animation: RefCell<Option<adw::TimedAnimation>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Grid {
        const NAME: &'static str = "LayerPaletteGrid";
        type Type = super::PaletteGrid;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Grid {
        fn dispose(&self) {
            self.obj().clear_reorder();
            for child in self.children.take() {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for Grid {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let size = if orientation == gtk::Orientation::Horizontal {
                MIN_COLUMNS * (TILE + GAP) - GAP
            } else {
                let columns = ((for_size + GAP) / (TILE + GAP)).max(MIN_COLUMNS);
                let rows = if self.history_rows.get() > 0 {
                    self.history_rows.get()
                } else {
                    ((self.children.borrow().len() as i32 + columns - 1) / columns).max(1)
                };
                rows * (TILE + GAP) - GAP
            };
            (
                if orientation == gtk::Orientation::Vertical && self.history_rows.get() > 0 {
                    TILE
                } else {
                    size
                },
                size,
                -1,
                -1,
            )
        }
        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let columns = ((width + GAP) / (TILE + GAP)).max(1);
            let rows = self.history_rows.get();
            let capacity = if rows > 0 {
                columns * rows.min(((height + GAP) / (TILE + GAP)).max(1))
            } else {
                i32::MAX
            };
            let children = self.children.borrow();
            for (index, child) in children.iter().enumerate() {
                let last = rows > 0 && index + 1 == children.len();
                let visible = last || (index as i32) < capacity - i32::from(rows > 0);
                child.set_child_visible(visible);
                if !visible {
                    continue;
                }
                let index = if last { capacity - 1 } else { index as i32 };
                let rect = cell_bounds(width, index as usize);
                child.allocate(
                    rect.width() as i32,
                    TILE,
                    -1,
                    Some(
                        gtk::gsk::Transform::new()
                            .translate(&gtk::graphene::Point::new(rect.x(), rect.y())),
                    ),
                );
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let slide = self.slide.borrow();
            for (index, child) in self.children.borrow().iter().enumerate() {
                if !child.is_child_visible() || slide.as_ref().is_some_and(|s| s.source == *child) {
                    continue;
                }
                snapshot.save();
                if let Some(slide) = slide.as_ref() {
                    snapshot.translate(&slide.offset(index, self.progress.get()));
                }
                self.obj().snapshot_child(child, snapshot);
                snapshot.restore();
            }
        }
    }
}
glib::wrapper! {
    pub struct PaletteGrid(ObjectSubclass<imp::Grid>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl PaletteGrid {
    pub fn new(history_rows: i32) -> Self {
        let grid: Self = glib::Object::new();
        grid.imp().history_rows.set(history_rows);
        grid.set_hexpand(true);
        grid.set_valign(gtk::Align::Start);
        grid
    }
    pub fn clear(&self) {
        self.clear_reorder();
        for child in self.imp().children.take() {
            child.unparent();
        }
        self.queue_resize();
    }
    pub fn append(&self, child: &impl IsA<gtk::Widget>) {
        child.set_parent(self);
        self.imp()
            .children
            .borrow_mut()
            .push(child.clone().upcast());
        self.queue_resize();
    }
    pub fn reconcile(&self, children: Vec<gtk::Widget>) {
        self.clear_reorder();
        let retained: std::collections::HashSet<_> = children.iter().collect();
        for child in self.imp().children.borrow().iter() {
            if !retained.contains(child) {
                child.unparent();
            }
        }
        for child in &children {
            if child.parent().is_none() {
                child.set_parent(self);
            }
        }
        *self.imp().children.borrow_mut() = children;
        self.queue_resize();
    }

    /// Hit fixed cells, including their gutters, never the moving presentation.
    pub fn slot_at(&self, point: gtk::graphene::Point) -> Option<usize> {
        if point.x() < 0. || point.x() >= self.width() as f32 || point.y() < 0. {
            return None;
        }
        let columns = ((self.width() + GAP) / (TILE + GAP)).max(1) as usize;
        let column = (point.x() * columns as f32 / (self.width() + GAP) as f32) as usize;
        let row = (point.y() / (TILE + GAP) as f32) as usize;
        let index = row * columns + column;
        (index < self.imp().children.borrow().len()).then_some(index)
    }

    pub fn preview_reorder(&self, source: &gtk::Widget, order: &[gtk::Widget]) {
        use adw::prelude::*;
        let imp = self.imp();
        let destinations: std::collections::HashMap<_, _> =
            order.iter().enumerate().map(|(i, w)| (w, i)).collect();
        let children = imp.children.borrow();
        let old = imp.slide.borrow();
        let mut from = Vec::with_capacity(children.len());
        let mut to = Vec::with_capacity(children.len());
        for (index, child) in children.iter().enumerate() {
            from.push(old.as_ref().map_or(gtk::graphene::Point::zero(), |s| {
                s.offset(index, imp.progress.get())
            }));
            let target = cell_bounds(
                self.width(),
                destinations.get(child).copied().unwrap_or(index),
            );
            let original = cell_bounds(self.width(), index);
            to.push(gtk::graphene::Point::new(
                target.x() - original.x(),
                target.y() - original.y(),
            ));
        }
        drop(old);
        if let Some(animation) = imp.animation.take() {
            animation.pause();
        }
        *imp.slide.borrow_mut() = Some(Slide {
            source: source.clone(),
            from,
            to,
        });
        imp.progress.set(0.);
        let target = adw::CallbackAnimationTarget::new(glib::clone!(
            #[weak(rename_to=grid)]
            self,
            move |progress| {
                grid.imp().progress.set(progress as f32);
                grid.queue_draw();
            }
        ));
        // Adwaita follows the system's reduced-motion setting and frame clock.
        let animation = adw::TimedAnimation::new(self, 0., 1., 140, target);
        animation.set_easing(adw::Easing::EaseOutCubic);
        animation.play();
        *imp.animation.borrow_mut() = Some(animation);
        self.queue_draw();
    }

    pub fn clear_reorder(&self) {
        use adw::prelude::*;
        if let Some(animation) = self.imp().animation.take() {
            animation.pause();
        }
        if self.imp().slide.take().is_some() {
            self.queue_draw();
        }
    }

    pub fn is_reordering(&self) -> bool {
        self.imp().slide.borrow().is_some()
    }

    #[cfg(test)]
    pub fn visual_bounds(&self, child: &gtk::Widget) -> Option<gtk::graphene::Rect> {
        let index = self
            .imp()
            .children
            .borrow()
            .iter()
            .position(|c| c == child)?;
        let bounds = child.compute_bounds(self)?;
        let offset = self
            .imp()
            .slide
            .borrow()
            .as_ref()
            .map_or(gtk::graphene::Point::zero(), |s| {
                s.offset(index, self.imp().progress.get())
            });
        Some(gtk::graphene::Rect::new(
            bounds.x() + offset.x(),
            bounds.y() + offset.y(),
            bounds.width(),
            bounds.height(),
        ))
    }
}
