//! Native presentation of the shared workspace customization models.
//! GTK owns gestures/widgets; catalogs, selection, validation and edits are Rust UI policy.
use super::*;

pub(super) struct ToolbarView {
    pub id: Panel,
    pub strip: TileStrip,
    tiles: Vec<ToolbarTile>,
    buttons: Vec<gtk::Button>,
    palette: gtk::CssProvider,
}

enum FieldValue {
    Size(gtk::SpinButton),
    Opacity(gtk::Scale),
    Color(gtk::ColorDialogButton),
}
struct ControlWidget {
    panel: Panel,
    control: PanelControl,
    widget: gtk::Widget,
    value: Option<FieldValue>,
}
struct Expanded {
    panel: Panel,
    stack: glib::WeakRef<gtk::Stack>,
    widget: gtk::Widget,
    ribbon: Option<(Axis, bool)>,
}

pub(super) struct Customization {
    pub toolbars: RefCell<Vec<ToolbarView>>,
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
    updating: Cell<bool>,
    controls: RefCell<Vec<ControlWidget>>,
    inspector: gtk::Popover,
    expanded: RefCell<Option<Expanded>>,
    inspector_body: gtk::Box,
    visibility: RefCell<Vec<(PanelControl, gtk::CheckButton)>>,
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
            updating: Cell::new(false),
            controls: RefCell::new(Vec::new()),
            inspector: gtk::Popover::new(),
            expanded: RefCell::new(None),
            inspector_body: gtk::Box::new(gtk::Orientation::Vertical, 8),
            visibility: RefCell::new(Vec::new()),
        }
    }

    pub fn bind(&self, w: &Rc<Workspace>) {
        for popover in [
            self.context.upcast_ref::<gtk::Popover>(),
            &self.popup,
            &self.inspector,
        ] {
            popover.set_parent(&w.surface);
            popover.add_css_class("panel-context-menu");
            w.watch_popover(popover);
        }
        self.inspector.add_css_class("expanded-panel");
        self.inspector_body.add_css_class("dock-panel");
        self.inspector.set_child(Some(&self.inspector_body));
        self.inspector.connect_closed(glib::clone!(
            #[weak]
            w,
            move |_| {
                if w.customization.expanded.borrow().is_some() {
                    w.customization.collapse_panel();
                    w.customize(CustomizationAction::CloseExpanded);
                }
            }
        ));
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
                w.install_context(widget, ContextTarget::Panel { panel: *panel }, false);
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
        for popover in [
            self.context.upcast_ref::<gtk::Popover>(),
            &self.popup,
            &self.inspector,
        ] {
            popover.unparent();
        }
    }

    // Popovers parented to a custom widget need the native layout hook, unlike
    // those owned by a GtkMenuButton. Keep them placed on window reallocations.
    pub fn present_popovers(&self) {
        for popover in [
            self.context.upcast_ref::<gtk::Popover>(),
            &self.popup,
            &self.inspector,
        ] {
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
        });
    }

    pub fn collapse_panel(&self) {
        let expanded = self.expanded.borrow_mut().take();
        if let Some(expanded) = expanded {
            self.inspector_body.remove(&expanded.widget);
            expanded.widget.set_size_request(-1, -1);
            if let Some(stack) = expanded.stack.upgrade() {
                stack.add_named(&expanded.widget, Some(&format!("{:?}", expanded.panel)));
            }
            if let Some((axis, standalone)) = expanded.ribbon {
                expanded
                    .widget
                    .downcast_ref::<TileStrip>()
                    .unwrap()
                    .configure(axis, standalone);
            }
            self.inspector.popdown();
        }
    }

    fn refresh_inspector(&self, w: &Rc<Workspace>, views: &[PanelView]) {
        let view = views.iter().find(|v| v.expanded);
        if self.expanded.borrow().as_ref().map(|e| e.panel) != view.map(|v| v.id) {
            self.collapse_panel();
            if let Some(view) = view {
                while let Some(child) = self.inspector_body.first_child() {
                    self.inspector_body.remove(&child);
                }
                self.visibility.borrow_mut().clear();
                let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                margins(&header, 8);
                let title = gtk::Label::new(Some(&view.title));
                title.add_css_class("heading");
                title.set_hexpand(true);
                title.set_xalign(0.0);
                header.append(&title);
                let close = w.action_button(
                    "",
                    UiAction::Customize {
                        action: CustomizationAction::CloseExpanded,
                    },
                );
                close.set_icon_name("window-close-symbolic");
                close.set_tooltip_text(Some("Close"));
                header.append(&close);
                self.inspector_body.append(&header);
                if !view.controls.is_empty() {
                    let visibility = gtk::Box::new(gtk::Orientation::Vertical, 6);
                    margins(&visibility, 8);
                    let label = gtk::Label::new(Some("Show in panel"));
                    label.set_xalign(0.0);
                    label.add_css_class("dim-label");
                    visibility.append(&label);
                    let grid = gtk::FlowBox::builder()
                        .selection_mode(gtk::SelectionMode::None)
                        .min_children_per_line(2)
                        .max_children_per_line(2)
                        .column_spacing(12)
                        .row_spacing(6)
                        .build();
                    for control in &view.controls {
                        let check = gtk::CheckButton::with_label(control.label);
                        check.set_widget_name(&format!("panel-visible-{:?}", control.control));
                        check.set_active(control.visible_in_panel);
                        let panel = view.id;
                        let control = control.control;
                        check.connect_toggled(glib::clone!(
                            #[weak]
                            w,
                            move |check| {
                                w.customize(CustomizationAction::SetControlVisible {
                                    panel,
                                    control,
                                    visible: check.is_active(),
                                });
                            }
                        ));
                        grid.insert(&check, -1);
                        self.visibility.borrow_mut().push((control, check));
                    }
                    visibility.append(&grid);
                    self.inspector_body.append(&visibility);
                }
                let widget = w.panel_widget(view.id);
                if let Some(stack) = widget.parent().and_downcast::<gtk::Stack>() {
                    stack.remove(&widget);
                    widget.set_size_request(360, 320);
                    self.inspector_body.append(&widget);
                    if let Ok(strip) = widget.clone().downcast::<TileStrip>() {
                        strip.configure(Axis::Horizontal, false);
                    }
                    *self.expanded.borrow_mut() = Some(Expanded {
                        panel: view.id,
                        stack: stack.downgrade(),
                        widget,
                        ribbon: (view.id.kind() == PanelKind::Tiles).then(|| {
                            let resolved = w.resolved();
                            let group = resolved
                                .groups
                                .iter()
                                .find(|g| g.panels.contains(&view.id))
                                .unwrap();
                            (group.axis, !group.tabs_visible)
                        }),
                    });
                    let [x, y] = self.anchor.get();
                    self.inspector
                        .set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
                    self.inspector.popup();
                }
            }
        }
        if let Some(view) = view {
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
                    w.install_context(&strip, ContextTarget::Ribbon { panel: config.id }, false);
                    toolbars.push(ToolbarView {
                        id: config.id,
                        strip,
                        tiles: Vec::new(),
                        buttons: Vec::new(),
                        palette: gtk::CssProvider::new(),
                    });
                    toolbars.len() - 1
                }
            };
            let toolbar = &mut toolbars[index];
            if toolbar.tiles == config.tiles() {
                continue;
            }
            toolbar.strip.clear();
            toolbar.buttons.clear();
            for tile in config.tiles() {
                let choice = tool_choice(tile.control);
                let panel = config.id;
                let id = tile.id;
                let button = gtk::Button::builder()
                    .icon_name(format!("layer-{}-symbolic", choice.icon))
                    .tooltip_text(&choice.label)
                    .build();
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
                    button
                        .style_context()
                        .add_provider(&toolbar.palette, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
                }
                // The wrapper stays targetable even when the command button is
                // disabled, so an unavailable command can still be moved/removed.
                let tile_root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                button.add_css_class("tile-button");
                button.set_hexpand(true);
                button.set_vexpand(true);
                tile_root.append(&button);
                w.install_panel_drag(&tile_root, DockItem::Tile { panel, tile: id });
                w.install_context(&tile_root, ContextTarget::Tile { panel, tile: id }, false);
                toolbar.strip.append(&tile_root);
                toolbar.buttons.push(button);
            }
            toolbar.tiles = config.tiles().to_vec();
        }
    }

    pub fn refresh(&self, w: &Rc<Workspace>) {
        self.updating.set(true);
        let Some((views, picker, control)) = w.gpu.borrow().as_ref().map(|g| {
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
                }
            }
            toolbar.palette.load_from_string(&format!(
                ".brush-color {{ -gtk-icon-palette: success {}; }}",
                w.color.rgba()
            ));
        }
        self.refresh_inspector(w, &views);
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
                .is_some_and(|c| c.shown);
            field.widget.set_visible(shown);
            match &field.value {
                Some(FieldValue::Size(input)) => input.set_value(brush.diameter as f64),
                Some(FieldValue::Opacity(input)) => input.set_value(brush.opacity as f64),
                Some(FieldValue::Color(input)) => {
                    let [r, g, b, a] = brush.color;
                    input.set_rgba(&gdk::RGBA::new(r, g, b, a));
                }
                None => (),
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
            group.append(&label);
            let value = match control {
                PanelControl::BrushSize => {
                    let spec = BRUSH_SIZE_CONTROL;
                    let input = gtk::SpinButton::with_range(spec.min, spec.max, spec.step);
                    input.set_digits(spec.digits);
                    shared_spin_icons(input.upcast_ref());
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
                    let input = scale(OPACITY_CONTROL);
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
                _ => unreachable!("system controls already built"),
            };
            group.set_visible(false);
            body.append(&group);
            self.customization
                .controls
                .borrow_mut()
                .push(ControlWidget {
                    panel,
                    control,
                    widget: group.upcast(),
                    value: Some(value),
                });
        }
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
        double_tap: bool,
    ) {
        widget.add_css_class("customizable-target");
        let click = gtk::GestureClick::new();
        click.set_name(Some("workspace-context-click"));
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |gesture, n, x, y| {
                let Some(widget) = gesture.widget() else {
                    return;
                };
                if !owns_context(&widget, x, y) {
                    return;
                }
                if gesture.current_button() == 3 {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    w.show_context(&widget, target, x, y);
                } else if double_tap
                    && n == 2
                    && matches!(gesture.current_button(), 0 | 1)
                    && let ContextTarget::Panel { panel } = target
                {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    w.customize(CustomizationAction::ShowAllControls { panel });
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
        let root = gtk::gio::Menu::new();
        let actions = gtk::gio::SimpleActionGroup::new();
        for (s, items) in menu.sections.into_iter().enumerate() {
            let section = gtk::gio::Menu::new();
            for (i, item) in items.into_iter().enumerate() {
                let id = format!("item-{s}-{i}");
                let model = gtk::gio::MenuItem::new(Some(item.label), None);
                let action = if let Some(selected) = item.selected {
                    let action = gtk::gio::SimpleAction::new_stateful(
                        &id,
                        Some(&String::static_variant_type()),
                        &(if selected { "selected" } else { "other" }).to_variant(),
                    );
                    model.set_action_and_target_value(
                        Some(&format!("context.{id}")),
                        Some(&"selected".to_variant()),
                    );
                    action
                } else {
                    model.set_detailed_action(&format!("context.{id}"));
                    gtk::gio::SimpleAction::new(&id, None)
                };
                action.connect_activate(glib::clone!(
                    #[weak(rename_to = w)]
                    self,
                    move |_, _| {
                        w.customization.context.popdown();
                        w.customize(item.action.clone());
                    }
                ));
                actions.add_action(&action);
                section.append_item(&model);
            }
            root.append_section(None, &section);
        }
        let popover = &self.customization.context;
        popover.insert_action_group("context", Some(&actions));
        popover.set_menu_model(Some(&root));
        popover.set_pointing_to(Some(&gdk::Rectangle::new(
            point.x() as i32,
            point.y() as i32,
            1,
            1,
        )));
        popover.popup();
        popover.present();
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
