//! Inline window-bar palette. Search stays in the shared, modal tool picker.
use super::*;

pub(super) struct Editor {
    pub root: gtk::ScrolledWindow,
    content: adw::WrapBox,
    sizes: [gtk::ToggleButton; 3],
    canvas_info: gtk::CheckButton,
    components: Vec<(HeaderDragSource, gtk::Box)>,
    selected: Cell<Option<u32>>,
    shown: Cell<bool>,
    pub height: Cell<f32>,
}
impl Editor {
    pub fn new() -> Self {
        let root = gtk::ScrolledWindow::new();
        root.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        root.set_propagate_natural_height(true);
        root.set_widget_name("header-editor");
        root.add_css_class("header-editor");
        let content = adw::WrapBox::new();
        content.set_widget_name("header-components");
        content.set_child_spacing(6);
        content.set_line_spacing(8);
        content.add_css_class("header-editor-content");
        root.set_child(Some(&content));
        let sizes = std::array::from_fn(|i| {
            let button = gtk::ToggleButton::with_label(HeaderSize::ALL[i].label());
            button.set_widget_name(&format!("header-size-{i}"));
            button
        });
        let canvas_info = gtk::CheckButton::with_label("Show footer");
        canvas_info.set_widget_name("header-canvas-info");
        Self {
            root,
            content,
            sizes,
            canvas_info,
            components: std::iter::once(HeaderDragSource::Tools)
                .chain(
                    HeaderItem::COMPONENTS
                        .into_iter()
                        .filter(|item| item.available_on(Platform::Gtk))
                        .map(HeaderDragSource::Component),
                )
                .map(|source| (source, gtk::Box::new(gtk::Orientation::Horizontal, 0)))
                .collect(),
            selected: Cell::new(None),
            shown: Cell::new(false),
            height: Cell::new(0.),
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        for (source, chip) in &self.components {
            let source = *source;
            let (label, icon) = match source {
                HeaderDragSource::Tools => ("Add Tools…", "list-add-symbolic"),
                HeaderDragSource::Component(item) => match item {
                    HeaderItem::Capy => ("Capy", "layer-zen-looking-up-symbolic"),
                    HeaderItem::Menu => ("Main Menu", "layer-menu-symbolic"),
                    HeaderItem::MenuLabels => ("Menu Labels", "view-list-symbolic"),
                    HeaderItem::Settings => ("Settings", "layer-settings-symbolic"),
                    HeaderItem::Fullscreen => ("Full Screen", "layer-fullscreen-enter-symbolic"),
                    HeaderItem::Workspaces => ("Workspaces", "view-grid-symbolic"),
                    HeaderItem::DocumentTitle => ("Document Title", "text-x-generic-symbolic"),
                    HeaderItem::Clock => ("Clock", "preferences-system-time-symbolic"),
                    HeaderItem::Battery => ("Battery", "battery-level-100-symbolic"),
                    HeaderItem::Space => ("Space", "insert-object-symbolic"),
                    _ => unreachable!(),
                },
                HeaderDragSource::Item(_) => unreachable!(),
            };
            let name = match source {
                HeaderDragSource::Tools => "tools".into(),
                HeaderDragSource::Component(item) => item.label().to_lowercase().replace(' ', "-"),
                _ => unreachable!(),
            };
            chip.set_widget_name(&format!("header-component-{name}"));
            chip.add_css_class("header-component");
            chip.set_tooltip_text(Some(&format!("Drag {label} into the title bar")));
            chip.update_property(&[gtk::accessible::Property::Label(label)]);
            w.register_drag(chip, DragTarget::Header(source));
            // One inert drag surface, including its padding, icon and label.
            // No button/activation behavior or separate handle hit target.
            let grip = crate::icons::image("layer-grip-symbolic");
            grip.add_css_class("header-component-grip");
            grip.set_valign(gtk::Align::Center);
            grip.set_widget_name(&format!("header-add-grip-{name}"));
            chip.append(&grip);
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            content.set_widget_name(&format!("header-add-{name}"));
            content.add_css_class("header-component-body");
            content.append(&crate::icons::image(icon));
            content.append(&gtk::Label::new(Some(label)));
            chip.append(&content);
            self.content.append(chip);
        }
        // Keep settings and confirmation together at the trailing edge. The
        // native wrap layout puts this group on another line only when needed.
        let options = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        options.set_widget_name("header-editor-options");
        options.set_hexpand(true);
        options.set_halign(gtk::Align::End);
        options.set_valign(gtk::Align::Center);
        options.set_margin_start(6);
        let choices = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        choices.add_css_class("linked");
        choices.set_homogeneous(true);
        choices.update_property(&[gtk::accessible::Property::Label("Title bar size")]);
        for (i, button) in self.sizes.iter().enumerate() {
            button.set_tooltip_text(Some(&format!("{} title bar", HeaderSize::ALL[i].label())));
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| {
                    w.dispatch(
                        HeaderAction::SetSize {
                            size: HeaderSize::ALL[i],
                        }
                        .action(),
                    );
                }
            ));
            choices.append(button);
        }
        options.append(&choices);
        self.canvas_info.connect_toggled(glib::clone!(
            #[weak]
            w,
            move |button| {
                if !w.refreshing.get() {
                    w.dispatch(
                        HeaderAction::CanvasInfo {
                            visible: button.is_active(),
                        }
                        .action(),
                    );
                }
            }
        ));
        options.append(&self.canvas_info);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        footer.set_halign(gtk::Align::End);
        let cancel = w.action_button("Cancel", HeaderAction::Cancel.action());
        cancel.set_widget_name("header-edit-cancel");
        footer.append(&cancel);
        let done = w.action_button("Done", HeaderAction::Edit { editing: false }.action());
        done.set_widget_name("header-edit-done");
        done.add_css_class("suggested-action");
        footer.append(&done);
        options.append(&footer);
        self.content.append(&options);
    }
    pub fn select(&self, w: &Workspace, selected: Option<u32>) {
        self.selected.set(selected);
        w.header.refresh_selection();
        w.header.root.queue_draw();
    }
    pub fn selected_item(&self) -> Option<u32> {
        self.selected.get()
    }
    pub fn focus(&self) {
        if let Some(button) = self.sizes.iter().find(|b| b.is_active()) {
            button.grab_focus();
        }
    }
    pub fn refresh_canvas_info(&self, visible: bool) {
        self.canvas_info.set_active(visible);
    }
    pub fn move_item(&self, w: &Rc<Workspace>, id: u32, forward: bool) {
        let action = w
            .header
            .model
            .borrow()
            .as_ref()
            .and_then(|m| m.step(id, forward));
        if let Some(action) = action {
            w.dispatch(action.action());
        }
    }
    pub fn refresh(&self, model: &HeaderLayout, editing: bool) {
        if self.shown.replace(editing) != editing {
            self.selected.set(None);
        }
        self.root.set_visible(editing);
        for (i, button) in self.sizes.iter().enumerate() {
            button.set_active(model.size as usize == i);
        }
        if self
            .selected
            .get()
            .is_some_and(|id| model.entry(id).is_err())
        {
            self.selected.set(None);
        }
        let capacity = model.entries().count() < 128;
        for (source, chip) in &self.components {
            chip.set_visible(match source {
                HeaderDragSource::Component(item) => {
                    !item.singleton() || !model.entries().any(|e| e.item == *item)
                }
                _ => true,
            });
            chip.set_sensitive(capacity);
        }
    }
    pub fn allocate(&self, width: f32, bar_height: f32, available_height: f32) {
        let width = (width - 12.).max(1.);
        let height = self
            .content
            .measure(gtk::Orientation::Vertical, width as i32)
            .1 as f32;
        let height = height.min((available_height - bar_height - 12.).max(1.));
        self.height.set(height + 12.);
        allocate_at(
            self.root.upcast_ref(),
            Bounds {
                x: 6.,
                y: bar_height + 6.,
                width,
                height,
            },
        );
    }
}
