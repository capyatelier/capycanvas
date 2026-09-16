//! GTK projection of workspace-owned window-bar items.
use super::*;

#[path = "workspace_header_drag.rs"]
mod drag;
#[path = "workspace_header_editor.rs"]
mod editor;

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Surface {
        pub owner: RefCell<std::rc::Weak<Workspace>>,
        pub children: RefCell<Vec<gtk::Widget>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Surface {
        const NAME: &'static str = "CapyWindowBar";
        type Type = super::BarSurface;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Surface {
        fn dispose(&self) {
            for child in self.children.take() {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for Surface {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }
        fn size_allocate(&self, width: i32, _: i32, _: i32) {
            if let Some(w) = self.owner.borrow().upgrade() {
                w.header.allocate(&w, width as f32);
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            for child in self
                .children
                .borrow()
                .iter()
                .filter(|c| c.is_visible() && c.is_child_visible())
            {
                self.obj().snapshot_child(child, snapshot);
            }
            if let Some(w) = self.owner.borrow().upgrade() {
                w.header.snapshot(snapshot);
            }
        }
    }
}
glib::wrapper! { pub struct BarSurface(ObjectSubclass<imp::Surface>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget; }
impl BarSurface {
    fn add(&self, child: &impl IsA<gtk::Widget>) {
        child.set_parent(self);
        self.imp()
            .children
            .borrow_mut()
            .push(child.as_ref().clone());
    }
    fn remove(&self, child: &impl IsA<gtk::Widget>) {
        self.imp()
            .children
            .borrow_mut()
            .retain(|c| c != child.as_ref());
        child.unparent();
    }
}
struct Item {
    entry: HeaderEntry,
    root: gtk::Box,
    content: gtk::Widget,
    button: Option<gtk::Button>,
    compact: Option<gtk::MenuButton>,
}
impl Item {
    fn switcher_width(&self) -> f32 {
        self.content
            .downcast_ref::<gtk::Stack>()
            .and_then(|s| s.child_by_name("full"))
            .filter(|w| w.is_visible())
            .map_or(0., |w| w.measure(gtk::Orientation::Horizontal, -1).1 as f32)
    }
}
pub(super) struct Header {
    pub root: BarSurface,
    handle: gtk::WindowHandle,
    native: [gtk::WindowControls; 2],
    items: RefCell<Vec<Item>>,
    overflow: [gtk::MenuButton; 3],
    recovery: gtk::MenuButton,
    editor: editor::Editor,
    model: RefCell<Option<HeaderLayout>>,
    pub editing: Cell<bool>,
    geometry: RefCell<HeaderGeometry>,
    measuring: Cell<bool>,
    drop: Cell<Option<(HeaderZone, Option<u32>)>>,
    drag: RefCell<Option<drag::NativeHeaderDrag>>,
    insets: Cell<[f32; 2]>,
}
impl Header {
    #[cfg(test)]
    pub fn geometry_for_test(&self) -> HeaderGeometry {
        self.geometry.borrow().clone()
    }
    #[cfg(test)]
    pub fn drag_for_test(&self) -> Option<HeaderDragPreview> {
        self.drag.borrow().as_ref().map(|d| d.preview.clone())
    }
    pub fn new() -> Self {
        let root: BarSurface = glib::Object::new();
        root.set_widget_name("workspace-window-bar");
        root.add_css_class("window-bar");
        let handle = gtk::WindowHandle::new();
        handle.set_widget_name("window-drag-area");
        root.add(&handle);
        let native = [
            gtk::WindowControls::new(gtk::PackType::Start),
            gtk::WindowControls::new(gtk::PackType::End),
        ];
        for controls in &native {
            root.add(controls);
        }
        let overflow = std::array::from_fn(|i| {
            let b = gtk::MenuButton::builder()
                .child(&crate::icons::image("layer-menu-symbolic"))
                .tooltip_text(format!(
                    "More {} title-bar items",
                    HeaderZone::ALL[i].label().to_lowercase()
                ))
                .build();
            b.set_widget_name(&format!("header-overflow-{i}"));
            b.add_css_class("flat");
            b.add_css_class("chrome-control");
            root.add(&b);
            b
        });
        let recovery = gtk::MenuButton::builder()
            .child(&crate::icons::image("layer-menu-symbolic"))
            .tooltip_text("Title bar recovery: menus and customization")
            .build();
        recovery.set_widget_name("header-recovery");
        recovery.add_css_class("flat");
        recovery.add_css_class("chrome-control");
        root.add(&recovery);
        let editor = editor::Editor::new();
        root.add(&editor.root);
        Self {
            root,
            handle,
            native,
            overflow,
            recovery,
            editor,
            items: RefCell::new(Vec::new()),
            model: RefCell::new(None),
            editing: Cell::new(false),
            geometry: RefCell::new(HeaderGeometry::default()),
            measuring: Cell::new(false),
            drop: Cell::new(None),
            drag: RefCell::new(None),
            insets: Cell::new([0.; 2]),
        }
    }
    pub fn height(&self) -> f32 {
        self.model
            .borrow()
            .as_ref()
            .map_or(HEADER_HEIGHT, |m| m.size.height())
            + if self.editing.get() {
                self.editor.height.get()
            } else {
                0.
            }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        *self.root.imp().owner.borrow_mut() = Rc::downgrade(w);
        w.system_status.battery.connect_visible_notify(glib::clone!(
            #[weak]
            w,
            move |_| w.header.root.queue_allocate()
        ));
        w.system_status.clock.connect_visible_notify(glib::clone!(
            #[weak]
            w,
            move |_| w.header.root.queue_allocate()
        ));
        self.editor.bind(w);
        let pick = gtk::GestureClick::new();
        pick.set_button(1);
        pick.connect_released(glib::clone!(
            #[weak]
            w,
            move |_, _, x, y| {
                if w.header.drop_at([x as f32, y as f32]).is_some() {
                    let selected = w
                        .header
                        .geometry
                        .borrow()
                        .items
                        .iter()
                        .find(|m| m.bounds.contains(x as f32, y as f32))
                        .map(|m| m.id);
                    if let Some(id) = selected {
                        if let Some(item) =
                            w.header.items.borrow().iter().find(|i| i.entry.id == id)
                        {
                            item.root.grab_focus();
                        }
                    } else {
                        w.header.root.grab_focus();
                    }
                    w.header.editor.select(&w, selected);
                }
            }
        ));
        self.root.add_controller(pick);
        // Only empty editable bar space belongs to this menu. Native caption
        // menus outside editing and menus on actual controls retain ownership.
        let background = gtk::GestureClick::new();
        background.set_button(3);
        background.set_propagation_phase(gtk::PropagationPhase::Capture);
        background.connect_pressed(glib::clone!(
            #[weak]
            w,
            move |gesture, _, x, y| {
                if w.header.drop_at([x as f32, y as f32]).is_some()
                    && !w
                        .header
                        .geometry
                        .borrow()
                        .items
                        .iter()
                        .any(|m| m.bounds.contains(x as f32, y as f32))
                    && w.header.root.pick(x, y, gtk::PickFlags::DEFAULT).as_ref()
                        == Some(w.header.root.upcast_ref())
                {
                    w.header.editor.select(&w, None);
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    w.show_context(
                        w.header.root.upcast_ref(),
                        ContextTarget::Header { id: None },
                        x,
                        y,
                    );
                }
            }
        ));
        self.root.add_controller(background);
        for (i, button) in self.overflow.iter().enumerate() {
            button.set_create_popup_func(glib::clone!(
                #[weak]
                w,
                move |button| {
                    button.set_popover(Some(&w.header.overflow_popup(&w, i)));
                }
            ));
        }
        self.recovery.set_create_popup_func(glib::clone!(
            #[weak]
            w,
            move |button| {
                let Some(model) = w
                    .gpu
                    .borrow()
                    .as_ref()
                    .map(|g| g.session.application_menu(ApplicationMenu::Primary))
                else {
                    return;
                };
                let popup = gtk::PopoverMenu::from_model(gtk::gio::MenuModel::NONE);
                w.populate_workspace_menu(&popup, model);
                w.watch_popover(popup.upcast_ref());
                button.set_popover(Some(&popup));
            }
        ));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            w,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                if !w.header.editing.get() || w.window.visible_dialog().is_some() {
                    return glib::Propagation::Proceed;
                }
                if key == gdk::Key::Escape {
                    let drag = w.workspace_drag.borrow().clone();
                    if let Some(drag) = drag {
                        w.workspace_drag_input(ContactPhase::Cancel, drag.point, drag.sequence);
                        return glib::Propagation::Stop;
                    }
                }
                if w.popovers
                    .borrow()
                    .iter()
                    .filter_map(|p| p.upgrade())
                    .any(|p| p.is_visible())
                {
                    return glib::Propagation::Proceed;
                }
                if matches!(key, gdk::Key::Tab | gdk::Key::ISO_Left_Tab) {
                    let direction = if key == gdk::Key::ISO_Left_Tab
                        || modifiers.contains(gdk::ModifierType::SHIFT_MASK)
                    {
                        gtk::DirectionType::TabBackward
                    } else {
                        gtk::DirectionType::TabForward
                    };
                    if !w.header.root.child_focus(direction) {
                        gtk::prelude::GtkWindowExt::set_focus(&w.window, None::<&gtk::Widget>);
                        w.header.root.child_focus(direction);
                    }
                    return glib::Propagation::Stop;
                }
                // Native Tab/Enter/menu behavior is unchanged. The only editor
                // commands are moving or removing the selected bar item.
                let focused_in_bar =
                    gtk::prelude::GtkWindowExt::focus(&w.window).is_some_and(|f| {
                        f == w.header.root
                            || (f.is_ancestor(&w.header.root)
                                && !f.is_ancestor(&w.header.editor.root))
                    });
                if focused_in_bar
                    && modifiers
                        .intersection(
                            gdk::ModifierType::CONTROL_MASK
                                | gdk::ModifierType::ALT_MASK
                                | gdk::ModifierType::SHIFT_MASK
                                | gdk::ModifierType::SUPER_MASK,
                        )
                        .is_empty()
                    && let Some(id) = w.header.editor.selected_item()
                {
                    match key {
                        gdk::Key::Delete | gdk::Key::BackSpace => {
                            w.dispatch(HeaderAction::Remove { id }.action())
                        }
                        gdk::Key::Left | gdk::Key::Right => {
                            w.header.editor.move_item(&w, id, key == gdk::Key::Right)
                        }
                        _ => return glib::Propagation::Proceed,
                    }
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        ));
        w.window.add_controller(keys);
    }
    pub fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        let projected = state.workspace.layout.header.projected_for(Platform::Gtk);
        let model = &projected;
        let editing = state.customization.header_editing;
        let was_editing = self.editing.replace(editing);
        let rebuild = was_editing != editing || self.model.borrow().as_ref() != Some(model);
        self.editor.refresh(model, editing);
        self.editor
            .refresh_canvas_info(state.workspace.layout.canvas_info.visible);
        self.handle.set_can_target(!editing);
        self.root.set_focusable(editing);
        if rebuild {
            // Replacement invalidates the native source. Never complete a
            // pending drop against a different workspace or newly rebuilt item.
            let drag = w
                .workspace_drag
                .borrow()
                .clone()
                .filter(|d| matches!(d.target, DragTarget::Header(_)));
            if let Some(drag) = drag {
                w.workspace_drag_input(ContactPhase::Cancel, drag.point, drag.sequence);
            }
            // Return focus from the shared context menu before recording the
            // item to restore across a model-driven widget replacement.
            w.dismiss_context();
            // Item widgets are projections. Preserve keyboard position across
            // their replacement, including removal of the focused item.
            let focused = gtk::prelude::GtkWindowExt::focus(&w.window).and_then(|focus| {
                self.items
                    .borrow()
                    .iter()
                    .position(|i| focus == i.root || focus.is_ancestor(&i.root))
            });
            let focused_id = focused.map(|index| self.items.borrow()[index].entry.id);
            self.drop.set(None);
            for popup in &self.overflow {
                popup.popdown();
            }
            // These are retained application controls, not disposable copies.
            for widget in [
                w.tab.clone().upcast::<gtk::Widget>(),
                w.workspaces.switcher.clone().upcast(),
                w.system_status.clock.clone().upcast(),
                w.system_status.battery.clone().upcast(),
            ] {
                if let Some(parent) = widget.parent() {
                    if let Some(stack) = parent.downcast_ref::<gtk::Stack>() {
                        stack.remove(&widget);
                    } else if let Some(row) = parent.downcast_ref::<gtk::Box>() {
                        row.remove(&widget);
                    } else if let Some(handle) = parent.downcast_ref::<gtk::WindowHandle>() {
                        handle.set_child(None::<&gtk::Widget>);
                    }
                }
            }
            for item in self.items.take() {
                self.root.remove(&item.root);
            }
            w.commands
                .borrow_mut()
                .retain(|(id, _)| *id != CommandId::ZenMode);
            *self.model.borrow_mut() = Some(model.clone());
            let items = model
                .entries()
                .map(|entry| self.build_item(w, entry, model.size, editing))
                .collect();
            *self.items.borrow_mut() = items;
            if editing && let Some(index) = focused {
                let items = self.items.borrow();
                let next = items
                    .iter()
                    .find(|i| Some(i.entry.id) == focused_id)
                    .or_else(|| items.get(index.min(items.len().saturating_sub(1))));
                if let Some(item) = next {
                    item.root.grab_focus();
                } else {
                    self.editor.focus();
                }
            }
            if editing && !was_editing {
                self.editor.focus();
            } else if !editing && was_editing {
                w.area.grab_focus();
            }
            self.root.queue_allocate();
            w.surface.queue_allocate();
        }
        for size in HeaderSize::ALL {
            let class = format!("header-{}", size.label().to_lowercase());
            if model.size == size {
                self.root.add_css_class(&class);
            } else {
                self.root.remove_css_class(&class);
            }
        }
        for menu in self.overflow.iter().chain(std::iter::once(&self.recovery)) {
            if let Some(image) = menu.child().and_downcast::<gtk::Image>() {
                image.set_pixel_size(model.size.icon());
            }
        }
        let clock = model.entries().any(|e| e.item == HeaderItem::Clock);
        let battery = model.entries().any(|e| e.item == HeaderItem::Battery);
        w.system_status.set_components(clock, battery);
        w.system_status.set_header_size(model.size);
        for item in self.items.borrow().iter() {
            if let Some(button) = &item.button {
                let (enabled, active) = match item.entry.item {
                    HeaderItem::Tool { control } => tool_state(state, control),
                    HeaderItem::Capy => (true, state.workspace.zen_mode),
                    // Full Screen is an action, not a selected drawing tool.
                    HeaderItem::Fullscreen => (true, false),
                    _ => (true, false),
                };
                button.set_sensitive(enabled || editing);
                let drawer = state
                    .customization
                    .drawer
                    .as_ref()
                    .is_some_and(|d| d.anchor == DrawerAnchor::Header { id: item.entry.id });
                selected(button, active);
                customization::drawer_origin(button, drawer.then_some(Edge::Bottom));
                if item.entry.item == HeaderItem::Fullscreen
                    && let Some(image) = button.child().and_downcast::<gtk::Image>()
                {
                    crate::icons::set(
                        &image,
                        Some(if state.fullscreen {
                            "layer-fullscreen-exit-symbolic"
                        } else {
                            "layer-fullscreen-enter-symbolic"
                        }),
                    );
                    button.set_tooltip_text(Some(if state.fullscreen {
                        "Leave Full Screen"
                    } else {
                        "Full Screen"
                    }));
                }
                if item.entry.item == HeaderItem::Capy {
                    if let Some(image) = button.child().and_downcast::<gtk::Image>() {
                        let icon = state
                            .commands
                            .iter()
                            .find(|c| c.id == CommandId::ZenMode)
                            .and_then(|c| c.icon)
                            .unwrap_or(ZenIcon::LookingUp.icon());
                        crate::icons::set(&image, Some(&format!("layer-{icon}-symbolic")));
                    }
                }
            }
            if let Some(compact) = &item.compact {
                let name = w
                    .workspaces
                    .manager
                    .as_ref()
                    .and_then(|m| {
                        m.active_id().and_then(|id| {
                            m.items()
                                .into_iter()
                                .find(|i| i.id == id)
                                .map(|i| i.metadata.name)
                        })
                    })
                    .unwrap_or_else(|| "Workspaces".into());
                compact.set_label(&name);
            }
        }
        for controls in &self.native {
            controls.set_visible(!w.window.is_fullscreen());
        }
        self.refresh_selection();
        self.root.queue_allocate();
    }
    fn refresh_selection(&self) {
        for item in self.items.borrow().iter() {
            if self.editing.get() && self.editor.selected_item() == Some(item.entry.id) {
                item.root.add_css_class("editing-selection");
            } else {
                item.root.remove_css_class("editing-selection");
            }
        }
    }
    pub fn is_editing(&self) -> bool {
        self.editing.get()
    }
    pub fn select_context_item(&self, w: &Workspace, id: u32) {
        let exists = self
            .model
            .borrow()
            .as_ref()
            .is_some_and(|m| m.entry(id).is_ok());
        if exists {
            self.editor.select(w, Some(id));
        }
    }
    fn build_item(
        &self,
        w: &Rc<Workspace>,
        entry: &HeaderEntry,
        size: HeaderSize,
        editing: bool,
    ) -> Item {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.set_widget_name(&format!("header-item-{}", entry.id));
        root.add_css_class("header-item");
        root.set_focusable(editing);
        root.update_property(&[gtk::accessible::Property::Label(&entry.item.label())]);
        let mut button = None;
        let mut compact = None;
        let content: gtk::Widget = match entry.item {
            HeaderItem::Capy
            | HeaderItem::Tool { .. }
            | HeaderItem::Settings
            | HeaderItem::Fullscreen => {
                let b = gtk::Button::new();
                b.add_css_class("flat");
                b.add_css_class("header-tool");
                b.set_tooltip_text(Some(&entry.item.label()));
                let icon = match entry.item {
                    HeaderItem::Tool {
                        control: ToolbarControl::Color,
                    } => "colors",
                    HeaderItem::Tool { control } => tool_choice(control).icon,
                    HeaderItem::Settings => "settings",
                    HeaderItem::Fullscreen => "fullscreen-enter",
                    _ => ZenIcon::LookingUp.icon(),
                };
                let image: gtk::Widget = if matches!(
                    entry.item,
                    HeaderItem::Tool {
                        control: ToolbarControl::Color
                    }
                ) {
                    w.customization.color_pair(w, size.icon())
                } else {
                    let image = crate::icons::image(&format!("layer-{icon}-symbolic"));
                    image.set_pixel_size(if entry.item == HeaderItem::Capy {
                        b.add_css_class("capy-button");
                        (size.tile() * 440. / 512.).round() as i32
                    } else {
                        size.icon()
                    });
                    image.upcast()
                };
                b.set_child(Some(&image));
                if entry.item
                    == (HeaderItem::Tool {
                        control: ToolbarControl::Divider,
                    })
                {
                    let line = gtk::DrawingArea::new();
                    line.set_draw_func(|area, cr, width, height| {
                        let c = area.color();
                        cr.set_source_rgba(c.red().into(), c.green().into(), c.blue().into(), 0.5);
                        cr.rectangle((width / 2) as f64, 10., 1., (height - 20).max(1) as f64);
                        let _ = cr.fill();
                    });
                    b.set_child(Some(&line));
                }
                let id = entry.id;
                let command = match entry.item {
                    HeaderItem::Capy => Some(CommandId::ZenMode),
                    HeaderItem::Settings => Some(CommandId::Settings),
                    HeaderItem::Fullscreen => Some(CommandId::Fullscreen),
                    _ => None,
                };
                if command == Some(CommandId::ZenMode) {
                    w.commands
                        .borrow_mut()
                        .push((CommandId::ZenMode, b.clone()));
                }
                b.connect_clicked(glib::clone!(
                    #[weak]
                    w,
                    move |_| {
                        if !w.header.editing.get() {
                            w.dispatch(if let Some(command) = command {
                                UiAction::Invoke { command }
                            } else {
                                UiAction::ActivateHeaderItem { id }
                            });
                        }
                    }
                ));
                button = Some(b.clone());
                b.upcast()
            }
            HeaderItem::Menu => {
                let menu = w.chrome_menu(ApplicationMenu::Primary);
                menu.set_child(Some(&crate::icons::image("layer-menu-symbolic")));
                menu.upcast()
            }
            HeaderItem::MenuLabels => {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                row.add_css_class("header-menu-labels");
                for id in ApplicationMenu::ALL {
                    row.append(&w.chrome_menu(id));
                }
                row.upcast()
            }
            HeaderItem::Workspaces => {
                let stack = gtk::Stack::new();
                stack.set_hhomogeneous(false);
                stack.set_vhomogeneous(false);
                w.workspaces.switcher.set_halign(gtk::Align::Center);
                stack.add_named(&w.workspaces.switcher, Some("full"));
                let menu = gtk::MenuButton::builder()
                    .label("Workspaces")
                    .tooltip_text("Switch workspace")
                    .build();
                menu.add_css_class("flat");
                menu.add_css_class("chrome-control");
                menu.set_widget_name("header-workspace-selector");
                menu.set_create_popup_func(glib::clone!(
                    #[weak]
                    w,
                    move |button| {
                        button.set_popover(Some(&w.workspaces.switcher_popup(&w)));
                    }
                ));
                stack.add_named(&menu, Some("compact"));
                stack.set_visible_child_name("full");
                compact = Some(menu);
                stack.upcast()
            }
            HeaderItem::DocumentTitle => w.tab.clone().upcast(),
            HeaderItem::Clock => w.system_status.clock.clone().upcast(),
            HeaderItem::Battery => w.system_status.battery.clone().upcast(),
            HeaderItem::Space => gtk::Box::new(gtk::Orientation::Horizontal, 0).upcast(),
        };
        fn size_images(widget: &gtk::Widget, size: i32) {
            if let Some(image) = widget.downcast_ref::<gtk::Image>() {
                image.set_pixel_size(size);
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                size_images(&widget, size);
            }
        }
        if entry.item == HeaderItem::Menu {
            size_images(&content, size.icon());
        }
        let content = if matches!(entry.item, HeaderItem::Clock | HeaderItem::Battery) {
            let stack = gtk::Stack::new();
            stack.set_hhomogeneous(false);
            stack.set_vhomogeneous(false);
            stack.add_named(&content, Some("value"));
            let placeholder = gtk::Label::new(Some(&entry.item.label()));
            stack.add_named(&placeholder, Some("placeholder"));
            stack.set_visible_child_name(if content.is_visible() {
                "value"
            } else {
                "placeholder"
            });
            stack.upcast::<gtk::Widget>()
        } else {
            content
        };
        let content = if matches!(
            entry.item,
            HeaderItem::DocumentTitle | HeaderItem::Clock | HeaderItem::Battery | HeaderItem::Space
        ) {
            let handle = gtk::WindowHandle::new();
            if entry.item != HeaderItem::Space {
                handle.add_css_class("header-readout");
            }
            handle.set_child(Some(&content));
            handle.upcast::<gtk::Widget>()
        } else {
            content
        };
        content.set_hexpand(true);
        content.set_valign(
            if matches!(
                entry.item,
                HeaderItem::MenuLabels
                    | HeaderItem::Workspaces
                    | HeaderItem::DocumentTitle
                    | HeaderItem::Clock
                    | HeaderItem::Battery
            ) {
                gtk::Align::Center
            } else {
                gtk::Align::Fill
            },
        );
        content.set_can_target(!editing);
        content.set_can_focus(!editing);
        if editing {
            let grip = crate::icons::image("layer-grip-symbolic");
            grip.add_css_class("header-grip");
            grip.set_valign(gtk::Align::Center);
            grip.set_size_request(20, 28);
            grip.set_widget_name(&format!("header-grip-{}", entry.id));
            grip.set_tooltip_text(Some("Drag to move this item"));
            root.append(&grip);
            w.register_drag(&root, DragTarget::Header(HeaderDragSource::Item(entry.id)));
        }
        root.append(&content);
        self.root.add(&root);
        w.install_context(&root, ContextTarget::Header { id: Some(entry.id) });
        let id = entry.id;
        root.connect_has_focus_notify(glib::clone!(
            #[weak]
            w,
            move |root| {
                if root.has_focus() && w.header.editing.get() {
                    w.header.select_context_item(&w, id);
                }
            }
        ));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            w,
            #[weak]
            root,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                if key == gdk::Key::Menu
                    || (key == gdk::Key::F10 && modifiers.contains(gdk::ModifierType::SHIFT_MASK))
                {
                    w.show_context(
                        root.upcast_ref(),
                        ContextTarget::Header { id: Some(id) },
                        0.,
                        root.height() as f64,
                    );
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        ));
        root.add_controller(keys);
        Item {
            entry: entry.clone(),
            root,
            content,
            button,
            compact,
        }
    }
    fn metrics(&self, size: HeaderSize) -> Vec<HeaderMetric> {
        self.items
            .borrow()
            .iter()
            .map(|item| {
                if matches!(item.entry.item, HeaderItem::Clock | HeaderItem::Battery)
                    && !self.editing.get()
                    && item
                        .content
                        .first_child()
                        .and_downcast::<gtk::Stack>()
                        .is_some_and(|s| s.visible_child_name().as_deref() == Some("placeholder"))
                {
                    return HeaderMetric {
                        id: item.entry.id,
                        width: 0.,
                        compact: 0.,
                    };
                }
                let grip = if self.editing.get() { 20. } else { 0. };
                let natural = match item.entry.item {
                    HeaderItem::Workspaces => item.switcher_width().max(144.),
                    HeaderItem::DocumentTitle => 180.,
                    HeaderItem::Space
                    | HeaderItem::Settings
                    | HeaderItem::Fullscreen
                    | HeaderItem::Capy
                    | HeaderItem::Tool { .. }
                    | HeaderItem::Menu => size.tile(),
                    _ => item
                        .content
                        .measure(gtk::Orientation::Horizontal, -1)
                        .1
                        .max(size.tile() as i32) as f32,
                };
                let compact = match item.entry.item {
                    HeaderItem::Workspaces => 144.,
                    HeaderItem::DocumentTitle => 80.,
                    _ => natural,
                };
                HeaderMetric {
                    id: item.entry.id,
                    width: natural + grip,
                    compact: compact + grip,
                }
            })
            .collect()
    }
    fn allocate(&self, w: &Rc<Workspace>, width: f32) {
        let Some(model) = self.model.borrow().clone() else {
            return;
        };
        for item in self
            .items
            .borrow()
            .iter()
            .filter(|i| matches!(i.entry.item, HeaderItem::Clock | HeaderItem::Battery))
        {
            if let Some(stack) = item.content.first_child().and_downcast::<gtk::Stack>() {
                let visible = match item.entry.item {
                    HeaderItem::Clock => w.system_status.clock.is_visible(),
                    _ => w.system_status.battery.is_visible(),
                };
                stack.set_visible_child_name(if visible { "value" } else { "placeholder" });
            }
        }
        let height = model.size.height();
        allocate_at(
            self.handle.upcast_ref(),
            Bounds {
                x: 0.,
                y: 0.,
                width,
                height,
            },
        );
        let native = self.native.each_ref().map(|c| {
            if c.is_visible() {
                let natural = c.measure(gtk::Orientation::Horizontal, -1).1 as f32;
                if natural > 0. { natural + 12. } else { 0. }
            } else {
                0.
            }
        });
        for (i, c) in self.native.iter().enumerate() {
            if c.is_visible() {
                allocate_at(
                    c.upcast_ref(),
                    Bounds {
                        x: if i == 0 { 6. } else { width - native[i] + 6. },
                        y: 6.,
                        width: (native[i] - 12.).max(0.),
                        height: model.size.tile(),
                    },
                );
            }
        }
        let recovery = !self.editing.get()
            && !model.entries().any(|e| {
                matches!(
                    e.item,
                    HeaderItem::Menu
                        | HeaderItem::Capy
                        | HeaderItem::Workspaces
                        | HeaderItem::MenuLabels
                )
            });
        self.recovery.set_child_visible(recovery);
        if recovery {
            allocate_at(
                self.recovery.upcast_ref(),
                Bounds {
                    x: width - native[1] - model.size.tile(),
                    y: 6.,
                    width: model.size.tile(),
                    height: model.size.tile(),
                },
            );
        }
        let insets = [
            native[0],
            native[1] + if recovery { model.size.tile() } else { 0. },
        ];
        self.insets.set(insets);
        let geometry = model.resolve(width, insets, &self.metrics(model.size), self.editing.get());
        if self
            .drag
            .borrow()
            .as_ref()
            .is_some_and(|drag| !drag.fits(width, insets))
        {
            let pending = w.workspace_drag.borrow().clone();
            if let Some(pending) = pending {
                w.workspace_drag_input(ContactPhase::Cancel, pending.point, pending.sequence);
            }
        }
        for item in self.items.borrow().iter() {
            let allocation = geometry.items.iter().find(|m| m.id == item.entry.id);
            if allocation.is_none()
                && self.editing.get()
                && gtk::prelude::GtkWindowExt::focus(&w.window)
                    .is_some_and(|f| f == item.root || f.is_ancestor(&item.root))
            {
                // A move or resize can put the focused item in overflow. Keep
                // its selection editable from the panel instead of losing focus.
                self.root.grab_focus();
            }
            item.root.set_child_visible(allocation.is_some());
            if let Some(a) = allocation {
                if item.compact.is_some()
                    && let Some(stack) = item.content.downcast_ref::<gtk::Stack>()
                {
                    let full = item.switcher_width();
                    let grip = if self.editing.get() { 20. } else { 0. };
                    stack.set_visible_child_name(
                        if full > 0. && a.bounds.width - grip + 0.01 >= full {
                            "full"
                        } else {
                            "compact"
                        },
                    );
                }
                allocate_at(item.root.upcast_ref(), a.bounds);
            }
        }
        for (i, button) in self.overflow.iter().enumerate() {
            button.set_child_visible(geometry.overflow[i].is_some());
            if let Some(b) = geometry.overflow[i] {
                allocate_at(button.upcast_ref(), b);
            }
        }
        if self.editing.get() {
            let old_height = self.editor.height.get();
            self.editor
                .allocate(width, height, w.surface.height() as f32);
            if old_height != self.editor.height.get() {
                w.surface.queue_allocate();
            }
        }
        *self.geometry.borrow_mut() = geometry;
        if !self.measuring.replace(true) {
            glib::idle_add_local_once(glib::clone!(
                #[weak]
                w,
                move || {
                    w.header.measuring.set(false);
                    let items = w.header.measurements();
                    w.dispatch(UiAction::MeasureHeader {
                        height: w.header.height(),
                        items,
                    });
                }
            ));
        }
    }
    fn measurements(&self) -> Vec<HeaderItemBounds> {
        let geometry = self.geometry.borrow();
        let mut items = geometry.items.clone();
        for (zone, ids) in geometry.hidden.iter().enumerate() {
            if let Some(bounds) = geometry.overflow[zone] {
                items.extend(ids.iter().map(|id| HeaderItemBounds { id: *id, bounds }));
            }
        }
        items.retain(|m| {
            self.model
                .borrow()
                .as_ref()
                .is_some_and(|model| model.entry(m.id).is_ok())
        });
        items
    }
    pub fn drawer_button(&self, id: u32) -> Option<gtk::Button> {
        self.items
            .borrow()
            .iter()
            .find(|i| i.entry.id == id && i.root.is_child_visible())
            .and_then(|i| i.button.clone())
    }
    pub fn drop_at(&self, point: [f32; 2]) -> Option<(HeaderZone, Option<u32>)> {
        if !self.editing.get() {
            return None;
        }
        let height = self.model.borrow().as_ref()?.size.height();
        self.geometry.borrow().destination(point, height)
    }
    pub fn clear_drop(&self) {
        self.drop.set(None);
        self.root.queue_draw();
    }
    fn snapshot(&self, snapshot: &gtk::Snapshot) {
        if !self.editing.get() {
            return;
        }
        let geometry = self.geometry.borrow();
        for (i, b) in geometry.zones.iter().enumerate() {
            let color = gdk::RGBA::new(
                0.4,
                0.65,
                0.9,
                if self.drop.get().is_some_and(|(z, _)| z.index() == i) {
                    0.8
                } else {
                    0.35
                },
            );
            for line in [
                Bounds { height: 1., ..*b },
                Bounds {
                    y: b.y + b.height - 1.,
                    height: 1.,
                    ..*b
                },
            ] {
                snapshot.append_color(
                    &color,
                    &gtk::graphene::Rect::new(line.x, line.y, line.width, line.height),
                );
            }
        }
    }
    fn overflow_popup(&self, w: &Rc<Workspace>, zone: usize) -> gtk::Popover {
        let popover = gtk::Popover::new();
        popover.set_widget_name("header-overflow-popup");
        let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
        if let Some(model) = self.model.borrow().as_ref() {
            for id in &self.geometry.borrow().hidden[zone] {
                let Ok(entry) = model.entry(*id) else {
                    continue;
                };
                let button = gtk::Button::with_label(&entry.item.label());
                button.set_widget_name(&format!("header-overflow-item-{id}"));
                if let HeaderItem::Tool { control } = entry.item
                    && let Some(state) = w.gpu.borrow().as_ref().map(|g| g.session.state().clone())
                {
                    let (enabled, active) = tool_state(&state, control);
                    button.set_sensitive(enabled || self.editing.get());
                    selected(&button, active);
                }
                let id = *id;
                button.connect_clicked(glib::clone!(
                    #[weak]
                    w,
                    #[weak]
                    popover,
                    move |_| {
                        if w.header.editing.get() {
                            popover.popdown();
                            w.header.editor.select(&w, Some(id));
                            w.header.root.grab_focus();
                        } else {
                            popover.popdown();
                            w.header.activate_overflow(&w, id);
                        }
                    }
                ));
                w.install_context(&button, ContextTarget::Header { id: Some(id) });
                list.append(&button);
            }
        }
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .max_content_height(400)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        popover.set_child(Some(&scroll));
        w.watch_popover(&popover);
        popover
    }
    fn activate_overflow(&self, w: &Rc<Workspace>, id: u32) {
        let Some(entry) = self
            .model
            .borrow()
            .as_ref()
            .and_then(|m| m.entry(id).ok())
            .cloned()
        else {
            return;
        };
        match entry.item {
            HeaderItem::Tool { .. } => {
                w.dispatch(UiAction::MeasureHeader {
                    height: self.height(),
                    items: self.measurements(),
                });
                w.dispatch(UiAction::ActivateHeaderItem { id });
            }
            HeaderItem::Capy | HeaderItem::Settings | HeaderItem::Fullscreen => {
                w.dispatch(UiAction::Invoke {
                    command: match entry.item {
                        HeaderItem::Capy => CommandId::ZenMode,
                        HeaderItem::Settings => CommandId::Settings,
                        _ => CommandId::Fullscreen,
                    },
                })
            }
            _ => {
                let zone = self
                    .model
                    .borrow()
                    .as_ref()
                    .and_then(|m| m.location(id))
                    .map(|(z, _)| z.index())
                    .unwrap_or(0);
                if entry.item == HeaderItem::Workspaces {
                    let popup = w.workspaces.switcher_popup(w);
                    self.overflow[zone].set_popover(Some(&popup));
                    popup.popup();
                } else if matches!(entry.item, HeaderItem::Menu | HeaderItem::MenuLabels) {
                    let model = w
                        .gpu
                        .borrow()
                        .as_ref()
                        .map(|g| g.session.application_menu(ApplicationMenu::Primary));
                    if let Some(model) = model {
                        // A transient nested menu is anchored to the visible overflow
                        // control, never to the unallocated hidden component.
                        let popup = gtk::PopoverMenu::from_model(gtk::gio::MenuModel::NONE);
                        w.populate_workspace_menu(&popup, model);
                        w.watch_popover(popup.upcast_ref());
                        popup.set_parent(&self.overflow[zone]);
                        popup.connect_closed(|p| p.unparent());
                        popup.popup();
                    }
                } else {
                    w.show_context(
                        self.overflow[zone].upcast_ref(),
                        ContextTarget::Header { id: Some(id) },
                        0.,
                        0.,
                    );
                }
            }
        }
    }
}
