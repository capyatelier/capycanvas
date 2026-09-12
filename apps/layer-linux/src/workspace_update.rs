//! Shared Rust publication, applied on GTK's display clock. Retained widgets
//! keep their native render nodes; moving allocations also moves picking,
//! clipping, resize handles and Navigator projection without scaling textures.
use super::*;

#[derive(Default)]
pub(super) struct Publication {
    pub model_revision: Cell<Option<u64>>,
    pub content_revision: Cell<Option<u64>>,
    layout_pending: Cell<bool>,
    pending: RefCell<Option<WorkspaceUpdate>>,
    current: RefCell<Option<WorkspaceUpdate>>,
    tick: RefCell<Option<gtk::TickCallbackId>>,
    placement: RefCell<Option<(u32, Bounds, Vec<(gtk::Widget, Bounds)>)>>,
    pub hits: RefCell<Option<Vec<TabHit>>>,
    #[cfg(test)]
    pub refreshes: Cell<usize>,
    #[cfg(test)]
    pub frames: RefCell<Vec<(i64, f64)>>,
    #[cfg(test)]
    pub inputs: RefCell<Vec<f64>>,
}

impl Workspace {
    pub(super) fn reset_workspace_publication(&self) {
        self.publication.layout_pending.set(false);
        self.publication.pending.borrow_mut().take();
        self.publication.current.borrow_mut().take();
        self.publication.placement.borrow_mut().take();
        self.publication.hits.borrow_mut().take();
    }

    pub(super) fn publish_workspace_layout(self: &Rc<Self>, update: WorkspaceUpdate) {
        self.publication.layout_pending.set(true);
        *self.publication.pending.borrow_mut() = Some(update);
        self.queue_workspace_frame();
    }

    pub(super) fn publish_workspace(self: &Rc<Self>, update: WorkspaceUpdate) {
        if self.publication.model_revision.get() != Some(update.model_revision) {
            return; // A nested measurement/host action established newer models.
        }
        let ended = update.drag.is_none();
        *self.publication.pending.borrow_mut() = Some(update);
        if ended {
            // Completion/cancellation cannot be overtaken by a queued frame.
            if let Some(tick) = self.publication.tick.borrow_mut().take() {
                tick.remove();
            }
            self.present_workspace();
        } else {
            self.queue_workspace_frame();
        }
    }

    fn queue_workspace_frame(self: &Rc<Self>) {
        if self.publication.tick.borrow().is_none() {
            let weak = Rc::downgrade(self);
            let tick = self.surface.add_tick_callback(move |_, _| {
                if let Some(w) = weak.upgrade() {
                    w.publication.tick.borrow_mut().take();
                    w.present_workspace();
                }
                glib::ControlFlow::Break
            });
            *self.publication.tick.borrow_mut() = Some(tick);
        }
    }

    fn present_workspace(self: &Rc<Self>) {
        let Some(update) = self.publication.pending.borrow_mut().take() else {
            return;
        };
        if self.publication.layout_pending.replace(false) {
            let gpu = self.gpu.borrow();
            let Some(gpu) = gpu.as_ref() else {
                return;
            };
            if self.publication.content_revision.get() != Some(update.content_revision)
                || gpu.session.workspace_model_revision() != update.model_revision
            {
                return;
            }
            // Keep widgets and their native render resources. GTK measures and
            // reflows them at the new allocation in this same frame.
            let source = &gpu.session.state().workspace.layout;
            let mut layout = self.surface.imp().layout.borrow_mut();
            layout.bands.clone_from(&source.bands);
            layout.floating.clone_from(&source.floating);
            layout.collapsed.clone_from(&source.collapsed);
            layout.fit_tab_groups.clone_from(&source.fit_tab_groups);
            layout.measurements.clone_from(&source.measurements);
            drop(layout);
            self.navigator.refresh(gpu.session.state());
            self.publication
                .model_revision
                .set(Some(update.model_revision));
            self.surface.queue_allocate();
            return;
        }
        if self.publication.model_revision.get() != Some(update.model_revision) {
            return;
        }
        #[cfg(test)]
        let start = std::time::Instant::now();
        let native_drag = self.workspace_drag.borrow().clone();
        if let Some(mut drag) = native_drag {
            self.update_tab_slide(&mut drag, update.drag.as_ref().and_then(|d| d.tab.as_ref()));
            *self.workspace_drag.borrow_mut() = Some(drag);
        }
        *self.drop_hint.borrow_mut() = update.drag.as_ref().and_then(|d| d.drop_hint.clone());
        *self.publication.current.borrow_mut() = Some(update);
        self.allocate_workspace_motion();
        self.surface.queue_draw();
        #[cfg(test)]
        self.publication.frames.borrow_mut().push((
            self.surface.frame_clock().map_or(0, |c| c.frame_time()),
            start.elapsed().as_secs_f64() * 1000.,
        ));
    }

    pub(super) fn allocate_workspace_motion(&self) {
        let current = self.publication.current.borrow();
        let Some(group) = current
            .as_ref()
            .and_then(|u| u.drag.as_ref())
            .and_then(|d| d.group.as_ref())
        else {
            return;
        };
        let mut placement = self.publication.placement.borrow_mut();
        if placement.as_ref().is_none_or(|(id, _, _)| *id != group.id) {
            let resolved = self.resolved();
            let Some(base) = resolved.groups.iter().find(|g| g.id == group.id) else {
                return;
            };
            let children = self
                .surface
                .imp()
                .children
                .borrow()
                .iter()
                .filter_map(|(slot, widget)| {
                    let bounds = match slot {
                        Slot::Group(id) if *id == group.id => base.bounds,
                        Slot::FloatingResize(id, edge) if *id == group.id => {
                            base.resize_handles.iter().find(|h| h.edge == *edge)?.bounds
                        }
                        _ => return None,
                    };
                    Some((widget.clone(), bounds))
                })
                .collect();
            *placement = Some((group.id, base.bounds, children));
        }
        let (_, base, children) = placement.as_ref().unwrap();
        let scale = self.surface.scale_factor() as f32;
        let x = (group.bounds.x * scale).round() / scale;
        let y = (group.bounds.y * scale).round() / scale;
        for (widget, bounds) in children {
            allocate_at(
                widget,
                Bounds {
                    x: x + bounds.x - base.x,
                    y: y + bounds.y - base.y,
                    ..*bounds
                },
            );
        }
    }
}
