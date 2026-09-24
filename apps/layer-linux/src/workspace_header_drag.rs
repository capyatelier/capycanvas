//! Native capture and presentation for the shared window-bar drag policy.
//! Widgets stay parented and the session stays unchanged until release.
use super::*;
use layer_ui::HeaderDragStart;

struct Visual {
    widget: gtk::Widget,
    opacity: f64,
    node: gtk::gsk::RenderNode,
    bounds: Bounds,
}
struct Neighbor {
    id: u32,
    visual: Visual,
    from: Bounds,
    to: Option<Bounds>,
}
pub(super) struct NativeHeaderDrag {
    policy: HeaderDrag,
    source: HeaderDragSource,
    held: Visual,
    neighbors: Vec<Neighbor>,
    overflow: Vec<(usize, Visual)>,
    pub(super) preview: HeaderDragPreview,
    width: f32,
    insets: [f32; 2],
    started: i64,
    duration: i64,
    tick: Option<gtk::TickCallbackId>,
    background: gdk::RGBA,
}
impl NativeHeaderDrag {
    pub fn fits(&self, width: f32, insets: [f32; 2]) -> bool {
        self.width == width && self.insets == insets
    }
    fn progress(&self, now: i64) -> f32 {
        if self.duration == 0 {
            return 1.;
        }
        let t = ((now - self.started) as f32 / self.duration as f32).clamp(0., 1.);
        1. - (1. - t).powi(3)
    }
}
fn between(from: Bounds, to: Bounds, t: f32) -> Bounds {
    Bounds {
        x: from.x + (to.x - from.x) * t,
        y: from.y + (to.y - from.y) * t,
        width: from.width + (to.width - from.width) * t,
        height: from.height + (to.height - from.height) * t,
    }
}
fn capture(widget: &gtk::Widget, surface: &DockSurface) -> Option<Visual> {
    let parent = widget.parent()?;
    let position = parent.compute_point(surface, &gtk::graphene::Point::zero())?;
    let b = widget.compute_bounds(surface)?;
    let snapshot = gtk::Snapshot::new();
    snapshot.translate(&position);
    parent.snapshot_child(widget, &snapshot);
    Some(Visual {
        widget: widget.clone(),
        opacity: widget.opacity(),
        node: snapshot.to_node()?,
        bounds: Bounds {
            x: b.x(),
            y: b.y(),
            width: b.width(),
            height: b.height(),
        },
    })
}
fn draw(visual: &Visual, at: Bounds, snapshot: &gtk::Snapshot, scale: f32) {
    snapshot.save();
    snapshot.translate(&gtk::graphene::Point::new(
        (at.x * scale).round() / scale,
        (at.y * scale).round() / scale,
    ));
    snapshot.scale(
        at.width / visual.bounds.width,
        at.height / visual.bounds.height,
    );
    snapshot.translate(&gtk::graphene::Point::new(
        -visual.bounds.x,
        -visual.bounds.y,
    ));
    snapshot.append_node(&visual.node);
    snapshot.restore();
}

impl Header {
    pub fn start_drag(
        &self,
        w: &Rc<Workspace>,
        source: HeaderDragSource,
        press: [f32; 2],
        widget: &gtk::Widget,
    ) -> bool {
        self.cancel_drag(w);
        let Some(model) = self.model.borrow().clone().filter(|_| self.editing.get()) else {
            return false;
        };
        let source_widget = match source {
            HeaderDragSource::Item(id) => self
                .items
                .borrow()
                .iter()
                .find(|i| i.entry.id == id)
                .map(|i| i.root.clone().upcast()),
            _ => Some(widget.clone()).filter(|p| p.has_css_class("header-component")),
        };
        let Some(held) = source_widget
            .as_ref()
            .and_then(|widget| capture(widget, &w.surface))
        else {
            return false;
        };
        let width = self.root.width() as f32;
        let insets = self.insets.get();
        let metrics = self.metrics(model.size);
        let Some(mut policy) = HeaderDrag::new(
            &model,
            HeaderDragStart {
                source,
                geometry: self.geometry.borrow().clone(),
                metrics: metrics.clone(),
                width,
                insets,
                press,
                grab: held.bounds,
            },
        ) else {
            return false;
        };
        let Some(preview) = policy.preview(press) else {
            return false;
        };
        let mut neighbors = Vec::new();
        for item in self.items.borrow().iter() {
            if source == HeaderDragSource::Item(item.entry.id) {
                continue;
            }
            let visible = item.root.is_child_visible();
            if !visible {
                // Hidden overflow items may be revealed by closing the source
                // slot. Capture them too, without reparenting any live control.
                let width = metrics
                    .iter()
                    .find(|m| m.id == item.entry.id)
                    .unwrap()
                    .compact;
                item.root.set_child_visible(true);
                allocate_at(
                    item.root.upcast_ref(),
                    Bounds {
                        x: 6.,
                        y: 6.,
                        width,
                        height: model.size.tile(),
                    },
                );
            }
            let visual = capture(item.root.upcast_ref(), &w.surface);
            item.root.set_child_visible(visible);
            if let Some(visual) = visual {
                let to = preview
                    .geometry
                    .items
                    .iter()
                    .find(|m| m.id == item.entry.id)
                    .map(|m| m.bounds);
                neighbors.push(Neighbor {
                    id: item.entry.id,
                    from: to.unwrap_or(visual.bounds),
                    to,
                    visual,
                });
            }
        }
        let mut overflow = Vec::new();
        for (index, button) in self.overflow.iter().enumerate() {
            let visible = button.is_child_visible();
            if !visible {
                button.set_child_visible(true);
                allocate_at(
                    button.upcast_ref(),
                    Bounds {
                        x: 6.,
                        y: 6.,
                        width: model.size.tile(),
                        height: model.size.tile(),
                    },
                );
            }
            if let Some(visual) = capture(button.upcast_ref(), &w.surface) {
                overflow.push((index, visual));
            }
            button.set_child_visible(visible);
        }
        for n in &neighbors {
            n.visual.widget.set_opacity(0.);
        }
        for (_, visual) in &overflow {
            visual.widget.set_opacity(0.);
        }
        held.widget.set_opacity(0.);
        let color = w
            .gpu
            .borrow()
            .as_ref()
            .map_or([48, 48, 48], |g| g.session.state().palette.panel.0);
        *self.drag.borrow_mut() = Some(NativeHeaderDrag {
            policy,
            source,
            held,
            neighbors,
            overflow,
            preview,
            width,
            insets,
            started: 0,
            duration: if gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations()) {
                120_000
            } else {
                0
            },
            tick: None,
            background: gdk::RGBA::new(
                color[0] as f32 / 255.,
                color[1] as f32 / 255.,
                color[2] as f32 / 255.,
                1.,
            ),
        });
        w.surface.queue_draw();
        true
    }
    pub fn drag_motion(&self, w: &Rc<Workspace>, point: [f32; 2]) {
        let mut active = self.drag.borrow_mut();
        let Some(drag) = active.as_mut() else {
            return;
        };
        let Some(preview) = drag.policy.preview(point) else {
            return;
        };
        let now = w.surface.frame_clock().map_or(0, |c| c.frame_time());
        let progress = drag.progress(now);
        let changed = drag.neighbors.iter().any(|n| {
            n.to != preview
                .geometry
                .items
                .iter()
                .find(|m| m.id == n.id)
                .map(|m| m.bounds)
        });
        if changed {
            for n in &mut drag.neighbors {
                n.from = between(n.from, n.to.unwrap_or(n.from), progress);
                n.to = preview
                    .geometry
                    .items
                    .iter()
                    .find(|m| m.id == n.id)
                    .map(|m| m.bounds);
            }
            drag.started = now;
            if drag.duration > 0 && drag.tick.is_none() {
                drag.tick = Some(w.surface.add_tick_callback(glib::clone!(
                    #[weak]
                    w,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move |surface, clock| {
                        let mut active = w.header.drag.borrow_mut();
                        let Some(drag) = active.as_mut() else {
                            return glib::ControlFlow::Break;
                        };
                        surface.queue_draw();
                        if clock.frame_time() >= drag.started + drag.duration {
                            drag.tick.take();
                            glib::ControlFlow::Break
                        } else {
                            glib::ControlFlow::Continue
                        }
                    }
                )));
            }
        }
        self.drop.set(preview.target);
        drag.preview = preview;
        self.root.queue_draw();
        w.surface.queue_draw();
    }
    pub fn finish_drag(&self, w: &Rc<Workspace>, point: [f32; 2], cancel: bool) {
        let result = self.drag.borrow_mut().as_mut().and_then(|drag| {
            if cancel
                || !self
                    .model
                    .borrow()
                    .as_ref()
                    .is_some_and(|m| drag.policy.is_current(m))
            {
                return None;
            }
            Some((drag.source, drag.policy.preview(point)?.action?))
        });
        self.cancel_drag(w);
        if let Some((source, action)) = result {
            let selected = match source {
                HeaderDragSource::Item(id) => Some(id),
                _ => None,
            };
            self.editor.select(w, selected);
            w.dispatch(action.action());
            if let Some(id) = selected {
                if let Some(item) = self
                    .items
                    .borrow()
                    .iter()
                    .find(|i| i.entry.id == id && i.root.is_child_visible())
                {
                    item.root.grab_focus();
                } else if self.editing.get() {
                    self.root.grab_focus();
                }
            }
        }
    }
    pub fn cancel_drag(&self, w: &Workspace) {
        if let Some(mut drag) = self.drag.take() {
            if let Some(tick) = drag.tick.take() {
                tick.remove();
            }
            drag.held.widget.set_opacity(drag.held.opacity);
            for n in drag.neighbors {
                n.visual.widget.set_opacity(n.visual.opacity);
            }
            for (_, visual) in drag.overflow {
                visual.widget.set_opacity(visual.opacity);
            }
            self.root.queue_allocate();
            w.surface.queue_draw();
        }
        self.clear_drop();
    }
    pub fn snapshot_drag(&self, snapshot: &gtk::Snapshot, now: i64, scale: f32) {
        let active = self.drag.borrow();
        let Some(drag) = active.as_ref() else {
            return;
        };
        let progress = drag.progress(now);
        for n in &drag.neighbors {
            if let Some(to) = n.to {
                draw(&n.visual, between(n.from, to, progress), snapshot, scale);
            }
        }
        for (index, visual) in &drag.overflow {
            if let Some(at) = drag.preview.geometry.overflow[*index] {
                draw(visual, at, snapshot, scale);
            }
        }
        let b = drag.preview.held;
        let shape = gtk::gsk::RoundedRect::from_rect(
            gtk::graphene::Rect::new(b.x, b.y, b.width, b.height),
            b.width.min(b.height) / 2. * crate::squircle::CORNER_FIT,
        );
        snapshot.push_rounded_clip(&shape);
        snapshot.append_color(&drag.background, &shape.bounds());
        snapshot.pop();
        draw(&drag.held, b, snapshot, scale);
        let color = if drag.preview.detached && matches!(drag.source, HeaderDragSource::Item(_)) {
            gdk::RGBA::new(0.95, 0.38, 0.36, 1.)
        } else {
            gdk::RGBA::new(0.35, 0.65, 1., 1.)
        };
        snapshot.append_border(&shape, &[2.; 4], &[color; 4]);
    }
}
