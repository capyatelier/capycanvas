//! Retained GTK toolbar editors. Binding, context and fitting policy is shared.
use super::*;
use crate::number_control::NumberControl;

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct ComponentBody {
        pub children: RefCell<Vec<gtk::Widget>>,
        pub vertical: Cell<bool>,
        pub slider: Cell<bool>,
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
            if children.len() < 2 {
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
                for (child, b) in
                    children
                        .iter()
                        .zip(toolbar_slider_layout(width as f32, height as f32, axis))
                {
                    allocate(child, b);
                }
            } else {
                let widths: Vec<_> = children[2..]
                    .iter()
                    .map(|w| w.measure(gtk::Orientation::Horizontal, height).1 as f32)
                    .collect();
                let (grip, fields, more) =
                    tool_options_layout(width as f32, height as f32, axis, &widths);
                allocate(&children[0], grip);
                allocate(&children[1], more);
                for (child, b) in children[2..].iter().zip(fields) {
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
                            children[1].grab_focus();
                        }
                        child.set_child_visible(false);
                    }
                }
            }
            if let Some(popover) = self.popover.borrow().as_ref().filter(|p| p.is_visible()) {
                let b = toolbar_slider_layout(width as f32, height as f32, axis)[1];
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
                .get(1)
                .and_then(|w| w.downcast_ref::<gtk::Button>())
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
                .get(2)
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
        self.queue_allocate();
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
        let handle = tiles::grip();
        handle.set_widget_name(&format!("component-grip-{}", tile.id));
        handle.set_tooltip_text(Some("Drag to move this toolbar component"));
        // The handle is immediate for every pointer device. No reorder target
        // is installed on the slider, dropdowns, or the surrounding editor.
        w.register_drag(
            &handle,
            DragTarget::Dock(DockItem::Tile {
                panel,
                tile: tile.id,
            }),
        );
        w.install_context(
            &handle,
            ContextTarget::Tile {
                panel,
                tile: tile.id,
            },
        );
        root.append(&handle);
        let button = gtk::Button::new();
        button.add_css_class("flat");
        button.set_widget_name(&format!("tile-{}", tile.id));
        button.set_tooltip_text(Some(&tool_choice(tile.control).label));
        root.append(&button);
        let value_label = gtk::Label::new(None);
        value_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        value_label.set_width_chars(1);
        value_label.set_hexpand(true);
        let binding = tile.control.slider();
        let (slider, editor) = if let Some(ref binding) = binding {
            let label_row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            label_row.append(&crate::icons::image(&format!(
                "layer-{}-symbolic",
                tool_choice(tile.control).icon
            )));
            label_row.append(&value_label);
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
        if let Some(field) = &state.numeric {
            if changed_context {
                self.editor.as_ref().unwrap().cancel_edit();
                if let Some(popover) = self.root.imp().popover.borrow().as_ref() {
                    popover.popdown();
                }
            }
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
            self.value_label.set_label(&value.edit);
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
                    if let Field::Numeric(n) = field {
                        n.cancel_edit();
                    }
                }
                self.fields.borrow_mut().clear();
                for child in self.root.imp().children.borrow_mut().drain(2..) {
                    child.unparent();
                }
                for option in options {
                    self.add_option(w, option, context);
                }
                self.root.queue_allocate();
            }
            for (field, option) in self.fields.borrow().iter().zip(options) {
                match (field, option) {
                    (Field::Numeric(n), ToolOption::Numeric(f)) => n.set_value(f.value as f64),
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
        row.set_valign(gtk::Align::Center);
        let field = match option {
            ToolOption::Numeric(f) => {
                let label = gtk::Label::new(Some(f.label));
                row.append(&label);
                let number = NumberControl::value_only(f.numeric.clone(), f.label);
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
                row.append(&gtk::Label::new(Some(label)));
                let names: Vec<_> = items.iter().map(|i| i.label).collect();
                let choice = gtk::DropDown::from_strings(&names);
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
