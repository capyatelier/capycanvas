//! Native presentation of the shared workspace customization models.
//! GTK owns gestures/widgets; catalogs, selection, validation and edits are Rust UI policy.
use super::*;

pub(super) fn drawer_origin(button: &gtk::Button, direction: Option<Edge>) {
    for (edge, class) in [
        (Edge::Top, "drawer-origin-top"),
        (Edge::Bottom, "drawer-origin-bottom"),
        (Edge::Left, "drawer-origin-left"),
        (Edge::Right, "drawer-origin-right"),
    ] {
        if direction == Some(edge) {
            button.add_css_class(class);
        } else {
            button.remove_css_class(class);
        }
    }
}

pub(super) fn tile_button(
    w: &Rc<Workspace>,
    config: &PanelConfig,
    tile: &ToolbarTile,
) -> gtk::Button {
    let choice = tool_choice(tile.control);
    let panel = config.id;
    let id = tile.id;
    let button = gtk::Button::builder().tooltip_text(&choice.label).build();
    let icon = gtk::Image::from_icon_name(&format!(
        "layer-{}-symbolic",
        if tile.control == ToolbarControl::Color {
            "colors"
        } else {
            choice.icon
        }
    ));
    icon.set_pixel_size(config.tile_style.icon_size() as i32);
    let label_lines = config.tile_style.label_lines();
    if label_lines > 0 {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        icon.set_size_request(TILE_SIZE as i32, -1);
        let label = gtk::Label::new(Some(&choice.label));
        label.set_hexpand(true);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_lines(label_lines as i32);
        label.set_max_width_chars(1);
        label.set_margin_end(4);
        let attributes = gtk::pango::AttrList::new();
        attributes.insert(gtk::pango::AttrInt::new_weight(
            if config.tile_style == TileStyle::Labeled {
                gtk::pango::Weight::Bold
            } else {
                gtk::pango::Weight::Normal
            },
        ));
        label.set_attributes(Some(&attributes));
        label.set_xalign(0.0);
        label.set_valign(gtk::Align::Center);
        row.append(&icon);
        row.append(&label);
        button.set_child(Some(&row));
    } else {
        button.set_child(Some(&icon));
    }
    button.add_css_class("flat");
    button.set_widget_name(&format!("tile-{id}"));
    button.update_property(&[gtk::accessible::Property::Label(&choice.label)]);
    button.connect_clicked(glib::clone!(
        #[weak]
        w,
        move |button| {
            if let Some(b) = button.compute_bounds(&w.surface) {
                w.customization
                    .anchor
                    .set([b.x() + b.width() * 0.5, b.y() + b.height()]);
            }
            w.dispatch(UiAction::ActivateTile { panel, tile: id });
        }
    ));
    if tile.control == ToolbarControl::Color {
        button.add_css_class("brush-color");
        #[allow(deprecated)]
        button.style_context().add_provider(
            &w.customization.palette,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    if tile.control == ToolbarControl::Divider {
        let line = gtk::Separator::new(gtk::Orientation::Horizontal);
        line.set_halign(gtk::Align::Center);
        line.set_valign(gtk::Align::Center);
        button.set_child(Some(&line));
        button.add_css_class("toolbar-divider");
    }
    button
}

pub(super) struct ToolbarView {
    pub id: Panel,
    pub strip: TileStrip,
    tiles: Vec<ToolbarTile>,
    buttons: Vec<gtk::Button>,
    style: TileStyle,
}

enum FieldValue {
    Size(crate::number_control::NumberControl),
    Opacity(crate::number_control::NumberControl),
    Color(gtk::ColorDialogButton),
    Brush(gtk::DropDown),
    Layer(gtk::DropDown),
    LayerOpacity(crate::number_control::NumberControl),
    Commands(Vec<(CommandId, gtk::Button)>),
}
struct ControlWidget {
    panel: Panel,
    control: PanelControl,
    widget: gtk::Widget,
    value: Option<FieldValue>,
    configuration: bool,
}
struct ToolbarOptionWidget {
    section: usize,
    item: usize,
    widget: gtk::Widget,
    hint: Option<gtk::Label>,
}

struct ToolbarManagerUi {
    dialog: adw::Dialog,
    description: gtk::Label,
    empty: gtk::Label,
    list: gtk::ListBox,
    delete: gtk::Button,
    panels: RefCell<Vec<Panel>>,
    rows_key: RefCell<String>,
    shown: Cell<bool>,
}
impl ToolbarManagerUi {
    fn new() -> Self {
        let dialog = adw::Dialog::builder()
            .content_width(480)
            .content_height(420)
            .build();
        dialog.set_widget_name("toolbar-manager");
        dialog.add_css_class("layer-preferences");
        Self {
            dialog,
            description: gtk::Label::new(None),
            empty: gtk::Label::new(None),
            list: gtk::ListBox::new(),
            delete: gtk::Button::new(),
            panels: RefCell::new(Vec::new()),
            rows_key: RefCell::new(String::new()),
            shown: Cell::new(false),
        }
    }
    fn bind(&self, w: &Rc<Workspace>) {
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        let body = gtk::Box::new(gtk::Orientation::Vertical, 18);
        margins(&body, 24);
        self.description.set_xalign(0.0);
        self.description.set_focusable(true);
        self.description.set_wrap(true);
        self.description.add_css_class("dim-label");
        body.append(&self.description);
        self.list.set_widget_name("managed-toolbars");
        self.list.set_selection_mode(gtk::SelectionMode::Single);
        self.list.add_css_class("boxed-list");
        self.list.set_valign(gtk::Align::Start);
        let contents = gtk::Box::new(gtk::Orientation::Vertical, 0);
        contents.append(&self.list);
        self.empty.add_css_class("dim-label");
        self.empty.set_vexpand(true);
        contents.append(&self.empty);
        body.append(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .child(&contents)
                .build(),
        );
        self.list.connect_row_selected(glib::clone!(
            #[weak]
            w,
            move |_, row| {
                let panel = row.and_then(|r| {
                    w.customization
                        .manager
                        .panels
                        .borrow()
                        .get(r.index() as usize)
                        .copied()
                });
                w.customize(CustomizationAction::SelectManagedToolbar { panel });
            }
        ));
        self.delete.set_widget_name("delete-managed-toolbar");
        self.delete.add_css_class("destructive-action");
        self.delete.set_halign(gtk::Align::End);
        self.delete.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                let action = w
                    .gpu
                    .borrow()
                    .as_ref()
                    .and_then(|g| g.session.toolbar_manager())
                    .and_then(|v| v.delete_action);
                if let Some(action) = action {
                    w.customize(action);
                }
            }
        ));
        body.append(&self.delete);
        view.set_content(Some(&body));
        self.dialog.set_child(Some(&view));
        self.dialog.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                if w.customization.manager.shown.replace(false) {
                    w.customize(CustomizationAction::CloseToolbarManager);
                }
            }
        ));
    }
    fn refresh(&self, w: &Rc<Workspace>, model: Option<ToolbarManagerView>) {
        let Some(model) = model else {
            if self.shown.replace(false) {
                self.dialog.close();
            }
            return;
        };
        self.dialog.set_title(model.title);
        self.description.set_label(model.description);
        self.empty.set_label(model.empty_label);
        self.empty.set_visible(model.toolbars.is_empty());
        self.list.set_visible(!model.toolbars.is_empty());
        self.delete.set_label(model.delete_label);
        self.delete.set_sensitive(model.delete_action.is_some());
        let key = serde_json::to_string(&model.toolbars).expect("serializable toolbars");
        if *self.rows_key.borrow() != key {
            *self.rows_key.borrow_mut() = key;
            while let Some(child) = self.list.first_child() {
                self.list.remove(&child);
            }
            *self.panels.borrow_mut() = model.toolbars.iter().map(|p| p.panel).collect();
            for toolbar in model.toolbars {
                let row = adw::ActionRow::new();
                row.set_use_markup(false);
                row.set_title(&toolbar.title);
                row.set_subtitle(&toolbar.subtitle);
                row.set_selectable(true);
                row.add_prefix(&gtk::Image::from_icon_name(&format!(
                    "layer-{}-symbolic",
                    toolbar.icon
                )));
                self.list.append(&row);
            }
        }
        let index = self
            .panels
            .borrow()
            .iter()
            .position(|p| Some(*p) == model.selected);
        self.list.select_row(
            index
                .and_then(|i| self.list.row_at_index(i as i32))
                .as_ref(),
        );
        if !self.shown.replace(true) {
            self.dialog.set_focus(Some(&self.description));
            self.dialog.present(Some(&w.window));
        }
    }
}

pub(super) struct Customization {
    pub toolbars: RefCell<Vec<ToolbarView>>,
    palette: gtk::CssProvider,
    palette_colors: Cell<Option<[[f32; 4]; 2]>>,
    context: gtk::PopoverMenu,
    popup: gtk::Popover,
    popup_control: Cell<Option<PanelControl>>,
    anchor: Cell<[f32; 2]>,
    picker: adw::Dialog,
    name: adw::EntryRow,
    search: gtk::SearchEntry,
    choices: gtk::ListBox,
    choices_key: RefCell<String>,
    error: gtk::Label,
    count: gtk::Label,
    confirm: gtk::Button,
    picker_shown: Cell<bool>,
    toolbar_dialog: adw::AlertDialog,
    toolbar_name: adw::EntryRow,
    toolbar_error: gtk::Label,
    toolbar_shown: Cell<bool>,
    manager: ToolbarManagerUi,
    updating: Cell<bool>,
    controls: RefCell<Vec<ControlWidget>>,
    expanded: Cell<Option<Panel>>,
    progress: Cell<f64>,
    transition_from: Cell<Option<PanelExpansion>>,
    closing: Cell<bool>,
    animation: RefCell<Option<adw::TimedAnimation>>,
    expanded_root: RefCell<Option<PanelColumns>>,
    visibility: RefCell<Vec<(PanelControl, gtk::CheckButton)>>,
    configuration_title: RefCell<Option<gtk::Label>>,
    toolbar_options: RefCell<Vec<ToolbarOptionWidget>>,
}

impl Customization {
    pub fn new() -> Self {
        let picker = adw::Dialog::builder()
            .content_width(480)
            .content_height(560)
            .title("Tools")
            .build();
        picker.set_widget_name("tool-picker");
        picker.add_css_class("layer-preferences");
        Self {
            toolbars: RefCell::new(Vec::new()),
            palette: gtk::CssProvider::new(),
            palette_colors: Cell::new(None),
            context: gtk::PopoverMenu::from_model(None::<&gtk::gio::Menu>),
            popup: gtk::Popover::new(),
            popup_control: Cell::new(None),
            anchor: Cell::new([320.0, 120.0]),
            picker,
            name: adw::EntryRow::new(),
            search: gtk::SearchEntry::new(),
            choices: gtk::ListBox::new(),
            choices_key: RefCell::new(String::new()),
            error: gtk::Label::new(None),
            count: gtk::Label::new(None),
            confirm: gtk::Button::new(),
            picker_shown: Cell::new(false),
            toolbar_dialog: adw::AlertDialog::new(None, None),
            toolbar_name: adw::EntryRow::new(),
            toolbar_error: gtk::Label::new(None),
            toolbar_shown: Cell::new(false),
            manager: ToolbarManagerUi::new(),
            updating: Cell::new(false),
            controls: RefCell::new(Vec::new()),
            expanded: Cell::new(None),
            progress: Cell::new(0.0),
            transition_from: Cell::new(None),
            closing: Cell::new(false),
            animation: RefCell::new(None),
            expanded_root: RefCell::new(None),
            visibility: RefCell::new(Vec::new()),
            configuration_title: RefCell::new(None),
            toolbar_options: RefCell::new(Vec::new()),
        }
    }

    pub fn bind(&self, w: &Rc<Workspace>) {
        self.manager.bind(w);
        self.toolbar_dialog.set_widget_name("toolbar-dialog");
        self.toolbar_dialog.add_response("cancel", "");
        self.toolbar_dialog.add_response("confirm", "");
        self.toolbar_dialog.set_close_response("cancel");
        self.toolbar_dialog.set_default_response(Some("confirm"));
        self.toolbar_dialog.connect_response(
            None,
            glib::clone!(
                #[weak]
                w,
                move |_, response| {
                    if w.customization.toolbar_shown.replace(false) {
                        w.customize(if response == "confirm" {
                            CustomizationAction::ConfirmToolbar
                        } else {
                            CustomizationAction::CancelToolbar
                        });
                    }
                }
            ),
        );
        let extra = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let group = adw::PreferencesGroup::new();
        self.toolbar_name.set_widget_name("edit-toolbar-name");
        self.toolbar_name.connect_changed(glib::clone!(
            #[weak]
            w,
            move |entry| w.customize(CustomizationAction::ToolbarName {
                name: entry.text().into()
            })
        ));
        group.add(&self.toolbar_name);
        self.toolbar_error.add_css_class("error");
        self.toolbar_error.set_wrap(true);
        extra.append(&group);
        extra.append(&self.toolbar_error);
        self.toolbar_dialog.set_extra_child(Some(&extra));
        for popover in [self.context.upcast_ref::<gtk::Popover>(), &self.popup] {
            popover.set_parent(&w.surface);
            popover.add_css_class("panel-context-menu");
            w.watch_popover(popover);
        }
        self.popup.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                if w.customization.popup_control.take().is_some() {
                    w.customize(CustomizationAction::CloseControl);
                }
            }
        ));
        for (panel, widget) in &w.panels {
            if panel.kind() != PanelKind::Tiles {
                w.install_context(widget, ContextTarget::Panel { panel: *panel });
            }
        }
        let view = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        header.set_show_start_title_buttons(false);
        header.set_show_end_title_buttons(false);
        let cancel = w.action_button(
            "Cancel",
            UiAction::Customize {
                action: CustomizationAction::CancelTools,
            },
        );
        header.pack_start(&cancel);
        self.confirm.add_css_class("suggested-action");
        self.confirm.set_widget_name("confirm-tools");
        self.confirm.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                w.customize(CustomizationAction::ConfirmTools);
            }
        ));
        header.pack_end(&self.confirm);
        view.add_top_bar(&header);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        margins(&body, 18);
        let name_group = adw::PreferencesGroup::new();
        name_group.add(&self.name);
        body.append(&name_group);
        self.name.set_widget_name("toolbar-name");
        self.name.connect_changed(glib::clone!(
            #[weak]
            w,
            move |entry| {
                w.customize(CustomizationAction::PickerName {
                    name: entry.text().into(),
                });
            }
        ));
        self.search.set_widget_name("tool-search");
        self.search.connect_search_changed(glib::clone!(
            #[weak]
            w,
            move |entry| {
                if w.customization.picker_shown.get() {
                    w.customize(CustomizationAction::PickerSearch {
                        query: entry.text().into(),
                    });
                }
            }
        ));
        body.append(&self.search);
        self.choices.set_selection_mode(gtk::SelectionMode::None);
        self.choices.add_css_class("boxed-list");
        let list = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&self.choices)
            .build();
        body.append(&list);
        self.count.set_xalign(0.0);
        self.count.add_css_class("dim-label");
        body.append(&self.count);
        self.error.set_xalign(0.0);
        self.error.set_wrap(true);
        self.error.add_css_class("error");
        body.append(&self.error);
        view.set_content(Some(&body));
        self.picker.set_child(Some(&view));
        self.picker.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                if w.customization.picker_shown.replace(false) {
                    w.customize(CustomizationAction::CancelTools);
                }
            }
        ));
    }

    pub fn dispose(&self) {
        self.collapse_panel();
        for popover in [self.context.upcast_ref::<gtk::Popover>(), &self.popup] {
            popover.unparent();
        }
    }

    // Popovers parented to a custom widget need the native layout hook, unlike
    // those owned by a GtkMenuButton. Keep them placed on window reallocations.
    pub fn present_popovers(&self) {
        for popover in [self.context.upcast_ref::<gtk::Popover>(), &self.popup] {
            if popover.is_visible() {
                popover.present();
            }
        }
    }

    pub fn track(&self, panel: Panel, control: PanelControl, widget: &impl IsA<gtk::Widget>) {
        self.controls.borrow_mut().push(ControlWidget {
            panel,
            control,
            widget: widget.clone().upcast(),
            value: None,
            configuration: false,
        });
    }

    pub fn collapse_panel(&self) {
        self.configuration_title.borrow_mut().take();
        self.toolbar_options.borrow_mut().clear();
        if let Some(animation) = self.animation.take() {
            animation.pause();
        }
        if let Some(root) = self.expanded_root.take() {
            root.set_configuration(None);
            root.remove_css_class("expanded-panel");
            root.imp().expansion.set(None);
        }
        self.expanded.set(None);
        self.progress.set(0.0);
        self.transition_from.set(None);
        self.closing.set(false);
        self.visibility.borrow_mut().clear();
        self.controls.borrow_mut().retain(|c| !c.configuration);
    }

    pub fn geometry(&self, w: &Workspace) -> Option<PanelExpansion> {
        let target = self.target_geometry(w, !self.closing.get())?;
        Some(self.transition_from.get().map_or(target, |from| {
            target.interpolate_from(from, self.progress.get() as f32)
        }))
    }

    fn target_geometry(&self, w: &Workspace, expanded: bool) -> Option<PanelExpansion> {
        let panel = self.expanded.get()?;
        let root = self.expanded_root.borrow().clone()?;
        let viewport = [
            w.surface.width().max(1) as f32,
            w.surface.height().max(1) as f32,
        ];
        let layout = w.surface.imp().layout.borrow();
        let sizing = layout.expanded_panel(viewport, panel, [0.0; 2], 1.0)?;
        let preview = w.panel_widget(panel);
        let preview_content = preview
            .downcast_ref::<gtk::ScrolledWindow>()
            .and_then(|s| s.child())
            .unwrap_or(preview);
        let preview_height = preview_content
            .measure(gtk::Orientation::Vertical, sizing.preview.width as i32)
            .1 as f32
            + TAB_BAR_HEIGHT;
        let config = root.imp().configuration.borrow().clone()?;
        let config_content = config
            .downcast_ref::<gtk::ScrolledWindow>()
            .and_then(|s| s.child())
            .unwrap_or(config);
        let config_height = config_content
            .measure(
                gtk::Orientation::Vertical,
                sizing.configuration.width as i32,
            )
            .1 as f32;
        layout.expanded_panel(
            viewport,
            panel,
            [preview_height, config_height],
            if expanded { 1.0 } else { 0.0 },
        )
    }

    pub fn placement(&self) -> Option<PanelExpansion> {
        self.expanded_root.borrow().as_ref()?.imp().expansion.get()
    }

    fn animate(&self, w: &Rc<Workspace>, opening: bool, from: Option<PanelExpansion>) {
        if let Some(animation) = self.animation.take() {
            animation.pause();
        }
        self.closing.set(!opening);
        self.transition_from.set(
            from.or_else(|| self.placement())
                .or_else(|| self.target_geometry(w, false)),
        );
        self.progress.set(0.0);
        let target = adw::CallbackAnimationTarget::new(glib::clone!(
            #[weak]
            w,
            move |value| {
                w.customization.progress.set(value);
                w.surface.queue_allocate();
            }
        ));
        let animation = adw::TimedAnimation::new(&w.surface, 0.0, 1.0, PANEL_EXPANSION_MS, target);
        animation.set_easing(adw::Easing::EaseOutCubic);
        animation.connect_done(glib::clone!(
            #[weak]
            w,
            move |_| {
                if w.customization.closing.get() {
                    w.customization.collapse_panel();
                } else {
                    w.customization.transition_from.set(None);
                }
                w.surface.queue_allocate();
            }
        ));
        *self.animation.borrow_mut() = Some(animation.clone());
        animation.play();
    }

    fn refresh_expansion(&self, w: &Rc<Workspace>, views: &[PanelView]) {
        let view = views.iter().find(|v| v.expanded);
        if view.is_none() {
            if self.expanded.get().is_some() && !self.closing.get() {
                self.animate(w, false, None);
            }
            return;
        }
        if self.expanded.get() != view.map(|v| v.id) {
            let same_group = self.expanded.get().zip(view).is_some_and(|(old, new)| {
                let layout = w.surface.imp().layout.borrow();
                layout.panel_group(old) == layout.panel_group(new.id)
            });
            let from = same_group.then(|| self.placement()).flatten();
            self.collapse_panel();
            if let Some(view) = view {
                let root = w
                    .groups
                    .borrow()
                    .iter()
                    .find(|g| g.panels.contains(&view.id))
                    .map(|g| (g.id, g.root.clone()));
                let Some((group, root)) = root else {
                    return;
                };
                let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
                margins(&body, 12);
                let title = gtk::Label::new(Some(&view.configuration_title));
                title.add_css_class("heading");
                title.set_hexpand(true);
                title.set_xalign(0.0);
                body.append(&title);
                *self.configuration_title.borrow_mut() = Some(title);
                let label = gtk::Label::new(Some(view.configuration_hint));
                label.set_wrap(true);
                label.set_xalign(0.0);
                label.add_css_class("dim-label");
                body.append(&label);
                for control in &view.controls {
                    let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
                    let check = gtk::CheckButton::with_label(control.label);
                    check.set_widget_name(&format!("panel-visible-{:?}", control.control));
                    check.set_active(control.visible_in_panel);
                    let panel = view.id;
                    let id = control.control;
                    check.connect_toggled(glib::clone!(
                        #[weak]
                        w,
                        move |check| {
                            w.customize(CustomizationAction::SetControlVisible {
                                panel,
                                control: id,
                                visible: check.is_active(),
                            });
                        }
                    ));
                    row.append(&check);
                    self.visibility.borrow_mut().push((id, check));
                    w.configuration_control(panel, id, &row);
                    body.append(&row);
                }
                for (s, section) in view.toolbar_options.iter().enumerate() {
                    let list = gtk::ListBox::new();
                    list.set_selection_mode(gtk::SelectionMode::None);
                    list.add_css_class("boxed-list");
                    let mut first_check = None;
                    for (i, item) in section.iter().enumerate() {
                        let Some(action) = item.action.clone() else {
                            continue;
                        };
                        let mut hint = None;
                        let widget: gtk::Widget = if let Some(active) = item.selected {
                            let check = gtk::CheckButton::with_label(&item.label);
                            check.set_group(first_check.as_ref());
                            if first_check.is_none() {
                                first_check = Some(check.clone());
                            }
                            check.set_active(active);
                            margins(&check, 9);
                            check.connect_toggled(glib::clone!(
                                #[weak]
                                w,
                                move |check| {
                                    if check.is_active() && !w.customization.updating.get() {
                                        w.dispatch(action.clone());
                                    }
                                }
                            ));
                            check.upcast()
                        } else {
                            let button = w.action_button(&item.label, action);
                            button.add_css_class("flat");
                            if !item.hint.is_empty() {
                                let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                                let title = gtk::Label::builder()
                                    .label(&item.label)
                                    .xalign(0.0)
                                    .hexpand(true)
                                    .build();
                                let value = gtk::Label::new(Some(&item.hint));
                                value.add_css_class("dim-label");
                                value.set_ellipsize(gtk::pango::EllipsizeMode::End);
                                value.set_max_width_chars(20);
                                row.append(&title);
                                row.append(&value);
                                button.set_child(Some(&row));
                                hint = Some(value);
                            }
                            button.upcast()
                        };
                        widget.set_sensitive(item.enabled);
                        list.append(&widget);
                        self.toolbar_options.borrow_mut().push(ToolbarOptionWidget {
                            section: s,
                            item: i,
                            widget,
                            hint,
                        });
                    }
                    body.append(&list);
                }
                let scroll = gtk::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk::PolicyType::Never)
                    .child(&body)
                    .build();
                scroll.add_css_class("panel-configuration");
                root.set_configuration(Some(scroll.upcast_ref()));
                root.add_css_class("expanded-panel");
                *self.expanded_root.borrow_mut() = Some(root);
                self.expanded.set(Some(view.id));
                w.surface.raise_group(group);
                self.animate(w, true, from);
            }
        } else if self.closing.get() {
            self.animate(w, true, None);
        }
        if let Some(view) = view {
            if let Some(title) = self.configuration_title.borrow().as_ref() {
                title.set_label(&view.configuration_title);
            }
            for ToolbarOptionWidget {
                section,
                item: index,
                widget,
                hint,
            } in self.toolbar_options.borrow().iter()
            {
                if let Some(item) = view
                    .toolbar_options
                    .get(*section)
                    .and_then(|s| s.get(*index))
                {
                    widget.set_sensitive(item.enabled);
                    if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
                        check.set_active(item.selected.unwrap_or(false));
                    }
                    if let Some(hint) = hint {
                        hint.set_label(&item.hint);
                    }
                }
            }
            for (control, check) in self.visibility.borrow().iter() {
                if let Some(control) = view.controls.iter().find(|c| c.control == *control) {
                    check.set_active(control.visible_in_panel);
                }
            }
        }
    }

    pub fn reconcile_toolbars(&self, w: &Rc<Workspace>, layout: &DockLayout) {
        self.toolbars
            .borrow_mut()
            .retain(|t| layout.panels.iter().any(|p| p.id == t.id));
        for config in layout
            .panels
            .iter()
            .filter(|p| p.id.kind() == PanelKind::Tiles)
        {
            let mut toolbars = self.toolbars.borrow_mut();
            let index = match toolbars.iter().position(|t| t.id == config.id) {
                Some(index) => index,
                None => {
                    let strip = if config.id == Panel::Toolbar {
                        w.toolbar.clone()
                    } else {
                        TileStrip::new()
                    };
                    strip.add_css_class("toolbar-controls");
                    let grip = tiles::grip();
                    w.install_panel_drag(&grip, DockItem::Panel { panel: config.id });
                    strip.set_grip(&grip);
                    w.install_context(&strip, ContextTarget::Ribbon { panel: config.id });
                    toolbars.push(ToolbarView {
                        id: config.id,
                        strip,
                        tiles: Vec::new(),
                        buttons: Vec::new(),
                        style: TileStyle::Small,
                    });
                    toolbars.len() - 1
                }
            };
            let toolbar = &mut toolbars[index];
            if toolbar.tiles == config.tiles() && toolbar.style == config.tile_style {
                continue;
            }
            toolbar.strip.clear();
            toolbar.strip.set_style(config.tile_style);
            toolbar.style = config.tile_style;
            toolbar.buttons.clear();
            for tile in config.tiles() {
                let panel = config.id;
                let id = tile.id;
                let button = tile_button(w, config, tile);
                // The wrapper stays targetable even when the command button is
                // disabled, so an unavailable command can still be moved/removed.
                let tile_root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                button.add_css_class("tile-button");
                button.set_hexpand(true);
                button.set_vexpand(true);
                tile_root.append(&button);
                w.install_panel_drag(&tile_root, DockItem::Tile { panel, tile: id });
                w.install_context(&tile_root, ContextTarget::Tile { panel, tile: id });
                toolbar.strip.append(&tile_root);
                toolbar.buttons.push(button);
            }
            toolbar.tiles = config.tiles().to_vec();
            toolbar.strip.set_tiles(config.tiles());
        }
    }

    pub fn drawer_button(&self, anchor: TileAnchor) -> Option<gtk::Button> {
        self.toolbars
            .borrow()
            .iter()
            .find(|bar| bar.id == anchor.panel)
            .and_then(|bar| {
                bar.tiles
                    .iter()
                    .zip(&bar.buttons)
                    .find(|(tile, _)| tile.id == anchor.tile)
                    .map(|(_, button)| button.clone())
            })
    }
    pub fn mark_drawer_origin(&self, origin: Option<(TileAnchor, Edge)>) {
        for bar in self.toolbars.borrow().iter() {
            if origin.is_some_and(|(anchor, _)| anchor.panel == bar.id) {
                bar.strip.add_css_class("drawer-source");
            } else {
                bar.strip.remove_css_class("drawer-source");
            }
            for (tile, button) in bar.tiles.iter().zip(&bar.buttons) {
                drawer_origin(
                    button,
                    origin
                        .filter(|(a, _)| a.panel == bar.id && a.tile == tile.id)
                        .map(|(_, d)| d),
                );
            }
        }
    }

    fn refresh_color_palette(&self, colors: &layer_ui::ColorState) -> bool {
        let next = [colors.foreground, colors.background];
        if self.palette_colors.replace(Some(next)) == Some(next) {
            return false;
        }
        // All toolbar projections in this window share these colors. Reloading
        // unchanged CSS invalidates GTK styling during unrelated tool updates.
        let rgba = |[r, g, b, a]: [f32; 4]| gdk::RGBA::new(r, g, b, a);
        self.palette.load_from_string(&format!(
            ".brush-color {{ -gtk-icon-palette: success {}, warning {}; }}",
            rgba(colors.foreground),
            rgba(colors.background)
        ));
        true
    }

    pub fn refresh(&self, w: &Rc<Workspace>) {
        self.updating.set(true);
        let Some((views, picker, control, prompt, manager)) = w.gpu.borrow().as_ref().map(|g| {
            self.refresh_color_palette(&g.session.state().colors);
            (
                g.session
                    .state()
                    .workspace
                    .layout
                    .panels
                    .iter()
                    .filter_map(|p| g.session.panel_view(p.id).ok())
                    .collect::<Vec<_>>(),
                g.session.tool_picker(),
                g.session.state().customization.control,
                g.session.toolbar_prompt(),
                g.session.toolbar_manager(),
            )
        }) else {
            self.updating.set(false);
            return;
        };
        for toolbar in self.toolbars.borrow().iter() {
            if let Some(view) = views.iter().find(|v| v.id == toolbar.id) {
                for (button, tile) in toolbar.buttons.iter().zip(&view.tiles) {
                    selected(button, tile.choice.selected);
                    button.set_sensitive(tile.enabled);
                    button.set_tooltip_text(Some(&tile.tooltip));
                }
            }
        }
        self.refresh_expansion(w, &views);
        let brush = w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .state()
            .brush
            .clone();
        for field in self.controls.borrow().iter() {
            let shown = views
                .iter()
                .find(|v| v.id == field.panel)
                .and_then(|v| v.controls.iter().find(|c| c.control == field.control))
                .is_some_and(|c| c.visible_in_panel);
            field.widget.set_visible(field.configuration || shown);
            match &field.value {
                Some(FieldValue::Size(input)) => input.set_value(brush.diameter as f64),
                Some(FieldValue::Opacity(input)) => input.set_value(brush.opacity as f64),
                Some(FieldValue::Color(input)) => {
                    let [r, g, b, a] = brush.color;
                    input.set_rgba(&gdk::RGBA::new(r, g, b, a));
                }
                None => (),
                Some(FieldValue::Brush(input)) => input.set_selected(
                    brush_categories()
                        .flat_map(|c| c.brushes)
                        .position(|b| b.id == brush.preset)
                        .unwrap_or(0) as u32,
                ),
                Some(FieldValue::Layer(input)) => {
                    let gpu = w.gpu.borrow();
                    let state = gpu.as_ref().unwrap().session.state();
                    let names: Vec<_> = state.layers.iter().map(|l| l.label.as_str()).collect();
                    input.set_model(Some(&gtk::StringList::new(&names)));
                    input.set_selected(
                        state.layers.iter().position(|l| l.editing).unwrap_or(0) as u32
                    );
                }
                Some(FieldValue::LayerOpacity(input)) => {
                    input.set_value(w.layer_panel.opacity.value())
                }
                Some(FieldValue::Commands(buttons)) => {
                    let gpu = w.gpu.borrow();
                    for (id, button) in buttons {
                        button.set_sensitive(gpu.as_ref().unwrap().session.command(*id).enabled);
                    }
                }
            }
        }
        if self.popup_control.get() != control {
            self.popup_control.set(control);
            self.popup.set_child(None::<&gtk::Widget>);
            if let Some(control) = control {
                let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
                margins(&body, 12);
                body.append(&gtk::Label::new(Some(control.label())));
                match control {
                    PanelControl::BrushColor => body.append(&w.color),
                    PanelControl::BrushOpacity => body.append(&w.opacity),
                    _ => unreachable!("core validates popup controls"),
                }
                self.popup.set_child(Some(&body));
                let [x, y] = self.anchor.get();
                self.popup
                    .set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
                self.popup.popup();
            } else {
                self.popup.popdown();
            }
        }
        if let Some(view) = picker {
            self.picker.set_title(view.title);
            self.confirm.set_label(view.confirm_label);
            self.confirm.set_sensitive(view.can_confirm);
            self.name.set_title(view.name_label);
            self.name.parent().unwrap().set_visible(view.name.is_some());
            if let Some(name) = &view.name
                && self.name.text() != *name
            {
                self.name.set_text(name);
            }
            self.search.set_placeholder_text(Some(view.search_hint));
            if self.search.text() != view.query {
                self.search.set_text(&view.query);
            }
            self.error.set_text(view.error.as_deref().unwrap_or(""));
            self.error.set_visible(view.error.is_some());
            self.count
                .set_text(&format!("{} selected", view.selected_count));
            let key = serde_json::to_string(&view.choices).expect("serializable tools");
            if *self.choices_key.borrow() != key {
                *self.choices_key.borrow_mut() = key;
                while let Some(child) = self.choices.first_child() {
                    self.choices.remove(&child);
                }
                for choice in view.choices {
                    let row = adw::ActionRow::new();
                    row.set_use_markup(false);
                    row.set_title(&choice.label);
                    row.set_subtitle(&choice.description);
                    row.add_prefix(&gtk::Image::from_icon_name(&format!(
                        "layer-{}-symbolic",
                        choice.icon
                    )));
                    let check = gtk::CheckButton::new();
                    check.set_active(choice.selected);
                    check.set_valign(gtk::Align::Center);
                    check.connect_toggled(glib::clone!(
                        #[weak]
                        w,
                        move |check| {
                            w.customize(CustomizationAction::PickerSelect {
                                control: choice.control,
                                selected: check.is_active(),
                            });
                        }
                    ));
                    row.add_suffix(&check);
                    row.set_activatable_widget(Some(&check));
                    self.choices.append(&row);
                }
            }
            if !self.picker_shown.replace(true) {
                self.picker.present(Some(&w.window));
                if view.name.is_some() {
                    self.name.grab_focus();
                } else {
                    self.search.grab_focus();
                }
            }
        } else if self.picker_shown.replace(false) {
            self.picker.close();
            self.choices_key.borrow_mut().clear();
        }
        self.manager.refresh(w, manager);
        if let Some(view) = prompt {
            self.toolbar_dialog.set_heading(Some(view.title));
            self.toolbar_dialog.set_body(&view.message);
            self.toolbar_dialog
                .extra_child()
                .unwrap()
                .set_visible(view.name.is_some() || view.error.is_some());
            self.toolbar_dialog
                .set_response_label("cancel", view.cancel_label);
            self.toolbar_dialog
                .set_response_label("confirm", view.confirm_label);
            self.toolbar_dialog
                .set_response_enabled("confirm", view.can_confirm);
            self.toolbar_dialog.set_response_appearance(
                "confirm",
                if view.destructive {
                    adw::ResponseAppearance::Destructive
                } else {
                    adw::ResponseAppearance::Suggested
                },
            );
            self.toolbar_name.set_title(view.name_label);
            self.toolbar_name.set_visible(view.name.is_some());
            if let Some(name) = view.name
                && self.toolbar_name.text() != name
            {
                self.toolbar_name.set_text(&name);
            }
            self.toolbar_error
                .set_label(view.error.as_deref().unwrap_or(""));
            self.toolbar_error.set_visible(view.error.is_some());
            if !self.toolbar_shown.replace(true) {
                self.toolbar_dialog.present(Some(&w.window));
                if !view.destructive {
                    self.toolbar_name.grab_focus();
                }
            }
        } else if self.toolbar_shown.replace(false) {
            self.toolbar_dialog.close();
        }
        self.updating.set(false);
        self.present_popovers();
    }
}

impl Workspace {
    pub(super) fn append_panel_fields(self: &Rc<Self>, panel: Panel, body: &gtk::Box) {
        for &control in PanelControl::available(panel) {
            if self
                .customization
                .controls
                .borrow()
                .iter()
                .any(|c| c.panel == panel && c.control == control)
            {
                continue;
            }
            let group = gtk::Box::new(gtk::Orientation::Vertical, 6);
            group.set_widget_name(&format!("panel-field-{panel:?}-{control:?}"));
            let label = gtk::Label::new(Some(control.label()));
            label.set_xalign(0.0);
            if !matches!(
                control,
                PanelControl::BrushSize | PanelControl::BrushOpacity | PanelControl::LayerOpacity
            ) {
                group.append(&label);
            }
            let value = self.panel_field(control, &group);
            group.set_visible(false);
            body.append(&group);
            self.customization
                .controls
                .borrow_mut()
                .push(ControlWidget {
                    panel,
                    control,
                    widget: group.upcast(),
                    value,
                    configuration: false,
                });
        }
    }

    fn configuration_control(
        self: &Rc<Self>,
        panel: Panel,
        control: PanelControl,
        body: &gtk::Box,
    ) {
        let group = gtk::Box::new(gtk::Orientation::Vertical, 6);
        group.set_widget_name(&format!("configure-{panel:?}-{control:?}"));
        let value = self.panel_field(control, &group);
        body.append(&group);
        self.customization
            .controls
            .borrow_mut()
            .push(ControlWidget {
                panel,
                control,
                widget: group.upcast(),
                value,
                configuration: true,
            });
    }

    fn panel_field(self: &Rc<Self>, control: PanelControl, group: &gtk::Box) -> Option<FieldValue> {
        Some(match control {
            PanelControl::Adjustments
            | PanelControl::Properties
            | PanelControl::Stats
            | PanelControl::Navigator
            | PanelControl::ToolSettings
            | PanelControl::ColorWheel => {
                // These schema-driven surfaces already occupy their panel body.
                return None;
            }
            PanelControl::BrushSize => {
                let input = crate::number_control::NumberControl::new(
                    NumericControl::brush_size(),
                    control.label(),
                    "",
                );
                input.connect_value_changed(glib::clone!(
                    #[weak(rename_to = w)]
                    self,
                    move |input| w.dispatch(UiAction::SetBrushSize {
                        value: input.value() as f32
                    })
                ));
                group.append(&input);
                FieldValue::Size(input)
            }
            PanelControl::BrushOpacity => {
                let input = crate::number_control::NumberControl::new(
                    NumericControl::percent(),
                    control.label(),
                    "",
                );
                input.connect_value_changed(glib::clone!(
                    #[weak(rename_to = w)]
                    self,
                    move |input| w.dispatch(UiAction::SetBrushOpacity {
                        value: input.value() as f32
                    })
                ));
                group.append(&input);
                FieldValue::Opacity(input)
            }
            PanelControl::BrushColor => {
                let input = gtk::ColorDialogButton::new(Some(
                    gtk::ColorDialog::builder().with_alpha(false).build(),
                ));
                input.connect_rgba_notify(glib::clone!(
                    #[weak(rename_to = w)]
                    self,
                    move |input| {
                        let c = input.rgba();
                        w.dispatch(UiAction::SetColor {
                            rgba: [c.red(), c.green(), c.blue(), c.alpha()],
                        });
                    }
                ));
                group.append(&input);
                FieldValue::Color(input)
            }
            PanelControl::Brushes => {
                let brushes: Vec<_> = brush_categories().flat_map(|c| c.brushes).collect();
                let labels: Vec<_> = brushes.iter().map(|b| b.label).collect();
                let input = gtk::DropDown::from_strings(&labels);
                input.connect_selected_notify(glib::clone!(
                    #[weak(rename_to = w)]
                    self,
                    move |input| {
                        if let Some(choice) = brushes.get(input.selected() as usize) {
                            w.dispatch(UiAction::SelectBrush { id: choice.id });
                        }
                    }
                ));
                group.append(&input);
                FieldValue::Brush(input)
            }
            PanelControl::Layers => {
                let input = gtk::DropDown::from_strings(&[]);
                input.connect_selected_notify(glib::clone!(
                    #[weak(rename_to = w)]
                    self,
                    move |input| {
                        let id = w.gpu.borrow().as_ref().and_then(|g| {
                            g.session
                                .state()
                                .layers
                                .get(input.selected() as usize)
                                .map(|l| l.id)
                        });
                        if let Some(id) = id {
                            w.dispatch(UiAction::SelectLayer { id });
                        }
                    }
                ));
                group.append(&input);
                FieldValue::Layer(input)
            }
            PanelControl::LayerOpacity => {
                let input = crate::number_control::NumberControl::new(
                    NumericControl::percent(),
                    control.label(),
                    "",
                );
                input.connect_value_changed(glib::clone!(
                    #[weak(rename_to = w)]
                    self,
                    move |input| {
                        w.dispatch(UiAction::SetLayerOpacity {
                            id: None,
                            opacity: input.value() as f32,
                        });
                    }
                ));
                group.append(&input);
                FieldValue::LayerOpacity(input)
            }
            PanelControl::SizePresets | PanelControl::LayerActions => {
                let grid = gtk::FlowBox::builder()
                    .selection_mode(gtk::SelectionMode::None)
                    .min_children_per_line(2)
                    .max_children_per_line(6)
                    .column_spacing(2)
                    .row_spacing(2)
                    .build();
                if control == PanelControl::SizePresets {
                    for &value in BRUSH_SIZES {
                        grid.insert(
                            &self.action_button(
                                &value.to_string(),
                                UiAction::SetBrushSize { value },
                            ),
                            -1,
                        );
                    }
                } else {
                    let mut buttons = Vec::new();
                    for command in CommandId::LAYERS {
                        let button =
                            self.action_button(command.label(), UiAction::Invoke { command });
                        grid.insert(&button, -1);
                        buttons.push((command, button));
                    }
                    group.append(&grid);
                    return Some(FieldValue::Commands(buttons));
                }
                group.append(&grid);
                return None;
            }
        })
    }
    fn customize(self: &Rc<Self>, action: CustomizationAction) {
        if !self.customization.updating.get() {
            self.dispatch(UiAction::Customize { action });
        }
    }

    pub(super) fn panel_widget(&self, panel: Panel) -> gtk::Widget {
        self.panels
            .iter()
            .find(|(p, _)| *p == panel)
            .map(|(_, w)| w.clone())
            .or_else(|| {
                self.customization
                    .toolbars
                    .borrow()
                    .iter()
                    .find(|t| t.id == panel)
                    .map(|t| t.strip.clone().upcast())
            })
            .expect("validated panel has a native view")
    }

    pub(super) fn install_context(
        self: &Rc<Self>,
        widget: &impl IsA<gtk::Widget>,
        target: ContextTarget,
    ) {
        widget.add_css_class("customizable-target");
        let click = gtk::GestureClick::new();
        click.set_name(Some("workspace-context-click"));
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |gesture, _, x, y| {
                let Some(widget) = gesture.widget() else {
                    return;
                };
                if !owns_context(&widget, x, y) {
                    return;
                }
                if gesture.current_button() == 3 {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    w.show_context(&widget, target, x, y);
                }
            }
        ));
        widget.add_controller(click);
        let hold = gtk::GestureLongPress::new();
        hold.set_name(Some("workspace-context-hold"));
        hold.set_touch_only(true);
        hold.set_propagation_phase(gtk::PropagationPhase::Capture);
        hold.connect_pressed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |gesture, x, y| {
                let Some(widget) = gesture.widget() else {
                    return;
                };
                if owns_context(&widget, x, y) {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    w.show_context(&widget, target, x, y);
                }
            }
        ));
        widget.add_controller(hold);
    }

    fn show_context(self: &Rc<Self>, widget: &gtk::Widget, target: ContextTarget, x: f64, y: f64) {
        let Some(menu) = self
            .gpu
            .borrow()
            .as_ref()
            .and_then(|g| g.session.context_menu(target).ok())
        else {
            return;
        };
        let Some(point) = widget.compute_point(
            &self.surface,
            &gtk::graphene::Point::new(x as f32, y as f32),
        ) else {
            return;
        };
        self.customization.anchor.set([point.x(), point.y()]);
        let popover = &self.customization.context;
        self.populate_workspace_menu(popover, menu);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(
            point.x() as i32,
            point.y() as i32,
            1,
            1,
        )));
        popover.popup();
        popover.present();
    }

    pub(crate) fn populate_workspace_menu(
        self: &Rc<Self>,
        popover: &gtk::PopoverMenu,
        menu: layer_ui::ContextMenu,
    ) {
        fn model(
            w: &Rc<Workspace>,
            popup: &gtk::PopoverMenu,
            sections: Vec<Vec<layer_ui::ContextMenuItem>>,
            prefix: &str,
            actions: &gtk::gio::SimpleActionGroup,
            children: &mut Vec<(String, gtk::Widget)>,
        ) -> gtk::gio::Menu {
            let root = gtk::gio::Menu::new();
            for (s, items) in sections.into_iter().enumerate() {
                if items.is_empty() {
                    continue;
                }
                let section = gtk::gio::Menu::new();
                for (i, item) in items.into_iter().enumerate() {
                    let id = format!("{prefix}-{s}-{i}");
                    if item.action.is_none() {
                        let submenu = model(w, popup, item.sections, &id, actions, children);
                        section.append_submenu(Some(&item.label), &submenu);
                        continue;
                    }
                    let model = gtk::gio::MenuItem::new(Some(&item.label), None);
                    let action = if let Some(selected) = item.selected {
                        let action =
                            gtk::gio::SimpleAction::new_stateful(&id, None, &selected.to_variant());
                        model.set_detailed_action(&format!("context.{id}"));
                        action
                    } else {
                        model.set_detailed_action(&format!("context.{id}"));
                        gtk::gio::SimpleAction::new(&id, None)
                    };
                    action.set_enabled(item.enabled);
                    if !item.hint.is_empty() {
                        let row = gtk::Box::new(gtk::Orientation::Horizontal, 24);
                        let title = gtk::Label::builder()
                            .label(&item.label)
                            .xalign(0.0)
                            .hexpand(true)
                            .build();
                        let hint = gtk::Label::new(Some(&item.hint));
                        hint.add_css_class("dim-label");
                        hint.set_ellipsize(gtk::pango::EllipsizeMode::End);
                        hint.set_max_width_chars(28);
                        row.append(&title);
                        row.append(&hint);
                        let button: gtk::Widget = if item.selected.is_some() {
                            let button = gtk::CheckButton::new();
                            button.set_action_name(Some(&format!("context.{id}")));
                            button.set_child(Some(&row));
                            button.upcast()
                        } else {
                            let button = gtk::Button::new();
                            button.add_css_class("flat");
                            button.set_action_name(Some(&format!("context.{id}")));
                            button.set_child(Some(&row));
                            button.upcast()
                        };
                        button.add_css_class("workspace-menu-item");
                        model.set_attribute_value("custom", Some(&id.to_variant()));
                        children.push((id.clone(), button));
                    }
                    let dispatch = item.action.unwrap();
                    action.connect_activate(glib::clone!(
                        #[weak]
                        w,
                        #[weak]
                        popup,
                        move |_, _| {
                            popup.popdown();
                            w.dispatch(dispatch.clone());
                        }
                    ));
                    actions.add_action(&action);
                    section.append_item(&model);
                }
                root.append_section(None, &section);
            }
            root
        }
        let actions = gtk::gio::SimpleActionGroup::new();
        let mut children = Vec::new();
        let root = model(
            self,
            popover,
            menu.sections,
            "item",
            &actions,
            &mut children,
        );
        popover.insert_action_group("context", Some(&actions));
        popover.set_menu_model(Some(&root));
        for (id, child) in children {
            popover.add_child(&child, &id);
        }
    }
}

// Capture-phase recognition must yield to a more specific target or native
// text editor. This also prevents the enclosing panel from stealing a tile hold.
fn owns_context(widget: &gtk::Widget, x: f64, y: f64) -> bool {
    let mut child = widget.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(current) = child {
        if &current == widget {
            return true;
        }
        if current.has_css_class("customizable-target")
            || current.is::<gtk::Editable>()
            || current.is::<gtk::TextView>()
        {
            return false;
        }
        child = current.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "native GTK palette: requires a Wayland display"]
    fn toolbar_palette_changes_only_with_paint_colors() {
        adw::init().unwrap();
        let view = Customization::new();
        let mut colors = layer_ui::ColorState::default();
        assert!(view.refresh_color_palette(&colors));
        assert!(!view.refresh_color_palette(&colors));
        let initial_css = view.palette.to_str();
        colors.slot = layer_ui::ColorSlot::Background;
        colors.space = layer_ui::ColorSpace::Hls;
        assert!(!view.refresh_color_palette(&colors));
        assert_eq!(view.palette.to_str(), initial_css);
        colors.foreground = [0.8, 0.2, 0.4, 1.];
        assert!(view.refresh_color_palette(&colors));
        assert_ne!(view.palette.to_str(), initial_css);
        assert!(!view.refresh_color_palette(&colors));
        colors.background = [0.1, 0.3, 0.9, 0.5];
        assert!(view.refresh_color_palette(&colors));
        assert!(!view.refresh_color_palette(&colors));
        // Each window owns its own provider, including first initialization
        // when every channel happens to be zero.
        let other = Customization::new();
        colors.foreground = [0.; 4];
        colors.background = [0.; 4];
        assert!(other.refresh_color_palette(&colors));
        assert_ne!(other.palette.to_str(), view.palette.to_str());
    }
}
