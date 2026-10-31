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
        pub natural_height: Cell<Option<i32>>,
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
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, if orientation == gtk::Orientation::Vertical {self.natural_height.get().unwrap_or(0)} else {0}, -1, -1)
        }
        fn size_allocate(&self, _: i32, height: i32, _: i32) {
            for (child, bounds) in self
                .children
                .borrow()
                .iter()
                .zip(self.geometry.borrow().iter())
            {
                let (width, height) = if self.natural_height.get().is_some() {
                    let width = bounds.width.max(child.measure(gtk::Orientation::Horizontal, -1).0 as f32);
                    (width, (height as f32).max(child.measure(gtk::Orientation::Vertical, width as i32).0 as f32))
                } else { (bounds.width, height as f32) };
                allocate_at(
                    child,
                    Bounds {
                        width, height,
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
pub(super) struct LayerInspector {
    pub(super) root: gtk::Box,
    pages: [Columns; 2],
    row_height: Cell<f32>,
    target: Cell<Option<(u64, Option<u64>)>>,
    anchor: Rc<Cell<Option<(u64, f32)>>>,
    restore: Rc<RefCell<Option<(gdk::FrameClock, glib::SignalHandlerId)>>>,
    rows: Cell<usize>,
    properties: Cell<bool>,
    pub(super) layers: Rc<LayerPanel>,
    pub(super) effects: Rc<EffectPanels>,
    stack: gtk::Stack,
    pub(super) footer: gtk::Box,
    button: gtk::Button,
    label: gtk::Label,
    summary: gtk::Label,
}
/// A control-height square that centres a compact icon, so disclosure icons
/// line up with the Layers row icons.
pub(super) fn icon_slot(image: &gtk::Image, name: &str) -> gtk::Box {
    let size = layer_ui::PANEL_CONTROL_METRICS.control_height as i32;
    let slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    slot.set_widget_name(name); slot.set_size_request(size, size); slot.set_valign(gtk::Align::Center); slot.set_hexpand(false);
    image.set_pixel_size(16); image.set_halign(gtk::Align::Center); image.set_hexpand(true);
    slot.append(image); slot
}
impl LayerInspector {
    pub(super) fn new(w: &Rc<Workspace>, toggle: UiAction) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_overflow(gtk::Overflow::Hidden);
        let layers = Rc::new(w.layer_panel.sharing(w.localization().clone()));
        layers.bind(w);
        let anchor = Rc::new(Cell::new(None));
        let input = gtk::EventControllerLegacy::new();
        input.set_propagation_phase(gtk::PropagationPhase::Capture);
        input.connect_event(glib::clone!(#[strong] anchor, move |_, event| {
            if matches!(event.event_type(), gdk::EventType::Scroll | gdk::EventType::ButtonPress | gdk::EventType::TouchBegin | gdk::EventType::KeyPress) { anchor.set(None); }
            glib::Propagation::Proceed
        }));
        layers.list.add_controller(input);
        let effects = Rc::new(EffectPanels::with_filter_types(false));
        let stack = gtk::Stack::builder().hhomogeneous(true).vhomogeneous(false).interpolate_size(true).transition_duration(PANEL_EXPANSION_MS).build();
        stack.connect_transition_running_notify(glib::clone!(#[weak] w, move |_| w.surface.queue_allocate()));
        stack.set_vexpand(true);
        stack.set_widget_name("layer-properties-stack");
        margins(&effects.properties, PANEL_CONTENT_INSET as i32);
        let properties = scroll(&effects.properties);
        properties.set_widget_name("drawer-panel-Properties");
        let pages = [layers.root.clone().upcast::<gtk::Widget>(), properties.upcast()].map(|child| {
            let page: Columns = glib::Object::new();
            child.set_parent(&page); page.imp().children.borrow_mut().push(child);
            page.imp().geometry.borrow_mut().push(Bounds::default());
            page
        });
        stack.add_named(&pages[0], Some("layers")); stack.add_named(&pages[1], Some("properties"));
        stack.set_visible_child_name("layers");
        let footer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        footer.add_css_class("layer-properties-footer");
        footer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let button = w.action_button("", toggle);
        button.set_widget_name("layer-properties-disclosure");
        button.add_css_class("flat");
        button.add_css_class("layer-properties-disclosure");
        button.set_height_request(layer_ui::PropertyDisclosure::HEIGHT as i32);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, layer_ui::PANEL_GROUP_SPACING as i32);
        content.append(&icon_slot(&crate::icons::image("layer-properties-symbolic"), "layer-properties-icon-slot"));
        let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        let label = gtk::Label::builder().xalign(0.).ellipsize(gtk::pango::EllipsizeMode::End).width_chars(1).build();
        label.set_widget_name("layer-properties-disclosure-label");
        let summary = gtk::Label::builder().xalign(0.).ellipsize(gtk::pango::EllipsizeMode::End).width_chars(1).build();
        summary.set_widget_name("layer-properties-disclosure-summary");
        summary.add_css_class("dim-label"); summary.add_css_class("caption");
        text.append(&label); text.append(&summary); content.append(&text);
        let chevron = crate::icons::image("layer-chevron-down-symbolic");
        chevron.add_css_class("layer-properties-chevron");
        content.append(&icon_slot(&chevron, "layer-properties-chevron-slot"));
        button.set_child(Some(&content)); footer.append(&button);
        root.append(&stack); root.append(&footer);
        Self {root, pages, row_height:Cell::new(layer_ui::TILE_SIZE), target:Cell::new(None), anchor, restore:Rc::new(RefCell::new(None)), rows:Cell::new(0),
            properties:Cell::new(false), layers, effects, stack, footer, button, label, summary}
    }
    pub(super) fn refresh(&self, w: &Rc<Workspace>, state: &UiState, regions: u32, open: bool) {
        if regions & (regions::DOCUMENT | regions::SETTINGS) != 0 {
            let target = Some((state.document_file.epoch, state.layer_tools.editing_layer.as_ref().map(|layer| layer.id)));
            if self.target.get() != target { self.anchor.set(None); }
            self.layers.refresh(state); self.effects.refresh(w, state);
            if let Some(position) = state.layer_tools.drawer_reveal_row(&state.layers, state.document_file.epoch, self.target.get()) {
                self.layers.reveal_row(position);
            }
            self.target.set(target);
        }
        self.rows.set(state.layers.len());
        let previous = self.properties.replace(open);
        if previous != open {
            if let Some((clock, callback)) = self.restore.borrow_mut().take() { clock.disconnect(callback); }
            if open { self.anchor.set(self.layers.scroll_anchor()); }
            else if self.anchor.get().is_some() { self.restore_anchor(w); }
        }
        let page = if open {"properties"} else {"layers"};
        if self.stack.visible_child_name().as_deref() != Some(page) {
            self.stack.set_visible_child_full(page, if open {gtk::StackTransitionType::SlideUp} else {gtk::StackTransitionType::SlideDown});
        }
        let disclosure = &state.layer_properties.disclosure;
        let label = if open { &disclosure.return_label } else { &disclosure.label };
        self.label.set_label(label);
        self.summary.set_label(&disclosure.summary);
        self.summary.set_visible(!open && !disclosure.summary.is_empty());
        self.button.set_sensitive(state.layer_properties.drawer_toggle_enabled(open));
        self.button.set_tooltip_text(Some(&if !open && !disclosure.summary.is_empty() {format!("{}\n{}", label, disclosure.summary)} else {label.clone()}));
        self.button.update_property(&[gtk::accessible::Property::Label(label), gtk::accessible::Property::Description(if open {""} else {&disclosure.summary})]);
        if open { self.button.add_css_class("open"); } else { self.button.remove_css_class("open"); }
    }
    pub(super) fn measure(&self, width: i32, viewport_height: f32) -> [f32; 2] {
        let mut measurement = self.layers.scroll_measurement(width);
        if measurement.unit_height > 0. { self.row_height.set(measurement.unit_height); }
        else { measurement.unit_height = self.row_height.get(); }
        let properties_height = self.effects.properties.measure(gtk::Orientation::Vertical, width).1 as f32;
        let heights = layer_ui::LayerDrawerSizing::pages(measurement, self.rows.get(), viewport_height, properties_height);
        for (page, height) in self.pages.iter().zip(heights) {
            let height = (height - layer_ui::LayerDrawerSizing::FOOTER_HEIGHT).max(0.).round() as i32;
            if page.imp().natural_height.replace(Some(height)) != Some(height) { page.queue_resize(); }
            let mut geometry = page.imp().geometry.borrow_mut();
            if geometry[0].width != width as f32 { geometry[0].width = width as f32; page.queue_allocate(); }
        }
        heights
    }
    fn restore_anchor(&self, w: &Rc<Workspace>) {
        let Some(clock) = self.root.frame_clock() else { self.anchor.set(None); return; };
        let confirmed = Rc::new(Cell::new(false));
        let callback = clock.connect_after_paint(glib::clone!(#[weak] w, #[weak(rename_to = root)] self.root, #[weak(rename_to = layers)] self.layers,
            #[weak(rename_to = stack)] self.stack, #[weak(rename_to = anchor)] self.anchor, #[weak(rename_to = restore)] self.restore, #[strong] confirmed,
            move |_| {
                let finish = || { anchor.set(None); if let Some((clock, callback)) = restore.borrow_mut().take() { clock.disconnect(callback); } };
                let Some(position) = anchor.get() else { finish(); return; };
                let drawer = w.drawers().into_iter().find(|drawer| drawer.view.borrow().as_ref().is_some_and(|view| root.is_ancestor(&view.root)));
                let Some(drawer) = drawer else { finish(); return; };
                if stack.is_transition_running() || drawer.from.borrow().is_some() { return; }
                let Some(placement) = drawer.target(&w) else { finish(); return; };
                if (root.height() as f32 - placement.bounds.height).abs() > 1. { return; }
                if layers.list.is_mapped() && layers.restore_scroll_anchor(position) {
                    if confirmed.replace(true) { finish(); return; }
                } else { confirmed.set(false); }
                root.queue_draw();
            }));
        *self.restore.borrow_mut() = Some((clock, callback));
    }
}
impl Drop for LayerInspector {
    fn drop(&mut self) {
        if let Some((clock, callback)) = self.restore.borrow_mut().take() { clock.disconnect(callback); }
    }
}

enum Body {
    Toolbar(ToolbarBody),
    Tools(ToolSet),
    Settings(Box<ToolSettings>),
    Color(ColorPanel),
    Palettes(Rc<crate::color_library::PalettePanel>),
    Sizes(crate::tool_panels::SizePanel),
    Layers(Rc<LayerPanel>),
    LayerInspector(LayerInspector),
    Effects(Panel, Rc<EffectPanels>),
    Navigator(crate::navigator::Navigator),
    Proof(Rc<crate::files::proof::ProofPanel>),
    Histogram(Rc<crate::histogram::Inspector>),
}
struct ToolbarBody {
    panel: Panel,
    strip: TileStrip,
    key: RefCell<Option<PanelConfig>>,
    items: RefCell<Vec<toolbar_components::TileWidget>>,
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
            items: RefCell::default(),
        }
    }
    fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        let config = state.workspace.layout.panel(self.panel).unwrap();
        let view = w.gpu.borrow().as_ref().and_then(|g| g.session.panel_view(self.panel).ok());
        let Some(view) = view else { return; };
        if self.key.borrow().as_ref() != Some(config) {
            self.strip.clear();
            self.strip.set_tiles(config.tiles());
            self.strip.set_style(config.tile_style);
            *self.items.borrow_mut() = config.tiles().iter().filter_map(|tile| {
                let choice = &view.tiles.iter().find(|view| view.id == tile.id && view.choice.control == tile.control)?.choice;
                let item = toolbar_components::TileWidget::new(w, config, tile, config.tile_style, choice);
                self.strip.append(&item.root());
                Some(item)
            }).collect();
            *self.key.borrow_mut() = Some(config.clone());
        }
        for (item, tile) in self.items.borrow().iter().zip(&view.tiles) {
            item.refresh(w, tile, tile.component.as_ref());
        }
    }
}
impl Body {
    fn widget(&self) -> gtk::Widget {
        match self {
            Self::Toolbar(v) => v.strip.clone().upcast(),
            Self::Tools(v) => v.root.clone().upcast(),
            Self::Settings(v) => v.root.clone().upcast(),
            Self::Color(v) => v.root.clone().upcast(),
            Self::Palettes(v) => v.root.clone().upcast(),
            Self::Sizes(v) => v.root.clone().upcast(),
            Self::Layers(v) => v.root.clone().upcast(),
            Self::LayerInspector(v) => v.root.clone().upcast(),
            Self::Navigator(v) => v.root.clone().upcast(),
            Self::Proof(v) => v.root.clone().upcast(),
            Self::Histogram(v) => v.root.clone().upcast(),
            Self::Effects(panel, v) => match panel {
                Panel::Adjustments => v.adjustments.clone().upcast(),
                Panel::FilterTypes => v.filter_types.clone().upcast(),
                Panel::Properties => v.properties.clone().upcast(),
                _ => v.stats.clone().upcast(),
            },
        }
    }
    fn refresh(&self, w: &Rc<Workspace>, state: &UiState, regions: u32, tool_set: Option<&ToolSetView>) -> bool {
        let inputs = match self {
            Self::Toolbar(_) => regions::LAYOUT | regions::BRUSH | regions::COMMANDS,
            Self::Tools(_) => regions::BRUSH | regions::SETTINGS | regions::DOCUMENT | regions::COMMANDS,
            Self::Settings(_) => regions::BRUSH | regions::DOCUMENT | regions::COMMANDS,
            Self::Color(_) => regions::COLOR_PREVIEW | regions::BRUSH | regions::DOCUMENT | regions::SETTINGS | regions::COMMANDS,
            Self::Palettes(_) => {
                regions::BRUSH | regions::DOCUMENT | regions::SETTINGS | regions::COMMANDS
            }
            Self::Sizes(_) => regions::BRUSH,
            Self::Layers(_) | Self::Effects(_, _) => regions::DOCUMENT,
            Self::LayerInspector(_) => regions::DOCUMENT | regions::CUSTOMIZATION | regions::SETTINGS | regions::LAYOUT,
            Self::Proof(_) => regions::DOCUMENT | regions::COMMANDS | regions::LAYOUT,
            Self::Histogram(_) => regions::HISTOGRAM | regions::SETTINGS | regions::LAYOUT,
            Self::Navigator(_) => {
                regions::CAMERA | regions::LAYOUT | regions::DOCUMENT | regions::COMMANDS
            }
        };
        if regions & inputs == 0 {
            return false;
        }
        match self {
            Self::Toolbar(v) => v.refresh(w, state),
            Self::Tools(v) if tool_set.is_some() => v.refresh(w, tool_set.unwrap(), state.theme),
            Self::Tools(v) => v.refresh_state(w, state),
            Self::Settings(v) => v.refresh(w, state),
            Self::Color(v) => v.refresh(&state.preview_colors(), w.paint_view_color(), w.picker_headroom()),
            Self::Palettes(v) => v.refresh(state, w.paint_view_color(), w.picker_headroom()),
            Self::Sizes(v) => v.refresh(w, state),
            Self::Layers(v) => v.refresh(state),
            Self::LayerInspector(v) => v.refresh(w, state, regions, state.customization.drawer.as_ref().is_some_and(|d| d.layer_properties == Some(true))),
            Self::Navigator(v) => v.refresh(state),
            Self::Proof(v) => v.refresh(w, state),
            Self::Histogram(v) => v.refresh(w, state),
            Self::Effects(_, v) => v.refresh(w, state),
        }
        true
    }
}
struct View {
    root: Columns,
    shadow: gtk::Box,
    connection: gtk::DrawingArea,
    connection_geometry: Rc<Cell<Option<DrawerConnection>>>,
    columns: Vec<gtk::Box>,
    clips: Vec<gtk::Widget>,
    bodies: Vec<Body>,
    tabs: RefCell<Vec<(Panel, gtk::Button)>>,
    tab_clip: Option<gtk::ScrolledWindow>,
    tab_strip: Option<crate::panel_tabs::PanelTabs>,
}
impl View {
    fn new(w: &Rc<Workspace>, drawer: &ContentDrawer, previous: Option<&Self>) -> Self {
        let connection = previous.map(|view| view.connection.clone()).unwrap_or_default();
        connection.set_widget_name("drawer-connection");
        connection.add_css_class("drawer-connection");
        let connection_geometry = previous.map(|view| view.connection_geometry.clone()).unwrap_or_default();
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
        let root: Columns = previous.map(|view| view.root.clone()).unwrap_or_else(glib::Object::new);
        root.set_widget_name(&match drawer.anchor {
            DrawerAnchor::Header { .. } => "tool-drawer".into(),
            DrawerAnchor::Tile { .. } => "tool-drawer".into(),
            DrawerAnchor::Column { column, .. } => format!("column-drawer-{column}"),
        });
        root.add_css_class("dock-panel");
        root.add_css_class("content-drawer");
        if drawer.tabs.is_some() {
            root.add_css_class("tabbed");
        }

        root.set_overflow(gtk::Overflow::Hidden);
        let shadow = previous.map(|view| view.shadow.clone()).unwrap_or_else(|| gtk::Box::new(gtk::Orientation::Vertical, 0));
        shadow.add_css_class("drawer-shadow");
        shadow.set_widget_name("drawer-shadow");
        shadow.set_can_target(false);
        let mut columns = Vec::new();
        let mut clips = Vec::new();
        let mut bodies = Vec::new();
        let mut effects = None;
        let mut tab_buttons = previous.map(|view| view.tabs.borrow().clone()).unwrap_or_default();
        let mut tab_clip = previous.and_then(|view| view.tab_clip.clone());
        let mut tab_strip = previous.and_then(|view| view.tab_strip.clone());
        for panels in &drawer.columns {
            let column = gtk::Box::new(gtk::Orientation::Vertical, WORKSPACE_SPACING as i32);
            for panel in panels {
                let body = match panel {
                    Panel::Toolbar | Panel::Commands | Panel::CustomToolbar(_) => {
                        Body::Toolbar(ToolbarBody::new(*panel))
                    }
                    Panel::Brushes | Panel::BrushSets | Panel::SculptSets | Panel::Tools => {
                        let v = ToolSet::for_panel(*panel);
                        margins(&v.root, PANEL_CONTENT_INSET as i32);
                        Body::Tools(v)
                    }
                    Panel::ToolSettings => Body::Settings(Box::new(ToolSettings::new())),
                    Panel::Color => {
                        let v = ColorPanel::new();
                        v.bind(w);
                        Body::Color(v)
                    }
                    Panel::Palettes => {
                        let v = crate::color_library::PalettePanel::new();
                        v.bind(w);
                        Body::Palettes(v)
                    }
                    Panel::Proof => Body::Proof(w.proof_panel.duplicate(w)),
                    Panel::Histogram => Body::Histogram(w.histogram.duplicate(w)),
                    Panel::Waveform => Body::Histogram(w.waveform.duplicate(w)),
                    Panel::Sizes => Body::Sizes(crate::tool_panels::SizePanel::new(w)),
                    Panel::Navigator => {
                        let v = crate::navigator::Navigator::new(&w.navigator_overviews);
                        v.bind(w);
                        Body::Navigator(v)
                    }
                    Panel::Layers if drawer.layer_properties.is_some() => Body::LayerInspector(LayerInspector::new(w, UiAction::Customize { action: CustomizationAction::ToggleLayerDrawerProperties })),
                    Panel::Layers => {
                        let v = Rc::new(w.layer_panel.sharing(w.localization().clone()));
                        v.bind(w);
                        v.root.set_height_request(360);
                        Body::Layers(v)
                    }
                    Panel::FilterTypes | Panel::Adjustments | Panel::Properties | Panel::Stats => {
                        let v = effects
                            .get_or_insert_with(|| Rc::new(EffectPanels::with_filter_types(drawer.columns.iter().flatten().any(|p| *p == Panel::FilterTypes))))
                            .clone();
                        if *panel == Panel::Adjustments {
                            v.adjustments.set_height_request(440);
                        }
                        if *panel == Panel::FilterTypes { v.filter_types.set_height_request(440); }
                        Body::Effects(*panel, v)
                    }
                };
                let widget = body.widget();
                if matches!(&body, Body::Histogram(_)) { margins(&widget, PANEL_CONTENT_INSET as i32); }
                widget.set_widget_name(&format!("drawer-panel-{panel:?}"));
                column.append(&widget);
                bodies.push(body);
            }
            let scroller = if drawer.layer_properties.is_some() {
                let widget = bodies.last().unwrap().widget();
                column.remove(&widget);
                widget
            } else {
                let viewport = gtk::Viewport::builder().vscroll_policy(gtk::ScrollablePolicy::Natural).child(&column).build();
                let scroller = scroll(&viewport);
                scroller.downcast_ref::<gtk::ScrolledWindow>().unwrap().vadjustment()
                    .connect_value_changed(glib::clone!(#[weak] w, move |_| w.surface.queue_allocate()));
                scroller
            };
            clips.push(scroller.clone());
            let child = if let Some(previous) = previous {
                let child = previous.root.imp().children.borrow()[columns.len()].clone();
                let container = child.downcast_ref::<gtk::Box>().unwrap();
                container.remove(&container.last_child().unwrap());
                scroller.add_css_class("panel-body");
                scroller.set_vexpand(true);
                container.append(&scroller);
                child
            } else if let Some(tabs) = &drawer.tabs {
                let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
                let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                header.add_css_class("dock-tabs");
                header.set_height_request(TAB_BAR_HEIGHT as i32);
                let labels = crate::panel_tabs::PanelTabs::new();
                w.install_panel_drag(&header, DockItem::Group { group: tabs.group });
                w.install_context(
                    &header,
                    layer_ui::ContextTarget::Group { group: tabs.group },
                );
                let layout = w.surface.imp().layout.borrow();
                for panel in &tabs.panels {
                    let config = layout.panel(*panel).unwrap();
                    let presentation = layout.tab_presentation(*panel);
                    let button = w.panel_tab_button(&config.title_localized(&w.localization()), tabs.group, *panel);
                    button.add_css_class("flat");
                    button.set_widget_name(&format!("column-drawer-tab-{panel:?}"));
                    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    let icon = w.customization.icon(w, config.icon(), 16);
                    icon.set_visible(presentation.show_icon);
                    content.append(&icon);
                    let label = gtk::Label::new(None);
                    label.set_visible(presentation.show_name);
                    content.append(&label);
                    if !presentation.show_name {
                        button.add_css_class("icon-only-tab");
                    }
                    button.set_child(Some(&content));
                    let panel = *panel;
                    w.on_localization(glib::clone!(#[weak] w, #[weak] button, #[weak] label, #[upgrade_or] false, move |localization| {
                        let Ok(title) = w.surface.imp().layout.borrow().panel(panel).map(|config| config.title_localized(localization)) else { return false };
                        label.set_label(&title);
                        button.set_tooltip_text(Some(&title));
                        button.update_property(&[gtk::accessible::Property::Label(&title)]);
                        true
                    }));
                    selected(&button, tabs.active == panel);
                    w.install_panel_drag(&button, DockItem::Panel { panel });
                    w.install_context(&button, layer_ui::ContextTarget::Panel { panel });
                    labels.append(&button);
                    tab_buttons.push((panel, button));
                }
                labels.set_hover_bottom(tabs.panels.iter().map(|panel| layout.tab_presentation(*panel).hover_bottom));
                let header_clip = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk::PolicyType::External)
                    .vscrollbar_policy(gtk::PolicyType::Never)
                    .hexpand(true)
                    .child(&labels)
                    .build());
                header.append(&header_clip);
                tab_clip = Some(header_clip);
                tab_strip = Some(labels);
                let grip = tiles::grip(&w.localization().text(layer_ui::MessageId::DOCUMENTS_DELIVERY_DRAG_PANEL));
                grip.set_widget_name("column-drawer-grip");
                grip.set_size_request(20, 24);
                grip.set_valign(gtk::Align::Center);
                header.append(&crate::panel_tabs::strip_end(&grip));
                container.append(&crate::panel_tabs::bar(&header));
                scroller.add_css_class("panel-body");
                scroller.set_vexpand(true);
                container.append(&scroller);
                container.upcast::<gtk::Widget>()
            } else {
                scroller
            };
            if previous.is_none() {
                child.set_parent(&root);
                root.imp().children.borrow_mut().push(child);
            }
            columns.push(column);
        }

        Self {
            root,
            shadow,
            connection,
            connection_geometry,
            columns,
            clips,
            bodies,
            tabs: RefCell::new(tab_buttons),
            tab_clip,
            tab_strip,
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
    source_corners: RefCell<Vec<(gtk::Widget, [bool; 4])>>,
    generation: Cell<u64>,
}
impl Drawer {
    pub fn refresh_theme(&self) {
        if let Some(tabs) = self.view.borrow().as_ref().and_then(|view| view.tab_strip.as_ref()) {
            tabs.queue_draw();
        }
    }
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
            source_corners: RefCell::default(),
            generation: Cell::new(0),
        })
    }
    pub fn layers(&self) -> Option<Rc<LayerPanel>> {
        self.view.borrow().as_ref()?.bodies.iter().find_map(|b| {
            if let Body::Layers(v) = b {
                Some(v.clone())
            } else if let Body::LayerInspector(v) = b {
                Some(v.layers.clone())
            } else {
                None
            }
        })
    }
    pub fn effects(&self) -> Option<Rc<EffectPanels>> {
        self.view.borrow().as_ref()?.bodies.iter().find_map(|b| {
            if let Body::Effects(_, v) = b {
                Some(v.clone())
            } else if let Body::LayerInspector(v) = b {
                Some(v.effects.clone())
            } else {
                None
            }
        })
    }
    pub fn placement(&self) -> Option<DrawerPlacement> {
        self.presented.borrow().clone()
    }
    pub fn measurement(&self) -> Option<layer_ui::ColumnDrawerMeasurement> {
        if self.closing.get() {
            return None;
        }
        let state = self.state.borrow();
        let DrawerAnchor::Column { group, .. } = state.as_ref()?.anchor else {
            return None;
        };
        let bounds = self.placement()?.bounds;
        (bounds.width > 0. && bounds.height > 0.)
            .then_some(layer_ui::ColumnDrawerMeasurement { group, bounds })
    }
    pub fn tab_widths(&self,panel:Panel)->Option<[f32;2]> {
        let view=self.view.borrow();let view=view.as_ref()?;
        let tabs=view.tabs.borrow();
        tabs.iter().find(|(id,_)|*id==panel).map(|(_,button)|crate::panel_tabs::tab_widths(button))
    }
    pub fn tab_hits(&self, w: &Workspace) -> Vec<TabHit> {
        let Some(measurement) = self.measurement() else {
            return Vec::new();
        };
        let view = self.view.borrow();
        let Some(view) = view.as_ref() else {
            return Vec::new();
        };
        let Some(clip) = view
            .tab_clip
            .as_ref()
            .and_then(|c| c.compute_bounds(&w.surface))
        else {
            return Vec::new();
        };
        let clip = Bounds {
            x: clip.x(),
            y: clip.y(),
            width: clip.width(),
            height: clip.height(),
        };
        view.tabs
            .borrow()
            .iter()
            .enumerate()
            .filter_map(|(index, (_, tab))| {
                let b = tab.compute_bounds(&w.surface)?;
                let bounds = Bounds {
                    x: b.x(),
                    y: b.y(),
                    width: b.width(),
                    height: b.height(),
                }
                .intersection(clip)?
                .intersection(measurement.bounds)?;
                Some(TabHit {
                    group: measurement.group,
                    index,
                    bounds,
                })
            })
            .collect()
    }
    pub fn tile_measurements(&self, w: &Workspace, out: &mut Vec<DrawerTileMeasurement>) {
        if self.closing.get() {
            return;
        }
        let view = self.view.borrow();
        let Some(view) = view.as_ref() else { return };
        let Some(placement) = self.placement() else {
            return;
        };
        for body in &view.bodies {
            let Body::Toolbar(bar) = body else { continue };
            let Some(clip) = view.clips.iter().find(|c| bar.strip.is_ancestor(*c)) else {
                continue;
            };
            let Some(clip) = clip.compute_bounds(&w.surface) else {
                continue;
            };
            let clip = Bounds {
                x: clip.x(),
                y: clip.y(),
                width: clip.width(),
                height: clip.height(),
            };
            let Some(clip) = clip.intersection(placement.bounds) else {
                continue;
            };
            let key = bar.key.borrow();
            let Some(config) = key.as_ref() else { continue };
            for (tile, item) in config.tiles().iter().zip(bar.items.borrow().iter()) {
                if !item.root().is_mapped() {
                    continue;
                }
                let target = item.root();
                let Some(b) = target.compute_bounds(&w.surface) else {
                    continue;
                };
                let Some(bounds) = (Bounds {
                    x: b.x(),
                    y: b.y(),
                    width: b.width(),
                    height: b.height(),
                })
                .intersection(clip) else {
                    continue;
                };
                out.push(DrawerTileMeasurement {
                    column: self.id,
                    anchor: TileAnchor {
                        panel: bar.panel,
                        tile: tile.id,
                    },
                    bounds,
                });
            }
        }
    }
    pub fn tile_button(&self, anchor: TileAnchor) -> Option<gtk::Button> {
        let view = self.view.borrow();
        view.as_ref()?.bodies.iter().find_map(|body| {
            let Body::Toolbar(bar) = body else {
                return None;
            };
            if bar.panel != anchor.panel {
                return None;
            }
            let key = bar.key.borrow();
            let index = key
                .as_ref()?
                .tiles()
                .iter()
                .position(|t| t.id == anchor.tile)?;
            bar.items.borrow().get(index).map(|item| item.button())
        })
    }
    pub fn mark_tile_origin(&self, origin: Option<(TileAnchor, Edge)>) {
        let view = self.view.borrow();
        let Some(view) = view.as_ref() else { return };
        for body in &view.bodies {
            let Body::Toolbar(bar) = body else { continue };
            if origin.is_some_and(|(a, _)| a.panel == bar.panel) {
                bar.strip.add_css_class("drawer-source");
            } else {
                bar.strip.remove_css_class("drawer-source");
            }
            let key = bar.key.borrow();
            let Some(config) = key.as_ref() else { continue };
            for (tile, item) in config.tiles().iter().zip(bar.items.borrow().iter()) {
                customization::drawer_origin(
                    &item.button(),
                    origin
                        .filter(|(a, _)| a.panel == bar.panel && a.tile == tile.id)
                        .map(|(_, edge)| edge),
                );
            }
        }
    }
    pub fn is_closed(&self) -> bool {
        self.state.borrow().is_none()
    }
    fn source_button(&self, w: &Workspace) -> Option<gtk::Button> {
        let anchor = self.state.borrow().as_ref()?.anchor;
        match anchor {
            DrawerAnchor::Header { id } => w.header.drawer_button(id),
            DrawerAnchor::Tile { panel, tile } => {
                w.columns
                    .drawers
                    .borrow()
                    .iter()
                    .find_map(|d| d.tile_button(TileAnchor { panel, tile }))
            }
            .or_else(|| w.customization.drawer_button(TileAnchor { panel, tile })),
            DrawerAnchor::Column { column, origin, .. } => w.columns.button(column, origin),
        }
    }
    pub fn source_container(&self, w: &Workspace) -> Option<gtk::Widget> {
        let mut widget: gtk::Widget = self.source_button(w)?.upcast();
        while let Some(parent) = widget.parent() {
            if parent == w.surface.clone().upcast::<gtk::Widget>() {
                return Some(widget);
            }
            widget = parent;
        }
        None
    }
    fn target(&self, w: &Workspace) -> Option<DrawerPlacement> {
        let state = self.state.borrow();
        let state = state.as_ref()?;
        let view = self.view.borrow();
        let view = view.as_ref()?;
        let layout = w.surface.imp().layout.borrow();
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        let place = |heights: &[f32]| {
            let gpu = w.gpu.borrow();
            let ui = gpu.as_ref()?.session.state();
            ui.customization
                .drawer_placement(state, &layout, viewport, heights)
        };
        let sizing = place(&vec![0.0; view.columns.len()])?;
        for body in &view.bodies {
            if let Body::LayerInspector(inspector) = body { inspector.measure(sizing.columns[0].width as i32, viewport[1]); }
        }
        if let (Some(panel), Some(column)) = (
            view.bodies.iter().find_map(|body| if let Body::Palettes(panel) = body { Some(panel) } else { None }),
            state.columns.iter().position(|panels| panels.contains(&Panel::Palettes)),
        ) { panel.content_measurement(sizing.columns[column].width as i32); }

        let heights: Vec<_> = view
            .columns
            .iter()
            .zip(&sizing.columns)
            .map(|(body, b)| {
                view.bodies.iter().find_map(|body| if let Body::LayerInspector(v) = body {Some(v.root.measure(gtk::Orientation::Vertical, b.width as i32).1 as f32)} else {None})
                    .unwrap_or_else(|| body.measure(gtk::Orientation::Vertical, b.width as i32).1 as f32)
                    + if state.tabs.is_some() {
                        TAB_BAR_HEIGHT
                    } else {
                        0.
                    }
            })
            .collect();
        place(&heights)
    }
    pub fn geometry(&self, w: &Workspace) -> Option<DrawerPlacement> {
        let Some(target) = self
            .target(w)
            .or_else(|| self.closing.get().then(|| self.placement()).flatten())
        else {
            // A scrolled-out origin has no visible child or chrome hit region.
            if self.state.borrow().is_some() {
                self.mark_origin(w, None);
            }
            self.presented.borrow_mut().take();
            return None;
        };
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
            if let Some(strip)=&view.tab_strip {strip.set_presentations(&result.tab_presentations);}
            let geometry = result.columns.clone();
            if *view.root.imp().geometry.borrow() != geometry {
                *view.root.imp().geometry.borrow_mut() = geometry;
                // GTK can skip size_allocate when the outer rectangle is
                // unchanged. Internal panel splits still need live allocation.
                view.root.queue_allocate();
            }
            let connection = result.connection();
            view.connection_geometry.set(connection);
            view.connection.queue_draw();
            let corners = connection.map_or([false; 4], |c| c.square_corners);
            customization::square_corners(&view.root, corners);
            customization::square_corners(&view.shadow, corners);
        }
        self.mark_origin(w, Some(&result));
        *self.presented.borrow_mut() = Some(result.clone());
        Some(result)
    }
    fn mark_origin(&self, w: &Workspace, placement: Option<&DrawerPlacement>) {
        let placement = placement.filter(|p| p.connection().is_some());
        self.mark_source_corners(w, placement);
        let anchor = self.state.borrow().as_ref().map(|s| s.anchor);
        let direction = placement.map(|p| p.direction);
        if self.id != 0 {
            let origin = anchor
                .and_then(|a| match a {
                    DrawerAnchor::Column { origin, .. } => Some(origin),
                    _ => None,
                })
                .zip(direction);
            w.columns.mark_drawer_origin(self.id, origin);
            return;
        }
        let header = anchor
            .and_then(|a| match a {
                DrawerAnchor::Header { id } => Some(id),
                _ => None,
            })
            .zip(direction);
        w.header.mark_drawer_origin(header);
        let origin = anchor.and_then(DrawerAnchor::tile).zip(direction);
        w.customization.mark_drawer_origin(origin);
        for parent in w.columns.drawers.borrow().iter() {
            parent.mark_tile_origin(origin);
        }
    }
    fn mark_source_corners(&self, w: &Workspace, placement: Option<&DrawerPlacement>) {
        let mut next = Vec::new();
        if let Some(placement) = placement {
            let mut ancestor = self.source_button(w).and_then(|b| b.parent());
            while let Some(widget) = ancestor {
                if widget == w.surface.clone().upcast::<gtk::Widget>() {
                    break;
                }
                if let Some(b) = widget.compute_bounds(&w.surface) {
                    let corners = placement.source_corners(Bounds {
                        x: b.x(),
                        y: b.y(),
                        width: b.width(),
                        height: b.height(),
                    });
                    if corners.into_iter().any(|c| c) {
                        next.push((widget.clone(), corners));
                    }
                }
                ancestor = widget.parent();
            }
        }
        let mut old = self.source_corners.borrow_mut();
        if *old == next {
            return;
        }
        let classes = [
            "drawer-source-nw",
            "drawer-source-ne",
            "drawer-source-se",
            "drawer-source-sw",
        ];
        for (widget, _) in old.iter() {
            for class in classes {
                widget.remove_css_class(class);
            }
        }
        for (widget, corners) in &next {
            for (joined, class) in corners.iter().zip(classes) {
                if *joined {
                    widget.add_css_class(class);
                }
            }
        }
        *old = next;
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
                    drawer.retire(&w);
                }
                w.surface.queue_allocate();
            }
        ));
        *self.animation.borrow_mut() = Some(animation.clone());
        animation.play();
    }
    fn retire(&self, w: &Rc<Workspace>) {
        if let Some(animation) = self.animation.take() { animation.pause(); }
        w.surface
            .remove_slots(|slot| matches!(slot, Slot::Drawer(id) | Slot::DrawerConnection(id) | Slot::DrawerShadow(id) if id == self.id));
        self.mark_origin(w, None);
        self.view.borrow_mut().take();
        self.state.borrow_mut().take();
        self.presented.borrow_mut().take();
    }
    fn finish_animation(&self) {
        if let Some(animation) = self.animation.take() { animation.pause(); }
        self.from.borrow_mut().take();
        self.closing.set(false);
        self.progress.set(1.0);
    }
    pub fn refresh_color_preview(&self, colors: &layer_ui::ColorState, display: crate::display_color::ViewColor, headroom: f32, preview: bool) {
        if let Some(view) = self.view.borrow().as_ref() {
            for body in &view.bodies {
                if let Body::Color(panel) = body && panel.root.is_mapped() {
                    if preview { panel.refresh_preview(colors, display, headroom); }
                    else { panel.refresh(colors, display, headroom); }
                }
            }
        }
    }
    pub fn refresh(
        self: &Rc<Self>,
        w: &Rc<Workspace>,
        state: &UiState,
        regions: u32,
        next: Option<&ContentDrawer>,
    ) {
        let replaced = self.generation.replace(state.toolbar_context_generation) != state.toolbar_context_generation;
        let Some(next) = next else {
            if replaced && self.state.borrow().is_some() {
                self.retire(w);
                w.surface.queue_allocate();
            } else if self.state.borrow().is_some() && !self.closing.get() {
                self.animate(w, true);
            }
            return;
        };
        let changed = self.state.borrow().as_ref() != Some(next) || self.closing.get();
        let switching = self
            .state
            .borrow()
            .as_ref()
            .is_some_and(|old| old.anchor != next.anchor);
        let same_tabs = |old: &ContentDrawer| match (&old.tabs, &next.tabs) {
            (Some(old), Some(next)) => old.group == next.group
                && (old.panels == next.panels || old.panels.len() == next.panels.len()
                    && old.panels.iter().all(|panel| next.panels.contains(panel))),
            (None, None) => true,
            _ => false,
        };
        let rebuild = self
            .state
            .borrow()
            .as_ref()
            .is_none_or(|old| old.columns != next.columns || !same_tabs(old) || old.layer_properties.is_some() != next.layer_properties.is_some());
        if rebuild {
            let previous = self.view.borrow().clone();
            let retain = previous.as_ref().filter(|view| {
                view.root.imp().children.borrow().len() == next.columns.len()
                    && next.tabs.is_some() && self.state.borrow().as_ref().is_some_and(same_tabs)
            });
            if retain.is_none() { w.surface
                .remove_slots(|slot| matches!(slot, Slot::Drawer(id) | Slot::DrawerConnection(id) | Slot::DrawerShadow(id) if id == self.id));
            }
            let view = Rc::new(View::new(w, next, retain.map(|view| view.as_ref())));
            if retain.is_none() {
            w.surface.add(Slot::DrawerShadow(self.id), &view.shadow);
            w.surface.add(Slot::Drawer(self.id), &view.root);
            w.surface
                .add(Slot::DrawerConnection(self.id), &view.connection);
            }
            *self.view.borrow_mut() = Some(view);
        }
        *self.state.borrow_mut() = Some(next.clone());
        self.closing.set(false);
        let mut refreshed = false;
        let mut rows_changed = false;
        if let Some(view) = self.view.borrow().as_ref() {
            if let Some(tabs) = &next.tabs {
                let layout = w.surface.imp().layout.borrow();
                let mut buttons = view.tabs.borrow_mut();
                if !buttons.iter().map(|(panel, _)| panel).eq(&tabs.panels) {
                    buttons.sort_unstable_by_key(|(panel, _)| tabs.panels.iter().position(|p| p == panel));
                    if let Some(strip) = &view.tab_strip {
                        strip.reorder(buttons.iter().map(|(_, button)| button));
                    }
                }
                for (panel, button) in buttons.iter() {
                    selected(button, *panel == tabs.active);
                }
                if let Some(labels) = view.tab_strip.as_ref() {
                    labels.set_hover_bottom(tabs.panels.iter().map(|panel| layout.tab_presentation(*panel).hover_bottom));
                    if changed { labels.queue_resize(); labels.queue_draw(); }
                }
            }
            for body in &view.bodies {
                if let Body::LayerInspector(inspector) = body {
                    rows_changed |= !rebuild && inspector.rows.get() != state.layers.len();
                }
                refreshed |= body.refresh(w, state, if rebuild || changed { regions::ALL } else { regions }, next.tool_set.as_ref());
            }
        }
        if !changed && !refreshed && regions & regions::LAYOUT == 0 {
            return;
        }
        // Change opener styles before GTK allocates/snapshots the toolbar.
        // Different tiles can share one retained drawer body, so body rebuilds
        // and size changes cannot be used as a proxy for opener changes.
        if switching {
            // Move the body with its new opener, keeping the connector present
            // throughout a switch, including a nested toolbar in a column drawer.
            self.finish_animation();
        }
        if changed {
            self.mark_origin(w, self.target(w).as_ref());
        }
        let resized = self.progress.get() >= 1.0
            && self
                .target(w)
                .zip(self.placement())
                .is_some_and(|(to, from)| to.bounds != from.bounds);
        if !switching && (changed || resized || rows_changed) {
            if next.layer_properties.is_some() && !rebuild { self.finish_animation(); }
            else { self.animate(w, false); }
        }
        w.surface.raise_drawer(self.id);
        w.surface.queue_allocate();
    }
}
