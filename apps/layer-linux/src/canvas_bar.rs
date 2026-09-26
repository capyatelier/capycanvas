//! Native projection of the shared canvas action bar: a glass panel beside the
//! selection, transform box or placed image. Contents, placement and edit
//! validation come from shared Rust; this widget only measures and presents.
use crate::workspace::Workspace;
use gtk::{glib, prelude::*};
use layer_ui::{
    Bounds, CANVAS_BAR_REAPPEAR_MS, CanvasBarItem, CanvasBarLayout, CanvasBarMeasure,
    CanvasBarView, ToolOption, UiAction,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const GAP: i32 = 4;
const PADDING: i32 = 6;

pub struct CanvasBar {
    pub root: gtk::Box,
    label: gtk::Label,
    items: gtk::Box,
    more: gtk::MenuButton,
    completion: gtk::Box,
    menu: gtk::PopoverMenu,
    view: RefCell<Option<CanvasBarView>>,
    fields: RefCell<Vec<gtk::Widget>>,
    layout: Cell<Option<CanvasBarLayout>>,
    suppressed: Cell<bool>,
    reappear: RefCell<Option<glib::SourceId>>,
}

fn same_schema(a: &CanvasBarView, b: &CanvasBarView) -> bool {
    let schema = |items: &[CanvasBarItem]| {
        items
            .iter()
            .map(|item| (item.label, std::mem::discriminant(&item.option)))
            .collect::<Vec<_>>()
    };
    a.context == b.context
        && a.label == b.label
        && schema(&a.items) == schema(&b.items)
        && schema(&a.completion) == schema(&b.completion)
        && a.items.iter().chain(&a.completion).zip(b.items.iter().chain(&b.completion)).all(|(x, y)| x.option.same_schema(&y.option))
}

impl CanvasBar {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, GAP);
        root.set_widget_name("canvas-action-bar");
        root.add_css_class("dock-panel");
        root.add_css_class("canvas-action-bar");
        root.set_visible(false);
        root.update_property(&[gtk::accessible::Property::Label("Canvas actions")]);
        let label = gtk::Label::new(None);
        label.add_css_class("canvas-action-bar-label");
        label.set_visible(false);
        let items = gtk::Box::new(gtk::Orientation::Horizontal, GAP);
        let more = gtk::MenuButton::new();
        more.set_child(Some(&crate::icons::image("layer-more-symbolic")));
        more.set_widget_name("canvas-bar-more");
        more.set_tooltip_text(Some("More"));
        more.update_property(&[gtk::accessible::Property::Label("More")]);
        more.add_css_class("flat");
        more.set_focus_on_click(false);
        more.set_can_focus(false);
        let completion = gtk::Box::new(gtk::Orientation::Horizontal, GAP);
        root.append(&label);
        root.append(&items);
        root.append(&more);
        root.append(&completion);
        let menu = gtk::PopoverMenu::from_model(None::<&gtk::gio::MenuModel>);
        more.set_popover(Some(&menu));
        Self {
            root,
            label,
            items,
            more,
            completion,
            menu,
            view: RefCell::new(None),
            fields: RefCell::new(Vec::new()),
            layout: Cell::new(None),
            suppressed: Cell::new(false),
            reappear: RefCell::new(None),
        }
    }

    pub fn bind(&self, workspace: &Rc<Workspace>) {
        workspace.watch_popover(self.menu.upcast_ref());
        self.menu.connect_show(glib::clone!(
            #[weak]
            workspace,
            move |_| workspace.populate_canvas_bar_menu()
        ));
    }

    pub fn fill_menu(&self, workspace: &Rc<Workspace>, menu: layer_ui::ContextMenu) {
        workspace.populate_workspace_menu(&self.menu, menu);
    }

    #[cfg(test)]
    pub fn menu_open(&self) -> bool {
        self.menu.is_visible()
    }

    pub fn context_menu_request(&self) -> Option<(layer_ui::CanvasBarContext, usize)> {
        let view = self.view.borrow();
        let view = view.as_ref()?;
        Some((view.context, self.layout.get().map_or(0, |l| l.items)))
    }

    /// Allocated bounds in workspace coordinates, kept while suppressed so
    /// hiding and showing never reallocates the workspace.
    pub fn bounds(&self) -> Option<Bounds> {
        self.view
            .borrow()
            .is_some()
            .then(|| self.layout.get().map(|l| l.bounds))
            .flatten()
    }

    /// Bounds while the bar can be seen and touched.
    pub fn visible_bounds(&self) -> Option<Bounds> {
        self.bounds().filter(|_| !self.suppressed.get())
    }

    fn present(&self) {
        let shown = !self.suppressed.get();
        self.root.set_opacity(if shown { 1. } else { 0. });
        self.root.set_can_target(shown);
    }

    pub fn refresh(&self, workspace: &Rc<Workspace>, view: Option<&CanvasBarView>) {
        let rebuild = match (self.view.borrow().as_ref(), view) {
            (Some(old), Some(new)) => !same_schema(old, new),
            (None, None) => false,
            _ => true,
        };
        if rebuild {
            self.rebuild(workspace, view);
        } else if let Some(view) = view {
            for (field, item) in self.fields.borrow().iter().zip(view.items.iter().chain(&view.completion)) {
                update(field, &item.option);
            }
        }
        *self.view.borrow_mut() = view.cloned();
        self.place(workspace);
    }

    fn rebuild(&self, workspace: &Rc<Workspace>, view: Option<&CanvasBarView>) {
        self.menu.popdown();
        for container in [&self.items, &self.completion] {
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }
        }
        self.fields.borrow_mut().clear();
        let Some(view) = view else {
            return;
        };
        self.label.set_label(view.label.as_deref().unwrap_or_default());
        self.label.set_visible(view.label.is_some());
        for (container, items, completion) in [
            (&self.items, &view.items, false),
            (&self.completion, &view.completion, true),
        ] {
            for item in items {
                let field = build(workspace, view.context, item, completion);
                container.append(&field);
                self.fields.borrow_mut().push(field);
            }
        }
    }

    /// Measure the controls and ask the session where the bar belongs.
    pub fn place(&self, workspace: &Rc<Workspace>) {
        let Some(view) = self.view.borrow().clone() else {
            self.layout.set(None);
            self.root.set_visible(false);
            workspace.queue_surface_allocate();
            return;
        };
        let width = |w: &gtk::Widget| w.measure(gtk::Orientation::Horizontal, -1).1 as f32;
        let fields = self.fields.borrow();
        for field in fields.iter() {
            field.set_visible(true);
        }
        let (items, completion) = fields.split_at(view.items.len());
        let height = fields
            .iter()
            .chain([self.more.upcast_ref::<gtk::Widget>()])
            .map(|w| w.measure(gtk::Orientation::Vertical, -1).1)
            .max()
            .unwrap_or(0) as f32;
        let measure = CanvasBarMeasure {
            context: view.context,
            label: if view.label.is_some() { width(self.label.upcast_ref()) } else { 0. },
            items: items.iter().map(width).collect(),
            completion: completion.iter().map(width).collect(),
            more: width(self.more.upcast_ref()),
            height: height + 2. * PADDING as f32,
            gap: GAP as f32,
            padding: PADDING as f32,
        };
        let layout = workspace
            .gpu
            .borrow()
            .as_ref()
            .and_then(|g| g.session.canvas_bar_layout(&measure));
        if let Some(layout) = layout {
            for (index, field) in items.iter().enumerate() {
                field.set_visible(index < layout.items);
            }
        }
        self.layout.set(layout);
        self.root.set_visible(layout.is_some());
        self.present();
        workspace.queue_surface_allocate();
    }

    /// Hide during canvas contacts and navigation; return once input settles.
    pub fn suppress(&self, workspace: &Rc<Workspace>, hidden: bool) {
        if let Some(source) = self.reappear.borrow_mut().take() {
            source.remove();
        }
        if hidden {
            if !self.suppressed.replace(true) {
                self.menu.popdown();
                self.present();
            }
            return;
        }
        if !self.suppressed.get() {
            return;
        }
        let source = glib::timeout_add_local_once(
            std::time::Duration::from_millis(CANVAS_BAR_REAPPEAR_MS.into()),
            glib::clone!(
                #[weak]
                workspace,
                move || {
                    let bar = &workspace.canvas_bar;
                    bar.reappear.borrow_mut().take();
                    bar.suppressed.set(false);
                    bar.place(&workspace);
                    bar.present();
                }
            ),
        );
        *self.reappear.borrow_mut() = Some(source);
    }

    /// The camera moved: hide now and reappear at the new place once it settles.
    pub fn defer(&self, workspace: &Rc<Workspace>) {
        if self
            .view
            .borrow()
            .as_ref()
            .is_some_and(|v| v.placement == layer_ui::CanvasBarPlacement::NearObject)
        {
            self.suppress(workspace, true);
            self.suppress(workspace, false);
        }
    }
}

fn build(
    workspace: &Rc<Workspace>,
    context: layer_ui::CanvasBarContext,
    item: &CanvasBarItem,
    completion: bool,
) -> gtk::Widget {
    let edit = move |action: UiAction| UiAction::CanvasBarEdit {
        context,
        action: Box::new(action),
    };
    match &item.option {
        ToolOption::Action { state, checkable } => {
            let button: gtk::Button = if *checkable {
                gtk::ToggleButton::new().upcast()
            } else {
                gtk::Button::new()
            };
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            if let Some(icon) = state.icon {
                content.append(&crate::icons::image(&format!("layer-{icon}-symbolic")));
            }
            content.append(&gtk::Label::new(Some(item.label)));
            button.set_child(Some(&content));
            button.update_property(&[gtk::accessible::Property::Label(state.label)]);
            button.set_tooltip_text(Some(&state.tooltip));
            button.set_widget_name(&format!("canvas-bar-{:?}", state.id));
            button.set_focus_on_click(false);
            button.set_can_focus(false);
            if completion
                && matches!(state.id, layer_ui::CommandId::ApplyTransform | layer_ui::CommandId::CompleteSelection)
            {
                button.add_css_class("suggested-action");
            } else {
                button.add_css_class("flat");
            }
            let action = edit(UiAction::Invoke { command: state.id });
            button.connect_clicked(glib::clone!(
                #[weak]
                workspace,
                move |button| {
                    if button.is_sensitive() {
                        workspace.dispatch(action.clone());
                    }
                }
            ));
            update(button.upcast_ref(), &item.option);
            button.upcast()
        }
        ToolOption::Choice { id, label, items, .. } => {
            let segments = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            segments.add_css_class("linked");
            segments.set_widget_name(&format!("canvas-bar-choice-{id}"));
            segments.update_property(&[gtk::accessible::Property::Label(label)]);
            for choice in items {
                let button = gtk::ToggleButton::new();
                let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                content.append(&crate::icons::image(&format!("layer-{}-symbolic", choice.icon)));
                content.append(&gtk::Label::new(Some(choice.label)));
                button.set_child(Some(&content));
                button.set_tooltip_text(Some(choice.label));
                button.set_focus_on_click(false);
                button.set_can_focus(false);
                button.set_active(choice.selected);
                let action = edit(choice.action.clone());
                button.connect_clicked(glib::clone!(
                    #[weak]
                    workspace,
                    move |_| workspace.dispatch(action.clone())
                ));
                segments.append(&button);
            }
            segments.upcast()
        }
        ToolOption::Numeric(_) | ToolOption::Range { .. } => gtk::Box::new(gtk::Orientation::Horizontal, 0).upcast(),
    }
}

fn update(field: &gtk::Widget, option: &ToolOption) {
    match option {
        ToolOption::Action { state, .. } => {
            field.set_sensitive(state.enabled);
            if let Some(toggle) = field.downcast_ref::<gtk::ToggleButton>()
                && toggle.is_active() != state.selected
            {
                toggle.set_active(state.selected);
            }
        }
        ToolOption::Choice { items, .. } => {
            let mut child = field.first_child();
            for item in items {
                let Some(button) = child else {
                    break;
                };
                if let Some(toggle) = button.downcast_ref::<gtk::ToggleButton>()
                    && toggle.is_active() != item.selected
                {
                    toggle.set_active(item.selected);
                }
                child = button.next_sibling();
            }
        }
        ToolOption::Numeric(_) | ToolOption::Range { .. } => {}
    }
}
