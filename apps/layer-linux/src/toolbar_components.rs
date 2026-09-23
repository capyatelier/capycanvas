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
        pub readout_scale: Cell<f64>,
        pub popover: RefCell<Option<gtk::Popover>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for ComponentBody {
        const NAME: &'static str = "CapyToolbarComponent";
        type Type = super::ComponentBody;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for ComponentBody {
        fn dispose(&self) {
            if let Some(p) = self.popover.take() {
                if p.parent().is_some() {
                    p.unparent();
                }
            }
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
                    child.allocate(
                        b.width.floor() as i32,
                        b.height.floor() as i32,
                        -1,
                        Some(
                            gtk::gsk::Transform::new()
                                .translate(&gtk::graphene::Point::new(b.x, b.y)),
                        ),
                    );
                }
            };
            if self.slider.get() {
                if let Some(label) = children[0]
                    .first_child()
                    .and_downcast::<gtk::Button>()
                    .and_then(|b| b.child())
                    .and_then(|row| row.last_child())
                    .and_downcast::<gtk::Label>()
                {
                    let text_width = label
                        .create_pango_layout(Some(&label.text()))
                        .pixel_size()
                        .0
                        .max(1);
                    let factor = if self.vertical.get() {
                        ((width - 2).max(1) as f64 / text_width as f64).min(1.)
                    } else {
                        1.
                    };
                    if (self.readout_scale.replace(factor) - factor).abs() > 0.001 {
                        let attrs = gtk::pango::AttrList::new();
                        attrs.insert(gtk::pango::AttrFloat::new_scale(factor));
                        label.set_attributes(Some(&attrs));
                    }
                }
                for (child, b) in children.iter().zip(toolbar_slider_layout(
                    width as f32,
                    height as f32,
                    axis,
                    children[0]
                        .measure(
                            if axis == Axis::Vertical {
                                gtk::Orientation::Vertical
                            } else {
                                gtk::Orientation::Horizontal
                            },
                            -1,
                        )
                        .1 as f32,
                )) {
                    allocate(child, b);
                }
            } else {
                let sizes: Vec<_> = children[1..]
                    .iter()
                    .map(|w| {
                        [
                            w.measure(gtk::Orientation::Horizontal, -1).1 as f32,
                            w.measure(gtk::Orientation::Vertical, -1).1 as f32,
                        ]
                    })
                    .collect();
                let button = [
                    children[0].measure(gtk::Orientation::Horizontal, -1).1 as f32,
                    children[0].measure(gtk::Orientation::Vertical, -1).1 as f32,
                ];
                let layout =
                    tool_options_layout(width as f32, height as f32, axis, &sizes, button, 6.);
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
            if let Some(popover) = self.popover.borrow().as_ref().filter(|p| p.is_visible()) {
                let b = toolbar_slider_layout(
                    width as f32,
                    height as f32,
                    axis,
                    children[0]
                        .measure(
                            if axis == Axis::Vertical {
                                gtk::Orientation::Vertical
                            } else {
                                gtk::Orientation::Horizontal
                            },
                            -1,
                        )
                        .1 as f32,
                )[0];
                let origin = popover.parent().and_then(|p| self.obj().compute_bounds(&p));
                if let Some(origin) = origin {
                    popover.set_pointing_to(Some(&gdk::Rectangle::new(
                        (origin.x() + b.x) as i32,
                        (origin.y() + b.y) as i32,
                        b.width as i32,
                        b.height as i32,
                    )));
                }
                popover.present();
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
    pub(crate) fn set_axis(&self, axis: Axis) {
        let vertical = axis == Axis::Vertical;
        if self.imp().vertical.replace(vertical) == vertical {
            return;
        }
        if vertical {
            self.add_css_class("vertical-component");
        } else {
            self.remove_css_class("vertical-component");
        }
        if self.imp().slider.get() {
            if let Some(row) = self
                .imp()
                .children
                .borrow()
                .get(0)
                .and_then(|w| w.first_child())
                .and_downcast::<gtk::Button>()
                .and_then(|b| b.child())
                .and_downcast::<gtk::Box>()
            {
                row.set_orientation(if vertical {
                    gtk::Orientation::Vertical
                } else {
                    gtk::Orientation::Horizontal
                });
            }
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
        if !self.imp().slider.get() {
            for row in self.imp().children.borrow().iter().skip(1) {
                let mut child = row.first_child();
                while let Some(w) = child {
                    if w.has_css_class("horizontal-option") {
                        w.set_visible(!vertical);
                    }
                    if w.has_css_class("vertical-option") {
                        w.set_visible(vertical);
                    }
                    if let Some(dropdown) = w.downcast_ref::<gtk::DropDown>() {
                        // The popup always retains icon and full label. Only the
                        // selected face becomes compact in a one-tile column.
                        dropdown.set_show_arrow(!vertical);
                        dropdown.set_factory(Some(&choice_factory(vertical)));
                    }
                    child = w.next_sibling();
                }
            }
        }
    }
}

enum Field {
    Numeric {
        inline: NumberControl,
        popup: NumberControl,
        button: gtk::MenuButton,
    },
    Choice(gtk::DropDown),
    Action(gtk::Button),
}
pub(super) struct Component {
    pub root: ComponentBody,
    pub button: gtk::Button,
    pub control: ToolbarControl,
    context: Cell<Option<ToolbarContext>>,
    contact_context: Cell<Option<ToolbarContext>>,
    popup_context: Cell<Option<ToolbarContext>>,
    updating: Cell<bool>,
    slider: Option<gtk::Scale>,
    editor: Option<NumberControl>,
    value_label: gtk::Label,
    schema: RefCell<Vec<ToolOption>>,
    fields: RefCell<Vec<Field>>,
}
impl Drop for Component {
    fn drop(&mut self) {
        // GTK can retain an unparented tile for a drag/snapshot. Its surface-
        // owned popup must retire when the editor does, not when GTK frees it.
        if let Some(popover) = self.root.imp().popover.take() {
            popover.popdown();
            if popover.parent().is_some() {
                popover.unparent();
            }
        }
    }
}
impl Component {
    pub fn new(w: &Rc<Workspace>, panel: Panel, tile: &ToolbarTile) -> Rc<Self> {
        let root: ComponentBody = glib::Object::new();
        root.set_overflow(gtk::Overflow::Hidden);
        root.add_css_class("toolbar-component");
        root.add_css_class("customizable-target");
        root.update_property(&[gtk::accessible::Property::Label(
            &tool_choice(tile.control).label,
        )]);
        let button = gtk::Button::new();
        button.set_hexpand(true);
        button.set_vexpand(true);
        button.add_css_class("flat");
        button.set_widget_name(&format!("tile-{}", tile.id));
        button.set_tooltip_text(Some(&tool_choice(tile.control).label));
        let cap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        cap.append(&button);
        w.install_panel_drag(
            &cap,
            DockItem::Tile {
                panel,
                tile: tile.id,
            },
        );
        w.install_context(
            &cap,
            ContextTarget::Tile {
                panel,
                tile: tile.id,
            },
        );
        // The value/More button is the tile body. Tracks and option fields are
        // separate siblings and retain their native immediate interactions.
        root.append(&cap);
        let value_label = gtk::Label::new(Some("—"));
        value_label.add_css_class("slider-readout");
        value_label.set_hexpand(true);
        let binding = tile.control.slider();
        let (slider, editor) = if let Some(ref binding) = binding {
            let label_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            label_row.append(&crate::icons::image(&format!(
                "layer-{}-symbolic",
                tool_choice(tile.control).icon
            )));
            label_row.append(&value_label);
            // Reserve the widest value to avoid resizing as the thumb moves.
            value_label.set_width_chars(if *binding == ToolbarNumericBinding::BrushSize {
                4
            } else {
                3
            });
            button.set_child(Some(&label_row));
            let spec = if *binding == ToolbarNumericBinding::BrushSize {
                NumericControl::brush_size()
            } else {
                NumericControl::percent()
            };
            let editor = NumberControl::new(spec, &tool_choice(tile.control).label, "");
            editor.set_widget_name(&format!("component-editor-{}", tile.id));
            editor.set_width_request(240);
            editor.set_margin_top(8);
            editor.set_margin_bottom(8);
            editor.set_margin_start(8);
            editor.set_margin_end(8);
            let popover = gtk::Popover::new();
            popover.set_child(Some(&editor));
            popover.set_parent(&w.surface);
            w.watch_popover(&popover);
            root.connect_unmap(glib::clone!(
                #[weak]
                popover,
                move |_| popover.popdown()
            ));
            *root.imp().popover.borrow_mut() = Some(popover);
            let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0., 1., 0.001);
            scale.set_draw_value(false);
            scale.set_hexpand(true);
            scale.set_vexpand(true);
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
            (Some(scale), Some(editor))
        } else {
            button.set_child(Some(&crate::icons::image("layer-more-symbolic")));
            button.set_tooltip_text(Some("More tool options"));
            button.update_property(&[gtk::accessible::Property::Label("More tool options")]);
            (None, None)
        };
        let component = Rc::new(Self {
            root,
            button,
            control: tile.control,
            context: Cell::new(None),
            contact_context: Cell::new(None),
            popup_context: Cell::new(None),
            updating: Cell::new(false),
            slider,
            editor,
            value_label,
            schema: RefCell::default(),
            fields: RefCell::default(),
        });
        let id = tile.id;
        component.button.connect_clicked(glib::clone!(
            #[weak]
            component,
            #[weak]
            w,
            move |_| {
                if let Some(popover) = component.root.imp().popover.borrow().as_ref() {
                    component.popup_context.set(component.context.get());
                    if let Some(b) = component.button.compute_bounds(&w.surface) {
                        popover.set_pointing_to(Some(&gdk::Rectangle::new(
                            b.x() as i32,
                            b.y() as i32,
                            b.width() as i32,
                            b.height() as i32,
                        )));
                    }
                    popover.popup();
                    popover.present();
                } else {
                    w.dispatch(UiAction::ActivateTile { panel, tile: id });
                }
            }
        ));
        if let Some(scale) = &component.slider {
            let events = gtk::EventControllerLegacy::new();
            events.set_propagation_phase(gtk::PropagationPhase::Capture);
            events.connect_event(glib::clone!(
                #[weak]
                component,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, event| {
                    use gdk::EventType as E;
                    match event.event_type() {
                        E::ButtonPress | E::TouchBegin => {
                            component.contact_context.set(component.context.get())
                        }
                        E::ButtonRelease | E::TouchEnd | E::TouchCancel => {
                            glib::idle_add_local_once(glib::clone!(
                                #[weak]
                                component,
                                move || component.contact_context.set(None)
                            ));
                        }
                        _ => (),
                    }
                    glib::Propagation::Proceed
                }
            ));
            scale.add_controller(events);
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
            component
                .editor
                .as_ref()
                .unwrap()
                .connect_value_changed(glib::clone!(
                    #[weak]
                    component,
                    #[weak]
                    w,
                    move |editor| {
                        if !component.updating.get()
                            && let Some(context) = component.popup_context.get()
                        {
                            w.dispatch(UiAction::ToolbarEdit {
                                context,
                                action: Box::new(
                                    component
                                        .control
                                        .slider()
                                        .unwrap()
                                        .action(editor.value() as f32),
                                ),
                            });
                        }
                    }
                ));
        }
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
                self.editor.as_ref().unwrap().cancel_edit();
                if let Some(popover) = self.root.imp().popover.borrow().as_ref() {
                    popover.popdown();
                }
            }
            let Some(field) = &state.numeric else {
                self.updating.set(false);
                return;
            };
            self.editor.as_ref().unwrap().set_value(field.value as f64);
            let value = field
                .numeric
                .resolve(field.value as f64, NumericOperation::Format)
                .unwrap();
            self.slider.as_ref().unwrap().set_value(value.fill);
            self.slider
                .as_ref()
                .unwrap()
                .update_property(&[gtk::accessible::Property::ValueText(&value.text)]);
            // Unit stays in the tooltip/accessibility label; a short value also
            // fits the narrow vertical presentation without rotated text.
            self.value_label
                .set_label(value.edit.strip_suffix(".0").unwrap_or(&value.edit));
            self.button
                .set_tooltip_text(Some(&format!("{}: {}", field.label, value.text)));
            self.button
                .update_property(&[gtk::accessible::Property::Label(&format!(
                    "{}: {}",
                    field.label, value.text
                ))]);
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
                    if let Field::Numeric {
                        inline,
                        popup,
                        button,
                    } = field
                    {
                        inline.cancel_edit();
                        popup.cancel_edit();
                        button.popdown();
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
                    (
                        Field::Numeric {
                            inline,
                            popup,
                            button,
                        },
                        ToolOption::Numeric(f),
                    ) => {
                        inline.set_value(f.value as f64);
                        popup.set_value(f.value as f64);
                        let value = f
                            .numeric
                            .resolve(f.value as f64, NumericOperation::Format)
                            .unwrap();
                        button.set_label(value.edit.strip_suffix(".0").unwrap_or(&value.edit));
                    }
                    (Field::Choice(d), ToolOption::Choice { items, .. }) => d.set_selected(
                        items
                            .iter()
                            .position(|i| i.selected)
                            .map_or(gtk::INVALID_LIST_POSITION, |i| i as u32),
                    ),
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

    fn add_option(
        self: &Rc<Self>,
        w: &Rc<Workspace>,
        option: &ToolOption,
        context: ToolbarContext,
    ) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.set_valign(gtk::Align::Center);
        let field = match option {
            ToolOption::Numeric(f) => {
                let label = gtk::Label::new(Some(f.label));
                label.add_css_class("horizontal-option");
                row.append(&label);
                let number = NumberControl::inline(f.numeric.clone(), f.label);
                number.add_css_class("horizontal-option");
                number.add_css_class("toolbar-number");
                let popup = NumberControl::new(f.numeric.clone(), f.label, "");
                popup.set_width_request(240);
                let button = gtk::MenuButton::new();
                button.set_hexpand(true);
                button.set_widget_name(&format!("toolbar-value-{}", f.id));
                button.set_direction(gtk::ArrowType::None);
                button.set_tooltip_text(Some(f.label));
                button.update_property(&[gtk::accessible::Property::Label(f.label)]);
                button.add_css_class("vertical-option");
                button.add_css_class("toolbar-number-menu");
                let popover = gtk::Popover::new();
                popover.set_child(Some(&popup));
                button.set_popover(Some(&popover));
                w.watch_popover(&popover);
                number.set_widget_name(&format!("toolbar-setting-{}", f.id));
                let id = f.id;
                for number in [&number, &popup] {
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
                }
                row.append(&number);
                row.append(&button);
                Field::Numeric {
                    inline: number,
                    popup,
                    button,
                }
            }
            ToolOption::Choice { id, label, items } => {
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
                    .factory(&choice_factory(false))
                    .list_factory(&choice_factory(false))
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
fn choice_factory(compact: bool) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.append(&crate::icons::image("layer-settings-symbolic"));
        if !compact {
            row.append(&gtk::Label::new(None));
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
