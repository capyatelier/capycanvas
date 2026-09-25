use super::*;

#[derive(Clone)]
pub(crate) struct NativeTabSlide {
    pub bounds: Bounds,
    pub clip: Bounds,
    pub tabs: Vec<SlidingTab>,
    source: usize,
    pub hits: Vec<TabHit>,
    started: Rc<Cell<i64>>,
    duration: i64,
    tick: Rc<RefCell<Option<gtk::TickCallbackId>>>,
}

#[derive(Clone)]
pub(crate) struct SlidingTab {
    pub widget: gtk::Widget,
    opacity: f64,
    node: gtk::gsk::RenderNode,
    pub bounds: Bounds,
    from: f32,
    pub to: f32,
}

impl SlidingTab {
    pub fn capture(
        widget: gtk::Widget,
        surface: &gtk::Widget,
        backing: impl FnOnce(&gtk::Snapshot, &gtk::graphene::Rect),
    ) -> Option<Self> {
        let parent = widget.parent()?;
        let position = parent.compute_point(surface, &gtk::graphene::Point::zero())?;
        let rect = widget.compute_bounds(surface)?;
        let snapshot = gtk::Snapshot::new();
        backing(&snapshot, &rect);
        snapshot.save();
        snapshot.translate(&position);
        parent.snapshot_child(&widget, &snapshot);
        snapshot.restore();
        Some(Self {
            opacity: widget.opacity(),
            widget,
            bounds: bounds(rect),
            node: snapshot.to_node()?,
            from: 0.,
            to: 0.,
        })
    }
}

impl NativeTabSlide {
    pub fn new(tabs: Vec<SlidingTab>, source: usize, clip: Bounds, group: u32) -> Option<Self> {
        let hits = tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| TabHit {
                group,
                index,
                bounds: tab.bounds,
            })
            .collect();
        Some(Self {
            bounds: tabs.get(source)?.bounds,
            clip,
            tabs,
            source,
            hits,
            started: Rc::new(Cell::new(0)),
            duration: if gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations()) {
                120_000
            } else {
                0
            },
            tick: Rc::new(RefCell::new(None)),
        })
    }

    fn progress(&self, now: i64) -> f32 {
        if self.duration == 0 {
            return 1.;
        }
        let t = ((now - self.started.get()) as f32 / self.duration as f32).clamp(0., 1.);
        1. - (1. - t).powi(3)
    }

    pub fn hide(&self) {
        for tab in &self.tabs {
            tab.widget.set_opacity(0.);
        }
    }

    pub fn restore(self) {
        if let Some(tick) = self.tick.borrow_mut().take() {
            tick.remove();
        }
        for tab in self.tabs {
            tab.widget.set_opacity(tab.opacity);
        }
    }

    pub fn retarget(
        &mut self,
        surface: &gtk::Widget,
        bounds: Bounds,
        offset: impl Fn(usize) -> f32,
    ) {
        self.bounds = bounds;
        if (0..self.tabs.len()).all(|index| self.tabs[index].to == offset(index)) {
            return;
        }
        let now = surface.frame_clock().map_or(0, |clock| clock.frame_time());
        let progress = self.progress(now);
        for (index, neighbor) in self.tabs.iter_mut().enumerate() {
            neighbor.from += (neighbor.to - neighbor.from) * progress;
            neighbor.to = offset(index);
        }
        self.started.set(now);
        if self.duration > 0 && self.tick.borrow().is_none() {
            let started = self.started.clone();
            let duration = self.duration;
            let tick = self.tick.clone();
            *self.tick.borrow_mut() = Some(surface.add_tick_callback(move |surface, clock| {
                surface.queue_draw();
                if clock.frame_time() >= started.get() + duration {
                    tick.borrow_mut().take();
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            }));
        }
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
            drag.tab_grab = NativeTabSlide::new(tabs, source, clip, group);
            break;
        }
    }

    pub(super) fn start_tab_slide(&self, drag: &mut NativeWorkspaceDrag) {
        drag.tab = drag.tab_grab.take();
        if let Some(tab) = &drag.tab {
            tab.hide();
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
        tab.retarget(self.surface.upcast_ref(), preview.bounds, |index| {
            preview
                .offsets
                .iter()
                .find(|offset| offset.index == index)
                .map_or(0., |offset| offset.x)
        });
    }

    pub(super) fn clear_tab_slide(&self, drag: &mut NativeWorkspaceDrag) {
        if let Some(tab) = drag.tab.take() {
            tab.restore();
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
    let selected = widget.has_css_class("selected-tool");
    let [r, g, b] = if selected {
        palette.panel
    } else {
        palette.tabbar
    }
    .0;
    let color = gdk::RGBA::new(r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.);
    SlidingTab::capture(widget, surface.upcast_ref(), |snapshot, rect| {
        let radius = SURFACE_RADIUS * crate::squircle::CORNER_FIT;
        let top = gtk::graphene::Size::new(radius, radius);
        let square = gtk::graphene::Size::new(0., 0.);
        snapshot.push_rounded_clip(&gtk::gsk::RoundedRect::new(*rect, top, top, square, square));
        snapshot.append_color(&color, rect);
        snapshot.pop();
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
    })
}
