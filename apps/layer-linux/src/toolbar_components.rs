//! Retained GTK toolbar editors. Binding, context and fitting policy is shared.
use super::*;
use crate::number_control::NumberControl;

/// One retained item for both docked strips and toolbar drawers. The root owns
/// layout/hit bounds; the opener anchors drawers and popup arrows.
pub(super) enum TileWidget {
    Button {
        root: gtk::Widget,
        button: gtk::Button,
    },
    Component(Rc<Component>),
}
impl TileWidget {
    pub fn new(w: &Rc<Workspace>, config: &PanelConfig, tile: &ToolbarTile) -> Self {
        if tile.control.is_component() {
            return Self::Component(Component::new(w, config.id, tile));
        }
        let button = customization::tile_button(w, config, tile);
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        button.add_css_class("tile-button");
        button.set_hexpand(true);
        button.set_vexpand(true);
        root.append(&button);
        w.install_panel_drag(
            &root,
            DockItem::Tile {
                panel: config.id,
                tile: tile.id,
            },
        );
        w.install_context(
            &root,
            ContextTarget::Tile {
                panel: config.id,
                tile: tile.id,
            },
        );
        Self::Button {
            root: root.upcast(),
            button,
        }
    }
    pub fn root(&self) -> gtk::Widget {
        match self {
            Self::Button { root, .. } => root.clone(),
            Self::Component(c) => c.root.clone().upcast(),
        }
    }
    pub fn button(&self) -> gtk::Button {
        match self {
            Self::Button { button, .. } => button.clone(),
            Self::Component(c) => c.button.clone(),
        }
    }
    pub fn refresh(&self, w: &Rc<Workspace>, tile: &TileView, view: Option<&ToolbarComponentView>) {
        match self {
            Self::Component(c) => {
                if let Some(view) = view {
                    c.refresh(w, view);
                }
            }
            Self::Button { button, .. } => {
                selected(button, tile.choice.selected);
                button.set_sensitive(tile.enabled);
                button.set_tooltip_text(Some(&tile.tooltip));
                if let Some(image) = button
                    .child()
                    .and_then(|child| {
                        if child.is::<gtk::Box>() {
                            child.first_child()
                        } else {
                            Some(child)
                        }
                    })
                    .and_downcast::<gtk::Image>()
                {
                    let name = format!("layer-{}-symbolic", tile.choice.icon);
                    if crate::icons::name(&image).as_deref() != Some(&name) {
                        crate::icons::set(&image, Some(&name));
                    }
                }
            }
        }
    }
}

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct ComponentBody {
        pub children: RefCell<Vec<gtk::Widget>>,
        pub vertical: Cell<bool>,
        pub slider: Cell<bool>,
        pub opacity: Cell<bool>,
        pub bookmarks: RefCell<Vec<SliderBookmark>>,
        pub bookmark_selected: Cell<[u8; 3]>,
        pub style: Cell<TileStyle>,
        pub options: Cell<ToolOptionsStyle>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for ComponentBody {
        const NAME: &'static str = "CapyToolbarComponent";
        type Type = super::ComponentBody;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for ComponentBody {
        fn dispose(&self) {
            for child in self.children.take() {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for ComponentBody {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }
        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let children = self.children.borrow();
            if children.is_empty() {
                return;
            }
            let axis = if self.vertical.get() {
                Axis::Vertical
            } else {
                Axis::Horizontal
            };
            let allocate = |child: &gtk::Widget, b: Bounds| {
                let visible = b.width >= 1.0 && b.height >= 1.0;
                child.set_child_visible(visible);
                if visible {
                    let mut editor = None;
                    let mut content = child.first_child();
                    while let Some(w) = content {
                        if let Some(number) = w.downcast_ref::<NumberControl>() {
                            number.fit_width(b.width.floor() as i32);
                            editor = Some(number.clone());
                        }
                        content = w.next_sibling();
                    }
                    child.allocate(
                        b.width.floor() as i32,
                        b.height.floor() as i32,
                        -1,
                        Some(
                            gtk::gsk::Transform::new()
                                .translate(&gtk::graphene::Point::new(b.x, b.y)),
                        ),
                    );
                    if let Some(editor) = editor {
                        editor.present_popover();
                    }
                }
            };
            if self.slider.get() {
                for (child, b) in
                    children
                        .iter()
                        .zip(toolbar_slider_layout(width as f32, height as f32, axis))
                {
                    allocate(child, b);
                }
            } else {
                let sizes: Vec<_> = children[1..]
                    .iter()
                    .map(|w| {
                        if w.has_css_class("option-segments") {
                            let row = w.downcast_ref::<gtk::Box>().unwrap();
                            let tile = self.style.get().size();
                            let mut count = 0.;
                            let mut child = row.first_child();
                            while let Some(button) = child {
                                count += 1.;
                                child = button.next_sibling();
                            }
                            // Keep connected choices together; narrow side bars
                            // stack them, wide toolboxes can retain the row.
                            let stacked = self.vertical.get() && (width as f32) < tile[0] * count;
                            row.set_orientation(if stacked {
                                gtk::Orientation::Vertical
                            } else {
                                gtk::Orientation::Horizontal
                            });
                            if self.vertical.get() {
                                [width as f32, tile[1] * if stacked { count } else { 1. }]
                            } else {
                                [tile[0] * count, tile[1]]
                            }
                        } else if w.has_css_class("option-action") || self.vertical.get() {
                            self.style.get().size()
                        } else {
                            [
                                w.measure(gtk::Orientation::Horizontal, -1).1 as f32,
                                w.measure(gtk::Orientation::Vertical, -1).1 as f32,
                            ]
                        }
                    })
                    .collect();
                let button = self.style.get().size();
                let layout = tool_options_layout(
                    width as f32,
                    height as f32,
                    axis,
                    &sizes,
                    button,
                    if self.vertical.get() { 2. } else { 10. },
                );
                allocate(&children[0], layout.more);
                for (child, b) in children[1..].iter().zip(layout.fields) {
                    if let Some(b) = b {
                        allocate(child, b);
                    } else {
                        if self
                            .obj()
                            .root()
                            .and_downcast::<gtk::Window>()
                            .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w))
                            .is_some_and(|focus| focus == *child || focus.is_ancestor(child))
                        {
                            if let Some(button) = children[0].first_child() {
                                button.grab_focus();
                            }
                        }
                        child.set_child_visible(false);
                    }
                }
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if self.slider.get() {
                if let Some(scale) = self
                    .children
                    .borrow()
                    .get(1)
                    .and_then(|w| w.downcast_ref::<gtk::Scale>())
                {
                    paint_track(
                        &self.obj(),
                        scale,
                        snapshot,
                        self.opacity.get(),
                        self.vertical.get(),
                    );
                }
            }
            for child in self
                .children
                .borrow()
                .iter()
                .filter(|c| c.is_child_visible())
            {
                self.obj().snapshot_child(child, snapshot);
            }
            if self.slider.get() {
                paint_bookmarks(&self.obj(), snapshot, &self.bookmarks.borrow());
            }
        }
    }
}
glib::wrapper! {
    pub struct ComponentBody(ObjectSubclass<imp::ComponentBody>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl ComponentBody {
    fn append(&self, child: &impl IsA<gtk::Widget>) {
        child.set_parent(self);
        self.imp()
            .children
            .borrow_mut()
            .push(child.clone().upcast());
    }
    pub(crate) fn set_presentation(&self, axis: Axis, style: TileStyle) {
        let vertical = axis == Axis::Vertical;
        let old_axis = self.imp().vertical.replace(vertical);
        let old_style = self.imp().style.replace(style);
        if old_axis == vertical && old_style == style {
            return;
        }
        crate::tiles::set_size_class(self, style, "component");
        if vertical {
            self.add_css_class("vertical-component");
        } else {
            self.remove_css_class("vertical-component");
        }
        if self.imp().slider.get() {
            if let Some(scale) = self
                .imp()
                .children
                .borrow()
                .get(1)
                .and_then(|w| w.downcast_ref::<gtk::Scale>())
            {
                scale.set_orientation(if vertical {
                    gtk::Orientation::Vertical
                } else {
                    gtk::Orientation::Horizontal
                });
                scale.set_inverted(vertical);
            }
        }
        self.update_option_axis();
        self.queue_allocate();
    }
    fn update_option_axis(&self) {
        let vertical = self.imp().vertical.get();
        let labeled = matches!(
            self.imp().style.get(),
            TileStyle::MediumLabeled | TileStyle::Labeled
        );
        let text = !vertical && self.imp().options.get().text;
        if vertical && labeled {
            self.add_css_class("labeled-component");
        } else {
            self.remove_css_class("labeled-component");
        }
        if self.imp().slider.get() {
            return;
        }
        for row in self.imp().children.borrow().iter().skip(1) {
            row.set_valign(
                if vertical
                    || row.has_css_class("option-action")
                    || row.has_css_class("option-segments")
                {
                    gtk::Align::Fill
                } else {
                    gtk::Align::Center
                },
            );
            let mut child = row.first_child();
            while let Some(w) = child {
                if w.has_css_class("option-label") {
                    w.set_visible(text);
                }
                if w.has_css_class("option-icon") {
                    w.set_visible(!vertical && !text);
                }
                if let Some(number) = w.downcast_ref::<NumberControl>() {
                    let title = row.tooltip_text().unwrap_or_default();
                    number.set_slider_visible(!vertical && self.imp().options.get().sliders);
                    number.set_popover_editor(vertical);
                    number.set_face(
                        vertical,
                        if vertical && labeled { &title } else { "" },
                        vertical && !labeled,
                        16,
                        self.imp().style.get() != TileStyle::Small,
                    );
                    number.set_vexpand(vertical);
                    number.set_valign(if vertical {
                        gtk::Align::Fill
                    } else {
                        gtk::Align::Center
                    });
                }
                if let Some(dropdown) = w.downcast_ref::<gtk::DropDown>() {
                    dropdown.set_show_arrow(!vertical);
                    dropdown.set_factory(Some(&choice_factory(vertical && !labeled, true, 16)));
                }
                if row.has_css_class("option-action") || row.has_css_class("option-segments") {
                    if let Some(image) = w
                        .clone()
                        .downcast::<gtk::Button>()
                        .ok()
                        .and_then(|b| b.child())
                        .and_downcast::<gtk::Image>()
                    {
                        image.set_pixel_size(self.imp().style.get().icon_size() as i32);
                    }
                }
                child = w.next_sibling();
            }
        }
    }
}

struct BrushPreview {
    popover: gtk::Popover,
    area: gtk::DrawingArea,
    label: gtk::Label,
    bookmark: gtk::Button,
    selected: Cell<Option<bool>>,
}

enum Field {
    Numeric(NumberControl),
    Choice(gtk::DropDown),
    Segments(Vec<gtk::ToggleButton>),
    Action(gtk::Button),
}
pub(super) struct Component {
    pub root: ComponentBody,
    pub button: gtk::Button,
    pub control: ToolbarControl,
    context: Cell<Option<ToolbarContext>>,
    contact_context: Cell<Option<ToolbarContext>>,
    updating: Cell<bool>,
    slider: Option<gtk::Scale>,
    preview: RefCell<Option<BrushPreview>>,
    outside: RefCell<Option<(gtk::Window, gtk::EventControllerLegacy)>>,
    bookmarks: RefCell<Vec<SliderBookmark>>,
    value: Cell<f32>,
    schema: RefCell<Vec<ToolOption>>,
    fields: RefCell<Vec<Field>>,
}
impl Component {
    pub fn new(w: &Rc<Workspace>, panel: Panel, tile: &ToolbarTile) -> Rc<Self> {
        let root: ComponentBody = glib::Object::new();
        root.set_overflow(gtk::Overflow::Hidden);
        root.add_css_class("toolbar-component");
        root.add_css_class("small-component");
        root.add_css_class("customizable-target");
        root.update_property(&[gtk::accessible::Property::Label(
            &tool_choice(tile.control).label,
        )]);
        let binding = tile.control.slider();
        let button = gtk::Button::new();
        button.set_hexpand(true);
        button.set_vexpand(true);
        button.add_css_class("flat");
        button.set_widget_name(&format!("tile-{}", tile.id));
        button.set_tooltip_text(Some(&tool_choice(tile.control).label));
        let cap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        cap.append(&button);
        let target = ContextTarget::Tile {
            panel,
            tile: tile.id,
        };
        w.install_panel_drag(
            &cap,
            DockItem::Tile {
                panel,
                tile: tile.id,
            },
        );
        w.install_context(&cap, target);
        root.append(&cap);
        let slider = if let Some(ref binding) = binding {
            button.add_css_class("slider-cap");
            let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0., 1., 0.001);
            scale.set_draw_value(false);
            scale.set_hexpand(true);
            scale.set_widget_name(&format!("component-slider-{}", tile.id));
            scale.update_property(&[gtk::accessible::Property::Label(
                &tool_choice(tile.control).label,
            )]);
            root.append(&scale);
            root.imp().slider.set(true);
            root.imp()
                .opacity
                .set(*binding == ToolbarNumericBinding::BrushOpacity);
            root.add_css_class("brush-slider");
            scale.add_css_class("brush-track");
            Some(scale)
        } else {
            root.imp()
                .options
                .set(tile.control.options_style().unwrap());
            button.set_child(Some(&crate::icons::image("layer-more-symbolic")));
            button.set_tooltip_text(Some("More tool options"));
            button.update_property(&[gtk::accessible::Property::Label("More tool options")]);
            w.install_context(&root, target);
            None
        };
        let component = Rc::new(Self {
            root,
            button,
            control: tile.control,
            context: Cell::new(None),
            contact_context: Cell::new(None),
            updating: Cell::new(false),
            slider,
            preview: RefCell::default(),
            outside: RefCell::default(),
            bookmarks: RefCell::default(),
            value: Cell::new(0.),
            schema: RefCell::default(),
            fields: RefCell::default(),
        });
        let id = tile.id;
        if component.slider.is_none() {
            component.button.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| {
                    w.dispatch(UiAction::ActivateTile { panel, tile: id });
                }
            ));
        }
        if let Some(scale) = &component.slider {
            component.install_slider_input(w);
            scale.connect_value_changed(glib::clone!(
                #[weak]
                component,
                #[weak]
                w,
                move |scale| {
                    if component.updating.get() {
                        return;
                    }
                    let binding = component.control.slider().unwrap();
                    let Some(field) = w
                        .gpu
                        .borrow()
                        .as_ref()
                        .and_then(|g| binding.field(g.session.state()))
                    else {
                        return;
                    };
                    let Ok(value) = field.numeric.resolve(
                        field.value as f64,
                        NumericOperation::Position {
                            position: scale.value(),
                        },
                    ) else {
                        return;
                    };
                    let current = field
                        .numeric
                        .resolve(
                            field.value as f64,
                            NumericOperation::Value {
                                value: field.value as f64,
                            },
                        )
                        .unwrap();
                    if value.value == current.value {
                        return;
                    }
                    if let Some(context) =
                        component.contact_context.get().or(component.context.get())
                    {
                        w.dispatch(UiAction::ToolbarEdit {
                            context,
                            action: Box::new(binding.action(value.value as f32)),
                        });
                    }
                }
            ));
        }
        component.root.connect_unrealize(glib::clone!(
            #[weak]
            component,
            move |_| component.close_preview()
        ));
        component
    }

    pub fn refresh(self: &Rc<Self>, w: &Rc<Workspace>, state: &ToolbarComponentView) {
        self.updating.set(true);
        let context = state.context;
        let changed_context = self.context.replace(Some(context)) != Some(context);
        if self.slider.is_some() {
            let enabled = state.numeric.is_some();
            self.slider.as_ref().unwrap().set_sensitive(enabled);
            self.button.set_sensitive(enabled);
            if changed_context {
                self.close_preview();
            }
            let Some(field) = &state.numeric else {
                self.updating.set(false);
                return;
            };
            self.value.set(field.value);
            self.bookmarks.replace(state.bookmarks.clone());
            self.root.imp().bookmarks.replace(state.bookmarks.clone());
            if let Some(gpu) = w.gpu.borrow().as_ref() {
                self.root
                    .imp()
                    .bookmark_selected
                    .set(gpu.session.state().palette.panel.0);
            }
            self.root.queue_draw();
            self.update_preview();
            let value = field
                .numeric
                .resolve(field.value as f64, NumericOperation::Format)
                .unwrap();
            self.slider.as_ref().unwrap().set_value(value.fill);
            self.slider
                .as_ref()
                .unwrap()
                .update_property(&[gtk::accessible::Property::ValueText(&value.text)]);
        } else {
            let options = &state.options;
            let same = !changed_context
                && self.schema.borrow().len() == options.len()
                && self
                    .schema
                    .borrow()
                    .iter()
                    .zip(options)
                    .all(|(a, b)| a.same_schema(b));
            if !same {
                for field in self.fields.borrow().iter() {
                    if let Field::Numeric(number) = field {
                        number.cancel_edit();
                    }
                }
                self.fields.borrow_mut().clear();
                for child in self.root.imp().children.borrow_mut().drain(1..) {
                    child.unparent();
                }
                for option in options {
                    self.add_option(w, option, context);
                }
                self.root.update_option_axis();
                self.root.queue_allocate();
            }
            for (field, option) in self.fields.borrow().iter().zip(options) {
                match (field, option) {
                    (Field::Numeric(number), ToolOption::Numeric(f)) => {
                        number.set_value(f.value as f64)
                    }
                    (Field::Choice(d), ToolOption::Choice { items, .. }) => d.set_selected(
                        items
                            .iter()
                            .position(|i| i.selected)
                            .map_or(gtk::INVALID_LIST_POSITION, |i| i as u32),
                    ),
                    (Field::Segments(buttons), ToolOption::Choice { items, .. }) => {
                        for (button, item) in buttons.iter().zip(items) {
                            button.set_active(item.selected);
                        }
                    }
                    (Field::Action(b), ToolOption::Action { state, .. }) => {
                        b.set_sensitive(state.enabled);
                        if let Some(b) = b.downcast_ref::<gtk::ToggleButton>() {
                            b.set_active(state.selected);
                        }
                    }
                    _ => (),
                }
            }
            self.schema.borrow_mut().clone_from(options);
        }
        self.updating.set(false);
    }

    fn close_preview(&self) {
        if let Some(preview) = self.preview.borrow_mut().take() {
            preview.popover.popdown();
            preview.popover.unparent();
        }
    }
    fn show_preview(self: &Rc<Self>, w: &Rc<Workspace>) {
        if self.preview.borrow().is_none() {
            let Some(context) = self.context.get() else {
                return;
            };
            let stamp = w
                .gpu
                .borrow()
                .as_ref()
                .and_then(|g| g.session.toolbar_stamp(context).ok());
            let Some(stamp) = stamp else {
                return;
            };
            let pixels: Vec<u8> = stamp.alpha.iter().flat_map(|&a| [a, a, a, a]).collect();
            let Ok(image) = gtk::cairo::ImageSurface::create_for_data(
                pixels,
                gtk::cairo::Format::ARgb32,
                stamp.size as i32,
                stamp.size as i32,
                stamp.size as i32 * 4,
            ) else {
                return;
            };
            let area = gtk::DrawingArea::new();
            let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            header.set_margin_start(12);
            header.set_margin_end(5);
            header.set_margin_top(4);
            header.set_valign(gtk::Align::Start);
            let label = gtk::Label::new(None);
            label.set_widget_name("slider-preview-label");
            label.set_xalign(0.);
            label.set_hexpand(true);
            let bookmark = gtk::Button::new();
            bookmark.add_css_class("flat");
            bookmark.set_size_request(28, 28);
            bookmark.set_widget_name("slider-bookmark");
            header.append(&label);
            header.append(&bookmark);
            let overlay = gtk::Overlay::new();
            overlay.set_child(Some(&area));
            overlay.add_overlay(&header);
            let popover: gtk::Popover = crate::squircle::Popover::new().upcast();
            popover.set_has_arrow(false);
            popover.set_autohide(false);
            popover.add_css_class("brush-preview");
            popover.set_widget_name("brush-slider-preview");
            let slider = self.slider.as_ref().unwrap();
            popover.set_parent(slider);
            popover.set_child(Some(&overlay));
            let vertical = self.root.imp().vertical.get();
            popover.set_position(if vertical {
                gtk::PositionType::Right
            } else {
                gtk::PositionType::Bottom
            });
            if let Some(component) = self.root.compute_bounds(slider) {
                let toolbar = self.root
                    .parent()
                    .and_then(|p| p.compute_bounds(slider))
                    .unwrap_or(component);
                // Grow the anchor on both sides. A directional offset points
                // back into the toolbar when GTK flips a bottom/right popup.
                let (x, y, width, height) = if vertical {
                    (
                        toolbar.x() - 8., component.y(),
                        toolbar.width() + 16., component.height(),
                    )
                } else {
                    (
                        component.x(), toolbar.y() - 8.,
                        component.width(), toolbar.height() + 16.,
                    )
                };
                popover.set_pointing_to(Some(&gdk::Rectangle::new(
                    x.floor() as i32,
                    y.floor() as i32,
                    width.ceil() as i32,
                    height.ceil() as i32,
                )));
            }
            area.set_draw_func(glib::clone!(
                #[weak(rename_to=component)]
                self,
                move |area, cr, _, _| {
                    let Ok(layout) = component.preview_layout(stamp.extent) else {
                        return;
                    };
                    let b = layout.stamp;
                    let ink = area.color();
                    let _ = cr.save();
                    crate::squircle::rounded_rect(cr, &gtk::gsk::RoundedRect::from_rect(
                        gtk::graphene::Rect::new(0., 0., layout.side, layout.side),
                        SURFACE_RADIUS,
                    ));
                    cr.clip();
                    let _ = cr.save();
                    let viewport = layout.viewport;
                    cr.rectangle(
                        viewport.x as f64,
                        viewport.y as f64,
                        viewport.width as f64,
                        viewport.height as f64,
                    );
                    cr.clip();
                    cr.translate(b.x as f64, b.y as f64);
                    cr.scale(
                        b.width as f64 / stamp.size as f64,
                        b.height as f64 / stamp.size as f64,
                    );
                    cr.set_source_rgba(
                        ink.red() as f64,
                        ink.green() as f64,
                        ink.blue() as f64,
                        layout.opacity as f64,
                    );
                    let _ = cr.mask_surface(&image, 0., 0.);
                    let _ = cr.restore();
                    if layout.header_fade > 0. {
                        let [r, g, b] = component
                            .root
                            .imp()
                            .bookmark_selected
                            .get()
                            .map(|c| c as f64 / 255.);
                        let fade =
                            gtk::cairo::LinearGradient::new(0., 0., 0., layout.header_fade as f64);
                        fade.add_color_stop_rgba(0., r, g, b, 0.65);
                        fade.add_color_stop_rgba(1., r, g, b, 0.);
                        let _ = cr.set_source(&fade);
                        cr.rectangle(0., 0., f64::from(layout.side), f64::from(layout.header_fade));
                        let _ = cr.fill();
                    }
                    let _ = cr.restore();
                }
            ));
            bookmark.connect_clicked(glib::clone!(
                #[weak(rename_to=component)]
                self,
                #[weak]
                w,
                move |_| {
                    w.dispatch(UiAction::ToolbarEdit {
                        context,
                        action: Box::new(UiAction::ToggleSliderBookmark {
                            control: component.control,
                        }),
                    });
                }
            ));
            self.preview.replace(Some(BrushPreview {
                popover,
                area,
                label,
                bookmark,
                selected: Cell::new(None),
            }));
        }
        self.update_preview();
        if let Some(preview) = self.preview.borrow().as_ref() {
            preview.popover.popup();
            preview.popover.present();
        }
    }
    fn preview_layout(&self, extent: f32) -> Result<SliderPreviewLayout, String> {
        slider_preview_layout(
            self.control,
            self.value.get(),
            self.root.width().max(self.root.height()) as f32,
            extent,
        )
    }
    fn update_preview(&self) {
        let preview = self.preview.borrow();
        let Some(preview) = preview.as_ref() else {
            return;
        };
        let Ok(layout) = self.preview_layout(1.) else {
            return;
        };
        preview.area.set_content_width(layout.side as i32);
        preview.area.set_content_height(layout.side as i32);
        preview.label.set_text(&layout.text);
        let selected = self.bookmarks.borrow().iter().any(|b| b.selected);
        if preview.selected.replace(Some(selected)) != Some(selected) {
            preview
                .bookmark
                .set_child(Some(&crate::icons::image(if selected {
                    "layer-minus-symbolic"
                } else {
                    "layer-plus-symbolic"
                })));
            preview.bookmark.set_tooltip_text(Some(if selected {
                "Remove bookmark"
            } else {
                "Bookmark this value"
            }));
        }
        preview.area.queue_draw();
    }
    fn install_slider_input(self: &Rc<Self>, w: &Rc<Workspace>) {
        let outside = gtk::EventControllerLegacy::new();
        outside.set_propagation_phase(gtk::PropagationPhase::Capture);
        outside.connect_event(glib::clone!(
            #[weak(rename_to=component)]
            self,
            #[weak]
            w,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, event| {
                let own_popup = component
                    .preview
                    .borrow()
                    .as_ref()
                    .and_then(|p| p.popover.surface());
                if own_popup.is_none() || event.surface() != own_popup {
                    match event.event_type() {
                        gdk::EventType::ButtonPress | gdk::EventType::TouchBegin => {
                            if let Some((x, y)) = event.position() {
                                let inside =
                                    component.root.compute_bounds(&w.window).is_some_and(|b| {
                                        b.contains_point(&gtk::graphene::Point::new(
                                            x as f32, y as f32,
                                        ))
                                    });
                                if event.surface() != w.window.surface() || !inside {
                                    component.close_preview();
                                }
                            }
                        }
                        gdk::EventType::KeyPress => {
                            if event
                                .downcast_ref::<gdk::KeyEvent>()
                                .is_some_and(|e| e.keyval() == gdk::Key::Escape)
                            {
                                component.close_preview();
                            }
                        }
                        gdk::EventType::FocusChange => {
                            // Let GTK finish transferring focus to popup surfaces.
                            glib::idle_add_local_once(glib::clone!(
                                #[weak]
                                component,
                                #[weak]
                                w,
                                move || {
                                    if !w.window.is_active() {
                                        component.close_preview();
                                    }
                                }
                            ));
                        }
                        _ => (),
                    }
                }
                glib::Propagation::Proceed
            }
        ));
        w.window.add_controller(outside.clone());
        self.outside
            .replace(Some((w.window.clone().upcast(), outside)));
        let scale = self.slider.as_ref().unwrap();
        let gesture = gtk::GestureDrag::new();
        gesture.set_button(1);
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        let origin = Rc::new(Cell::new([0., 0.]));
        let moved = Rc::new(Cell::new(false));
        gesture.connect_drag_begin(glib::clone!(
            #[weak(rename_to=component)]
            self,
            #[weak]
            w,
            #[strong]
            origin,
            #[strong]
            moved,
            move |g, x, y| {
                g.set_state(gtk::EventSequenceState::Claimed);
                origin.set([x, y]);
                moved.set(false);
                component.contact_context.set(component.context.get());
                component.pick_slider([x, y], true);
                component.show_preview(&w);
            }
        ));
        gesture.connect_drag_update(glib::clone!(
            #[weak(rename_to=component)]
            self,
            #[strong]
            origin,
            #[strong]
            moved,
            move |_, x, y| {
                if !moved.get() && x.hypot(y) < 3. {
                    return;
                }
                moved.set(true);
                component.pick_slider([origin.get()[0] + x, origin.get()[1] + y], false);
            }
        ));
        gesture.connect_drag_end(glib::clone!(
            #[weak(rename_to=component)]
            self,
            #[weak]
            w,
            #[strong]
            moved,
            move |_, _, _| {
                component.contact_context.set(None);
                if moved.get() {
                    component.close_preview();
                } else {
                    component.show_preview(&w);
                }
            }
        ));
        gesture.connect_cancel(glib::clone!(
            #[weak(rename_to=component)]
            self,
            move |_, _| {
                component.contact_context.set(None);
                component.close_preview();
            }
        ));
        scale.add_controller(gesture);
        self.button.connect_clicked(glib::clone!(
            #[weak(rename_to=component)]
            self,
            #[weak]
            w,
            move |_| component.show_preview(&w)
        ));
    }
    fn pick_slider(&self, point: [f64; 2], snap: bool) {
        let scale = self.slider.as_ref().unwrap();
        let vertical = self.root.imp().vertical.get();
        let range = scale.range_rect();
        let (start, end) = scale.slider_range();
        let half = (end - start) as f64 / 2.;
        let length = if vertical {
            range.height()
        } else {
            range.width()
        } as f64
            - half * 2.;
        if length <= 0. {
            return;
        }
        let p = if vertical {
            point[1] - range.y() as f64
        } else {
            point[0] - range.x() as f64
        };
        let position = ((p - half) / length).clamp(0., 1.);
        let position = if vertical { 1. - position } else { position };
        let values: Vec<_> = if snap {
            self.bookmarks.borrow().iter().map(|m| m.value).collect()
        } else {
            Vec::new()
        };
        if let Ok(value) = slider_bookmark_value(self.control, &values, position, length) {
            let number = self
                .control
                .slider()
                .unwrap()
                .numeric()
                .resolve(value, NumericOperation::Format)
                .unwrap();
            scale.set_value(number.fill);
        }
    }

    fn add_option(
        self: &Rc<Self>,
        w: &Rc<Workspace>,
        option: &ToolOption,
        context: ToolbarContext,
    ) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row.add_css_class("panel-control-row");
        row.add_css_class("customizable-target");
        row.set_valign(gtk::Align::Center);
        let field = match option {
            ToolOption::Numeric(f) => {
                let label = gtk::Label::new(Some(f.label));
                label.add_css_class("option-label");
                row.set_tooltip_text(Some(f.label));
                row.append(&label);
                let icon =
                    crate::icons::image(&format!("layer-{}-symbolic", tool_setting_icon(f.id)));
                icon.add_css_class("option-icon");
                icon.set_visible(false);
                row.append(&icon);
                let number = NumberControl::compact(f.numeric.clone(), f.label);
                number.set_icon(tool_setting_icon(f.id));
                number.add_css_class("toolbar-number");
                number.set_widget_name(&format!("toolbar-setting-{}", f.id));
                let id = f.id;
                for target in [label.upcast_ref::<gtk::Widget>(), icon.upcast_ref()] {
                    target.set_tooltip_text(Some(&format!("{} — double-click to reset", f.label)));
                    let reset = gtk::GestureClick::new();
                    reset.set_button(1);
                    reset.connect_pressed(glib::clone!(
                        #[weak]
                        w,
                        move |gesture, count, _, _| {
                            if count == 2 {
                                gesture.set_state(gtk::EventSequenceState::Claimed);
                                w.dispatch(UiAction::ToolbarEdit {
                                    context,
                                    action: Box::new(UiAction::ResetToolSetting { id: id.into() }),
                                });
                            }
                        }
                    ));
                    target.add_controller(reset);
                }
                number.connect_value_changed(glib::clone!(
                    #[weak(rename_to=component)]
                    self,
                    #[weak]
                    w,
                    move |number| {
                        if !component.updating.get() {
                            w.dispatch(UiAction::ToolbarEdit {
                                context,
                                action: Box::new(UiAction::SetToolSetting {
                                    id: id.into(),
                                    value: number.value() as f32,
                                }),
                            });
                        }
                    }
                ));
                row.append(&number);
                Field::Numeric(number)
            }
            ToolOption::Choice {
                id,
                label,
                segmented: true,
                items,
            } => {
                row.add_css_class("linked");
                row.add_css_class("selection-modes");
                row.add_css_class("option-segments");
                row.set_spacing(0);
                row.set_homogeneous(true);
                row.set_widget_name(&format!("toolbar-segments-{id}"));
                row.update_property(&[gtk::accessible::Property::Label(label)]);
                let mut buttons = Vec::new();
                for (index, item) in items.iter().enumerate() {
                    let button = gtk::ToggleButton::new();
                    button.set_child(Some(&crate::icons::image(&format!(
                        "layer-{}-symbolic",
                        item.icon
                    ))));
                    button.add_css_class("tile-button");
                    button.set_hexpand(true);
                    button.set_vexpand(true);
                    button.set_tooltip_text(Some(item.label));
                    button.set_widget_name(&format!("toolbar-segment-{id}-{index}"));
                    button.update_property(&[gtk::accessible::Property::Label(item.label)]);
                    if let Some(first) = buttons.first() {
                        button.set_group(Some(first));
                    }
                    let action = item.action.clone();
                    button.connect_toggled(glib::clone!(
                        #[weak(rename_to=component)]
                        self,
                        #[weak]
                        w,
                        move |button| {
                            if !component.updating.get() && button.is_active() {
                                w.dispatch(UiAction::ToolbarEdit {
                                    context,
                                    action: Box::new(action.clone()),
                                });
                            }
                        }
                    ));
                    row.append(&button);
                    buttons.push(button);
                }
                Field::Segments(buttons)
            }
            ToolOption::Choice {
                id,
                label,
                segmented: false,
                items,
            } => {
                // Store the core icon alongside each label in a native model.
                let model = gtk::gio::ListStore::new::<glib::BoxedAnyObject>();
                for item in items {
                    model.append(&glib::BoxedAnyObject::new((
                        item.icon.to_string(),
                        item.label.to_string(),
                    )));
                }
                let choice = gtk::DropDown::builder()
                    .model(&model)
                    .factory(&choice_factory(false, true, 16))
                    .list_factory(&choice_factory(false, false, 16))
                    .build();
                choice.set_hexpand(true);
                choice.set_tooltip_text(Some(label));
                choice.set_widget_name(&format!("toolbar-choice-{id}"));
                choice.update_property(&[gtk::accessible::Property::Label(label)]);
                let actions: Vec<_> = items.iter().map(|i| i.action.clone()).collect();
                choice.connect_selected_notify(glib::clone!(
                    #[weak(rename_to=component)]
                    self,
                    #[weak]
                    w,
                    move |choice| {
                        if !component.updating.get()
                            && let Some(action) = actions.get(choice.selected() as usize)
                        {
                            w.dispatch(UiAction::ToolbarEdit {
                                context,
                                action: Box::new(action.clone()),
                            });
                        }
                    }
                ));
                row.append(&choice);
                Field::Choice(choice)
            }
            ToolOption::Action { state, checkable } => {
                row.add_css_class("option-action");
                let button: gtk::Button = if *checkable {
                    gtk::ToggleButton::with_label(state.label).upcast()
                } else {
                    gtk::Button::with_label(state.label)
                };
                if let Some(icon) = state.icon {
                    button.set_child(Some(&crate::icons::image(&format!(
                        "layer-{icon}-symbolic"
                    ))));
                }
                button.update_property(&[gtk::accessible::Property::Label(state.label)]);
                button.add_css_class("flat");
                button.add_css_class("tile-button");
                button.set_hexpand(true);
                button.set_vexpand(true);
                button.set_tooltip_text(Some(&state.tooltip));
                button.set_widget_name(&format!("toolbar-action-{:?}", state.id));
                let command = state.id;
                button.connect_clicked(glib::clone!(
                    #[weak(rename_to=component)]
                    self,
                    #[weak]
                    w,
                    move |_| {
                        if !component.updating.get() {
                            w.dispatch(UiAction::ToolbarEdit {
                                context,
                                action: Box::new(UiAction::Invoke { command }),
                            });
                        }
                    }
                ));
                row.append(&button);
                Field::Action(button)
            }
        };
        self.root.append(&row);
        self.fields.borrow_mut().push(field);
    }
}

/// Both dropdown faces and popup rows use the application's SVG icon provider.
fn choice_factory(compact: bool, face: bool, icon_size: i32) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.set_halign(if compact {
            gtk::Align::Center
        } else {
            gtk::Align::Fill
        });
        row.set_hexpand(true);
        row.set_valign(gtk::Align::Center);
        let image = crate::icons::image("layer-settings-symbolic");
        image.set_pixel_size(icon_size);
        row.append(&image);
        if !compact {
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            if face {
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.set_max_width_chars(16);
            }
            row.append(&label);
        }
        item.downcast_ref::<gtk::ListItem>()
            .unwrap()
            .set_child(Some(&row));
    });
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let data = item
            .item()
            .unwrap()
            .downcast::<glib::BoxedAnyObject>()
            .unwrap();
        let data = data.borrow::<(String, String)>();
        let row = item.child().unwrap();
        let image = row.first_child().unwrap().downcast::<gtk::Image>().unwrap();
        crate::icons::set(&image, Some(&format!("layer-{}-symbolic", data.0)));
        if let Some(label) = image.next_sibling().and_downcast::<gtk::Label>() {
            label.set_text(&data.1);
        }
        row.set_tooltip_text(Some(&data.1));
    });
    factory
}

/// Custom trough only: GtkRange still owns hit testing, keyboard input,
/// accessibility and the thumb. Both gradients align with its native travel.
fn paint_track(
    root: &ComponentBody,
    scale: &gtk::Scale,
    snapshot: &gtk::Snapshot,
    opacity: bool,
    vertical: bool,
) {
    if !scale.is_child_visible() {
        return;
    }
    let Some(b) = scale.compute_bounds(root) else {
        return;
    };
    let range = scale.range_rect();
    let x = b.x() as f64 + range.x() as f64;
    let y = b.y() as f64 + range.y() as f64;
    let w = range.width() as f64;
    let h = range.height() as f64;
    let length = if vertical { h } else { w };
    if length < 16. {
        return;
    }
    let cr = snapshot.append_cairo(&gtk::graphene::Rect::new(
        b.x(),
        b.y(),
        b.width(),
        b.height(),
    ));
    if vertical {
        cr.translate(x + w * 0.5, y + h - 6.);
        cr.rotate(-std::f64::consts::FRAC_PI_2);
    } else {
        cr.translate(x + 6., y + h * 0.5);
    }
    let length = length - 12.;
    let wide = 8.;
    let narrow = if opacity { wide } else { 2.5 };
    cr.move_to(0., -narrow);
    cr.line_to(length - 3., -wide);
    cr.curve_to(length + 1., -wide, length + 1., wide, length - 3., wide);
    cr.line_to(0., narrow);
    cr.curve_to(-3., narrow, -3., -narrow, 0., -narrow);
    cr.close_path();
    cr.clip();
    let ink = root.color();
    let alpha = if scale.is_sensitive() { 1. } else { 0.35 };
    if opacity {
        cr.set_source_rgba(
            ink.red() as f64,
            ink.green() as f64,
            ink.blue() as f64,
            0.08 * alpha,
        );
        let _ = cr.paint();
        cr.set_source_rgba(
            ink.red() as f64,
            ink.green() as f64,
            ink.blue() as f64,
            0.2 * alpha,
        );
        for col in -1..(length / 4.) as i32 + 2 {
            for row in -2..2 {
                if (col + row) % 2 == 0 {
                    cr.rectangle(col as f64 * 4., row as f64 * 4., 4., 4.);
                }
            }
        }
        let _ = cr.fill();
        let gradient = gtk::cairo::LinearGradient::new(0., 0., length, 0.);
        gradient.add_color_stop_rgba(
            0.,
            ink.red() as f64,
            ink.green() as f64,
            ink.blue() as f64,
            0.,
        );
        gradient.add_color_stop_rgba(
            1.,
            ink.red() as f64,
            ink.green() as f64,
            ink.blue() as f64,
            0.65 * alpha,
        );
        let _ = cr.set_source(&gradient);
    } else {
        cr.set_source_rgba(
            ink.red() as f64,
            ink.green() as f64,
            ink.blue() as f64,
            0.22 * alpha,
        );
    }
    let _ = cr.paint();
}

fn paint_bookmarks(root: &ComponentBody, snapshot: &gtk::Snapshot, bookmarks: &[SliderBookmark]) {
    let children = root.imp().children.borrow();
    let Some(scale) = children.get(1).and_then(|w| w.downcast_ref::<gtk::Scale>()) else {
        return;
    };
    let Some(b) = scale.compute_bounds(root) else {
        return;
    };
    let range = scale.range_rect();
    let (start, end) = scale.slider_range();
    let half = (end - start) as f32 / 2.;
    let vertical = root.imp().vertical.get();
    let travel = if vertical {
        range.height()
    } else {
        range.width()
    } as f32
        - half * 2.;
    let color = root.color();
    for mark in bookmarks {
        // GTK rounds its thumb allocation. Use that actual center for a selected
        // mark so the line stays centered at every value and display scale.
        let position = if mark.selected {
            (start + end) as f32 / 2. - if vertical { range.y() } else { range.x() } as f32
        } else {
            half + travel
                * if vertical {
                    1. - mark.fill as f32
                } else {
                    mark.fill as f32
                }
        };
        let rect = if vertical {
            gtk::graphene::Rect::new(
                b.x() + range.x() as f32 + range.width() as f32 / 2. - 7.,
                b.y() + range.y() as f32 + position - 1.,
                14.,
                2.,
            )
        } else {
            gtk::graphene::Rect::new(
                b.x() + range.x() as f32 + position - 1.,
                b.y() + range.y() as f32 + range.height() as f32 / 2. - 7.,
                2.,
                14.,
            )
        };
        let [r, g, b] = root.imp().bookmark_selected.get();
        let selected = gdk::RGBA::new(r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.);
        snapshot.append_color(if mark.selected { &selected } else { &color }, &rect);
    }
}

impl Drop for Component {
    fn drop(&mut self) {
        self.close_preview();
        if let Some((window, controller)) = self.outside.take() {
            window.remove_controller(&controller);
        }
    }
}
