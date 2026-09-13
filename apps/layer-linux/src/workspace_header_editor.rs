//! Inline window-bar editing chrome. Tool search uses the existing modal picker.
use super::*;

pub(super) struct Editor {
    pub root: gtk::Box,
    pub size: gtk::DropDown,
    pub zones: [gtk::ToggleButton; 3],
    destination: gtk::Label,
    remove: gtk::Button,
    add_tools: gtk::Button,
    components: Vec<(HeaderItem, gtk::Box)>,
    pub insertion: Cell<(HeaderZone, Option<u32>)>,
    selected: Cell<Option<u32>>,
    shown: Cell<bool>,
    pub height: Cell<f32>,
}
impl Editor {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.set_widget_name("header-editor");
        root.add_css_class("header-editor");
        let size = gtk::DropDown::from_strings(&["Small", "Medium", "Large"]);
        size.set_widget_name("header-size");
        size.set_tooltip_text(Some("Window-bar size"));
        let zones = std::array::from_fn(|i| {
            let button = gtk::ToggleButton::with_label(HeaderZone::ALL[i].label());
            button.set_widget_name(&format!("header-zone-{i}"));
            button.add_css_class("header-zone");
            button.set_tooltip_text(Some(&format!(
                "Add at the end of the {} region",
                HeaderZone::ALL[i].label().to_lowercase()
            )));
            button
        });
        let destination = gtk::Label::new(None);
        destination.set_widget_name("header-insertion-label");
        destination.set_xalign(0.);
        destination.set_hexpand(true);
        destination.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let remove = gtk::Button::with_label("Remove");
        remove.set_widget_name("header-remove-item");
        let add_tools = gtk::Button::with_label("Add Tools…");
        add_tools.set_widget_name("header-add-tools");
        Self {
            root,
            size,
            zones,
            destination,
            remove,
            add_tools,
            components: HeaderItem::COMPONENTS
                .into_iter()
                .map(|item| (item, gtk::Box::new(gtk::Orientation::Horizontal, 0)))
                .collect(),
            insertion: Cell::new((HeaderZone::Left, None)),
            selected: Cell::new(None),
            shown: Cell::new(false),
            height: Cell::new(0.),
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        // Two groups wrap as units on narrow windows; Cancel and Done stay together.
        let controls = adw::WrapBox::new();
        controls.set_child_spacing(12);
        controls.set_line_spacing(6);
        controls.set_justify(adw::JustifyMode::Spread);
        controls.set_justify_last_line(true);
        let settings = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        settings.append(&self.size);
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
                let visible = state.workspace.layout.canvas_info.visible;
                let mut info = ContextMenuItem::command(
                    "Show Zoom and Rotation",
                    HeaderAction::CanvasInfo { visible: !visible }.action(),
                );
                info.selected = Some(visible);
                let popup = gtk::PopoverMenu::from_model(gtk::gio::MenuModel::NONE);
                w.populate_workspace_menu(
                    &popup,
                    ContextMenu {
                        title: "Workspace UI".into(),
                        sections: vec![
                            vec![info],
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
        settings.append(&options);
        controls.append(&settings);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        self.add_tools.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                let (zone, before) = w.header.editor.insertion.get();
                w.dispatch(HeaderAction::InsertTools { zone, before }.action());
            }
        ));
        actions.append(&self.add_tools);
        let cancel = w.action_button("Cancel", HeaderAction::Cancel.action());
        cancel.set_widget_name("header-edit-cancel");
        actions.append(&cancel);
        let done = w.action_button("Done", HeaderAction::Edit { editing: false }.action());
        done.set_widget_name("header-edit-done");
        done.add_css_class("suggested-action");
        actions.append(&done);
        controls.append(&actions);
        self.root.append(&controls);

        let destination = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        destination.append(&self.destination);
        self.remove.add_css_class("flat");
        self.remove.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                if let Some(id) = w.header.editor.selected.get() {
                    w.dispatch(HeaderAction::Remove { id }.action());
                }
            }
        ));
        destination.append(&self.remove);
        self.root.append(&destination);
        let palette = adw::WrapBox::new();
        palette.set_widget_name("header-components");
        palette.set_child_spacing(6);
        palette.set_line_spacing(6);
        for (item, chip) in &self.components {
            let item = *item;
            let (label, icon) = match item {
                HeaderItem::Capy => ("Capy", "layer-capy-looking-up-symbolic"),
                HeaderItem::Menu => ("Main Menu", "layer-menu-symbolic"),
                HeaderItem::MenuLabels => ("Menu Labels", "view-list-symbolic"),
                HeaderItem::Workspaces => ("Workspaces", "view-grid-symbolic"),
                HeaderItem::DocumentTitle => ("Document Title", "text-x-generic-symbolic"),
                HeaderItem::Clock => ("Clock", "preferences-system-time-symbolic"),
                HeaderItem::Battery => ("Battery", "battery-level-100-symbolic"),
                HeaderItem::Space => ("Space", "insert-object-symbolic"),
                _ => unreachable!(),
            };
            let name = item.label().to_lowercase().replace(' ', "-");
            chip.set_widget_name(&format!("header-component-{name}"));
            chip.add_css_class("header-component");
            let grip = gtk::Button::from_icon_name("layer-grip-symbolic");
            grip.add_css_class("header-component-grip");
            grip.add_css_class("flat");
            grip.set_valign(gtk::Align::Center);
            grip.set_widget_name(&format!("header-add-grip-{name}"));
            grip.set_tooltip_text(Some(&format!("Drag {label} into the window bar")));
            w.register_drag(&grip, DragTarget::HeaderAdd(item));
            chip.append(&grip);
            let button = gtk::Button::new();
            button.add_css_class("flat");
            button.add_css_class("drag-hold");
            button.set_widget_name(&format!("header-add-{name}"));
            button.set_tooltip_text(Some(&format!(
                "Add {label} at the marked position, or hold then drag"
            )));
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            content.append(&gtk::Image::from_icon_name(icon));
            content.append(&gtk::Label::new(Some(label)));
            button.set_child(Some(&content));
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| {
                    let (zone, before) = w.header.editor.insertion.get();
                    w.dispatch(HeaderAction::Add { zone, before, item }.action());
                }
            ));
            w.register_drag(&button, DragTarget::HeaderAdd(item));
            let hold = gtk::GestureLongPress::new();
            hold.set_touch_only(false);
            hold.set_propagation_phase(gtk::PropagationPhase::Capture);
            hold.connect_pressed(glib::clone!(
                #[weak]
                w,
                move |gesture, _, _| {
                    if let Some(drag) = w.workspace_drag.borrow_mut().as_mut()
                        && drag.wait_for_hold
                        && !drag.started
                        && gesture.widget().as_ref() == Some(&drag.source)
                    {
                        drag.held = true;
                        w.set_drag_cursor(drag, "grab");
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                    }
                }
            ));
            button.add_controller(hold);
            chip.append(&button);
            palette.append(chip);
        }
        self.root.append(&palette);
        let hint = gtk::Label::new(Some(
            "Click the bar to choose a position. Drag grips to arrange items.",
        ));
        hint.set_wrap(true);
        hint.set_xalign(0.);
        hint.add_css_class("dim-label");
        self.root.append(&hint);
        for (i, button) in self.zones.iter().enumerate() {
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| {
                    w.header.editor.select(&w, HeaderZone::ALL[i], None, None);
                }
            ));
        }
    }
    pub fn select(
        &self,
        w: &Workspace,
        zone: HeaderZone,
        before: Option<u32>,
        selected: Option<u32>,
    ) {
        self.insertion.set((zone, before));
        self.selected.set(selected);
        if let Some(model) = w.header.model.borrow().as_ref() {
            self.refresh(model, true);
        }
        w.header.refresh_selection();
        w.header.root.queue_draw();
    }
    pub fn selected_item(&self) -> Option<u32> {
        self.selected.get()
    }
    pub fn refresh(&self, model: &HeaderLayout, editing: bool) {
        if self.shown.replace(editing) != editing {
            self.insertion.set((HeaderZone::Left, None));
            self.selected.set(None);
        }
        self.root.set_visible(editing);
        for zone in &self.zones {
            zone.set_visible(editing);
        }
        self.size.set_selected(model.size as u32);
        let (mut zone, mut before) = self.insertion.get();
        if let Some(id) = before {
            if let Some((current, _)) = model.location(id) {
                zone = current;
            } else {
                before = None;
            }
        }
        self.insertion.set((zone, before));
        let label = before.and_then(|id| model.entry(id).ok()).map_or_else(
            || format!("Add to {} · at end", zone.label()),
            |entry| format!("Add to {} · before {}", zone.label(), entry.item.label()),
        );
        self.destination.set_text(&label);
        self.destination.set_tooltip_text(Some(&label));
        let selected = self.selected.get().and_then(|id| model.entry(id).ok());
        if selected.is_none() {
            self.selected.set(None);
        }
        self.remove.set_sensitive(selected.is_some());
        self.remove.set_tooltip_text(Some(&selected.map_or_else(
            || "Click an item in the bar to remove it".into(),
            |e| format!("Remove {} from the window bar", e.item.label()),
        )));
        let capacity = model.entries().count() < 128;
        self.add_tools.set_sensitive(capacity);
        for (item, chip) in &self.components {
            chip.set_visible(!item.singleton() || !model.entries().any(|e| e.item == *item));
            chip.set_sensitive(capacity);
        }
        for (i, button) in self.zones.iter().enumerate() {
            button.set_active(zone.index() == i);
        }
    }
    pub fn allocate(&self, width: f32, bar_height: f32, geometry: &HeaderGeometry) {
        for (i, button) in self.zones.iter().enumerate() {
            let zone = geometry.zones[i];
            allocate_at(
                button.upcast_ref(),
                Bounds {
                    x: zone.x,
                    y: bar_height,
                    width: zone.width,
                    height: 26.,
                },
            );
        }
        let width = (width - 12.).clamp(1., 640.);
        let height = self
            .root
            .measure(gtk::Orientation::Vertical, width as i32)
            .1 as f32;
        self.height.set(28. + height + 6.);
        allocate_at(
            self.root.upcast_ref(),
            Bounds {
                x: 6.,
                y: bar_height + 28.,
                width,
                height,
            },
        );
    }
}
