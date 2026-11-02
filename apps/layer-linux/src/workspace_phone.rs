use super::*;

pub(super) struct Phone {
    pub root: drawers::Columns,
    tools: RefCell<Vec<(ToolbarTile, toolbar_components::TileWidget, ToolbarControl)>>,
    sliders: RefCell<Vec<Rc<toolbar_components::Component>>>,
    chips: RefCell<Vec<gtk::WindowHandle>>,
    native: [gtk::WindowControls; 2],
    buttons: RefCell<Vec<(usize, gtk::WindowControls)>>,
    decoration: RefCell<String>,
    reported: Cell<Option<([f32; 2], [u32; 2])>>,
    menu: phone_menu::Menu,
    menu_key: Cell<Option<([f32; 2], Bounds, Bounds, Bounds)>>,
    loans: RefCell<Vec<(gtk::Widget, gtk::Widget, gtk::Box)>>,
    bridge: gtk::DrawingArea,
    connection: Rc<Cell<Option<DrawerConnection>>>,
    pub placement: RefCell<Option<DrawerPlacement>>,
}
impl Phone {
    pub fn new() -> Self {
        let root: drawers::Columns = glib::Object::new();
        root.imp().child_input.set(true);
        root.add_css_class("window-bar");
        root.add_css_class("phone-frame");
        root.add_css_class("header-medium");
        let menu = phone_menu::Menu::new();
        let bridge = gtk::DrawingArea::new();
        bridge.add_css_class("drawer-connection");
        bridge.set_can_target(false);
        let connection = Rc::new(Cell::new(None));
        drawers::paint_connection(&bridge, connection.clone());
        Self { root, tools: RefCell::default(), sliders: RefCell::default(), chips: RefCell::default(),
            native: [gtk::WindowControls::new(gtk::PackType::Start), gtk::WindowControls::new(gtk::PackType::End)],
            buttons: RefCell::default(), decoration: RefCell::default(), reported: Cell::new(None), menu, menu_key: Cell::new(None),
            loans: RefCell::default(), bridge, connection, placement: RefCell::default() }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        self.menu.bind(w);
        let held = Rc::new(Cell::new(false));
        let root = w.zen_capy.parent().unwrap();
        let (click, suppress) = customization::extend_button(w, &root, &w.zen_capy, None);
        let hold = gtk::GestureLongPress::new();
        hold.set_touch_only(false);
        hold.set_propagation_phase(gtk::PropagationPhase::Capture);
        hold.connect_pressed(glib::clone!(#[weak] w, #[strong] held, #[strong] suppress, move |gesture, _, _| {
            if w.gpu.borrow().as_ref().is_some_and(|g| g.session.phone_layout().is_some()) {
                held.set(true);
                suppress.set(true);
                gesture.set_state(gtk::EventSequenceState::Claimed);
                w.dispatch(UiAction::Phone { action: PhoneAction::CapyHold });
            }
        }));
        root.add_controller(hold.clone());
        hold.group_with(&click);
        gtk::Settings::for_display(&gtk::prelude::WidgetExt::display(&w.window))
            .connect_gtk_decoration_layout_notify(glib::clone!(#[weak] w, move |_| {
                glib::idle_add_local_once(glib::clone!(#[weak] w, move || w.refresh(regions::LAYOUT)));
            }));
        let release = gtk::EventControllerLegacy::new();
        release.set_propagation_phase(gtk::PropagationPhase::Capture);
        release.connect_event(glib::clone!(#[weak] w, #[strong] held, #[upgrade_or] glib::Propagation::Proceed, move |_, event| {
            if matches!(event.event_type(), gdk::EventType::ButtonRelease | gdk::EventType::TouchEnd | gdk::EventType::TouchCancel)
                && held.replace(false) {
                let position = event.position().filter(|_| event.event_type() != gdk::EventType::TouchCancel)
                    .and_then(|(x, y)| w.window.compute_point(&w.surface, &gtk::graphene::Point::new(x as f32, y as f32)))
                    .map_or([-1.; 2], |p| [p.x(), p.y()]);
                w.dispatch(UiAction::Phone { action: PhoneAction::CapyDrop { position } });
            }
            glib::Propagation::Proceed
        }));
        w.window.add_controller(release);
    }
    fn add(&self, child: &impl IsA<gtk::Widget>) {
        child.set_parent(&self.root);
        self.root.imp().children.borrow_mut().push(child.clone().upcast());
        self.root.imp().geometry.borrow_mut().push(Bounds::default());
    }
    fn detach(widget: &gtk::Widget) {
        if let Some(parent) = widget.parent().and_downcast::<gtk::Box>() { parent.remove(widget); }
        else if let Some(parent) = widget.parent().and_downcast::<gtk::Stack>() { parent.remove(widget); }
    }
    fn loan(&self, widget: &impl IsA<gtk::Widget>, destination: &gtk::Box) {
        if widget.parent().as_ref() == Some(destination.upcast_ref()) { return; }
        if let Some(parent) = widget.parent() {
            Self::detach(widget.upcast_ref());
            self.loans.borrow_mut().retain(|(w, _, _)| w != widget.upcast_ref());
            self.loans.borrow_mut().push((widget.clone().upcast(), parent, destination.clone()));
        }
        destination.append(widget);
    }
    pub fn restore_loans(&self) {
        for (widget, parent, destination) in self.loans.take() {
            if widget.parent().as_ref() != Some(destination.upcast_ref()) { continue; }
            Self::detach(&widget);
            if let Some(parent) = parent.downcast_ref::<gtk::Box>() { parent.append(&widget); }
            else if let Some(parent) = parent.downcast_ref::<gtk::Stack>() { parent.add_named(&widget, Some("full")); }
        }
    }
    pub fn main_menu(&self, w: &Rc<Workspace>, popup: &gtk::PopoverMenu) {
        let Some((extras, label)) = w.gpu.borrow().as_ref().map(|g| (g.session.main_menu_extras(),
            DocumentTabLabel::new(w.documents.selected(), &g.session.state().document_file, &w.localization()).menu_label(w.documents.len()))) else { return; };
        let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
        body.set_widget_name("capy-main-extra");
        if extras[0] {
            let drawing = gtk::Button::with_label(&label);
            drawing.add_css_class("flat");
            drawing.set_sensitive(w.documents.len() > 1);
            drawing.connect_clicked(glib::clone!(#[weak] w, move |_| w.documents.show_selector(&w)));
            body.append(&drawing);
        }
        if extras[1] { self.loan(&w.workspaces.switcher, &body); }
        if extras.into_iter().any(|v| v) {
            let menu = popup.menu_model().and_downcast::<gtk::gio::Menu>().unwrap();
            let item = gtk::gio::MenuItem::new(None, None);
            item.set_attribute_value("custom", Some(&"capy-extra".to_variant()));
            menu.prepend_item(&item);
            popup.add_child(&body, "capy-extra");
        }
    }
    pub fn refresh(&self, w: &Rc<Workspace>, state: &UiState, regions: u32) {
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        let Some(_) = state.phone_presentation(viewport) else {
            if w.zen_capy.has_css_class("phone-capy") {
                self.restore_loans();
                w.zen_capy.remove_css_class("phone-capy");
                w.zen_capy.reset_property(gtk::AccessibleProperty::Label);
                w.system_status.set_visibility(w.window.is_fullscreen());
            }
            return;
        };
        let structural = regions & !(regions::BRUSH | regions::CAMERA) != 0
            || self.root.imp().children.borrow().is_empty();
        w.zen_capy.add_css_class("phone-capy");
        if self.root.imp().children.borrow().is_empty() {
            for i in 0..4 {
                let chip = gtk::WindowHandle::new();
                chip.set_widget_name(["phone-tools-chip", "phone-sliders-chip", "phone-bar-tools", "phone-bar-capy"][i]);
                if i < 2 { chip.add_css_class("header-bar"); }
                self.add(&chip);
                self.chips.borrow_mut().push(chip);
            }
            for (side, group) in self.native.iter().enumerate() {
                group.set_widget_name(&format!("native-group-{side}"));
                group.add_css_class("phone-controls");
                group.connect_empty_notify(glib::clone!(#[weak] w, move |_| {
                    glib::idle_add_local_once(glib::clone!(#[weak] w, move || w.refresh(regions::LAYOUT)));
                }));
                self.add(group);
            }
            self.add(&self.bridge);
            self.add(&self.menu.root);
        }
        if structural {
            let decoration = gtk::Settings::for_display(&gtk::prelude::WidgetExt::display(&w.window)).property::<String>("gtk-decoration-layout");
            if *self.decoration.borrow() != decoration {
                for (_, button) in self.buttons.take() {
                    let index = self.root.imp().children.borrow().iter().position(|child| child == &button);
                    if let Some(i) = index {
                        self.root.imp().children.borrow_mut().remove(i);
                        self.root.imp().geometry.borrow_mut().remove(i);
                    }
                    button.unparent();
                }
                for (side, names) in decoration.split(':').take(2).enumerate() {
                    for (index, name) in names.split(',').filter(|s| !s.is_empty()).enumerate() {
                        let button = gtk::WindowControls::new(if side == 0 { gtk::PackType::Start } else { gtk::PackType::End });
                        button.set_decoration_layout(Some(&if side == 0 { format!("{name}:") } else { format!(":{name}") }));
                        button.set_widget_name(&format!("native-control-{side}-{index}"));
                        button.add_css_class("phone-controls");
                        self.add(&button);
                        self.buttons.borrow_mut().push((side, button));
                    }
                }
                *self.decoration.borrow_mut() = decoration;
            }
            for group in self.native.iter().chain(self.buttons.borrow().iter().map(|(_, group)| group)) {
                let mut child = group.first_child();
                while let Some(button) = child {
                    child = button.next_sibling();
                    button.set_hexpand(true);
                    button.set_vexpand(true);
                }
            }
            let native = self.native.each_ref().map(|group| if group.is_empty() { 0. } else { group.measure(gtk::Orientation::Horizontal, -1).1 as f32 });
            let native_counts = [0, 1].map(|side| self.buttons.borrow().iter().filter(|(s, b)| *s == side && !b.is_empty()).count() as u32);
            if (native, native_counts) != (state.phone.native, state.phone.native_counts)
                && self.reported.replace(Some((native, native_counts))) != Some((native, native_counts)) {
                glib::idle_add_local_once(glib::clone!(#[weak] w, move || w.dispatch(UiAction::Phone { action: PhoneAction::Metrics { native, native_counts, insets: [0.; 4] } })));
            }
        }
        let tiles = state.workspace.layout.phone.as_deref().unwrap_or(&[]);
        let changed = self.tools.borrow().iter().map(|(tile, _, _)| tile).ne(tiles.iter());
        if changed {
            for (_, item, _) in self.tools.take() {
                let child = item.root();
                let index = self.root.imp().children.borrow().iter().position(|w| w == &child).unwrap();
                self.root.imp().children.borrow_mut().remove(index);
                self.root.imp().geometry.borrow_mut().remove(index);
                child.unparent();
            }
            for tile in tiles {
                let choice = tool_choice_localized(tile.control, &w.localization());
                let item = toolbar_components::TileWidget::new(w, None, tile, TileStyle::Medium, &choice);
                item.button().set_widget_name(&format!("phone-tool-{}", tile.id));
                item.button().add_css_class("header-tool");
                item.button().add_css_class("in-bar");
                self.add(&item.root());
                self.tools.borrow_mut().push((tile.clone(), item, tile.control));
            }
        }
        if changed || structural {
            for ((_, item, control), view) in self.tools.borrow_mut().iter_mut().zip(state.phone_tools()) {
                *control = view.resolved_control;
                item.refresh(w, &view, view.component.as_ref());
            }
        } else {
            for (tile, item, control) in self.tools.borrow().iter() {
                item.refresh_selection(tool_state(state, *control).1, tile.control.has_variants());
            }
        }
        if self.sliders.borrow().is_empty() {
            for control in PhoneLayout::SLIDERS {
                let choice = tool_choice_localized(control, &w.localization());
                let slider = toolbar_components::Component::slider(w, control, TileStyle::Medium, &choice);
                self.add(&slider.root);
                self.sliders.borrow_mut().push(slider);
            }
        }
        for slider in self.sliders.borrow().iter() {
            if let Some(view) = state.toolbar_component(slider.control) { slider.refresh(w, &view); }
        }
        if state.phone.menu {
            if structural { self.menu.refresh(w, state); }
            self.loan(&w.workspaces.switcher, &self.menu.bodies[0]);
            self.menu.bodies[0].reorder_child_after(&w.workspaces.switcher, Some(&self.menu.drawing_row));
            self.loan(&w.system_status.clock, &self.menu.drawing_row);
            self.loan(&w.system_status.battery, &self.menu.drawing_row);
            w.system_status.set_components(true, true);
            w.system_status.set_visibility(true);
        } else {
            self.menu.clear();
            self.menu_key.set(None);
        }
        if changed || structural { w.surface.queue_allocate(); }
    }
    pub fn workspaces(&self, w: &Rc<Workspace>) { self.menu.workspaces(w); }
    pub fn button(&self, w: &Workspace, tile: Option<u32>) -> Option<gtk::Button> {
        tile.map_or_else(|| Some(w.zen_capy.clone()), |id| self.tools.borrow().iter().find(|(tile, _, _)| tile.id == id).map(|(_, item, _)| item.button()))
    }
    pub fn mark_origin(&self, w: &Workspace, anchor: Option<DrawerAnchor>, direction: Option<Edge>) {
        customization::drawer_origin(&w.zen_capy, direction.filter(|_| anchor == Some(DrawerAnchor::Phone { tile: None })));
        for (tile, item, _) in self.tools.borrow().iter() {
            customization::drawer_origin(&item.button(), direction.filter(|_| anchor == Some(DrawerAnchor::Phone { tile: Some(tile.id) })));
        }
    }
    pub fn button_in_hit(button: &gtk::Button, visual: Bounds, hit: Bounds) {
        button.set_margin_start((visual.x.round() - hit.x.round()) as i32);
        button.set_margin_end(((hit.x + hit.width).round() - (visual.x + visual.width).round()) as i32);
        button.set_margin_top((visual.y.round() - hit.y.round()) as i32);
        button.set_margin_bottom(((hit.y + hit.height).round() - (visual.y + visual.height).round()) as i32);
    }
    pub fn allocate(&self, w: &Workspace, p: &PhoneLayout, state: PhoneState) {
        let vertical = p.edge != Edge::Top;
        for (index, (tile, item, _)) in self.tools.borrow().iter().enumerate() {
            if tile.control == ToolbarControl::Divider {
                item.root().set_can_target(false);
                if let Some(line) = item.button().child().and_downcast::<gtk::Separator>() {
                    crate::tiles::orient_divider(&line, if vertical { gtk::Orientation::Horizontal } else { gtk::Orientation::Vertical });
                }
            } else { Self::button_in_hit(&item.button(), p.tools[index], p.tool_hits[index]); }
            let size = p.icon.round().max(1.) as i32;
            if let Some(content) = customization::tool_button_content(&item.button()) {
                if let Some(image) = content.downcast_ref::<gtk::Image>() {
                    if image.pixel_size() != size { image.set_pixel_size(size); }
                } else if let Some(pair) = content.downcast_ref::<crate::display_color::ColorPair>() { pair.set_size(size); }
            }
        }
        let tile = p.tile.round().max(1.) as i32;
        w.zen_capy.set_size_request(tile, tile);
        Self::button_in_hit(&w.zen_capy, p.capy, p.capy_hit);
        if let Some(image) = w.zen_capy.child().and_downcast::<gtk::Image>() {
            let size = p.capy_icon.round().max(1.) as i32;
            if image.pixel_size() != size { image.set_pixel_size(size); }
        }
        let axis = if vertical { Axis::Vertical } else { Axis::Horizontal };
        for slider in self.sliders.borrow().iter() { slider.root.set_presentation(axis, TileStyle::Medium); }
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        if state.menu {
            self.menu.size_controls(p);
            let key = (viewport, p.capy, p.bars[0], p.work_area);
            if self.menu_key.get() != Some(key) {
                self.menu_key.set(Some(key));
                let widths = self.menu.bodies.each_ref().map(|body| body.measure(gtk::Orientation::Horizontal, -1));
                let columns = p.menu_column_widths(widths.map(|width| width.0 as f32), widths.map(|width| width.1 as f32));
                let heights = std::array::from_fn(|index| self.menu.bodies[index].measure(gtk::Orientation::Vertical, columns[index] as i32).1);
                let placement = p.menu(viewport, columns, heights.map(|height| height as f32)).map(|p| p.snapped(1.));
                *self.placement.borrow_mut() = placement;
            }
        } else {
            self.menu_key.set(None);
            self.placement.borrow_mut().take();
        }
        let menu = self.placement.borrow();
        if let Some(placement) = menu.as_ref() {
            let view = self.menu.geometry(placement);
            let geometry = [view.columns[0], view.columns[1], view.separator];
            self.menu.separator.set_orientation(if view.separator.width > view.separator.height {
                gtk::Orientation::Horizontal
            } else { gtk::Orientation::Vertical });
            for (child, bounds) in self.menu.root.imp().children.borrow().iter().zip(&geometry) {
                child.set_child_visible(bounds.width > 0. && bounds.height > 0.);
            }
            let mut current = self.menu.root.imp().geometry.borrow_mut();
            if current.as_slice() != geometry.as_slice() {
                current.copy_from_slice(&geometry);
                self.menu.root.queue_allocate();
            }
        }
        self.connection.set(menu.as_ref().and_then(|p| p.connection()));
        self.bridge.queue_draw();
        customization::square_corners(&self.menu.root, self.connection.get().map_or([false; 4], |c| c.square_corners));
        customization::drawer_origin(&w.zen_capy, menu.as_ref().map(|p| p.direction));
        let mut changed = false;
        let mut geometry = self.root.imp().geometry.borrow_mut();
        for (child, bounds) in self.root.imp().children.borrow().iter().zip(geometry.iter_mut()) {
            let b = if let Some(index) = self.chips.borrow().iter().position(|c| child == c) {
                    Some(if index < 2 { p.chips[index] } else { p.bars[index - 2] })
                } else if child == &self.menu.root { menu.as_ref().map(|p| p.bounds) }
                else if child == &self.bridge { self.connection.get().map(|p| p.bounds) }
                else if let Some(side) = self.native.iter().position(|n| child == n) { (!vertical).then_some(p.controls[side]) }
                else if let Some((side, _)) = self.buttons.borrow().iter().find(|(_, b)| child == b) {
                    if vertical {
                        self.buttons.borrow().iter().filter(|(s, b)| s == side && !b.is_empty()).position(|(_, b)| child == b)
                            .and_then(|index| p.native_control(*side, index as u32))
                    } else { None }
                }
                else if let Some(index) = self.tools.borrow().iter().position(|(_, item, _)| child == &item.root()) {
                    Some(if self.tools.borrow()[index].0.control == ToolbarControl::Divider { p.tools[index] } else { p.tool_hits[index] })
                }
                else if let Some(index) = self.sliders.borrow().iter().position(|s| child == &s.root) { Some([p.size, p.strength][index]) }
                else { None };
            let b = b.filter(|b| b.width > 0. && b.height > 0. && !state.hidden);
            changed |= child.is_child_visible() != b.is_some();
            child.set_child_visible(b.is_some());
            if let Some(b) = b && *bounds != b {
                *bounds = b;
                changed = true;
            }
        }
        if changed { self.root.queue_allocate(); }
    }
}
