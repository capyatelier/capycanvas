//! Native content projections. State, panel composition and positioning are in
//! layer-ui; this adapter supplies measurements and the usual expansion animation.
use super::*;
use crate::{
    effects::EffectPanels,
    layers::LayerPanel,
    tool_panels::{ColorPanel, ToolSet, ToolSettings},
};

mod allocation {
    use super::*;
    #[derive(Default)]
    pub struct Columns {
        pub children: RefCell<Vec<gtk::Widget>>,
        pub geometry: RefCell<Vec<Bounds>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Columns {
        const NAME: &'static str = "CapyContentDrawer";
        type Type = super::Columns;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Columns {
        fn dispose(&self) {
            for child in self.children.take() {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for Columns {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }
        fn size_allocate(&self, _: i32, height: i32, _: i32) {
            for (child, bounds) in self
                .children
                .borrow()
                .iter()
                .zip(self.geometry.borrow().iter())
            {
                allocate_at(
                    child,
                    Bounds {
                        height: height as f32,
                        ..*bounds
                    },
                );
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            for child in self.children.borrow().iter() {
                self.obj().snapshot_child(child, snapshot);
            }
        }
    }
}
glib::wrapper! {
    pub struct Columns(ObjectSubclass<allocation::Columns>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

enum Body {
    Toolbar(ToolbarBody),
    Tools(ToolSet),
    Settings(ToolSettings),
    Color(ColorPanel),
    Sizes(crate::tool_panels::SizePanel),
    Layers(Rc<LayerPanel>),
    Effects(Panel, Rc<EffectPanels>),
    Navigator(crate::navigator::Navigator),
}
struct ToolbarBody {
    panel: Panel,
    strip: TileStrip,
    key: RefCell<Option<PanelConfig>>,
    buttons: RefCell<Vec<gtk::Button>>,
    palette: gtk::CssProvider,
}
impl ToolbarBody {
    fn new(panel: Panel) -> Self {
        let strip = TileStrip::new();
        strip.add_css_class("toolbar-controls");
        strip.configure(Axis::Vertical, false);
        Self {
            panel,
            strip,
            key: RefCell::default(),
            buttons: RefCell::default(),
            palette: gtk::CssProvider::new(),
        }
    }
    fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        let config = state.workspace.layout.panel(self.panel).unwrap();
        if self.key.borrow().as_ref() != Some(config) {
            self.strip.clear();
            self.strip.set_tiles(config.tiles());
            self.strip.set_style(config.tile_style);
            let buttons: Vec<_> = config
                .tiles()
                .iter()
                .map(|t| {
                    let b = customization::tile_button(w, config, t, &self.palette);
                    self.strip.append(&b);
                    b
                })
                .collect();
            *self.buttons.borrow_mut() = buttons;
            *self.key.borrow_mut() = Some(config.clone());
        }
        if let Some(view) = w
            .gpu
            .borrow()
            .as_ref()
            .and_then(|g| g.session.panel_view(self.panel).ok())
        {
            for (button, tile) in self.buttons.borrow().iter().zip(view.tiles) {
                selected(button, tile.choice.selected);
                button.set_sensitive(tile.enabled);
                button.set_tooltip_text(Some(&tile.tooltip));
            }
        }
        self.palette.load_from_string(&format!(
            ".brush-color {{ -gtk-icon-palette: success {}; }}",
            w.color.rgba()
        ));
    }
}
impl Body {
    fn widget(&self) -> gtk::Widget {
        match self {
            Self::Toolbar(v) => v.strip.clone().upcast(),
            Self::Tools(v) => v.root.clone().upcast(),
            Self::Settings(v) => v.root.clone().upcast(),
            Self::Color(v) => v.root.clone().upcast(),
            Self::Sizes(v) => v.root.clone().upcast(),
            Self::Layers(v) => v.root.clone().upcast(),
            Self::Navigator(v) => v.root.clone().upcast(),
            Self::Effects(panel, v) => match panel {
                Panel::Adjustments => v.adjustments.clone().upcast(),
                Panel::Properties => v.properties.clone().upcast(),
                _ => v.stats.clone().upcast(),
            },
        }
    }
    fn refresh(&self, w: &Rc<Workspace>, state: &UiState, regions: u32) -> bool {
        let inputs = match self {
            Self::Toolbar(_) => regions::LAYOUT | regions::BRUSH | regions::COMMANDS,
            Self::Tools(_) => regions::BRUSH | regions::SETTINGS | regions::DOCUMENT,
            Self::Settings(_) => regions::BRUSH | regions::DOCUMENT | regions::COMMANDS,
            Self::Color(_) | Self::Sizes(_) => regions::BRUSH,
            Self::Layers(_) | Self::Effects(_, _) => regions::DOCUMENT,
            Self::Navigator(_) => {
                regions::CAMERA | regions::LAYOUT | regions::DOCUMENT | regions::COMMANDS
            }
        };
        if regions & inputs == 0 {
            return false;
        }
        match self {
            Self::Toolbar(v) => v.refresh(w, state),
            Self::Tools(v) => v.refresh(w, &state.tool_set, state.theme),
            Self::Settings(v) => v.refresh(w, state),
            Self::Color(v) => v.refresh(&state.colors),
            Self::Sizes(v) => v.refresh(&state.brush),
            Self::Layers(v) => v.refresh(state),
            Self::Navigator(v) => v.refresh(state),
            Self::Effects(_, v) => v.refresh(w, state),
        }
        true
    }
}
struct View {
    root: Columns,
    connection: gtk::DrawingArea,
    connection_geometry: Rc<Cell<Option<DrawerConnection>>>,
    columns: Vec<gtk::Box>,
    bodies: Vec<Body>,
}
impl View {
    fn new(w: &Rc<Workspace>, drawer: &ContentDrawer) -> Self {
        let connection = gtk::DrawingArea::new();
        connection.set_widget_name("drawer-connection");
        connection.add_css_class("drawer-connection");
        let connection_geometry = Rc::new(Cell::new(None::<DrawerConnection>));
        let geometry = connection_geometry.clone();
        connection.set_draw_func(move |area, cr, _, _| {
            let Some(c) = geometry.get() else {
                return;
            };
            let color = area.color();
            cr.set_source_rgba(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                color.alpha().into(),
            );
            let [xx, yx, xy, yy, x, y] = c.transform.map(f64::from);
            cr.transform(gtk::cairo::Matrix::new(xx, yx, xy, yy, x, y));
            cr.rectangle(0.0, 0.0, c.length.into(), c.depth.into());
            concave_foot(cr, 0.0, c.depth.into(), c.radii[0].into(), -1.0);
            concave_foot(cr, c.length.into(), c.depth.into(), c.radii[1].into(), 1.0);
            let _ = cr.fill();
        });
        let root: Columns = glib::Object::new();
        root.set_widget_name(&match drawer.anchor {
            DrawerAnchor::Tile { .. } => "tool-drawer".into(),
            DrawerAnchor::Column { column, .. } => format!("column-drawer-{column}"),
        });
        root.add_css_class("dock-panel");
        root.add_css_class("content-drawer");
        root.set_overflow(gtk::Overflow::Hidden);
        let mut columns = Vec::new();
        let mut bodies = Vec::new();
        let mut effects = None;
        for panels in &drawer.columns {
            let column = gtk::Box::new(gtk::Orientation::Vertical, WORKSPACE_SPACING as i32);
            for panel in panels {
                let body = match panel {
                    Panel::Toolbar | Panel::CustomToolbar(_) => {
                        Body::Toolbar(ToolbarBody::new(*panel))
                    }
                    Panel::Brushes => {
                        let v = ToolSet::new();
                        margins(&v.root, PANEL_CONTENT_INSET as i32);
                        Body::Tools(v)
                    }
                    Panel::ToolSettings => Body::Settings(ToolSettings::new()),
                    Panel::Color => {
                        let v = ColorPanel::new();
                        v.bind(w);
                        Body::Color(v)
                    }
                    Panel::Sizes => Body::Sizes(crate::tool_panels::SizePanel::new(w)),
                    Panel::Navigator => {
                        let v = crate::navigator::Navigator::new(&w.navigator_images);
                        v.bind(w);
                        Body::Navigator(v)
                    }
                    Panel::Layers => {
                        let v = Rc::new(LayerPanel::new());
                        v.bind(w);
                        v.root.set_height_request(360);
                        Body::Layers(v)
                    }
                    Panel::Adjustments | Panel::Properties | Panel::Stats => {
                        let v = effects
                            .get_or_insert_with(|| Rc::new(EffectPanels::new()))
                            .clone();
                        if *panel == Panel::Adjustments {
                            v.adjustments.set_height_request(440);
                        }
                        Body::Effects(*panel, v)
                    }
                };
                let widget = body.widget();
                widget.set_widget_name(&format!("drawer-panel-{panel:?}"));
                column.append(&widget);
                bodies.push(body);
            }
            let scroller = scroll(&column);
            let child = if let Some(tabs) = &drawer.tabs {
                let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
                let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                header.add_css_class("dock-tabs");
                header.set_height_request(TAB_BAR_HEIGHT as i32);
                let layout = w.surface.imp().layout.borrow();
                for panel in &tabs.panels {
                    let config = layout.panel(*panel).unwrap();
                    let presentation = layout.tab_presentation(*panel);
                    let button = w.action_button(
                        config.title(),
                        UiAction::SelectPanelTab {
                            group: tabs.group,
                            panel: *panel,
                        },
                    );
                    button.add_css_class("flat");
                    button.set_widget_name(&format!("column-drawer-tab-{panel:?}"));
                    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    if presentation.show_icon {
                        content.append(&gtk::Image::from_icon_name(&format!(
                            "layer-{}-symbolic",
                            config.icon()
                        )));
                    }
                    if presentation.show_name {
                        let label = gtk::Label::new(Some(config.title()));
                        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                        content.append(&label);
                    } else {
                        button.add_css_class("icon-only-tab");
                    }
                    button.set_child(Some(&content));
                    selected(&button, tabs.active == *panel);
                    header.append(&button);
                }
                let header_clip = gtk::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk::PolicyType::External)
                    .vscrollbar_policy(gtk::PolicyType::Never)
                    .child(&header)
                    .build();
                container.append(&header_clip);
                scroller.set_vexpand(true);
                container.append(&scroller);
                container.upcast::<gtk::Widget>()
            } else {
                scroller
            };
            child.set_parent(&root);
            root.imp().children.borrow_mut().push(child);
            columns.push(column);
        }
        Self {
            root,
            connection,
            connection_geometry,
            columns,
            bodies,
        }
    }
}

pub(crate) struct Drawer {
    pub id: u32,
    state: RefCell<Option<ContentDrawer>>,
    view: RefCell<Option<Rc<View>>>,
    presented: RefCell<Option<DrawerPlacement>>,
    from: RefCell<Option<DrawerPlacement>>,
    progress: Cell<f32>,
    closing: Cell<bool>,
    animation: RefCell<Option<adw::TimedAnimation>>,
}
impl Drawer {
    pub fn new(id: u32) -> Rc<Self> {
        Rc::new(Self {
            id,
            state: RefCell::default(),
            view: RefCell::default(),
            presented: RefCell::default(),
            from: RefCell::default(),
            progress: Cell::new(1.0),
            closing: Cell::new(false),
            animation: RefCell::default(),
        })
    }
    pub fn layers(&self) -> Option<Rc<LayerPanel>> {
        self.view.borrow().as_ref()?.bodies.iter().find_map(|b| {
            if let Body::Layers(v) = b {
                Some(v.clone())
            } else {
                None
            }
        })
    }
    pub fn effects(&self) -> Option<Rc<EffectPanels>> {
        self.view.borrow().as_ref()?.bodies.iter().find_map(|b| {
            if let Body::Effects(_, v) = b {
                Some(v.clone())
            } else {
                None
            }
        })
    }
    pub fn placement(&self) -> Option<DrawerPlacement> {
        self.presented.borrow().clone()
    }
    pub fn is_closed(&self) -> bool {
        self.state.borrow().is_none()
    }
    pub fn snapshot_origin(&self, w: &Workspace, snapshot: &gtk::Snapshot) {
        let Some(anchor) = self.state.borrow().as_ref().map(|s| s.anchor) else {
            return;
        };
        let Some(button) = (match anchor {
            DrawerAnchor::Tile { panel, tile } => w
                .zen
                .drawer_button(TileAnchor { panel, tile })
                .or_else(|| w.customization.drawer_button(TileAnchor { panel, tile })),
            DrawerAnchor::Column { column, origin, .. } => w.columns.button(column, origin),
        }) else {
            return;
        };
        let Some(parent) = button.parent() else {
            return;
        };
        let Some(position) = parent.compute_point(&w.surface, &gtk::graphene::Point::new(0.0, 0.0))
        else {
            return;
        };
        // Reuse GTK's cached native button node above the drawer shadow. No
        // reparenting, texture copies or duplicate input widget are involved.
        snapshot.save();
        snapshot.translate(&position);
        parent.snapshot_child(&button, snapshot);
        snapshot.restore();
    }
    fn target(&self, w: &Workspace) -> Option<DrawerPlacement> {
        let state = self.state.borrow();
        let state = state.as_ref()?;
        let view = self.view.borrow();
        let view = view.as_ref()?;
        let layout = w.surface.imp().layout.borrow();
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        let partial = w
            .gpu
            .borrow()
            .as_ref()
            .is_some_and(|g| g.session.state().partial_zen());
        let sizing = state.placement(&layout, viewport, &vec![0.0; view.columns.len()], partial)?;
        let heights: Vec<_> = view
            .columns
            .iter()
            .zip(&sizing.columns)
            .map(|(body, b)| {
                body.measure(gtk::Orientation::Vertical, b.width as i32).1 as f32
                    + if state.tabs.is_some() {
                        TAB_BAR_HEIGHT
                    } else {
                        0.
                    }
            })
            .collect();
        state.placement(&layout, viewport, &heights, partial)
    }
    pub fn geometry(&self, w: &Workspace) -> Option<DrawerPlacement> {
        let target = self
            .target(w)
            .or_else(|| self.closing.get().then(|| self.placement()).flatten())?;
        let end = if self.closing.get() {
            target.closed()
        } else {
            target
        };
        let result = self.from.borrow().as_ref().map_or_else(
            || end.clone(),
            |from| end.interpolate_from(from, self.progress.get()),
        );
        if let Some(view) = self.view.borrow().as_ref() {
            *view.root.imp().geometry.borrow_mut() = result.columns.clone();
            let connection = result.connection();
            view.connection_geometry.set(connection);
            view.connection.queue_draw();
            for (index, class) in ["join-nw", "join-ne", "join-se", "join-sw"]
                .into_iter()
                .enumerate()
            {
                if connection.is_some_and(|c| c.square_corners[index]) {
                    view.root.add_css_class(class);
                } else {
                    view.root.remove_css_class(class);
                }
            }
        }
        let origin = self
            .state
            .borrow()
            .as_ref()
            .and_then(|s| s.anchor.tile().map(|a| (a, result.direction)));
        if self.id == 0 {
            w.customization.mark_drawer_origin(origin);
            w.zen
                .mark_drawer_origin(origin.map(|(anchor, _)| (anchor, &result)));
        }
        *self.presented.borrow_mut() = Some(result.clone());
        Some(result)
    }
    fn animate(self: &Rc<Self>, w: &Rc<Workspace>, closing: bool) {
        if let Some(animation) = self.animation.take() {
            animation.pause();
        }
        *self.from.borrow_mut() = self
            .placement()
            .or_else(|| self.target(w).map(|p| p.closed()));
        self.closing.set(closing);
        self.progress.set(0.0);
        let target = adw::CallbackAnimationTarget::new(glib::clone!(
            #[weak]
            w,
            #[weak(rename_to=drawer)]
            self,
            move |v| {
                drawer.progress.set(v as f32);
                w.surface.queue_allocate();
            }
        ));
        let animation = adw::TimedAnimation::new(&w.surface, 0.0, 1.0, PANEL_EXPANSION_MS, target);
        animation.set_easing(adw::Easing::EaseOutCubic);
        animation.connect_done(glib::clone!(
            #[weak]
            w,
            #[weak(rename_to=drawer)]
            self,
            move |_| {
                drawer.from.borrow_mut().take();
                if drawer.closing.get() {
                    w.surface
                        .remove_slots(|slot| matches!(slot, Slot::Drawer(id) | Slot::DrawerConnection(id) if id == drawer.id));
                    if drawer.id == 0 {
                        w.customization.mark_drawer_origin(None);
                        w.zen.mark_drawer_origin(None);
                    }
                    drawer.view.borrow_mut().take();
                    drawer.state.borrow_mut().take();
                    drawer.presented.borrow_mut().take();
                }
                w.surface.queue_allocate();
            }
        ));
        *self.animation.borrow_mut() = Some(animation.clone());
        animation.play();
    }
    pub fn refresh(
        self: &Rc<Self>,
        w: &Rc<Workspace>,
        state: &UiState,
        regions: u32,
        next: Option<&ContentDrawer>,
    ) {
        let Some(next) = next else {
            if self.state.borrow().is_some() && !self.closing.get() {
                self.animate(w, true);
            }
            return;
        };
        let changed = self.state.borrow().as_ref() != Some(next) || self.closing.get();
        let rebuild = self
            .state
            .borrow()
            .as_ref()
            .is_none_or(|old| old.columns != next.columns || old.tabs != next.tabs);
        if rebuild {
            w.surface
                .remove_slots(|slot| matches!(slot, Slot::Drawer(id) | Slot::DrawerConnection(id) if id == self.id));
            let view = Rc::new(View::new(w, next));
            w.surface.add(Slot::Drawer(self.id), &view.root);
            w.surface
                .add(Slot::DrawerConnection(self.id), &view.connection);
            *self.view.borrow_mut() = Some(view);
        }
        *self.state.borrow_mut() = Some(next.clone());
        let mut refreshed = false;
        if let Some(view) = self.view.borrow().as_ref() {
            for body in &view.bodies {
                refreshed |= body.refresh(w, state, if rebuild { regions::ALL } else { regions });
            }
        }
        if !changed && !refreshed && regions & regions::LAYOUT == 0 {
            return;
        }
        let resized = self.progress.get() >= 1.0
            && self
                .target(w)
                .zip(self.placement())
                .is_some_and(|(to, from)| to.bounds != from.bounds);
        if changed || resized {
            self.animate(w, false);
        }
        w.surface.raise_drawer(self.id);
        w.surface.queue_allocate();
    }
}
