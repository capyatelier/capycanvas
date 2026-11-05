use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gdk, glib};
use layer_core::EffectValue;
use layer_ui::{EffectAction, PaperPickerView, UiAction, UiState};
use std::{cell::{Cell, RefCell}, rc::{Rc, Weak}, sync::Arc};

const TILE: i32 = 96;
const THUMBNAIL: i32 = 28;

fn texture(preview: &[u8]) -> gdk::Texture {
    let size = layer_core::paper::PREVIEW_SIZE;
    gdk::MemoryTexture::new(size as i32, size as i32, gdk::MemoryFormat::G8, &glib::Bytes::from(preview), size).upcast()
}

#[derive(Clone)]
pub(crate) struct PaperThumbnail {
    pub image: gtk::Image,
    size: i32,
    settled: Rc<RefCell<Option<Option<Arc<str>>>>>,
}
impl PaperThumbnail {
    pub fn new(size: i32) -> Self {
        let image = gtk::Image::new();
        image.set_size_request(size, size);
        image.set_overflow(gtk::Overflow::Hidden);
        image.set_can_target(false);
        image.add_css_class("paper-preview");
        Self { image, size, settled: Rc::default() }
    }
    pub fn show(&self, workspace: &Workspace, id: Option<&Arc<str>>) {
        if self.settled.borrow().as_ref().is_some_and(|settled| settled.as_ref() == id) { return; }
        let preview = id.and_then(|id| workspace.gpu.try_borrow().ok()?.as_ref()?.session.paper_preview(id));
        match &preview {
            Some(preview) => {
                self.image.set_paintable(Some(&texture(preview)));
                self.image.set_pixel_size(self.size);
            }
            None => {
                crate::icons::set(&self.image, Some("layer-paper-symbolic"));
                self.image.set_pixel_size(self.size / 2);
            }
        }
        *self.settled.borrow_mut() = (preview.is_some() || id.is_none()).then(|| id.cloned());
    }
    pub fn show_icon(&self, icon: &str) {
        crate::icons::set(&self.image, Some(icon));
        self.image.set_pixel_size(self.size / 2);
        *self.settled.borrow_mut() = None;
    }
}

fn picker(state: &UiState, layer: u64) -> Option<(PaperPickerView, usize)> {
    if state.layer_properties.layer != Some(layer) { return None; }
    state.layer_properties.controls.iter().find_map(|control| match (&control.paper, &control.value) {
        (Some(view), EffectValue::Choice(selected)) => Some((view.clone(), *selected as usize)),
        _ => None,
    })
}

fn act(workspace: &Rc<Workspace>, action: EffectAction) {
    match action {
        EffectAction::RenamePaper { id, name } => {
            glib::MainContext::default().spawn_local(glib::clone!(#[strong] workspace, async move { crate::files::paper::rename(&workspace, id, name).await }));
        }
        action => workspace.dispatch(UiAction::Effect { action }),
    }
}

pub struct PaperPicker {
    pub button: gtk::Button,
    thumbnail: PaperThumbnail,
    name: gtk::Label,
}
impl PaperPicker {
    pub fn new(workspace: &Rc<Workspace>, layer: u64) -> Rc<Self> {
        let thumbnail = PaperThumbnail::new(THUMBNAIL);
        let name = gtk::Label::builder().xalign(0.).hexpand(true).ellipsize(gtk::pango::EllipsizeMode::End).build();
        let content = gtk::Box::new(gtk::Orientation::Horizontal, crate::panel_controls::SPACING);
        content.append(&thumbnail.image);
        content.append(&name);
        let button = gtk::Button::builder().child(&content).hexpand(true).build();
        button.set_widget_name("property-texture");
        button.connect_clicked(glib::clone!(#[weak] workspace, move |_| open(&workspace, layer)));
        Rc::new(Self { button, thumbnail, name })
    }

    pub fn update(&self, workspace: &Workspace, view: &PaperPickerView, selected: u32, label: &str) {
        let tile = view.tiles.get(selected as usize);
        self.thumbnail.show(workspace, tile.and_then(|tile| tile.id.as_ref()));
        self.name.set_label(tile.map_or("", |tile| tile.label.as_ref()));
        self.button.set_tooltip_text(Some(label));
        self.button.update_property(&[gtk::accessible::Property::Label(label),
            gtk::accessible::Property::Description(tile.map_or("", |tile| tile.label.as_ref()))]);
    }
}

pub(crate) struct TextureDialog {
    this: Weak<Self>,
    workspace: Weak<Workspace>,
    layer: u64,
    epoch: u64,
    dialog: adw::Dialog,
    title: gtk::Label,
    viewport: gtk::Viewport,
    grid: gtk::FlowBox,
    menu: gtk::Popover,
    import: gtk::Button,
    cancel: gtk::Button,
    apply: gtk::Button,
    view: RefCell<Option<PaperPickerView>>,
    pending: RefCell<Option<Arc<str>>>,
    updating: Cell<bool>,
}
impl Drop for TextureDialog {
    fn drop(&mut self) {
        if self.menu.parent().is_some() { self.menu.unparent(); }
    }
}

pub(crate) fn open(workspace: &Rc<Workspace>, layer: u64) {
    let Some(state) = workspace.gpu.borrow().as_ref().map(|g| g.session.state().clone()) else { return };
    let Some((view, selected)) = picker(&state, layer) else { return };
    let dialog = TextureDialog::build(workspace, layer, state.document_file.epoch);
    dialog.update(&view, selected);
    *workspace.texture_dialog.borrow_mut() = Some(dialog.clone());
    crate::page_dialog::present_intent(&dialog.dialog, &workspace.window,view.dialog);
    let viewport = dialog.viewport.clone();
    dialog.grid.add_tick_callback(move |grid, _| {
        let Some(child) = grid.selected_children().into_iter().next() else { return glib::ControlFlow::Break };
        if child.compute_bounds(grid).is_none_or(|bounds| bounds.height() <= 0.) { return glib::ControlFlow::Continue }
        child.grab_focus();
        viewport.scroll_to(&child, None);
        glib::ControlFlow::Break
    });
}

pub(crate) fn refresh(workspace: &Workspace, state: &UiState) {
    let Some(dialog) = workspace.texture_dialog.borrow().clone() else { return };
    if let Some((view, selected)) = picker(state, dialog.layer).filter(|_| state.document_file.epoch == dialog.epoch) {
        dialog.update(&view, selected);
    }
}

pub(crate) fn texture_grid() -> gtk::FlowBox {
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .min_children_per_line(2)
        .max_children_per_line(6)
        .row_spacing(14)
        .column_spacing(12)
        .valign(gtk::Align::Start)
        .build();
    grid.add_css_class("texture-grid");
    grid
}

pub(crate) fn texture_tile(thumbnail: &PaperThumbnail, workspace: &Workspace, id: Option<&Arc<str>>, label: &str) -> gtk::FlowBoxChild {
    thumbnail.image.set_widget_name("texture-preview");
    thumbnail.show(workspace, id);
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("texture-tile");
    frame.set_halign(gtk::Align::Center);
    frame.append(&thumbnail.image);
    let caption = gtk::Label::builder().label(label).wrap(true).lines(2).justify(gtk::Justification::Center)
        .ellipsize(gtk::pango::EllipsizeMode::End).width_chars(1).max_width_chars(1).build();
    caption.add_css_class("caption");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.set_size_request(TILE + 6, -1);
    content.set_valign(gtk::Align::Start);
    content.append(&frame);
    content.append(&caption);
    let child = gtk::FlowBoxChild::builder().child(&content).tooltip_text(label).build();
    child.update_property(&[gtk::accessible::Property::Label(label)]);
    child
}

impl TextureDialog {
    fn build(workspace: &Rc<Workspace>, layer: u64, epoch: u64) -> Rc<Self> {
        let dialog = adw::Dialog::builder().content_width(720).content_height(560).build();
        dialog.set_widget_name("texture-dialog");
        dialog.add_css_class("picker-dialog");
        let title = gtk::Label::new(None);
        title.add_css_class("title-4");
        title.set_halign(gtk::Align::Center);
        let grid = texture_grid();
        grid.set_widget_name("texture-grid");
        let page = gtk::Box::new(gtk::Orientation::Vertical, 16);
        page.add_css_class("dialog-page");
        page.append(&title);
        page.append(&grid);
        let viewport = gtk::Viewport::builder().vscroll_policy(gtk::ScrollablePolicy::Natural).child(&crate::squircle::Squircles::new(&page)).build();
        let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&viewport)
            .build());
        let import = gtk::Button::new();
        import.set_widget_name("texture-import");
        let cancel = gtk::Button::new();
        cancel.set_widget_name("texture-cancel");
        let apply = gtk::Button::new();
        apply.add_css_class("suggested-action");
        apply.set_widget_name("texture-apply");
        let gap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        gap.set_hexpand(true);
        let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bottom.add_css_class("dialog-footer");
        for widget in [import.upcast_ref::<gtk::Widget>(), gap.upcast_ref(), cancel.upcast_ref(), apply.upcast_ref()] { bottom.append(widget); }
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("dialog-content");
        content.append(&scroll);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&crate::squircle::Squircles::new(&bottom));
        dialog.set_child(Some(&content));
        dialog.set_default_widget(Some(&apply));
        crate::page_dialog::configure(&dialog, crate::page_dialog::Metadata::new().title(&title, "label").primary(&apply).cancel(&cancel));
        let menu = gtk::Popover::new();
        crate::menu_presentation::configure(&menu,layer_ui::MenuTarget::PaperTile,None);
        menu.set_widget_name("paper-tile-menu");
        let texture = Rc::new_cyclic(|this| Self {
            this: this.clone(), workspace: Rc::downgrade(workspace), layer, epoch, dialog, title, viewport, grid, menu, import, cancel, apply,
            view: RefCell::default(), pending: RefCell::default(), updating: Cell::new(false),
        });
        texture.bind(workspace);
        texture
    }

    fn bind(&self, workspace: &Rc<Workspace>) {
        let weak = self.this.clone();
        self.grid.connect_selected_children_changed(glib::clone!(#[strong] weak, move |grid| {
            let Some(dialog) = weak.upgrade().filter(|dialog| !dialog.updating.get()) else { return };
            let Some(index) = grid.selected_children().first().and_then(|child| usize::try_from(child.index()).ok()) else { return };
            let id = dialog.view.borrow().as_ref().and_then(|view| view.tiles.get(index)).and_then(|tile| tile.id.clone());
            *dialog.pending.borrow_mut() = id;
        }));
        self.import.connect_clicked(glib::clone!(#[strong] weak, move |_| {
            let Some(dialog) = weak.upgrade() else { return };
            let action = dialog.view.borrow().as_ref().and_then(|view| view.import.clone());
            crate::page_dialog::close_dialog(&dialog.dialog);
            if let (Some(action), Some(workspace)) = (action, dialog.workspace.upgrade()) { act(&workspace, action.action); }
        }));
        self.cancel.connect_clicked(glib::clone!(#[strong] weak, move |_| if let Some(dialog) = weak.upgrade() { crate::page_dialog::close_dialog(&dialog.dialog); }));
        self.apply.connect_clicked(glib::clone!(#[strong] weak, move |_| {
            let Some(dialog) = weak.upgrade() else { return };
            dialog.finish();
            crate::page_dialog::close_dialog(&dialog.dialog);
        }));
        self.dialog.connect_closed(glib::clone!(#[strong] weak, move |_| {
            let Some(dialog) = weak.upgrade() else { return };
            dialog.menu.popdown();
            if let Some(workspace) = dialog.workspace.upgrade() {
                workspace.texture_dialog.borrow_mut().take_if(|open| Rc::ptr_eq(open, &dialog));
            }
        }));
        workspace.on_localization(glib::clone!(#[strong] weak, move |localization| {
            let Some(dialog) = weak.upgrade() else { return false };
            dialog.relabel(localization);
            true
        }));
        self.relabel(&workspace.localization());
    }

    fn relabel(&self, localization: &layer_ui::Localizer) {
        let copy = layer_ui::NativeCopy::new(localization).paper;
        self.dialog.set_title(&copy.choose);
        self.title.set_text(&copy.choose);
        self.cancel.set_label(&layer_ui::CommonCopy::new(localization).cancel);
        self.apply.set_label(&copy.use_texture);
    }

    fn finish(&self) {
        let Some(workspace) = self.workspace.upgrade() else { return };
        let state = workspace.gpu.borrow().as_ref().map(|g| g.session.state().clone());
        let current = state.as_ref().filter(|state| state.document_file.epoch == self.epoch).and_then(|state| picker(state, self.layer));
        let Some((view, selected)) = current else {
            if let Some(gpu) = workspace.gpu.borrow_mut().as_mut() { gpu.session.raise_message_notice(layer_ui::MessageId::COMMON_ACTION_FAILED); }
            workspace.changed(Ok(layer_ui::UiChange { regions: layer_ui::regions::HOST, ..Default::default() }));
            return;
        };
        let pending = self.pending.borrow().clone();
        let Some(index) = view.tiles.iter().position(|tile| tile.id == pending).filter(|index| *index != selected) else { return };
        workspace.dispatch(UiAction::Effect { action: EffectAction::Set { layer: self.layer, key: "texture".into(), value: EffectValue::Choice(index as u32) } });
    }

    fn update(&self, view: &PaperPickerView, selected: usize) {
        let Some(workspace) = self.workspace.upgrade() else { return };
        if self.view.borrow().as_ref() != Some(view) {
            self.menu.popdown();
            if self.menu.parent().is_some() { self.menu.unparent(); }
            self.updating.set(true);
            self.grid.remove_all();
            self.updating.set(false);
            for (index, tile) in view.tiles.iter().enumerate() { self.append_tile(&workspace, index, tile); }
            if let Some(import) = &view.import { self.import.set_label(&import.label); }
            self.import.set_visible(view.import.is_some());
            let pending = self.pending.borrow().clone();
            if self.view.borrow().is_none() || !view.tiles.iter().any(|tile| tile.id == pending) {
                *self.pending.borrow_mut() = view.tiles.get(selected).and_then(|tile| tile.id.clone());
            }
            *self.view.borrow_mut() = Some(view.clone());
        }
        let pending = self.pending.borrow().clone();
        self.updating.set(true);
        match view.tiles.iter().position(|tile| tile.id == pending).and_then(|index| self.grid.child_at_index(index as i32)) {
            Some(child) => self.grid.select_child(&child),
            None => self.grid.unselect_all(),
        }
        self.updating.set(false);
    }

    fn append_tile(&self, workspace: &Workspace, index: usize, tile: &layer_ui::PaperTileView) {
        let child = texture_tile(&PaperThumbnail::new(TILE), workspace, tile.id.as_ref(), &tile.label);
        child.set_widget_name(&format!("texture-tile-{index}"));
        if !tile.actions.is_empty() {
            let click = gtk::GestureClick::builder().button(3).build();
            click.connect_pressed(glib::clone!(#[strong(rename_to = weak)] self.this, move |_, _, _, _| if let Some(dialog) = weak.upgrade() { dialog.open_menu(index); }));
            child.add_controller(click);
            let hold = gtk::GestureLongPress::new();
            hold.connect_pressed(glib::clone!(#[strong(rename_to = weak)] self.this, move |_, _, _| if let Some(dialog) = weak.upgrade() { dialog.open_menu(index); }));
            child.add_controller(hold);
        }
        self.grid.append(&child);
    }

    fn open_menu(&self, index: usize) {
        let Some(actions) = self.view.borrow().as_ref().and_then(|view| view.tiles.get(index)).map(|tile| tile.actions.clone()) else { return };
        let Some(child) = self.grid.child_at_index(index as i32).filter(|_| !actions.is_empty()) else { return };
        let items = gtk::Box::new(gtk::Orientation::Vertical, 0);
        for action in actions {
            let button = gtk::Button::with_label(&action.label);
            button.add_css_class("flat");
            button.connect_clicked(glib::clone!(#[strong(rename_to = weak)] self.this, move |_| {
                let Some(dialog) = weak.upgrade() else { return };
                dialog.menu.popdown();
                if let Some(workspace) = dialog.workspace.upgrade() { act(&workspace, action.action.clone()); }
            }));
            items.append(&button);
        }
        self.menu.set_child(Some(&items));
        if self.menu.parent().as_ref() != Some(child.upcast_ref()) {
            if self.menu.parent().is_some() { self.menu.unparent(); }
            self.menu.set_parent(&child);
        }
        self.menu.popup();
    }
}
