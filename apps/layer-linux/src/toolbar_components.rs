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
                        if w.has_css_class("option-action") || self.vertical.get() {
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
                    if self.vertical.get() { 4. } else { 10. },
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
            row.set_valign(if vertical || row.has_css_class("option-action") {
                gtk::Align::Fill
            } else {
                gtk::Align::Center
            });
            let mut child = row.first_child();
            while let Some(w) = child {
                if w.has_css_class("option-label") {
                    w.set_visible(text);
                }
                if let Some(number) = w.downcast_ref::<NumberControl>() {
                    let title = row.tooltip_text().unwrap_or_default();
                    number.set_slider_visible(!vertical && self.imp().options.get().sliders);
                    number.set_face(
                        !text,
                        if vertical && labeled { &title } else { "" },
                        vertical && !labeled,
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
                    dropdown
                        .set_factory(Some(&choice_factory(!text && !(vertical && labeled), true)));
                }
                child = w.next_sibling();
            }
        }
    }
}

enum Field {
    Numeric(NumberControl),
    Choice(gtk::DropDown),
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
    editor: Option<NumberControl>,
    schema: RefCell<Vec<ToolOption>>,
    fields: RefCell<Vec<Field>>,
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
        let binding = tile.control.slider();
        let editor = binding.as_ref().map(|binding| {
            let spec = if *binding == ToolbarNumericBinding::BrushSize {
                NumericControl::brush_size()
            } else {
                NumericControl::percent()
            };
            let editor = NumberControl::compact(spec, &tool_choice(tile.control).label);
            editor.set_slider_visible(false);
            editor.set_widget_name(&format!("component-value-{}", tile.id));
            editor.add_css_class("slider-readout");
            editor
        });
        let button = editor
            .as_ref()
            .map_or_else(gtk::Button::new, NumberControl::value_button);
        button.set_hexpand(true);
        button.set_vexpand(true);
        button.add_css_class("flat");
        button.set_widget_name(&format!("tile-{}", tile.id));
        button.set_tooltip_text(Some(&tool_choice(tile.control).label));
        let cap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        if let Some(editor) = &editor {
            cap.append(editor);
        } else {
            cap.append(&button);
        }
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
            // Empty bar space is a presentation menu, not a reorderable tile.
            let hold = gtk::GestureLongPress::new();
            hold.set_touch_only(false);
            hold.set_propagation_phase(gtk::PropagationPhase::Capture);
            hold.connect_pressed(glib::clone!(
                #[weak]
                w,
                #[weak]
                root,
                move |g, x, y| {
                    if !crate::input::touch_or_pen(g)
                        && root.pick(x, y, gtk::PickFlags::DEFAULT).as_ref()
                            == Some(root.upcast_ref())
                    {
                        g.set_state(gtk::EventSequenceState::Claimed);
                        w.show_context(root.upcast_ref(), target, x, y);
                    }
                }
            ));
            root.add_controller(hold);
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
            editor,
            schema: RefCell::default(),
            fields: RefCell::default(),
        });
        let id = tile.id;
        if component.editor.is_none() {
            component.button.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| {
                    w.dispatch(UiAction::ActivateTile { panel, tile: id });
                }
            ));
        }
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
                .connect_interaction(glib::clone!(
                    #[weak]
                    component,
                    move |_, phase| {
                        match phase {
                            ContactPhase::Down => {
                                component.contact_context.set(component.context.get())
                            }
                            _ => component.contact_context.set(None),
                        }
                    }
                ));
            component
                .editor
                .as_ref()
                .unwrap()
                .connect_interaction(glib::clone!(
                    #[weak]
                    component,
                    move |_, phase| {
                        match phase {
                            ContactPhase::Down => {
                                component.contact_context.set(component.context.get())
                            }
                            _ => component.contact_context.set(None),
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
                            && let Some(context) =
                                component.contact_context.get().or(component.context.get())
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
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row.add_css_class("customizable-target");
        row.set_valign(gtk::Align::Center);
        let field = match option {
            ToolOption::Numeric(f) => {
                let label = gtk::Label::new(Some(f.label));
                label.add_css_class("option-label");
                row.set_tooltip_text(Some(f.label));
                row.append(&label);
                let number = NumberControl::compact(f.numeric.clone(), f.label);
                number.set_icon(tool_setting_icon(f.id));
                number.add_css_class("toolbar-number");
                number.set_widget_name(&format!("toolbar-setting-{}", f.id));
                let id = f.id;
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
                    .factory(&choice_factory(false, true))
                    .list_factory(&choice_factory(false, false))
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
fn choice_factory(compact: bool, face: bool) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.set_halign(if compact {
            gtk::Align::Center
        } else {
            gtk::Align::Fill
        });
        row.set_hexpand(true);
        row.append(&crate::icons::image("layer-settings-symbolic"));
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
