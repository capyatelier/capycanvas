//! GTK projection of workspace-owned window-bar items.
use super::*;

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
    editor: gtk::Box,
    size: gtk::DropDown,
    model: RefCell<Option<HeaderLayout>>,
    pub editing: Cell<bool>,
    geometry: RefCell<HeaderGeometry>,
    measuring: Cell<bool>,
    drop: Cell<Option<(HeaderZone, Option<u32>)>>,
}
impl Header {
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
                .icon_name("layer-menu-symbolic")
                .tooltip_text(format!(
                    "More {} window-bar items",
                    HeaderZone::ALL[i].label().to_lowercase()
                ))
                .build();
            b.set_widget_name(&format!("header-overflow-{i}"));
            b.add_css_class("flat");
            root.add(&b);
            b
        });
        let recovery = gtk::MenuButton::builder()
            .icon_name("layer-menu-symbolic")
            .tooltip_text("Window bar recovery: menus and customization")
            .build();
        recovery.set_widget_name("header-recovery");
        recovery.add_css_class("flat");
        root.add(&recovery);
        let editor = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        editor.set_widget_name("header-editor");
        editor.add_css_class("header-editor");
        let size = gtk::DropDown::from_strings(&["Small", "Medium", "Large"]);
        size.set_widget_name("header-size");
        root.add(&editor);
        Self {
            root,
            handle,
            native,
            overflow,
            recovery,
            editor,
            size,
            items: RefCell::new(Vec::new()),
            model: RefCell::new(None),
            editing: Cell::new(false),
            geometry: RefCell::new(HeaderGeometry::default()),
            measuring: Cell::new(false),
            drop: Cell::new(None),
        }
    }
    pub fn height(&self) -> f32 {
        self.model
            .borrow()
            .as_ref()
            .map_or(HEADER_HEIGHT, |m| m.size.height())
            + if self.editing.get() { 48. } else { 0. }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        *self.root.imp().owner.borrow_mut() = Rc::downgrade(w);
        w.system_status.battery.connect_visible_notify(glib::clone!(
            #[weak]
            w,
            move |_| w.header.root.queue_allocate()
        ));
        let add = gtk::MenuButton::builder().label("Add Item…").build();
        add.set_widget_name("header-add-item");
        add.set_create_popup_func(glib::clone!(
            #[weak]
            w,
            move |button| {
                button.set_popover(Some(&w.header.add_picker(&w)));
            }
        ));
        self.editor.append(&add);
        self.editor.append(&self.size);
        self.size.connect_selected_notify(glib::clone!(
            #[weak]
            w,
            move |size| {
                if !w.refreshing.get() {
                    w.dispatch(
                        HeaderAction::SetSize {
                            size: HeaderSize::ALL[size.selected().min(2) as usize],
                        }
                        .action(),
                    );
                }
            }
        ));
        let options = gtk::MenuButton::builder().label("Options").build();
        options.set_widget_name("header-options");
        options.set_create_popup_func(glib::clone!(
            #[weak]
            w,
            move |button| {
                let Some(state) = w.gpu.borrow().as_ref().map(|g| g.session.state().clone()) else {
                    return;
                };
                let info = state.workspace.layout.canvas_info;
                let mut visible = ContextMenuItem::command(
                    "Show Zoom and Rotation",
                    HeaderAction::CanvasInfo {
                        visible: !info.visible,
                        anchor: info.anchor,
                    }
                    .action(),
                );
                visible.selected = Some(info.visible);
                let positions = OverlayAnchor::ALL
                    .into_iter()
                    .map(|anchor| {
                        let mut item = ContextMenuItem::command(
                            anchor.label(),
                            HeaderAction::CanvasInfo {
                                visible: info.visible,
                                anchor,
                            }
                            .action(),
                        );
                        item.selected = Some(info.anchor == anchor);
                        item
                    })
                    .collect();
                let popup = gtk::PopoverMenu::from_model(gtk::gio::MenuModel::NONE);
                w.populate_workspace_menu(
                    &popup,
                    ContextMenu {
                        title: "Workspace UI".into(),
                        sections: vec![
                            vec![visible],
                            positions,
                            vec![ContextMenuItem::command(
                                "Restore Window Bar Defaults",
                                HeaderAction::RestoreDefaults.action(),
                            )],
                        ],
                    },
                );
                w.watch_popover(popup.upcast_ref());
                button.set_popover(Some(&popup));
            }
        ));
        self.editor.append(&options);
        for command in [CommandId::UndoWorkspace, CommandId::RedoWorkspace] {
            let b = w.command_button(command);
            self.editor.append(&b);
        }
        let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        space.set_hexpand(true);
        self.editor.append(&space);
        let done = gtk::Button::with_label("Done");
        done.set_widget_name("header-edit-done");
        done.add_css_class("suggested-action");
        done.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| w.dispatch(HeaderAction::Edit { editing: false }.action())
        ));
        self.editor.append(&done);
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
            move |_, key, _, _modifiers| {
                if key == gdk::Key::Escape && w.header.editing.get() {
                    let drag = w.workspace_drag.borrow().clone();
                    if let Some(drag) = drag {
                        w.workspace_drag_input(ContactPhase::Cancel, drag.point, drag.sequence);
                        return glib::Propagation::Stop;
                    }
                    if w.popovers
                        .borrow()
                        .iter()
                        .filter_map(|p| p.upgrade())
                        .any(|p| p.is_visible())
                    {
                        return glib::Propagation::Proceed;
                    }
                    w.dispatch(HeaderAction::Edit { editing: false }.action());
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        ));
        w.window.add_controller(keys);
    }
    pub fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        let model = &state.workspace.layout.header;
        let editing = state.customization.header_editing;
        let rebuild =
            self.editing.replace(editing) != editing || self.model.borrow().as_ref() != Some(model);
        self.editor.set_visible(editing);
        self.size.set_selected(model.size as u32);
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
            w.dismiss_context();
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
                    self.size.grab_focus();
                }
            }
            self.root.queue_allocate();
            w.surface.queue_allocate();
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
                    _ => (true, false),
                };
                button.set_sensitive(enabled || editing);
                selected(
                    button,
                    active
                        || state.customization.drawer.as_ref().is_some_and(|d| {
                            d.anchor == DrawerAnchor::Header { id: item.entry.id }
                        }),
                );
                if item.entry.item == HeaderItem::Capy {
                    if let Some(image) = button.child().and_downcast::<gtk::Image>() {
                        let icon = state
                            .commands
                            .iter()
                            .find(|c| c.id == CommandId::ZenMode)
                            .and_then(|c| c.icon)
                            .unwrap_or("capy-looking-up");
                        image.set_icon_name(Some(&format!("layer-{icon}-symbolic")));
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
        self.root.queue_allocate();
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
        root.add_css_class("drag-hold");
        root.set_focusable(editing);
        root.update_property(&[gtk::accessible::Property::Label(&entry.item.label())]);
        let mut button = None;
        let mut compact = None;
        let content: gtk::Widget = match entry.item {
            HeaderItem::Capy | HeaderItem::Tool { .. } => {
                let b = gtk::Button::new();
                b.add_css_class("flat");
                b.add_css_class("header-tool");
                b.set_tooltip_text(Some(&entry.item.label()));
                let icon = match entry.item {
                    HeaderItem::Tool {
                        control: ToolbarControl::Color,
                    } => "colors",
                    HeaderItem::Tool { control } => tool_choice(control).icon,
                    _ => ZenIcon::LookingUp.icon(),
                };
                let image = gtk::Image::from_icon_name(&format!("layer-{icon}-symbolic"));
                image.set_pixel_size(size.icon());
                b.set_child(Some(&image));
                if entry.item
                    == (HeaderItem::Tool {
                        control: ToolbarControl::Color,
                    })
                {
                    b.add_css_class("brush-color");
                    #[allow(deprecated)]
                    b.style_context().add_provider(
                        &w.customization.palette,
                        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                    );
                }
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
                let capy = entry.item == HeaderItem::Capy;
                if capy {
                    w.commands
                        .borrow_mut()
                        .push((CommandId::ZenMode, b.clone()));
                }
                b.connect_clicked(glib::clone!(
                    #[weak]
                    w,
                    move |_| {
                        if !w.header.editing.get() {
                            w.dispatch(if capy {
                                UiAction::Invoke {
                                    command: CommandId::ZenMode,
                                }
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
                menu.set_icon_name("layer-menu-symbolic");
                menu.upcast()
            }
            HeaderItem::MenuLabels => {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
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
                let menu = w.chrome_menu(ApplicationMenu::Window);
                menu.set_label("Workspaces");
                menu.set_widget_name("header-workspace-selector");
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
        let content = if entry.item == HeaderItem::Battery {
            let stack = gtk::Stack::new();
            stack.set_hhomogeneous(false);
            stack.set_vhomogeneous(false);
            stack.add_named(&content, Some("value"));
            let placeholder = gtk::Label::new(Some("Battery"));
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
            handle.set_child(Some(&content));
            handle.upcast::<gtk::Widget>()
        } else {
            content
        };
        content.set_hexpand(true);
        content.set_valign(gtk::Align::Fill);
        content.set_can_target(!editing);
        content.set_can_focus(!editing);
        if editing {
            let grip = gtk::Button::with_label("⠿");
            grip.add_css_class("flat");
            grip.add_css_class("header-grip");
            grip.set_widget_name(&format!("header-grip-{}", entry.id));
            grip.set_tooltip_text(Some("Drag to move this item"));
            w.register_drag(&grip, DragTarget::Header(entry.id));
            root.append(&grip);
        }
        root.append(&content);
        self.root.add(&root);
        w.register_drag(&root, DragTarget::Header(entry.id));
        w.install_context(&root, ContextTarget::Header { id: Some(entry.id) });
        let id = entry.id;
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_released(glib::clone!(
            #[weak]
            w,
            move |g, _, x, y| {
                if w.header.editing.get()
                    && let Some(widget) = g.widget()
                    && x >= 0.
                    && y >= 0.
                    && x <= widget.width() as f64
                    && y <= widget.height() as f64
                {
                    w.show_context(&widget, ContextTarget::Header { id: Some(id) }, x, y);
                }
            }
        ));
        root.add_controller(click);
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
                if key == gdk::Key::Delete && w.header.editing.get() {
                    w.dispatch(HeaderAction::Remove { id }.action());
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
                if item.entry.item == HeaderItem::Battery
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
                    HeaderItem::Space => size.tile() / 2.,
                    HeaderItem::Capy | HeaderItem::Tool { .. } | HeaderItem::Menu => size.tile(),
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
            .filter(|i| i.entry.item == HeaderItem::Battery)
        {
            if let Some(stack) = item.content.first_child().and_downcast::<gtk::Stack>() {
                stack.set_visible_child_name(if w.system_status.battery.is_visible() {
                    "value"
                } else {
                    "placeholder"
                });
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
                c.measure(gtk::Orientation::Horizontal, -1).1 as f32
            } else {
                0.
            }
        });
        for (i, c) in self.native.iter().enumerate() {
            if c.is_visible() {
                allocate_at(
                    c.upcast_ref(),
                    Bounds {
                        x: if i == 0 { 0. } else { width - native[i] },
                        y: 0.,
                        width: native[i],
                        height,
                    },
                );
            }
        }
        let recovery = !self.editing.get()
            && !model.entries().any(|e| {
                matches!(
                    e.item,
                    HeaderItem::Menu | HeaderItem::Capy | HeaderItem::Workspaces
                ) || (e.item == HeaderItem::MenuLabels && model.show_menu_labels)
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
        let geometry = model.resolve(width, insets, &self.metrics(model.size), self.editing.get());
        for item in self.items.borrow().iter() {
            let allocation = geometry.items.iter().find(|m| m.id == item.entry.id);
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
            allocate_at(
                self.editor.upcast_ref(),
                Bounds {
                    x: 6.,
                    y: height,
                    width: (width - 12.).max(1.),
                    height: 42.,
                },
            );
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
        let geometry = self.geometry.borrow();
        HeaderZone::ALL.into_iter().find_map(|zone| {
            let bounds = geometry.zones[zone.index()];
            if !bounds.contains(point[0], point[1]) {
                return None;
            }
            let before = geometry
                .items
                .iter()
                .filter(|m| bounds.contains(m.bounds.x + m.bounds.width / 2., m.bounds.y + 1.))
                .find(|m| point[0] < m.bounds.x + m.bounds.width / 2.)
                .map(|m| m.id);
            Some((zone, before))
        })
    }
    pub fn drag_motion(&self, point: [f32; 2]) {
        self.drop.set(self.drop_at(point));
        self.root.queue_draw();
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
        if let Some((zone, before)) = self.drop.get() {
            let bounds = geometry.zones[zone.index()];
            let x = before
                .and_then(|id| {
                    geometry
                        .items
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.bounds.x)
                })
                .unwrap_or_else(|| {
                    geometry
                        .items
                        .iter()
                        .filter(|m| bounds.contains(m.bounds.x + 1., m.bounds.y + 1.))
                        .map(|m| m.bounds.x + m.bounds.width)
                        .fold(bounds.x, f32::max)
                });
            snapshot.append_color(
                &gdk::RGBA::new(0.3, 0.65, 1., 1.),
                &gtk::graphene::Rect::new(x - 1., bounds.y, 2., bounds.height),
            );
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
                    move |button| {
                        if w.header.editing.get() {
                            w.show_context(
                                button.upcast_ref(),
                                ContextTarget::Header { id: Some(id) },
                                0.,
                                0.,
                            );
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
            HeaderItem::Capy => w.dispatch(UiAction::Invoke {
                command: CommandId::ZenMode,
            }),
            _ => {
                let zone = self
                    .model
                    .borrow()
                    .as_ref()
                    .and_then(|m| m.location(id))
                    .map(|(z, _)| z.index())
                    .unwrap_or(0);
                if matches!(
                    entry.item,
                    HeaderItem::Menu | HeaderItem::MenuLabels | HeaderItem::Workspaces
                ) {
                    let menu = if entry.item == HeaderItem::Workspaces {
                        ApplicationMenu::Window
                    } else {
                        ApplicationMenu::Primary
                    };
                    let model = w
                        .gpu
                        .borrow()
                        .as_ref()
                        .map(|g| g.session.application_menu(menu));
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
    fn add_picker(&self, w: &Rc<Workspace>) -> gtk::Popover {
        let popup = gtk::Popover::new();
        popup.set_widget_name("header-add-picker");
        let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let zone = gtk::DropDown::from_strings(&["Left", "Center", "Right"]);
        zone.set_widget_name("header-add-zone");
        body.append(&zone);
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search controls and tools"));
        search.set_widget_name("header-add-search");
        body.append(&search);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_width(280)
            .min_content_height(300)
            .max_content_height(420)
            .child(&list)
            .build();
        body.append(&scroll);
        let platform = Platform::Gtk;
        let mut entries = HeaderItem::COMPONENTS.to_vec();
        entries.extend(layer_ui::header_tool_catalog(platform));
        let mut rows = Vec::new();
        for item in entries {
            let label = item.label();
            let button = gtk::Button::with_label(&label);
            button.set_widget_name(&format!(
                "header-add-{}",
                label.to_lowercase().replace(' ', "-")
            ));
            button.set_sensitive(self.model.borrow().as_ref().is_some_and(|m| {
                m.entries().count() < 128
                    && (!item.singleton() || !m.entries().any(|e| e.item == item))
            }));
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                #[weak]
                popup,
                #[weak]
                zone,
                move |_| {
                    let zone = HeaderZone::ALL[zone.selected().min(2) as usize];
                    popup.popdown();
                    w.dispatch(
                        HeaderAction::Add {
                            zone,
                            before: None,
                            item,
                        }
                        .action(),
                    );
                }
            ));
            list.append(&button);
            rows.push((label.to_lowercase(), button));
        }
        search.connect_search_changed(move |search| {
            let query = search.text().to_lowercase();
            for (text, row) in &rows {
                row.set_visible(query.split_whitespace().all(|word| text.contains(word)));
            }
        });
        popup.set_child(Some(&body));
        w.watch_popover(&popup);
        popup
    }
}
