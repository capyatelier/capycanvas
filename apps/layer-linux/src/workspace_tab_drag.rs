use super::*;

#[derive(Clone)]
pub(super) struct NativeTabSlide {
    pub bounds: Bounds,
    pub clip: Bounds,
    pub tabs: Vec<SlidingTab>,
    source: usize,
    pub hits: Vec<TabHit>,
    started: i64,
    duration: i64,
    tick: Rc<RefCell<Option<gtk::TickCallbackId>>>,
}

#[derive(Clone)]
pub(super) struct SlidingTab {
    pub widget: gtk::Widget,
    opacity: f64,
    node: gtk::gsk::RenderNode,
    bounds: Bounds,
    from: f32,
    pub to: f32,
}

impl NativeTabSlide {
    fn progress(&self, now: i64) -> f32 {
        if self.duration == 0 {
            return 1.;
        }
        let t = ((now - self.started) as f32 / self.duration as f32).clamp(0., 1.);
        1. - (1. - t).powi(3)
    }

    pub fn snapshot(&self, snapshot: &gtk::Snapshot, now: i64, scale: f32) {
        snapshot.push_clip(&gtk::graphene::Rect::new(
            self.clip.x,
            self.clip.y,
            self.clip.width,
            self.clip.height,
        ));
        let progress = self.progress(now);
        // Neighbors animate below the tab held by the pointer.
        for index in (0..self.tabs.len())
            .filter(|i| *i != self.source)
            .chain(std::iter::once(self.source))
        {
            let tab = &self.tabs[index];
            let offset = if index == self.source {
                self.bounds.x - tab.bounds.x
            } else {
                tab.from + (tab.to - tab.from) * progress
            };
            snapshot.save();
            snapshot.translate(&gtk::graphene::Point::new(
                (offset * scale).round() / scale,
                0.,
            ));
            snapshot.append_node(&tab.node);
            snapshot.restore();
        }
        snapshot.pop();
    }
}

impl Workspace {
    pub(super) fn grab_tab_slide(self: &Rc<Self>, drag: &mut NativeWorkspaceDrag) {
        let DragTarget::Dock(DockItem::Panel { panel }) = drag.target else {
            return;
        };
        let Some((group, palette)) = self.gpu.borrow().as_ref().and_then(|g| {
            Some((
                g.session.state().workspace.layout.panel_group(panel)?,
                g.session.state().palette,
            ))
        }) else {
            return;
        };
        let mut picked = self.surface.pick(
            drag.origin[0] as f64,
            drag.origin[1] as f64,
            gtk::PickFlags::DEFAULT,
        );
        while let Some(widget) = picked {
            picked = widget.parent();
            if !widget.is::<gtk::Button>() {
                continue;
            }
            let Some(parent) = widget.parent() else {
                return;
            };
            let Some(clip) = tab_clip(&widget, &self.surface) else {
                return;
            };
            let mut child = parent.first_child();
            let mut tabs = Vec::new();
            while let Some(tab) = child {
                child = tab.next_sibling();
                if let Some(tab) = snapshot_tab(tab, &self.surface, palette) {
                    tabs.push(tab);
                }
            }
            let Some(source) = tabs.iter().position(|t| t.widget == widget) else {
                return;
            };
            let hits = tabs
                .iter()
                .enumerate()
                .map(|(index, tab)| TabHit {
                    group,
                    index,
                    bounds: tab.bounds,
                })
                .collect();
            let duration = if gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations())
            {
                120_000
            } else {
                0
            };
            drag.tab_grab = Some(NativeTabSlide {
                bounds: tabs[source].bounds,
                clip,
                tabs,
                source,
                hits,
                started: 0,
                duration,
                tick: Rc::new(RefCell::new(None)),
            });
            break;
        }
    }

    pub(super) fn start_tab_slide(&self, drag: &mut NativeWorkspaceDrag) {
        drag.tab = drag.tab_grab.take();
        if let Some(tab) = &drag.tab {
            for tab in &tab.tabs {
                tab.widget.set_opacity(0.);
            }
            self.queue_tab_joins();
        }
    }

    pub(super) fn update_tab_slide(
        self: &Rc<Self>,
        drag: &mut NativeWorkspaceDrag,
        presentation: Option<&WorkspaceTabPresentation>,
    ) {
        let Some(tab) = drag.tab.as_mut() else { return };
        let Some(presentation) = presentation else {
            self.clear_tab_slide(drag);
            return;
        };
        let preview = &presentation.preview;
        tab.bounds = preview.bounds;
        if preview
            .offsets
            .iter()
            .any(|offset| tab.tabs[offset.index].to != offset.x)
        {
            let now = self
                .surface
                .frame_clock()
                .map_or(0, |clock| clock.frame_time());
            let progress = tab.progress(now);
            for offset in &preview.offsets {
                let neighbor = &mut tab.tabs[offset.index];
                neighbor.from += (neighbor.to - neighbor.from) * progress;
                neighbor.to = offset.x;
            }
            tab.started = now;
            if tab.duration > 0 && tab.tick.borrow().is_none() {
                let weak = Rc::downgrade(self);
                let tick = self.surface.add_tick_callback(move |surface, clock| {
                    let Some(w) = weak.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    let drag = w.workspace_drag.borrow();
                    let Some(tab) = drag.as_ref().and_then(|d| d.tab.as_ref()) else {
                        return glib::ControlFlow::Break;
                    };
                    surface.queue_draw();
                    if clock.frame_time() >= tab.started + tab.duration {
                        tab.tick.borrow_mut().take();
                        glib::ControlFlow::Break
                    } else {
                        glib::ControlFlow::Continue
                    }
                });
                *tab.tick.borrow_mut() = Some(tick);
            }
        }
    }

    pub(super) fn clear_tab_slide(&self, drag: &mut NativeWorkspaceDrag) {
        if let Some(tab) = drag.tab.take() {
            if let Some(tick) = tab.tick.borrow_mut().take() {
                tick.remove();
            }
            for tab in tab.tabs {
                tab.widget.set_opacity(tab.opacity);
            }
            self.queue_tab_joins();
            self.surface.queue_draw();
        }
    }

    fn queue_tab_joins(&self) {
        for group in self.groups.borrow().iter() {
            group.tab_joins.queue_draw();
        }
    }
}

pub(super) fn tab_clip(tab: &gtk::Widget, surface: &DockSurface) -> Option<Bounds> {
    let mut ancestor = tab.parent();
    let mut clip = None;
    while let Some(node) = ancestor {
        if node.has_css_class("dock-tabs") {
            return clip;
        }
        if node.is::<gtk::ScrolledWindow>() {
            clip = node.compute_bounds(surface).map(bounds);
        }
        ancestor = node.parent();
    }
    None
}

fn bounds(b: gtk::graphene::Rect) -> Bounds {
    Bounds {
        x: b.x(),
        y: b.y(),
        width: b.width(),
        height: b.height(),
    }
}

fn snapshot_tab(
    widget: gtk::Widget,
    surface: &DockSurface,
    palette: ThemePalette,
) -> Option<SlidingTab> {
    let parent = widget.parent()?;
    let position = parent.compute_point(surface, &gtk::graphene::Point::zero())?;
    let rect = widget.compute_bounds(surface)?;
    let selected = widget.has_css_class("selected-tool");
    let [r, g, b] = if selected {
        palette.panel
    } else {
        palette.tabbar
    }
    .0;
    let color = gdk::RGBA::new(r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.);
    let snapshot = gtk::Snapshot::new();
    snapshot.push_rounded_clip(&gtk::gsk::RoundedRect::from_rect(rect, 6.));
    snapshot.append_color(&color, &rect);
    snapshot.pop();
    snapshot.save();
    snapshot.translate(&position);
    parent.snapshot_child(&widget, &snapshot);
    snapshot.restore();
    if selected {
        let cr = snapshot.append_cairo(&gtk::graphene::Rect::new(
            rect.x() - 6.,
            rect.y(),
            rect.width() + 12.,
            rect.height(),
        ));
        cr.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            1.,
        );
        let y = (rect.y() + rect.height()) as f64;
        concave_foot(&cr, rect.x() as f64, y, 6., -1.);
        concave_foot(&cr, (rect.x() + rect.width()) as f64, y, 6., 1.);
        let _ = cr.fill();
    }
    Some(SlidingTab {
        opacity: widget.opacity(),
        widget,
        bounds: bounds(rect),
        node: snapshot.to_node()?,
        from: 0.,
        to: 0.,
    })
}
