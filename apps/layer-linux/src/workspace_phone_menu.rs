use super::*;

mod native {
    use super::*;
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
struct Pane {
    _popup: gtk::PopoverMenu,
    widget: InlineMenu,
    stack: gtk::Stack,
    navigation_handler: Option<glib::SignalHandlerId>,
    state: customization::InlineMenuState,
    row: gtk::Widget,
}
impl Drop for Pane {
    fn drop(&mut self) {
        if let Some(handler) = self.navigation_handler.take() { self.stack.disconnect(handler); }
    }
}
impl Pane {
    fn select(&self, page: &PhoneMenuPage) {
        self.stack.set_visible_child_name("main");
        for depth in 1..=page.path.len() {
            let target = PhoneMenuNavigation::page_name(&page.path[..depth]);
            let mut rows = Vec::new();
            descendants(&self.stack.visible_child().unwrap(), &mut rows);
            if let Some(row) = rows.into_iter().find(|row| row.type_().name() == "GtkModelButton"
                && row.property::<Option<String>>("menu-name").as_deref() == Some(target.as_str())) { row.activate(); }
        }
    }
}
pub(super) struct Menu {
    pub root: drawers::Columns,
    body: gtk::Box,
    separator: gtk::Separator,
    buttons: [gtk::Button; 5],
    drawing: RefCell<Option<gtk::Widget>>,
    pane: RefCell<Option<Pane>>,
    model: Rc<RefCell<Option<PhoneMenu>>>,
    navigation: Rc<RefCell<PhoneMenuNavigation>>,
}
fn descendants(widget: &gtk::Widget, found: &mut Vec<gtk::Widget>) {
    found.push(widget.clone());
    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        descendants(&widget, found);
    }
}
impl Menu {
    pub fn new() -> Self {
        let root: drawers::Columns = glib::Object::new();
        root.set_widget_name("phone-menu");
        root.add_css_class("content-drawer");
        root.add_css_class("dock-panel");
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
        Self { root, body, separator, buttons, drawing: RefCell::default(), pane: RefCell::default(), model: Rc::default(),
            navigation: Rc::new(RefCell::default()) }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        for (index, button) in self.buttons.iter().enumerate() {
            let model = self.model.clone();
            button.connect_clicked(glib::clone!(#[weak] w, #[strong] model, move |_| {
                let action = model.borrow().as_ref().and_then(|model| model.strip[index].action.clone());
                if let Some(action) = action { w.dispatch(action); }
            }));
        }
    }
    pub fn clear(&self) {
        self.model.borrow_mut().take();
        *self.navigation.borrow_mut() = PhoneMenuNavigation::default();
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
    fn select(&self, w: &Workspace, drawings: bool) {
        let model = self.model.borrow();
        let Some(model) = model.as_ref() else { return; };
        let found = if drawings { self.navigation.borrow_mut().drawings(&model.list, &w.localization()) }
            else { self.navigation.borrow_mut().workspaces(&model.list, &w.localization()) };
        if found && let Some(pane) = self.pane.borrow().as_ref() {
            let name = self.navigation.borrow().name();
            if let Some(page) = PhoneMenuNavigation::pages(&model.list).iter().find(|page| page.name() == name) { pane.select(page); }
        }
    }
    pub fn workspaces(&self, w: &Rc<Workspace>) { self.select(w, false); }
    pub fn drawings(&self, w: &Rc<Workspace>) { self.select(w, true); }
    pub fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        if !state.phone.menu { self.clear(); return; }
        let (mut model, selector) = {
            let gpu = w.gpu.borrow();
            let session = &gpu.as_ref().unwrap().session;
            let labels = w.documents.model.borrow().labels(&state.document_file, |parked| &parked.canvas.session.state().document_file, &w.localization());
            (session.phone_menu().unwrap(), DrawingSelector::new(&labels, w.documents.selected(), &session.command(CommandId::NewDocument), &w.localization()))
        };
        model.list.sections.insert(0, vec![selector.page.clone()]);
        for (button, item) in self.buttons.iter().zip(&model.strip) {
            button.set_tooltip_text(Some(&item.label));
            button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
            button.set_sensitive(item.enabled);
            crate::icons::set(&button.child().and_downcast::<gtk::Image>().unwrap(), item.icon.as_deref());
            if let Some(active) = item.selected {
                button.downcast_ref::<gtk::ToggleButton>().unwrap().set_active(active);
                button.update_state(&[gtk::accessible::State::Checked(if active { gtk::AccessibleTristate::True } else { gtk::AccessibleTristate::False })]);
                selected(button, active);
            }
        }
        let reusable = self.model.borrow().is_some() && self.pane.borrow().as_ref().is_some_and(|pane| pane.state.update(&model.list));
        if !reusable {
            if let Some(pane) = self.pane.take() { self.body.remove(&pane.widget); }
            self.drawing.borrow_mut().take();
            let name = self.navigation.borrow().name();
            if !self.navigation.borrow_mut().select(&model.list, &name) { *self.navigation.borrow_mut() = PhoneMenuNavigation::default(); }
            let (popup, stack, live) = w.inline_workspace_menu(model.list.clone());
            stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
            stack.set_hhomogeneous(false); stack.set_vhomogeneous(false);
            stack.set_widget_name("phone-menu-pages");
            let pages = PhoneMenuNavigation::pages(&model.list);
            let mut menu_row = None;
            for page in &pages {
                let child = stack.child_by_name(&page.name()).unwrap();
                stack.page(&child).set_title(&page.menu.title);
                child.set_widget_name(&format!("phone-menu-page-{}", page.name()));
                let mut rows = Vec::new(); descendants(&child, &mut rows);
                for label in rows.iter().filter_map(|widget| widget.downcast_ref::<gtk::Label>())
                    .filter(|label| label.css_name() == "label" && label.parent().is_some_and(|parent| parent.type_().name() == "GtkModelButton")) {
                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                }
                let mut rows = rows.into_iter().filter(|widget| widget.type_().name() == "GtkModelButton");
                if !page.path.is_empty() && let Some(back) = rows.next() { back.set_property("text", &page.menu.title); back.set_widget_name("phone-menu-back"); swipe::release_activation(&back); }
                for (item, row) in page.menu.sections.iter().flatten().filter(|item| !item.is_heading()).zip(rows) {
                    swipe::release_activation(&row);
                    if item.action.is_none() && item.enabled && !item.sections.is_empty() { row.set_property("text", &item.label); }
                    if page.path.is_empty() && item == &selector.page {
                        row.set_widget_name("phone-drawing"); row.set_hexpand(true);
                        *self.drawing.borrow_mut() = Some(row.clone());
                    } else if page.path.is_empty() { menu_row.get_or_insert_with(|| row.clone()); }
                    match &item.action {
                        Some(UiAction::Invoke { command: CommandId::NewDocument }) if page.path == [[0, 0]] => row.set_widget_name("phone-new-drawing"),
                        Some(UiAction::Phone { action: PhoneAction::Drawing { id, .. } }) => {
                            row.set_widget_name(&format!("phone-drawing-{id}"));
                            let close = w.action_button("", UiAction::Phone { action: PhoneAction::Drawing { id: *id, close: true } });
                            close.set_widget_name(&format!("phone-drawing-close-{id}"));
                            close.set_child(Some(&crate::icons::image("window-close-symbolic")));
                            close.add_css_class("flat"); close.add_css_class("header-tool"); close.set_width_request(34);
                            close.set_tooltip_text(Some(&item.hint));
                            close.update_property(&[gtk::accessible::Property::Label(&item.hint)]);
                            let parent = row.parent().and_downcast::<gtk::Box>().unwrap();
                            let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                            parent.insert_child_after(&line, Some(&row)); parent.remove(&row);
                            row.set_hexpand(true); line.append(&row); line.append(&close);
                            let mut children = Vec::new(); descendants(&row, &mut children);
                            for label in children.iter().filter_map(|w| w.downcast_ref::<gtk::Label>()).filter(|label| label.css_name() == "label") {
                                label.set_use_underline(false); label.set_ellipsize(gtk::pango::EllipsizeMode::Middle); label.set_width_chars(1);
                            }
                        }
                        _ => (),
                    }
                }
            }
            let contents: MenuContents = glib::Object::new();
            contents.imp().root.set(stack.child_by_name("main").as_ref());
            contents.append(&stack);
            swipe::bind(&contents, &stack, self.navigation.clone());
            let widget: InlineMenu = glib::Object::new();
            widget.set_orientation(gtk::Orientation::Vertical);
            widget.add_css_class("menu"); widget.add_css_class("phone-menu-native"); widget.append(&contents);
            self.body.append(&widget);
            let mut pane = Pane { _popup: popup, widget, stack, navigation_handler: None, state: live, row: menu_row.unwrap() };
            if let Some(page) = pages.iter().find(|page| page.name() == self.navigation.borrow().name()) { pane.select(page); }
            let menu = model.list.clone(); let navigation = self.navigation.clone();
            pane.navigation_handler = Some(pane.stack.connect_visible_child_name_notify(glib::clone!(#[weak] w, move |stack| {
                if let Some(name) = stack.visible_child_name() { navigation.borrow_mut().select(&menu, &name); w.surface.queue_allocate(); }
            })));
            *self.pane.borrow_mut() = Some(pane);
        }
        *self.model.borrow_mut() = Some(model);
        if let Some(row) = self.drawing.borrow().as_ref() {
            row.set_property("text", &selector.label);
            row.set_property("accel", (!selector.count.is_empty()).then_some("0"));
            row.reset_property(gtk::AccessibleProperty::KeyShortcuts);
            let mut children = Vec::new(); descendants(row, &mut children);
            let arrow = children.iter().find(|child| child.css_name() == "arrow" && child.parent().as_ref() == Some(row));
            for label in children.iter().filter_map(|w| w.downcast_ref::<gtk::Label>()) {
                if label.css_name() == "accelerator" { label.set_text(&selector.count); label.insert_before(row, arrow); }
                else if label.css_name() == "label" { label.set_use_underline(false); label.set_ellipsize(gtk::pango::EllipsizeMode::End); label.set_width_chars(1); }
            }
        }
    }
}
