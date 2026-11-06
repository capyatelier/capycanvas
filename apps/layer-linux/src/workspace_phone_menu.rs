use super::*;

mod native {
    use super::*;
    #[derive(Default)]
    pub struct Row;
    #[glib::object_subclass]
    impl ObjectSubclass for Row {
        const NAME: &'static str = "CapyMenuRow";
        type Type = super::MenuRow;
        type ParentType = gtk::Button;
        fn class_init(class: &mut Self::Class) { class.set_css_name("modelbutton"); class.set_accessible_role(gtk::AccessibleRole::MenuItem); }
    }
    impl ObjectImpl for Row {}
    impl WidgetImpl for Row {}
    impl ButtonImpl for Row {}
    #[derive(Default)]
    pub struct CheckRow;
    #[glib::object_subclass]
    impl ObjectSubclass for CheckRow {
        const NAME: &'static str = "CapyCheckedMenuRow";
        type Type = super::MenuCheckRow;
        type ParentType = gtk::Button;
        fn class_init(class: &mut Self::Class) { class.set_css_name("modelbutton"); class.set_accessible_role(gtk::AccessibleRole::MenuItemCheckbox); }
    }
    impl ObjectImpl for CheckRow {}
    impl WidgetImpl for CheckRow {}
    impl ButtonImpl for CheckRow {}
    #[derive(Default)]
    pub struct Menu;
    #[glib::object_subclass]
    impl ObjectSubclass for Menu {
        const NAME: &'static str = "CapyInlineMenu";
        type Type = super::InlineMenu;
        type ParentType = gtk::Box;
        fn class_init(class: &mut Self::Class) { class.set_css_name("popover"); }
    }
    impl ObjectImpl for Menu {}
    impl WidgetImpl for Menu {}
    impl BoxImpl for Menu {}
    #[derive(Default)]
    pub struct Contents { pub root: glib::WeakRef<gtk::Widget>, pub(super) swipe: super::swipe::State }
    #[glib::object_subclass]
    impl ObjectSubclass for Contents {
        const NAME: &'static str = "CapyInlineMenuContents";
        type Type = super::MenuContents;
        type ParentType = gtk::Widget;
        fn class_init(class: &mut Self::Class) { class.set_css_name("contents"); }
    }
    impl ObjectImpl for Contents {
        fn dispose(&self) { if let Some(child) = self.obj().first_child() { child.unparent(); } }
    }
    impl WidgetImpl for Contents {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            self.root.upgrade().or_else(|| self.obj().first_child()).map_or(gtk::SizeRequestMode::ConstantSize, |child| child.request_mode())
        }
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            (if orientation == gtk::Orientation::Horizontal { self.root.upgrade() } else { self.obj().first_child() })
                .map_or((0, 0, -1, -1), |child| child.measure(orientation, for_size))
        }
        fn compute_expand(&self, horizontal: &mut bool, vertical: &mut bool) {
            if let Some(child) = self.obj().first_child() { *horizontal |= child.compute_expand(gtk::Orientation::Horizontal); *vertical |= child.compute_expand(gtk::Orientation::Vertical); }
        }
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() { child.allocate(width, height, baseline, None); }
            super::swipe::allocate(self, width, height, baseline);
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            super::swipe::snapshot(self, snapshot);
        }
    }
}
glib::wrapper! {
    pub struct InlineMenu(ObjectSubclass<native::Menu>)
        @extends gtk::Box, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}
glib::wrapper! {
    pub struct MenuContents(ObjectSubclass<native::Contents>)
        @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl MenuContents { fn append(&self, child: &impl IsA<gtk::Widget>) { child.set_parent(self); } }
#[path = "phone_menu_swipe.rs"]
mod swipe;
glib::wrapper! {
    pub struct MenuRow(ObjectSubclass<native::Row>)
        @extends gtk::Button, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}
glib::wrapper! {
    pub struct MenuCheckRow(ObjectSubclass<native::CheckRow>)
        @extends gtk::Button, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}
struct RowView {
    page: PhoneMenuRoute,
    position: [usize; 2],
    button: gtk::Button,
    label: gtk::Label,
    check: Option<gtk::Image>,
    icon: gtk::Image,
    detail: gtk::Label,
    close: Option<gtk::Button>,
}
struct Pane { widget: InlineMenu, contents: MenuContents, stack: gtk::Stack, row: gtk::Button, rows: Vec<RowView> }
pub(super) struct Menu {
    pub root: drawers::Columns,
    body: gtk::Box,
    separator: gtk::Separator,
    buttons: [gtk::Button; 5],
    pane: RefCell<Option<Pane>>,
    model: Rc<RefCell<Option<PhoneMenuView>>>,
    contact: Rc<RefCell<PhoneMenuContact>>,
}
fn compatible(a: &PhoneMenuView, b: &PhoneMenuView) -> bool {
    a.pages.len() == b.pages.len() && a.pages.iter().zip(&b.pages).all(|(a, b)| a.id == b.id && a.parent == b.parent && a.title == b.title
        && a.checks == b.checks && a.sections.len() == b.sections.len() && a.sections.iter().zip(&b.sections).all(|(a, b)|
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.is_heading == b.is_heading && a.selected.is_some() == b.selected.is_some() && a.target == b.target && a.close.is_some() == b.close.is_some())))
}
fn row_model<'a>(model: &'a PhoneMenuView, page: &PhoneMenuRoute, position: [usize; 2]) -> Option<&'a PhoneMenuRow> {
    model.pages.iter().find(|p| &p.id == page)?.sections.get(position[0])?.get(position[1])
}
fn route(w: &Workspace) -> PhoneMenuRoute { w.gpu.borrow().as_ref().unwrap().session.state().phone.menu_navigation.current() }
fn show_page(stack: &gtk::Stack, route: &PhoneMenuRoute) {
    if stack.visible_child_name().as_deref() == Some(route.name().as_str()) { return; }
    stack.set_visible_child_name(&route.name());
    if let Some(scroll) = stack.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>() { scroll.vadjustment().set_value(0.); }
}
impl Menu {
    pub fn new() -> Self {
        let root: drawers::Columns = glib::Object::new();
        root.set_widget_name("phone-menu");
        root.add_css_class("content-drawer");
        root.add_css_class("dock-panel");
        for class in ["window-bar", "header-medium", "phone-bar-tiles"] { root.add_css_class(class); }
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.set_widget_name("phone-menu-list");
        let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never).child(&body).build());
        scroll.set_widget_name("phone-menu-scroll");
        scroll.set_parent(&root);
        root.imp().children.borrow_mut().push(scroll.upcast());
        let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        separator.set_widget_name("phone-menu-separator");
        separator.set_can_target(false);
        separator.set_parent(&root);
        root.imp().children.borrow_mut().push(separator.clone().upcast());
        let buttons = std::array::from_fn(|index| {
            let button: gtk::Button = if matches!(index, 2 | 4) { gtk::ToggleButton::new().upcast() } else { gtk::Button::new() };
            button.set_widget_name(["phone-preferences", "phone-search", "phone-finger", "phone-zen", "phone-swap"][index]);
            button.add_css_class("flat"); button.add_css_class("header-tool");
            button.set_child(Some(&gtk::Image::new()));
            button.set_parent(&root);
            root.imp().children.borrow_mut().push(button.clone().upcast());
            button
        });
        *root.imp().geometry.borrow_mut() = vec![Bounds::default(); 7];
        Self { root, body, separator, buttons, pane: RefCell::default(), model: Rc::default(),
            contact: Rc::new(RefCell::default()) }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        let scroll = self.root.first_child().and_downcast::<gtk::ScrolledWindow>().unwrap();
        let body = self.body.clone(); let contact = self.contact.clone();
        scroll.vadjustment().connect_value_changed(glib::clone!(#[weak] body, #[strong] contact, move |_| {
            contact.borrow_mut().cancel_press(); swipe::cancel_pressed(body.upcast_ref());
        }));
        for (index, button) in self.buttons.iter().enumerate() {
            let model = self.model.clone();
            button.connect_clicked(glib::clone!(#[weak] w, #[strong] model, move |_| {
                let action = model.borrow().as_ref().and_then(|model| model.strip[index].action.clone());
                if let Some(action) = action { w.dispatch(action); }
            }));
        }
    }
    pub fn row_height(&self) -> f32 {
        self.pane.borrow().as_ref().map_or(0., |pane| pane.row.measure(gtk::Orientation::Vertical, -1).1 as f32)
    }
    pub fn measure(&self, phone: &PhoneLayout, row_height: f32) -> (f32, f32) {
        let pane = self.pane.borrow();
        let Some(root) = pane.as_ref().and_then(|pane| pane.stack.child_by_name("main")) else { return (0., 0.); };
        let pane = pane.as_ref().unwrap();
        let (minimum, natural, _, _) = pane.widget.measure(gtk::Orientation::Horizontal, -1);
        let inset = (natural - root.measure(gtk::Orientation::Horizontal, -1).1).max(0);
        let width = phone.menu_width(minimum as f32, natural as f32, row_height);
        let content_width = (width as i32 - inset).max(1);
        let fixed = (pane.widget.measure(gtk::Orientation::Vertical, width as i32).1
            - pane.stack.measure(gtk::Orientation::Vertical, content_width).1).max(0);
        (width, (root.measure(gtk::Orientation::Vertical, content_width).1 + fixed) as f32)
    }
    pub fn allocate(&self, phone: &PhoneLayout, placement: &DrawerPlacement, row_height: f32) {
        let view = phone.menu_geometry(placement, row_height);
        self.separator.set_orientation(if phone.edge == Edge::Top { gtk::Orientation::Horizontal } else { gtk::Orientation::Vertical });
        let [a, b, c, d, e] = view.buttons;
        let geometry = [view.list, view.separator, a, b, c, d, e];
        for button in &self.buttons {
            if let Some(image) = button.child().and_downcast::<gtk::Image>() { image.set_pixel_size(view.icon.round().max(1.) as i32); }
        }
        if self.root.imp().geometry.borrow().as_slice() != geometry.as_slice() {
            self.root.imp().geometry.borrow_mut().copy_from_slice(&geometry);
            self.root.queue_allocate();
        }
    }
    pub fn refresh(&self, w: &Rc<Workspace>) {
        let model = {
            let gpu = w.gpu.borrow(); let session = &gpu.as_ref().unwrap().session;
            let labels = w.documents.model.borrow().labels(&session.state().document_file, |parked| &parked.canvas.session.state().document_file, &w.localization());
            let selector = DrawingSelector::new(&labels, w.documents.selected(), &session.command(CommandId::NewDocument), &w.localization());
            session.phone_menu().unwrap().view(&selector, Platform::Gtk, &w.localization())
        };
        let reusable = self.model.borrow().as_ref().is_some_and(|old| compatible(old, &model));
        *self.model.borrow_mut() = Some(model);
        if !reusable { self.rebuild(w); }
        let model = self.model.borrow(); let model = model.as_ref().unwrap();
        for (button, item) in self.buttons.iter().zip(&model.strip) {
            button.set_tooltip_text(Some(&item.label)); button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
            button.set_sensitive(item.enabled); crate::icons::set(&button.child().and_downcast::<gtk::Image>().unwrap(), item.icon.as_deref());
            if let Some(active) = item.selected {
                button.downcast_ref::<gtk::ToggleButton>().unwrap().set_active(active);
                selected(button, active);
            }
        }
        if let Some(pane) = self.pane.borrow().as_ref() {
            if pane.stack.visible_child_name().as_deref() != Some(route(w).name().as_str()) { swipe::cancel(&pane.contents, &self.contact); }
            show_page(&pane.stack, &route(w));
            for row in &pane.rows {
                let item = row_model(model, &row.page, row.position).unwrap();
                row.label.set_text(&item.label); row.button.set_sensitive(item.enabled);
                row.button.update_property(&[gtk::accessible::Property::Label(&item.label), gtk::accessible::Property::HasPopup(item.target.is_some())]);
                if item.selected == Some(true) { row.button.set_state_flags(gtk::StateFlags::CHECKED, false); } else { row.button.unset_state_flags(gtk::StateFlags::CHECKED); }
                if let Some(check) = &row.check {
                    check.set_opacity(if item.selected == Some(true) { 1. } else { 0. });
                    if let Some(value) = item.selected { row.button.update_state(&[gtk::accessible::State::Checked(if value { gtk::AccessibleTristate::True } else { gtk::AccessibleTristate::False })]); }
                }
                crate::icons::set(&row.icon, item.icon.as_deref()); row.icon.set_visible(item.icon.is_some());
                let detail = if item.count.is_empty() { item.bindings.first().map(|key| key.chord.localized_label(Platform::Gtk, &w.localization())).unwrap_or_default() } else { item.count.clone() };
                row.detail.set_text(&detail); row.detail.set_visible(!detail.is_empty());
                if let (Some(close), Some(copy)) = (&row.close, &item.close) {
                    close.set_width_request(model.close_width as i32); close.set_tooltip_text(Some(&copy.label));
                    close.update_property(&[gtk::accessible::Property::Label(&copy.label)]);
                }
            }
        }
        w.surface.queue_allocate();
    }
    fn bind_row(&self, w: &Rc<Workspace>, button: &gtk::Button, page: PhoneMenuRoute, hit: PhoneMenuHit) {
        let contact = self.contact.clone(); let model = self.model.clone();
        let pointer = Rc::new(Cell::new(false));
        let events = gtk::EventControllerLegacy::new(); events.set_propagation_phase(gtk::PropagationPhase::Capture);
        events.connect_event(glib::clone!(#[strong] contact, #[strong] hit, #[strong] pointer, #[strong] page, move |_, event| {
            if matches!(event.event_type(), gdk::EventType::ButtonPress | gdk::EventType::TouchBegin) { pointer.set(true); contact.borrow_mut().press(&page, hit.clone()); }
            if event.event_type() == gdk::EventType::TouchCancel { contact.borrow_mut().cancel_press(); }
            if event.event_type() == gdk::EventType::KeyPress { pointer.set(false); }
            glib::Propagation::Proceed
        }));
        button.add_controller(events);
        button.connect_clicked(glib::clone!(#[weak] w, #[strong] contact, #[strong] model, move |button| {
            if !button.is_mapped() || page != route(&w) { return; }
            if pointer.replace(false) && !contact.borrow_mut().release(&route(&w), &hit, true) { return; }
            let action = {
            let borrowed = model.borrow(); let Some(model) = borrowed.as_ref() else { return; };
            match &hit {
                PhoneMenuHit::Back => Some(UiAction::Phone { action: PhoneAction::MenuBack { route: page.clone() } }),
                PhoneMenuHit::Row(position) => row_model(model, &page, position.map(|i| i as usize)).and_then(|row| {
                    row.target.as_ref().map(|route| UiAction::Phone { action: PhoneAction::MenuSelect { route: route.clone() } }).or_else(|| row.action.clone())
                }),
                PhoneMenuHit::Close(position) => row_model(model, &page, position.map(|i| i as usize)).and_then(|row| row.close.as_ref().map(|close| close.action.clone())),
                PhoneMenuHit::Strip(_) => None,
            }
            };
            if let Some(action) = action { w.dispatch(action); }
        }));
    }
    fn rebuild(&self, w: &Rc<Workspace>) {
        if let Some(pane) = self.pane.take() { self.body.remove(&pane.widget); }
        let model = self.model.borrow(); let model = model.as_ref().unwrap();
        let stack = gtk::Stack::builder().hhomogeneous(false).vhomogeneous(false).transition_type(gtk::StackTransitionType::SlideLeftRight).build();
        stack.set_widget_name("phone-menu-pages");
        let mut rows = Vec::new(); let mut measure_row = None;
        for page in &model.pages {
            let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
            body.set_widget_name(&format!("phone-menu-page-{}", page.id.name()));
            if page.parent.is_some() {
                let back: MenuRow = glib::Object::new(); back.set_widget_name("phone-menu-back");
                let line = gtk::CenterBox::new();
                let arrow = crate::icons::image("go-previous-symbolic"); arrow.add_css_class("menu-arrow"); arrow.add_css_class("menu-back-arrow");
                line.set_start_widget(Some(&arrow)); let label = gtk::Label::builder().label(&page.title).hexpand(true).ellipsize(gtk::pango::EllipsizeMode::End).build(); line.set_center_widget(Some(&label));
                back.set_child(Some(&line)); body.append(&back);
                self.bind_row(w, back.upcast_ref(), page.id.clone(), PhoneMenuHit::Back);
            }
            for (s, section) in page.sections.iter().enumerate() {
                if s > 0 { body.append(&gtk::Separator::new(gtk::Orientation::Horizontal)); }
                for (i, item) in section.iter().enumerate() {
                    if item.is_heading { let label = gtk::Label::new(Some(&item.label)); label.add_css_class("heading"); label.set_xalign(0.); body.append(&label); continue; }
                    let button: gtk::Button = if item.selected.is_some() { glib::Object::new::<MenuCheckRow>().upcast() } else { glib::Object::new::<MenuRow>().upcast() }; button.set_hexpand(true);
                    let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                    let check = page.checks.then(|| { let image = crate::icons::image("object-select-symbolic"); image.add_css_class("menu-check"); image.set_pixel_size(14); image.set_valign(gtk::Align::Center); image.set_margin_end(6); line.append(&image); image });
                    let icon = gtk::Image::new(); icon.set_pixel_size(16); icon.set_margin_end(6); line.append(&icon);
                    let label = gtk::Label::builder().xalign(0.).hexpand(true).width_chars(1).ellipsize(if item.ellipsis == PhoneMenuEllipsis::Middle { gtk::pango::EllipsizeMode::Middle } else { gtk::pango::EllipsizeMode::End }).build();
                    label.set_widget_name("menu-label"); line.append(&label);
                    let detail = gtk::Label::new(None); detail.set_widget_name("menu-detail"); detail.add_css_class("dim-label"); detail.set_margin_start(12); line.append(&detail);
                    if item.target.is_some() { button.add_css_class("submenu-row"); let arrow = crate::icons::image("go-next-symbolic"); arrow.add_css_class("menu-arrow"); arrow.add_css_class("menu-forward-arrow"); arrow.set_valign(gtk::Align::Center); arrow.set_margin_start(6); line.append(&arrow); }
                    button.set_child(Some(&line));
                    if item.target.as_ref().is_some_and(|target| target.page == Some(MenuPageId::Drawings)) { button.set_widget_name("phone-drawing"); }
                    if page.id.page == Some(MenuPageId::Drawings) && matches!(item.action, Some(UiAction::Invoke { command: CommandId::NewDocument })) { button.set_widget_name("phone-new-drawing"); }
                    if let Some(UiAction::Phone { action: PhoneAction::Drawing { id, .. } }) = item.action { button.set_widget_name(&format!("phone-drawing-{id}")); }
                    self.bind_row(w, button.upcast_ref(), page.id.clone(), PhoneMenuHit::Row([s as u32, i as u32]));
                    let close = item.close.as_ref().map(|_| {
                        let close = gtk::Button::new(); close.add_css_class("flat"); close.add_css_class("header-tool"); close.set_child(Some(&crate::icons::image("window-close-symbolic")));
                        if let Some(UiAction::Phone { action: PhoneAction::Drawing { id, .. } }) = item.action { close.set_widget_name(&format!("phone-drawing-close-{id}")); }
                        self.bind_row(w, &close, page.id.clone(), PhoneMenuHit::Close([s as u32, i as u32])); close
                    });
                    if let Some(close) = &close { let row = gtk::Box::new(gtk::Orientation::Horizontal, 0); row.append(&button); row.append(close); body.append(&row); } else { body.append(&button); }
                    if page.parent.is_none() && measure_row.is_none() && item.target.as_ref().is_none_or(|route| route.page != Some(MenuPageId::Drawings)) { measure_row = Some(button.clone()); }
                    rows.push(RowView { page: page.id.clone(), position: [s, i], button, label, check, icon, detail, close });
                }
            }
            stack.add_titled(&body, Some(&page.id.name()), &page.title);
        }
        let contents: MenuContents = glib::Object::new(); contents.imp().root.set(stack.child_by_name("main").as_ref()); contents.append(&stack);
        swipe::bind(w, &contents, &stack, self.contact.clone());
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(#[weak] w, #[weak] stack, #[upgrade_or] glib::Propagation::Proceed, move |_, key, _, _| {
            let rtl = stack.direction() == gtk::TextDirection::Rtl;
            if key == gdk::Key::Escape || key == if rtl { gdk::Key::Right } else { gdk::Key::Left } {
                w.dispatch(UiAction::Phone { action: PhoneAction::MenuBack { route: route(&w) } });
                return glib::Propagation::Stop;
            }
            if matches!(key, gdk::Key::Up | gdk::Key::Down) {
                stack.child_focus(if key == gdk::Key::Up { gtk::DirectionType::TabBackward } else { gtk::DirectionType::TabForward }); return glib::Propagation::Stop;
            }
            if key == if rtl { gdk::Key::Left } else { gdk::Key::Right } {
                if let Some(focus) = gtk::prelude::GtkWindowExt::focus(&w.window).and_downcast::<gtk::Button>().filter(|row| row.has_css_class("submenu-row")) { focus.emit_clicked(); }
                return glib::Propagation::Stop;
            }
            if matches!(key, gdk::Key::Home | gdk::Key::End) {
                if let Some(page) = stack.visible_child() {
                    let mut buttons = Vec::new();
                    fn visit(widget: &gtk::Widget, buttons: &mut Vec<gtk::Widget>) {
                        if widget.is::<gtk::Button>() && widget.is_sensitive() { buttons.push(widget.clone()); return; }
                        let mut child = widget.first_child(); while let Some(widget) = child { child = widget.next_sibling(); visit(&widget, buttons); }
                    }
                    visit(&page, &mut buttons);
                    if let Some(button) = if key == gdk::Key::Home { buttons.first() } else { buttons.last() } { button.grab_focus(); }
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        })); contents.add_controller(keys);
        let widget: InlineMenu = glib::Object::new(); widget.set_orientation(gtk::Orientation::Vertical);
        widget.add_css_class("menu"); widget.add_css_class("phone-menu-native"); widget.append(&contents); self.body.append(&widget);
        show_page(&stack, &route(w));
        *self.pane.borrow_mut() = Some(Pane { widget, contents, stack, row: measure_row.unwrap(), rows });
    }
}
