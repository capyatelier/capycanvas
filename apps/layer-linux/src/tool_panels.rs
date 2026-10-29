//! Native projections of shared tool controls and color-wheel geometry.
use crate::display_color::{ColorPatch, ViewColor};
use crate::{
    number_control::NumberControl,
    workspace::{Workspace, selected},
};
use gtk::{glib, prelude::*, subclass::prelude::*};
use layer_ui::{
    ColorAction, ColorPanelLayout, ColorShape, ColorSlot, ColorState,
    ColorWheelGeometry, Theme, ToolSetItem, ToolSetView, ToolSetting, ToolSettingAction, UiAction,
    ToolActionGroup, UiState,
};

const TOOL_ROW_HEIGHT: i32 = 44;

/// A body can be projected in a dock or a tool drawer without reparenting the
/// other view. Preview textures are shared by the existing immutable cache.
pub struct ToolSet {
    pub root: gtk::Box,
    panel: layer_ui::Panel,
    groups: gtk::FlowBox,
    divider: gtk::Separator,
    list: gtk::Box,
    pub group_buttons: RefCell<Vec<gtk::Button>>,
    pub buttons: RefCell<Vec<(ToolSetItem, gtk::Button, Option<gtk::Picture>)>>,
    view: RefCell<ToolSetView>,
    theme: Cell<Option<Theme>>,
}
impl ToolSet {
    pub fn new() -> Self {
        Self::for_panel(layer_ui::Panel::Brushes)
    }
    pub fn for_panel(panel: layer_ui::Panel) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let groups = gtk::FlowBox::builder()
            .homogeneous(false)
            .min_children_per_line(1)
            .max_children_per_line(if matches!(panel, layer_ui::Panel::BrushSets | layer_ui::Panel::SculptSets) { 1 } else { 16 })
            .selection_mode(gtk::SelectionMode::None)
            .column_spacing(2)
            .row_spacing(2)
            .build();
        groups.add_css_class("tool-groups");
        let divider = gtk::Separator::builder().orientation(gtk::Orientation::Horizontal)
            .margin_start(4).margin_end(4).visible(false).build();
        divider.set_widget_name("tool-set-divider");
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        root.append(&groups);
        root.append(&divider);
        root.append(&list);
        groups.set_visible(panel != layer_ui::Panel::Tools);
        list.set_visible(!matches!(panel, layer_ui::Panel::BrushSets | layer_ui::Panel::SculptSets));
        Self {
            root,
            panel,
            groups,
            divider,
            list,
            group_buttons: RefCell::default(),
            buttons: RefCell::default(),
            view: RefCell::default(),
            theme: Cell::new(None),
        }
    }
    pub fn refresh_state(&self, workspace: &Rc<Workspace>, state: &UiState) {
        self.refresh(workspace, state.tool_panel(self.panel), state.theme);
    }
    pub fn refresh(&self, workspace: &Rc<Workspace>, view: &ToolSetView, theme: Theme) {
        let same = |a: &[ToolSetItem], b: &[ToolSetItem]| {
            a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| {
                    a.label == b.label
                        && a.action == b.action
                        && a.icon == b.icon
                        && a.preview == b.preview
                })
        };
        self.groups.set_visible(self.panel != layer_ui::Panel::Tools && !view.groups.is_empty());
        self.divider.set_visible(self.groups.is_visible() && self.list.is_visible() && !view.subtools.is_empty());
        let previous = self.view.borrow();
        if !same(&previous.groups, &view.groups) {
            while let Some(child) = self.groups.first_child() {
                self.groups.remove(&child);
            }
            let mut buttons = self.group_buttons.borrow_mut();
            buttons.clear();
            for item in &view.groups {
                let button = workspace.action_button(&item.label, item.action.clone());
                button.add_css_class("flat");
                button.add_css_class("tool-group");
                let media = matches!(self.panel, layer_ui::Panel::BrushSets | layer_ui::Panel::SculptSets);
                if media { button.set_widget_name(&format!("{}-{}", if self.panel == layer_ui::Panel::SculptSets { "sculpt-set" } else { "brush-set" }, item.icon)); }
                button.set_size_request(
                    if media { (layer_ui::BRUSH_SETS_MIN_WIDTH - 2.0 * layer_ui::PANEL_CONTENT_INSET) as i32 } else { -1 },
                    if media { TOOL_ROW_HEIGHT } else { layer_ui::PANEL_CONTROL_METRICS.control_height as i32 },
                );
                let content = aligned_icon_label(&item.label, item.icon, if media { 0.0 } else { 1.0 });
                if let Some(label) = content.last_child().and_downcast::<gtk::Label>() {
                    if media { crate::panel_controls::two_lines(&label); }
                    else { label.set_max_width_chars(-1); }
                }
                button.set_child(Some(&content));
                if let Some(grab) = layer_ui::BubbleGrab::for_choice(&item.action) {
                    workspace.register_tear(&button, grab);
                }
                self.groups.insert(&button, -1);
                buttons.push(button);
            }
        }
        let rebuild = !same(&previous.subtools, &view.subtools);
        if rebuild {
            // Short selection command rows must not set a tall minimum for
            // the adjacent settings panel.
            let selection = view.subtools.iter().any(|i| matches!(i.action,
                UiAction::Invoke {command: layer_ui::CommandId::TonalSelect}));
            while let Some(child) = self.list.first_child() {
                self.list.remove(&child);
            }
            let mut buttons = self.buttons.borrow_mut();
            buttons.clear();
            for item in &view.subtools {
                let button = workspace.action_button(&item.label, item.action.clone());
                button.add_css_class("flat");
                button.add_css_class("brush-choice");
                let preview = item.preview.map(|id| {
                    button.set_widget_name(&format!("brush-{id}"));
                    button.add_css_class("tool-preview-choice");
                    let preview = gtk::Picture::builder()
                        .can_shrink(true)
                        .content_fit(gtk::ContentFit::Fill)
                        .height_request(layer_ui::TOOL_PREVIEW_METRICS.preview_height as i32)
                        .build();
                    let content: PreviewRow = glib::Object::new();
                    preview.set_parent(&content);
                    preview_caption(item).set_parent(&content);
                    button.set_child(Some(&content));
                    preview
                });
                if preview.is_none() {
                    button.set_size_request(-1, if selection {32} else {TOOL_ROW_HEIGHT});
                    if let UiAction::Invoke { command } = item.action {
                        button.set_widget_name(&format!("tool-choice-{command:?}"));
                    }
                    button.set_child(Some(&tool_label(item)));
                }
                if let Some(grab) = layer_ui::BubbleGrab::for_choice(&item.action) {
                    workspace.register_tear(&button, grab);
                }
                self.list.append(&button);
                buttons.push((item.clone(), button, preview));
            }
        }
        for (button, item) in self.group_buttons.borrow().iter().zip(&view.groups) {
            selected(button, item.selected);
            show_availability(button, item);
        }
        let theme_changed = self.theme.replace(Some(theme)) != Some(theme);
        for ((_, button, preview), item) in self.buttons.borrow().iter().zip(&view.subtools) {
            selected(button, item.selected);
            show_availability(button, item);
            if (rebuild || theme_changed)
                && let (Some(id), Some(preview)) = (item.preview, preview)
            {
                preview.set_paintable(Some(&crate::previews::texture(id, theme)));
            }
        }
        drop(previous);
        *self.view.borrow_mut() = view.clone();
    }
}
fn show_availability(button: &gtk::Button, item: &ToolSetItem) {
    button.set_sensitive(item.enabled);
    button.set_tooltip_text(item.disabled_reason.as_deref());
    if item.enabled && item.disabled_reason.is_some() { button.add_css_class("unavailable-tool"); } else { button.remove_css_class("unavailable-tool"); }
}
fn tool_label(item: &ToolSetItem) -> gtk::Box {
    let row = aligned_icon_label(&item.label, item.icon, if matches!(item.action, UiAction::ColorPicker { .. }) { 0.0 } else { 1.0 });
    if let Some(label) = row.last_child().and_downcast::<gtk::Label>() { crate::panel_controls::two_lines(&label); }
    row
}
fn preview_caption(item: &ToolSetItem) -> gtk::Box {
    let height = layer_ui::PANEL_CONTROL_METRICS.control_height as i32;
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.set_can_target(false);
    let icon = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    icon.add_css_class("tool-caption-surface");
    icon.set_size_request(height, height);
    icon.set_halign(gtk::Align::Start);
    icon.set_valign(gtk::Align::Center);
    icon.set_homogeneous(true);
    icon.append(&crate::icons::image(&format!("layer-{}-symbolic", item.icon)));
    row.append(&icon);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    row.append(&spacer);
    let name = gtk::Label::new(Some(&item.label));
    name.add_css_class("tool-caption-surface");
    name.add_css_class("tool-caption-name");
    name.set_valign(gtk::Align::Center);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    row.append(&name);
    row
}

pub fn icon_label(text: &str, icon: &str) -> gtk::Box {
    aligned_icon_label(text, icon, 0.0)
}

pub(crate) fn aligned_icon_label(text: &str, icon: &str, xalign: f32) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.set_valign(gtk::Align::Center);
    row.append(&crate::icons::image(&format!("layer-{}-symbolic", icon)));
    let label = gtk::Label::new(Some(text));
    label.set_hexpand(true);
    label.set_xalign(xalign);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(1);
    row.append(&label);
    row
}
mod preview_row {
    use super::*;
    #[derive(Default)]
    pub struct PreviewRow;
    #[glib::object_subclass]
    impl ObjectSubclass for PreviewRow {
        const NAME: &'static str = "CapyPreviewRow";
        type Type = super::PreviewRow;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for PreviewRow {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() { child.unparent(); }
        }
    }
    impl WidgetImpl for PreviewRow {
        fn measure(&self, axis: gtk::Orientation, size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            let (minimum, natural, _, _) = obj.last_child().unwrap().measure(axis, size);
            if axis == gtk::Orientation::Horizontal { (minimum, natural, -1, -1) }
            else {
                let [minimum, natural] = layer_ui::TOOL_PREVIEW_METRICS.heights(natural as f32);
                (minimum.ceil() as i32, natural.ceil() as i32, -1, -1)
            }
        }
        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let obj = self.obj();
            let caption = obj.last_child().unwrap();
            let preview = obj.first_child().unwrap();
            let bounds = layer_ui::TOOL_PREVIEW_METRICS.layout(width as f32, height as f32,
                caption.measure(gtk::Orientation::Vertical, width).1 as f32);
            for (widget, bounds) in [(preview, bounds[0]), (caption, bounds[1])] {
                widget.allocate(width, bounds.height as i32, -1, Some(gtk::gsk::Transform::new()
                    .translate(&gtk::graphene::Point::new(bounds.x, bounds.y.floor()))));
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            obj.snapshot_child(&obj.first_child().unwrap(), snapshot);
            obj.snapshot_child(&obj.last_child().unwrap(), snapshot);
        }
    }
}
glib::wrapper! {
    pub struct PreviewRow(ObjectSubclass<preview_row::PreviewRow>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct ToolSettings {
    pub root: gtk::Box,
    settings: gtk::Box,
    form: gtk::Box,
    modes: gtk::Box,
    mode_container: gtk::Box,
    extra: gtk::Box,
    extra_fields: RefCell<Vec<(layer_ui::ToolOption,crate::tool_extra::ExtraField)>>,
    extra_context: Cell<Option<layer_ui::ToolbarContext>>,
    picker: crate::color_picker::Settings,
    fields: RefCell<Vec<(ToolSetting, NumberControl)>>,
    headings: RefCell<Vec<(&'static str, gtk::Label)>>,
    range: RefCell<Option<Rc<crate::range_control::RangeControl>>>,
    actions: RefCell<Vec<(ToolSettingAction, std::sync::Arc<str>, gtk::Widget)>>,
    selection_actions: gtk::MenuButton,
    selection_menu: gtk::PopoverMenu,
    selection_bound: Cell<bool>,
    updating: Rc<Cell<bool>>,
}
impl ToolSettings {
    pub fn new() -> Self {
        let localization = crate::launch_localization();
        let root = body();
        root.add_css_class("tool-settings");
        let settings = gtk::Box::new(gtk::Orientation::Vertical, crate::panel_controls::CONTROL_SPACING);
        let form = gtk::Box::new(gtk::Orientation::Vertical, crate::panel_controls::CONTROL_SPACING);
        let picker = crate::color_picker::Settings::new(localization);
        let mode_container=gtk::Box::new(gtk::Orientation::Vertical, crate::panel_controls::CONTROL_SPACING);
        let modes=gtk::Box::new(gtk::Orientation::Horizontal,0);
        modes.set_homogeneous(true);
        modes.set_widget_name("selection-mode-row");
        modes.add_css_class("linked");
        modes.add_css_class("selection-modes");
        modes.update_property(&[gtk::accessible::Property::Label(&ToolActionGroup::SelectionMode.localized_label(localization))]);
        mode_container.append(&modes);
        let selection_menu = gtk::PopoverMenu::from_model(None::<&gtk::gio::MenuModel>);
        let selection_actions = gtk::MenuButton::builder()
            .label(localization.text(layer_ui::MessageId::MENU_SELECT).as_ref())
            .popover(&selection_menu)
            .build();
        selection_actions.set_widget_name("selection-actions");
        mode_container.append(&selection_actions);
        root.append(&mode_container);
        let extra=gtk::Box::new(gtk::Orientation::Vertical, crate::panel_controls::CONTROL_SPACING);
        settings.append(&extra);
        settings.append(&form);
        root.append(&settings);
        root.append(&picker.root);
        Self {
            root, settings, form, picker, extra, modes, mode_container, extra_fields:RefCell::default(), extra_context:Cell::new(None),
            fields: RefCell::default(),
            headings: RefCell::default(),
            range: RefCell::default(),
            actions: RefCell::default(),
            selection_actions,
            selection_menu,
            selection_bound: Cell::new(false),
            updating: Rc::new(Cell::new(false)),
        }
    }
    pub(crate) fn set_localization(&self, localization: std::sync::Arc<layer_ui::Localizer>) {
        self.modes.update_property(&[gtk::accessible::Property::Label(&ToolActionGroup::SelectionMode.localized_label(&localization))]);
        self.selection_actions.set_label(&localization.text(layer_ui::MessageId::MENU_SELECT));
        for (control, input) in self.fields.borrow().iter() { input.set_caption(&control.label, "", localization.clone()); }
        self.picker.set_localization(&localization);
    }
    pub fn refresh(&self, workspace: &Rc<Workspace>, state: &UiState) {
        let picking = state.layer_tools.tool.picks_color();
        let compact = state.layer_tools.tool.selection_tool() == Some(layer_ui::SelectionTool::Tonal);
        let action_caption=|command: &layer_ui::CommandState| state.canvas_bar.as_ref().into_iter().flat_map(|bar| &bar.completion).find_map(|item| match &item.option {
            layer_ui::ToolOption::Action {state,..} if state.id==command.id => Some(item.label.clone()),
            _ => None,
        }).unwrap_or_else(|| command.label.clone());
        self.settings.set_visible(!picking);
        self.extra.set_visible(!picking && state.tool_extra.iter().any(|option| !matches!(option, layer_ui::ToolOption::Choice { beside: Some(_), .. })));
        let mode_group=state.tool_actions.iter().find_map(|action| action.group().filter(|group| group.segmented()));
        self.mode_container.set_visible(!picking && mode_group.is_some());
        if let Some(group)=mode_group { self.modes.update_property(&[gtk::accessible::Property::Label(&group.localized_label(&workspace.localization()))]); }
        self.selection_actions.set_visible(!compact && state.layer_tools.tool.selection_tool().is_some());
        if !self.selection_bound.replace(true) {
            workspace.watch_popover(self.selection_menu.upcast_ref());
            self.selection_menu.connect_show(glib::clone!(
                #[weak]
                workspace,
                move |menu| {
                    let model = workspace
                        .gpu
                        .borrow()
                        .as_ref()
                        .map(|g| g.session.selection_menu(layer_ui::SelectionMenu::Selection));
                    if let Some(model) = model {
                        workspace.populate_workspace_menu(menu, model);
                    }
                }
            ));
        }
        self.picker.root.set_visible(picking);
        if picking { self.picker.refresh(workspace, state); return; }
        let context=state.toolbar_context();
        let context_changed=self.extra_context.get()!=Some(context);
        let same=self.extra_context.get()==Some(context) && self.extra_fields.borrow().len()==state.tool_extra.len() && self.extra_fields.borrow().iter().zip(&state.tool_extra).all(|((old,_),next)|old.same_schema(next));
        if !same {
            self.extra_context.set(Some(context));
            self.extra_fields.borrow_mut().clear();
            while let Some(child)=self.extra.first_child() {self.extra.remove(&child);}
            for option in &state.tool_extra {
                let field=crate::tool_extra::ExtraField::new(workspace,option,context);
                if !matches!(option, layer_ui::ToolOption::Choice { beside: Some(_), .. }) { self.extra.append(&field.root); }
                self.extra_fields.borrow_mut().push((option.clone(),field));
            }
        } else {
            for ((old,field),next) in self.extra_fields.borrow_mut().iter_mut().zip(&state.tool_extra) {field.refresh(next);*old=next.clone();}
        }
        let controls = &state.tool_settings;
        self.updating.set(true);
        let mut fields = self.fields.take();
        let mut actions = self.actions.take();
        let same_schema = same && !context_changed && fields.len() == controls.len()
            && fields.iter().zip(controls).all(|((old, _), next)| {
                old.id == next.id
                    && old.group_id == next.group_id
                    && old.numeric == next.numeric
            })
            && actions.len() == state.tool_actions.len()
            && actions
                .iter()
                .zip(&state.tool_actions)
                .all(|((old, _, _), next)| old == next);
        if !same_schema {
            if let Some(range) = self.range.borrow().as_ref() { range.retire(); }
            for (option, field) in self.extra_fields.borrow().iter() {
                if matches!(option, layer_ui::ToolOption::Choice { beside: Some(_), .. })
                    && let Some(parent) = field.root.parent().and_downcast::<gtk::Box>() { parent.remove(&field.root); }
            }
            while let Some(child) = self.form.first_child() {
                self.form.remove(&child);
            }
            while let Some(child)=self.modes.first_child() {self.modes.remove(&child);}
            fields.clear();
            self.headings.borrow_mut().clear();
            self.range.borrow_mut().take();
            actions.clear();
            let mut group = "";
            let mut grouped = None::<gtk::Box>;
            let inline_labels=gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
            let inline_values=gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
            let dropdown_labels=gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
            for control in controls {
                if compact && control.id == "tonal_upper" { continue; }
                if compact && control.id == "tonal_lower" {
                    let upper = controls.iter().find(|f| f.id == "tonal_upper").unwrap();
                    let weak = Rc::downgrade(workspace);
                    let ids = [control.id, upper.id];
                    let range = crate::range_control::RangeControl::new("tonal", "Range in stops relative to reference white (0)", [control, upper], move |index, value| {
                        if let Some(w) = weak.upgrade() {
                            w.dispatch(UiAction::ToolbarEdit { context, action: Box::new(UiAction::SetToolSetting { id: ids[index].into(), value: value as f32 }) });
                        }
                    }, workspace.localization().clone());
                    self.form.append(&range.root);
                    fields.extend([(control.clone(), range.inputs[0].clone()), (upper.clone(), range.inputs[1].clone())]);
                    self.range.replace(Some(range));
                    continue;
                }
                if group != control.group_id {
                    group = control.group_id;
                    grouped = None;
                    let extras = self.extra_fields.borrow();
                    let beside = extras.iter().find(|(option, _)| matches!(option, layer_ui::ToolOption::Choice { beside: Some(id), .. } if *id == control.id));
                    if !group.is_empty() {
                        let title = gtk::Label::new(Some(beside.map_or(control.group.as_ref(), |(option, _)| match option { layer_ui::ToolOption::Choice { label, .. } => label.as_ref(), _ => control.group.as_ref() })));
                        title.add_css_class("heading");
                        title.add_css_class("dim-label");
                        title.set_xalign(0.0);
                        title.set_margin_top(if self.form.first_child().is_some() {
                            crate::panel_controls::SPACING - crate::panel_controls::CONTROL_SPACING
                        } else { 0 });
                        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
                        self.form.append(&title);
                        self.headings.borrow_mut().push((control.id, title));
                    }
                    if let Some((_, field)) = beside {
                        let row = crate::panel_controls::action_row(crate::panel_controls::COMPOUND_SPACING);
                        let numbers = gtk::Box::new(gtk::Orientation::Vertical, crate::panel_controls::COMPOUND_SPACING);
                        numbers.set_hexpand(true);
                        row.append(&field.root); row.append(&numbers); self.form.append(&row);
                        grouped = Some(numbers);
                    }
                }
                let input = if compact || grouped.is_some() {
                    NumberControl::labeled_inline(control.numeric.clone(), &control.label, &control.tooltip_localized(&workspace.localization()), &inline_labels, &inline_values, workspace.localization().clone())
                } else { NumberControl::panel(control.numeric.clone(), &control.label, workspace.localization().clone()) };
                input.fit_spin_width();
                if grouped.is_some() { input.set_slider_visible(false); }
                input.set_widget_name(&format!("tool-setting-{}", control.id));
                let id = control.id;
                if control.preview { input.connect_edit_phase(glib::clone!(#[weak] workspace, move |phase| {
                    workspace.dispatch(UiAction::ToolbarEdit { context, action: Box::new(UiAction::ToolSettingContact { id: id.into(), phase, value: None }) });
                })); }
                input.connect_reset_requested(glib::clone!(#[weak] workspace, move || {
                    workspace.dispatch(UiAction::ToolbarEdit { context, action: Box::new(UiAction::ResetToolSetting { id: id.into() }) });
                }));
                input.connect_value_changed(glib::clone!(
                    #[weak]
                    workspace,
                    move |input| {
                        workspace.dispatch(UiAction::ToolbarEdit { context, action: Box::new(UiAction::SetToolSetting {
                            id: id.into(),
                            value: input.value() as f32,
                        }) });
                    }
                ));
                grouped.as_ref().unwrap_or(&self.form).append(&input);
                fields.push((control.clone(), input));
            }
            let mut mode_group: Option<gtk::ToggleButton> = None;
            let mut action_row = None::<(layer_ui::ToolActionRow, gtk::Box)>;
            for action in &state.tool_actions {
                let command = state
                    .commands
                    .iter()
                    .find(|c| c.id == action.command)
                    .expect("core command exists");
                let mode = action.group().is_some_and(ToolActionGroup::segmented);
                let caption=action_caption(command);
                if action.checkable && let Some(group)=action.group().filter(|group| !group.segmented()) {
                    let existing=actions.iter().find(|(old,_,_)| old.group()==Some(group));
                    let dropdown=if let Some((_,_,widget))=existing { widget.clone() } else {
                        let commands=state.tool_actions.iter().filter(|a| a.checkable && a.group()==Some(group)).map(|a| a.command).collect::<Vec<_>>();
                        let states=commands.iter().map(|id| state.commands.iter().find(|c| c.id==*id).unwrap()).collect::<Vec<_>>();
                        let dropdown=crate::panel_controls::command_dropdown(&states);
                        dropdown.set_widget_name(&format!("tool-action-group-{}",group.id()));
                        let updating=self.updating.clone();
                        dropdown.connect_selected_notify(glib::clone!(#[weak] workspace,move |dropdown| {
                            if !updating.get() && let Some(&command)=commands.get(dropdown.selected() as usize) { workspace.dispatch(UiAction::Invoke {command}); }
                        }));
                        self.form.append(&crate::panel_controls::row(&group.localized_label(&workspace.localization()),&dropdown,crate::panel_controls::CONTROL_SPACING,Some(&dropdown_labels)));
                        dropdown.upcast()
                    };
                    actions.push((*action,command.label.clone(),dropdown));
                    action_row=None;
                    continue;
                }
                let widget: gtk::Widget = if mode {
                    let button = gtk::ToggleButton::new();
                    button.set_size_request(layer_ui::PANEL_CONTROL_METRICS.control_height as i32, layer_ui::PANEL_CONTROL_METRICS.control_height as i32);
                    let image = crate::icons::image(&format!("layer-{}-symbolic", command.icon.unwrap()));
                    image.set_pixel_size(16);
                    button.set_child(Some(&image));
                    button.update_property(&[gtk::accessible::Property::Label(&command.label)]);
                    if let Some(first) = &mode_group { button.set_group(Some(first)); }
                    else { mode_group = Some(button.clone()); }
                    let updating = self.updating.clone();
                    let id = action.command;
                    button.connect_toggled(glib::clone!(
                        #[weak]
                        workspace,
                        move |button| {
                            if !updating.get() && button.is_active() {
                                workspace.dispatch(UiAction::Invoke { command: id });
                            }
                        }
                    ));
                    button.upcast()
                } else if action.checkable {
                    let check = crate::panel_controls::check(&command.label);
                    check.set_tooltip_text(Some(&command.label));
                    let updating = self.updating.clone();
                    let id = action.command;
                    check.connect_toggled(glib::clone!(
                        #[weak]
                        workspace,
                        move |_| {
                            if !updating.get() {
                                workspace.dispatch(UiAction::Invoke { command: id });
                            }
                        }
                    ));
                    check.upcast()
                } else {
                    let button = workspace.action_button(
                        &caption,
                        UiAction::Invoke {
                            command: action.command,
                        },
                    );
                    button.set_child(Some(&icon_label(&caption, command.icon.unwrap())));
                    button.upcast()
                };
                widget.set_widget_name(&format!("tool-action-{:?}", action.command));
                if mode { self.modes.append(&widget); }
                else {
                    if let Some(pair)=action.row() {
                        if pair.icon_only() && let Some(button)=widget.downcast_ref::<gtk::Button>() {
                            let icon=crate::icons::image(&format!("layer-{}-symbolic",command.icon.unwrap()));
                            icon.set_pixel_size(16);button.set_child(Some(&icon));button.add_css_class("tool-icon-action");
                        }
                        if action_row.as_ref().is_none_or(|(old,_)| *old!=pair) {
                            let row=crate::panel_controls::action_row(crate::panel_controls::CONTROL_SPACING);self.form.append(&row);action_row=Some((pair,row));
                        }
                        action_row.as_ref().unwrap().1.append(&widget);
                    } else {
                        action_row=None;
                        self.form.append(&widget);
                    }
                }
                actions.push((*action, caption, widget));
            }
        }
        for (id, title) in self.headings.borrow().iter() {
            if let Some(control) = controls.iter().find(|control| control.id == *id) { title.set_text(&control.group); }
        }
        for ((old, input), control) in fields.iter_mut().zip(controls) {
            if old.label != control.label { input.set_caption(&control.label, "", workspace.localization()); input.set_tooltip_text(Some(&control.tooltip_localized(&workspace.localization()))); }
            *old = control.clone();
            input.set_value(control.value as f64);
        }
        if let Some(range) = self.range.borrow().as_ref() {
            range.set_values([fields[0].1.value(), fields[1].1.value()]);
        }
        let mut refreshed_groups=Vec::new();
        for (action, label, widget) in actions.iter_mut() {
            if let Some(command) = state.commands.iter().find(|c| c.id == action.command) {
                if let Some(dropdown)=widget.downcast_ref::<gtk::DropDown>() {
                    let group=action.group().unwrap();
                    *label=command.label.clone();
                    if refreshed_groups.contains(&group) {continue;}
                    refreshed_groups.push(group);
                    let commands=state.tool_actions.iter().filter(|a| a.checkable && a.group()==Some(group)).filter_map(|a| state.commands.iter().find(|c| c.id==a.command)).collect::<Vec<_>>();
                    crate::panel_controls::update_commands(dropdown,&commands);
                    let caption=group.localized_label(&workspace.localization());
                    dropdown.update_property(&[gtk::accessible::Property::Label(&caption)]);
                    dropdown.set_tooltip_text(Some(&caption));
                    if let Some(caption_label)=dropdown.parent().and_then(|row| row.first_child()).and_downcast::<gtk::Label>() {caption_label.set_label(&caption);}
                    dropdown.set_sensitive(commands.iter().any(|c| c.enabled));
                    dropdown.set_selected(commands.iter().position(|c| c.selected).map_or(gtk::INVALID_LIST_POSITION,|i| i as u32));
                    continue;
                }
                let caption=action_caption(command);
                if *label != caption {
                    if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
                        check.set_label(Some(&caption));
                    } else if let Some(button) = widget.downcast_ref::<gtk::Button>()
                        && let Some(child) = button.child().and_then(|row| row.last_child()).and_downcast::<gtk::Label>() { child.set_text(&caption); }
                    widget.update_property(&[gtk::accessible::Property::Label(&command.label)]);
                    *label = caption;
                }
                widget.set_sensitive(command.enabled);
                widget.set_tooltip_text(Some(&command.tooltip));
                if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
                    check.set_active(command.selected);
                }
                if let Some(button) = widget.downcast_ref::<gtk::ToggleButton>() {
                    button.set_active(command.selected);
                    selected(button, command.selected);
                }
            }
        }
        self.fields.replace(fields);
        self.actions.replace(actions);
        self.updating.set(false);
    }
}

pub fn size_grid(workspace: &Rc<Workspace>) -> (adw::WrapBox, Vec<(f32, gtk::Button)>) {
    let style = layer_ui::TileStyle::Small;
    let grid = adw::WrapBox::builder()
        .child_spacing(style.gap() as i32)
        .line_spacing(style.gap() as i32)
        .build();
    let buttons = layer_ui::brush_size_presets().map(|preset| {
        let button = workspace.action_button("", UiAction::SetBrushSize { value: preset.value });
        button.add_css_class("flat");
        button.add_css_class("size-preset");
        button.set_widget_name(&format!("size-preset-{}", preset.label));
        button.set_tooltip_text(Some(&format!("{} px", preset.label)));
        button.update_property(&[gtk::accessible::Property::Label(&format!("{} px", preset.label))]);
        let preview = gtk::Overlay::new();
        let dot = gtk::DrawingArea::builder().width_request(layer_ui::BRUSH_SIZE_TILE[0] as i32).height_request(layer_ui::BRUSH_SIZE_TILE[1] as i32).build();
        dot.set_draw_func(move |area, cr, width, height| {
            let color = area.color();
            let fade = gtk::cairo::LinearGradient::new(0., 0., 0., height as f64);
            for (position, alpha) in [(0.4, 1.), (0.65, 0.2), (1., 0.)] {
                fade.add_color_stop_rgba(position, color.red() as f64, color.green() as f64, color.blue() as f64, color.alpha() as f64 * alpha);
            }
            let _ = cr.set_source(&fade);
            cr.arc(width as f64 * 0.5, width as f64 * 0.5, preset.preview_diameter as f64 * 0.5, 0., std::f64::consts::TAU);
            let _ = cr.fill();
        });
        preview.set_child(Some(&dot));
        let label = gtk::Label::new(Some(&preset.label));
        label.add_css_class("size-label");
        label.set_valign(gtk::Align::End);
        label.set_margin_bottom(2);
        label.set_can_target(false);
        preview.add_overlay(&label);
        button.set_child(Some(&preview));
        grid.append(&button);
        (preset.value, button)
    }).collect();
    (grid, buttons)
}

pub struct SizePanel {
    pub root: gtk::Box,
    number: NumberControl,
    grid: adw::WrapBox,
    buttons: Vec<(f32, gtk::Button)>,
}
impl SizePanel {
    pub fn new(workspace: &Rc<Workspace>) -> Self {
        let root = body();
        root.set_spacing(crate::panel_controls::SPACING);
        let number = NumberControl::panel(layer_ui::NumericControl::brush_size(), &workspace.localization().text(layer_ui::MessageId::WORKSPACE_CONTROL_BRUSH_SIZE), workspace.localization().clone());
        number.set_widget_name("drawer-control-brush-size");
        number.connect_reset_requested(glib::clone!(#[weak] workspace, move || {
            workspace.dispatch(UiAction::ResetToolSetting { id: "size".into() });
        }));
        number.connect_value_changed(glib::clone!(
            #[weak]
            workspace,
            move |i| {
                workspace.dispatch(UiAction::SetBrushSize {
                    value: i.value() as f32,
                });
            }
        ));
        let (grid, buttons) = size_grid(workspace);
        root.append(&number);
        root.append(&grid);
        Self {
            root,
            number,
            grid,
            buttons,
        }
    }
    pub fn refresh(&self, workspace: &Workspace, state: &layer_ui::UiState) {
        let brush = &state.brush;
        let config = state.workspace.layout.panel(layer_ui::Panel::Sizes).unwrap();
        self.number.set_visible(config.shows(layer_ui::PanelControl::BrushSize));
        self.grid.set_visible(config.shows(layer_ui::PanelControl::SizePresets));
        self.number.set_caption(&workspace.localization().text(layer_ui::MessageId::WORKSPACE_CONTROL_BRUSH_SIZE), "", workspace.localization().clone());
        self.number.set_value(brush.diameter as f64);
        for (value, button) in &self.buttons {
            selected(button, *value == brush.diameter);
        }
    }
}

fn body() -> gtk::Box {
    let root = crate::panel_controls::column();
    let inset = layer_ui::PANEL_CONTENT_INSET as i32;
    root.set_margin_start(inset);
    root.set_margin_end(inset);
    root.set_margin_top(inset);
    root.set_margin_bottom(inset);
    root
}

// Shared allocations reserve the curved readout band and the optional HDR footer.
// Four 36px tiles (144px panel, 128px content) is the smallest supported width.
mod wheel_button {
    use super::*;
    #[derive(Default)]
    pub struct WheelButton {
        pub readout: Cell<bool>,
        pub rotation: Cell<f32>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for WheelButton {
        const NAME: &'static str = "CapyColorWheelButton";
        type Type = super::WheelButton;
        type ParentType = gtk::Button;
    }
    impl ObjectImpl for WheelButton {}
    impl WidgetImpl for WheelButton {
        fn state_flags_changed(&self, previous: &gtk::StateFlags) {
            self.parent_state_flags_changed(previous);
            if self.obj().state_flags().contains(gtk::StateFlags::PRELIGHT)
                != previous.contains(gtk::StateFlags::PRELIGHT) {
                self.obj().queue_draw();
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let o = self.obj();
            let center = gtk::graphene::Point::new(o.width() as f32 * 0.5, o.height() as f32 * 0.5);
            snapshot.save();
            snapshot.translate(&center);
            snapshot.rotate(self.rotation.get());
            snapshot.translate(&gtk::graphene::Point::new(-center.x(), -center.y()));
            self.parent_snapshot(snapshot);
            if o.has_css_class("color-swatch") && let Some(bounds) = o.compute_bounds(&*o) {
                let highlighted = o.has_css_class("selected-tool")
                    || o.state_flags().contains(gtk::StateFlags::PRELIGHT);
                let mut ink = o.color();
                if !highlighted { ink.set_alpha(ink.alpha() * 0.25); }
                snapshot.append_border(
                    &gtk::gsk::RoundedRect::from_rect(bounds, bounds.width().min(bounds.height()) * 0.5),
                    &[if highlighted { 2. } else { 1. }; 4], &[ink; 4],
                );
            }
            if self.readout.get() && let Some(wheel) = o.parent().and_downcast::<ColorWheel>() {
                crate::color_readout::draw(o.upcast_ref(), snapshot, &wheel.imp().color.borrow(), wheel.imp().view.get());
            }
            snapshot.restore();
        }
        fn contains(&self, x: f64, y: f64) -> bool {
            let o = self.obj();
            let (w, h) = (o.width() as f64, o.height() as f64);
            if self.readout.get() {
                let r = ColorPanelLayout::new((w * 2.) as f32)
                    .map(|layout| layout.wheel[2] as f64 * 0.49 + 2.)
                    .unwrap_or(1.);
                x >= 0. && y >= 0. && x <= w && y <= h && (x - w).hypot(y - h) >= r
            } else {
                o.compute_bounds(&*o).is_some_and(|b| ColorPanelLayout::circle_contains(
                    [b.x(), b.y(), b.width(), b.height()], [x as f32, y as f32],
                ))
            }
        }
    }
    impl ButtonImpl for WheelButton {}
}
glib::wrapper! {
    pub struct WheelButton(ObjectSubclass<wheel_button::WheelButton>) @extends gtk::Button, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}
impl WheelButton {
    pub(crate) fn swatch() -> (Self, ColorPatch) {
        let button: Self = glib::Object::new();
        button.add_css_class("flat");
        button.add_css_class("color-swatch");
        let sample = ColorPatch::new(true);
        sample.set_size_request(16, 16);
        button.set_child(Some(&sample));
        (button, sample)
    }
    pub(crate) fn set_selected(&self, active: bool) {
        if self.has_css_class("selected-tool") != active {
            selected(self, active);
            self.queue_draw();
        }
    }
}
#[path = "color_wheel_field.rs"]
mod field;

mod wheel {
    use super::*;
    type Disc = (u32, f32, ColorShape, layer_core::color::RgbSpace, ViewColor, f32, f32, gtk::gdk::Texture);
    #[derive(Default)]
    pub struct Wheel {
        pub color: RefCell<ColorState>,
        pub preview: Cell<bool>,
        pub resizing: Cell<bool>,
        pub surface_node: RefCell<Option<(f32, gtk::gsk::RenderNode)>>,
        pub(super) field: crate::color_preview_raster::PreviewRaster<super::field::Key, super::field::Request>,
        pub(super) field_path: RefCell<Option<super::field::FieldPath>>,
        pub intensity: RefCell<Option<crate::hdr_color_scale::HdrColorScale>>,
        pub hdr: Cell<bool>,
        pub bare: Cell<bool>,
        pub bare_range: Cell<(i32, i32)>,
        pub bubble_layout: Cell<Option<layer_ui::ColorBubbleLayout>>,
        // Background precedes foreground so their deliberate overlap also picks correctly.
        pub corners: RefCell<Vec<WheelButton>>,
        pub view: Cell<ViewColor>,
        pub headroom: Cell<f32>,
        pub linear_field: RefCell<Option<super::field::LinearField>>,
        pub disc: RefCell<Option<Disc>>,
        pub ring: RefCell<Option<super::field::Guide>>,
        #[cfg(test)]
        pub field_render_ms: RefCell<Vec<f64>>,
        #[cfg(test)]
        pub snapshot_ms: RefCell<Vec<f64>>,
        #[cfg(test)]
        pub refresh_ms: RefCell<Vec<f64>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Wheel {
        const NAME: &'static str = "CapyColorWheel";
        type Type = super::ColorWheel;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Wheel {
        fn dispose(&self) {
            if let Some(intensity) = self.intensity.take() { intensity.unparent(); }
            for button in self.corners.borrow_mut().drain(..) {
                button.unparent();
            }
        }
    }
    impl WidgetImpl for Wheel {
        fn contains(&self, x: f64, y: f64) -> bool {
            let (size, [left, top]) = self.obj().drawing_bounds();
            let radius = size * 0.5;
            (x as f32 - left - radius).hypot(y as f32 - top - radius) <= radius
                || self.hdr.get() && {
                    let (stage, origin) = self.obj().stage_bounds();
                    layer_ui::HdrIntensityArc::new(stage).is_some_and(|arc| arc.contains([x as f32 - origin[0], y as f32 - origin[1]]))
                }
        }
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            if self.bare.get() {
                return if orientation == gtk::Orientation::Vertical {
                    let height = |width: i32| layer_ui::BareColorWheel::new(width as f32,self.hdr.get()).map_or(0,|layout|layout.height.ceil() as i32);
                    let (low, high) = self.bare_range.get();
                    (height(low), height(if for_size < 0 { high } else { for_size.clamp(low, high) }), -1, -1)
                } else {
                    let (low, high) = self.bare_range.get();
                    (low, high, -1, -1)
                };
            }
            if orientation == gtk::Orientation::Vertical {
                let height = |size: i32| if self.hdr.get() {
                    ColorPanelLayout::with_hdr(size as f32).unwrap().height().ceil() as i32
                } else { ColorPanelLayout::new(size as f32).unwrap().height().ceil() as i32 };
                (height(128), height(if for_size < 0 { 226 } else { for_size.max(128) }), -1, -1)
            } else {
                (128, 226, -1, -1)
            }
        }
        fn size_allocate(&self, _width: i32, _height: i32, baseline: i32) {
            let (size, [x, y]) = self.obj().stage_bounds();
            let Some(layout) = (if self.hdr.get() { ColorPanelLayout::with_hdr(size) } else { ColorPanelLayout::new(size) }) else {
                return;
            };
            let boxes = [
                layout.background,
                layout.foreground,
                layout.transparent,
                layout.shapes[0],
                layout.shapes[1],
                layout.swap,
                layout.readout,
                layout.edit,
            ];
            for (button, rotation) in self
                .corners
                .borrow()
                .iter()
                .skip(3)
                .zip(layout.shape_rotations)
            {
                button.imp().rotation.set(rotation);
            }
            for (button, [bx, by, w, h]) in self.corners.borrow().iter().zip(boxes) {
                button.allocate(
                    w.round() as i32,
                    h.round() as i32,
                    baseline,
                    Some(
                        gtk::gsk::Transform::new()
                            .translate(&gtk::graphene::Point::new(x + bx.round(), y + by.round())),
                    ),
                );
            }
            if let Some(intensity) = self.intensity.borrow().as_ref().filter(|i| i.is_visible()) {
                intensity.allocate(size.round() as i32, layout.height().ceil() as i32, baseline,
                    Some(gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(x, y))));
            }
        }
        fn snapshot(&self, output: &gtk::Snapshot) {
            #[cfg(test)]
            let started = std::time::Instant::now();
            let (size, [x, y]) = self.obj().drawing_bounds();
            let content = gtk::Snapshot::new();
            let snapshot = &content;
            if let Some(geometry) = ColorWheelGeometry::new(size) {
                snapshot.save();
                snapshot.translate(&gtk::graphene::Point::new(x, y));
                if !self.resizing.get() || self.surface_node.borrow().is_none() {
                    let surfaces = gtk::Snapshot::new();
                    let bounds = gtk::graphene::Rect::new(0., 0., size, size);
                    let state = self.color.borrow();
                    let shape = state.shape;
                    let view = self.view.get();
                    let side = (size * self.obj().scale_factor() as f32).ceil() as u32;
                    let space = state.rgb_space();
                    let key = field::Key::new(side, &state, view, self.headroom.get());
                    let current = self.disc.borrow().as_ref().map(|d| (d.0,d.1,d.2,d.3,d.4,d.5,d.6));
                    let ring_ready = self.ring.borrow().as_ref().is_some_and(|(s, p, c, v, _)|
                        (*s, *p, *c, *v) == (side, shape, space, view));
                    self.obj().request_field(key, &state, current == Some(key.tuple()) && ring_ready);
                    if let Some(cache) = self.ring.borrow().as_ref() {
                        surfaces.append_texture(&cache.4, &bounds);
                    }
                    if let Some(cache) = self.disc.borrow().as_ref() {
                        match shape {
                            ColorShape::Circle => {
                                let radius = geometry.disc_radius();
                                surfaces.push_rounded_clip(&gtk::gsk::RoundedRect::from_rect(
                                    gtk::graphene::Rect::new(geometry.center[0] - radius, geometry.center[1] - radius,
                                        radius * 2., radius * 2.), radius));
                            }
                            ColorShape::Square => {
                                let [x, y, width] = geometry.square;
                                surfaces.push_rounded_clip(&gtk::gsk::RoundedRect::from_rect(
                                    gtk::graphene::Rect::new(x, y, width, width), (size * 0.02).min(6.)));
                            }
                            ColorShape::Triangle => {
                                let mut field_path = self.field_path.borrow_mut();
                                if field_path.as_ref().is_none_or(|p| p.size != size) {
                                    *field_path = Some(field::FieldPath::new(size, &geometry));
                                }
                                surfaces.push_fill(&field_path.as_ref().unwrap().path, gtk::gsk::FillRule::Winding);
                            }
                        }
                        surfaces.append_texture(&cache.7, &bounds);
                        surfaces.pop();
                    }
                    *self.surface_node.borrow_mut() = surfaces.to_node().map(|node| (size, node));
                }
                if let Some((original, node)) = self.surface_node.borrow().as_ref() {
                    snapshot.save();
                    snapshot.scale(size / original, size / original);
                    snapshot.append_node(node);
                    snapshot.restore();
                }
                draw_wheel(snapshot, &self.color.borrow(), &geometry, self.view.get(), self.headroom.get());
                snapshot.restore();
            }
            let mut child = self.obj().first_child();
            while let Some(widget) = child {
                if widget.is_visible() { self.obj().snapshot_child(&widget, snapshot); }
                child = widget.next_sibling();
            }
            crate::squircle::append_round(output, content);
            #[cfg(test)]
            self.snapshot_ms.borrow_mut().push(started.elapsed().as_secs_f64() * 1000.);
        }
    }
}
glib::wrapper! {
    pub struct ColorWheel(ObjectSubclass<wheel::Wheel>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
const BARE_WHEEL: (i32, i32) = (layer_ui::BareColorWheel::MIN as i32, layer_ui::BareColorWheel::MAX as i32);
fn reset_gestures(widget: &gtk::Widget) {
    let controllers = widget.observe_controllers();
    for i in 0..controllers.n_items() {
        if let Some(gesture) = controllers.item(i).and_downcast::<gtk::Gesture>() { gesture.reset(); }
    }
    let mut child = widget.first_child();
    while let Some(widget) = child { child = widget.next_sibling(); reset_gestures(&widget); }
}
impl ColorWheel {
    pub(crate) fn bare() -> Self {
        let wheel: Self = glib::Object::new();
        wheel.imp().bare.set(true);
        wheel.imp().bare_range.set(BARE_WHEEL);
        let intensity = crate::hdr_color_scale::HdrColorScale::new();
        intensity.hide_caption();
        intensity.set_visible(false);
        intensity.set_parent(&wheel);
        *wheel.imp().intensity.borrow_mut() = Some(intensity);
        wheel
    }
    pub(crate) fn set_bubble_layout(&self, layout: layer_ui::ColorBubbleLayout) {
        if self.imp().bubble_layout.replace(Some(layout)) != Some(layout) { self.queue_resize(); }
    }
    pub(crate) fn intensity(&self) -> Option<crate::hdr_color_scale::HdrColorScale> {
        self.imp().intensity.borrow().clone()
    }
    pub(crate) fn set_state(&self, state: &ColorState, view: ViewColor, headroom: f32, preview: bool, intensity_range: Option<[f32; 2]>) -> bool {
        let imp = self.imp();
        if imp.preview.replace(preview) != preview {
            imp.field.cancel();
            self.queue_draw();
        }
        let hdr = matches!(view, ViewColor::Mapped { .. });
        if let Some(intensity) = imp.intensity.borrow().as_ref() {
            intensity.set_visible(hdr);
            intensity.set_sensitive(state.slot != ColorSlot::Transparent);
            if hdr { intensity.refresh_color(state, view, headroom, preview, intensity_range); }
        }
        if imp.hdr.replace(hdr) != hdr { self.queue_resize(); }
        if !hdr { imp.linear_field.borrow_mut().take(); }
        let previous_headroom = imp.headroom.replace(headroom);
        let previous_view = imp.view.replace(view);
        if previous_view == view && previous_headroom == headroom && *imp.color.borrow() == *state {
            return false;
        }
        *imp.color.borrow_mut() = state.clone();
        imp.surface_node.borrow_mut().take();
        self.queue_draw();
        true
    }
    pub(crate) fn connect_pick(&self, send: impl Fn(ColorAction) + 'static) {
        let send = Rc::new(send);
        let part = Rc::new(Cell::new(None));
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin(glib::clone!(
            #[weak(rename_to = wheel)]
            self,
            #[strong]
            part,
            #[strong]
            send,
            move |gesture, x, y| {
                part.set(None);
                // Popovers are separate native surfaces but still descendants
                // in GTK's widget tree. Never pick through their buttons.
                let native_surface = wheel.native().and_then(|n| n.surface());
                if gesture
                    .current_event()
                    .and_then(|e| e.surface())
                    .is_some_and(|s| Some(s) != native_surface)
                    || wheel
                        .pick(x, y, gtk::PickFlags::DEFAULT)
                        .is_some_and(|w| w != wheel.clone().upcast::<gtk::Widget>())
                {
                    return;
                }
                let (size, [ox, oy]) = wheel.drawing_bounds();
                let point = [x as f32 - ox, y as f32 - oy];
                let hit = ColorWheelGeometry::new(size)
                    .and_then(|g| g.hit_shape(point, wheel.imp().color.borrow().shape));
                part.set(hit);
                if let Some(part) = hit {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    send(ColorAction::PickWheel { part, point, size });
                }
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak(rename_to = wheel)]
            self,
            #[strong]
            part,
            move |gesture, dx, dy| {
                if let Some(part) = part.get()
                    && let Some((x, y)) = gesture.start_point()
                {
                    let (size, [ox, oy]) = wheel.drawing_bounds();
                    send(ColorAction::PickWheel {
                        part,
                        point: [(x + dx) as f32 - ox, (y + dy) as f32 - oy],
                        size,
                    });
                }
            }
        ));
        drag.connect_drag_end({
            let part = part.clone();
            move |_, _, _| part.set(None)
        });
        drag.connect_cancel(move |_, _| part.set(None));
        self.add_controller(drag);
    }
    fn connect_history(&self, workspace: &Rc<Workspace>) {
        let touches = Rc::new(crate::color_touch::ColorTouches::default());
        let pressed = Rc::new(RefCell::new(None::<Option<gtk::gdk::EventSequence>>));
        let owner = Rc::downgrade(workspace);
        let contact = |owner: &std::rc::Weak<Workspace>, phase| {
            if let Some(w) = owner.upgrade() { w.dispatch(UiAction::ColorContact { phase }); }
        };
        let input = gtk::EventControllerLegacy::new();
        input.set_propagation_phase(gtk::PropagationPhase::Capture);
        input.connect_event(glib::clone!(#[weak(rename_to = wheel)] self, #[strong] touches, #[strong] pressed, #[strong] owner,
            #[upgrade_or] glib::Propagation::Proceed,
            move |_, event| {
                use gtk::gdk::EventType;
                let Some(w) = owner.upgrade() else { return glib::Propagation::Proceed };
                let at = event.position().and_then(|(x, y)| crate::input::widget_point(&wheel, x, y)).map(|p| [p.x(), p.y()]);
                let inside = at.is_some_and(|[x, y]| wheel.owns_point(x as f64, y as f64));
                let primary = event.downcast_ref::<gtk::gdk::ButtonEvent>().is_some_and(|b| b.button() == gtk::gdk::BUTTON_PRIMARY);
                let touch = crate::color_touch::ColorTouches::phase(event).map(|phase| (phase, event.event_sequence()));
                let opened = pressed.borrow().clone();
                let finger_down = touch.as_ref().is_some_and(|(phase, _)| *phase == layer_ui::ContactPhase::Down);
                match (event.event_type(), touch, opened) {
                    (EventType::ButtonPress, _, None) if primary && inside => {
                        *pressed.borrow_mut() = Some(None);
                        contact(&owner, layer_ui::ContactPhase::Down);
                    }
                    (_, Some((layer_ui::ContactPhase::Down, sequence)), None) if inside => {
                        *pressed.borrow_mut() = Some(Some(sequence));
                        contact(&owner, layer_ui::ContactPhase::Down);
                    }
                    (EventType::ButtonRelease, _, Some(None)) if primary => {
                        pressed.borrow_mut().take();
                        contact(&owner, layer_ui::ContactPhase::Up);
                    }
                    (_, Some((phase @ (layer_ui::ContactPhase::Up | layer_ui::ContactPhase::Cancel), sequence)), Some(Some(opened)))
                        if sequence == opened =>
                    {
                        pressed.borrow_mut().take();
                        contact(&owner, phase);
                    }
                    _ => {}
                }
                if !touches.forward(&w, wheel.upcast_ref(), event, at, inside) {
                    return glib::Propagation::Proceed;
                }
                if finger_down {
                    reset_gestures(wheel.upcast_ref());
                }
                glib::Propagation::Stop
            }));
        self.add_controller(input);
        self.connect_unmap(move |wheel| {
            if let Some(w) = owner.upgrade() { touches.cancel(&w, wheel.upcast_ref()); }
            if pressed.borrow_mut().take().is_some() { contact(&owner, layer_ui::ContactPhase::Up); }
        });
    }
    fn owns_point(&self, x: f64, y: f64) -> bool {
        self.contains(x, y) && self.pick(x, y, gtk::PickFlags::DEFAULT).is_some_and(|picked| {
            picked == *self.upcast_ref::<gtk::Widget>() || self.intensity().is_some_and(|i| picked == *i.upcast_ref::<gtk::Widget>())
        })
    }
    fn stage_bounds(&self) -> (f32, [f32; 2]) {
        if let Some(layout) = self.imp().bubble_layout.get() { return (layout.stage, [0.; 2]); }
        if self.imp().bare.get() {
            let Some(layout)=layer_ui::BareColorWheel::fit(self.width() as f32,self.height() as f32,self.imp().hdr.get()) else {return (0.,[0.;2]);};
            return (layout.stage,[(self.width() as f32-layout.wheel)*0.5-layout.inset,-layout.inset]);
        }
        let mut size = self.width().min(self.height()).max(0);
        let mut footer = 0.;
        {
            let layout_for = |side| if self.imp().hdr.get() { ColorPanelLayout::with_hdr(side) } else { ColorPanelLayout::new(side) };
            // Fit the shared, width-dependent footer when a short panel constrains height.
            let (mut low, mut high) = (128, size);
            size = 0;
            while low <= high {
                let candidate = low + (high - low) / 2;
                let height = layout_for(candidate as f32).unwrap().height().ceil() as i32;
                if height <= self.height() { size = candidate; low = candidate + 1; }
                else { high = candidate - 1; }
            }
            if let Some(layout) = layout_for(size as f32) {
                footer = layout.height().ceil() - size as f32;
            }
        }
        let size = size as f32;
        (
            size,
            [
                (self.width() as f32 - size) * 0.5,
                (self.height() as f32 - size - footer) * 0.5,
            ],
        )
    }
    pub(crate) fn drawing_bounds(&self) -> (f32, [f32; 2]) {
        let (size, [x, y]) = self.stage_bounds();
        let Some(layout) = ColorPanelLayout::new(size) else {
            return (0., [x, y]);
        };
        (layout.wheel[2], [x + layout.wheel[0], y + layout.wheel[1]])
    }
}
pub struct ColorPanel {
    shape_descriptions: RefCell<[String; 3]>,
    localization: RefCell<std::sync::Arc<layer_ui::Localizer>>,
    pub root: gtk::Box,
    initialized: Cell<bool>,
    wheel: ColorWheel,
    swatches: Vec<(ColorSlot, WheelButton, ColorPatch)>,
    shape_buttons: [WheelButton; 2],
    readout: WheelButton,
    swap: WheelButton,
    intensity: crate::hdr_color_scale::HdrColorScale,
    edit_color: WheelButton,
}
impl ColorPanel {
    pub fn new() -> Self {
        let localization = crate::launch_localization().clone();
        let copy = layer_ui::NativeCopy::new(&localization).color;
        let shape_descriptions = [ColorShape::Circle, ColorShape::Square, ColorShape::Triangle]
            .map(|shape| layer_ui::NativeCaption::ColorShape { shape }.message(&localization));
        let root = body();
        root.add_css_class("color-panel");
        let wheel: ColorWheel = glib::Object::new();
        wheel.set_hexpand(true);
        wheel.set_valign(gtk::Align::Fill);
        wheel.set_widget_name("color-wheel");
        root.append(&wheel);
        let intensity = crate::hdr_color_scale::HdrColorScale::new();
        intensity.set_parent(&wheel);
        *wheel.imp().intensity.borrow_mut() = Some(intensity.clone());
        let edit_color: WheelButton = glib::Object::new();
        edit_color.set_icon_name("document-edit-symbolic");
        edit_color.set_tooltip_text(Some(&copy.edit_menu));
        edit_color.update_property(&[gtk::accessible::Property::Label(&copy.edit)]);
        edit_color.add_css_class("flat");
        edit_color.add_css_class("color-utility");
        edit_color.add_css_class("color-swap");
        edit_color.set_widget_name("color-edit-button");
        let mut swatches = Vec::new();
        for (slot, label) in [
            (ColorSlot::Background, localization.text(layer_ui::MessageId::COMMANDS_BACKGROUND_COLOR)),
            (ColorSlot::Foreground, localization.text(layer_ui::MessageId::COMMANDS_FOREGROUND_COLOR)),
            (ColorSlot::Transparent, localization.text(layer_ui::MessageId::COMMANDS_TRANSPARENT_PAINT)),
        ] {
            let (button, sample) = WheelButton::swatch();
            button.set_tooltip_text(Some(&label));
            button.update_property(&[gtk::accessible::Property::Label(&label)]);
            button.set_widget_name(&format!("color-{slot:?}"));
            button.set_parent(&wheel);
            wheel.imp().corners.borrow_mut().push(button.clone());
            swatches.push((slot, button, sample));
        }
        let shape_buttons = std::array::from_fn(|i| {
            let button: WheelButton = glib::Object::new();
            button.add_css_class("flat");
            button.add_css_class("color-shape");
            button.set_widget_name(&format!("color-shape-{i}"));
            button.set_parent(&wheel);
            wheel.imp().corners.borrow_mut().push(button.clone());
            button
        });
        let swap: WheelButton = glib::Object::new();
        crate::icons::set_button(&swap, "layer-color-swap-symbolic");
        swap.add_css_class("flat");
        swap.add_css_class("color-utility");
        swap.add_css_class("color-swap");
        swap.set_widget_name("color-swap");
        swap.set_tooltip_text(Some(&copy.swap));
        swap.update_property(&[gtk::accessible::Property::Label(
            &copy.swap,
        )]);
        swap.set_parent(&wheel);
        wheel.imp().corners.borrow_mut().push(swap.clone());
        let readout: WheelButton = glib::Object::new();
        readout.imp().readout.set(true);
        readout.add_css_class("flat");
        readout.add_css_class("color-readout");
        readout.set_widget_name("color-readout");
        readout.set_parent(&wheel);
        wheel.imp().corners.borrow_mut().push(readout.clone());
        edit_color.set_parent(&wheel);
        wheel.imp().corners.borrow_mut().push(edit_color.clone());
        Self {
            localization: RefCell::new(localization), shape_descriptions: RefCell::new(shape_descriptions),
            root,
            wheel,
            initialized: Cell::new(false),
            swatches,
            shape_buttons,
            readout,
            swap,
            intensity,
            edit_color,
        }
    }
    pub fn bind(&self, workspace: &Rc<Workspace>) {
        self.intensity.connect_value_changed(glib::clone!(#[weak] workspace, move |i| {
            if i.updating() { return; }
            workspace.dispatch(UiAction::Color { action: ColorAction::HdrIntensity { stops: i.value() as f32 } });
            // Rejected out-of-storage-range edits leave the native control at
            // the accepted value, including in retained color-panel drawers.
            let colors = workspace.gpu.borrow().as_ref().map(|g| g.session.state().display_colors().clone());
            if let Some(colors) = colors { i.refresh_color(&colors, workspace.view_color(), workspace.picker_headroom(), false, None); }
        }));
        self.edit_color.connect_clicked(glib::clone!(#[weak] workspace, move |_| {
            let slot = workspace.gpu.borrow().as_ref().map(|g| g.session.state().display_colors().slot);
            if let Some(slot) = slot { crate::color_editor::show(&workspace, slot); }
        }));
        for (slot, button, _) in &self.swatches {
            let slot = *slot;
            button.connect_clicked(glib::clone!(
                #[weak]
                workspace,
                move |_| workspace.dispatch(UiAction::Color {
                    action: ColorAction::Select { slot }
                })
            ));
            if slot == ColorSlot::Transparent {
                continue;
            }
            let edit = gtk::GestureClick::new();
            edit.set_button(1);
            edit.set_propagation_phase(gtk::PropagationPhase::Capture);
            edit.connect_pressed(glib::clone!(#[weak] workspace, move |g, count, _, _| {
                if count == 2 {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    crate::color_editor::show(&workspace, slot);
                }
            }));
            button.add_controller(edit);
            workspace.install_context(button, layer_ui::ContextTarget::ColorSlot { slot });
        }
        for (i, button) in self.shape_buttons.iter().enumerate() {
            button.connect_clicked(glib::clone!(
                #[weak]
                workspace,
                #[weak(rename_to = wheel)]
                self.wheel,
                move |_| {
                    let shape = wheel.imp().color.borrow().other_shapes()[i];
                    workspace.dispatch(UiAction::Color {
                        action: ColorAction::Shape { shape },
                    });
                }
            ));
        }
        self.readout.connect_clicked(glib::clone!(
            #[weak]
            workspace,
            move |_| workspace.dispatch(UiAction::Color {
                action: ColorAction::ToggleReadout
            })
        ));
        let swap = glib::clone!(
            #[weak]
            workspace,
            move || {
                workspace.dispatch(UiAction::Color {
                    action: ColorAction::Swap,
                });
            }
        );
        self.swap.connect_clicked({
            let swap = swap.clone();
            move |_| swap()
        });
        workspace.install_menu(&self.wheel, layer_ui::ContextTarget::ColorWheel);
        self.wheel.connect_history(workspace);
        let workspace = Rc::downgrade(workspace);
        self.wheel.connect_pick(move |action| {
            if let Some(workspace) = workspace.upgrade() { workspace.dispatch(UiAction::Color { action }); }
        });
    }
    pub(crate) fn set_localization(&self, localization: std::sync::Arc<layer_ui::Localizer>) {
        let copy = layer_ui::NativeCopy::new(&localization).color;
        *self.shape_descriptions.borrow_mut() = [ColorShape::Circle, ColorShape::Square, ColorShape::Triangle]
            .map(|shape| layer_ui::NativeCaption::ColorShape { shape }.message(&localization));
        self.edit_color.set_tooltip_text(Some(&copy.edit_menu));
        self.edit_color.update_property(&[gtk::accessible::Property::Label(&copy.edit)]);
        self.swap.set_tooltip_text(Some(&copy.swap));
        self.swap.update_property(&[gtk::accessible::Property::Label(&copy.swap)]);
        for (slot, button, _) in &self.swatches {
            let message = match slot {
                ColorSlot::Background => layer_ui::MessageId::COMMANDS_BACKGROUND_COLOR,
                ColorSlot::Foreground => layer_ui::MessageId::COMMANDS_FOREGROUND_COLOR,
                _ => layer_ui::MessageId::COMMANDS_TRANSPARENT_PAINT,
            };
            let label = localization.text(message);
            button.set_tooltip_text(Some(&label));
            button.update_property(&[gtk::accessible::Property::Label(&label)]);
        }
        *self.localization.borrow_mut() = localization;
        self.initialized.set(false);
    }
    pub fn headroom(&self) -> f32 { self.wheel.imp().headroom.get() }
    pub fn set_resizing(&self, resizing: bool) {
        if self.wheel.imp().resizing.replace(resizing) != resizing {
            self.wheel.imp().field.cancel();
            self.intensity.set_resizing(resizing);
            self.wheel.queue_draw();
        }
    }
    pub fn refresh_preview(&self, state: &ColorState, view: ViewColor, headroom: f32) {
        self.refresh_color(state, view, headroom, true);
    }
    pub fn refresh(&self, state: &ColorState, view: ViewColor, headroom: f32) {
        self.refresh_color(state, view, headroom, false);
    }
    fn refresh_color(&self, state: &ColorState, view: ViewColor, headroom: f32, preview: bool) {
        #[cfg(test)]
        let started = std::time::Instant::now();
        self.edit_color.set_sensitive(state.slot != ColorSlot::Transparent);
        let changed = self.wheel.set_state(state, view, headroom, preview, None);
        if self.initialized.replace(true) && !changed {
            return;
        }
        let shape_descriptions = self.shape_descriptions.borrow();
        let localization = self.localization.borrow();
        for (button, shape) in self.shape_buttons.iter().zip(state.other_shapes()) {
            let (icon, description) = match shape {
                ColorShape::Circle => ("layer-color-circle-symbolic", &shape_descriptions[0]),
                ColorShape::Square => ("layer-color-square-symbolic", &shape_descriptions[1]),
                ColorShape::Triangle => ("layer-color-triangle-symbolic", &shape_descriptions[2]),
            };
            crate::icons::set_button(button, icon);
            button.set_tooltip_text(Some(description));
            button.update_property(&[gtk::accessible::Property::Label(description)]);
        }
        let description = state.picker_description_localized(view.space(), matches!(view, ViewColor::Mapped { .. }), &localization);
        self.readout.set_tooltip_text(Some(&description));
        self.readout
            .update_property(&[gtk::accessible::Property::Label(&description)]);
        self.readout.queue_draw();
        let front = self.swatches.iter().find(|(slot, _, _)| *slot == state.front_swatch()).unwrap();
        let rear = self.swatches.iter().find(|(slot, _, _)|
            matches!(slot, ColorSlot::Foreground | ColorSlot::Background) && *slot != front.0).unwrap();
        if front.1.prev_sibling().as_ref() != Some(rear.1.upcast_ref()) {
            front.1.insert_after(&self.wheel, Some(&rear.1));
        }
        for (slot, button, sample) in &self.swatches {
            button.set_selected(*slot == state.slot);
            let color = match slot { ColorSlot::Foreground => state.foreground, ColorSlot::Background => state.background, ColorSlot::Temporary => state.temporary, ColorSlot::Transparent => layer_core::color::RgbColor { linear_rgb: None, space: state.rgb_space(), rgba: [0.; 4] } };
            sample.set_display_color(color, view, headroom);
        }
        #[cfg(test)]
        self.wheel.imp().refresh_ms.borrow_mut().push(started.elapsed().as_secs_f64() * 1000.);
    }
}

fn draw_wheel(snapshot: &gtk::Snapshot, state: &ColorState, g: &ColorWheelGeometry, view: ViewColor, headroom: f32) {
    let hue = state.wheel_hue_color_in(state.wheel_components()[0], view.space());
    let color = state.preview_in(state.definition(), view.space());
    let radius = g.marker_radius();
    for (index, (point, rgb)) in [
        (state.wheel_hue_marker(g, state.wheel_components()[0]), hue),
        (state.wheel_marker(g), [color[0], color[1], color[2]]),
    ].into_iter().enumerate() {
        let bounds = gtk::graphene::Rect::new(point[0]-radius, point[1]-radius, radius*2.,radius*2.);
        snapshot.push_rounded_clip(&gtk::gsk::RoundedRect::from_rect(bounds, radius));
        if index == 1 && matches!(view, ViewColor::Mapped { .. }) {
            let mut p = state.definition().linear_in(state.rgb_space()).unwrap(); p[3] = 1.;
            crate::display_color::append_picker_solid(snapshot, view, headroom, state.rgb_space(), p, &bounds);
        } else { crate::display_color::append_solid(snapshot, view, [rgb[0],rgb[1],rgb[2],1.], &bounds); }
        snapshot.pop();
        crate::display_color::marker_outline(snapshot, point, radius);
    }
}
