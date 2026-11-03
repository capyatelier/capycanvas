use super::*;

impl Workspace {
    pub(crate) fn show_drawings(self: &Rc<Self>) {
        if self.gpu.borrow().as_ref().is_some_and(|gpu| gpu.session.phone_layout().is_some()) { self.phone.drawings(self); }
        else { self.documents.show_selector(self); }
    }
}

#[derive(Clone, Copy)]
enum Child { Chip(usize), Title, NativeGroup(usize), NativeButton(usize), Bridge, Menu, Tool(usize, bool), Slider(usize), Settings }

pub(super) struct Phone {
    pub root: drawers::Columns,
    contact: Rc<RefCell<Option<(PhoneBarContact, gdk::Event)>>>,
    tools: RefCell<Vec<(ToolbarTile, toolbar_components::TileWidget, ToolbarControl)>>,
    sliders: RefCell<Vec<Rc<toolbar_components::Component>>>,
    settings: RefCell<Option<toolbar_components::TileWidget>>,
    title: gtk::WindowHandle,
    children: RefCell<Vec<Child>>,
    native: [gtk::WindowControls; 2],
    buttons: RefCell<Vec<(usize, gtk::WindowControls)>>,
    decoration: RefCell<String>,
    reported: Cell<Option<([f32; 2], [u32; 2], bool)>>,
    menu: phone_menu::Menu,
    menu_key: Cell<Option<([f32; 2], Bounds, Bounds, Bounds, f32)>>,
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
        let title = gtk::WindowHandle::new();
        title.set_widget_name("phone-title-band");
        let bridge = gtk::DrawingArea::new();
        bridge.add_css_class("drawer-connection");
        bridge.set_can_target(false);
        let connection = Rc::new(Cell::new(None));
        drawers::paint_connection(&bridge, connection.clone());
        Self { root, contact: Rc::default(), tools: RefCell::default(), sliders: RefCell::default(), settings: RefCell::default(), title,
            children: RefCell::default(),
            native: [gtk::WindowControls::new(gtk::PackType::Start), gtk::WindowControls::new(gtk::PackType::End)],
            buttons: RefCell::default(), decoration: RefCell::default(), reported: Cell::new(None), menu, menu_key: Cell::new(None),
            loans: RefCell::default(), bridge, connection, placement: RefCell::default() }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        self.menu.bind(w);
        self.bind_window_move(w);
        let held = Rc::new(Cell::new(false));
        let root = w.zen_capy.parent().unwrap();
        root.add_css_class("customizable-target");
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
    fn bind_window_move(&self, w: &Rc<Workspace>) {
        let contact = self.contact.clone();
        w.install_menu(&self.root, ContextTarget::PhoneToolbar);
        let drag = gtk::GestureDrag::new();
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        drag.connect_drag_begin(glib::clone!(#[weak] w, #[strong] contact, move |gesture, x, y| {
            if w.visible_dialog().is_some() { gesture.set_state(gtk::EventSequenceState::Denied); return; }
            let point = w.window.compute_point(&w.surface, &gtk::graphene::Point::new(x as f32, y as f32));
            let start = point.and_then(|p| {
                let picked = w.surface.pick(p.x() as f64, p.y() as f64, gtk::PickFlags::DEFAULT)?;
                let capy = w.zen_capy.parent()?;
                if ![w.phone.root.upcast_ref::<gtk::Widget>(), &capy].iter().any(|root| picked == **root || picked.is_ancestor(*root)) { return None; }
                let gpu = w.gpu.borrow();
                let gpu = gpu.as_ref()?;
                let layout = gpu.session.phone_layout()?;
                let title = layout.title.width > 0. && layout.title.contains(p.x(), p.y());
                if title && picked != w.phone.title && !picked.is_ancestor(&w.phone.title) { return None; }
                let native = std::iter::successors(Some(picked), |widget| widget.parent()).any(|widget|
                    widget.is::<gtk::Button>() && widget.ancestor(gtk::WindowControls::static_type()).is_some());
                PhoneBarContact::begin(&layout, [p.x(), p.y()], native).map(|mut contact| { contact.set_tools(gpu.session.state().workspace.layout.phone.as_deref().unwrap_or(&[])); contact })
            });
            contact.replace(start.zip(gesture.current_event()));
            if contact.borrow().is_none() { gesture.set_state(gtk::EventSequenceState::Denied); }
        }));
        drag.connect_drag_update(glib::clone!(#[weak] w, #[strong] contact, move |gesture, dx, dy| {
            let past_slop = w.window.drag_check_threshold(0, 0, dx as i32, dy as i32);
            let outcome = {
                let mut active = contact.borrow_mut();
                let Some((active, press)) = active.as_mut() else { return; };
                let gpu = w.gpu.borrow();
                let Some(gpu) = gpu.as_ref() else { return; };
                let Some(layout) = gpu.session.phone_layout() else { return; };
                let Some((x,y)) = press.position() else { return; };
                active.motion(&layout, gpu.session.state().workspace.layout.phone.as_deref().unwrap_or(&[]), [(x+dx) as f32,(y+dy) as f32], past_slop)
            };
            if matches!(outcome, PhoneBarOutcome::Held | PhoneBarOutcome::Dragging) { gesture.set_state(gtk::EventSequenceState::Claimed); w.surface.queue_allocate(); return; }
            if outcome == PhoneBarOutcome::Cancel { contact.take(); gesture.set_state(gtk::EventSequenceState::Claimed); return; }
            if outcome != PhoneBarOutcome::WindowMove { return; }
            let (_, press) = contact.take().unwrap();
            if let (Some(surface), Some(device), Some((x, y))) =
                (w.window.surface().and_downcast::<gdk::Toplevel>(), press.device(), press.position()) {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                w.chrome_contacts.borrow_mut().retain(|_, event| event.as_ptr() != press.as_ptr());
                w.update_zen();
                surface.begin_move(&device, 1, x, y, press.time());
            }
        }));
        drag.connect_drag_end(glib::clone!(#[weak] w, #[strong] contact, move |gesture, dx, dy| {
            let Some((mut active, press)) = contact.take() else { return; };
            let outcome = w.gpu.borrow().as_ref().and_then(|g| g.session.phone_layout().map(|layout| {
                let (x,y) = press.position().unwrap_or_default();
                active.finish(&layout, g.session.state().workspace.layout.phone.as_deref().unwrap_or(&[]), [(x+dx) as f32,(y+dy) as f32])
            })).unwrap_or(PhoneBarOutcome::Cancel);
            match outcome {
                PhoneBarOutcome::Move { tile, before } => w.dispatch(UiAction::Phone { action: PhoneAction::MoveTool { tile, before } }),
                PhoneBarOutcome::Menu(tile) => {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    let (x,y) = press.position().unwrap_or_default();
                    w.show_context(w.surface.upcast_ref(), tile.map_or(ContextTarget::PhoneToolbar, |tile| ContextTarget::PhoneTile { tile }), x, y);
                }
                PhoneBarOutcome::Cancel => { gesture.set_state(gtk::EventSequenceState::Claimed); }
                _ => ()
            }
            w.surface.queue_allocate();
        }));
        drag.connect_cancel(glib::clone!(#[weak] w, #[strong] contact, move |_, _| { contact.take(); w.surface.queue_allocate(); }));
        w.window.add_controller(drag.clone());
        let hold = gtk::GestureLongPress::new();
        hold.set_touch_only(false);
        hold.set_propagation_phase(gtk::PropagationPhase::Capture);
        hold.connect_pressed(glib::clone!(#[weak] w, move |gesture, _, _| {
            if let Some((active, _)) = contact.borrow_mut().as_mut() && active.arm() == PhoneBarOutcome::Held { gesture.set_state(gtk::EventSequenceState::Claimed); }
            w.surface.queue_allocate();
        }));
        w.window.add_controller(hold.clone());
        hold.group_with(&drag);
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        key.connect_key_pressed(glib::clone!(#[weak] w, #[upgrade_or] glib::Propagation::Proceed, move |_, key, _, _| {
            if key == gdk::Key::Escape && let Some((mut contact, _)) = w.phone.contact.take() {
                contact.cancel(); w.surface.queue_allocate(); return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }));
        w.window.add_controller(key);
    }

    fn add(&self, child: &impl IsA<gtk::Widget>, role: Child) {
        child.set_parent(&self.root);
        self.root.imp().children.borrow_mut().push(child.clone().upcast());
        self.root.imp().geometry.borrow_mut().push(Bounds::default());
        self.children.borrow_mut().push(role);
    }
    fn remove(&self, child: &gtk::Widget) {
        let index = self.root.imp().children.borrow().iter().position(|widget| widget == child).unwrap();
        self.root.imp().children.borrow_mut().remove(index);
        self.root.imp().geometry.borrow_mut().remove(index);
        self.children.borrow_mut().remove(index);
        child.unparent();
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
    pub fn refresh(&self, w: &Rc<Workspace>, state: &UiState, structural: bool) {
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        let Some(_) = state.phone_presentation(viewport) else {
            self.contact.take();
            self.root.imp().lifted.borrow_mut().take();
            if w.zen_capy.has_css_class("phone-capy") {
                self.restore_loans();
                w.zen_capy.remove_css_class("phone-capy");
                w.zen_capy.reset_property(gtk::AccessibleProperty::Label);
                w.system_status.set_visibility(w.window.is_fullscreen());
            }
            return;
        };
        let structural = structural || self.root.imp().children.borrow().is_empty();
        w.zen_capy.add_css_class("phone-capy");
        if self.root.imp().children.borrow().is_empty() {
            for i in 0..4 {
                let chip = gtk::WindowHandle::new();
                chip.set_widget_name(["phone-tools-chip", "phone-sliders-chip", "phone-bar-tools", "phone-bar-capy"][i]);
                if i < 2 { chip.add_css_class("header-bar"); }
                self.add(&chip, Child::Chip(i));
            }
            self.add(&self.title, Child::Title);
            for (side, group) in self.native.iter().enumerate() {
                group.set_widget_name(&format!("native-group-{side}"));
                group.add_css_class("phone-controls");
                group.connect_empty_notify(glib::clone!(#[weak] w, move |_| {
                    glib::idle_add_local_once(glib::clone!(#[weak] w, move || w.refresh(regions::LAYOUT)));
                }));
                self.add(group, Child::NativeGroup(side));
            }
            self.add(&self.bridge, Child::Bridge);
            self.add(&self.menu.root, Child::Menu);
        }
        if structural {
            let decoration = gtk::Settings::for_display(&gtk::prelude::WidgetExt::display(&w.window)).property::<String>("gtk-decoration-layout");
            if *self.decoration.borrow() != decoration {
                for (_, button) in self.buttons.take() {
                    self.remove(button.upcast_ref());
                }
                for (side, names) in decoration.split(':').take(2).enumerate() {
                    for (index, name) in names.split(',').filter(|s| !s.is_empty()).enumerate() {
                        let button = gtk::WindowControls::new(if side == 0 { gtk::PackType::Start } else { gtk::PackType::End });
                        button.set_decoration_layout(Some(&if side == 0 { format!("{name}:") } else { format!(":{name}") }));
                        button.set_widget_name(&format!("native-control-{side}-{index}"));
                        button.add_css_class("phone-controls");
                        self.add(&button, Child::NativeButton(side));
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
            let movable = !w.window.is_fullscreen();
            if (native, native_counts, movable) != (state.phone.native, state.phone.native_counts, state.phone.movable)
                && self.reported.replace(Some((native, native_counts, movable))) != Some((native, native_counts, movable)) {
                glib::idle_add_local_once(glib::clone!(#[weak] w, move || w.dispatch(UiAction::Phone { action: PhoneAction::Metrics { native, native_counts, insets: [0.; 4], movable } })));
            }
        }
        let tiles = state.workspace.layout.phone.as_deref().unwrap_or(&[]);
        let changed = self.tools.borrow().iter().map(|(tile, _, _)| tile).ne(tiles.iter());
        if changed {
            for (_, item, _) in self.tools.take() {
                self.remove(&item.root());
            }
            for (index, tile) in tiles.iter().enumerate() {
                let choice = tool_choice_localized(tile.control, &w.localization());
                let item = toolbar_components::TileWidget::new(w, None, tile, TileStyle::Medium, &choice);
                w.install_menu(&item.root(), ContextTarget::PhoneTile { tile: tile.id });
                item.button().set_widget_name(&format!("phone-tool-{}", tile.id));
                item.button().add_css_class("header-tool");
                item.button().add_css_class("in-bar");
                self.add(&item.root(), Child::Tool(index, tile.control == ToolbarControl::Divider));
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
            for (index, control) in PhoneLayout::SLIDERS.into_iter().enumerate() {
                let choice = tool_choice_localized(control, &w.localization());
                let slider = toolbar_components::Component::slider(w, control, TileStyle::Medium, &choice);
                self.add(&slider.root, Child::Slider(index));
                self.sliders.borrow_mut().push(slider);
            }
        }
        for slider in self.sliders.borrow().iter() {
            if let Some(view) = state.toolbar_component(slider.control) { slider.refresh(w, &view); }
        }
        let settings = state.phone_settings();
        if self.settings.borrow().is_none() {
            let item = toolbar_components::TileWidget::control_button(w, DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::ToolSettings }, PhoneLayout::SETTINGS, TileStyle::Medium, &settings.choice);
            item.root().add_css_class("customizable-target");
            item.button().set_widget_name("phone-tool-settings");
            item.button().add_css_class("header-tool");
            self.add(&item.root(), Child::Settings);
            self.settings.replace(Some(item));
        }
        self.settings.borrow().as_ref().unwrap().refresh(w, &settings, None);
        if state.phone.menu {
            if structural { self.menu.refresh(w, state); self.menu_key.set(None); }
        } else {
            self.menu.clear();
            self.menu_key.set(None);
        }
        if changed || structural { w.surface.queue_allocate(); }
    }
    pub fn workspaces(&self, w: &Rc<Workspace>) { self.menu.workspaces(w); }
    pub fn drawings(&self, w: &Rc<Workspace>) {
        if !w.gpu.borrow().as_ref().is_some_and(|g| g.session.state().phone.menu) { w.dispatch(UiAction::Phone { action: PhoneAction::Capy }); }
        self.menu.drawings(w);
    }
    pub fn button(&self, w: &Workspace, anchor: PhoneDrawerAnchor) -> Option<gtk::Button> {
        match anchor {
            PhoneDrawerAnchor::Capy => Some(w.zen_capy.clone()),
            PhoneDrawerAnchor::ToolSettings => self.settings.borrow().as_ref().map(|item| item.button()),
            PhoneDrawerAnchor::Tool(id) => self.tools.borrow().iter().find(|(tile, _, _)| tile.id == id).map(|(_, item, _)| item.button()),
        }
    }
    pub fn mark_origin(&self, w: &Workspace, anchor: Option<DrawerAnchor>, direction: Option<Edge>) {
        customization::drawer_origin(&w.zen_capy, direction.filter(|_| anchor == Some(DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::Capy })));
        for (tile, item, _) in self.tools.borrow().iter() {
            customization::drawer_origin(&item.button(), direction.filter(|_| anchor == Some(DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::Tool(tile.id) })));
        }
        if let Some(item) = self.settings.borrow().as_ref() {
            customization::drawer_origin(&item.button(), direction.filter(|_| anchor == Some(DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::ToolSettings })));
        }
    }
    pub fn button_in_hit(button: &gtk::Button, visual: Bounds, hit: Bounds) {
        button.set_margin_start((visual.x.round() - hit.x.round()) as i32);
        button.set_margin_end(((hit.x + hit.width).round() - (visual.x + visual.width).round()) as i32);
        button.set_margin_top((visual.y.round() - hit.y.round()) as i32);
        button.set_margin_bottom(((hit.y + hit.height).round() - (visual.y + visual.height).round()) as i32);
    }
    pub fn allocate(&self, w: &Workspace, p: &PhoneLayout, state: PhoneState) {
        let tool_bounds = {
            let mut active = self.contact.borrow_mut();
            if active.as_ref().is_some_and(|(contact, _)| !contact.valid(p, self.tools.borrow().iter().map(|(tile, _, _)| tile))) { active.take(); }
            let lifted = active.as_ref().and_then(|(contact, _)| contact.lifted());
            let color = w.palette.get().map_or([48,48,48], |palette| palette.panel.0);
            self.root.imp().lifted.replace(lifted.map(|i| (self.tools.borrow()[i].1.root(),
                gdk::RGBA::new(color[0] as f32 / 255., color[1] as f32 / 255., color[2] as f32 / 255., 1.))));
            active.as_ref().map_or(p.tools, |(contact, _)| contact.geometry(p))
        };
        let vertical = p.edge != Edge::Top;
        for (index, (tile, item, _)) in self.tools.borrow().iter().enumerate() {
            if tile.control == ToolbarControl::Divider {
                item.root().set_can_target(true);
                if let Some(line) = item.button().child().and_downcast::<gtk::Separator>() {
                    crate::tiles::orient_divider(&line, if vertical { gtk::Orientation::Horizontal } else { gtk::Orientation::Vertical });
                }
            } else {
                Self::button_in_hit(&item.button(), tool_bounds[index], if self.root.imp().lifted.borrow().is_some() { tool_bounds[index] } else { p.tool_hits[index] });
            }
            let size = p.icon.round().max(1.) as i32;
            if let Some(content) = customization::tool_button_content(&item.button()) {
                if let Some(image) = content.downcast_ref::<gtk::Image>() {
                    if image.pixel_size() != size { image.set_pixel_size(size); }
                } else if let Some(pair) = content.downcast_ref::<crate::display_color::ColorPair>() { pair.set_size(size); }
            }
        }
        let tile = p.tile.round().max(1.) as i32;
        if let Some(item) = self.settings.borrow().as_ref() {
            Self::button_in_hit(&item.button(), p.settings, p.settings_hit);
            if let Some(image) = customization::tool_button_content(&item.button()).and_downcast::<gtk::Image>() { image.set_pixel_size(p.icon.round().max(1.) as i32); }
        }
        w.zen_capy.set_size_request(tile, tile);
        Self::button_in_hit(&w.zen_capy, p.capy, p.capy_target);
        if let Some(image) = w.zen_capy.child().and_downcast::<gtk::Image>() {
            let size = p.capy_icon.round().max(1.) as i32;
            if image.pixel_size() != size { image.set_pixel_size(size); }
        }
        let axis = if vertical { Axis::Vertical } else { Axis::Horizontal };
        for slider in self.sliders.borrow().iter() { slider.root.set_presentation(axis, TileStyle::Medium); }
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        let row_height = if state.menu { self.menu.row_height() } else { 0. };
        if state.menu {
            let key = (viewport, p.capy, p.bars[0], p.work_area, row_height);
            if self.menu_key.get() != Some(key) {
                self.menu_key.set(Some(key));
                let (width, height) = self.menu.measure(p, row_height);
                let placement = p.menu(viewport, width, height, row_height).map(|p| p.snapped(1.));
                *self.placement.borrow_mut() = placement;
            }
        } else {
            self.menu_key.set(None);
            self.placement.borrow_mut().take();
        }
        let menu = self.placement.borrow();
        if let Some(placement) = menu.as_ref() {
            self.menu.allocate(p, placement, row_height);
        }
        self.connection.set(menu.as_ref().and_then(|p| p.connection()));
        self.bridge.queue_draw();
        customization::square_corners(&self.menu.root, self.connection.get().map_or([false; 4], |c| c.square_corners));
        customization::drawer_origin(&w.zen_capy, menu.as_ref().map(|p| p.direction));
        let mut changed = false;
        let mut geometry = self.root.imp().geometry.borrow_mut();
        let mut native_indices = [0; 2];
        for ((child, bounds), role) in self.root.imp().children.borrow().iter().zip(geometry.iter_mut()).zip(self.children.borrow().iter()) {
            let b = match *role {
                Child::Chip(index) => Some(if index < 2 { p.chips[index] } else { p.bars[index - 2] }),
                Child::Title => Some(p.title),
                Child::Menu => menu.as_ref().map(|p| p.bounds),
                Child::Bridge => self.connection.get().map(|p| p.bounds),
                Child::Settings => Some(p.settings_hit),
                Child::NativeGroup(side) => (!vertical).then_some(p.controls[side]),
                Child::NativeButton(side) => {
                    if !vertical || child.downcast_ref::<gtk::WindowControls>().unwrap().is_empty() { None }
                    else {
                        let index = native_indices[side];
                        native_indices[side] += 1;
                        p.native_control(side, index)
                    }
                }
                Child::Tool(index, divider) => Some(if self.root.imp().lifted.borrow().is_some() || divider { tool_bounds[index] } else { p.tool_hits[index] }),
                Child::Slider(index) => Some([p.size, p.strength][index]),
            };
            let b = b.filter(|b| b.width > 0. && b.height > 0.);
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
