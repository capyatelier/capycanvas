//! Native projection of the shared canvas action bar: a glass panel beside the
//! selection, transform box or placed image. Contents, placement and edit
//! validation come from shared Rust; this widget only measures and presents.
use crate::workspace::{
    Workspace,
    toolbar_components::{Field, action_button, segment_buttons},
};
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
    fields: RefCell<Vec<(gtk::Widget, Option<Field>)>>,
    updating: Cell<bool>,
    layout: Cell<Option<CanvasBarLayout>>,
    suppressed: Cell<bool>,
    reappear: RefCell<Option<glib::SourceId>>,
}

fn same_schema(a: &CanvasBarView, b: &CanvasBarView) -> bool {
    let same = |x: &[CanvasBarItem], y: &[CanvasBarItem]| {
        x.len() == y.len() && x.iter().zip(y).all(|(x, y)| x.label == y.label && x.option.same_schema(&y.option))
    };
    a.context == b.context && a.label == b.label && same(&a.items, &b.items) && same(&a.completion, &b.completion)
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
            updating: Cell::new(false),
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
        self.updating.set(true);
        if rebuild {
            self.rebuild(workspace, view);
        }
        if let Some(view) = view {
            for ((_, field), item) in self.fields.borrow().iter().zip(view.items.iter().chain(&view.completion)) {
                if let Some(field) = field {
                    field.update(&item.option);
                }
            }
        }
        self.updating.set(false);
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
                container.append(&field.0);
                self.fields.borrow_mut().push(field);
            }
        }
    }

    pub fn place(&self, workspace: &Rc<Workspace>) {
        let Some(view) = self.view.borrow().clone() else {
            self.layout.set(None);
            self.root.set_visible(false);
            workspace.queue_surface_allocate();
            return;
        };
        let width = |w: &gtk::Widget| w.measure(gtk::Orientation::Horizontal, -1).1 as f32;
        let fields = self.fields.borrow();
        for (field, _) in fields.iter() {
            field.set_visible(true);
        }
        let (items, completion) = fields.split_at(view.items.len());
        let height = fields
            .iter()
            .map(|(w, _)| w)
            .chain([self.more.upcast_ref::<gtk::Widget>()])
            .map(|w| w.measure(gtk::Orientation::Vertical, -1).1)
            .max()
            .unwrap_or(0) as f32;
        let measure = CanvasBarMeasure {
            context: view.context,
            label: if view.label.is_some() { width(self.label.upcast_ref()) } else { 0. },
            items: items.iter().map(|(w, _)| width(w)).collect(),
            completion: completion.iter().map(|(w, _)| width(w)).collect(),
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
            for (index, (field, _)) in items.iter().enumerate() {
                field.set_visible(index < layout.items);
            }
        }
        self.layout.set(layout);
        self.root.set_visible(layout.is_some());
        self.present();
        workspace.place_notice();
        workspace.queue_surface_allocate();
    }

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
) -> (gtk::Widget, Option<Field>) {
    let send = glib::clone!(
        #[weak]
        workspace,
        move |action: UiAction| {
            if !workspace.canvas_bar.updating.get() {
                workspace.dispatch(UiAction::CanvasBarEdit { context, action: Box::new(action) });
            }
        }
    );
    let unfocused = |widget: &gtk::Widget| {
        widget.set_focus_on_click(false);
        widget.set_can_focus(false);
    };
    match &item.option {
        ToolOption::Action { state, checkable } => {
            let button = action_button(state, *checkable, Some(item.label), send);
            button.set_widget_name(&format!("canvas-bar-{:?}", state.id));
            unfocused(button.upcast_ref());
            let accent = completion
                && matches!(state.id, layer_ui::CommandId::ApplyTransform | layer_ui::CommandId::CompleteSelection);
            button.add_css_class(if accent { "suggested-action" } else { "flat" });
            (button.clone().upcast(), Some(Field::Action(button)))
        }
        ToolOption::Choice { id, label, segmented: true, items } => {
            let segments = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            segments.set_widget_name(&format!("canvas-bar-choice-{id}"));
            let buttons = segment_buttons(&segments, label, items, true, send);
            buttons.iter().for_each(|button| unfocused(button.upcast_ref()));
            (segments.upcast(), Some(Field::Segments(buttons)))
        }
        ToolOption::Choice { id, label, .. } => {
            let button = gtk::MenuButton::new();
            button.set_widget_name(&format!("canvas-bar-choice-{id}"));
            button.update_property(&[gtk::accessible::Property::Label(label)]);
            button.set_tooltip_text(Some(label));
            button.set_always_show_arrow(true);
            button.add_css_class("flat");
            unfocused(button.upcast_ref());
            let (image, text) = (gtk::Image::new(), gtk::Label::new(None));
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            content.append(&image);
            content.append(&text);
            button.set_child(Some(&content));
            let popover = gtk::PopoverMenu::from_model(None::<&gtk::gio::MenuModel>);
            button.set_popover(Some(&popover));
            workspace.watch_popover(popover.upcast_ref());
            let choice = *id;
            popover.connect_show(glib::clone!(
                #[weak]
                workspace,
                move |popover| workspace.populate_canvas_bar_choice(popover, context, choice)
            ));
            (button.upcast(), Some(Field::Menu(image, text)))
        }
        ToolOption::Numeric(_) | ToolOption::Range { .. } => (gtk::Box::new(gtk::Orientation::Horizontal, 0).upcast(), None),
    }
}
