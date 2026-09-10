//! Native projection of core-owned Zen sections. Entering Zen never reparents
//! or edits saved panels; tool actions are shared with ordinary toolbar tiles.
use super::*;

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Sections {
        pub children: RefCell<Vec<(Bounds, TileStrip)>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Sections {
        const NAME: &'static str = "CapyZenSections";
        type Type = super::Sections;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Sections {
        fn dispose(&self) {
            for (_, child) in self.children.take() {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for Sections {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }
        fn contains(&self, x: f64, y: f64) -> bool {
            self.children
                .borrow()
                .iter()
                .any(|(b, _)| b.contains(x as f32, y as f32))
        }
        fn size_allocate(&self, _: i32, _: i32, _: i32) {
            for (b, child) in self.children.borrow().iter() {
                allocate_at(child.upcast_ref(), *b);
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            for (_, child) in self.children.borrow().iter() {
                self.obj().snapshot_child(child, snapshot);
            }
        }
    }
}
glib::wrapper! {
    pub struct Sections(ObjectSubclass<imp::Sections>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[derive(Default)]
pub(super) struct Zen {
    root: RefCell<Option<Sections>>,
    key: RefCell<Vec<SectionKey>>,
    buttons: RefCell<Vec<(TileAnchor, gtk::Button)>>,
    palette: gtk::CssProvider,
    active: Cell<bool>,
}
type SectionKey = (Panel, Edge, TileStyle, Vec<u32>);
impl Zen {
    pub fn drawer_button(&self, anchor: TileAnchor) -> Option<gtk::Button> {
        self.active
            .get()
            .then(|| {
                self.buttons
                    .borrow()
                    .iter()
                    .find(|(a, _)| *a == anchor)
                    .map(|(_, b)| b.clone())
            })
            .flatten()
    }
    pub fn mark_drawer_origin(&self, origin: Option<(TileAnchor, &DrawerPlacement)>) {
        for (anchor, button) in self.buttons.borrow().iter() {
            customization::drawer_origin(
                button,
                origin
                    .filter(|(a, _)| a == anchor)
                    .map(|(_, p)| p.direction),
            );
        }
        if let Some(root) = self.root.borrow().as_ref() {
            for ((bounds, strip), (panel, _, _, tiles)) in root
                .imp()
                .children
                .borrow()
                .iter()
                .zip(self.key.borrow().iter())
            {
                let source = origin.filter(|(a, _)| a.panel == *panel && tiles.contains(&a.tile));
                if source.is_some() {
                    strip.add_css_class("drawer-source");
                } else {
                    strip.remove_css_class("drawer-source");
                }
                let corners = source.map_or([false; 4], |(_, p)| p.source_corners(*bounds));
                for (joined, class) in corners
                    .into_iter()
                    .zip(["join-nw", "join-ne", "join-se", "join-sw"])
                {
                    if joined {
                        strip.add_css_class(class);
                    } else {
                        strip.remove_css_class(class);
                    }
                }
            }
        }
    }
    pub fn allocate(&self, layout: &DockLayout, viewport: [f32; 2]) {
        if !self.active.get() {
            return;
        }
        let Some(root) = self.root.borrow().clone() else {
            return;
        };
        let model = layout.zen_toolbars(viewport);
        for ((bounds, strip), section) in root
            .imp()
            .children
            .borrow_mut()
            .iter_mut()
            .zip(model.sections)
        {
            *bounds = section.bounds;
            strip.set_projection(TileLayout {
                tiles: section.tiles.into_iter().map(|(_, b)| b).collect(),
                grip: None,
                insertion: Vec::new(),
            });
        }
    }
    pub fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        if self.active.replace(state.partial_zen()) != state.partial_zen() {
            w.surface.queue_allocate();
        }
        if !self.active.get() {
            return;
        }
        let current = self.root.borrow().clone();
        let root = current.unwrap_or_else(|| {
            let root: Sections = glib::Object::new();
            root.set_widget_name("zen-toolbars");
            w.surface.add(Slot::ZenToolbars, &root);
            *self.root.borrow_mut() = Some(root.clone());
            root
        });
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        let model = state.workspace.layout.zen_toolbars(viewport);
        let key: Vec<_> = model
            .sections
            .iter()
            .map(|s| {
                (
                    s.panel,
                    s.edge,
                    s.style,
                    s.tiles.iter().map(|t| t.0).collect(),
                )
            })
            .collect();
        if *self.key.borrow() != key {
            for (_, child) in root.imp().children.take() {
                child.unparent();
            }
            self.buttons.borrow_mut().clear();
            for section in &model.sections {
                let config = state.workspace.layout.panel(section.panel).unwrap();
                let strip = TileStrip::new();
                strip.add_css_class("dock-panel");
                strip.add_css_class("toolbar-controls");
                strip.add_css_class("zen-section");
                strip.set_style(section.style);
                strip.configure(
                    if matches!(section.edge, Edge::Left | Edge::Right) {
                        Axis::Vertical
                    } else {
                        Axis::Horizontal
                    },
                    true,
                );
                for (id, _) in &section.tiles {
                    let tile = config.tiles().iter().find(|t| t.id == *id).unwrap();
                    let button = customization::tile_button(w, config, tile, &self.palette);
                    button.set_widget_name(&format!("zen-tile-{id}"));
                    strip.append(&button);
                    self.buttons.borrow_mut().push((
                        TileAnchor {
                            panel: section.panel,
                            tile: *id,
                        },
                        button,
                    ));
                }
                strip.set_parent(&root);
                root.imp()
                    .children
                    .borrow_mut()
                    .push((section.bounds, strip));
            }
            *self.key.borrow_mut() = key;
            w.surface.queue_allocate();
        }
        self.allocate(&state.workspace.layout, viewport);
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().unwrap().session;
        for config in state
            .workspace
            .layout
            .panels
            .iter()
            .filter(|p| p.id.kind() == PanelKind::Tiles)
        {
            let Ok(view) = session.panel_view(config.id) else {
                continue;
            };
            for (anchor, button) in self
                .buttons
                .borrow()
                .iter()
                .filter(|(a, _)| a.panel == config.id)
            {
                if let Some(tile) = view.tiles.iter().find(|t| t.id == anchor.tile) {
                    selected(button, tile.choice.selected);
                    button.set_sensitive(tile.enabled);
                    button.set_tooltip_text(Some(&tile.tooltip));
                }
            }
        }
        self.palette.load_from_string(&format!(
            ".brush-color {{ -gtk-icon-palette: success {}; }}",
            w.color.rgba()
        ));
        w.surface.raise_drawer();
    }
}
