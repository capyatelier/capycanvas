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
    pub struct Contents;
    #[glib::object_subclass]
    impl ObjectSubclass for Contents {
        const NAME: &'static str = "CapyInlineMenuContents";
        type Type = super::MenuContents;
        type ParentType = gtk::Box;
        fn class_init(class: &mut Self::Class) { class.set_css_name("contents"); }
    }
    impl ObjectImpl for Contents {}
    impl WidgetImpl for Contents {}
    impl BoxImpl for Contents {}
}
glib::wrapper! {
    pub struct InlineMenu(ObjectSubclass<native::Menu>)
        @extends gtk::Box, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}
glib::wrapper! {
    pub struct MenuContents(ObjectSubclass<native::Contents>)
        @extends gtk::Box, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}
struct Pane {
    _popup: gtk::PopoverMenu,
    widget: InlineMenu,
    stack: gtk::Stack,
    navigation_handler: Option<glib::SignalHandlerId>,
}
impl Drop for Pane {
    fn drop(&mut self) {
        if let Some(handler) = self.navigation_handler.take() { self.stack.disconnect(handler); }
    }
}
impl Pane {
    fn select(stack: &gtk::Stack, page: &PhoneMenuPage) {
        stack.set_visible_child_name("main");
        for depth in 1..=page.path.len() {
            let target = PhoneMenuNavigation::page_name(&page.path[..depth]);
            let mut rows = Vec::new();
            descendants(&stack.visible_child().unwrap(), &mut rows);
            if let Some(row) = rows.into_iter().find(|row| row.type_().name() == "GtkModelButton"
                && row.property::<Option<String>>("menu-name").as_deref() == Some(target.as_str())) { row.activate(); }
        }
    }
}
pub(super) struct Menu {
    pub root: drawers::Columns,
    pub bodies: [gtk::Box; 2],
    pub separator: gtk::Separator,
    pub drawing: gtk::Button,
    pub drawing_row: gtk::Box,
    preferences: RefCell<Option<gtk::Button>>,
    panes: RefCell<Vec<Pane>>,
    model: RefCell<Option<[ContextMenu; 2]>>,
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
        let bodies = std::array::from_fn(|index| {
            let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
            body.set_widget_name(&format!("phone-menu-column-{index}"));
            let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never).child(&body).build());
            scroll.set_widget_name(&format!("phone-menu-scroll-{index}"));
            scroll.set_parent(&root);
            root.imp().children.borrow_mut().push(scroll.upcast());
            root.imp().geometry.borrow_mut().push(Bounds::default());
            body
        });
        let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        separator.set_widget_name("phone-menu-separator");
        separator.set_can_target(false);
        separator.set_parent(&root);
        root.imp().children.borrow_mut().push(separator.clone().upcast());
        root.imp().geometry.borrow_mut().push(Bounds::default());
        let drawing = gtk::Button::with_label("");
        if let Some(label) = drawing.child().and_downcast::<gtk::Label>() {
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_width_chars(1);
            label.set_xalign(0.);
        }
        drawing.set_widget_name("phone-drawing");
        drawing.add_css_class("flat");
        drawing.set_hexpand(true);
        let drawing_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        drawing_row.append(&drawing);
        bodies[0].append(&drawing_row);
        Self { root, bodies, separator, drawing, drawing_row, preferences: RefCell::default(), panes: RefCell::default(), model: RefCell::default(),
            navigation: Rc::new(RefCell::default()) }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        self.drawing.connect_clicked(glib::clone!(#[weak] w, move |_| w.documents.show_selector(&w)));
    }
    pub fn clear(&self) {
        self.model.borrow_mut().take();
        *self.navigation.borrow_mut() = PhoneMenuNavigation::default();
    }
    pub fn geometry(&self, placement: &DrawerPlacement) -> PhoneMenuGeometry {
        self.navigation.borrow().geometry(placement)
    }
    pub fn size_controls(&self, layout: &PhoneLayout) {
        if let Some(button) = self.preferences.borrow().as_ref() {
            let tile = layout.tile.round().max(1.) as i32;
            button.set_size_request(tile, tile);
            if let Some(image) = button.child().and_downcast::<gtk::Image>() {
                let icon = layout.icon.round().max(1.) as i32;
                if image.pixel_size() != icon { image.set_pixel_size(icon); }
            }
        }
    }
    pub fn workspaces(&self, w: &Rc<Workspace>) {
        let Some(model) = self.model.borrow().clone() else { return; };
        let found = self.navigation.borrow_mut().workspaces(&model[1], &w.localization());
        if found && let Some(pane) = self.panes.borrow().get(1) {
            let name = self.navigation.borrow().name();
            if let Some(page) = PhoneMenuNavigation::pages(&model[1]).iter().find(|page| page.name() == name) {
                Pane::select(&pane.stack, page);
            }
        }
    }
    pub fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        if !state.phone.menu { self.clear(); return; }
        let (model, settings) = {
            let gpu = w.gpu.borrow();
            let session = &gpu.as_ref().unwrap().session;
            (session.phone_menu_columns(), session.command(CommandId::Settings))
        };
        if self.model.borrow().as_ref() != Some(&model) {
            for (body, pane) in self.bodies.iter().zip(self.panes.take()) { body.remove(&pane.widget); }
            let selected = self.navigation.borrow().name();
            if !self.navigation.borrow_mut().select(&model[1], &selected) {
                *self.navigation.borrow_mut() = PhoneMenuNavigation::default();
            }
            for (index, menu) in model.iter().enumerate() {
                let (popup, stack) = w.inline_workspace_menu(menu.clone());
                stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
                stack.set_hhomogeneous(false);
                stack.set_vhomogeneous(false);
                if index == 1 { stack.set_widget_name("phone-menu-pages"); }
                let pages = PhoneMenuNavigation::pages(menu);
                for page in &pages {
                    let child = stack.child_by_name(&page.name()).unwrap();
                    stack.page(&child).set_title(&page.menu.title);
                    child.set_widget_name(&format!("phone-menu-page-{}", page.name()));
                    let mut rows = Vec::new();
                    descendants(&child, &mut rows);
                    for label in rows.iter().filter_map(|widget| widget.downcast_ref::<gtk::Label>())
                        .filter(|label| label.css_name() == "label" && label.parent().is_some_and(|parent| parent.type_().name() == "GtkModelButton")) {
                        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    }
                    let mut rows = rows.into_iter().filter(|widget| widget.type_().name() == "GtkModelButton");
                    if !page.path.is_empty() && let Some(back) = rows.next() {
                        back.set_property("text", &page.menu.title);
                        back.set_widget_name("phone-menu-back");
                    }
                    for (item, row) in page.menu.sections.iter().flatten().filter(|item| !item.is_heading()).zip(rows) {
                        if item.action.is_none() && item.enabled && !item.sections.is_empty() {
                            row.set_property("text", &item.label);
                        }
                        match &item.action {
                            Some(UiAction::Phone { action: PhoneAction::SwapSides }) => row.set_widget_name("phone-swap"),
                            Some(UiAction::Invoke { command: CommandId::ZenMode }) => row.set_widget_name("phone-zen"),
                            Some(UiAction::Invoke { command: CommandId::SearchCommands }) if index == 0 => {
                                row.set_widget_name("phone-search");
                                row.set_hexpand(true);
                                let gear = w.action_button("", UiAction::Invoke { command: CommandId::Settings });
                                gear.set_widget_name("phone-preferences");
                                gear.set_child(Some(&crate::icons::image("layer-settings-symbolic")));
                                gear.add_css_class("flat");
                                gear.add_css_class("header-tool");
                                gear.set_tooltip_text(Some(&settings.label));
                                gear.update_property(&[gtk::accessible::Property::Label(&settings.label)]);
                                gear.set_sensitive(settings.enabled);
                                let chip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                                chip.set_valign(gtk::Align::Center);
                                chip.add_css_class("header-bar");
                                chip.append(&gear);
                                if let Some(parent) = row.parent().and_downcast::<gtk::Box>() {
                                    parent.set_orientation(gtk::Orientation::Horizontal);
                                    parent.set_spacing(8);
                                    parent.append(&chip);
                                }
                                *self.preferences.borrow_mut() = Some(gear);
                            }
                            _ => (),
                        }
                    }
                }
                let navigation_handler = if index == 1 {
                    let name = self.navigation.borrow().name();
                    if let Some(page) = pages.iter().find(|page| page.name() == name) { Pane::select(&stack, page); }
                    let model = menu.clone();
                    let navigation = self.navigation.clone();
                    Some(stack.connect_visible_child_name_notify(glib::clone!(#[weak] w, move |stack| {
                        if let Some(name) = stack.visible_child_name() {
                            navigation.borrow_mut().select(&model, &name);
                            w.surface.queue_allocate();
                        }
                    })))
                } else { None };
                let contents: MenuContents = glib::Object::new();
                contents.set_orientation(gtk::Orientation::Vertical);
                contents.append(&stack);
                let widget: InlineMenu = glib::Object::new();
                widget.set_orientation(gtk::Orientation::Vertical);
                widget.add_css_class("menu");
                widget.add_css_class("phone-menu-native");
                if index == 0 { widget.add_css_class("phone-menu-controls"); }
                widget.append(&contents);
                self.bodies[index].append(&widget);
                self.panes.borrow_mut().push(Pane { _popup: popup, widget, stack, navigation_handler });
            }
            for scroll in self.root.imp().children.borrow().iter().filter_map(|child| child.downcast_ref::<gtk::ScrolledWindow>()) {
                scroll.vadjustment().set_value(0.);
            }
            *self.model.borrow_mut() = Some(model);
        }
        self.drawing.set_label(&DocumentTabLabel::new(w.documents.selected(), &state.document_file, &w.localization()).menu_label(w.documents.len()));
        self.drawing.set_sensitive(w.documents.len() > 1);
    }
}
